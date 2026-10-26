//! Tier 1: Feature Coverage (F1 to F11)
//! Verifies each core feature in isolation via opaque-box CLI binary execution.

use super::common::{
    TestEnvironment, REAL_CATPPUCCIN_DIR, REAL_CATPPUCCIN_ID, REAL_CATPPUCCIN_VER,
};
use std::fs;

// ============================================================================
// F1: Package Indexing & Discovery (search, list)
// ============================================================================

#[test]
fn test_tier1_f1_registry_indexing_and_search() {
    let env = TestEnvironment::new("t1-f1-search");

    // 1. Search with empty query -> lists all packages in registry
    let out_empty = env.run(&["search"]);
    assert_eq!(out_empty.status.code(), Some(0));
    let stdout_empty = String::from_utf8_lossy(&out_empty.stdout);
    assert!(stdout_empty.contains(REAL_CATPPUCCIN_ID));
    assert!(stdout_empty.contains(REAL_CATPPUCCIN_VER));
    assert!(stdout_empty.contains("bar_themes_modules"));

    // 2. Search by matching keyword "waybar"
    let out_query = env.run(&["search", "waybar"]);
    assert_eq!(out_query.status.code(), Some(0));
    let stdout_query = String::from_utf8_lossy(&out_query.stdout);
    assert!(stdout_query.contains(REAL_CATPPUCCIN_ID));
    assert!(stdout_query.contains("Catppuccin Waybar Mocha Theme"));

    // 3. Search with non-matching query exits 0 with graceful message
    let out_none = env.run(&["search", "nonexistent_custom_xyz"]);
    assert_eq!(out_none.status.code(), Some(0));
    let stdout_none = String::from_utf8_lossy(&out_none.stdout);
    assert!(stdout_none.contains("No packages found matching"));
}

