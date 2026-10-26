//! Adversarial Empirical Test Suite for Rollback Engine and Installed Packages Isolation.
//!
//! Evaluates:
//! 1. Multi-version upgrade chains (v1 -> v2 -> v3) and older historical rollback behavior.
//! 2. Multi-package isolation: rollback of package A must not mutate or delete package B.
//! 3. High-concurrency stress testing of parallel installations and rollbacks across threads.
//! 4. Startup crash recovery (`recover()`) when interrupted transactions exist alongside completed transactions for the same package.
//! 5. Error boundaries and defense-in-depth validation for transaction rollback states.

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, Barrier, Mutex},
    thread,
};

use rusqlite::params;
use woderrise_core::{
    sha256_bytes, sha256_file, BackupManager, Database, InstallationEngine, WoderriseError,
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
        ".woderrise-adv-{prefix}-{}",
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

fn prepare_versioned_package(
    pkg_dir: &Path,
    pkg_id: &str,
    version: &str,
    files: &[(&str, &str, &[u8], bool)],
) {
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
  name: Adversarial Test Package
  version: '{version}'
  type: config
  category: test
  license: MIT
  description: adversarial package for stress testing
artifacts:
{artifacts_yaml}install:
  files:
{files_yaml}"#
    );

    fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();
}

