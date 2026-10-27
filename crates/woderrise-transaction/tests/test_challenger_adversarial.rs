//! Comprehensive Adversarial Stress Test Suite for `woderrise-transaction`
//!
//! Evaluates the transaction engine under hostile, abnormal, and edge-case conditions:
//! 1. Simulated crash/panic mid-Phase-4 with multiple files, nested subdirectories, and stray temp files.
//! 2. Simulated crash during in-flight `RollingBack` state.
//! 3. Cross-device EXDEV safety across disparate directory trees with large binary payloads.
//! 4. Dirty-state detection with subtle modifications, non-forced abort, and forced rollback.
//! 5. Multi-tier LIFO order violation enforcement.
//! 6. Deep directory pruning with boundary containment and user file preservation.
//! 7. GNU Stow / Chezmoi symlink preservation during both install and rollback.
//! 8. Security audit hostile inputs: case-insensitive script extensions, executable modes, and Git symlinks.

mod common;

use camino::Utf8Path;
use common::TestEnv;
use std::fs;
use tempfile::TempDir;
use woderrise_core::error::{PathError, SecurityError, TransactionError, WoderriseError};
use woderrise_core::models::{
    ConfigComponent, ConflictStatus, FileOperationRecord, JournalEntry, OperationType,
    TransactionStatus,
};
use woderrise_detect::TreeEntry;
use woderrise_transaction::{
    AtomicSwapper, BackupSnapshotEngine, JournalManager, SecurityAuditor, StagingWorkspace,
    StartupRecovery, TransactionEngine,
};

