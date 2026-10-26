//! Complete integration and verification test suite for Milestone M6.
//! Validates manifest structure, artifact hashes, SQLite FTS5 ingestion, diff planning, and rollback.

use std::{fs, path::PathBuf};
use woderrise_core::{
    catalog, diff::FileDiffAction, expand_target, sha256_file, BackupManager, Database,
    DiffLineKind, InstallationEngine, Manifest,
};

fn registry_packages_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages")
}

fn hyprland_pkg_dir() -> PathBuf {
    registry_packages_dir().join("com.catppuccin.hyprland/1.0.0")
}

fn create_temp_env(name: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join(format!("home_{name}"));
    let backups = tmp.path().join(format!("backups_{name}"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&backups).unwrap();
    (tmp, home, backups)
}

#[test]
fn test_hyprland_manifest_structure_and_validation() {
    let pkg_dir = hyprland_pkg_dir();
    let manifest_path = pkg_dir.join("woderrise.yaml");
    assert!(
        manifest_path.exists(),
        "Manifest file must exist at {}",
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

    // Previews palette: must declare 10 authentic colors across Mocha, Macchiato, Frappe, Latte
    let previews = manifest.previews.as_ref().expect("Previews must be present");
    assert_eq!(previews.palette.len(), 10, "Must declare 10 palette swatches");
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
    // Preview thumbnail points to existing relative file
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
    let sys_dep_names: Vec<&str> = deps.system.iter().map(|s| s.name.as_str()).collect();
    assert!(sys_dep_names.contains(&"hyprland"));
    assert!(sys_dep_names.contains(&"waybar"));
    assert!(sys_dep_names.contains(&"hyprlock"));
    assert!(sys_dep_names.contains(&"hypridle"));

    // Exactly 9 artifacts and 9 install files
    assert_eq!(manifest.artifacts.len(), 9, "Must contain exactly 9 artifacts");
    assert_eq!(manifest.install.files.len(), 9, "Must install 9 config files");
    for file in &manifest.install.files {
        assert_eq!(file.mode.as_deref(), Some("0644"));
        assert_eq!(file.strategy.as_deref(), Some("replace"));
        assert!(file.backup_before_write);
        assert!(
            file.target.starts_with("~/.config/hypr/")
                || file.target.starts_with("~/.config/waybar/")
        );
    }
}

#[test]
fn test_hyprland_artifact_hash_verification() {
    let pkg_dir = hyprland_pkg_dir();
    let manifest_path = pkg_dir.join("woderrise.yaml");
    let manifest = Manifest::from_file(&manifest_path).unwrap();

    // Verify through core verification engine
    catalog::verify_artifacts(&manifest, &pkg_dir)
        .expect("Core artifact verification must succeed");

    // Manually verify every single artifact hash
    for artifact in &manifest.artifacts {
        let path = pkg_dir.join(&artifact.path);
        assert!(
            path.is_file(),
            "Artifact file does not exist: {}",
            path.display()
        );
        let computed = sha256_file(&path).expect("Failed to compute SHA256");
        assert_eq!(
            computed, artifact.sha256,
            "SHA256 mismatch for {}",
            artifact.path
        );
    }

    // Verify contents of configuration payloads are authentic and non-empty
    let hyprland_conf = fs::read_to_string(pkg_dir.join("artifacts/hyprland.conf")).unwrap();
    assert!(hyprland_conf.contains("monitor =") || hyprland_conf.contains("monitor="));
    assert!(hyprland_conf.contains("exec-once = waybar"));
    assert!(hyprland_conf.contains("source = ~/.config/hypr/mocha.conf"));

    let hyprlock_conf = fs::read_to_string(pkg_dir.join("artifacts/hyprlock.conf")).unwrap();
    assert!(hyprlock_conf.contains("background {"));
    assert!(hyprlock_conf.contains("input-field {"));

    let hypridle_conf = fs::read_to_string(pkg_dir.join("artifacts/hypridle.conf")).unwrap();
    assert!(hypridle_conf.contains("listener {"));
    assert!(hypridle_conf.contains("timeout ="));

    for flavor in &["mocha.conf", "macchiato.conf", "frappe.conf", "latte.conf"] {
        let content = fs::read_to_string(pkg_dir.join(format!("artifacts/{flavor}"))).unwrap();
        assert!(content.contains("$base = rgb("));
        assert!(content.contains("$mauve = rgb("));
    }

    let waybar_cfg = fs::read_to_string(pkg_dir.join("artifacts/waybar-config.jsonc")).unwrap();
    assert!(waybar_cfg.contains("hyprland/workspaces"));

    let waybar_style = fs::read_to_string(pkg_dir.join("artifacts/waybar-style.css")).unwrap();
    assert!(waybar_style.contains("@define-color") || waybar_style.contains("#waybar"));
}

#[test]
fn test_hyprland_sqlite_catalog_fts5_ingestion_and_search() {
    let reg_dir = registry_packages_dir();
    let db = Database::in_memory().expect("in-memory db");
    catalog::migrate(&db).expect("catalog migration");

    let count = catalog::import_registry(&db, &reg_dir).expect("registry import");
    assert_eq!(
        count, 2,
        "Both com.catppuccin.waybar and com.catppuccin.hyprland must be indexed"
    );

    // Exact catalog record lookup
    let entry =
        catalog::get(&db, "com.catppuccin.hyprland", "1.0.0").expect("get hyprland entry");
    assert_eq!(entry.id, "com.catppuccin.hyprland");
    assert_eq!(entry.version, "1.0.0");
    assert_eq!(entry.category, "desktop_setups");
    assert_eq!(entry.license, "MIT");
    assert!(entry.validated);
    assert!(!entry.yanked);

    let manifest_path = hyprland_pkg_dir().join("woderrise.yaml");
    assert_eq!(entry.manifest_sha256, sha256_file(&manifest_path).unwrap());

    // Manifest rehydration via catalog
    let loaded_manifest =
        catalog::manifest(&db, "com.catppuccin.hyprland", "1.0.0").expect("catalog::manifest");
    assert_eq!(loaded_manifest.package.id, "com.catppuccin.hyprland");

    // FTS5 Search Queries
    let res = catalog::search(&db, "hyprland", 10).unwrap();
    assert!(res.iter().any(|e| e.id == "com.catppuccin.hyprland"));

    let res = catalog::search(&db, "desktop_setups", 10).unwrap();
    assert!(res.iter().any(|e| e.id == "com.catppuccin.hyprland"));

    let res = catalog::search(&db, "catppuccin", 10).unwrap();
    assert_eq!(res.len(), 2, "Catppuccin query must match both packages");

    let res = catalog::search(&db, "mocha", 10).unwrap();
    assert_eq!(res.len(), 2, "Mocha query must match both packages");

    let res = catalog::search(&db, "latte", 10).unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "com.catppuccin.hyprland");

    let res = catalog::search(&db, "hyprlock", 10).unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "com.catppuccin.hyprland");

    let res = catalog::search(&db, "hypridle", 10).unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "com.catppuccin.hyprland");

    let res = catalog::search(&db, "nonexistent_query_term_123", 10).unwrap();
    assert!(res.is_empty());
}

