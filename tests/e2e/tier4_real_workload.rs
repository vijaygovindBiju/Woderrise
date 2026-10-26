//! Tier 4: Real-World Application Workload Scenarios
//! End-to-end desktop customization workflow using real package `com.catppuccin.waybar/1.0.0`.

use super::common::{
    TestEnvironment, REAL_CATPPUCCIN_DIR, REAL_CATPPUCCIN_ID, REAL_CATPPUCCIN_VER,
};
use std::fs;

#[test]
fn test_tier4_catppuccin_waybar_complete_customization_lifecycle() {
    let env = TestEnvironment::new("t4-catppuccin-lifecycle");
    let target = env.target_file();

    // ------------------------------------------------------------------------
    // Step 1: Discovery
    // Search the catalog using FTS5 for waybar customization packages
    // ------------------------------------------------------------------------
    let search_out = env.run(&["search", "waybar"]);
    assert_eq!(search_out.status.code(), Some(0));
    let search_txt = String::from_utf8_lossy(&search_out.stdout);
    assert!(search_txt.contains(REAL_CATPPUCCIN_ID));
    assert!(search_txt.contains(REAL_CATPPUCCIN_VER));
    assert!(search_txt.contains("bar_themes_modules"));
    assert!(search_txt.contains("Catppuccin Waybar Mocha Theme"));

    // ------------------------------------------------------------------------
    // Step 2: Metadata Inspection
    // Inspect manifest details, license, author provenance, and palette swatches
    // ------------------------------------------------------------------------
    let info_out = env.run(&["info", REAL_CATPPUCCIN_ID]);
    assert_eq!(info_out.status.code(), Some(0));
    let info_txt = String::from_utf8_lossy(&info_out.stdout);
    assert!(info_txt.contains("License:      MIT"));
    assert!(info_txt.contains("Repository:   https://github.com/catppuccin/waybar"));
    assert!(info_txt.contains("Catppuccin contributors"));
    assert!(info_txt.contains("Display Server: wayland_only"));
    assert!(info_txt.contains("#1e1e2e"));
    assert!(info_txt.contains("#89b4fa"));
    assert!(info_txt.contains("#cba6f7"));
    assert!(info_txt.contains("#cdd6f4"));

    // ------------------------------------------------------------------------
    // Step 3: Diff Preview (Clean Initial State)
    // Preview source-versus-target diff prior to installation
    // ------------------------------------------------------------------------
    assert!(!target.exists(), "Target configuration must not exist prior to install");
    let diff_out = env.run(&["diff", REAL_CATPPUCCIN_DIR]);
    assert_eq!(diff_out.status.code(), Some(0));
    let diff_txt = String::from_utf8_lossy(&diff_out.stdout);
    assert!(diff_txt.contains("[CREATE]"));
    assert!(diff_txt.contains("Planned Backup: None"));
    assert!(diff_txt.contains("+@define-color rosewater #f5e0dc;"));
    assert!(diff_txt.contains("+@define-color base #1e1e2e;"));
    assert!(!target.exists(), "Diff must be read-only and not place files");

    // ------------------------------------------------------------------------
    // Step 4: Transactional Installation
    // Perform installation and verify atomic placement and journal record
    // ------------------------------------------------------------------------
    let inst_out = env.run(&["install", REAL_CATPPUCCIN_DIR]);
    assert_eq!(inst_out.status.code(), Some(0), "Install failed: {}", String::from_utf8_lossy(&inst_out.stderr));
    let txn_id = String::from_utf8_lossy(&inst_out.stdout).trim().to_string();
    assert!(txn_id.to_uppercase().starts_with("TXN-"));

    // Verify placed target file
    assert!(target.exists());
    let installed_bytes = fs::read(&target).expect("failed to read installed target file");
    assert!(installed_bytes.len() > 100);

    // Verify journal status
    let conn = env.open_db();
    let (status, initiator): (String, String) = conn
        .query_row(
            "SELECT status, initiator FROM transactions WHERE txn_id = ?1",
            rusqlite::params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("journal entry missing");
    assert_eq!(status, "COMMITTED");
    assert_eq!(initiator, "user_cli");

    // ------------------------------------------------------------------------
    // Step 5: User Customization & Modification Diff Preview
    // Simulate user editing the installed style file
    // ------------------------------------------------------------------------
    let custom_rule = "\n/* Custom user override for active workspace button */\n#waybar button.active { background: #89b4fa; }\n";
    let mut modified_content = String::from_utf8_lossy(&installed_bytes).to_string();
    modified_content.push_str(custom_rule);
    fs::write(&target, &modified_content).expect("failed to write custom modifications");

    // Diff preview after local edits must detect [MODIFY] action and predict backup path
    let diff2_out = env.run(&["diff", REAL_CATPPUCCIN_DIR]);
    assert_eq!(diff2_out.status.code(), Some(0));
    let diff2_txt = String::from_utf8_lossy(&diff2_out.stdout);
    assert!(diff2_txt.contains("[MODIFY]"));
    assert!(diff2_txt.contains("backups/"));
    assert!(diff2_txt.contains("-#waybar button.active { background: #89b4fa; }"));

    // ------------------------------------------------------------------------
    // Step 6: Safe Rollback
    // Rollback without force detects user edit conflict; rollback --force succeeds
    // ------------------------------------------------------------------------
    let rb_unforced = env.run(&["rollback", &txn_id]);
    assert_eq!(
        rb_unforced.status.code(),
        Some(1),
        "Rollback without --force on locally edited file must detect conflict"
    );

    let rb_out = env.run(&["rollback", &txn_id, "--force"]);
    assert_eq!(rb_out.status.code(), Some(0), "Rollback with --force failed: {}", String::from_utf8_lossy(&rb_out.stderr));

    // Because the file was originally created by this transaction, rollback removes it
    assert!(!target.exists(), "Target file must be removed upon rollback");

    // Verify journal state consistency
    let orig_status: String = conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            rusqlite::params![txn_id],
            |r| r.get(0),
        )
        .expect("original transaction not found");
    assert_eq!(orig_status, "ROLLED_BACK");

    let rb_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE txn_type = 'ROLLBACK' AND status = 'COMMITTED'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rb_count, 1);

    let installed_pkgs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = ?1",
            rusqlite::params![REAL_CATPPUCCIN_ID],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(installed_pkgs, 0);
}

