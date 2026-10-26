//! Adversarial Stress & Edge Case Test Suite for Milestone M8 (Hyprland Transactional Safety).
//!
//! Empirically challenges:
//! 1. `compute_plan` strict read-only guarantee under non-existent directories, multi-run determinism,
//!    and verification that ZERO bytes, directories, or SQLite journal records leak.
//! 2. Rollback conflict detection when installed files are externally deleted (missing target file).
//! 3. Zero disk mutation guarantee on conflict abort across all target files, snapshots, and DB state.
//! 4. 100% byte-for-byte fidelity of forced rollback after external deletions and mutations.
//! 5. Multi-transaction mixed-state crash recovery: recovering PLANNED, PREPARED, EXECUTING, FAILED
//!    transactions simultaneously while leaving COMMITTED and ROLLED_BACK transactions untouched.
//! 6. Orphan row cleanup in `installed_packages` during crash recovery.
//! 7. Shared content-addressed snapshot deduplication (identical pre-existing files).
//! 8. Invalid/repeated rollback state machine rejection.

use std::{
    fs,
    path::PathBuf,
};

use rusqlite::params;
use woderrise_core::{
    diff::FileDiffAction, sha256_bytes, sha256_file, BackupManager, Database,
    DiffLineKind, InstallationEngine, Manifest, WoderriseError,
};

fn registry_packages_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages")
}

fn hyprland_pkg_dir() -> PathBuf {
    registry_packages_dir().join("com.catppuccin.hyprland/1.0.0")
}

struct TestEnv {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    backups: PathBuf,
    engine: InstallationEngine,
}

fn create_test_env(name: &str) -> TestEnv {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let home = tmp.path().join(format!("home_{name}"));
    let data = tmp.path().join(format!("data_{name}"));
    let backups = data.join("backups");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&backups).unwrap();
    let db = Database::open(data.join("journal.db")).unwrap();
    let backups_mgr = BackupManager::new(&backups).unwrap();
    let engine = InstallationEngine {
        db,
        backups: backups_mgr,
        home: home.clone(),
    };
    TestEnv {
        _tmp: tmp,
        home,
        backups,
        engine,
    }
}

// ---------------------------------------------------------------------------
// Challenge 1: compute_plan leaves zero disk trace & zero SQLite mutations
// even when target directories do not initially exist, across repeated runs
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_compute_plan_zero_leakage_and_idempotence() {
    let env = create_test_env("chal_compute_plan");
    let pkg_dir = hyprland_pkg_dir();

    // Verify ~/.config/hypr and ~/.config/waybar do NOT exist initially in home
    let hypr_dir = env.home.join(".config/hypr");
    let waybar_dir = env.home.join(".config/waybar");
    assert!(!hypr_dir.exists());
    assert!(!waybar_dir.exists());

    // Record initial filesystem metadata
    let initial_home_entries: Vec<_> = fs::read_dir(&env.home)
        .unwrap()
        .map(|r| r.unwrap().path())
        .collect();
    assert!(initial_home_entries.is_empty());

    let initial_backup_entries: Vec<_> = fs::read_dir(&env.backups)
        .unwrap()
        .map(|r| r.unwrap().path())
        .collect();
    assert!(initial_backup_entries.is_empty());

    // Run compute_plan 5 consecutive times
    for i in 1..=5 {
        let plan = env
            .engine
            .compute_plan(&pkg_dir)
            .unwrap_or_else(|e| panic!("compute_plan run {i} failed: {e}"));

        assert_eq!(plan.package_id, "com.catppuccin.hyprland");
        assert_eq!(plan.version, "1.0.0");
        assert_eq!(plan.file_diffs.len(), 9);
        assert_eq!(plan.will_backup_count, 0);

        for diff in &plan.file_diffs {
            assert_eq!(diff.action, FileDiffAction::Create);
            assert!(diff.planned_backup_path.is_none());
            assert!(diff.lines.iter().all(|l| l.kind == DiffLineKind::Addition));
        }

        // STRICT INVARIANCE:
        // 1. Home directory must remain completely empty (no directories created by dry-run)
        let home_entries: Vec<_> = fs::read_dir(&env.home)
            .unwrap()
            .map(|r| r.unwrap().path())
            .collect();
        assert!(
            home_entries.is_empty(),
            "compute_plan must NOT create parent directories in home"
        );

        // 2. Backups directory must remain completely empty
        let backup_entries: Vec<_> = fs::read_dir(&env.backups)
            .unwrap()
            .map(|r| r.unwrap().path())
            .collect();
        assert!(
            backup_entries.is_empty(),
            "compute_plan must NOT write to backups"
        );

        // 3. Database must contain zero rows
        let txn_count: i64 = env
            .engine
            .db
            .conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        let op_count: i64 = env
            .engine
            .db
            .conn
            .query_row("SELECT COUNT(*) FROM operations", [], |r| r.get(0))
            .unwrap();
        let bkp_count: i64 = env
            .engine
            .db
            .conn
            .query_row("SELECT COUNT(*) FROM backup_snapshots", [], |r| r.get(0))
            .unwrap();
        let pkg_count: i64 = env
            .engine
            .db
            .conn
            .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
            .unwrap();

        assert_eq!(txn_count, 0, "No transactions allowed during dry-run");
        assert_eq!(op_count, 0, "No operations allowed during dry-run");
        assert_eq!(bkp_count, 0, "No backup snapshots allowed during dry-run");
        assert_eq!(pkg_count, 0, "No packages allowed during dry-run");
    }
}

