mod common;

use common::TestEnv;
use std::fs::{self, File};
use std::io::Write;
use woderrise_core::error::{TransactionError, WoderriseError};
use woderrise_core::models::{ConfigComponent, OperationType, TransactionStatus};
use woderrise_transaction::{
    BackupSnapshotEngine, JournalManager, StagingWorkspace, StartupRecovery,
};

#[test]
fn test_wal_state_transition_durability() {
    let env = TestEnv::new();
    let journal_mgr = JournalManager::new(&env.xdg).expect("Journal manager");
    let tx_id = "tx_wal_001";

    let backup_dir = env.xdg.backup_dir(tx_id).expect("Backup dir");

    // 1. Begin applying in WAL
    journal_mgr
        .begin_applying(
            tx_id,
            ConfigComponent::Hyprland,
            "user/repo",
            "commit123",
            &backup_dir,
            Vec::new(),
        )
        .expect("Begin applying");

    let journal = journal_mgr.load_journal().expect("Load journal");
    let active = journal
        .active_transaction
        .expect("Active transaction present");
    assert_eq!(active.id, tx_id);
    assert_eq!(active.status, TransactionStatus::Applying);

    // 2. Commit transaction
    let entry = journal_mgr
        .commit_transaction(tx_id)
        .expect("Commit transaction");
    assert_eq!(entry.id, tx_id);
    assert_eq!(entry.status, TransactionStatus::Committed);

    let journal_after = journal_mgr
        .load_journal()
        .expect("Load journal after commit");
    assert!(journal_after.active_transaction.is_none());
    assert_eq!(journal_after.transactions.len(), 1);
    assert_eq!(journal_after.transactions[0].id, tx_id);
    assert_eq!(
        journal_after.transactions[0].status,
        TransactionStatus::Committed
    );
}

#[test]
fn test_wal_rejects_concurrent_active_transaction() {
    let env = TestEnv::new();
    let journal_mgr = JournalManager::new(&env.xdg).expect("Journal manager");

    let backup_dir = env.xdg.backup_dir("tx_1").expect("Backup dir 1");
    journal_mgr
        .begin_applying(
            "tx_1",
            ConfigComponent::Hyprland,
            "user/repo",
            "commit1",
            &backup_dir,
            Vec::new(),
        )
        .expect("Begin tx_1");

    let err = journal_mgr
        .begin_applying(
            "tx_2",
            ConfigComponent::Waybar,
            "user/repo",
            "commit2",
            &backup_dir,
            Vec::new(),
        )
        .expect_err("Must reject concurrent active transaction");

    match err {
        WoderriseError::Transaction(TransactionError::TransactionInProgress { active_tx_id }) => {
            assert_eq!(active_tx_id, "tx_1");
        }
        other => panic!("Expected TransactionInProgress, got: {:?}", other),
    }
}

#[test]
fn test_interrupted_phase4_simulated_crash_recovery() {
    let env = TestEnv::new();
    let journal_mgr = JournalManager::new(&env.xdg).expect("Journal manager");
    let tx_id = "tx_crash_001";

    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create hypr dir");

    // 1. Initial desktop state:
    // Existing hyprland.conf with original content
    let hyprland_conf = hypr_dir.join("hyprland.conf");
    let original_content = b"# Genuine Original Hyprland Config\n";
    fs::write(&hyprland_conf, original_content).expect("Write original hyprland.conf");

    // Destination extra.conf does NOT exist initially
    let extra_conf = hypr_dir.join("extra.conf");
    assert!(!extra_conf.exists());

    // 2. Stage files and create backup snapshot
    let staging = StagingWorkspace::new(&env.xdg, tx_id).expect("Staging workspace");
    let staged_hypr = staging
        .stage_file(
            camino::Utf8Path::new("hyprland.conf"),
            b"# New Hyprland Version",
            Some(hyprland_conf.clone()),
            "100644",
            None,
        )
        .expect("Stage hyprland");
    let staged_extra = staging
        .stage_file(
            camino::Utf8Path::new("extra.conf"),
            b"# Brand New Extra Config",
            Some(extra_conf.clone()),
            "100644",
            None,
        )
        .expect("Stage extra");

    let backup_engine = BackupSnapshotEngine::new(&env.xdg, tx_id).expect("Backup engine");
    let operations = backup_engine
        .create_snapshot(
            &[staged_hypr, staged_extra],
            ConfigComponent::Hyprland,
            &env.xdg,
        )
        .expect("Create snapshot");

    // Verify snapshot was created for hyprland.conf
    assert_eq!(operations.len(), 2);
    assert_eq!(operations[0].op_type, OperationType::Overwrite);
    assert_eq!(operations[1].op_type, OperationType::Create);

    // 3. Write WAL applying state
    journal_mgr
        .begin_applying(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "deadbeef",
            &backup_engine.backup_dir,
            operations,
        )
        .expect("Begin applying");

    // 4. SIMULATE SUDDEN CRASH MID-PHASE-4:
    // - Overwritten file is left corrupted / partially written
    fs::write(&hyprland_conf, b"# Corrupted half-written crash data!").expect("Corrupt hyprland");
    // - Newly created file was created before crash
    fs::write(&extra_conf, b"# Created right before crash").expect("Write extra");
    // - Stray temporary file left behind by killed process
    let stray_tmp = hypr_dir.join("hyprland.conf.woderrise_tmp.interrupted-uuid");
    let mut stray_file = File::create(&stray_tmp).expect("Create stray file");
    stray_file
        .write_all(b"Abandoned temp data")
        .expect("Write stray");
    drop(stray_file);
    assert!(stray_tmp.exists());

    // 5. Invoke Startup Crash Recovery
    let recovery_outcome = StartupRecovery::scan_and_recover(&journal_mgr, &env.xdg)
        .expect("Run crash recovery")
        .expect("Interrupted transaction must be detected and recovered");

    assert_eq!(recovery_outcome.tx_id, tx_id);
    assert_eq!(
        recovery_outcome.recovered_status,
        TransactionStatus::RecoveredRollback
    );

    // 6. Verify restored state:
    // - Original file content restored verbatim from snapshot
    let restored_hypr = fs::read(&hyprland_conf).expect("Read restored hyprland");
    assert_eq!(restored_hypr, original_content);

    // - Newly created file removed
    assert!(
        !extra_conf.exists(),
        "Newly created file was not removed by crash recovery!"
    );

    // - Stray temporary file deleted
    assert!(
        !stray_tmp.exists(),
        "Stray temporary file was not cleaned up!"
    );

    // 7. Verify journal consistency:
    let journal_after = journal_mgr
        .load_journal()
        .expect("Load journal after recovery");
    assert!(
        journal_after.active_transaction.is_none(),
        "active_transaction must be cleared to None after recovery"
    );
    assert_eq!(journal_after.transactions.len(), 1);
    assert_eq!(
        journal_after.transactions[0].status,
        TransactionStatus::RecoveredRollback
    );
}

#[test]
fn test_startup_recovery_noop_when_clean() {
    let env = TestEnv::new();
    let journal_mgr = JournalManager::new(&env.xdg).expect("Journal manager");

    let outcome = StartupRecovery::scan_and_recover(&journal_mgr, &env.xdg)
        .expect("Recovery check on clean environment");
    assert!(outcome.is_none());
}
