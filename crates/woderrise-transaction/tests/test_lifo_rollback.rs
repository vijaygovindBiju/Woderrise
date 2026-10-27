mod common;

use common::TestEnv;
use std::fs;
use woderrise_core::error::{TransactionError, WoderriseError};
use woderrise_core::models::{ConfigComponent, TransactionStatus};
use woderrise_detect::TreeEntry;
use woderrise_transaction::TransactionEngine;

#[test]
fn test_lifo_rollback_restores_pre_install_state() {
    let env = TestEnv::new();
    let engine = TransactionEngine::new(env.xdg.clone()).expect("Transaction engine");
    let tx_id = "tx_lifo_001";

    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create hypr dir");

    // 1. Initial file
    let hypr_conf = hypr_dir.join("hyprland.conf");
    let original_bytes = b"# Original Hyprland Config V1\n";
    fs::write(&hypr_conf, original_bytes).expect("Write original");

    // 2. Stage transaction with 1 overwrite and 1 new file in nested subdir
    let entries = vec![
        (
            TreeEntry::new("hyprland.conf", "100644", "blob", "sha1", Some(100)),
            b"# Installed Hyprland Config V2\n".to_vec(),
        ),
        (
            TreeEntry::new("sub/theme.conf", "100644", "blob", "sha2", Some(100)),
            b"# Brand new nested theme config\n".to_vec(),
        ),
    ];

    let report = engine
        .stage_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "commit1",
            &entries,
        )
        .expect("Stage transaction");
    assert!(report.passed);

    let staging_workspace =
        woderrise_transaction::StagingWorkspace::new(&env.xdg, tx_id).expect("Staging workspace");
    let manifest = staging_workspace.load_manifest().expect("Load manifest");

    // 3. Apply transaction
    let entry = engine
        .apply_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "commit1",
            &manifest.files,
        )
        .expect("Apply transaction");
    assert_eq!(entry.status, TransactionStatus::Committed);

    // Verify files were installed
    assert_eq!(
        fs::read(&hypr_conf).expect("Read installed hyprland"),
        b"# Installed Hyprland Config V2\n"
    );
    let new_file = hypr_dir.join("sub").join("theme.conf");
    assert!(new_file.exists());

    // 4. Execute 1-Click Rollback
    let rollback_report = engine
        .rollback_latest(false)
        .expect("Rollback latest transaction");

    assert_eq!(rollback_report.tx_id, tx_id);
    assert_eq!(rollback_report.restored_files.len(), 1);
    assert_eq!(rollback_report.removed_files.len(), 1);

    // Assert original content was restored verbatim
    assert_eq!(
        fs::read(&hypr_conf).expect("Read restored hyprland"),
        original_bytes
    );

    // Assert newly created file was deleted
    assert!(
        !new_file.exists(),
        "New file was not deleted during rollback!"
    );

    // Assert empty parent directory was pruned
    let sub_dir = hypr_dir.join("sub");
    assert!(
        !sub_dir.exists(),
        "Empty directory 'sub' was not pruned during rollback!"
    );

    // Verify journal status is RolledBack
    let history = engine.get_history().expect("Get history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, TransactionStatus::RolledBack);
}

#[test]
fn test_lifo_order_violation() {
    let env = TestEnv::new();
    let engine = TransactionEngine::new(env.xdg.clone()).expect("Transaction engine");

    // Install tx_1
    let entries_1 = vec![(
        TreeEntry::new("config1.conf", "100644", "blob", "sha1", Some(50)),
        b"# Config 1\n".to_vec(),
    )];
    let _rep1 = engine
        .stage_transaction(
            "tx_1",
            ConfigComponent::Hyprland,
            "user/repo",
            "c1",
            &entries_1,
        )
        .expect("Stage 1");
    let ws1 = woderrise_transaction::StagingWorkspace::new(&env.xdg, "tx_1").unwrap();
    let m1 = ws1.load_manifest().unwrap();
    engine
        .apply_transaction(
            "tx_1",
            ConfigComponent::Hyprland,
            "user/repo",
            "c1",
            &m1.files,
        )
        .expect("Apply 1");

    // Install tx_2
    let entries_2 = vec![(
        TreeEntry::new("config2.conf", "100644", "blob", "sha2", Some(50)),
        b"# Config 2\n".to_vec(),
    )];
    let _rep2 = engine
        .stage_transaction(
            "tx_2",
            ConfigComponent::Hyprland,
            "user/repo",
            "c2",
            &entries_2,
        )
        .expect("Stage 2");
    let ws2 = woderrise_transaction::StagingWorkspace::new(&env.xdg, "tx_2").unwrap();
    let m2 = ws2.load_manifest().unwrap();
    engine
        .apply_transaction(
            "tx_2",
            ConfigComponent::Hyprland,
            "user/repo",
            "c2",
            &m2.files,
        )
        .expect("Apply 2");

    // Attempt to rollback older tx_1 while newer tx_2 exists
    let err = engine
        .rollback_transaction("tx_1", false)
        .expect_err("Must reject rolling back older transaction when newer exists (LIFO)");

    match err {
        WoderriseError::Transaction(TransactionError::LifoViolation { tx_id, top_tx_id }) => {
            assert_eq!(tx_id, "tx_1");
            assert_eq!(top_tx_id, "tx_2");
        }
        other => panic!("Expected LifoViolation, got: {:?}", other),
    }

    // Rollback top tx_2 first
    engine
        .rollback_transaction("tx_2", false)
        .expect("Rollback tx_2");

    // Now tx_1 can be rolled back safely
    engine
        .rollback_transaction("tx_1", false)
        .expect("Rollback tx_1");
}

