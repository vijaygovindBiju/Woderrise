//! Integration Test Suite for `woderrise` CLI.
//! Validates:
//! 1. Subcommands: `diff`, `search`, `info`, `recover`.
//! 2. Standard exit codes: 0 (Success), 1 (Runtime failure), 2 (Clap usage error).
//! 3. Standard error output prefix: `woderrise: <error>`.
//! 4. Edge cases: Empty search, missing package, uncommitted files intact, zero mocks.

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use woderrise_core::{catalog, Database};

/// RAII Temporary Directory Guard ensuring automatic cleanup on drop.
struct TempDirGuard(PathBuf);
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Deref for TempDirGuard {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

/// Computes workspace root directory from CARGO_MANIFEST_DIR.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Creates an isolated temporary environment with home, data, and backup directories.
fn create_test_env(prefix: &str) -> (TempDirGuard, PathBuf, PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("woderrise-cli-test-{prefix}-{nonce}"));
    let home = root.join("home");
    let data = root.join("data");
    fs::create_dir_all(&home).expect("failed to create temp home");
    fs::create_dir_all(&data).expect("failed to create temp data");
    (TempDirGuard(root), home, data)
}

/// Helper to build and run the `woderrise` binary with isolated environment.
fn run_woderrise(home: &Path, data: &Path, args: &[&str]) -> std::process::Output {
    let bin_path = env!("CARGO_BIN_EXE_woderrise");
    let ws = workspace_root();
    let reg_dir = ws.join("registry/packages");
    Command::new(bin_path)
        .current_dir(&ws)
        .env("HOME", home)
        .env("WODERRISE_DATA_DIR", data)
        .env("WODERRISE_REGISTRY_DIR", &reg_dir)
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .expect("failed to execute woderrise binary")
}

const REAL_CATPPUCCIN_DIR: &str = "registry/packages/com.catppuccin.waybar/1.0.0";

// ============================================================================
// 1. DIFF SUBCOMMAND TESTS
// ============================================================================

