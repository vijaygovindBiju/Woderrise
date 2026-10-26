//! Comprehensive Adversarial Stress Test Suite for Transaction & Rollback Engine.
//! Covers:
//! 1. Atomic two-pass rollback (zero partial restoration on conflict).
//! 2. Forced rollback overriding conflict and cleaning up `installed_packages`.
//! 3. Repeated rollback rejection (ROLLED_BACK transactions cannot be rolled back again).
//! 4. SQLite WAL journal tables consistency across all lifecycle phases.
//! 5. Progress observer monotonicity under multi-file load and concurrency.
//! 6. Crash recovery of interrupted transactions and partial operation compensation.

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use rusqlite::params;
use woderrise_core::{
    sha256_bytes, sha256_file, BackupManager, Database, InstallStage, InstallationEngine,
    ProgressCallback, WoderriseError,
};

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

fn create_temp_env(prefix: &str) -> (TempDirGuard, PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        ".woderrise-stress-{prefix}-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let home = root.join("home");
    let data = root.join("data");
    let pkg = root.join("pkg");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&pkg).unwrap();
    (TempDirGuard(root), home, data, pkg)
}

fn make_engine(home: &Path, data: &Path) -> InstallationEngine {
    let db = Database::open(data.join("journal.db")).unwrap();
    let backups = BackupManager::new(data.join("backups")).unwrap();
    InstallationEngine {
        db,
        backups,
        home: home.to_path_buf(),
    }
}

/// Helper to generate package payload and manifest with specified files.
/// `files`: slice of (relative_src, target_rel_path, content_bytes, backup_before_write)
fn prepare_package(pkg_dir: &Path, pkg_id: &str, files: &[(&str, &str, &[u8], bool)]) {
    let mut artifacts_yaml = String::new();
    let mut files_yaml = String::new();

    for (src, target, content, backup) in files {
        let src_path = pkg_dir.join(src);
        if let Some(parent) = src_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&src_path, content).unwrap();
        let hash = sha256_bytes(content);

        artifacts_yaml.push_str(&format!(
            "  - path: {src}\n    sha256: {hash}\n"
        ));
        files_yaml.push_str(&format!(
            "    - source: {src}\n      target: {target}\n      backup_before_write: {backup}\n"
        ));
    }

    let manifest = format!(
        r#"schema_version: '1.0.0'
package:
  id: {pkg_id}
  name: Stress Test Package
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: adversarial stress test package
artifacts:
{artifacts_yaml}install:
  files:
{files_yaml}"#
    );

    fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();
}