#[test]
fn test_adversarial_simulated_crash_mid_phase4_multi_file_and_subdirs() {
    let env = TestEnv::new();
    let journal_mgr = JournalManager::new(&env.xdg).expect("Journal manager");
    let tx_id = "tx_crash_adversarial_001";

    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create hypr dir");

    // 1. Establish pre-existing desktop state
    let hyprland_conf = hypr_dir.join("hyprland.conf");
    let original_hypr = b"# Genuine Original hyprland.conf\nexec-once = waybar\n";
    fs::write(&hyprland_conf, original_hypr).expect("Write original hyprland.conf");

    let sub_dir = hypr_dir.join("sub");
    fs::create_dir_all(&sub_dir).expect("Create sub dir");
    let existing_sub = sub_dir.join("colors.conf");
    let original_colors = b"$primary = 0xff112233\n$secondary = 0xff445566\n";
    fs::write(&existing_sub, original_colors).expect("Write original colors.conf");

    // Brand new files to be created
    let new_file_1 = hypr_dir.join("deep").join("nested").join("theme.conf");
    let new_file_2 = hypr_dir.join("assets").join("icon.svg");

    // 2. Stage all incoming files
    let staging = StagingWorkspace::new(&env.xdg, tx_id).expect("Staging workspace");
    let staged_hypr = staging
        .stage_file(
            Utf8Path::new("hyprland.conf"),
            b"# Replaced Hyprland Config V2",
            Some(hyprland_conf.clone()),
            "100644",
            None,
        )
        .expect("Stage hyprland");
    let staged_colors = staging
        .stage_file(
            Utf8Path::new("sub/colors.conf"),
            b"$primary = 0xffaabbcc",
            Some(existing_sub.clone()),
            "100644",
            None,
        )
        .expect("Stage colors");
    let staged_new1 = staging
        .stage_file(
            Utf8Path::new("deep/nested/theme.conf"),
            b"# Deep theme configuration",
            Some(new_file_1.clone()),
            "100644",
            None,
        )
        .expect("Stage new1");
    let staged_new2 = staging
        .stage_file(
            Utf8Path::new("assets/icon.svg"),
            b"<svg>new icon</svg>",
            Some(new_file_2.clone()),
            "100644",
            None,
        )
        .expect("Stage new2");

    let staged_list = vec![staged_hypr, staged_colors, staged_new1, staged_new2];

    // 3. Create pre-installation backup snapshot
    let backup_engine = BackupSnapshotEngine::new(&env.xdg, tx_id).expect("Backup engine");
    let operations = backup_engine
        .create_snapshot(&staged_list, ConfigComponent::Hyprland, &env.xdg)
        .expect("Create snapshot");

    assert_eq!(operations.len(), 4);
    assert_eq!(operations[0].op_type, OperationType::Overwrite);
    assert_eq!(operations[1].op_type, OperationType::Overwrite);
    assert_eq!(operations[2].op_type, OperationType::Create);
    assert_eq!(operations[3].op_type, OperationType::Create);

    // 4. WAL commit to Applying state
    journal_mgr
        .begin_applying(
            tx_id,
            ConfigComponent::Hyprland,
            "adversary/dots",
            "c0ffee123",
            &backup_engine.backup_dir,
            operations,
        )
        .expect("Begin applying");

    // 5. SIMULATE CATASTROPHIC MID-WRITE CRASH:
    // - File 0 was swapped
    fs::write(&hyprland_conf, b"# Replaced Hyprland Config V2").expect("Write swapped hypr");
    // - File 1 was corrupted midway (0-byte truncation or garbage)
    fs::write(&existing_sub, b"# TRUNCATED CRASHED DATA!!!").expect("Corrupt colors");
    // - New file 1 was created with its directory tree
    if let Some(p) = new_file_1.parent() {
        fs::create_dir_all(p).expect("Create parent for new_file_1");
    }
    fs::write(&new_file_1, b"# Created right before power loss!").expect("Write new1");
    // - New file 2 was not reached yet
    assert!(!new_file_2.exists());

    // - Abandoned stray temporary files in multiple directories
    let stray1 = hypr_dir.join("hyprland.conf.woderrise_tmp.aborted-uuid-1");
    fs::write(&stray1, b"partial temp 1").expect("Write stray 1");
    let stray2 = sub_dir.join("colors.conf.woderrise_tmp.aborted-uuid-2");
    fs::write(&stray2, b"partial temp 2").expect("Write stray 2");
    let stray3 = hypr_dir
        .join("deep")
        .join("nested")
        .join(".woderrise_tmp.aborted-uuid-3");
    fs::write(&stray3, b"partial temp 3").expect("Write stray 3");

    // 6. Run Startup Crash Recovery
    let recovery_outcome = StartupRecovery::scan_and_recover(&journal_mgr, &env.xdg)
        .expect("Run scan_and_recover")
        .expect("Must recover interrupted transaction");

    assert_eq!(recovery_outcome.tx_id, tx_id);
    assert_eq!(
        recovery_outcome.recovered_status,
        TransactionStatus::RecoveredRollback
    );

    // 7. Verify all files restored to verbatim pre-install state
    let restored_hypr = fs::read(&hyprland_conf).expect("Read hyprland");
    assert_eq!(restored_hypr, original_hypr);

    let restored_colors = fs::read(&existing_sub).expect("Read colors");
    assert_eq!(restored_colors, original_colors);

    // 8. Verify newly created files deleted and empty directories pruned
    assert!(
        !new_file_1.exists(),
        "New file 1 was not removed during crash recovery!"
    );
    assert!(
        !hypr_dir.join("deep").exists(),
        "Empty deep directory hierarchy was not pruned!"
    );

    // 9. Verify all stray temporary files purged
    assert!(!stray1.exists(), "Stray 1 was not pruned!");
    assert!(!stray2.exists(), "Stray 2 was not pruned!");
    assert!(!stray3.exists(), "Stray 3 was not pruned!");

    // 10. Verify journal consistency
    let journal = journal_mgr.load_journal().expect("Load journal");
    assert!(journal.active_transaction.is_none());
    assert_eq!(journal.transactions.len(), 1);
    assert_eq!(
        journal.transactions[0].status,
        TransactionStatus::RecoveredRollback
    );

    // 11. Idempotency: second recovery call returns None
    let second_run =
        StartupRecovery::scan_and_recover(&journal_mgr, &env.xdg).expect("Second scan_and_recover");
    assert!(second_run.is_none());
}