// ---------------------------------------------------------------------------
// Scenario 1: Multi-version Upgrade Chain (v1 -> v2 -> v3) & Historical Rollback
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_multi_version_upgrade_rollback_preserves_newest() {
    let (_guard, home, data, pkg_root) = create_temp_env("multiversion");
    let engine = make_engine(&home, &data);
    let pkg_id = "com.adversarial.chain";

    let v1_dir = pkg_root.join("v1");
    let v2_dir = pkg_root.join("v2");
    let v3_dir = pkg_root.join("v3");

    // v1.0.0: creates shared app.conf and v1_only.conf
    prepare_versioned_package(
        &v1_dir,
        pkg_id,
        "1.0.0",
        &[
            ("app.conf", ".config/adv/app.conf", b"v1_content", false),
            ("v1_only.conf", ".config/adv/v1_only.conf", b"v1_exclusive", false),
        ],
    );

    let txn_v1 = engine
        .install(&v1_dir, "tester")
        .expect("v1 install must succeed");

    // Verify v1 recorded
    let (v1_rec_ver, v1_rec_txn): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
            params![pkg_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(v1_rec_ver, "1.0.0");
    assert_eq!(v1_rec_txn, txn_v1);

    // v2.0.0: updates shared app.conf (with backup) and adds v2_only.conf
    prepare_versioned_package(
        &v2_dir,
        pkg_id,
        "2.0.0",
        &[
            ("app.conf", ".config/adv/app.conf", b"v2_content", true),
            ("v2_only.conf", ".config/adv/v2_only.conf", b"v2_exclusive", false),
        ],
    );

    let txn_v2 = engine
        .install(&v2_dir, "tester")
        .expect("v2 install must succeed");

    // Verify v2 recorded
    let (v2_rec_ver, v2_rec_txn): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
            params![pkg_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(v2_rec_ver, "2.0.0");
    assert_eq!(v2_rec_txn, txn_v2);

    // v3.0.0: updates shared app.conf (with backup) and adds v3_only.conf
    prepare_versioned_package(
        &v3_dir,
        pkg_id,
        "3.0.0",
        &[
            ("app.conf", ".config/adv/app.conf", b"v3_content", true),
            ("v3_only.conf", ".config/adv/v3_only.conf", b"v3_exclusive", false),
        ],
    );

    let txn_v3 = engine
        .install(&v3_dir, "tester")
        .expect("v3 install must succeed");

    // Verify v3 recorded
    let (v3_rec_ver, v3_rec_txn): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
            params![pkg_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(v3_rec_ver, "3.0.0");
    assert_eq!(v3_rec_txn, txn_v3);

    // Invariant 1: Unforced rollback of v1.0.0 must be BLOCKED due to conflict on app.conf
    let err_unforced = engine.rollback(&txn_v1, false);
    match err_unforced {
        Err(WoderriseError::Conflict(path)) => {
            assert!(path.contains("app.conf"), "Conflict must be detected on modified file");
        }
        other => panic!("Expected Conflict error on unforced rollback of v1, got: {:?}", other),
    }

    // Active package in installed_packages must remain completely unaffected after rejected rollback
    let post_fail_count: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = ?1 AND active_version = '3.0.0' AND install_txn_id = ?2",
            params![pkg_id, txn_v3],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(post_fail_count, 1);

    // Invariant 2: Forced rollback of historical txn_v1
    let rb_v1 = engine
        .rollback(&txn_v1, true)
        .expect("Forced rollback of historical v1 must succeed");
    assert!(!rb_v1.is_empty());

    // Verify txn_v1 status is ROLLED_BACK and rb_v1 is COMMITTED
    let status_v1: String = engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            params![txn_v1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status_v1, "ROLLED_BACK");

    let status_rb1: String = engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            params![rb_v1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status_rb1, "COMMITTED");

    // CORE REQUIREMENT: v3.0.0 is STILL the active package in installed_packages!
    let (still_v3_ver, still_v3_txn): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
            params![pkg_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("v3.0.0 record must NOT be deleted by historical v1.0.0 rollback");
    assert_eq!(still_v3_ver, "3.0.0");
    assert_eq!(still_v3_txn, txn_v3);

    // Invariant 3: Forced rollback of intermediate txn_v2
    let rb_v2 = engine
        .rollback(&txn_v2, true)
        .expect("Forced rollback of intermediate v2 must succeed");
    assert!(!rb_v2.is_empty());

    // v3.0.0 must STILL remain active in installed_packages
    let (still_v3_ver_after_v2, still_v3_txn_after_v2): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
            params![pkg_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("v3.0.0 record must NOT be deleted by historical v2.0.0 rollback");
    assert_eq!(still_v3_ver_after_v2, "3.0.0");
    assert_eq!(still_v3_txn_after_v2, txn_v3);

    // Invariant 4: Rollback of the currently active version txn_v3
    let rb_v3 = engine
        .rollback(&txn_v3, true)
        .expect("Rollback of active v3 must succeed");
    assert!(!rb_v3.is_empty());

    // installed_packages entry for this package must now be completely removed
    let final_pkg_count: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = ?1",
            params![pkg_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(final_pkg_count, 0, "installed_packages must be cleaned up when active txn is rolled back");
}

// ---------------------------------------------------------------------------
// Scenario 2: Multi-Package Isolation (Package A & Package B)
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_multi_package_isolation_rollback_one_preserves_other() {
    let (_guard, home, data, pkg_root) = create_temp_env("multipackage");
    let engine = make_engine(&home, &data);

    let pkg_a_dir = pkg_root.join("pkg_a");
    let pkg_b_dir = pkg_root.join("pkg_b");

    let pkg_a_id = "com.adversarial.pkg_a";
    let pkg_b_id = "com.adversarial.pkg_b";

    let a_content = b"configuration_for_package_a";
    let b_content = b"configuration_for_package_b";

    prepare_versioned_package(
        &pkg_a_dir,
        pkg_a_id,
        "1.0.0",
        &[("settings.json", ".config/pkg_a/settings.json", a_content, false)],
    );

    prepare_versioned_package(
        &pkg_b_dir,
        pkg_b_id,
        "2.5.0",
        &[("settings.json", ".config/pkg_b/settings.json", b_content, false)],
    );

    let txn_a = engine.install(&pkg_a_dir, "tester").expect("Install A must succeed");
    let txn_b = engine.install(&pkg_b_dir, "tester").expect("Install B must succeed");

    // Verify both packages recorded in database
    let count_total: i64 = engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_total, 2);

    let (b_ver_before, b_txn_before): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
            params![pkg_b_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(b_ver_before, "2.5.0");
    assert_eq!(b_txn_before, txn_b);

    // Rollback Package A
    let rb_a = engine.rollback(&txn_a, false).expect("Rollback of A must succeed");
    assert!(!rb_a.is_empty());

    // File system checks
    let path_a = home.join(".config/pkg_a/settings.json");
    let path_b = home.join(".config/pkg_b/settings.json");
    assert!(!path_a.exists(), "Package A file must be removed by rollback");
    assert!(path_b.exists(), "Package B file must remain intact");
    assert_eq!(fs::read(&path_b).unwrap(), b_content);

    // Database checks: Package A is DELETED, Package B is COMPLETELY UNTOUCHED
    let count_a: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = ?1",
            params![pkg_a_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_a, 0, "Package A must be deleted from installed_packages");

    let (b_ver_after, b_txn_after): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
            params![pkg_b_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("Package B must still exist in installed_packages");
    assert_eq!(b_ver_after, "2.5.0", "Package B version must be unchanged");
    assert_eq!(b_txn_after, txn_b, "Package B install_txn_id must be unchanged");

    let count_after_a_rb: i64 = engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_after_a_rb, 1, "Only Package B must remain in installed_packages");

    // Clean up Package B via rollback as well
    let rb_b = engine.rollback(&txn_b, false).expect("Rollback of B must succeed");
    assert!(!rb_b.is_empty());
    assert!(!path_b.exists(), "Package B file must be removed");

    let final_count: i64 = engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(final_count, 0, "All installed packages must be cleaned up");
}

// ---------------------------------------------------------------------------
// Scenario 3: Concurrently Trigger Multiple Installations and Rollbacks
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_concurrent_installations_and_rollbacks() {
    let (_guard, home, data, pkg_root) = create_temp_env("concurrency");
    let engine = Arc::new(Mutex::new(make_engine(&home, &data)));

    let num_threads = 6;
    let mut thread_pkg_dirs = Vec::new();

    // Prepare packages for 6 distinct threads
    for i in 0..num_threads {
        let pkg_id = format!("com.adversarial.conc_{i}");
        let pkg_dir = pkg_root.join(format!("pkg_{i}"));
        let target_file = format!(".config/conc/pkg_{i}.conf");
        let content = format!("content_for_thread_{i}").into_bytes();
        prepare_versioned_package(
            &pkg_dir,
            &pkg_id,
            "1.0.0",
            &[(
                "conf.txt",
                &target_file,
                content.as_slice(),
                false,
            )],
        );
        thread_pkg_dirs.push((pkg_id, pkg_dir));
    }

    // Phase 1: Parallel Installations
    let barrier_install = Arc::new(Barrier::new(num_threads));
    let mut handles = Vec::new();

    for (i, (pkg_id, pkg_dir)) in thread_pkg_dirs.clone().into_iter().enumerate() {
        let engine_clone = Arc::clone(&engine);
        let barrier_clone = Arc::clone(&barrier_install);

        let handle = thread::spawn(move || {
            barrier_clone.wait();
            let engine_guard = engine_clone.lock().unwrap();
            let txn = engine_guard
                .install(&pkg_dir, &format!("worker_{i}"))
                .expect("Concurrent install must succeed");
            (i, pkg_id, txn)
        });
        handles.push(handle);
    }

    let mut installed_records = Vec::new();
    for h in handles {
        installed_records.push(h.join().expect("Worker thread panicked"));
    }
    installed_records.sort_by_key(|(i, _, _)| *i);

    // Verify all 6 packages exist in installed_packages
    {
        let engine_guard = engine.lock().unwrap();
        let total_installed: i64 = engine_guard
            .db
            .conn
            .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total_installed, num_threads as i64);

        for (_, pkg_id, txn) in &installed_records {
            let active_txn: String = engine_guard
                .db
                .conn
                .query_row(
                    "SELECT install_txn_id FROM installed_packages WHERE package_id = ?1",
                    params![pkg_id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(&active_txn, txn);
        }
    }

    // Phase 2: Concurrent mixed operations:
    // Even threads (0, 2, 4) rollback their packages
    // Odd threads (1, 3, 5) upgrade their packages to v2.0.0
    let barrier_mixed = Arc::new(Barrier::new(num_threads));
    let mut mixed_handles = Vec::new();

    // Prepare upgrade packages for odd threads
    let mut upgrade_dirs = Vec::new();
    for i in 0..num_threads {
        if i % 2 == 1 {
            let pkg_id = format!("com.adversarial.conc_{i}");
            let up_dir = pkg_root.join(format!("pkg_up_{i}"));
            let target_file = format!(".config/conc/pkg_{i}.conf");
            let content = format!("upgraded_content_for_thread_{i}").into_bytes();
            prepare_versioned_package(
                &up_dir,
                &pkg_id,
                "2.0.0",
                &[(
                    "conf.txt",
                    &target_file,
                    content.as_slice(),
                    true,
                )],
            );
            upgrade_dirs.push((i, pkg_id, up_dir));
        }
    }

    for (i, pkg_id, txn) in installed_records {
        let engine_clone = Arc::clone(&engine);
        let barrier_clone = Arc::clone(&barrier_mixed);
        let up_info = upgrade_dirs.iter().find(|(idx, _, _)| *idx == i).cloned();

        let handle = thread::spawn(move || {
            barrier_clone.wait();
            let engine_guard = engine_clone.lock().unwrap();
            if i % 2 == 0 {
                // Rollback
                let rb_txn = engine_guard
                    .rollback(&txn, false)
                    .expect("Concurrent rollback must succeed");
                (i, pkg_id, "ROLLED_BACK", rb_txn)
            } else {
                // Upgrade
                let (_, _, up_dir) = up_info.unwrap();
                let up_txn = engine_guard
                    .install(&up_dir, &format!("worker_upgrade_{i}"))
                    .expect("Concurrent upgrade must succeed");
                (i, pkg_id, "UPGRADED", up_txn)
            }
        });
        mixed_handles.push(handle);
    }

    let mut mixed_results = Vec::new();
    for h in mixed_handles {
        mixed_results.push(h.join().expect("Worker thread panicked in mixed phase"));
    }

    // Verify final state:
    // Exactly 3 packages remain in installed_packages (the upgraded ones: 1, 3, 5)
    // The rolled back ones (0, 2, 4) are completely absent.
    {
        let engine_guard = engine.lock().unwrap();
        let final_count: i64 = engine_guard
            .db
            .conn
            .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(final_count, (num_threads / 2) as i64);

        for (_i, pkg_id, action, txn_result) in mixed_results {
            if action == "ROLLED_BACK" {
                let pkg_exists: i64 = engine_guard
                    .db
                    .conn
                    .query_row(
                        "SELECT COUNT(*) FROM installed_packages WHERE package_id = ?1",
                        params![pkg_id],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(
                    pkg_exists, 0,
                    "Package {} was rolled back, must NOT exist in installed_packages",
                    pkg_id
                );
            } else {
                let (ver, txn): (String, String) = engine_guard
                    .db
                    .conn
                    .query_row(
                        "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
                        params![pkg_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(ver, "2.0.0", "Package {} must be upgraded to 2.0.0", pkg_id);
                assert_eq!(txn, txn_result, "install_txn_id must match upgrade txn for {}", pkg_id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scenario 4: Startup `recover()` with Interrupted Alongside Completed Txns
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_startup_recovery_interrupted_with_completed_same_package() {
    let (_guard, home, data, pkg_root) = create_temp_env("recovery");
    let engine = make_engine(&home, &data);

    let pkg_id = "com.adversarial.recovery_pkg";
    let v1_dir = pkg_root.join("v1");
    let target_file = home.join(".config/recovery/app.conf");
    let v1_content = b"valid_committed_v1_config";

    prepare_versioned_package(
        &v1_dir,
        pkg_id,
        "1.0.0",
        &[("app.conf", ".config/recovery/app.conf", v1_content, false)],
    );

    // 1. Install v1.0.0 successfully -> COMMITTED
    let txn_v1 = engine.install(&v1_dir, "tester").expect("v1 install must succeed");

    // Verify v1 is in installed_packages
    let (v1_ver, v1_txn): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
            params![pkg_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(v1_ver, "1.0.0");
    assert_eq!(v1_txn, txn_v1);

    // 2. Simulate a crashed upgrade to v2.0.0 for the SAME package_id:
    // Create an interrupted transaction `txn_v2_crashed` with status EXECUTING
    let txn_v2_crashed = format!("TXN-CRASH-{}", uuid::Uuid::new_v4());
    engine
        .db
        .conn
        .execute(
            "INSERT INTO transactions VALUES(?1,NULL,?2,'2.0.0','INSTALL','EXECUTING',1000,NULL,'crashed_proc',NULL,NULL)",
            params![txn_v2_crashed, pkg_id],
        )
        .unwrap();

    // Create a backup snapshot as if Backup stage took place
    let backup_id = engine
        .backups
        .snapshot(&target_file, &txn_v2_crashed, &engine.db)
        .unwrap();

    // Mutate the target file with corrupted/in-flight v2 data
    fs::write(&target_file, b"corrupted_in_flight_v2_content").unwrap();
    let corrupted_hash = sha256_file(&target_file).unwrap();

    // Record completed operation with backup in operations table
    engine
        .db
        .conn
        .execute(
            "INSERT INTO operations VALUES('OP-CRASH-1',?1,1,'MODIFY_FILE',?2,?3,?4,?5,'COMPLETED',NULL)",
            params![
                txn_v2_crashed,
                target_file.to_string_lossy(),
                sha256_bytes(v1_content),
                corrupted_hash,
                backup_id
            ],
        )
        .unwrap();

    // Also record a planned operation that never executed
    engine
        .db
        .conn
        .execute(
            "INSERT INTO operations VALUES('OP-CRASH-2',?1,2,'CREATE_FILE',?2,NULL,'somehash',NULL,'PLANNED',NULL)",
            params![
                txn_v2_crashed,
                home.join(".config/recovery/uncreated.conf").to_string_lossy()
            ],
        )
        .unwrap();

    // In installed_packages, the row still points to txn_v1 (or even if an orphan row existed)
    // 3. Trigger startup recovery
    let recovered_count = engine.recover().expect("Startup recover must succeed");
    assert!(recovered_count >= 1, "Must recover at least 1 interrupted transaction");

    // Invariant A: Crashed txn_v2 must be marked ABORTED_RECOVERED
    let crash_status: String = engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            params![txn_v2_crashed],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(crash_status, "ABORTED_RECOVERED");

    // Invariant B: Target file must be compensated/restored from backup
    let restored_content = fs::read(&target_file).unwrap();
    assert_eq!(restored_content, v1_content, "Target file must be restored to v1 content");

    // Invariant C (CRUCIAL): Completed v1.0.0 record in installed_packages MUST NOT BE REMOVED!
    // With the old buggy query `DELETE FROM installed_packages WHERE install_txn_id = ?1 OR package_id = ?2`,
    // this row would have been erroneously wiped out!
    let (still_v1_ver, still_v1_txn): (String, String) = engine
        .db
        .conn
        .query_row(
            "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = ?1",
            params![pkg_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("Completed transaction v1.0.0 record must NOT be deleted by recovery of interrupted v2.0.0 transaction!");
    assert_eq!(still_v1_ver, "1.0.0");
    assert_eq!(still_v1_txn, txn_v1);

    // Part D: Orphan record recovery test
    // What if a transaction crashed after writing to installed_packages with install_txn_id = txn_orphan?
    let txn_orphan = format!("TXN-ORPHAN-{}", uuid::Uuid::new_v4());
    let orphan_pkg_id = "com.adversarial.orphan";
    engine
        .db
        .conn
        .execute(
            "INSERT INTO transactions VALUES(?1,NULL,?2,'1.0.0','INSTALL','EXECUTING',2000,NULL,'crashed_proc',NULL,NULL)",
            params![txn_orphan, orphan_pkg_id],
        )
        .unwrap();
    engine
        .db
        .conn
        .execute(
            "INSERT INTO installed_packages(package_id,active_version,package_type,installed_at,updated_at,is_explicit,ref_count,install_txn_id) VALUES(?1,'1.0.0','config',2000,2000,1,1,?2)",
            params![orphan_pkg_id, txn_orphan],
        )
        .unwrap();

    let rec2 = engine.recover().expect("Second recover must succeed");
    assert_eq!(rec2, 1);

    // Orphan row for txn_orphan must be purged
    let orphan_count: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = ?1",
            params![orphan_pkg_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphan_count, 0, "Orphan installed_packages row must be deleted");

    // But valid v1.0.0 for com.adversarial.recovery_pkg MUST STILL EXIST
    let still_v1_count: i64 = engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = ?1",
            params![pkg_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(still_v1_count, 1, "Valid package must still exist");
}

// ---------------------------------------------------------------------------
// Scenario 5: Error Boundaries and Defense-in-Depth Validation
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_rollback_error_boundaries_and_rejections() {
    let (_guard, home, data, pkg_root) = create_temp_env("error_boundaries");
    let engine = make_engine(&home, &data);

    // 1. Non-existent transaction ID
    let err_nonexistent = engine.rollback("NON_EXISTENT_TXN", false);
    match err_nonexistent {
        Err(WoderriseError::Other(msg)) => {
            assert!(msg.contains("not found"), "Error must state transaction not found: {}", msg);
        }
        other => panic!("Expected Other('not found'), got: {:?}", other),
    }

    // 2. Transaction with non-COMMITTED status (e.g. PLANNED, EXECUTING, FAILED)
    let txn_planned = engine
        .db
        .begin_transaction("com.adv.dummy", "1.0.0", "INSTALL", "tester")
        .unwrap();

    let err_planned = engine.rollback(&txn_planned, false);
    match err_planned {
        Err(WoderriseError::Other(msg)) => {
            assert!(
                msg.contains("must be COMMITTED"),
                "Error must indicate transaction must be COMMITTED: {}",
                msg
            );
        }
        other => panic!("Expected Other('must be COMMITTED'), got: {:?}", other),
    }

    // 3. Repeated rollback rejection
    let pkg_dir = pkg_root.join("single");
    prepare_versioned_package(
        &pkg_dir,
        "com.adv.single",
        "1.0.0",
        &[("test.conf", ".config/single/test.conf", b"content", false)],
    );
    let txn_ok = engine.install(&pkg_dir, "tester").unwrap();

    // First rollback succeeds
    let rb_ok = engine.rollback(&txn_ok, false).expect("First rollback must succeed");
    assert!(!rb_ok.is_empty());

    // Second rollback must be rejected (status is ROLLED_BACK)
    let err_second = engine.rollback(&txn_ok, false);
    match err_second {
        Err(WoderriseError::Other(msg)) => {
            assert!(
                msg.contains("ROLLED_BACK") && msg.contains("must be COMMITTED"),
                "Error must reject already rolled back transaction: {}",
                msg
            );
        }
        other => panic!("Expected Other with status ROLLED_BACK, got: {:?}", other),
    }
}