// ---------------------------------------------------------------------------
// Objective 2.1: Atomic Two-Pass Rollback (Zero Partial Restoration)
// ---------------------------------------------------------------------------
#[test]
fn test_stress_atomic_two_pass_rollback_zero_partial_restoration() {
    let (_guard, home, data, pkg) = create_temp_env("two-pass-rollback");
    let engine = make_engine(&home, &data);

    // Prepare 6 target files:
    // 3 existing files (to be modified with backups)
    let f1_target = home.join(".config/app/file1.conf");
    let f2_target = home.join(".config/app/file2.conf");
    let f3_target = home.join(".config/app/nested/file3.conf");
    fs::create_dir_all(f1_target.parent().unwrap()).unwrap();
    fs::create_dir_all(f3_target.parent().unwrap()).unwrap();

    let orig_f1 = b"original_content_file_1";
    let orig_f2 = b"original_content_file_2";
    let orig_f3 = b"original_content_file_3";
    fs::write(&f1_target, orig_f1).unwrap();
    fs::write(&f2_target, orig_f2).unwrap();
    fs::write(&f3_target, orig_f3).unwrap();

    // 3 new files (to be created by installation)
    let f4_target = home.join(".config/app/file4_new.conf");
    let f5_target = home.join(".config/app/file5_new.conf");
    let f6_target = home.join(".config/app/file6_new.conf");

    let new_f1 = b"installed_content_file_1";
    let new_f2 = b"installed_content_file_2";
    let new_f3 = b"installed_content_file_3";
    let new_f4 = b"installed_new_file_4";
    let new_f5 = b"installed_new_file_5";
    let new_f6 = b"installed_new_file_6";

    let files = [
        ("src1.conf", ".config/app/file1.conf", new_f1.as_slice(), true),
        ("src2.conf", ".config/app/file2.conf", new_f2.as_slice(), true),
        ("src3.conf", ".config/app/nested/file3.conf", new_f3.as_slice(), true),
        ("src4.conf", ".config/app/file4_new.conf", new_f4.as_slice(), false),
        ("src5.conf", ".config/app/file5_new.conf", new_f5.as_slice(), false),
        ("src6.conf", ".config/app/file6_new.conf", new_f6.as_slice(), false),
    ];
    prepare_package(&pkg, "com.stress.twopass", &files);

    let txn = engine.install(&pkg, "stress_tester").expect("Install must succeed");

    // Verify all 6 target files reflect installed content
    assert_eq!(fs::read(&f1_target).unwrap(), new_f1);
    assert_eq!(fs::read(&f2_target).unwrap(), new_f2);
    assert_eq!(fs::read(&f3_target).unwrap(), new_f3);
    assert_eq!(fs::read(&f4_target).unwrap(), new_f4);
    assert_eq!(fs::read(&f5_target).unwrap(), new_f5);
    assert_eq!(fs::read(&f6_target).unwrap(), new_f6);

    // Adversarial Tampering:
    // We tamper with file 3 (modified post-install).
    let tampered_content = b"USER_TAMPERED_CONTENT_FILE_3";
    fs::write(&f3_target, tampered_content).unwrap();

    // Pass 1 conflict check must catch this before mutating ANY file on disk
    let rollback_result = engine.rollback(&txn, false);
    match rollback_result {
        Err(WoderriseError::Conflict(path)) => {
            assert!(
                path.contains("file3.conf"),
                "Conflict must report tampered file path, got: {path}"
            );
        }
        other => panic!("Expected Conflict error, got: {:?}", other),
    }

    // CRITICAL EMPIRICAL VERIFICATION OF ZERO PARTIAL RESTORATION:
    // Files 1 and 2 (which appear earlier in sequence) must NOT have been restored!
    assert_eq!(
        fs::read(&f1_target).unwrap(),
        new_f1,
        "File 1 must remain installed (no partial restoration!)"
    );
    assert_eq!(
        fs::read(&f2_target).unwrap(),
        new_f2,
        "File 2 must remain installed (no partial restoration!)"
    );
    assert_eq!(
        fs::read(&f3_target).unwrap(),
        tampered_content,
        "File 3 must keep user tampered content"
    );
    // Files 4, 5, 6 (created files) must NOT have been deleted!
    assert_eq!(
        fs::read(&f4_target).unwrap(),
        new_f4,
        "File 4 must remain intact"
    );
    assert_eq!(
        fs::read(&f5_target).unwrap(),
        new_f5,
        "File 5 must remain intact"
    );
    assert_eq!(
        fs::read(&f6_target).unwrap(),
        new_f6,
        "File 6 must remain intact"
    );

    // Verify database state: installed_packages must still contain the package
    let count: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.stress.twopass'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "Package must still be recorded as installed");

    // Transaction status must remain COMMITTED (not ROLLED_BACK)
    let txn_status: String = engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            [&txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(txn_status, "COMMITTED");

    // No rollback transaction should exist
    let rb_count: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE txn_type = 'ROLLBACK'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rb_count, 0, "No rollback transaction should be created on conflict failure");
}