#[test]
fn test_adversarial_simulated_crash_during_rollback() {
    let env = TestEnv::new();
    let journal_mgr = JournalManager::new(&env.xdg).expect("Journal manager");
    let tx_id = "tx_crash_in_rollback";

    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create hypr dir");

    let target_file = hypr_dir.join("hyprland.conf");
    let original_bytes = b"# Original Stable Config\n";
    fs::write(&target_file, original_bytes).expect("Write original");

    // Create backup snapshot
    let backup_dir = env.xdg.backup_dir(tx_id).expect("Backup dir");
    fs::create_dir_all(&backup_dir).expect("Create backup dir");
    let backup_file = backup_dir.join("hyprland.conf");
    fs::write(&backup_file, original_bytes).expect("Write backup");

    let operations = vec![FileOperationRecord {
        op_type: OperationType::Overwrite,
        target_file: target_file.clone(),
        backup_file: Some(backup_file),
        pre_sha256: Some("original_hash".to_string()),
        post_sha256: Some("installed_hash".to_string()),
        mode: Some("100644".to_string()),
    }];

    // Commit an active transaction directly as if installation succeeded
    let entry = JournalEntry {
        id: tx_id.to_string(),
        timestamp: chrono::Utc::now(),
        component: ConfigComponent::Hyprland,
        status: TransactionStatus::Committed,
        source_repo: "user/dots".to_string(),
        source_commit: "commit_abc".to_string(),
        backup_directory: backup_dir,
        operations,
    };

    // Transition WAL into RollingBack status as if rollback started then crashed
    journal_mgr
        .begin_rolling_back(&entry)
        .expect("Begin rolling back");

    // Target file is corrupted or partially rolled back
    fs::write(&target_file, b"# Corrupt during rollback failure").expect("Write corrupt");

    // Startup recovery should detect the in-flight RollingBack transaction and complete restoration
    let outcome = StartupRecovery::scan_and_recover(&journal_mgr, &env.xdg)
        .expect("Run recovery on crashed rollback")
        .expect("Must recover in-flight RollingBack transaction");

    assert_eq!(outcome.tx_id, tx_id);
    assert_eq!(
        outcome.recovered_status,
        TransactionStatus::RecoveredRollback
    );

    // Verify original content restored
    let restored = fs::read(&target_file).expect("Read target file");
    assert_eq!(restored, original_bytes);

    let journal = journal_mgr.load_journal().expect("Load journal");
    assert!(journal.active_transaction.is_none());
}

#[test]
fn test_adversarial_cross_device_exdev_safety_large_and_binary_payloads() {
    // Simulate distinct filesystems using two independent TempDir instances
    let src_temp = TempDir::new().expect("Source temp dir");
    let dst_temp = TempDir::new().expect("Target temp dir");

    let src_path = Utf8Path::from_path(src_temp.path())
        .expect("UTF-8 src")
        .join("staging")
        .join("heavy_asset.bin");
    let dst_path = Utf8Path::from_path(dst_temp.path())
        .expect("UTF-8 dst")
        .join("nested")
        .join("target_asset.bin");

    // Generate 1 MB binary payload with arbitrary non-ASCII bytes and null bytes
    let mut payload = Vec::with_capacity(1024 * 1024);
    for i in 0..(1024 * 1024) {
        payload.push((i % 256) as u8);
    }

    if let Some(p) = src_path.parent() {
        fs::create_dir_all(p).expect("Create src parent");
    }
    fs::write(&src_path, &payload).expect("Write binary payload");

    // Perform atomic swap across separate directory trees
    AtomicSwapper::swap_file(&src_path, &dst_path).expect("Cross-directory atomic swap");

    // Destination file exists and matches payload exactly
    assert!(dst_path.exists());
    let read_dst = fs::read(&dst_path).expect("Read dst file");
    assert_eq!(read_dst.len(), payload.len());
    assert_eq!(read_dst, payload);

    // Ensure sibling temp file in dst_path.parent() was completely cleaned up
    let parent = dst_path.parent().unwrap();
    let strays = AtomicSwapper::prune_stray_tmp_files(parent).expect("Prune strays");
    assert_eq!(strays, 0);

    // Test error condition: source file does not exist
    let missing_src = src_path.parent().unwrap().join("non_existent.bin");
    let missing_dst = parent.join("should_fail.bin");
    let err = AtomicSwapper::swap_file(&missing_src, &missing_dst).expect_err("Must fail");
    assert!(matches!(err, WoderriseError::Io(_)));
    assert!(!missing_dst.exists());
}