#[test]
fn test_tier4_preexisting_file_backup_and_restoration_on_rollback() {
    let env = TestEnvironment::new("t4-preexisting-backup");
    let target = env.target_file();
    fs::create_dir_all(target.parent().unwrap()).unwrap();

    let initial_user_config = "/* My original waybar styles before Woderrise */\n#workspaces { font-size: 14px; }\n";
    fs::write(&target, initial_user_config).unwrap();

    // 1. Install package over existing configuration
    let inst_out = env.run(&["install", REAL_CATPPUCCIN_DIR]);
    assert_eq!(inst_out.status.code(), Some(0));
    let txn_id = String::from_utf8_lossy(&inst_out.stdout).trim().to_string();

    // Verify target file was replaced with Catppuccin theme
    let installed_content = fs::read_to_string(&target).unwrap();
    assert!(installed_content.contains("@define-color base #1e1e2e;"));

    // Verify backup snapshot was created in backups/
    let backups_dir = env.data.join("backups");
    assert!(backups_dir.is_dir());
    let mut backup_entries = fs::read_dir(&backups_dir).unwrap();
    assert!(backup_entries.next().is_some(), "Backup snapshot must exist in content-addressed store");

    // 2. Rollback
    let rb_out = env.run(&["rollback", &txn_id]);
    assert_eq!(rb_out.status.code(), Some(0));

    // INVARIANT: User's original pre-existing configuration must be faithfully restored!
    assert!(target.exists());
    let restored_content = fs::read_to_string(&target).unwrap();
    assert_eq!(restored_content, initial_user_config);
}
