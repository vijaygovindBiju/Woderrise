//! Tier 3: Cross-Feature Interactions & Combinatorial State Transitions
//! Verifies state transitions across sequences of commands and crash recovery workflows.

use super::common::{
    TestEnvironment, REAL_CATPPUCCIN_DIR, REAL_CATPPUCCIN_ID, REAL_CATPPUCCIN_VER,
};
use std::fs;

// ============================================================================
// 1. Sequential Full Lifecycle Across All Subcommands
// ============================================================================

#[test]
fn test_tier3_lifecycle_search_info_diff_install_diff_rollback() {
    let env = TestEnvironment::new("t3-lifecycle");
    let target = env.target_file();

    // 1. Search prior to install: package is not installed
    let out_s1 = env.run(&["search", "waybar"]);
    assert_eq!(out_s1.status.code(), Some(0));
    let s1 = String::from_utf8_lossy(&out_s1.stdout);
    assert!(s1.contains(REAL_CATPPUCCIN_ID));
    assert!(!s1.contains("[installed]"));

    // 2. Info inspection
    let out_info = env.run(&["info", REAL_CATPPUCCIN_ID]);
    assert_eq!(out_info.status.code(), Some(0));
    let s_info = String::from_utf8_lossy(&out_info.stdout);
    assert!(s_info.contains("Catppuccin Waybar Mocha Theme"));

    // 3. Diff preview before installation: action [CREATE]
    let out_diff1 = env.run(&["diff", REAL_CATPPUCCIN_ID]);
    assert_eq!(out_diff1.status.code(), Some(0));
    let s_d1 = String::from_utf8_lossy(&out_diff1.stdout);
    assert!(s_d1.contains("[CREATE]"));
    assert!(!target.exists(), "diff must be strictly read-only");

    // 4. Install
    let out_inst = env.run(&["install", REAL_CATPPUCCIN_DIR]);
    assert_eq!(out_inst.status.code(), Some(0));
    let txn_id = String::from_utf8_lossy(&out_inst.stdout).trim().to_string();
    assert!(target.exists());

    // 5. Search post-install: package is marked [installed]
    let out_s2 = env.run(&["search", "waybar"]);
    assert_eq!(out_s2.status.code(), Some(0));
    let s2 = String::from_utf8_lossy(&out_s2.stdout);
    assert!(s2.contains("[installed]"));

    // 6. Diff post-install: action [UNCHANGED]
    let out_diff2 = env.run(&["diff", REAL_CATPPUCCIN_ID]);
    assert_eq!(out_diff2.status.code(), Some(0));
    let s_d2 = String::from_utf8_lossy(&out_diff2.stdout);
    assert!(s_d2.contains("[UNCHANGED]"));

    // 7. User mutates target file: append custom rules
    let mut cur_content = fs::read_to_string(&target).unwrap();
    cur_content.push_str("\n/* user customized modification */\n");
    fs::write(&target, &cur_content).unwrap();

    // Diff after mutation: action [MODIFY], planned backup is predicted
    let out_diff3 = env.run(&["diff", REAL_CATPPUCCIN_ID]);
    assert_eq!(out_diff3.status.code(), Some(0));
    let s_d3 = String::from_utf8_lossy(&out_diff3.stdout);
    assert!(s_d3.contains("[MODIFY]"));
    assert!(s_d3.contains("backups/"));

    // 8. Rollback without force when file was modified must detect conflict and exit code 1
    let out_rb_unforced = env.run(&["rollback", &txn_id]);
    assert_eq!(
        out_rb_unforced.status.code(),
        Some(1),
        "Rollback without --force on modified file must detect conflict"
    );

    // Rollback with --force overrides conflict and restores clean state
    let out_rb_force = env.run(&["rollback", &txn_id, "--force"]);
    assert_eq!(
        out_rb_force.status.code(),
        Some(0),
        "Rollback with --force must succeed: {}",
        String::from_utf8_lossy(&out_rb_force.stderr)
    );

    // Target file is cleanly removed
    assert!(!target.exists(), "Target file must be removed upon rollback");

    // 9. Search post-rollback: package is no longer [installed]
    let out_s3 = env.run(&["search", "waybar"]);
    assert_eq!(out_s3.status.code(), Some(0));
    let s3 = String::from_utf8_lossy(&out_s3.stdout);
    assert!(!s3.contains("[installed]"));

    // 10. List post-rollback shows both transactions
    let out_list = env.run(&["list"]);
    assert_eq!(out_list.status.code(), Some(0));
    let s_list = String::from_utf8_lossy(&out_list.stdout);
    assert!(s_list.contains("ROLLED_BACK"));
    assert!(s_list.contains("COMMITTED"));
}