#[test]
fn test_hyprland_diff_and_install_plan() {
    let pkg_dir = hyprland_pkg_dir();
    let (_tmp, home, backups) = create_temp_env("diff-plan");
    let db = Database::in_memory().unwrap();

    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(&backups).unwrap(),
        home: home.clone(),
    };

    // --- Scenario 1: Clean Installation (No targets exist) ---
    let plan = engine.compute_plan(&pkg_dir).expect("compute_plan clean");
    assert_eq!(plan.package_id, "com.catppuccin.hyprland");
    assert_eq!(plan.version, "1.0.0");
    assert_eq!(plan.file_diffs.len(), 9);
    assert_eq!(plan.will_backup_count, 0);

    for diff in &plan.file_diffs {
        assert_eq!(diff.action, FileDiffAction::Create);
        assert!(diff.planned_backup_path.is_none());
        assert!(!diff.lines.is_empty());
        assert!(diff.lines.iter().all(|l| l.kind == DiffLineKind::Addition));
    }

    // --- Scenario 2: Overwriting Pre-existing Configurations ---
    let pre_hypr_path = home.join(".config/hypr/hyprland.conf");
    fs::create_dir_all(pre_hypr_path.parent().unwrap()).unwrap();
    let old_hypr_bytes = b"# Pre-existing user custom hyprland config\nmonitor=,preferred,auto,1\n";
    fs::write(&pre_hypr_path, old_hypr_bytes).unwrap();
    let expected_hypr_backup_hash = sha256_file(&pre_hypr_path).unwrap();

    let pre_waybar_path = home.join(".config/waybar/config.jsonc");
    fs::create_dir_all(pre_waybar_path.parent().unwrap()).unwrap();
    let old_waybar_bytes = b"{\n  \"custom\": \"existing-bar-config\"\n}\n";
    fs::write(&pre_waybar_path, old_waybar_bytes).unwrap();
    let expected_waybar_backup_hash = sha256_file(&pre_waybar_path).unwrap();

    let plan2 = engine
        .compute_plan(&pkg_dir)
        .expect("compute_plan with existing files");
    assert_eq!(plan2.will_backup_count, 2);

    let hypr_diff = plan2
        .file_diffs
        .iter()
        .find(|d| d.target_path == pre_hypr_path)
        .unwrap();
    assert_eq!(hypr_diff.action, FileDiffAction::Modify);
    assert_eq!(
        hypr_diff.planned_backup_path,
        Some(backups.join(&expected_hypr_backup_hash))
    );
    assert!(hypr_diff
        .lines
        .iter()
        .any(|l| l.kind == DiffLineKind::Deletion));
    assert!(hypr_diff
        .lines
        .iter()
        .any(|l| l.kind == DiffLineKind::Addition));

    let waybar_diff = plan2
        .file_diffs
        .iter()
        .find(|d| d.target_path == pre_waybar_path)
        .unwrap();
    assert_eq!(waybar_diff.action, FileDiffAction::Modify);
    assert_eq!(
        waybar_diff.planned_backup_path,
        Some(backups.join(&expected_waybar_backup_hash))
    );

    // --- Scenario 3: Execution and Rollback Verification ---
    let txn = engine
        .install(&pkg_dir, "test-m6-verifier")
        .expect("install com.catppuccin.hyprland");

    // Verify planned backup files actually exist on disk with exact pre-existing bytes
    let backed_up_hypr = backups.join(&expected_hypr_backup_hash);
    assert!(
        backed_up_hypr.is_file(),
        "Backup file must exist at {}",
        backed_up_hypr.display()
    );
    assert_eq!(fs::read(&backed_up_hypr).unwrap(), old_hypr_bytes);

    let backed_up_waybar = backups.join(&expected_waybar_backup_hash);
    assert!(
        backed_up_waybar.is_file(),
        "Backup file must exist at {}",
        backed_up_waybar.display()
    );
    assert_eq!(fs::read(&backed_up_waybar).unwrap(), old_waybar_bytes);

    // Verify all 9 files now exist on disk with new artifact content
    for file in &Manifest::from_file(pkg_dir.join("woderrise.yaml"))
        .unwrap()
        .install
        .files
    {
        let target = expand_target(&file.target, &home).unwrap();
        assert!(target.is_file());
        assert_eq!(
            sha256_file(&target).unwrap(),
            sha256_file(pkg_dir.join(&file.source)).unwrap()
        );
    }

    // Verify transactional rollback restores original state
    let rollback_txn = engine.rollback(&txn, false).expect("rollback");
    assert!(!rollback_txn.is_empty());

    // Pre-existing files restored
    assert_eq!(fs::read(&pre_hypr_path).unwrap(), old_hypr_bytes);
    assert_eq!(fs::read(&pre_waybar_path).unwrap(), old_waybar_bytes);

    // Files that did not exist before should be removed
    let hyprlock_path = home.join(".config/hypr/hyprlock.conf");
    assert!(
        !hyprlock_path.exists(),
        "Newly created file must be removed on rollback"
    );
}