#[test]
fn test_adversarial_dirty_state_detection_subtle_modification() {
    let env = TestEnv::new();
    let engine = TransactionEngine::new(env.xdg.clone()).expect("Transaction engine");
    let tx_id = "tx_subtle_dirty";

    let waybar_dir = env.xdg.component_root(ConfigComponent::Waybar);
    fs::create_dir_all(&waybar_dir).expect("Create waybar dir");
    let config_file = waybar_dir.join("config.jsonc");

    let original_json = b"{\n  \"layer\": \"top\",\n  \"position\": \"top\"\n}\n";
    fs::write(&config_file, original_json).expect("Write original json");

    let entries = vec![(
        TreeEntry::new("config.jsonc", "100644", "blob", "sha_waybar", Some(200)),
        b"{\n  \"layer\": \"bottom\",\n  \"position\": \"bottom\"\n}\n".to_vec(),
    )];

    engine
        .stage_transaction(
            tx_id,
            ConfigComponent::Waybar,
            "user/dots",
            "c_dirty",
            &entries,
        )
        .expect("Stage transaction");

    let ws = StagingWorkspace::new(&env.xdg, tx_id).unwrap();
    let manifest = ws.load_manifest().unwrap();
    engine
        .apply_transaction(
            tx_id,
            ConfigComponent::Waybar,
            "user/dots",
            "c_dirty",
            &manifest.files,
        )
        .expect("Apply transaction");

    // SUBTLE USER EDIT: single trailing space added to config.jsonc
    let mut modified = fs::read(&config_file).expect("Read config");
    modified.push(b' '); // Appending 1 space
    fs::write(&config_file, &modified).expect("Write subtle dirty config");

    // 1. Unforced rollback must detect dirty state and halt immediately
    let err = engine
        .rollback_latest(false)
        .expect_err("Rollback must be rejected due to subtle 1-byte dirty state");

    match err {
        WoderriseError::Transaction(TransactionError::DirtyStateDetected {
            file,
            expected_sha256,
            actual_sha256,
        }) => {
            assert_eq!(file, config_file);
            assert_ne!(expected_sha256, actual_sha256);
        }
        other => panic!("Expected DirtyStateDetected, got: {:?}", other),
    }

    // Ensure file on disk was NOT touched during the aborted check
    assert_eq!(fs::read(&config_file).unwrap(), modified);

    // 2. Forced rollback must bypass the dirty-state check and restore original content
    let report = engine
        .rollback_latest(true)
        .expect("Forced rollback must succeed");
    assert_eq!(report.tx_id, tx_id);

    // Verify original content was restored verbatim
    let restored = fs::read(&config_file).expect("Read restored file");
    assert_eq!(restored, original_json);
}

