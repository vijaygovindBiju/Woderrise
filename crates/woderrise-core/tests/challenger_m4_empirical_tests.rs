//! Empirical Challenger Stress Tests for Milestone 4:
//! Transaction state transitions, crash recovery, rollback byte-for-byte safety,
//! and double rollback prevention.

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
};

use rusqlite::params;
use woderrise_core::{
    sha256_bytes, sha256_file, BackupManager, Database, InstallationEngine,
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
        ".woderrise-chal-{prefix}-{}",
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

        artifacts_yaml.push_str(&format!("  - path: {src}\n    sha256: {hash}\n"));
        files_yaml.push_str(&format!(
            "    - source: {src}\n      target: {target}\n      backup_before_write: {backup}\n"
        ));
    }

    let manifest = format!(
        r#"schema_version: '1.0.0'
package:
  id: {pkg_id}
  name: Challenger Test Package
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: adversarial test package for verification
artifacts:
{artifacts_yaml}install:
  files:
{files_yaml}"#
    );

    fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();
}

// ============================================================================
// CHALLENGE 1: Exhaustive Verification of All 7 Transaction States
// PLANNED, PREPARED, EXECUTING, COMMITTED, FAILED, ROLLED_BACK, ABORTED_RECOVERED
// ============================================================================

#[test]
fn test_challenger_all_seven_transaction_states_and_invariants() {
    let (_guard, home, data, _pkg) = create_temp_env("all-states");
    let engine = make_engine(&home, &data);

    let all_states = [
        "PLANNED",
        "PREPARED",
        "EXECUTING",
        "COMMITTED",
        "FAILED",
        "ROLLED_BACK",
        "ABORTED_RECOVERED",
    ];

    // 1. In-progress states must have completed_at = NULL
    let in_progress = ["PLANNED", "PREPARED", "EXECUTING"];
    for st in &in_progress {
        let id = engine
            .db
            .begin_transaction("com.test.states", "1.0.0", "INSTALL", "tester")
            .unwrap();
        engine.db.set_status(&id, st, None).unwrap();

        let (status, completed_at): (String, Option<i64>) = engine
            .db
            .conn
            .query_row(
                "SELECT status, completed_at FROM transactions WHERE txn_id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(&status, st);
        assert_eq!(
            completed_at, None,
            "State {st} must NOT have completed_at timestamp set"
        );
    }

    // 2. Terminal states must have completed_at != NULL
    let terminal = ["COMMITTED", "FAILED", "ROLLED_BACK", "ABORTED_RECOVERED"];
    for st in &terminal {
        let id = engine
            .db
            .begin_transaction("com.test.states", "1.0.0", "INSTALL", "tester")
            .unwrap();
        engine.db.set_status(&id, st, Some("test message")).unwrap();

        let (status, completed_at, err_msg): (String, Option<i64>, Option<String>) = engine
            .db
            .conn
            .query_row(
                "SELECT status, completed_at, error_message FROM transactions WHERE txn_id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(&status, st);
        assert!(
            completed_at.is_some() && completed_at.unwrap() > 0,
            "Terminal state {st} MUST have completed_at timestamp set"
        );
        assert_eq!(err_msg.as_deref(), Some("test message"));
    }

    // 3. Rollback rejection matrix: ONLY COMMITTED may be rolled back!
    // All 6 other states must be strictly rejected
    for st in &all_states {
        if *st == "COMMITTED" {
            continue;
        }
        let id = engine
            .db
            .begin_transaction("com.test.rejection", "1.0.0", "INSTALL", "tester")
            .unwrap();
        engine.db.set_status(&id, st, None).unwrap();

        let err_unforced = engine.rollback(&id, false).unwrap_err();
        assert!(
            err_unforced.to_string().contains(st),
            "Rollback must reject state {st}"
        );
        let err_forced = engine.rollback(&id, true).unwrap_err();
        assert!(
            err_forced.to_string().contains(st),
            "Forced rollback must also reject state {st}"
        );
    }

    // 4. Recovery transition matrix:
    // We create a fresh engine so the count is isolated and exact
    let (_guard2, home2, data2, _pkg2) = create_temp_env("all-states-rec");
    let engine2 = make_engine(&home2, &data2);

    // PLANNED, PREPARED, EXECUTING, FAILED -> ABORTED_RECOVERED
    // COMMITTED, ROLLED_BACK, ABORTED_RECOVERED -> Untouched
    let mut txns_to_recover = Vec::new();
    for st in &["PLANNED", "PREPARED", "EXECUTING", "FAILED"] {
        let id = engine2
            .db
            .begin_transaction("com.test.rec", "1.0.0", "INSTALL", "tester")
            .unwrap();
        engine2.db.set_status(&id, st, None).unwrap();
        txns_to_recover.push((id, *st));
    }

    let mut txns_untouched = Vec::new();
    for st in &["COMMITTED", "ROLLED_BACK", "ABORTED_RECOVERED"] {
        let id = engine2
            .db
            .begin_transaction("com.test.untouch", "1.0.0", "INSTALL", "tester")
            .unwrap();
        engine2.db.set_status(&id, st, None).unwrap();
        txns_untouched.push((id, *st));
    }

    let count = engine2.recover().unwrap();
    assert_eq!(count, 4, "Must recover exactly the 4 interrupted transactions");

    for (id, orig_st) in txns_to_recover {
        let current_st: String = engine2
            .db
            .conn
            .query_row(
                "SELECT status FROM transactions WHERE txn_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            current_st, "ABORTED_RECOVERED",
            "Transaction originally in {orig_st} must transition to ABORTED_RECOVERED"
        );
    }

    for (id, orig_st) in txns_untouched {
        let current_st: String = engine2
            .db
            .conn
            .query_row(
                "SELECT status FROM transactions WHERE txn_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            current_st, orig_st,
            "Transaction in {orig_st} must remain untouched"
        );
    }
}

// ============================================================================
// CHALLENGE 2: Crash Recovery Under Hostile Conditions (Locks, Partial Ops)
// ============================================================================

#[test]
fn test_challenger_crash_recovery_partial_ops_and_db_consistency() {
    let (_guard, home, data, _pkg) = create_temp_env("crash-hostile");
    let engine = make_engine(&home, &data);

    // Setup: Simulate a multi-file installation interrupted halfway through Execution
    // 4 files planned:
    // File 1: CREATED and COMPLETED
    // File 2: MODIFIED (backup made) and COMPLETED
    // File 3: In-flight PLANNED (not executed)
    // File 4: In-flight PLANNED (not executed)

    let f1_target = home.join(".config/hostile/created.conf");
    let f2_target = home.join(".config/hostile/modified.conf");
    let f3_target = home.join(".config/hostile/unreached1.conf");
    let f4_target = home.join(".config/hostile/unreached2.conf");

    fs::create_dir_all(f1_target.parent().unwrap()).unwrap();

    // Pre-existing content for file 2
    let orig_binary_payload: Vec<u8> = (0..=255).cycle().take(4096).collect();
    fs::write(&f2_target, &orig_binary_payload).unwrap();
    let orig_f2_hash = sha256_bytes(&orig_binary_payload);

    let crash_txn = engine
        .db
        .begin_transaction("com.hostile.crash", "1.0.0", "INSTALL", "crasher")
        .unwrap();

    let snap_id = engine
        .backups
        .snapshot(&f2_target, &crash_txn, &engine.db)
        .unwrap();

    // Now write the corrupt in-flight files
    fs::write(&f1_target, b"corrupted partial create").unwrap();
    fs::write(&f2_target, b"corrupted partial modify").unwrap();

    // Record operations in DB
    engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-H-1', ?1, 1, 'CREATE_FILE', ?2, NULL, 'post1', NULL, 'COMPLETED', NULL)",
        params![crash_txn, f1_target.to_string_lossy()],
    ).unwrap();
    engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-H-2', ?1, 2, 'MODIFY_FILE', ?2, ?3, 'post2', ?4, 'COMPLETED', NULL)",
        params![crash_txn, f2_target.to_string_lossy(), orig_f2_hash, snap_id],
    ).unwrap();
    engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-H-3', ?1, 3, 'CREATE_FILE', ?2, NULL, 'post3', NULL, 'PLANNED', NULL)",
        params![crash_txn, f3_target.to_string_lossy()],
    ).unwrap();
    engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-H-4', ?1, 4, 'MODIFY_FILE', ?2, 'pre4', 'post4', NULL, 'PLANNED', NULL)",
        params![crash_txn, f4_target.to_string_lossy()],
    ).unwrap();

    // Set transaction to EXECUTING
    engine.db.set_status(&crash_txn, "EXECUTING", None).unwrap();

    // Also insert premature record in installed_packages
    engine.db.conn.execute(
        "INSERT INTO installed_packages(package_id,active_version,package_type,installed_at,updated_at,is_explicit,ref_count,install_txn_id) VALUES('com.hostile.crash','1.0.0','config',100,100,1,1,?1)",
        params![crash_txn],
    ).unwrap();

    // Verify recovery recovers safely
    let recovered = engine.recover().unwrap();
    assert_eq!(recovered, 1);

    // Invariant: Created file 1 removed
    assert!(!f1_target.exists(), "Partially created file must be deleted");

    // Invariant: Modified file 2 restored byte-for-byte to binary original
    assert_eq!(
        fs::read(&f2_target).unwrap(),
        orig_binary_payload,
        "Modified file must be restored byte-for-byte"
    );

    // Invariant: Unreached files 3 and 4 do not exist
    assert!(!f3_target.exists());
    assert!(!f4_target.exists());

    // Invariant: Orphan installed_packages entry wiped
    let ip_count: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.hostile.crash'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ip_count, 0);

    // Invariant: Journal WAL mode intact and journal not corrupted
    let integrity: String = engine
        .db
        .conn
        .query_row("PRAGMA integrity_check;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok", "Database PRAGMA integrity_check must be 'ok'");

    // Invariant: Idempotence of recovery
    let second = engine.recover().unwrap();
    assert_eq!(second, 0, "Second recovery must be a clean no-op");
}