#[test]
fn test_tier1_f1_list_transactions_empty_initially() {
    let env = TestEnvironment::new("t1-f1-list");
    let output = env.run(&["list"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim().is_empty());
}

// ============================================================================
// F2: Diff & Impact Inspection Engine (diff)
// ============================================================================

#[test]
fn test_tier1_f2_diff_clean_install_create_action() {
    let env = TestEnvironment::new("t1-f2-diff");
    let target = env.target_file();
    assert!(!target.exists());

    let output = env.run(&["diff", REAL_CATPPUCCIN_DIR]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[CREATE]"));
    assert!(stdout.contains("catppuccin-mocha.css"));
    assert!(stdout.contains("Planned Backup: None"));
    assert!(stdout.contains("+@define-color base #1e1e2e;"));

    // INVARIANT: diff must be strictly read-only; target file must not be touched!
    assert!(!target.exists(), "diff must be strictly read-only");
}

#[test]
fn test_tier1_f2_diff_summary_mode() {
    let env = TestEnvironment::new("t1-f2-summary");
    let output = env.run(&["diff", REAL_CATPPUCCIN_DIR, "--summary"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Summary: 1 to create, 0 to modify, 0 unchanged, 0 to backup."));
    // Summary mode should omit the detailed Myers line differences
    assert!(!stdout.contains("+@define-color base #1e1e2e;"));
}

#[test]
fn test_tier1_f2_diff_no_color_flag() {
    let env = TestEnvironment::new("t1-f2-no-color");
    let output = env.run(&["diff", REAL_CATPPUCCIN_DIR, "--no-color"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\x1b[32m"), "ANSI escape codes must be suppressed with --no-color");
    assert!(stdout.contains("+@define-color base #1e1e2e;"));
}

// ============================================================================
// F3: Transactional Safety, Backups, and Rollback
// ============================================================================

#[test]
fn test_tier1_f3_transactional_install_and_journal_record() {
    let env = TestEnvironment::new("t1-f3-install");
    let target = env.target_file();
    assert!(!target.exists());

    let output = env.run(&["install", REAL_CATPPUCCIN_DIR]);
    assert_eq!(output.status.code(), Some(0), "Install failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let txn_id = stdout.trim();
    assert!(
        txn_id.to_uppercase().starts_with("TXN-"),
        "Expected transaction ID, got: {txn_id}"
    );

    // Target file must exist and contain genuine payload
    assert!(target.exists());
    let content = fs::read_to_string(&target).expect("failed to read installed target file");
    assert!(content.contains("@define-color base #1e1e2e;"));

    // Verify SQLite WAL Journal DB
    let conn = env.open_db();
    let (status, package_id, version): (String, String, String) = conn
        .query_row(
            "SELECT status, package_id, version FROM transactions WHERE txn_id = ?1",
            rusqlite::params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("transaction record missing in database");

    assert_eq!(status, "COMMITTED");
    assert_eq!(package_id, REAL_CATPPUCCIN_ID);
    assert_eq!(version, REAL_CATPPUCCIN_VER);

    let active_version: String = conn
        .query_row(
            "SELECT active_version FROM installed_packages WHERE package_id = ?1",
            rusqlite::params![REAL_CATPPUCCIN_ID],
            |r| r.get(0),
        )
        .expect("installed_packages entry missing");
    assert_eq!(active_version, REAL_CATPPUCCIN_VER);
}

#[test]
fn test_tier1_f3_safe_rollback_restores_clean_filesystem() {
    let env = TestEnvironment::new("t1-f3-rollback");
    let target = env.target_file();

    // 1. Install
    let inst_out = env.run(&["install", REAL_CATPPUCCIN_DIR]);
    assert_eq!(inst_out.status.code(), Some(0));
    let txn_id = String::from_utf8_lossy(&inst_out.stdout).trim().to_string();
    assert!(target.exists());

    // 2. Rollback
    let rb_out = env.run(&["rollback", &txn_id]);
    assert_eq!(rb_out.status.code(), Some(0), "Rollback failed: {}", String::from_utf8_lossy(&rb_out.stderr));

    // Target file must be deleted because it was newly created by the install
    assert!(!target.exists(), "Target file must be removed after rollback");

    // Journal verification:
    // Original transaction marked ROLLED_BACK
    let conn = env.open_db();
    let orig_status: String = conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            rusqlite::params![txn_id],
            |r| r.get(0),
        )
        .expect("original transaction record not found");
    assert_eq!(orig_status, "ROLLED_BACK");

    // New rollback transaction recorded as COMMITTED
    let rb_txn_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE txn_type = 'ROLLBACK' AND status = 'COMMITTED'",
            [],
            |r| r.get(0),
        )
        .expect("rollback transaction query failed");
    assert_eq!(rb_txn_count, 1);

    // installed_packages entry must be deleted
    let installed_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = ?1",
            rusqlite::params![REAL_CATPPUCCIN_ID],
            |r| r.get(0),
        )
        .expect("installed_packages count query failed");
    assert_eq!(installed_count, 0);
}

// ============================================================================
// F4 & F5: Progress Reporting & Test Hygiene
// ============================================================================

#[test]
fn test_tier1_f4_progress_reporting_and_status() {
    let env = TestEnvironment::new("t1-f4-progress");
    // Verify install returns non-empty output with exit code 0
    let output = env.run(&["install", REAL_CATPPUCCIN_DIR]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty());
}

#[test]
fn test_tier1_f5_test_workspace_hygiene() {
    let env = TestEnvironment::new("t1-f5-hygiene");
    // Assert that env allocates strictly inside temp_dir()
    let temp_root = std::env::temp_dir();
    assert!(env.home.starts_with(&temp_root));
    assert!(env.data.starts_with(&temp_root));

    // Workspace root must not contain stray test databases
    let ws = TestEnvironment::workspace_root();
    assert!(!ws.join("journal.db").exists());
    assert!(!ws.join("backups").exists());
}

// ============================================================================
// F6: Metadata, License, Authors, Compatibility Display (info)
// ============================================================================

#[test]
fn test_tier1_f6_info_metadata_and_provenance_display() {
    let env = TestEnvironment::new("t1-f6-info");

    let output = env.run(&["info", REAL_CATPPUCCIN_DIR]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Package:      com.catppuccin.waybar v1.0.0"));
    assert!(stdout.contains("Name:         Catppuccin Waybar Mocha Theme"));
    assert!(stdout.contains("Type:         theme"));
    assert!(stdout.contains("Category:     bar_themes_modules"));
    assert!(stdout.contains("License:      MIT"));
    assert!(stdout.contains("Repository:   https://github.com/catppuccin/waybar"));
    assert!(stdout.contains("Catppuccin contributors"));
    assert!(stdout.contains("Display Server: wayland_only"));
    assert!(stdout.contains("Architectures:  x86_64, aarch64"));
    assert!(stdout.contains("base"));
    assert!(stdout.contains("#1e1e2e"));
    assert!(stdout.contains("blue"));
    assert!(stdout.contains("#89b4fa"));
    assert!(stdout.contains("mauve"));
    assert!(stdout.contains("#cba6f7"));
    assert!(stdout.contains("text"));
    assert!(stdout.contains("#cdd6f4"));
}

// ============================================================================
// F7: Multi-File Diff Markers and Planned Backup Destinations
// ============================================================================

#[test]
fn test_tier1_f7_diff_modify_and_backup_destination_prediction() {
    let env = TestEnvironment::new("t1-f7-diff-modify");
    let target = env.target_file();
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "/* pre-existing custom waybar styles */\n").unwrap();

    let output = env.run(&["diff", REAL_CATPPUCCIN_DIR]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[MODIFY]"));
    assert!(stdout.contains("backups/"));
    assert!(stdout.contains("-/* pre-existing custom waybar styles */"));
    assert!(stdout.contains("+@define-color base #1e1e2e;"));
}

// ============================================================================
// F8 & F9: Status and Offline Resilience
// ============================================================================

#[test]
fn test_tier1_f8_f9_offline_resilience_and_no_network_dependence() {
    let env = TestEnvironment::new("t1-f9-offline");
    let bin_path = env!("CARGO_BIN_EXE_woderrise");
    let ws = TestEnvironment::workspace_root();
    let reg_dir = ws.join("registry/packages");

    // Run with non-routable proxy to simulate complete network blackout
    let output = std::process::Command::new(bin_path)
        .current_dir(&ws)
        .env("HOME", &env.home)
        .env("WODERRISE_DATA_DIR", &env.data)
        .env("WODERRISE_REGISTRY_DIR", &reg_dir)
        .env("HTTP_PROXY", "http://127.0.0.1:65534")
        .env("HTTPS_PROXY", "http://127.0.0.1:65534")
        .env("ALL_PROXY", "socks5://127.0.0.1:65534")
        .arg("--data-dir")
        .arg(&env.data)
        .args(["search", "waybar"])
        .output()
        .expect("failed to run CLI in offline mode");

    assert_eq!(output.status.code(), Some(0), "CLI must function 100% offline without crashing");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(REAL_CATPPUCCIN_ID));
}

// ============================================================================
// F10: CLI Capabilities Matrix
// ============================================================================

#[test]
fn test_tier1_f10_cli_capabilities_matrix() {
    let env = TestEnvironment::new("t1-f10-matrix");

    // All subcommands must accept --help with exit code 0
    for sub in &["diff", "search", "info", "recover", "list", "lint", "install", "rollback"] {
        let out = env.run(&[sub, "--help"]);
        assert_eq!(out.status.code(), Some(0), "Subcommand {sub} --help failed");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("Usage:"));
    }

    // Missing arguments must exit with code 2 (clap usage error)
    let out_missing = env.run(&["diff"]);
    assert_eq!(out_missing.status.code(), Some(2));

    let out_info_missing = env.run(&["info"]);
    assert_eq!(out_info_missing.status.code(), Some(2));
}

// ============================================================================
// F11: Interrupted Transaction Recovery (recover)
// ============================================================================

#[test]
fn test_tier1_f11_recover_interrupted_transactions() {
    let env = TestEnvironment::new("t1-f11-recover");
    let conn = env.open_db();

    // Run search once to ensure tables are created
    let _ = env.run(&["search"]);

    // Insert an interrupted EXECUTING transaction
    let txn_id = "txn-tier1-interrupted-01";
    conn.execute(
        "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator)
         VALUES (?1, 'com.catppuccin.waybar', '1.0.0', 'INSTALL', 'EXECUTING', 1000, 'test_runner')",
        rusqlite::params![txn_id],
    )
    .unwrap();

    let output = env.run(&["recover"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1"));
    assert!(stdout.contains("ABORTED_RECOVERED"));

    // Verify database record transitioned
    let status: String = conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            rusqlite::params![txn_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "ABORTED_RECOVERED");
}