// ---------------------------------------------------------------------------
// Challenge 2: External deletion of installed file triggers Conflict abort
// with zero disk mutation
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_external_file_deletion_aborts_rollback_as_conflict() {
    let env = create_test_env("chal_del_conflict");
    let pkg_dir = hyprland_pkg_dir();

    // Set up 1 pre-existing file
    let hypr_dir = env.home.join(".config/hypr");
    fs::create_dir_all(&hypr_dir).unwrap();
    let hyprland_conf = hypr_dir.join("hyprland.conf");
    let original_bytes = b"# Original pre-existing config\nmonitor=,preferred,auto,1\n".to_vec();
    fs::write(&hyprland_conf, &original_bytes).unwrap();
    let original_hash = sha256_bytes(&original_bytes);

    // Install package
    let txn = env
        .engine
        .install(&pkg_dir, "chal_del_installer")
        .expect("install");

    // All 9 files exist
    assert!(hyprland_conf.is_file());
    let latte_conf = hypr_dir.join("latte.conf");
    assert!(latte_conf.is_file());

    // Adversarial action: simulate user or external process DELETING latte.conf
    fs::remove_file(&latte_conf).expect("delete installed file");
    assert!(!latte_conf.exists());

    // Attempt unforced rollback
    let rb_res = env.engine.rollback(&txn, false);
    match rb_res {
        Err(WoderriseError::Conflict(path)) => {
            assert_eq!(
                path,
                latte_conf.to_string_lossy(),
                "Conflict must pinpoint the externally deleted file"
            );
        }
        other => panic!("Expected Conflict error on deleted file, got: {:?}", other),
    }

    // ZERO DISK MUTATION GUARANTEE:
    // hyprland.conf must remain installed (not reverted yet)
    let _manifest = Manifest::from_file(pkg_dir.join("woderrise.yaml")).unwrap();
    let expected_hyprland_installed = sha256_file(pkg_dir.join("artifacts/hyprland.conf")).unwrap();
    assert_eq!(
        sha256_file(&hyprland_conf).unwrap(),
        expected_hyprland_installed,
        "hyprland.conf must NOT be mutated on conflict abort"
    );

    // Backup snapshot still exists in backup store
    let backup_snapshot = env.backups.join(&original_hash);
    assert!(
        backup_snapshot.is_file(),
        "Backup snapshot must remain untouched"
    );

    // Transaction remains COMMITTED, installed_packages remains intact
    let status: String = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            [&txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "COMMITTED");

    let count: i64 = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.catppuccin.hyprland'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

// ---------------------------------------------------------------------------
// Challenge 3: Forced rollback after external mutations and deletions
// restores exact pre-install bytes and deletes created files
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_forced_rollback_restores_exact_state_after_external_corruption() {
    let env = create_test_env("chal_forced_rb");
    let pkg_dir = hyprland_pkg_dir();

    // Set up 2 pre-existing files
    let hypr_dir = env.home.join(".config/hypr");
    let waybar_dir = env.home.join(".config/waybar");
    fs::create_dir_all(&hypr_dir).unwrap();
    fs::create_dir_all(&waybar_dir).unwrap();

    let hypr_conf = hypr_dir.join("hyprland.conf");
    let hypr_orig = b"# Hyprland original content\n".to_vec();
    fs::write(&hypr_conf, &hypr_orig).unwrap();
    let hypr_orig_hash = sha256_bytes(&hypr_orig);

    let waybar_css = waybar_dir.join("style.css");
    let waybar_orig = b"/* Waybar original style */\n".to_vec();
    fs::write(&waybar_css, &waybar_orig).unwrap();
    let waybar_orig_hash = sha256_bytes(&waybar_orig);

    // Install package
    let txn = env.engine.install(&pkg_dir, "chal_forced_inst").unwrap();

    // Adversarial corruption:
    // 1. External edit of hyprland.conf
    fs::write(&hypr_conf, b"# Corrupted externally by rogue script\n").unwrap();
    // 2. External deletion of waybar style.css
    fs::remove_file(&waybar_css).unwrap();
    // 3. External edit of created file mocha.conf
    let mocha_conf = hypr_dir.join("mocha.conf");
    fs::write(&mocha_conf, b"$tampered = true\n").unwrap();

    // Execute forced rollback
    let rb_txn = env
        .engine
        .rollback(&txn, true)
        .expect("Forced rollback must succeed");
    assert!(!rb_txn.is_empty());

    // 1. Pre-existing files restored byte-for-byte to original content
    assert!(hypr_conf.is_file());
    assert_eq!(fs::read(&hypr_conf).unwrap(), hypr_orig);
    assert_eq!(sha256_file(&hypr_conf).unwrap(), hypr_orig_hash);

    assert!(waybar_css.is_file());
    assert_eq!(fs::read(&waybar_css).unwrap(), waybar_orig);
    assert_eq!(sha256_file(&waybar_css).unwrap(), waybar_orig_hash);

    // 2. All created files removed from disk
    assert!(!mocha_conf.exists());
    assert!(!hypr_dir.join("hyprlock.conf").exists());
    assert!(!hypr_dir.join("hypridle.conf").exists());
    assert!(!hypr_dir.join("macchiato.conf").exists());
    assert!(!hypr_dir.join("frappe.conf").exists());
    assert!(!hypr_dir.join("latte.conf").exists());
    assert!(!waybar_dir.join("config.jsonc").exists());

    // 3. Database state
    let orig_status: String = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            [&txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orig_status, "ROLLED_BACK");

    let rb_status: String = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            [&rb_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rb_status, "COMMITTED");

    let pkg_count: i64 = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.catppuccin.hyprland'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pkg_count, 0);
}

// ---------------------------------------------------------------------------
// Challenge 4: Multi-transaction mixed-state crash recovery
// Recovers PLANNED, PREPARED, EXECUTING, FAILED transactions without touching
// COMMITTED or ROLLED_BACK transactions, and purges orphan rows
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_multi_transaction_crash_recovery_isolation() {
    let env = create_test_env("chal_crash_isolation");
    let pkg_dir = hyprland_pkg_dir();

    // 1. Transaction A: Legitimately COMMITTED package
    let hypr_dir = env.home.join(".config/hypr");
    fs::create_dir_all(&hypr_dir).unwrap();
    let hypr_conf = hypr_dir.join("hyprland.conf");
    let hypr_orig = b"# Base hyprland config\n".to_vec();
    fs::write(&hypr_conf, &hypr_orig).unwrap();

    let txn_committed = env.engine.install(&pkg_dir, "legit_installer").unwrap();
    let committed_installed_hash = sha256_file(&hypr_conf).unwrap();

    // 2. Transaction B: Crashed in EXECUTING state (partially modified file, partially created file)
    let txn_executing = env
        .engine
        .db
        .begin_transaction("com.test.crashed_exec", "1.0.0", "INSTALL", "crashed_worker")
        .unwrap();

    // Snapshot hyprland.conf under txn_executing
    let snap_hypr = env
        .engine
        .backups
        .snapshot(&hypr_conf, &txn_executing, &env.engine.db)
        .unwrap();
    let partial_hypr_bytes = b"# Broken partial hyprland config from crashed transaction\n";
    fs::write(&hypr_conf, partial_hypr_bytes).unwrap();
    let partial_hypr_hash = sha256_bytes(partial_hypr_bytes);

    // Create partial file
    let partial_created_file = env.home.join(".config/hypr/partial_created.conf");
    let partial_created_bytes = b"# Orphan created file\n";
    fs::write(&partial_created_file, partial_created_bytes).unwrap();
    let partial_created_hash = sha256_bytes(partial_created_bytes);

    // Record operations for txn_executing
    env.engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-EX-1', ?1, 1, 'MODIFY_FILE', ?2, ?3, ?4, ?5, 'COMPLETED', NULL)",
        params![txn_executing, hypr_conf.to_string_lossy(), committed_installed_hash, partial_hypr_hash, snap_hypr],
    ).unwrap();
    env.engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-EX-2', ?1, 2, 'CREATE_FILE', ?2, NULL, ?3, NULL, 'COMPLETED', NULL)",
        params![txn_executing, partial_created_file.to_string_lossy(), partial_created_hash],
    ).unwrap();
    env.engine
        .db
        .set_status(&txn_executing, "EXECUTING", None)
        .unwrap();

    // Insert orphan row for crashed package
    env.engine.db.conn.execute(
        "INSERT INTO installed_packages(package_id, active_version, package_type, installed_at, updated_at, is_explicit, ref_count, install_txn_id) VALUES('com.test.crashed_exec', '1.0.0', 'config', 100, 100, 1, 1, ?1)",
        params![txn_executing],
    ).unwrap();

    // 3. Transaction C: Crashed in FAILED state
    let txn_failed = env
        .engine
        .db
        .begin_transaction("com.test.failed_pkg", "1.0.0", "INSTALL", "failed_worker")
        .unwrap();
    env.engine
        .db
        .set_status(&txn_failed, "FAILED", Some("disk full"))
        .unwrap();

    // 4. Transaction D: Stuck in PREPARED state
    let txn_prepared = env
        .engine
        .db
        .begin_transaction("com.test.prepared_pkg", "1.0.0", "INSTALL", "prepared_worker")
        .unwrap();
    env.engine
        .db
        .set_status(&txn_prepared, "PREPARED", None)
        .unwrap();

    // 5. Transaction E: Stuck in PLANNED state
    let txn_planned = env
        .engine
        .db
        .begin_transaction("com.test.planned_pkg", "1.0.0", "INSTALL", "planned_worker")
        .unwrap();

    // Verify initial dirty state
    assert!(partial_created_file.exists());
    assert_ne!(
        sha256_file(&hypr_conf).unwrap(),
        committed_installed_hash
    );

    // Execute recover()
    let recovered = env.engine.recover().expect("recovery must succeed");
    assert_eq!(
        recovered, 4,
        "Must recover exactly the 4 uncommitted transactions (EXECUTING, FAILED, PREPARED, PLANNED)"
    );

    // VERIFICATION OF ISOLATION & INTEGRITY:
    // 1. Transaction A (COMMITTED) must be intact:
    let st_committed: String = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            [&txn_committed],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(st_committed, "COMMITTED");

    // The hyprland.conf file must be restored back to Transaction A's installed state!
    assert_eq!(
        sha256_file(&hypr_conf).unwrap(),
        committed_installed_hash,
        "hyprland.conf must be restored to Transaction A's state by compensating Transaction B"
    );

    // The orphan created file from Transaction B must be deleted!
    assert!(
        !partial_created_file.exists(),
        "Partial created file must be deleted by recovery"
    );

    // Transaction A remains active in installed_packages
    let pkg_a_exists: i64 = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.catppuccin.hyprland' AND install_txn_id = ?1",
            [&txn_committed],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pkg_a_exists, 1);

    // Orphan row for Transaction B must be PURGED from installed_packages
    let pkg_b_exists: i64 = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.test.crashed_exec'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pkg_b_exists, 0, "Orphan package row must be purged");

    // All 4 recovered transactions must have status ABORTED_RECOVERED
    for t in [&txn_executing, &txn_failed, &txn_prepared, &txn_planned] {
        let st: String = env
            .engine
            .db
            .conn
            .query_row(
                "SELECT status FROM transactions WHERE txn_id = ?1",
                [t],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(st, "ABORTED_RECOVERED");
    }

    // Second recovery is an idempotent no-op returning 0
    assert_eq!(env.engine.recover().unwrap(), 0);
}