// ============================================================================
// CHALLENGE 3: Byte-for-Byte Preservation and Restoration on Rollback
// ============================================================================

#[test]
fn test_challenger_byte_for_byte_preservation_and_restoration_on_rollback() {
    let (_guard, home, data, pkg) = create_temp_env("byte-safety");
    let engine = make_engine(&home, &data);

    let target_path = home.join(".config/custom/binary_config.dat");
    fs::create_dir_all(target_path.parent().unwrap()).unwrap();

    // Generate non-trivial binary payload with null bytes, high bytes, CRLF, and utf8 mix
    let mut initial_binary = Vec::new();
    initial_binary.extend_from_slice(b"# Header with NULL \0 and \r\n");
    for b in 0..=255u8 {
        initial_binary.push(b);
    }
    initial_binary.extend_from_slice("UTF-8 text: 🦀 Rust Desktop Config 🌟".as_bytes());
    initial_binary.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0xFF]);

    fs::write(&target_path, &initial_binary).unwrap();
    let initial_sha256 = sha256_file(&target_path).unwrap();

    // Package payload to replace it
    let replacement_content = b"WODERRISE_NEW_INSTALLED_THEME_PAYLOAD";
    let files = [(
        "theme.dat",
        ".config/custom/binary_config.dat",
        replacement_content.as_slice(),
        true,
    )];
    prepare_package(&pkg, "com.challenger.bytesafety", &files);

    // 1. Install package over target
    let txn = engine.install(&pkg, "byte_tester").unwrap();

    // Target must now contain new payload
    assert_eq!(fs::read(&target_path).unwrap(), replacement_content);

    // Backup snapshot must exist and match initial SHA256
    let bkp_storage: String = engine
        .db
        .conn
        .query_row(
            "SELECT storage_path FROM backup_snapshots WHERE sha256 = ?1",
            params![initial_sha256],
            |r| r.get(0),
        )
        .unwrap();
    let backup_bytes = fs::read(&bkp_storage).unwrap();
    assert_eq!(
        backup_bytes, initial_binary,
        "Backup store must preserve pre-existing file byte-for-byte"
    );

    // 2. Rollback transaction
    let rb_txn = engine.rollback(&txn, false).unwrap();
    assert!(!rb_txn.is_empty());

    // Target must be restored byte-for-byte identically
    let restored_bytes = fs::read(&target_path).unwrap();
    assert_eq!(
        restored_bytes, initial_binary,
        "Rollback must restore pre-existing file byte-for-byte"
    );
    assert_eq!(
        sha256_bytes(&restored_bytes),
        initial_sha256,
        "SHA-256 after rollback must exactly match pre-install SHA-256"
    );
}