#[test]
fn test_dirty_state_detection() {
    let env = TestEnv::new();
    let engine = TransactionEngine::new(env.xdg.clone()).expect("Transaction engine");
    let tx_id = "tx_dirty_001";

    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create hypr dir");
    let hypr_conf = hypr_dir.join("hyprland.conf");
    fs::write(&hypr_conf, b"# Original Initial\n").expect("Write initial");

    let entries = vec![(
        TreeEntry::new("hyprland.conf", "100644", "blob", "sha1", Some(100)),
        b"# Staged Version Installed\n".to_vec(),
    )];

    engine
        .stage_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "commit1",
            &entries,
        )
        .expect("Stage");
    let ws = woderrise_transaction::StagingWorkspace::new(&env.xdg, tx_id).unwrap();
    let m = ws.load_manifest().unwrap();
    engine
        .apply_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "commit1",
            &m.files,
        )
        .expect("Apply");

    // EXTERNAL USER MODIFICATION: User edits hyprland.conf in their text editor
    fs::write(&hypr_conf, b"# Manual User Modification After Install!\n").expect("User edit");

    // Rollback without force must be aborted due to dirty state
    let err = engine
        .rollback_latest(false)
        .expect_err("Rollback must detect dirty state and halt");

    match err {
        WoderriseError::Transaction(TransactionError::DirtyStateDetected { file, .. }) => {
            assert_eq!(file, hypr_conf);
        }
        other => panic!("Expected DirtyStateDetected, got: {:?}", other),
    }

    // Rollback with force_dirty = true must succeed
    let report = engine
        .rollback_latest(true)
        .expect("Forced dirty rollback must succeed");
    assert_eq!(report.tx_id, tx_id);

    // Verify original initial content is restored
    assert_eq!(
        fs::read(&hypr_conf).expect("Read restored"),
        b"# Original Initial\n"
    );
}

#[test]
fn test_hyprland_legacy_lua_deactivation_and_rollback() {
    let env = TestEnv::new();
    let engine = TransactionEngine::new(env.xdg.clone()).expect("Transaction engine");
    let tx_id = "tx_lua_001";

    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create hypr dir");

    // Legacy hyprland.conf exists
    let legacy_conf = hypr_dir.join("hyprland.conf");
    let legacy_content = b"# Legacy Hyprland Conf Syntax\n";
    fs::write(&legacy_conf, legacy_content).expect("Write legacy conf");

    // Installing modern hyprland.lua
    let entries = vec![(
        TreeEntry::new("hyprland.lua", "100644", "blob", "sha_lua", Some(150)),
        b"-- Modern Hyprland Lua Configuration\n".to_vec(),
    )];

    engine
        .stage_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "commit_lua",
            &entries,
        )
        .expect("Stage lua");
    let ws = woderrise_transaction::StagingWorkspace::new(&env.xdg, tx_id).unwrap();
    let m = ws.load_manifest().unwrap();

    engine
        .apply_transaction(
            tx_id,
            ConfigComponent::Hyprland,
            "user/dots",
            "commit_lua",
            &m.files,
        )
        .expect("Apply lua");

    let lua_file = hypr_dir.join("hyprland.lua");
    let disabled_legacy = hypr_dir.join("hyprland.conf.disabled_by_woderrise");

    // Verify lua was installed and legacy conf was disabled
    assert!(lua_file.exists());
    assert!(!legacy_conf.exists(), "Legacy conf was not deactivated!");
    assert!(
        disabled_legacy.exists(),
        "hyprland.conf.disabled_by_woderrise was not created!"
    );

    // Rollback
    engine
        .rollback_latest(false)
        .expect("Rollback lua installation");

    // Verify lua was removed and legacy conf was restored
    assert!(!lua_file.exists(), "Lua file was not removed!");
    assert!(legacy_conf.exists(), "Legacy conf was not restored!");
    assert_eq!(
        fs::read(&legacy_conf).expect("Read restored legacy"),
        legacy_content
    );
    assert!(
        !disabled_legacy.exists(),
        "Disabled legacy file still exists!"
    );
}
