//! Comprehensive Integration Test Suite for Milestone M8: Hyprland Transactional Safety & Non-Destructive Coexistence.
//!
//! Validates the 8 core transactional safety requirements:
//! 1. FTS5 Search Discovery & Metadata Validation for `com.catppuccin.hyprland/1.0.0`
//! 2. Realistic Mixed Hyprland Environment Setup & Pre-Flight Validation
//! 3. Strictly Read-Only Myers Line Diff Calculation & Deterministic Backup Path Prediction
//! 4. 7-Stage Atomic Installation with Content-Addressed Backups
//! 5. SQLite WAL Journal Consistency, Foreign Keys & Table Invariants
//! 6. Non-Destructive Coexistence: External Modification Pre-Flight Conflict Detection
//! 7. 100% Reversible Forced Rollback Byte-for-Byte Restoration & Cleanup
//! 8. Interrupted Transaction Crash Recovery & Compensation

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use rusqlite::params;
use woderrise_core::{
    catalog, diff::FileDiffAction, expand_target, sha256_bytes, sha256_file, BackupManager,
    Database, DiffLineKind, InstallStage, InstallationEngine, Manifest, ProgressCallback,
    WoderriseError,
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

struct MixedHyprlandState {
    // 4 pre-existing files
    hyprland_conf_path: PathBuf,
    hyprland_conf_bytes: Vec<u8>,
    hyprland_conf_hash: String,

    mocha_conf_path: PathBuf,
    mocha_conf_bytes: Vec<u8>,
    mocha_conf_hash: String,

    waybar_config_path: PathBuf,
    waybar_config_bytes: Vec<u8>,
    waybar_config_hash: String,

    waybar_style_path: PathBuf,
    waybar_style_bytes: Vec<u8>,
    waybar_style_hash: String,

    // 5 non-existent files
    hyprlock_conf_path: PathBuf,
    hypridle_conf_path: PathBuf,
    macchiato_conf_path: PathBuf,
    frappe_conf_path: PathBuf,
    latte_conf_path: PathBuf,
}

fn setup_mixed_hyprland_environment(home: &Path) -> MixedHyprlandState {
    let hypr_dir = home.join(".config/hypr");
    let waybar_dir = home.join(".config/waybar");
    fs::create_dir_all(&hypr_dir).unwrap();
    fs::create_dir_all(&waybar_dir).unwrap();

    let hyprland_conf_path = hypr_dir.join("hyprland.conf");
    let hyprland_conf_bytes = b"# Pre-existing user Hyprland configuration\n\
monitor=DP-1,2560x1440@144,0x0,1\n\
monitor=HDMI-A-1,1920x1080@60,2560x0,1\n\
$terminal = kitty\n\
$fileManager = thunar\n\
bind = SUPER, Q, exec, $terminal\n\
bind = SUPER, C, killactive,\n\
exec-once = waybar &\n"
        .to_vec();
    fs::write(&hyprland_conf_path, &hyprland_conf_bytes).unwrap();
    let hyprland_conf_hash = sha256_bytes(&hyprland_conf_bytes);

    let mocha_conf_path = hypr_dir.join("mocha.conf");
    let mocha_conf_bytes = b"# Pre-existing user custom mocha palette override\n\
$base = rgb(1e1e2e)\n\
$text = rgb(cdd6f4)\n\
$custom_accent = rgb(ff5555)\n"
        .to_vec();
    fs::write(&mocha_conf_path, &mocha_conf_bytes).unwrap();
    let mocha_conf_hash = sha256_bytes(&mocha_conf_bytes);

    let waybar_config_path = waybar_dir.join("config.jsonc");
    let waybar_config_bytes = b"{\n\
  \"layer\": \"top\",\n\
  \"position\": \"top\",\n\
  \"modules-left\": [\"hyprland/workspaces\"],\n\
  \"modules-center\": [\"clock\"],\n\
  \"modules-right\": [\"network\", \"battery\"]\n\
}\n"
        .to_vec();
    fs::write(&waybar_config_path, &waybar_config_bytes).unwrap();
    let waybar_config_hash = sha256_bytes(&waybar_config_bytes);

    let waybar_style_path = waybar_dir.join("style.css");
    let waybar_style_bytes = b"* {\n\
  font-family: \"JetBrainsMono Nerd Font\";\n\
  font-size: 13px;\n\
}\n\
#waybar {\n\
  background-color: rgba(30, 30, 46, 0.9);\n\
  color: #cdd6f4;\n\
}\n"
        .to_vec();
    fs::write(&waybar_style_path, &waybar_style_bytes).unwrap();
    let waybar_style_hash = sha256_bytes(&waybar_style_bytes);

    let hyprlock_conf_path = hypr_dir.join("hyprlock.conf");
    let hypridle_conf_path = hypr_dir.join("hypridle.conf");
    let macchiato_conf_path = hypr_dir.join("macchiato.conf");
    let frappe_conf_path = hypr_dir.join("frappe.conf");
    let latte_conf_path = hypr_dir.join("latte.conf");

    // Assert that the 5 files definitely do not exist initially
    assert!(!hyprlock_conf_path.exists());
    assert!(!hypridle_conf_path.exists());
    assert!(!macchiato_conf_path.exists());
    assert!(!frappe_conf_path.exists());
    assert!(!latte_conf_path.exists());

    MixedHyprlandState {
        hyprland_conf_path,
        hyprland_conf_bytes,
        hyprland_conf_hash,
        mocha_conf_path,
        mocha_conf_bytes,
        mocha_conf_hash,
        waybar_config_path,
        waybar_config_bytes,
        waybar_config_hash,
        waybar_style_path,
        waybar_style_bytes,
        waybar_style_hash,
        hyprlock_conf_path,
        hypridle_conf_path,
        macchiato_conf_path,
        frappe_conf_path,
        latte_conf_path,
    }
}

// ===========================================================================
// Test 1: Full FTS5 search discovery and metadata validation for com.catppuccin.hyprland/1.0.0
// ===========================================================================
#[test]
fn test_01_hyprland_fts5_catalog_discovery_and_metadata_validation() {
    let pkg_dir = hyprland_pkg_dir();
    let manifest_path = pkg_dir.join("woderrise.yaml");
    assert!(
        manifest_path.exists(),
        "Manifest must exist at {}",
        manifest_path.display()
    );

    let manifest =
        Manifest::from_file(&manifest_path).expect("Manifest must parse and validate cleanly");
    assert_eq!(manifest.schema_version, "1.0.0");
    assert_eq!(manifest.package.id, "com.catppuccin.hyprland");
    assert_eq!(manifest.package.version, "1.0.0");
    assert_eq!(manifest.package.package_type, "config");
    assert_eq!(manifest.package.category, "desktop_setups");
    assert_eq!(manifest.package.license, "MIT");
    assert!(manifest
        .package
        .authors
        .iter()
        .any(|a| a.name.contains("Catppuccin")));
    assert_eq!(
        manifest.package.repository.as_deref(),
        Some("https://github.com/catppuccin/hyprland")
    );
    assert!(
        manifest.package.description.contains("Catppuccin")
            && manifest.package.description.contains("Hyprland")
    );
    assert!(manifest.package.tags.contains(&"hyprland".to_string()));

    // Palette inspection: exactly 10 authentic swatches across Mocha, Macchiato, Frappe, Latte
    let previews = manifest.previews.as_ref().expect("Previews must be present");
    assert_eq!(
        previews.palette.len(),
        10,
        "Must declare exactly 10 palette swatches"
    );
    let expected_names = [
        "mocha_base",
        "mocha_mauve",
        "mocha_blue",
        "mocha_text",
        "macchiato_base",
        "macchiato_mauve",
        "frappe_base",
        "frappe_mauve",
        "latte_base",
        "latte_mauve",
    ];
    for name in &expected_names {
        assert!(
            previews.palette.iter().any(|c| c.name == *name),
            "Palette must contain swatch {name}"
        );
    }
    for color in &previews.palette {
        assert_eq!(
            color.hex.len(),
            7,
            "Hex color must be #RRGGBB: {}",
            color.hex
        );
        assert!(color.hex.starts_with('#'));
        assert!(color.hex[1..].chars().all(|c| c.is_ascii_hexdigit()));
    }
    assert_eq!(previews.thumbnail, "artifacts/hyprland.conf");
    assert!(pkg_dir.join(&previews.thumbnail).is_file());

    // Compatibility
    let compat = manifest
        .compatibility
        .as_ref()
        .expect("Compatibility must be present");
    assert_eq!(compat.display_server.as_deref(), Some("wayland_only"));
    assert!(compat.architectures.contains(&"x86_64".to_string()));
    assert!(compat.architectures.contains(&"aarch64".to_string()));
    assert!(compat
        .compositors
        .iter()
        .any(|c| c.name == "hyprland" && c.version == ">=0.40.0"));

    // System dependencies
    let deps = manifest
        .dependencies
        .as_ref()
        .expect("Dependencies must be present");
    let sys_names: Vec<&str> = deps.system.iter().map(|s| s.name.as_str()).collect();
    for req in ["hyprland", "waybar", "hyprlock", "hypridle"] {
        assert!(
            sys_names.contains(&req),
            "Dependencies must include system package {req}"
        );
    }

    // Artifacts cryptographic verification
    assert_eq!(manifest.artifacts.len(), 9);
    catalog::verify_artifacts(&manifest, &pkg_dir)
        .expect("All 9 artifacts must pass cryptographic SHA-256 verification");

    // Ingest into SQLite FTS5 catalog
    let db = Database::in_memory().expect("in-memory db");
    catalog::migrate(&db).expect("catalog migration");
    let count = catalog::import_registry(&db, registry_packages_dir()).expect("registry import");
    assert!(count >= 1, "Must index at least 1 package");

    let entry = catalog::get(&db, "com.catppuccin.hyprland", "1.0.0").expect("catalog::get");
    assert_eq!(entry.id, "com.catppuccin.hyprland");
    assert_eq!(entry.version, "1.0.0");
    assert_eq!(entry.category, "desktop_setups");
    assert!(entry.validated);
    assert!(!entry.yanked);

    let rehydrated =
        catalog::manifest(&db, "com.catppuccin.hyprland", "1.0.0").expect("catalog::manifest");
    assert_eq!(rehydrated.package.id, "com.catppuccin.hyprland");

    // FTS5 Queries
    let queries = [
        "hyprland",
        "desktop_setups",
        "mocha",
        "latte",
        "catppuccin",
        "hyprlock",
        "hypridle",
    ];
    for q in queries {
        let results = catalog::search(&db, q, 10).unwrap();
        assert!(
            results.iter().any(|e| e.id == "com.catppuccin.hyprland"),
            "FTS5 query '{q}' must return com.catppuccin.hyprland"
        );
    }
    let non_matching = catalog::search(&db, "nonexistent_query_term_12345", 10).unwrap();
    assert!(
        non_matching.is_empty(),
        "Non-matching query must return 0 results"
    );
}

// ===========================================================================
// Test 2: Realistic mixed Hyprland environment setup and pre-flight validation
// ===========================================================================
#[test]
fn test_02_realistic_mixed_hyprland_environment_setup_and_preflight() {
    let env = create_test_env("m8-test2");
    let mixed = setup_mixed_hyprland_environment(&env.home);

    // Verify 4 pre-existing files
    assert!(mixed.hyprland_conf_path.is_file());
    assert!(mixed.mocha_conf_path.is_file());
    assert!(mixed.waybar_config_path.is_file());
    assert!(mixed.waybar_style_path.is_file());

    assert_eq!(
        sha256_file(&mixed.hyprland_conf_path).unwrap(),
        mixed.hyprland_conf_hash
    );
    assert_eq!(
        sha256_file(&mixed.mocha_conf_path).unwrap(),
        mixed.mocha_conf_hash
    );
    assert_eq!(
        sha256_file(&mixed.waybar_config_path).unwrap(),
        mixed.waybar_config_hash
    );
    assert_eq!(
        sha256_file(&mixed.waybar_style_path).unwrap(),
        mixed.waybar_style_hash
    );

    // Verify 5 non-existent files
    assert!(!mixed.hyprlock_conf_path.exists());
    assert!(!mixed.hypridle_conf_path.exists());
    assert!(!mixed.macchiato_conf_path.exists());
    assert!(!mixed.frappe_conf_path.exists());
    assert!(!mixed.latte_conf_path.exists());

    // Map all 9 install files declared in manifest
    let pkg_dir = hyprland_pkg_dir();
    let manifest = Manifest::from_file(pkg_dir.join("woderrise.yaml")).unwrap();
    assert_eq!(manifest.install.files.len(), 9);

    let mut existing_count = 0;
    let mut missing_count = 0;

    for f in &manifest.install.files {
        let target = expand_target(&f.target, &env.home).unwrap();
        assert_eq!(f.mode.as_deref(), Some("0644"));
        assert_eq!(f.strategy.as_deref(), Some("replace"));
        assert!(f.backup_before_write);

        if target.exists() {
            existing_count += 1;
            assert!(
                target == mixed.hyprland_conf_path
                    || target == mixed.mocha_conf_path
                    || target == mixed.waybar_config_path
                    || target == mixed.waybar_style_path,
                "Unexpected existing target: {}",
                target.display()
            );
        } else {
            missing_count += 1;
            assert!(
                target == mixed.hyprlock_conf_path
                    || target == mixed.hypridle_conf_path
                    || target == mixed.macchiato_conf_path
                    || target == mixed.frappe_conf_path
                    || target == mixed.latte_conf_path,
                "Unexpected missing target: {}",
                target.display()
            );
        }
    }

    assert_eq!(existing_count, 4, "Must have exactly 4 pre-existing targets");
    assert_eq!(missing_count, 5, "Must have exactly 5 non-existent targets");
}

// ===========================================================================
// Test 3: Strictly read-only Myers diff calculation (compute_plan)
// ===========================================================================
#[test]
fn test_03_strictly_read_only_myers_diff_and_backup_prediction() {
    let env = create_test_env("m8-test3");
    let mixed = setup_mixed_hyprland_environment(&env.home);
    let pkg_dir = hyprland_pkg_dir();

    // Baseline assertions: backups directory has 0 files, DB has 0 transactions
    let initial_backup_entries = fs::read_dir(&env.backups).map(|r| r.count()).unwrap_or(0);
    assert_eq!(
        initial_backup_entries, 0,
        "Backups directory must initially be empty"
    );

    let count_txn: i64 = env
        .engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    let count_op: i64 = env
        .engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM operations", [], |r| r.get(0))
        .unwrap();
    let count_bkp: i64 = env
        .engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM backup_snapshots", [], |r| r.get(0))
        .unwrap();
    let count_pkg: i64 = env
        .engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_txn, 0);
    assert_eq!(count_op, 0);
    assert_eq!(count_bkp, 0);
    assert_eq!(count_pkg, 0);

    // Execute compute_plan
    let plan = env
        .engine
        .compute_plan(&pkg_dir)
        .expect("compute_plan must succeed");
    assert_eq!(plan.package_id, "com.catppuccin.hyprland");
    assert_eq!(plan.version, "1.0.0");
    assert_eq!(plan.file_diffs.len(), 9);
    assert_eq!(plan.will_backup_count, 4);

    let mut modify_count = 0;
    let mut create_count = 0;

    for diff in &plan.file_diffs {
        if diff.target_path == mixed.hyprland_conf_path {
            modify_count += 1;
            assert_eq!(diff.action, FileDiffAction::Modify);
            assert_eq!(
                diff.planned_backup_path,
                Some(env.backups.join(&mixed.hyprland_conf_hash))
            );
            assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Deletion));
            assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Addition));
        } else if diff.target_path == mixed.mocha_conf_path {
            modify_count += 1;
            assert_eq!(diff.action, FileDiffAction::Modify);
            assert_eq!(
                diff.planned_backup_path,
                Some(env.backups.join(&mixed.mocha_conf_hash))
            );
            assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Deletion));
            assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Addition));
        } else if diff.target_path == mixed.waybar_config_path {
            modify_count += 1;
            assert_eq!(diff.action, FileDiffAction::Modify);
            assert_eq!(
                diff.planned_backup_path,
                Some(env.backups.join(&mixed.waybar_config_hash))
            );
            assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Deletion));
            assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Addition));
        } else if diff.target_path == mixed.waybar_style_path {
            modify_count += 1;
            assert_eq!(diff.action, FileDiffAction::Modify);
            assert_eq!(
                diff.planned_backup_path,
                Some(env.backups.join(&mixed.waybar_style_hash))
            );
            assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Deletion));
            assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Addition));
        } else {
            create_count += 1;
            assert_eq!(diff.action, FileDiffAction::Create);
            assert!(diff.planned_backup_path.is_none());
            assert!(diff.lines.iter().all(|l| l.kind == DiffLineKind::Addition));
            assert!(diff.lines.iter().all(|l| l.kind != DiffLineKind::Deletion));
        }
    }

    assert_eq!(modify_count, 4, "Must classify exactly 4 files as [MODIFY]");
    assert_eq!(create_count, 5, "Must classify exactly 5 files as [CREATE]");

    // STRICT READ-ONLY GUARANTEES:
    // 1. All 4 pre-existing files still match their original bytes exactly
    assert_eq!(
        fs::read(&mixed.hyprland_conf_path).unwrap(),
        mixed.hyprland_conf_bytes
    );
    assert_eq!(
        fs::read(&mixed.mocha_conf_path).unwrap(),
        mixed.mocha_conf_bytes
    );
    assert_eq!(
        fs::read(&mixed.waybar_config_path).unwrap(),
        mixed.waybar_config_bytes
    );
    assert_eq!(
        fs::read(&mixed.waybar_style_path).unwrap(),
        mixed.waybar_style_bytes
    );

    // 2. All 5 non-existent files STILL do not exist
    assert!(!mixed.hyprlock_conf_path.exists());
    assert!(!mixed.hypridle_conf_path.exists());
    assert!(!mixed.macchiato_conf_path.exists());
    assert!(!mixed.frappe_conf_path.exists());
    assert!(!mixed.latte_conf_path.exists());

    // 3. Predicted backup paths do NOT exist on disk
    let predicted_hypr = env.backups.join(&mixed.hyprland_conf_hash);
    let predicted_mocha = env.backups.join(&mixed.mocha_conf_hash);
    let predicted_waybar_cfg = env.backups.join(&mixed.waybar_config_hash);
    let predicted_waybar_style = env.backups.join(&mixed.waybar_style_hash);

    assert!(
        !predicted_hypr.exists(),
        "Predicted backup must not be created during compute_plan"
    );
    assert!(
        !predicted_mocha.exists(),
        "Predicted backup must not be created during compute_plan"
    );
    assert!(
        !predicted_waybar_cfg.exists(),
        "Predicted backup must not be created during compute_plan"
    );
    assert!(
        !predicted_waybar_style.exists(),
        "Predicted backup must not be created during compute_plan"
    );

    let post_backup_entries = fs::read_dir(&env.backups).map(|r| r.count()).unwrap_or(0);
    assert_eq!(
        post_backup_entries, 0,
        "No backup files must be created by compute_plan"
    );

    // 4. Zero mutations in SQLite DB
    let post_txn: i64 = env
        .engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    let post_op: i64 = env
        .engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM operations", [], |r| r.get(0))
        .unwrap();
    let post_bkp: i64 = env
        .engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM backup_snapshots", [], |r| r.get(0))
        .unwrap();
    let post_pkg: i64 = env
        .engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        post_txn, 0,
        "No transactions must be recorded by compute_plan"
    );
    assert_eq!(post_op, 0, "No operations must be recorded by compute_plan");
    assert_eq!(
        post_bkp, 0,
        "No backup snapshots must be recorded by compute_plan"
    );
    assert_eq!(
        post_pkg, 0,
        "No installed packages must be recorded by compute_plan"
    );
}