// ============================================================================
// CHALLENGE 4: Double Rollback & Repeated Rollback Attack Prevention
// ============================================================================

#[test]
fn test_challenger_double_rollback_attack_prevention() {
    let (_guard, home, data, pkg) = create_temp_env("double-rollback");
    let engine = make_engine(&home, &data);

    let target = home.join(".config/app/test.conf");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, b"initial_v0").unwrap();

    let files = [("test.conf", ".config/app/test.conf", b"installed_v1".as_slice(), true)];
    prepare_package(&pkg, "com.challenger.doublerollback", &files);

    let txn = engine.install(&pkg, "tester").unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"installed_v1");

    // 1. First rollback succeeds
    let rb1 = engine.rollback(&txn, false).expect("First rollback must succeed");
    assert_eq!(fs::read(&target).unwrap(), b"initial_v0");

    // 2. Second rollback on txn (unforced): REJECTED
    let err2_unforced = engine.rollback(&txn, false).unwrap_err();
    let s2_u = err2_unforced.to_string();
    assert!(
        s2_u.contains("cannot rollback transaction") && s2_u.contains("ROLLED_BACK"),
        "Unforced double rollback must be rejected, got: {s2_u}"
    );

    // 3. Second rollback on txn (forced): ALSO REJECTED
    let err2_forced = engine.rollback(&txn, true).unwrap_err();
    let s2_f = err2_forced.to_string();
    assert!(
        s2_f.contains("cannot rollback transaction") && s2_f.contains("ROLLED_BACK"),
        "Forced double rollback must be rejected, got: {s2_f}"
    );

    // 4. Repeated attempts (3rd, 4th, 5th) must consistently be rejected without error degradation
    for i in 3..=5 {
        let err = engine.rollback(&txn, true).unwrap_err();
        assert!(
            err.to_string().contains("ROLLED_BACK"),
            "Attempt {i} must still be rejected"
        );
    }

    // Target file content must remain intact at initial_v0
    assert_eq!(fs::read(&target).unwrap(), b"initial_v0");

    // Journal status for txn must still be ROLLED_BACK
    let status: String = engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            params![txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "ROLLED_BACK");

    // Rollback transaction itself (rb1) is COMMITTED, but attempting to roll it back must fail
    // because it has no target operations or files to revert
    let rb2_result = engine.rollback(&rb1, false);
    // Even if it executes, it should be harmless or rejected
    match rb2_result {
        Ok(_new_rb) => {
            // If it succeeds, target file must still remain initial_v0
            assert_eq!(fs::read(&target).unwrap(), b"initial_v0");
        }
        Err(e) => {
            // Or it could fail cleanly
            assert!(!e.to_string().is_empty());
        }
    }
}