// ============================================================================
// 2. Interrupted Install Recovery Followed by Reinstallation
// ============================================================================

#[test]
fn test_tier3_interrupted_install_recovery_then_reinstall() {
    let env = TestEnvironment::new("t3-interrupted-reinstall");
    let target = env.target_file();

    // 1. Initialize schema via search
    let _ = env.run(&["search"]);
    let conn = env.open_db();

    // 2. Simulate interrupted EXECUTING transaction
    let interrupted_txn = "txn-crash-interrupted-77";
    conn.execute(
        "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator)
         VALUES (?1, ?2, ?3, 'INSTALL', 'EXECUTING', 12345, 'cli_user')",
        rusqlite::params![interrupted_txn, REAL_CATPPUCCIN_ID, REAL_CATPPUCCIN_VER],
    )
    .unwrap();

    // Insert tentative record in installed_packages
    conn.execute(
        "INSERT INTO installed_packages(package_id, active_version, package_type, installed_at, updated_at, is_explicit, ref_count, install_txn_id)
         VALUES (?1, ?2, 'theme', 12345, 12345, 1, 1, ?3)",
        rusqlite::params![REAL_CATPPUCCIN_ID, REAL_CATPPUCCIN_VER, interrupted_txn],
    )
    .unwrap();

    // Simulate an uncommitted half-written partial file
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "/* partially written corrupt file from crashed install */").unwrap();

    // Record COMPLETED operation in operations table so recovery knows which file to compensate
    conn.execute(
        "INSERT INTO operations(op_id, txn_id, sequence_num, op_type, target_path, pre_sha256, post_sha256, backup_id, status)
         VALUES ('op-crash-01', ?1, 1, 'CREATE_FILE', ?2, NULL, 'partial_sha', NULL, 'COMPLETED')",
        rusqlite::params![interrupted_txn, target.to_str().unwrap()],
    )
    .unwrap();

    // 3. Execute recovery
    let out_rec = env.run(&["recover"]);
    assert_eq!(out_rec.status.code(), Some(0));
    let s_rec = String::from_utf8_lossy(&out_rec.stdout);
    assert!(s_rec.contains("Successfully recovered 1 transaction(s)"));

    // INVARIANT: Uncommitted file must be removed!
    assert!(!target.exists(), "Uncommitted file must be deleted during recovery");

    // INVARIANT: installed_packages must be purged
    let inst_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = ?1",
            rusqlite::params![REAL_CATPPUCCIN_ID],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(inst_count, 0);

    // 4. Fresh reinstallation now succeeds smoothly
    let out_inst = env.run(&["install", REAL_CATPPUCCIN_DIR]);
    assert_eq!(out_inst.status.code(), Some(0));
    assert!(target.exists());

    // Content is authentic
    let content = fs::read_to_string(&target).unwrap();
    assert!(content.contains("@define-color base #1e1e2e;"));
}

// ============================================================================
// 3. Double Rollback Prevention
// ============================================================================

#[test]
fn test_tier3_double_rollback_prevention() {
    let env = TestEnvironment::new("t3-double-rollback");

    // 1. Install package
    let out_inst = env.run(&["install", REAL_CATPPUCCIN_DIR]);
    assert_eq!(out_inst.status.code(), Some(0));
    let txn_id = String::from_utf8_lossy(&out_inst.stdout).trim().to_string();

    // 2. Rollback once -> succeeds
    let out_rb1 = env.run(&["rollback", &txn_id]);
    assert_eq!(out_rb1.status.code(), Some(0));

    // 3. Attempt to rollback the same transaction again -> must fail with code 1
    let out_rb2 = env.run(&["rollback", &txn_id]);
    assert_eq!(out_rb2.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out_rb2.stderr);
    assert!(stderr.starts_with("woderrise:"));
}