// ===========================================================================
// Test 4: 7-stage atomic installation with content-addressed backups
// ===========================================================================
#[test]
fn test_04_seven_stage_atomic_installation_with_content_addressed_backups() {
    let env = create_test_env("m8-test4");
    let mixed = setup_mixed_hyprland_environment(&env.home);
    let pkg_dir = hyprland_pkg_dir();

    // Compute plan first to record predicted backup paths
    let plan = env.engine.compute_plan(&pkg_dir).unwrap();
    let predicted_backups: Vec<PathBuf> = plan
        .file_diffs
        .iter()
        .filter_map(|d| d.planned_backup_path.clone())
        .collect();
    assert_eq!(predicted_backups.len(), 4);

    let progress_events = Arc::new(Mutex::new(Vec::new()));
    let progress_events_c = Arc::clone(&progress_events);

    let cb: ProgressCallback = Box::new(move |stage, msg, frac| {
        progress_events_c
            .lock()
            .unwrap()
            .push((stage, msg.to_string(), frac));
    });

    let txn = env
        .engine
        .install_with_progress(&pkg_dir, "m8_install_verifier", Some(cb))
        .expect("install com.catppuccin.hyprland");
    assert!(!txn.is_empty());

    let recorded = progress_events.lock().unwrap().clone();
    assert!(!recorded.is_empty());

    // 1. Verify all 7 stages recorded monotonically
    let stages: Vec<InstallStage> = recorded.iter().map(|(s, _, _)| *s).collect();
    assert!(stages.contains(&InstallStage::Resolve));
    assert!(stages.contains(&InstallStage::Validate));
    assert!(stages.contains(&InstallStage::Plan));
    assert!(stages.contains(&InstallStage::Backup));
    assert!(stages.contains(&InstallStage::Execute));
    assert!(stages.contains(&InstallStage::Verify));
    assert!(stages.contains(&InstallStage::Commit));

    let fractions: Vec<f32> = recorded.iter().map(|(_, _, f)| *f).collect();
    for window in fractions.windows(2) {
        assert!(
            window[0] <= window[1] + 1e-6,
            "Progress fractions not monotonic: {} > {}",
            window[0],
            window[1]
        );
    }
    assert_eq!(*fractions.last().unwrap(), 1.0);

    // 2. Verify all 9 files exist on disk with artifact content and 0644 mode
    let manifest = Manifest::from_file(pkg_dir.join("woderrise.yaml")).unwrap();
    for file in &manifest.install.files {
        let target = expand_target(&file.target, &env.home).unwrap();
        assert!(
            target.is_file(),
            "Target file must exist: {}",
            target.display()
        );
        let actual_hash = sha256_file(&target).unwrap();
        let expected_hash = sha256_file(pkg_dir.join(&file.source)).unwrap();
        assert_eq!(
            actual_hash,
            expected_hash,
            "File content mismatch for {}",
            target.display()
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o644, "Mode must be 0644 for {}", target.display());
        }
    }

    // 3. Verify exactly 4 content-addressed backup files were created
    let backup_files: Vec<PathBuf> = fs::read_dir(&env.backups)
        .unwrap()
        .map(|r| r.unwrap().path())
        .collect();
    assert_eq!(backup_files.len(), 4, "Must have exactly 4 backup files");

    // Check each predicted backup matches the actual backup file on disk
    for predicted in &predicted_backups {
        assert!(
            backup_files.contains(predicted),
            "Actual backup store missing predicted path: {}",
            predicted.display()
        );
        assert!(predicted.is_file());
    }

    // Verify backup contents match original bytes byte-for-byte
    assert_eq!(
        fs::read(env.backups.join(&mixed.hyprland_conf_hash)).unwrap(),
        mixed.hyprland_conf_bytes
    );
    assert_eq!(
        fs::read(env.backups.join(&mixed.mocha_conf_hash)).unwrap(),
        mixed.mocha_conf_bytes
    );
    assert_eq!(
        fs::read(env.backups.join(&mixed.waybar_config_hash)).unwrap(),
        mixed.waybar_config_bytes
    );
    assert_eq!(
        fs::read(env.backups.join(&mixed.waybar_style_hash)).unwrap(),
        mixed.waybar_style_bytes
    );
}