// ---------------------------------------------------------------------------
// Objective 2.2: Rollback Forced Mode Overrides Conflict and Cleans Up
// ---------------------------------------------------------------------------
#[test]
fn test_stress_rollback_forced_mode_overrides_conflict_and_cleans_up() {
    let (_guard, home, data, pkg) = create_temp_env("force-rollback");
    let engine = make_engine(&home, &data);

    let f1_target = home.join(".config/app/conf1.txt");
    let f2_target = home.join(".config/app/conf2.txt");
    fs::create_dir_all(f1_target.parent().unwrap()).unwrap();
    fs::write(&f1_target, b"original_conf1").unwrap();

    let files = [
        ("conf1.txt", ".config/app/conf1.txt", b"installed_conf1".as_slice(), true),
        ("conf2.txt", ".config/app/conf2.txt", b"installed_conf2".as_slice(), false),
    ];
    prepare_package(&pkg, "com.stress.force", &files);

    let txn = engine.install(&pkg, "tester").unwrap();

    // External mutation: user edits both files post-install
    fs::write(&f1_target, b"mutated_conf1").unwrap();
    fs::write(&f2_target, b"mutated_conf2").unwrap();

    // Non-forced rollback fails
    assert!(matches!(
        engine.rollback(&txn, false),
        Err(WoderriseError::Conflict(_))
    ));

    // Forced rollback: must succeed despite conflict!
    let rb_txn = engine.rollback(&txn, true).expect("Forced rollback must succeed");
    assert!(!rb_txn.is_empty());

    // File 1 (modified with backup) must be restored to original pre-install content
    assert_eq!(
        fs::read(&f1_target).unwrap(),
        b"original_conf1",
        "Target with backup must be restored to original"
    );

    // File 2 (created new) must be deleted from disk
    assert!(
        !f2_target.exists(),
        "Created file must be removed from disk upon rollback"
    );

    // installed_packages table must be completely cleared for this package
    let pkg_count: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.stress.force'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pkg_count, 0, "installed_packages must be cleaned up");

    // Target transaction must be ROLLED_BACK
    let orig_status: String = engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            [&txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orig_status, "ROLLED_BACK");

    // Rollback transaction must be COMMITTED with txn_type = 'ROLLBACK'
    let (rb_status, rb_type): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT status, txn_type FROM transactions WHERE txn_id = ?1",
            [&rb_txn],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(rb_status, "COMMITTED");
    assert_eq!(rb_type, "ROLLBACK");
}

// ---------------------------------------------------------------------------
// Objective 2.3: Repeated Rollback and Invalid State Rollback Rejected
// ---------------------------------------------------------------------------
#[test]
fn test_stress_repeated_rollback_rejected_and_non_committed_blocked() {
    let (_guard, home, data, pkg) = create_temp_env("repeated-rollback");
    let engine = make_engine(&home, &data);

    let files = [("file.txt", ".config/file.txt", b"content".as_slice(), false)];
    prepare_package(&pkg, "com.stress.repeat", &files);

    let txn = engine.install(&pkg, "tester").unwrap();

    // First rollback succeeds
    assert!(engine.rollback(&txn, false).is_ok());

    // Second rollback on already rolled back transaction MUST fail (with force=false)
    let err_unforced = engine.rollback(&txn, false).unwrap_err();
    let err_str = err_unforced.to_string();
    assert!(
        err_str.contains("cannot rollback transaction") && err_str.contains("ROLLED_BACK"),
        "Unforced repeat rollback must be rejected, got: {err_str}"
    );

    // Second rollback on already rolled back transaction MUST ALSO fail (even with force=true)
    let err_forced = engine.rollback(&txn, true).unwrap_err();
    let err_forced_str = err_forced.to_string();
    assert!(
        err_forced_str.contains("cannot rollback transaction") && err_forced_str.contains("ROLLED_BACK"),
        "Forced repeat rollback must also be rejected, got: {err_forced_str}"
    );

    // Rollback of non-existent transaction must fail
    let err_nonexist = engine.rollback("TXN-NONEXISTENT-UUID-1234", false).unwrap_err();
    assert!(err_nonexist.to_string().contains("not found"));

    // Rollback of transactions in non-COMMITTED states (PLANNED, PREPARED, EXECUTING, FAILED)
    let states = ["PLANNED", "PREPARED", "EXECUTING", "FAILED", "ABORTED_RECOVERED"];
    for st in states {
        let dummy_txn = engine
            .db
            .begin_transaction("com.dummy", "1.0.0", "INSTALL", "tester")
            .unwrap();
        engine.db.set_status(&dummy_txn, st, None).unwrap();

        let err = engine.rollback(&dummy_txn, false).unwrap_err();
        assert!(
            err.to_string().contains(st),
            "State {st} must be rejected for rollback"
        );
        let err_f = engine.rollback(&dummy_txn, true).unwrap_err();
        assert!(
            err_f.to_string().contains(st),
            "State {st} must be rejected for forced rollback too"
        );
    }
}

