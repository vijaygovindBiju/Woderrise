//! Tier 2: Boundary & Corner Cases
//! Exercises extreme, adversarial, and invalid inputs across the opaque-box CLI boundary.

use super::common::{TestEnvironment, REAL_CATPPUCCIN_DIR};
use std::{fs, os::unix::fs::PermissionsExt};

// ============================================================================
// 1. Search Query Boundaries & FTS5 Metacharacters
// ============================================================================

#[test]
fn test_tier2_search_empty_query_lists_all() {
    let env = TestEnvironment::new("t2-search-empty");
    let output = env.run(&["search", ""]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PACKAGE ID"));
    assert!(stdout.contains("com.catppuccin.waybar"));
}

#[test]
fn test_tier2_search_fts5_special_metacharacters_no_crash() {
    let env = TestEnvironment::new("t2-search-fts5");
    let adversarial_queries = [
        r#"""#,
        r#"""""#,
        r#"*"#,
        r#"***"#,
        r#"'"#,
        r#"''"#,
        r#"OR"#,
        r#"AND"#,
        r#"NOT"#,
        r#"AND OR NOT"#,
        r#"( )"#,
        r#"((()))"#,
        r#"^"#,
        r#":"#,
        r#"{ }"#,
        r#"%"#,
        r#"%%"#,
        r#";"#,
        r#"\*; DROP TABLE transactions; --"#,
        r#"catppuccin*"#,
        r#"waybar AND"#,
        r#"NOT theme"#,
        r#"///"#,
    ];

    for query in adversarial_queries {
        let output = env.run(&["search", query]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "FTS5 query '{query}' crashed CLI process"
        );
    }
}

#[test]
fn test_tier2_search_limit_boundaries() {
    let env = TestEnvironment::new("t2-search-limit");

    // Limit 0: Valid query, displays header without package rows
    let out_zero = env.run(&["search", "waybar", "--limit", "0"]);
    assert_eq!(out_zero.status.code(), Some(0));

    // Limit 1: Returns at most 1 package row
    let out_one = env.run(&["search", "", "--limit", "1"]);
    assert_eq!(out_one.status.code(), Some(0));
    let stdout_one = String::from_utf8_lossy(&out_one.stdout);
    assert!(stdout_one.contains("com.catppuccin.waybar") || stdout_one.contains("com.catppuccin.hyprland"));

    // Limit 1000: Extreme upper bound without integer overflow
    let out_high = env.run(&["search", "", "--limit", "1000"]);
    assert_eq!(out_high.status.code(), Some(0));
}

// ============================================================================
// 2. Non-Existent Resources & Missing Targets
// ============================================================================

#[test]
fn test_tier2_info_nonexistent_package_exit_code_1() {
    let env = TestEnvironment::new("t2-info-nonexist");
    let output = env.run(&["info", "com.nonexistent.package.foo"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("woderrise:"));
    assert!(stderr.contains("package not found"));
}

#[test]
fn test_tier2_diff_nonexistent_package_exit_code_1() {
    let env = TestEnvironment::new("t2-diff-nonexist");
    let output = env.run(&["diff", "com.nonexistent.package.bar"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("woderrise:"));
    assert!(stderr.contains("package not found"));
}

#[test]
fn test_tier2_rollback_nonexistent_transaction_exit_code_1() {
    let env = TestEnvironment::new("t2-rb-nonexist");
    let output = env.run(&["rollback", "txn-does-not-exist-99999"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("woderrise:"));
}

// ============================================================================
// 3. Recovery Selectivity & Clean State No-Op
// ============================================================================

#[test]
fn test_tier2_recover_clean_state_noop() {
    let env = TestEnvironment::new("t2-rec-clean");
    let output = env.run(&["recover"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No interrupted transactions found. Journal and filesystem are clean."));
}

#[test]
fn test_tier2_recovery_multi_state_selectivity() {
    let env = TestEnvironment::new("t2-rec-multistate");
    // Seed DB schema
    let _ = env.run(&["search"]);
    let conn = env.open_db();

    // Insert 6 transactions across all lifecycle states
    let txns = [
        ("txn-s-committed", "COMMITTED"),
        ("txn-s-rolledback", "ROLLED_BACK"),
        ("txn-s-planned", "PLANNED"),
        ("txn-s-prepared", "PREPARED"),
        ("txn-s-executing", "EXECUTING"),
        ("txn-s-failed", "FAILED"),
    ];

    for (id, status) in txns {
        conn.execute(
            "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator)
             VALUES (?1, 'com.catppuccin.waybar', '1.0.0', 'INSTALL', ?2, 1000, 'test_runner')",
            rusqlite::params![id, status],
        )
        .unwrap();
    }

    let output = env.run(&["recover"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Successfully recovered 4 transaction(s)"));

    // Verify invariants:
    // COMMITTED and ROLLED_BACK must remain strictly untouched
    let s_comm: String = conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = 'txn-s-committed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(s_comm, "COMMITTED");

    let s_rb: String = conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = 'txn-s-rolledback'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(s_rb, "ROLLED_BACK");

    // PLANNED, PREPARED, EXECUTING, FAILED must all transition to ABORTED_RECOVERED
    for id in ["txn-s-planned", "txn-s-prepared", "txn-s-executing", "txn-s-failed"] {
        let st: String = conn
            .query_row(
                "SELECT status FROM transactions WHERE txn_id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(st, "ABORTED_RECOVERED", "Transaction {id} was not recovered");
    }
}

// ============================================================================
// 4. Path Traversal & Unwriteable Filesystem Boundaries
// ============================================================================

#[test]
fn test_tier2_path_traversal_rejection() {
    let env = TestEnvironment::new("t2-path-traversal");
    let bogus_manifest = env.home.join("bogus_manifest.yaml");
    fs::write(
        &bogus_manifest,
        r#"
schema_version: '1.0.0'
package:
  id: com.evil.traversal
  name: Evil Traversal
  version: '1.0.0'
  type: theme
  category: bar_themes_modules
  license: MIT
  description: Path traversal attempt
artifacts: []
install:
  files:
    - source: /dev/null
      target: ../../../etc/evil.conf
"#,
    )
    .unwrap();

    let _lint_out = env.run(&["lint", bogus_manifest.to_str().unwrap()]);
    // Linter parses YAML syntax; when installing or computing diff, engine must reject
    let diff_out = env.run(&["diff", bogus_manifest.to_str().unwrap()]);
    assert_ne!(diff_out.status.code(), Some(0));
}

#[test]
fn test_tier2_unwriteable_target_permission_denied() {
    let env = TestEnvironment::new("t2-unwriteable");
    let waybar_dir = env.home.join(".config/waybar");
    fs::create_dir_all(&waybar_dir).unwrap();

    // Make ~/.config/waybar read-only (0o555 = read/execute, no write)
    fs::set_permissions(&waybar_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let output = env.run(&["install", REAL_CATPPUCCIN_DIR]);

    // Restore write permissions before dropping guard so cleanup succeeds
    let _ = fs::set_permissions(&waybar_dir, fs::Permissions::from_mode(0o755));

    // Must fail safely with exit code 1 and not panic
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("woderrise:"));
}

// ============================================================================
// 5. Standard Exit Code Matrix
// ============================================================================

#[test]
fn test_tier2_cli_standard_exit_codes_matrix() {
    let env = TestEnvironment::new("t2-exit-codes");

    // Exit code 0: Clean operations
    assert_eq!(env.run(&["--help"]).status.code(), Some(0));
    assert_eq!(env.run(&["search", "--help"]).status.code(), Some(0));

    // Exit code 1: Runtime failures
    assert_eq!(env.run(&["diff", "no_such_pkg_ever"]).status.code(), Some(1));
    assert_eq!(env.run(&["info", "no_such_pkg_ever"]).status.code(), Some(1));
    assert_eq!(env.run(&["rollback", "txn-no-such-id"]).status.code(), Some(1));

    // Exit code 2: Clap usage errors
    assert_eq!(env.run(&["diff"]).status.code(), Some(2));
    assert_eq!(env.run(&["info"]).status.code(), Some(2));
    assert_eq!(env.run(&["--unknown-flag-xyz"]).status.code(), Some(2));
    assert_eq!(env.run(&["totally_unknown_subcommand"]).status.code(), Some(2));
}