#[test]
fn test_cli_diff_valid_package_clean_install_create_action() {
    let (_guard, home, data) = create_test_env("diff-create");
    let target_file = home.join(".config/waybar/catppuccin-mocha.css");
    assert!(!target_file.exists());

    let output = run_woderrise(&home, &data, &["diff", REAL_CATPPUCCIN_DIR]);
    assert!(
        output.status.success(),
        "diff command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("CREATE") || stdout.contains("Create") || stdout.contains("[CREATE]"));
    assert!(stdout.contains("catppuccin-mocha.css"));
    assert!(stdout.contains("+"));

    // INVARIANT: diff must be strictly read-only; target file must not be created!
    assert!(!target_file.exists(), "diff must not create target file!");
}

#[test]
fn test_cli_diff_valid_package_existing_file_modify_action() {
    let (_guard, home, data) = create_test_env("diff-modify");
    let target_file = home.join(".config/waybar/catppuccin-mocha.css");
    fs::create_dir_all(target_file.parent().unwrap()).unwrap();
    fs::write(&target_file, "/* old existing waybar styles */\n").unwrap();

    let output = run_woderrise(&home, &data, &["diff", REAL_CATPPUCCIN_DIR]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MODIFY") || stdout.contains("Modify") || stdout.contains("[MODIFY]"));
    assert!(stdout.contains("catppuccin-mocha.css"));
    assert!(stdout.contains("-/* old existing waybar styles */"));
    assert!(stdout.contains("+"));
    // Planned backup location indicated
    assert!(stdout.contains("backup") || stdout.contains("backups"));

    // INVARIANT: Target file remains untouched
    assert_eq!(
        fs::read_to_string(&target_file).unwrap(),
        "/* old existing waybar styles */\n"
    );
}

#[test]
fn test_cli_diff_summary_flag() {
    let (_guard, home, data) = create_test_env("diff-summary");
    let output = run_woderrise(&home, &data, &["diff", REAL_CATPPUCCIN_DIR, "--summary"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("CREATE") || stdout.contains("Create") || stdout.contains("1"));
}

#[test]
fn test_cli_diff_no_color_flag() {
    let (_guard, home, data) = create_test_env("diff-nocolor");
    let output = run_woderrise(&home, &data, &["diff", REAL_CATPPUCCIN_DIR, "--no-color"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\x1b[32m"));
    assert!(!stdout.contains("\x1b[31m"));
}

#[test]
fn test_cli_diff_nonexistent_package_exits_with_code_1() {
    let (_guard, home, data) = create_test_env("diff-nonexistent");
    let output = run_woderrise(&home, &data, &["diff", "/path/that/does/not/exist/at/all"]);
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("woderrise:"));
}

#[test]
fn test_cli_diff_missing_argument_exits_with_code_2() {
    let (_guard, home, data) = create_test_env("diff-missing-arg");
    let output = run_woderrise(&home, &data, &["diff"]);
    assert_eq!(output.status.code(), Some(2)); // Clap argument parsing failure

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage: woderrise diff") || stderr.contains("required"));
}

// ============================================================================
// 2. SEARCH SUBCOMMAND TESTS
// ============================================================================

#[test]
fn test_cli_search_matching_query() {
    let (_guard, home, data) = create_test_env("search-match");
    let ws = workspace_root();
    // Pre-populate database with genuine packages
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    let output = run_woderrise(&home, &data, &["search", "waybar"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("com.catppuccin.waybar"));
    assert!(stdout.contains("1.0.0"));
    assert!(stdout.contains("Catppuccin") || stdout.contains("Mocha"));
}

#[test]
fn test_cli_search_empty_query_lists_all_validated_packages() {
    let (_guard, home, data) = create_test_env("search-empty");
    let ws = workspace_root();
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    // Passing "" as search query
    let output = run_woderrise(&home, &data, &["search", ""]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("com.catppuccin.waybar"));

    // Running search without arguments
    let output_no_arg = run_woderrise(&home, &data, &["search"]);
    assert_eq!(output_no_arg.status.code(), Some(0));
    let stdout_no_arg = String::from_utf8_lossy(&output_no_arg.stdout);
    assert!(stdout_no_arg.contains("com.catppuccin.waybar"));
}

#[test]
fn test_cli_search_non_matching_query_exits_0_with_empty_results() {
    let (_guard, home, data) = create_test_env("search-nomatch");
    let ws = workspace_root();
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    let output = run_woderrise(&home, &data, &["search", "nonexistent_query_xyz_987"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("com.catppuccin.waybar"));
}

#[test]
fn test_cli_search_fts5_special_characters_no_crash() {
    let (_guard, home, data) = create_test_env("search-special");
    let ws = workspace_root();
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    for query in &["cat*", "\"waybar\"", "mocha OR waybar", "(*)", "^test", "''"] {
        let output = run_woderrise(&home, &data, &["search", query]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "FTS5 query failed on special char input: {query}"
        );
    }
}

#[test]
fn test_cli_search_limit_flag() {
    let (_guard, home, data) = create_test_env("search-limit");
    let ws = workspace_root();
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    let output = run_woderrise(&home, &data, &["search", "--limit", "1"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("com.catppuccin.waybar") || stdout.contains("com.catppuccin.hyprland"));
}

// ============================================================================
// 3. INFO SUBCOMMAND TESTS
// ============================================================================

#[test]
fn test_cli_info_valid_package_id_and_version() {
    let (_guard, home, data) = create_test_env("info-valid");
    let ws = workspace_root();
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    let output = run_woderrise(&home, &data, &["info", "com.catppuccin.waybar/1.0.0"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("com.catppuccin.waybar"));
    assert!(stdout.contains("1.0.0"));
    assert!(stdout.contains("MIT"));
    assert!(stdout.contains("Catppuccin contributors"));
    assert!(stdout.contains("https://github.com/catppuccin/waybar"));
    assert!(stdout.contains("wayland_only"));
    assert!(stdout.contains("#1e1e2e"));
    assert!(stdout.contains("#89b4fa"));
    assert!(stdout.contains("#cba6f7"));
    assert!(stdout.contains("#cdd6f4"));
    assert!(stdout.contains("catppuccin-mocha.css"));
}

#[test]
fn test_cli_info_package_id_without_version() {
    let (_guard, home, data) = create_test_env("info-id-only");
    let ws = workspace_root();
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    let output = run_woderrise(&home, &data, &["info", "com.catppuccin.waybar"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("com.catppuccin.waybar"));
    assert!(stdout.contains("1.0.0"));
    assert!(stdout.contains("MIT"));
}

#[test]
fn test_cli_info_local_directory_path() {
    let (_guard, home, data) = create_test_env("info-path");
    let output = run_woderrise(&home, &data, &["info", REAL_CATPPUCCIN_DIR]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("com.catppuccin.waybar"));
    assert!(stdout.contains("1.0.0"));
    assert!(stdout.contains("MIT"));
}

#[test]
fn test_cli_info_no_color_flag() {
    let (_guard, home, data) = create_test_env("info-nocolor");
    let output = run_woderrise(&home, &data, &["info", REAL_CATPPUCCIN_DIR, "--no-color"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\x1b[48;2;"));
    assert!(stdout.contains("#1e1e2e"));
}

#[test]
fn test_cli_info_nonexistent_package_exits_code_1() {
    let (_guard, home, data) = create_test_env("info-missing");
    let output = run_woderrise(&home, &data, &["info", "com.nonexistent.package/1.0.0"]);
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("woderrise:"));
}

// ============================================================================
// 4. RECOVER SUBCOMMAND TESTS
// ============================================================================

#[test]
fn test_cli_recover_interrupted_transaction_in_journal() {
    let (_guard, home, data) = create_test_env("recover-interrupted");
    let db = Database::open(data.join("journal.db")).unwrap();

    // Insert an interrupted PLANNED transaction
    let txn_id = "test-txn-interrupted-001";
    db.conn
        .execute(
            "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator)
             VALUES (?1, 'com.catppuccin.waybar', '1.0.0', 'INSTALL', 'PLANNED', 1000, 'test_runner')",
            rusqlite::params![txn_id],
        )
        .unwrap();

    let output = run_woderrise(&home, &data, &["recover"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1") || stdout.contains("Recovered") || stdout.contains("recovered"));

    // Verify database state: status must be ABORTED_RECOVERED!
    let status: String = db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            rusqlite::params![txn_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "ABORTED_RECOVERED");
}

#[test]
fn test_cli_recover_clean_state_noop() {
    let (_guard, home, data) = create_test_env("recover-clean");
    let db = Database::open(data.join("journal.db")).unwrap();

    // Insert a COMMITTED transaction
    db.conn
        .execute(
            "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, completed_at, initiator)
             VALUES ('txn-ok', 'com.catppuccin.waybar', '1.0.0', 'INSTALL', 'COMMITTED', 1000, 1050, 'test')",
            [],
        )
        .unwrap();

    let output = run_woderrise(&home, &data, &["recover"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0") || stdout.contains("clean") || stdout.contains("No"));

    // Verify transaction remained COMMITTED
    let status: String = db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = 'txn-ok'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "COMMITTED");
}

#[test]
fn test_cli_recover_executing_transaction_cleans_installed_packages() {
    let (_guard, home, data) = create_test_env("recover-executing");
    let db = Database::open(data.join("journal.db")).unwrap();

    let txn_id = "test-txn-executing-002";
    db.conn
        .execute(
            "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator)
             VALUES (?1, 'com.catppuccin.waybar', '1.0.0', 'INSTALL', 'EXECUTING', 1000, 'test_runner')",
            rusqlite::params![txn_id],
        )
        .unwrap();

    db.conn
        .execute(
            "INSERT INTO installed_packages(package_id, active_version, package_type, installed_at, updated_at, install_txn_id)
             VALUES ('com.catppuccin.waybar', '1.0.0', 'theme', 1000, 1000, ?1)",
            rusqlite::params![txn_id],
        )
        .unwrap();

    let output = run_woderrise(&home, &data, &["recover"]);
    assert_eq!(output.status.code(), Some(0));

    // Verify transaction status is ABORTED_RECOVERED
    let status: String = db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            rusqlite::params![txn_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "ABORTED_RECOVERED");

    // Verify installed_packages record was cleaned up
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE install_txn_id = ?1",
            rusqlite::params![txn_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

// ============================================================================
// 5. STANDARD EXIT CODES & UNCOMMITTED REPOSITORY INTEGRITY
// ============================================================================

#[test]
fn test_cli_standard_exit_codes_matrix() {
    let (_guard, home, data) = create_test_env("exit-codes");
    // Help -> 0
    let out = run_woderrise(&home, &data, &["--help"]);
    assert_eq!(out.status.code(), Some(0));

    // Unknown subcommand -> 2
    let out = run_woderrise(&home, &data, &["unknown-subcommand"]);
    assert_eq!(out.status.code(), Some(2));

    // Unknown option -> 2
    let out = run_woderrise(&home, &data, &["--invalid-option-xyz"]);
    assert_eq!(out.status.code(), Some(2));

    // Missing positional argument for info -> 2
    let out = run_woderrise(&home, &data, &["info"]);
    assert_eq!(out.status.code(), Some(2));

    // Missing positional argument for diff -> 2
    let out = run_woderrise(&home, &data, &["diff"]);
    assert_eq!(out.status.code(), Some(2));

    // Runtime error -> 1
    let out = run_woderrise(&home, &data, &["lint", "/path/to/missing.yaml"]);
    assert_eq!(out.status.code(), Some(1));
}