// ---------------------------------------------------------------------------
// Objective 2.4: Database State Consistency Across Lifecycle
// ---------------------------------------------------------------------------
#[test]
fn test_stress_database_state_consistency_wal_tables() {
    let (_guard, home, data, pkg) = create_temp_env("db-consistency");
    let engine = make_engine(&home, &data);

    // 1. Verify SQLite pragmas
    let journal_mode: String = engine
        .db
        .conn
        .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal", "Journal mode must be WAL");

    let fk: i32 = engine
        .db
        .conn
        .query_row("PRAGMA foreign_keys;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1, "Foreign keys must be enabled");

    // 2. Initial state audit: all tables empty
    let txn_count: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0)).unwrap();
    let op_count: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM operations", [], |r| r.get(0)).unwrap();
    let bkp_count: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM backup_snapshots", [], |r| r.get(0)).unwrap();
    let pkg_count: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0)).unwrap();
    assert_eq!(txn_count, 0);
    assert_eq!(op_count, 0);
    assert_eq!(bkp_count, 0);
    assert_eq!(pkg_count, 0);

    // 3. Install multi-file package with pre-existing target (creating 1 backup)
    let target1 = home.join(".config/db_test/file1.json");
    fs::create_dir_all(target1.parent().unwrap()).unwrap();
    fs::write(&target1, b"{\"initial\": true}").unwrap();
    let pre_hash = sha256_bytes(b"{\"initial\": true}");

    let files = [
        ("file1.json", ".config/db_test/file1.json", b"{\"updated\": true}".as_slice(), true),
        ("file2.json", ".config/db_test/file2.json", b"{\"created\": true}".as_slice(), false),
    ];
    prepare_package(&pkg, "com.stress.dbcheck", &files);

    let txn = engine.install(&pkg, "db_auditor").unwrap();

    // 4. Audit tables after INSTALL:
    // transactions table:
    let (t_id, pkg_id, ver, t_type, status, started, completed, initiator, error): (
        String, String, String, String, String, i64, Option<i64>, String, Option<String>,
    ) = engine
        .db
        .conn
        .query_row(
            "SELECT txn_id, package_id, version, txn_type, status, started_at, completed_at, initiator, error_message FROM transactions WHERE txn_id = ?1",
            [&txn],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
        )
        .unwrap();
    assert_eq!(t_id, txn);
    assert_eq!(pkg_id, "com.stress.dbcheck");
    assert_eq!(ver, "1.0.0");
    assert_eq!(t_type, "INSTALL");
    assert_eq!(status, "COMMITTED");
    assert!(started > 0);
    assert!(completed.is_some() && completed.unwrap() >= started);
    assert_eq!(initiator, "db_auditor");
    assert!(error.is_none());

    // operations table:
    struct OpRecord {
        seq: i64,
        op_type: String,
        _target: String,
        pre_sha: Option<String>,
        _post_sha: String,
        backup_id: Option<String>,
        status: String,
    }
    let mut op_stmt = engine.db.conn.prepare(
        "SELECT sequence_num, op_type, target_path, pre_sha256, post_sha256, backup_id, status FROM operations WHERE txn_id = ?1 ORDER BY sequence_num ASC"
    ).unwrap();
    let ops: Vec<OpRecord> = op_stmt
        .query_map([&txn], |r| {
            Ok(OpRecord {
                seq: r.get(0)?,
                op_type: r.get(1)?,
                _target: r.get(2)?,
                pre_sha: r.get(3)?,
                _post_sha: r.get(4)?,
                backup_id: r.get(5)?,
                status: r.get(6)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(ops.len(), 2);
    // Op 1: MODIFY_FILE
    assert_eq!(ops[0].seq, 1);
    assert_eq!(ops[0].op_type, "MODIFY_FILE");
    assert_eq!(ops[0].pre_sha.as_deref(), Some(pre_hash.as_str()));
    assert!(ops[0].backup_id.is_some(), "Op 1 must reference a backup_id");
    assert_eq!(ops[0].status, "COMPLETED");

    // Op 2: CREATE_FILE
    assert_eq!(ops[1].seq, 2);
    assert_eq!(ops[1].op_type, "CREATE_FILE");
    assert_eq!(ops[1].pre_sha, None);
    assert_eq!(ops[1].backup_id, None);
    assert_eq!(ops[1].status, "COMPLETED");

    // backup_snapshots table:
    let b_id = ops[0].backup_id.as_ref().unwrap();
    let (snap_txn, orig_p, storage_p, snap_hash): (String, String, String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT txn_id, original_path, storage_path, sha256 FROM backup_snapshots WHERE backup_id = ?1",
            [b_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(snap_txn, txn);
    assert_eq!(orig_p, target1.to_string_lossy());
    assert_eq!(snap_hash, pre_hash);
    assert!(Path::new(&storage_p).exists());
    assert_eq!(sha256_file(&storage_p).unwrap(), pre_hash);

    // installed_packages table:
    let (inst_ver, inst_type, inst_txn): (String, String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, package_type, install_txn_id FROM installed_packages WHERE package_id = 'com.stress.dbcheck'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(inst_ver, "1.0.0");
    assert_eq!(inst_type, "config");
    assert_eq!(inst_txn, txn);

    // 5. Test Failed Rollback (tamper with file 1):
    fs::write(&target1, b"tampered").unwrap();
    let conflict_err = engine.rollback(&txn, false).unwrap_err();
    assert!(matches!(conflict_err, WoderriseError::Conflict(_)));

    // Audit DB state after failed rollback: must be 100% IDENTICAL to post-install state!
    let txn_count_post_fail: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0)).unwrap();
    assert_eq!(txn_count_post_fail, 1, "No transaction row added on failed rollback");
    let pkg_count_post_fail: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.stress.dbcheck'", [], |r| r.get(0)).unwrap();
    assert_eq!(pkg_count_post_fail, 1, "installed_packages still present");

    // 6. Test Successful Forced Rollback:
    let rb_txn = engine.rollback(&txn, true).unwrap();

    // Audit DB state after successful rollback:
    // transactions: 2 rows now (install + rollback)
    let total_txns: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0)).unwrap();
    assert_eq!(total_txns, 2);

    let (st_orig, st_rb): (String, String) = (
        engine.db.conn.query_row("SELECT status FROM transactions WHERE txn_id = ?1", [&txn], |r| r.get(0)).unwrap(),
        engine.db.conn.query_row("SELECT status FROM transactions WHERE txn_id = ?1", [&rb_txn], |r| r.get(0)).unwrap(),
    );
    assert_eq!(st_orig, "ROLLED_BACK");
    assert_eq!(st_rb, "COMMITTED");

    // installed_packages: exactly 0 rows
    let pkg_after_rb: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.stress.dbcheck'", [], |r| r.get(0)).unwrap();
    assert_eq!(pkg_after_rb, 0);

    // operations: 2 original rows preserved (audit history not truncated)
    let total_ops: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM operations", [], |r| r.get(0)).unwrap();
    assert_eq!(total_ops, 2);

    // backup_snapshots: 1 original row preserved
    let total_snaps: i64 = engine.db.conn.query_row("SELECT COUNT(*) FROM backup_snapshots", [], |r| r.get(0)).unwrap();
    assert_eq!(total_snaps, 1);

    // SQLite integrity check passes
    let integrity: String = engine.db.conn.query_row("PRAGMA integrity_check;", [], |r| r.get(0)).unwrap();
    assert_eq!(integrity, "ok");
}

// ---------------------------------------------------------------------------
// Objective 2.5: Progress Observer Under Multi-File Load & Concurrency
// ---------------------------------------------------------------------------
#[test]
fn test_stress_progress_observer_under_load_and_concurrency() {
    let (_guard, home, data, pkg) = create_temp_env("progress-stress");
    let engine = make_engine(&home, &data);

    // Create a 20-file manifest (10 existing with backup, 10 new)
    const FILE_COUNT: usize = 20;
    let mut files_spec: Vec<(String, String, Vec<u8>, bool)> = Vec::with_capacity(FILE_COUNT);

    for i in 0..FILE_COUNT {
        let is_existing = i < 10;
        let target_rel = format!(".config/stress/file_{i}.txt");
        let target_full = home.join(&target_rel);

        if is_existing {
            fs::create_dir_all(target_full.parent().unwrap()).unwrap();
            fs::write(&target_full, format!("original_content_{i}").as_bytes()).unwrap();
        }

        files_spec.push((
            format!("payload_{i}.txt"),
            target_rel,
            format!("installed_new_content_{i}").into_bytes(),
            is_existing,
        ));
    }

    let files_ref: Vec<(&str, &str, &[u8], bool)> = files_spec
        .iter()
        .map(|(s, t, c, b)| (s.as_str(), t.as_str(), c.as_slice(), *b))
        .collect();

    prepare_package(&pkg, "com.stress.load", &files_ref);

    // Progress collector with thread-safe recording
    let events = Arc::new(Mutex::new(Vec::<(InstallStage, String, f32)>::new()));
    let events_clone = Arc::clone(&events);

    let progress: ProgressCallback = Box::new(move |stage, msg, frac| {
        // Simulate TUI / worker rendering delay
        thread::sleep(Duration::from_millis(2));
        events_clone
            .lock()
            .unwrap()
            .push((stage, msg.to_string(), frac));
    });

    let txn = engine
        .install_with_progress(&pkg, "stress_worker", Some(progress))
        .expect("Multi-file install must succeed");
    assert!(!txn.is_empty());

    let recorded = events.lock().unwrap().clone();
    assert!(
        recorded.len() >= 45,
        "Expected >= 45 progress events for 20 files, got {}",
        recorded.len()
    );

    // Verify all 7 stages fired
    let stages: Vec<InstallStage> = recorded.iter().map(|(s, _, _)| *s).collect();
    let expected_stages = [
        InstallStage::Resolve,
        InstallStage::Validate,
        InstallStage::Plan,
        InstallStage::Backup,
        InstallStage::Execute,
        InstallStage::Verify,
        InstallStage::Commit,
    ];
    for expected in &expected_stages {
        assert!(
            stages.contains(expected),
            "Progress callback must fire stage: {:?}",
            expected
        );
    }

    // Verify stages occur in exact monotonic order
    let mut last_stage_idx = 0;
    for (stage, _, _) in &recorded {
        let current_stage_idx = expected_stages.iter().position(|s| s == stage).unwrap();
        assert!(
            current_stage_idx >= last_stage_idx,
            "Stage order regression: {:?} appeared after {:?}",
            stage,
            expected_stages[last_stage_idx]
        );
        last_stage_idx = current_stage_idx;
    }

    // Verify fractions are monotonically non-decreasing and end at 1.00
    let fractions: Vec<f32> = recorded.iter().map(|(_, _, f)| *f).collect();
    assert_eq!(fractions[0], 0.05);
    for window in fractions.windows(2) {
        assert!(
            window[0] <= window[1] + 1e-6,
            "Fractions not monotonic: {} > {}",
            window[0],
            window[1]
        );
    }
    assert_eq!(*fractions.last().unwrap(), 1.0);

    // High Concurrency Stress Test:
    // Spawn 6 concurrent worker threads each installing their own isolated package
    let mut handles = Vec::new();
    for thread_idx in 0..6 {
        let handle = thread::spawn(move || {
            let (_guard, home, data, pkg) = create_temp_env(&format!("thread-{thread_idx}"));
            let engine = make_engine(&home, &data);

            let files = [
                ("t_file1.txt", ".config/t1.txt", b"thread_data_1".as_slice(), false),
                ("t_file2.txt", ".config/t2.txt", b"thread_data_2".as_slice(), false),
            ];
            let pkg_id = format!("com.stress.thread.{thread_idx}");
            prepare_package(&pkg, &pkg_id, &files);

            let t_events = Arc::new(Mutex::new(Vec::new()));
            let t_events_c = Arc::clone(&t_events);

            let cb: ProgressCallback = Box::new(move |s, m, f| {
                thread::sleep(Duration::from_millis(1));
                t_events_c.lock().unwrap().push((s, m.to_string(), f));
            });

            let txn = engine
                .install_with_progress(&pkg, "concurrency_tester", Some(cb))
                .unwrap();
            let rec = t_events.lock().unwrap().clone();

            let frac: Vec<f32> = rec.iter().map(|(_, _, f)| *f).collect();
            for w in frac.windows(2) {
                assert!(w[0] <= w[1] + 1e-6);
            }
            assert_eq!(*frac.last().unwrap(), 1.0);

            // Rollback in the concurrent thread
            let rb = engine.rollback(&txn, false).unwrap();
            assert!(!rb.is_empty());
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("Concurrent install thread failed");
    }
}

// ---------------------------------------------------------------------------
// Objective 2.6: Interrupted Transaction Crash Recovery and Compensation
// ---------------------------------------------------------------------------
#[test]
fn test_stress_interrupted_transaction_crash_recovery_and_compensation() {
    let (_guard, home, data, _pkg) = create_temp_env("crash-recovery");
    let engine = make_engine(&home, &data);

    // Scenario 1: Interrupted transaction during EXECUTING state.
    // 2 files:
    // File 1 was created by installation (pre was None).
    // File 2 was modified by installation (pre was original hash, backup snapshot created).
    let f1_target = home.join(".config/crash/created.txt");
    let f2_target = home.join(".config/crash/modified.txt");
    fs::create_dir_all(f1_target.parent().unwrap()).unwrap();

    // Create File 2 original content and snapshot
    let orig_content = b"original_modified_content";
    fs::write(&f2_target, orig_content).unwrap();

    let crash_txn = engine
        .db
        .begin_transaction("com.stress.crash", "1.0.0", "INSTALL", "crash_sim")
        .unwrap();

    let snap_id = engine.backups.snapshot(&f2_target, &crash_txn, &engine.db).unwrap();

    // Now write the new installed files (as if the installer wrote them)
    fs::write(&f1_target, b"installed_file_1_created").unwrap();
    fs::write(&f2_target, b"installed_file_2_modified").unwrap();

    // Insert operations in the DB as COMPLETED
    engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-CRASH-1', ?1, 1, 'CREATE_FILE', ?2, NULL, 'post1', NULL, 'COMPLETED', NULL)",
        params![crash_txn, f1_target.to_string_lossy()],
    ).unwrap();
    engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-CRASH-2', ?1, 2, 'MODIFY_FILE', ?2, 'pre2', 'post2', ?3, 'COMPLETED', NULL)",
        params![crash_txn, f2_target.to_string_lossy(), snap_id],
    ).unwrap();

    // Set transaction status to EXECUTING (simulating mid-flight crash before COMMIT)
    engine.db.set_status(&crash_txn, "EXECUTING", None).unwrap();

    // Insert orphan row in installed_packages as can happen during crash races
    engine.db.conn.execute(
        "INSERT INTO installed_packages(package_id,active_version,package_type,installed_at,updated_at,is_explicit,ref_count,install_txn_id) VALUES(?1,'1.0.0','config',0,0,1,1,?2)",
        params!["com.stress.crash", crash_txn],
    ).unwrap();

    // Scenario 2: Another transaction left in PLANNED state
    let planned_txn = engine
        .db
        .begin_transaction("com.stress.planned", "2.0.0", "INSTALL", "crash_sim2")
        .unwrap();
    // (remains in PLANNED)

    // Execute Startup Recovery
    let recovered_count = engine.recover().expect("Crash recovery must succeed");
    assert_eq!(recovered_count, 2, "Must recover both EXECUTING and PLANNED transactions");

    // File 1 (created) must be removed
    assert!(!f1_target.exists(), "Partially created file must be removed by recovery");

    // File 2 (modified) must be restored from its backup snapshot
    assert_eq!(
        fs::read(&f2_target).unwrap(),
        orig_content,
        "Partially modified file must be restored from backup"
    );

    // Orphan row in installed_packages must be cleaned up
    let pkg_count: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.stress.crash'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pkg_count, 0, "Orphan installed_packages record must be removed");

    // Transaction statuses must be ABORTED_RECOVERED
    let st1: String = engine.db.conn.query_row("SELECT status FROM transactions WHERE txn_id = ?1", [&crash_txn], |r| r.get(0)).unwrap();
    let st2: String = engine.db.conn.query_row("SELECT status FROM transactions WHERE txn_id = ?1", [&planned_txn], |r| r.get(0)).unwrap();
    assert_eq!(st1, "ABORTED_RECOVERED");
    assert_eq!(st2, "ABORTED_RECOVERED");

    // Second recovery call should do nothing and return 0 (idempotent)
    let second_recover = engine.recover().unwrap();
    assert_eq!(second_recover, 0, "Repeated recovery must be a no-op");
}