#[test]
fn test_adversarial_lifo_order_violation_multi_tier() {
    let env = TestEnv::new();
    let engine = TransactionEngine::new(env.xdg.clone()).expect("Transaction engine");

    // Stage & apply 3 transactions in sequence: TX_1, TX_2, TX_3
    for i in 1..=3 {
        let tx_id = format!("tx_lifo_{}", i);
        let filename = format!("file_{}.conf", i);
        let entries = vec![(
            TreeEntry::new(&filename, "100644", "blob", format!("sha_{}", i), Some(50)),
            format!("# Content of file {}\n", i).into_bytes(),
        )];

        engine
            .stage_transaction(
                &tx_id,
                ConfigComponent::Hyprland,
                "user/repo",
                &format!("commit_{}", i),
                &entries,
            )
            .expect("Stage");

        let ws = StagingWorkspace::new(&env.xdg, &tx_id).unwrap();
        let m = ws.load_manifest().unwrap();
        engine
            .apply_transaction(
                &tx_id,
                ConfigComponent::Hyprland,
                "user/repo",
                &format!("commit_{}", i),
                &m.files,
            )
            .expect("Apply");
    }

    // LIFO Stack is: [tx_lifo_1, tx_lifo_2, tx_lifo_3] (top: tx_lifo_3)

    // Attempting to rollback tx_lifo_1 must fail, naming tx_lifo_3 as top
    let err1 = engine
        .rollback_transaction("tx_lifo_1", false)
        .expect_err("Rollback tx_lifo_1 must violate LIFO");
    match err1 {
        WoderriseError::Transaction(TransactionError::LifoViolation { tx_id, top_tx_id }) => {
            assert_eq!(tx_id, "tx_lifo_1");
            assert_eq!(top_tx_id, "tx_lifo_3");
        }
        other => panic!("Expected LifoViolation, got: {:?}", other),
    }

    // Attempting to rollback tx_lifo_2 must fail, naming tx_lifo_3 as top
    let err2 = engine
        .rollback_transaction("tx_lifo_2", false)
        .expect_err("Rollback tx_lifo_2 must violate LIFO");
    match err2 {
        WoderriseError::Transaction(TransactionError::LifoViolation { tx_id, top_tx_id }) => {
            assert_eq!(tx_id, "tx_lifo_2");
            assert_eq!(top_tx_id, "tx_lifo_3");
        }
        other => panic!("Expected LifoViolation, got: {:?}", other),
    }

    // Rollback top transaction tx_lifo_3: succeeds
    engine
        .rollback_transaction("tx_lifo_3", false)
        .expect("Rollback tx_lifo_3 succeeds");

    // Now top is tx_lifo_2. Attempting tx_lifo_1 must fail naming tx_lifo_2
    let err3 = engine
        .rollback_transaction("tx_lifo_1", false)
        .expect_err("Rollback tx_lifo_1 must violate LIFO with top tx_lifo_2");
    match err3 {
        WoderriseError::Transaction(TransactionError::LifoViolation { tx_id, top_tx_id }) => {
            assert_eq!(tx_id, "tx_lifo_1");
            assert_eq!(top_tx_id, "tx_lifo_2");
        }
        other => panic!("Expected LifoViolation, got: {:?}", other),
    }

    // Rollback tx_lifo_2: succeeds
    engine
        .rollback_transaction("tx_lifo_2", false)
        .expect("Rollback tx_lifo_2 succeeds");

    // Finally rollback tx_lifo_1: succeeds
    engine
        .rollback_transaction("tx_lifo_1", false)
        .expect("Rollback tx_lifo_1 succeeds");

    // History now has all 3 marked RolledBack
    let history = engine.get_history().expect("Get history");
    assert_eq!(history.len(), 3);
    for h in history {
        assert_eq!(h.status, TransactionStatus::RolledBack);
    }
}