// ===========================================================================
// Test 5: SQLite WAL journal consistency, foreign keys & table invariants
// ===========================================================================
#[test]
fn test_05_sqlite_wal_journal_consistency_and_table_invariants() {
    let env = create_test_env("m8-test5");
    let mixed = setup_mixed_hyprland_environment(&env.home);
    let pkg_dir = hyprland_pkg_dir();

    // 1. SQLite Pragmas
    let journal_mode: String = env
        .engine
        .db
        .conn
        .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        journal_mode.to_lowercase(),
        "wal",
        "Database must operate in WAL mode"
    );

    let fk: i32 = env
        .engine
        .db
        .conn
        .query_row("PRAGMA foreign_keys;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1, "Foreign keys must be enabled");

    let integrity: String = env
        .engine
        .db
        .conn
        .query_row("PRAGMA integrity_check;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        integrity.to_lowercase(),
        "ok",
        "Database integrity check must pass"
    );

    // Install package
    let txn = env
        .engine
        .install(&pkg_dir, "m8_wal_auditor")
        .expect("install");

    // Re-verify integrity post-install
    let post_integrity: String = env
        .engine
        .db
        .conn
        .query_row("PRAGMA integrity_check;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        post_integrity.to_lowercase(),
        "ok",
        "Database integrity check post-install must pass"
    );

    // Audit `transactions` table:
    let (t_id, pkg_id, ver, t_type, status, started, completed, initiator, error): (
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        String,
        Option<String>,
    ) = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT txn_id, package_id, version, txn_type, status, started_at, completed_at, initiator, error_message FROM transactions WHERE txn_id = ?1",
            [&txn],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
        )
        .unwrap();
    assert_eq!(t_id, txn);
    assert_eq!(pkg_id, "com.catppuccin.hyprland");
    assert_eq!(ver, "1.0.0");
    assert_eq!(t_type, "INSTALL");
    assert_eq!(status, "COMMITTED");
    assert!(started > 0);
    assert!(completed.is_some() && completed.unwrap() >= started);
    assert_eq!(initiator, "m8_wal_auditor");
    assert!(error.is_none());

    struct OperationRow {
        _op_id: String,
        sequence_num: i64,
        op_type: String,
        target_path: String,
        pre_sha256: Option<String>,
        _post_sha256: String,
        backup_id: Option<String>,
        status: String,
    }

    // Audit `operations` table: exactly 9 operations
    let mut stmt = env
        .engine
        .db
        .conn
        .prepare("SELECT op_id, sequence_num, op_type, target_path, pre_sha256, post_sha256, backup_id, status FROM operations WHERE txn_id = ?1 ORDER BY sequence_num ASC")
        .unwrap();
    let ops: Vec<OperationRow> = stmt
        .query_map([&txn], |r| {
            Ok(OperationRow {
                _op_id: r.get(0)?,
                sequence_num: r.get(1)?,
                op_type: r.get(2)?,
                target_path: r.get(3)?,
                pre_sha256: r.get(4)?,
                _post_sha256: r.get(5)?,
                backup_id: r.get(6)?,
                status: r.get(7)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(ops.len(), 9, "Must have exactly 9 operations");
    for (i, row) in ops.iter().enumerate() {
        assert_eq!(
            row.sequence_num,
            (i + 1) as i64,
            "Operations must have monotonic sequence numbers"
        );
        assert_eq!(
            row.status, "COMPLETED",
            "All operations must have status COMPLETED"
        );
    }

    let modify_ops: Vec<&OperationRow> = ops
        .iter()
        .filter(|row| row.op_type == "MODIFY_FILE")
        .collect();
    let create_ops: Vec<&OperationRow> = ops
        .iter()
        .filter(|row| row.op_type == "CREATE_FILE")
        .collect();
    assert_eq!(modify_ops.len(), 4, "Must have 4 MODIFY_FILE operations");
    assert_eq!(create_ops.len(), 5, "Must have 5 CREATE_FILE operations");

    for row in &modify_ops {
        assert!(row.pre_sha256.is_some(), "MODIFY_FILE must record pre_sha256");
        assert!(row.backup_id.is_some(), "MODIFY_FILE must record backup_id");
        let path = PathBuf::from(&row.target_path);
        if path == mixed.hyprland_conf_path {
            assert_eq!(row.pre_sha256.as_deref(), Some(mixed.hyprland_conf_hash.as_str()));
        } else if path == mixed.mocha_conf_path {
            assert_eq!(row.pre_sha256.as_deref(), Some(mixed.mocha_conf_hash.as_str()));
        } else if path == mixed.waybar_config_path {
            assert_eq!(row.pre_sha256.as_deref(), Some(mixed.waybar_config_hash.as_str()));
        } else if path == mixed.waybar_style_path {
            assert_eq!(row.pre_sha256.as_deref(), Some(mixed.waybar_style_hash.as_str()));
        }
    }

    for row in &create_ops {
        assert!(row.pre_sha256.is_none(), "CREATE_FILE must have None pre_sha256");
        assert!(row.backup_id.is_none(), "CREATE_FILE must have None backup_id");
    }

    struct SnapshotRow {
        backup_id: String,
        txn_id: String,
        original_path: String,
        storage_path: String,
        sha256: String,
        file_mode: u32,
    }

    // Audit `backup_snapshots` table: exactly 4 rows
    let mut bkp_stmt = env
        .engine
        .db
        .conn
        .prepare("SELECT backup_id, txn_id, original_path, storage_path, sha256, file_mode FROM backup_snapshots WHERE txn_id = ?1")
        .unwrap();
    let snapshots: Vec<SnapshotRow> = bkp_stmt
        .query_map([&txn], |r| {
            Ok(SnapshotRow {
                backup_id: r.get(0)?,
                txn_id: r.get(1)?,
                original_path: r.get(2)?,
                storage_path: r.get(3)?,
                sha256: r.get(4)?,
                file_mode: r.get(5)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(snapshots.len(), 4, "Must have 4 backup snapshots recorded");
    for row in snapshots {
        assert!(row.backup_id.starts_with("BKP-"), "Backup ID must start with BKP-");
        assert_eq!(row.txn_id, txn);
        assert_eq!(row.file_mode, 0o644);
        assert!(Path::new(&row.storage_path).is_file());
        assert_eq!(sha256_file(&row.storage_path).unwrap(), row.sha256);
        assert!(
            row.original_path == mixed.hyprland_conf_path.to_string_lossy()
                || row.original_path == mixed.mocha_conf_path.to_string_lossy()
                || row.original_path == mixed.waybar_config_path.to_string_lossy()
                || row.original_path == mixed.waybar_style_path.to_string_lossy()
        );
    }

    // Audit `installed_packages` table: exactly 1 row
    let (inst_id, inst_ver, inst_type, inst_txn): (String, String, String, String) = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT package_id, active_version, package_type, install_txn_id FROM installed_packages WHERE package_id = ?1",
            ["com.catppuccin.hyprland"],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(inst_id, "com.catppuccin.hyprland");
    assert_eq!(inst_ver, "1.0.0");
    assert_eq!(inst_type, "config");
    assert_eq!(inst_txn, txn);
}

// ===========================================================================
// Test 6: Non-destructive coexistence: external modification pre-flight conflict detection
// ===========================================================================
#[test]
fn test_06_nondestructive_coexistence_external_edit_aborts_rollback_as_conflict() {
    let env = create_test_env("m8-test6");
    let mixed = setup_mixed_hyprland_environment(&env.home);
    let pkg_dir = hyprland_pkg_dir();

    // 1. Install package
    let txn = env
        .engine
        .install(&pkg_dir, "m8_conflict_verifier")
        .unwrap();

    // 2. Simulate external edit of ~/.config/hypr/hyprland.conf by user
    let user_edited_content = b"# Hyprland configuration file with Catppuccin theme integration\n\
# User custom hotkey added post-installation\n\
bind = SUPER, RETURN, exec, foot\n\
windowrule = float, ^(pavucontrol)$\n"
        .to_vec();
    fs::write(&mixed.hyprland_conf_path, &user_edited_content).unwrap();
    let tampered_hash = sha256_bytes(&user_edited_content);

    // Record hashes of other 8 files before attempting rollback
    let manifest = Manifest::from_file(pkg_dir.join("woderrise.yaml")).unwrap();
    let mut other_hashes = Vec::new();
    for f in &manifest.install.files {
        let target = expand_target(&f.target, &env.home).unwrap();
        if target != mixed.hyprland_conf_path {
            other_hashes.push((target.clone(), sha256_file(&target).unwrap()));
        }
    }
    assert_eq!(other_hashes.len(), 8);

    // 3. Attempt unforced rollback (force = false)
    let rollback_result = env.engine.rollback(&txn, false);

    // 4. Must fail fast with WoderriseError::Conflict
    match rollback_result {
        Err(WoderriseError::Conflict(conflict_path)) => {
            assert_eq!(
                conflict_path,
                mixed.hyprland_conf_path.to_string_lossy(),
                "Conflict must pinpoint ~/.config/hypr/hyprland.conf"
            );
        }
        other => panic!("Expected Err(WoderriseError::Conflict), got: {:?}", other),
    }

    // 5. ZERO DISK MUTATION GUARANTEE:
    // User-edited file must remain completely untouched with user's custom changes
    assert_eq!(
        fs::read(&mixed.hyprland_conf_path).unwrap(),
        user_edited_content,
        "User-edited file must NOT be altered on conflict abort"
    );
    assert_eq!(
        sha256_file(&mixed.hyprland_conf_path).unwrap(),
        tampered_hash
    );

    // The other 8 files must remain untouched
    for (target, expected_hash) in &other_hashes {
        assert!(
            target.is_file(),
            "File must still exist: {}",
            target.display()
        );
        assert_eq!(sha256_file(target).unwrap(), *expected_hash);
    }

    // The 4 backup snapshots must still exist in backup store
    assert!(env.backups.join(&mixed.hyprland_conf_hash).is_file());
    assert!(env.backups.join(&mixed.mocha_conf_hash).is_file());
    assert!(env.backups.join(&mixed.waybar_config_hash).is_file());
    assert!(env.backups.join(&mixed.waybar_style_hash).is_file());

    // Original transaction in SQLite DB remains COMMITTED
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
    assert_eq!(
        status, "COMMITTED",
        "Transaction status must remain COMMITTED"
    );

    // Package remains installed in installed_packages
    let pkg_exists: i64 = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.catppuccin.hyprland'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        pkg_exists, 1,
        "Package must remain active in installed_packages"
    );

    // No orphan rollback transactions created in DB
    let rollback_txns: i64 = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE txn_type = 'ROLLBACK'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        rollback_txns, 0,
        "No rollback transaction records must be committed on abort"
    );
}

// ===========================================================================
// Test 7: 100% reversible forced rollback (force = true)
// ===========================================================================
#[test]
fn test_07_reversible_forced_rollback_byte_for_byte_restoration() {
    let env = create_test_env("m8-test7");
    let mixed = setup_mixed_hyprland_environment(&env.home);
    let pkg_dir = hyprland_pkg_dir();

    // 1. Install package
    let txn = env
        .engine
        .install(&pkg_dir, "m8_forced_rb_verifier")
        .unwrap();

    // Tamper with hyprland.conf (user edit)
    fs::write(
        &mixed.hyprland_conf_path,
        b"# User edited post install\nwindowrule=float,^(kitty)$",
    )
    .unwrap();

    // 2. Execute forced rollback
    let rb_txn = env
        .engine
        .rollback(&txn, true)
        .expect("Forced rollback must succeed");
    assert!(
        !rb_txn.is_empty(),
        "Rollback transaction ID must not be empty"
    );

    // 3. 100% BYTE-FOR-BYTE RESTORATION of all 4 pre-existing files:
    // hyprland.conf
    assert!(mixed.hyprland_conf_path.is_file());
    assert_eq!(
        fs::read(&mixed.hyprland_conf_path).unwrap(),
        mixed.hyprland_conf_bytes,
        "hyprland.conf must be restored byte-for-byte to pre-installation state"
    );
    assert_eq!(
        sha256_file(&mixed.hyprland_conf_path).unwrap(),
        mixed.hyprland_conf_hash
    );

    // mocha.conf
    assert!(mixed.mocha_conf_path.is_file());
    assert_eq!(
        fs::read(&mixed.mocha_conf_path).unwrap(),
        mixed.mocha_conf_bytes,
        "mocha.conf must be restored byte-for-byte"
    );
    assert_eq!(
        sha256_file(&mixed.mocha_conf_path).unwrap(),
        mixed.mocha_conf_hash
    );

    // waybar config.jsonc
    assert!(mixed.waybar_config_path.is_file());
    assert_eq!(
        fs::read(&mixed.waybar_config_path).unwrap(),
        mixed.waybar_config_bytes,
        "waybar config.jsonc must be restored byte-for-byte"
    );
    assert_eq!(
        sha256_file(&mixed.waybar_config_path).unwrap(),
        mixed.waybar_config_hash
    );

    // waybar style.css
    assert!(mixed.waybar_style_path.is_file());
    assert_eq!(
        fs::read(&mixed.waybar_style_path).unwrap(),
        mixed.waybar_style_bytes,
        "waybar style.css must be restored byte-for-byte"
    );
    assert_eq!(
        sha256_file(&mixed.waybar_style_path).unwrap(),
        mixed.waybar_style_hash
    );

    // 4. CLEAN REMOVAL of all 5 newly created files:
    assert!(
        !mixed.hyprlock_conf_path.exists(),
        "hyprlock.conf must be cleanly deleted"
    );
    assert!(
        !mixed.hypridle_conf_path.exists(),
        "hypridle.conf must be cleanly deleted"
    );
    assert!(
        !mixed.macchiato_conf_path.exists(),
        "macchiato.conf must be cleanly deleted"
    );
    assert!(
        !mixed.frappe_conf_path.exists(),
        "frappe.conf must be cleanly deleted"
    );
    assert!(
        !mixed.latte_conf_path.exists(),
        "latte.conf must be cleanly deleted"
    );

    // 5. SQLite WAL journal state verification:
    // Original transaction status is ROLLED_BACK
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
    assert_eq!(
        orig_status, "ROLLED_BACK",
        "Original transaction must be ROLLED_BACK"
    );

    // Rollback transaction status is COMMITTED
    let (rb_type, rb_status): (String, String) = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT txn_type, status FROM transactions WHERE txn_id = ?1",
            [&rb_txn],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(rb_type, "ROLLBACK");
    assert_eq!(rb_status, "COMMITTED");

    // Package entry is completely purged from installed_packages
    let pkg_rows: i64 = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.catppuccin.hyprland'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        pkg_rows, 0,
        "Package must be purged from installed_packages"
    );

    // Re-attempting rollback on already rolled back transaction must be rejected
    let repeat_rb = env.engine.rollback(&txn, true);
    assert!(
        repeat_rb.is_err(),
        "Cannot rollback already rolled back transaction"
    );
    let err_msg = repeat_rb.unwrap_err().to_string();
    assert!(err_msg.contains("ROLLED_BACK"));
}

// ===========================================================================
// Test 8: Interrupted transaction crash recovery & compensation
// ===========================================================================
#[test]
fn test_08_interrupted_transaction_crash_recovery_and_compensation() {
    let env = create_test_env("m8-test8");
    let mixed = setup_mixed_hyprland_environment(&env.home);

    // Simulate an interrupted installation transaction for com.catppuccin.hyprland:
    let crash_txn = env
        .engine
        .db
        .begin_transaction(
            "com.catppuccin.hyprland",
            "1.0.0",
            "INSTALL",
            "crash_sim_initiator",
        )
        .unwrap();

    // 1. Snapshot and modify 2 pre-existing files (hyprland.conf and waybar/config.jsonc)
    let snap_hypr = env
        .engine
        .backups
        .snapshot(&mixed.hyprland_conf_path, &crash_txn, &env.engine.db)
        .unwrap();
    let new_hypr_bytes = b"# Interrupted installation payload for hyprland.conf";
    fs::write(&mixed.hyprland_conf_path, new_hypr_bytes).unwrap();
    let new_hypr_hash = sha256_bytes(new_hypr_bytes);

    let snap_waybar = env
        .engine
        .backups
        .snapshot(&mixed.waybar_config_path, &crash_txn, &env.engine.db)
        .unwrap();
    let new_waybar_bytes = b"{\"interrupted\": \"config.jsonc\"}";
    fs::write(&mixed.waybar_config_path, new_waybar_bytes).unwrap();
    let new_waybar_hash = sha256_bytes(new_waybar_bytes);

    // 2. Create 2 new files on disk (hyprlock.conf and latte.conf)
    let new_hyprlock_bytes = b"# Interrupted hyprlock payload";
    fs::write(&mixed.hyprlock_conf_path, new_hyprlock_bytes).unwrap();
    let new_hyprlock_hash = sha256_bytes(new_hyprlock_bytes);

    let new_latte_bytes = b"# Interrupted latte palette";
    fs::write(&mixed.latte_conf_path, new_latte_bytes).unwrap();
    let new_latte_hash = sha256_bytes(new_latte_bytes);

    // 3. Record COMPLETED operations in database for all 4
    env.engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-M8-1', ?1, 1, 'MODIFY_FILE', ?2, ?3, ?4, ?5, 'COMPLETED', NULL)",
        params![crash_txn, mixed.hyprland_conf_path.to_string_lossy(), mixed.hyprland_conf_hash, new_hypr_hash, snap_hypr],
    ).unwrap();

    env.engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-M8-2', ?1, 2, 'CREATE_FILE', ?2, NULL, ?3, NULL, 'COMPLETED', NULL)",
        params![crash_txn, mixed.hyprlock_conf_path.to_string_lossy(), new_hyprlock_hash],
    ).unwrap();

    env.engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-M8-3', ?1, 3, 'MODIFY_FILE', ?2, ?3, ?4, ?5, 'COMPLETED', NULL)",
        params![crash_txn, mixed.waybar_config_path.to_string_lossy(), mixed.waybar_config_hash, new_waybar_hash, snap_waybar],
    ).unwrap();

    env.engine.db.conn.execute(
        "INSERT INTO operations VALUES('OP-M8-4', ?1, 4, 'CREATE_FILE', ?2, NULL, ?3, NULL, 'COMPLETED', NULL)",
        params![crash_txn, mixed.latte_conf_path.to_string_lossy(), new_latte_hash],
    ).unwrap();

    // 4. Set transaction status to EXECUTING (crash occurred before Commit)
    env.engine
        .db
        .set_status(&crash_txn, "EXECUTING", None)
        .unwrap();

    // 5. Insert orphan row into installed_packages (crash during commit race)
    env.engine.db.conn.execute(
        "INSERT INTO installed_packages(package_id, active_version, package_type, installed_at, updated_at, is_explicit, ref_count, install_txn_id) VALUES(?1, '1.0.0', 'config', 0, 0, 1, 1, ?2)",
        params!["com.catppuccin.hyprland", crash_txn],
    ).unwrap();

    // 6. Another transaction left in PLANNED state
    let planned_txn = env
        .engine
        .db
        .begin_transaction(
            "com.catppuccin.hyprland.extra",
            "0.9.0",
            "INSTALL",
            "orphan_planner",
        )
        .unwrap();

    // Pre-recovery assertions: files are in dirty/corrupted state
    assert_ne!(
        fs::read(&mixed.hyprland_conf_path).unwrap(),
        mixed.hyprland_conf_bytes
    );
    assert_ne!(
        fs::read(&mixed.waybar_config_path).unwrap(),
        mixed.waybar_config_bytes
    );
    assert!(mixed.hyprlock_conf_path.exists());
    assert!(mixed.latte_conf_path.exists());

    // 7. Execute Crash Recovery
    let recovered_count = env.engine.recover().expect("Crash recovery must succeed");
    assert_eq!(
        recovered_count, 2,
        "Must recover both EXECUTING and PLANNED transactions"
    );

    // 8. Verify compensation of filesystem:
    // Modified files restored to original bytes from backup snapshots:
    assert_eq!(
        fs::read(&mixed.hyprland_conf_path).unwrap(),
        mixed.hyprland_conf_bytes,
        "hyprland.conf must be restored to original bytes by recovery compensation"
    );
    assert_eq!(
        sha256_file(&mixed.hyprland_conf_path).unwrap(),
        mixed.hyprland_conf_hash
    );

    assert_eq!(
        fs::read(&mixed.waybar_config_path).unwrap(),
        mixed.waybar_config_bytes,
        "waybar config.jsonc must be restored to original bytes by recovery compensation"
    );
    assert_eq!(
        sha256_file(&mixed.waybar_config_path).unwrap(),
        mixed.waybar_config_hash
    );

    // Created files removed from disk:
    assert!(
        !mixed.hyprlock_conf_path.exists(),
        "hyprlock.conf must be removed on crash recovery"
    );
    assert!(
        !mixed.latte_conf_path.exists(),
        "latte.conf must be removed on crash recovery"
    );

    // Untouched files remain untouched:
    assert_eq!(
        fs::read(&mixed.mocha_conf_path).unwrap(),
        mixed.mocha_conf_bytes
    );
    assert_eq!(
        fs::read(&mixed.waybar_style_path).unwrap(),
        mixed.waybar_style_bytes
    );

    // 9. Verify SQLite DB cleanups:
    // Orphan installed_packages row removed
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
    assert_eq!(
        pkg_count, 0,
        "Orphan installed_packages record must be removed"
    );

    // Transaction statuses transitioned to ABORTED_RECOVERED
    let st1: String = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            [&crash_txn],
            |r| r.get(0),
        )
        .unwrap();
    let st2: String = env
        .engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id = ?1",
            [&planned_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(st1, "ABORTED_RECOVERED");
    assert_eq!(st2, "ABORTED_RECOVERED");

    // 10. Idempotence: Second recover() call is a no-op returning 0
    let second_recover = env.engine.recover().expect("Second recovery must succeed");
    assert_eq!(
        second_recover, 0,
        "Second recovery call must be idempotent no-op"
    );
}