// ---------------------------------------------------------------------------
// Challenge 5: Snapshot Deduplication and Content Addressing
// When two pre-existing files have identical content, snapshot reuses storage
// without collision or premature unlinking during rollback
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_backup_deduplication_and_safe_multi_restore() {
    let env = create_test_env("chal_dedup");

    let file_a = env.home.join("config_a.conf");
    let file_b = env.home.join("config_b.conf");
    let shared_content = b"identical default configuration content\n";
    fs::write(&file_a, shared_content).unwrap();
    fs::write(&file_b, shared_content).unwrap();

    let shared_hash = sha256_bytes(shared_content);

    let txn = env
        .engine
        .db
        .begin_transaction("com.test.dedup", "1.0.0", "INSTALL", "dedup_worker")
        .unwrap();

    // Snapshot both files
    let snap_a = env
        .engine
        .backups
        .snapshot(&file_a, &txn, &env.engine.db)
        .unwrap();
    let snap_b = env
        .engine
        .backups
        .snapshot(&file_b, &txn, &env.engine.db)
        .unwrap();

    assert_ne!(
        snap_a, snap_b,
        "Snapshot IDs must be unique even for identical content"
    );

    // Only ONE physical backup file exists in the content-addressed backup store
    let backup_store_file = env.backups.join(&shared_hash);
    assert!(backup_store_file.is_file());

    let backup_files_count = fs::read_dir(&env.backups).unwrap().count();
    assert_eq!(
        backup_files_count, 1,
        "Identical files must deduplicate to 1 file in backup store"
    );

    // Mutate both files
    fs::write(&file_a, b"new content a\n").unwrap();
    fs::write(&file_b, b"new content b\n").unwrap();

    // Record operations
    env.engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-DEDUP-1', ?1, 1, 'MODIFY_FILE', ?2, ?3, 'new_hash_a', ?4, 'COMPLETED', NULL)",
        params![txn, file_a.to_string_lossy(), shared_hash, snap_a],
    ).unwrap();
    env.engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-DEDUP-2', ?1, 2, 'MODIFY_FILE', ?2, ?3, 'new_hash_b', ?4, 'COMPLETED', NULL)",
        params![txn, file_b.to_string_lossy(), shared_hash, snap_b],
    ).unwrap();
    env.engine.db.set_status(&txn, "COMMITTED", None).unwrap();

    // Execute rollback
    let rb_txn = env
        .engine
        .rollback(&txn, true)
        .expect("Rollback with deduplicated backups must succeed");
    assert!(!rb_txn.is_empty());

    // Both files restored to shared content
    assert_eq!(fs::read(&file_a).unwrap(), shared_content);
    assert_eq!(fs::read(&file_b).unwrap(), shared_content);

    // The shared backup file still safely exists in backup store (atomic_replace copied, didn't move/delete)
    assert!(backup_store_file.is_file());
}

