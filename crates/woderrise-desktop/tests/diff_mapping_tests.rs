use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use woderrise_core::{BackupManager, Database, InstallationEngine};
use woderrise_desktop::view_model::{map_install_plan, InstallPlanViewModel};

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

fn create_test_env(prefix: &str) -> (TempDirGuard, PathBuf, PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("woderrise-desktop-test-{prefix}-{nonce}"));
    let home = root.join("home");
    let data = root.join("data");
    fs::create_dir_all(&home).expect("failed to create temp home");
    fs::create_dir_all(&data).expect("failed to create temp data");
    (TempDirGuard(root), home, data)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn test_diff_mapping_clean_install_create_action() {
    let (_guard, home, data) = create_test_env("diff-create");
    let db = Database::open(data.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups")).unwrap(),
        home: home.clone(),
    };

    let ws = workspace_root();
    let pkg_dir = ws.join("registry/packages/com.catppuccin.waybar/1.0.0");
    let plan = engine.compute_plan(&pkg_dir).expect("compute_plan failed");

    let vm: InstallPlanViewModel = map_install_plan(&plan);

    assert_eq!(vm.package_id, "com.catppuccin.waybar");
    assert_eq!(vm.version, "1.0.0");
    assert_eq!(vm.file_diffs.len(), 1);

    let fd = &vm.file_diffs[0];
    assert_eq!(fd.action, "CREATE");
    assert_eq!(fd.planned_backup_path, "None (new file)");
    assert!(fd.has_changes);
    assert_eq!(fd.additions_count, 26); // 26 color definition lines added
    assert_eq!(fd.deletions_count, 0);
    assert_eq!(vm.will_backup_count, 0);
    assert!(vm.summary_text.contains("1 file affected"));
    assert!(vm.summary_text.contains("+26 / -0"));
}

#[test]
fn test_diff_mapping_existing_file_modify_action_and_backup_prediction() {
    let (_guard, home, data) = create_test_env("diff-modify");
    let db = Database::open(data.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups")).unwrap(),
        home: home.clone(),
    };

    // Create existing target file
    let target_file = home.join(".config/waybar/catppuccin-mocha.css");
    fs::create_dir_all(target_file.parent().unwrap()).unwrap();
    let existing_content =
        "/* Custom Old Waybar */\n@define-color base #000000;\n@define-color text #ffffff;\n";
    fs::write(&target_file, existing_content).unwrap();

    let ws = workspace_root();
    let pkg_dir = ws.join("registry/packages/com.catppuccin.waybar/1.0.0");
    let plan = engine.compute_plan(&pkg_dir).expect("compute_plan failed");

    let vm: InstallPlanViewModel = map_install_plan(&plan);

    assert_eq!(vm.file_diffs.len(), 1);
    let fd = &vm.file_diffs[0];
    assert_eq!(fd.action, "MODIFY");
    assert!(fd.has_changes);
    assert_ne!(fd.planned_backup_path, "None (new file)");
    assert!(fd.planned_backup_path.contains("backups"));
    assert_eq!(vm.will_backup_count, 1);
    assert!(fd.additions_count > 0);
    assert!(fd.deletions_count > 0);
    assert!(vm.summary_text.contains("1 pre-install backup planned"));
}

#[test]
fn test_diff_mapping_identical_file_unchanged_action() {
    let (_guard, home, data) = create_test_env("diff-unchanged");
    let db = Database::open(data.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups")).unwrap(),
        home: home.clone(),
    };

    let ws = workspace_root();
    let pkg_dir = ws.join("registry/packages/com.catppuccin.waybar/1.0.0");
    let source_artifact = pkg_dir.join("artifacts/mocha.css");
    let target_file = home.join(".config/waybar/catppuccin-mocha.css");
    fs::create_dir_all(target_file.parent().unwrap()).unwrap();
    fs::copy(&source_artifact, &target_file).unwrap();

    let plan = engine.compute_plan(&pkg_dir).expect("compute_plan failed");
    let vm: InstallPlanViewModel = map_install_plan(&plan);

    assert_eq!(vm.file_diffs.len(), 1);
    let fd = &vm.file_diffs[0];
    assert_eq!(fd.action, "UNCHANGED");
    assert!(!fd.has_changes);
    assert_eq!(fd.additions_count, 0);
    assert_eq!(fd.deletions_count, 0);
    assert_eq!(vm.will_backup_count, 1);
}