#[test]
fn test_adversarial_deep_directory_pruning_boundary_protection() {
    let env = TestEnv::new();
    let engine = TransactionEngine::new(env.xdg.clone()).expect("Transaction engine");
    let tx_id = "tx_deep_prune";

    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create hypr dir");

    // Pre-existing user file inside a shared directory: hypr/alpha/beta/user_kept.conf
    let alpha_beta_dir = hypr_dir.join("alpha").join("beta");
    fs::create_dir_all(&alpha_beta_dir).expect("Create alpha/beta");
    let user_kept = alpha_beta_dir.join("user_kept.conf");
    fs::write(&user_kept, b"# User manually created file\n").expect("Write user_kept");

    // Transaction will create a new file in a deep nested subdirectory:
    // hypr/alpha/beta/gamma/delta/installed.conf
    let entries = vec![(
        TreeEntry::new(
            "alpha/beta/gamma/delta/installed.conf",
            "100644",
            "blob",
            "sha_deep",
            Some(100),
        ),
        b"# Installed deep configuration\n".to_vec(),
    )];

    engine
        .stage_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "c_deep",
            &entries,
        )
        .expect("Stage");

    let ws = StagingWorkspace::new(&env.xdg, tx_id).unwrap();
    let m = ws.load_manifest().unwrap();
    engine
        .apply_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "c_deep",
            &m.files,
        )
        .expect("Apply");

    let deep_file = alpha_beta_dir
        .join("gamma")
        .join("delta")
        .join("installed.conf");
    assert!(deep_file.exists());

    // Rollback
    let report = engine
        .rollback_latest(false)
        .expect("Rollback deep transaction");
    assert_eq!(report.tx_id, tx_id);

    // Assertions:
    // 1. installed.conf deleted
    assert!(!deep_file.exists());
    // 2. gamma/delta pruned
    assert!(!alpha_beta_dir.join("gamma").join("delta").exists());
    // 3. gamma pruned
    assert!(!alpha_beta_dir.join("gamma").exists());
    // 4. alpha/beta NOT pruned because user_kept.conf exists inside it!
    assert!(alpha_beta_dir.exists());
    assert!(user_kept.exists());
    // 5. Component root hypr NOT pruned
    assert!(hypr_dir.exists());
}

#[cfg(unix)]
#[test]
fn test_adversarial_symlink_preservation_and_dereferencing() {
    use std::os::unix::fs::symlink;

    let env = TestEnv::new();
    let engine = TransactionEngine::new(env.xdg.clone()).expect("Transaction engine");
    let tx_id = "tx_stow_full_lifecycle";

    // 1. External dotfiles repository (e.g., ~/dotfiles/hypr/hyprland.conf)
    let dotfiles_dir = env.base_path.join("dotfiles").join("hypr");
    fs::create_dir_all(&dotfiles_dir).expect("Create dotfiles dir");
    let real_dotfile = dotfiles_dir.join("hyprland.conf");
    let original_dotfile_bytes = b"# Original Dotfile Managed By GNU Stow\n";
    fs::write(&real_dotfile, original_dotfile_bytes).expect("Write real dotfile");

    // 2. Symlink in ~/.config/hypr/hyprland.conf -> ~/dotfiles/hypr/hyprland.conf
    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create hypr dir");
    let stow_symlink = hypr_dir.join("hyprland.conf");
    symlink(&real_dotfile, &stow_symlink).expect("Create stow symlink");

    // 3. Stage and apply a transaction targeting hyprland.conf
    let entries = vec![(
        TreeEntry::new("hyprland.conf", "100644", "blob", "sha_stow", Some(200)),
        b"# Updated Content via Woderrise Transaction\n".to_vec(),
    )];

    engine
        .stage_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "c_stow",
            &entries,
        )
        .expect("Stage transaction");

    let ws = StagingWorkspace::new(&env.xdg, tx_id).unwrap();
    let manifest = ws.load_manifest().unwrap();

    engine
        .apply_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "c_stow",
            &manifest.files,
        )
        .expect("Apply transaction");

    // 4. Assert symlink was preserved and NOT clobbered with a regular file
    let meta_after_apply = fs::symlink_metadata(&stow_symlink).expect("Symlink metadata");
    assert!(
        meta_after_apply.file_type().is_symlink(),
        "Symlink was severed and turned into regular file!"
    );

    // Reading through symlink returns the updated content
    assert_eq!(
        fs::read(&stow_symlink).expect("Read via symlink"),
        b"# Updated Content via Woderrise Transaction\n"
    );
    // Real destination was updated
    assert_eq!(
        fs::read(&real_dotfile).expect("Read real dotfile"),
        b"# Updated Content via Woderrise Transaction\n"
    );

    // 5. Rollback transaction
    let report = engine
        .rollback_latest(false)
        .expect("Rollback symlink transaction");
    assert_eq!(report.tx_id, tx_id);

    // 6. Assert symlink STILL exists and points to real dotfile with original bytes restored
    let meta_after_rollback =
        fs::symlink_metadata(&stow_symlink).expect("Symlink metadata after rollback");
    assert!(
        meta_after_rollback.file_type().is_symlink(),
        "Symlink was corrupted during rollback!"
    );

    assert_eq!(
        fs::read(&stow_symlink).expect("Read restored via symlink"),
        original_dotfile_bytes
    );
    assert_eq!(
        fs::read(&real_dotfile).expect("Read restored real dotfile"),
        original_dotfile_bytes
    );
}

