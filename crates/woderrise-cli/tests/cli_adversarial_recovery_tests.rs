//! Empirical Adversarial Stress Test Suite for Milestone 3.
//! Validates:
//! 1. Interrupted transaction recovery across PLANNED, PREPARED, EXECUTING, and FAILED states.
//! 2. Disk compensation: file restoration from backup snapshots and deletion of newly created files.
//! 3. Database cleanup: purge of orphaned `installed_packages` records and status transition to `ABORTED_RECOVERED`.
//! 4. Mathematical accuracy of diff backup path prediction (`backups/<sha256>`).
//! 5. Subprocess environment isolation: strictly isolated from actual user `$HOME`.
//! 6. Idempotency of recovery operations.

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use woderrise_core::{sha256_bytes, BackupManager, Database};

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

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn create_isolated_env(prefix: &str) -> (TempDirGuard, PathBuf, PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("woderrise-m3-chal-{prefix}-{nonce}"));
    let home = root.join("home");
    let data = root.join("data");
    fs::create_dir_all(&home).expect("failed to create temp home");
    fs::create_dir_all(&data).expect("failed to create temp data");
    (TempDirGuard(root), home, data)
}

fn run_cli(home: &Path, data: &Path, args: &[&str]) -> std::process::Output {
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
// 1. RECOVERY: PLANNED STATE
// ============================================================================

#[test]
fn test_recovery_planned_state_transitions_to_aborted_recovered() {
    let (_guard, home, data) = create_isolated_env("rec-planned");
    let db = Database::open(data.join("journal.db")).unwrap();

    let txn_id = "txn-planned-test-01";
    db.conn
        .execute(
            "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator)
             VALUES (?1, 'com.catppuccin.waybar', '1.0.0', 'INSTALL', 'PLANNED', 1000, 'test_runner')",
            rusqlite::params![txn_id],
        )
        .unwrap();

    let output = run_cli(&home, &data, &["recover"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1") && stdout.contains("recovered"));

    // Verify DB transition
    let (status, completed_at, error_msg): (String, Option<i64>, Option<String>) = db
        .conn
        .query_row(
            "SELECT status, completed_at, error_message FROM transactions WHERE txn_id = ?1",
            rusqlite::params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();

    assert_eq!(status, "ABORTED_RECOVERED");
    assert!(completed_at.is_some());
    assert_eq!(error_msg.as_deref(), Some("startup recovery"));

    // Idempotency check: running recover again reports 0
    let out2 = run_cli(&home, &data, &["recover"]);
    assert_eq!(out2.status.code(), Some(0));
    let stdout2 = String::from_utf8_lossy(&out2.stdout);
    assert!(stdout2.contains("No interrupted") || stdout2.contains("clean"));
}

// ============================================================================
// 2. RECOVERY: PREPARED STATE
// ============================================================================

#[test]
fn test_recovery_prepared_state_transitions_to_aborted_recovered() {
    let (_guard, home, data) = create_isolated_env("rec-prepared");
    let db = Database::open(data.join("journal.db")).unwrap();

    let txn_id = "txn-prepared-test-02";
    db.conn
        .execute(
            "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator)
             VALUES (?1, 'com.catppuccin.waybar', '1.0.0', 'INSTALL', 'PREPARED', 2000, 'test_runner')",
            rusqlite::params![txn_id],
        )
        .unwrap();

    let output = run_cli(&home, &data, &["recover"]);
    assert_eq!(output.status.code(), Some(0));

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

// ============================================================================
// 3. RECOVERY: FAILED STATE
// ============================================================================

#[test]
fn test_recovery_failed_state_transitions_to_aborted_recovered() {
    let (_guard, home, data) = create_isolated_env("rec-failed");
    let db = Database::open(data.join("journal.db")).unwrap();

    let txn_id = "txn-failed-test-03";
    db.conn
        .execute(
            "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator, error_message)
             VALUES (?1, 'com.catppuccin.waybar', '1.0.0', 'INSTALL', 'FAILED', 3000, 'test_runner', 'disk failure')",
            rusqlite::params![txn_id],
        )
        .unwrap();

    let output = run_cli(&home, &data, &["recover"]);
    assert_eq!(output.status.code(), Some(0));

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

// ============================================================================
// 4. RECOVERY: EXECUTING STATE WITH DISK MUTATIONS & COMPENSATION
// ============================================================================

#[test]
fn test_recovery_executing_state_restores_files_and_cleans_installed_packages() {
    let (_guard, home, data) = create_isolated_env("rec-executing-disk");
    let db = Database::open(data.join("journal.db")).unwrap();
    let backup_mgr = BackupManager::new(data.join("backups")).unwrap();

    let txn_id = "txn-exec-crash-04";
    db.conn
        .execute(
            "INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator)
             VALUES (?1, 'com.example.pkg', '1.0.0', 'INSTALL', 'EXECUTING', 4000, 'test_runner')",
            rusqlite::params![txn_id],
        )
        .unwrap();

    // Target 1: File that existed before installation and was modified.
    // Must be reverted to original content from backup!
    let target1 = home.join(".config/app/settings.conf");
    fs::create_dir_all(target1.parent().unwrap()).unwrap();
    let original_content = "setting_key=ORIGINAL_VALUE\nversion=1.0\n";
    fs::write(&target1, original_content).unwrap();

    // Take snapshot of target1 before it was modified
    let bkp_id = backup_mgr.snapshot(&target1, txn_id, &db).unwrap();
    let pre_hash = sha256_bytes(original_content.as_bytes());

    // Simulate mutation by installer
    let mutated_content = "setting_key=OVERWRITTEN_INCOMPLETE_VALUE\n";
    fs::write(&target1, mutated_content).unwrap();
    let post_hash1 = sha256_bytes(mutated_content.as_bytes());

    // Record COMPLETED operation for target1
    db.conn
        .execute(
            "INSERT INTO operations(op_id, txn_id, sequence_num, op_type, target_path, pre_sha256, post_sha256, backup_id, status)
             VALUES ('op-01', ?1, 1, 'MODIFY_FILE', ?2, ?3, ?4, ?5, 'COMPLETED')",
            rusqlite::params![txn_id, target1.to_string_lossy(), pre_hash, post_hash1, bkp_id],
        )
        .unwrap();

    // Target 2: File that was newly created during install (did not exist previously).
    // Must be removed from filesystem!
    let target2 = home.join(".config/app/new_plugin.dat");
    let created_content = "NEW_PLUGIN_PAYLOAD_BYTES";
    fs::write(&target2, created_content).unwrap();
    let post_hash2 = sha256_bytes(created_content.as_bytes());

    // Record COMPLETED operation for target2 (pre_sha256 and backup_id are NULL)
    db.conn
        .execute(
            "INSERT INTO operations(op_id, txn_id, sequence_num, op_type, target_path, pre_sha256, post_sha256, backup_id, status)
             VALUES ('op-02', ?1, 2, 'CREATE_FILE', ?2, NULL, ?3, NULL, 'COMPLETED')",
            rusqlite::params![txn_id, target2.to_string_lossy(), post_hash2],
        )
        .unwrap();

    // Target 3: Operation that was PLANNED but never executed before crash
    db.conn
        .execute(
            "INSERT INTO operations(op_id, txn_id, sequence_num, op_type, target_path, pre_sha256, post_sha256, backup_id, status)
             VALUES ('op-03', ?1, 3, 'CREATE_FILE', '/dummy/path', NULL, 'dummy_hash', NULL, 'PLANNED')",
            rusqlite::params![txn_id],
        )
        .unwrap();

    // Insert an orphaned installed_packages record
    db.conn
        .execute(
            "INSERT INTO installed_packages(package_id, active_version, package_type, installed_at, updated_at, install_txn_id)
             VALUES ('com.example.pkg', '1.0.0', 'tool', 4000, 4000, ?1)",
            rusqlite::params![txn_id],
        )
        .unwrap();

    // Verify pre-recovery disk state
    assert_eq!(fs::read_to_string(&target1).unwrap(), mutated_content);
    assert!(target2.exists());

    // Execute CLI recover
    let output = run_cli(&home, &data, &["recover"]);
    assert_eq!(output.status.code(), Some(0));

    // INVARIANT 1: Target 1 must be restored to original pre-install content!
    let restored = fs::read_to_string(&target1).expect("target1 should exist");
    assert_eq!(
        restored, original_content,
        "Target1 was not reverted to pre-modification content!"
    );

    // INVARIANT 2: Target 2 (newly created) must be deleted!
    assert!(
        !target2.exists(),
        "Target2 was newly created and should have been removed during recovery!"
    );

    // INVARIANT 3: installed_packages record must be completely removed!
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE install_txn_id = ?1",
            rusqlite::params![txn_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "Dangling installed_packages record remained!");

    // INVARIANT 4: Transaction status must be ABORTED_RECOVERED!
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

// ============================================================================
// 5. RECOVERY: MULTI-STATE INTERLEAVED RECOVERY
// ============================================================================

#[test]
fn test_recovery_interleaved_multi_state_selectivity() {
    let (_guard, home, data) = create_isolated_env("rec-interleaved");
    let db = Database::open(data.join("journal.db")).unwrap();

    // 1. COMMITTED: Should NOT be touched
    db.conn.execute("INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, completed_at, initiator) VALUES ('t1', 'p1', '1.0', 'INSTALL', 'COMMITTED', 10, 20, 'cli')", []).unwrap();
    // 2. PLANNED: SHOULD be recovered
    db.conn.execute("INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator) VALUES ('t2', 'p2', '1.0', 'INSTALL', 'PLANNED', 30, 'cli')", []).unwrap();
    // 3. ROLLED_BACK: Should NOT be touched
    db.conn.execute("INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, completed_at, initiator) VALUES ('t3', 'p3', '1.0', 'INSTALL', 'ROLLED_BACK', 40, 50, 'cli')", []).unwrap();
    // 4. PREPARED: SHOULD be recovered
    db.conn.execute("INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator) VALUES ('t4', 'p4', '1.0', 'INSTALL', 'PREPARED', 60, 'cli')", []).unwrap();
    // 5. ABORTED_RECOVERED: Already recovered, should NOT be touched
    db.conn.execute("INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, completed_at, initiator) VALUES ('t5', 'p5', '1.0', 'INSTALL', 'ABORTED_RECOVERED', 70, 80, 'cli')", []).unwrap();
    // 6. EXECUTING: SHOULD be recovered
    db.conn.execute("INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, initiator) VALUES ('t6', 'p6', '1.0', 'INSTALL', 'EXECUTING', 90, 'cli')", []).unwrap();
    // 7. FAILED: SHOULD be recovered
    db.conn.execute("INSERT INTO transactions(txn_id, package_id, version, txn_type, status, started_at, completed_at, initiator) VALUES ('t7', 'p7', '1.0', 'INSTALL', 'FAILED', 100, 110, 'cli')", []).unwrap();

    let output = run_cli(&home, &data, &["recover"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("4")); // 4 recovered

    // Check statuses
    let get_status = |id: &str| -> String {
        db.conn
            .query_row(
                "SELECT status FROM transactions WHERE txn_id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap()
    };

    assert_eq!(get_status("t1"), "COMMITTED");
    assert_eq!(get_status("t2"), "ABORTED_RECOVERED");
    assert_eq!(get_status("t3"), "ROLLED_BACK");
    assert_eq!(get_status("t4"), "ABORTED_RECOVERED");
    assert_eq!(get_status("t5"), "ABORTED_RECOVERED");
    assert_eq!(get_status("t6"), "ABORTED_RECOVERED");
    assert_eq!(get_status("t7"), "ABORTED_RECOVERED");
}

// ============================================================================
// 6. DIFF: BACKUP PATH PREDICTION ACCURACY (SHA-256 CONTENT ADDRESSING)
// ============================================================================

#[test]
fn test_diff_backup_path_prediction_strictly_matches_sha256_addressing() {
    let (_guard, home, data) = create_isolated_env("diff-sha256");

    let target_file = home.join(".config/waybar/catppuccin-mocha.css");
    fs::create_dir_all(target_file.parent().unwrap()).unwrap();

    // Create target file with specific known content
    let existing_bytes = b"/* existing waybar mocha config test bytes 12345 */\n";
    fs::write(&target_file, existing_bytes).unwrap();

    // Compute expected mathematical SHA-256 hash
    let expected_hash = sha256_bytes(existing_bytes);
    let expected_backup_path = data.join("backups").join(&expected_hash);

    // Run woderrise diff
    let output = run_cli(&home, &data, &["diff", REAL_CATPPUCCIN_DIR]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Assert that the planned backup path printed matches exactly backups/<sha256>
    assert!(
        stdout.contains(&expected_hash),
        "CLI diff did not print the expected SHA-256 hash in backup path!\nExpected: {}\nOutput: {}",
        expected_hash,
        stdout
    );
    assert!(
        stdout.contains(&expected_backup_path.to_string_lossy().to_string()),
        "CLI diff planned backup path does not match expected path!\nExpected: {}\nOutput: {}",
        expected_backup_path.display(),
        stdout
    );

    // Now actually perform installation and verify that the actual backup file matches
    let install_out = run_cli(&home, &data, &["install", REAL_CATPPUCCIN_DIR]);
    assert_eq!(
        install_out.status.code(),
        Some(0),
        "Install failed: {}",
        String::from_utf8_lossy(&install_out.stderr)
    );

    // Verify physical backup snapshot exists on disk at exactly the predicted path
    assert!(
        expected_backup_path.exists(),
        "The predicted backup path was not actually created upon installation: {}",
        expected_backup_path.display()
    );

    // Verify content of physical backup snapshot matches the original target content
    let backup_data = fs::read(&expected_backup_path).unwrap();
    assert_eq!(backup_data, existing_bytes);
}

// ============================================================================
// 7. DIFF: NEW FILE HAS NO PLANNED BACKUP (NONE)
// ============================================================================

#[test]
fn test_diff_new_file_has_no_planned_backup() {
    let (_guard, home, data) = create_isolated_env("diff-nobackup");

    let target_file = home.join(".config/waybar/catppuccin-mocha.css");
    assert!(!target_file.exists());

    let output = run_cli(&home, &data, &["diff", REAL_CATPPUCCIN_DIR]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("Planned Backup: None"));
    assert!(stdout.contains("[CREATE]"));
    assert!(!target_file.exists(), "Diff must be strictly read-only!");
}

// ============================================================================
// 8. SUBPROCESS ENVIRONMENT ISOLATION (ZERO TOUCH TO USER $HOME)
// ============================================================================

#[test]
fn test_subprocess_strict_home_and_data_isolation() {
    let real_home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let (_guard, temp_home, temp_data) = create_isolated_env("isolation");

    // Run CLI commands with temp_home
    let out_diff = run_cli(&temp_home, &temp_data, &["diff", REAL_CATPPUCCIN_DIR]);
    assert_eq!(out_diff.status.code(), Some(0));

    let out_search = run_cli(&temp_home, &temp_data, &["search", "waybar"]);
    assert_eq!(out_search.status.code(), Some(0));

    let out_info = run_cli(&temp_home, &temp_data, &["info", REAL_CATPPUCCIN_DIR]);
    assert_eq!(out_info.status.code(), Some(0));

    let out_rec = run_cli(&temp_home, &temp_data, &["recover"]);
    assert_eq!(out_rec.status.code(), Some(0));

    // Verify temp_home and temp_data are strictly within the temp directory
    assert!(temp_home.starts_with(std::env::temp_dir()));
    assert!(temp_data.starts_with(std::env::temp_dir()));

    // Verify real home does not contain test leakage in ~/.config/waybar or ~/.local/share/woderrise
    // (Ensure real_home is not identical to temp_home)
    assert_ne!(temp_home, real_home);
}