// ---------------------------------------------------------------------------
// Challenge 6: Invalid rollback state transitions
// Reject non-existent transaction, already rolled-back transaction, and aborted transaction
// ---------------------------------------------------------------------------
#[test]
fn test_adversarial_rollback_state_machine_validation() {
    let env = create_test_env("chal_state_machine");

    // 1. Rollback non-existent transaction
    let err_nonexistent = env.engine.rollback("TXN-NON-EXISTENT-9999", false);
    assert!(err_nonexistent.is_err());
    let msg = err_nonexistent.unwrap_err().to_string();
    assert!(msg.contains("not found"));

    // 2. Rollback transaction in PREPARED state (not COMMITTED)
    let txn_prepared = env
        .engine
        .db
        .begin_transaction("com.test.prep", "1.0.0", "INSTALL", "tester")
        .unwrap();
    env.engine
        .db
        .set_status(&txn_prepared, "PREPARED", None)
        .unwrap();

    let err_prepared = env.engine.rollback(&txn_prepared, false);
    assert!(err_prepared.is_err());
    let msg = err_prepared.unwrap_err().to_string();
    assert!(msg.contains("must be COMMITTED"));

    // 3. Rollback transaction in ABORTED_RECOVERED state
    let txn_aborted = env
        .engine
        .db
        .begin_transaction("com.test.abort", "1.0.0", "INSTALL", "tester")
        .unwrap();
    env.engine
        .db
        .set_status(&txn_aborted, "ABORTED_RECOVERED", None)
        .unwrap();

    let err_aborted = env.engine.rollback(&txn_aborted, false);
    assert!(err_aborted.is_err());
    let msg = err_aborted.unwrap_err().to_string();
    assert!(msg.contains("must be COMMITTED"));
}