#[test]
fn test_adversarial_security_audit_hostile_inputs() {
    let env = TestEnv::new();

    // 1. Case-insensitive script extension check (.SH, .Bash, .PY, .BIN)
    for bad_ext in &["script.SH", "runner.Bash", "tool.PY", "blob.BIN", "exec.Rb"] {
        let entry = TreeEntry::new(*bad_ext, "100644", "blob", "sha_bad", Some(100));
        let err = SecurityAuditor::audit_entry(&entry, ConfigComponent::Hyprland, &env.xdg)
            .expect_err(&format!("Must reject uppercase extension {}", bad_ext));
        assert!(matches!(
            err,
            WoderriseError::Security(SecurityError::ScriptExecutionBlocked { .. })
        ));
    }

    // 2. Mode 120000 Git tree symlink object rejection (CVE-2021-21300)
    for symlink_mode in &["120000", "120755", "120644"] {
        let entry = TreeEntry::new("symlink_obj", *symlink_mode, "blob", "sha_sym", Some(50));
        let err = SecurityAuditor::audit_entry(&entry, ConfigComponent::Hyprland, &env.xdg)
            .expect_err(&format!("Must reject symlink mode {}", symlink_mode));
        assert!(matches!(
            err,
            WoderriseError::Path(PathError::ForbiddenSymlink { .. })
        ));
    }

    // 3. Executable permissions (100755, 0755, etc.)
    for exec_mode in &["100755", "0755"] {
        let entry = TreeEntry::new("normal.conf", *exec_mode, "blob", "sha_exec", Some(50));
        let err = SecurityAuditor::audit_entry(&entry, ConfigComponent::Hyprland, &env.xdg)
            .expect_err(&format!("Must reject executable mode {}", exec_mode));
        assert!(matches!(
            err,
            WoderriseError::Security(SecurityError::ScriptExecutionBlocked { .. })
        ));
    }

    // 4. Hardware display configurations protection
    for hw_file in &[
        "monitors.conf",
        "workspaces.conf",
        "hardware.conf",
        "sub/monitors.conf",
    ] {
        let entry = TreeEntry::new(*hw_file, "100644", "blob", "sha_hw", Some(100));
        let mapping = SecurityAuditor::audit_entry(&entry, ConfigComponent::Hyprland, &env.xdg)
            .expect("Audit must accept hardware config");
        assert!(mapping.is_hardware_protected);
        assert_eq!(mapping.conflict_status, ConflictStatus::HardwareProtected);
    }
}
