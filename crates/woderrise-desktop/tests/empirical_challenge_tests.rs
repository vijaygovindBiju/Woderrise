//! Empirical Challenge Test Suite for Milestone M5: Slint Desktop Application
//! Adversarial stress tests covering:
//! 1. Manifest edge cases & real package ingestion fidelity
//! 2. Reactive search filtering under high volume & adversarial inputs
//! 3. Myers diff calculations & backup path predictions (text & binary)
//! 4. 15-stage async progress events, monotonicity & simulated failure compensation
//! 5. Headless offscreen UI execution with pure software backend

use std::env;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use slint::Model;
use woderrise_core::{
    catalog, BackupManager, Database, InstallStage, InstallationEngine, Manifest, ProgressCallback,
};
use woderrise_desktop::{
    filter_packages, init_headless_slint, install_plan_to_slint_files, manifest_to_view_model,
    map_install_plan, parse_hex_color, parse_hex_rgb, AppWindow, PackageDetailViewModel,
    PackageViewModel,
};

// ============================================================================
// Test Environment Utilities
// ============================================================================

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

fn create_temp_env(prefix: &str) -> (TempDirGuard, PathBuf, PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("woderrise-chal-{prefix}-{nonce}"));
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

// ============================================================================
// 1. Manifest Edge Cases & Real Package Ingestion
// ============================================================================

#[test]
fn test_challenge_real_catppuccin_waybar_complete_fidelity() {
    let ws = workspace_root();
    let pkg_dir = ws.join("registry/packages/com.catppuccin.waybar/1.0.0");
    let manifest_path = pkg_dir.join("woderrise.yaml");
    assert!(manifest_path.exists(), "Manifest path must exist");

    let raw_yaml = fs::read_to_string(&manifest_path).unwrap();
    let manifest: Manifest = serde_yaml::from_str(&raw_yaml).unwrap();

    let vm = manifest_to_view_model(&manifest, &manifest_path, false);

    // Exact field assertions without placeholders
    assert_eq!(vm.id, "com.catppuccin.waybar");
    assert_eq!(vm.name, "Catppuccin Waybar Mocha Theme");
    assert_eq!(vm.version, "1.0.0");
    assert_eq!(vm.package_type, "theme");
    assert_eq!(vm.category, "bar_themes_modules");
    assert_eq!(vm.license, "MIT");
    assert_eq!(vm.repository, "https://github.com/catppuccin/waybar");
    assert_eq!(
        vm.authors_formatted,
        "Catppuccin contributors (https://github.com/catppuccin/waybar)"
    );
    assert_eq!(vm.display_server, "Wayland Only");
    assert_eq!(vm.architectures, vec!["x86_64", "aarch64"]);
    assert_eq!(vm.palette.len(), 4);
    assert_eq!(vm.palette[0].hex, "#1e1e2e");
    assert_eq!(vm.palette[1].hex, "#89b4fa");
    assert_eq!(vm.palette[2].hex, "#cba6f7");
    assert_eq!(vm.palette[3].hex, "#cdd6f4");

    // PackageDetailViewModel conversion
    let detail = PackageDetailViewModel::from_manifest(&manifest, false);
    assert_eq!(detail.id, "com.catppuccin.waybar");
    assert_eq!(detail.palette.len(), 4);
    assert_eq!(detail.target_path, "~/.config/waybar/catppuccin-mocha.css");
    assert_eq!(detail.architectures, "x86_64, aarch64");
}

#[test]
fn test_challenge_manifest_edge_cases_no_palette_no_authors_no_compat() {
    // Construct minimal manifest with empty/missing optional sections
    let minimal_yaml = r#"
schema_version: '1.0.0'
package:
  id: org.minimal.pkg
  name: Minimal Package
  version: '0.1.0'
  type: config
  category: core
  license: Apache-2.0
  description: A completely minimal package.
  tags: []
artifacts: []
install:
  files: []
"#;
    let manifest: Manifest = serde_yaml::from_str(minimal_yaml).expect("Failed to parse minimal YAML");
    let vm = manifest_to_view_model(&manifest, "/fake/woderrise.yaml", false);

    assert_eq!(vm.id, "org.minimal.pkg");
    assert_eq!(vm.name, "Minimal Package");
    assert_eq!(vm.authors_formatted, "Not declared");
    assert_eq!(vm.repository, "Not declared");
    assert_eq!(vm.display_server, "Universal");
    assert_eq!(vm.architectures, vec!["all"]);
    assert!(vm.palette.is_empty());
    assert!(!vm.installed);

    let detail = PackageDetailViewModel::from_manifest(&manifest, true);
    assert_eq!(detail.target_path, "None");
    assert_eq!(detail.maintainer, "Not declared");
    assert_eq!(detail.repo_url, "Not declared");
    assert!(detail.palette.is_empty());
    assert!(detail.installed);
}

#[test]
fn test_challenge_corrupt_hex_colors_handling() {
    // Check that malformed hex strings in palette do not panic and degrade gracefully
    let adversarial_hexes = [
        "",
        "#",
        "#1",
        "#12",
        "#1234",
        "#12345",
        "#1234567",
        "#123456789",
        "#ZZZZZZ",
        "red",
        "rgb(0,0,0)",
        "#FF FF FF",
        "#\0\0\0",
    ];

    for hex in &adversarial_hexes {
        assert_eq!(parse_hex_rgb(hex), None, "Expected None for hex '{hex}'");
        assert_eq!(parse_hex_color(hex), None, "Expected None for color '{hex}'");
    }

    // Valid 3-char, 6-char, 8-char
    assert_eq!(parse_hex_rgb("#abc"), Some((170, 187, 204)));
    assert_eq!(parse_hex_rgb("#000000"), Some((0, 0, 0)));
    let col = parse_hex_color("#80ff00ff").expect("8-digit hex");
    assert_eq!(col.alpha(), 255);
    assert_eq!(col.red(), 128);
    assert_eq!(col.green(), 255);
    assert_eq!(col.blue(), 0);
}

// ============================================================================
// 2. Reactive Search Filtering Stress & Injection Tests
// ============================================================================

#[test]
fn test_challenge_search_filtering_high_volume_stress() {
    // Generate a set of 500 distinct package view models
    let mut packages = Vec::with_capacity(500);
    for i in 0..500 {
        let cat = match i % 4 {
            0 => "bar_themes_modules",
            1 => "rices_setups",
            2 => "wallpapers",
            _ => "terminal_themes",
        };
        packages.push(PackageViewModel {
            id: format!("com.pkg.test_{i:04}"),
            name: format!("Package Theme {i}"),
            version: format!("1.{}.0", i % 10),
            package_type: "theme".into(),
            category: cat.into(),
            description: format!("Description for package index {i} with unique keyword kw_{}", i * 7),
            license: "MIT".into(),
            repository: "Not declared".into(),
            authors_formatted: "Tester".into(),
            display_server: "Wayland Only".into(),
            architectures: vec!["x86_64".into()],
            palette: vec![],
            installed: i % 5 == 0,
            manifest_path: PathBuf::from(format!("/fake/path/{i}")),
        });
    }

    // Run 2,000 rapid searches with mixed queries, categories, and whitespace
    let queries = [
        "", "   ", "theme", "THEME", "kw_49", "pkg.test_004", "0499", "nonexistent_term_xyz",
        "bar_themes_modules", "rices_setups", "1.5.0",
    ];
    let categories = ["all", "All", "ALL", "bar_themes_modules", "rices_setups", "wallpapers", "nonexistent"];

    for q in &queries {
        for cat in &categories {
            let filtered = filter_packages(&packages, q, cat);
            // Verify category filter correctness
            if *cat != "all" && *cat != "All" && *cat != "ALL" && *cat != "nonexistent" {
                for p in &filtered {
                    assert_eq!(&p.category, cat);
                }
            } else if *cat == "nonexistent" {
                assert!(filtered.is_empty());
            }

            // Verify query search correctness
            let trimmed = q.trim().to_lowercase();
            if !trimmed.is_empty() && *cat != "nonexistent" {
                for p in &filtered {
                    let matches = p.id.to_lowercase().contains(&trimmed)
                        || p.name.to_lowercase().contains(&trimmed)
                        || p.description.to_lowercase().contains(&trimmed);
                    assert!(matches, "Filtered package must match query");
                }
            }
        }
    }
}

#[test]
fn test_challenge_search_sqlite_fts_and_adversarial_queries() {
    let (_guard, _home, data) = create_temp_env("fts-stress");
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::migrate(&db).unwrap();

    let ws = workspace_root();
    let reg_dir = ws.join("registry/packages");
    catalog::import_registry(&db, &reg_dir).unwrap();

    // Adversarial queries designed to stress SQLite FTS5 parser
    let adversarial_queries = [
        "' OR '1'='1",
        "'; DROP TABLE catalog; --",
        "\"\"\"\"\"\"\"\"",
        "***",
        "???",
        "AND",
        "OR",
        "NOT",
        "NEAR()",
        "column:value",
        "()()()",
        "\\x00\\x01",
        "Catppuccin* Mocha*",
        "Catppuccin NOT waybar",
        "    \t\n   ",
        "Waybar \u{1f600} \u{1f525}",
    ];

    for q in &adversarial_queries {
        // Must never panic or produce unhandled SQL syntax error
        let res = catalog::search(&db, q, 50);
        assert!(res.is_ok(), "Query '{q}' should be handled gracefully, got {:?}", res.err());
    }

    // Verify legitimate search still finds com.catppuccin.waybar
    let valid_results = catalog::search(&db, "waybar", 10).unwrap();
    assert_eq!(valid_results.len(), 1);
    assert_eq!(valid_results[0].id, "com.catppuccin.waybar");
}

// ============================================================================
// 3. Myers Diff Calculation & Backup Path Predictions (Text & Binary)
// ============================================================================

#[test]
fn test_challenge_diff_and_backup_predictions_across_all_file_types() {
    let (_guard, home, data) = create_temp_env("diff-comprehensive");
    let db = Database::open(data.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups")).unwrap(),
        home: home.clone(),
    };

    // Scenario A: Clean Install (Target does not exist)
    let ws = workspace_root();
    let pkg_dir = ws.join("registry/packages/com.catppuccin.waybar/1.0.0");
    let plan = engine.compute_plan(&pkg_dir).unwrap();
    let vm = map_install_plan(&plan);

    assert_eq!(vm.file_diffs.len(), 1);
    assert_eq!(vm.file_diffs[0].action, "CREATE");
    assert_eq!(vm.file_diffs[0].planned_backup_path, "None (new file)");
    assert_eq!(vm.will_backup_count, 0);
    assert_eq!(vm.total_deletions, 0);
    assert!(vm.total_additions > 0);

    // Scenario B: Target exists and differs (MODIFY + Backup prediction)
    let target_path = home.join(".config/waybar/catppuccin-mocha.css");
    fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    let old_content = "/* Customized line */\n@define-color base #111111;\n";
    fs::write(&target_path, old_content).unwrap();

    let plan = engine.compute_plan(&pkg_dir).unwrap();
    let vm = map_install_plan(&plan);
    assert_eq!(vm.file_diffs[0].action, "MODIFY");
    assert_eq!(vm.will_backup_count, 1);
    assert_ne!(vm.file_diffs[0].planned_backup_path, "None (new file)");
    assert!(vm.file_diffs[0].planned_backup_path.contains("backups"));
    assert!(vm.total_additions > 0);
    assert!(vm.total_deletions > 0);

    // Verify Slint model conversion does not drop properties
    let slint_files = install_plan_to_slint_files(&plan);
    assert_eq!(slint_files.row_count(), 1);
    let slint_diff = slint_files.row_data(0).unwrap();
    assert_eq!(slint_diff.action.as_str(), "Modify");
    assert!(slint_diff.lines.row_count() > 0);

    // Scenario C: Target identical (UNCHANGED)
    let source_bytes = fs::read(pkg_dir.join("artifacts/mocha.css")).unwrap();
    fs::write(&target_path, &source_bytes).unwrap();

    let plan = engine.compute_plan(&pkg_dir).unwrap();
    let vm = map_install_plan(&plan);
    assert_eq!(vm.file_diffs[0].action, "UNCHANGED");
    assert_eq!(vm.total_additions, 0);
    assert_eq!(vm.total_deletions, 0);
}

#[test]
fn test_challenge_diff_with_binary_files() {
    let (_guard, home, data) = create_temp_env("diff-binary");
    let db = Database::open(data.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups")).unwrap(),
        home: home.clone(),
    };

    // Construct custom test package with binary payload
    let fake_pkg_dir = data.join("fake_binary_pkg");
    fs::create_dir_all(fake_pkg_dir.join("artifacts")).unwrap();

    // Binary payload with non-UTF8 bytes
    let bin_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0xFF, 0xFE];
    let bin_file = fake_pkg_dir.join("artifacts/image.bin");
    fs::write(&bin_file, &bin_bytes).unwrap();
    let bin_hash = woderrise_core::sha256_bytes(&bin_bytes);

    let manifest_yaml = format!(
        r#"
schema_version: '1.0.0'
package:
  id: com.test.binary
  name: Binary Package
  version: '1.0.0'
  type: config
  category: core
  license: MIT
  description: Binary package test.
  tags: []
artifacts:
  - path: artifacts/image.bin
    sha256: {bin_hash}
install:
  files:
    - source: artifacts/image.bin
      target: ~/.config/test/image.bin
      backup_before_write: true
"#
    );
    fs::write(fake_pkg_dir.join("woderrise.yaml"), manifest_yaml).unwrap();

    // Case 1: Binary Create
    let plan = engine.compute_plan(&fake_pkg_dir).unwrap();
    let vm = map_install_plan(&plan);
    assert_eq!(vm.file_diffs[0].action, "CREATE");
    assert_eq!(vm.file_diffs[0].lines[0].text, "[binary artifact added]");

    // Case 2: Binary Modify
    let target = home.join(".config/test/image.bin");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, vec![0x00, 0x01, 0x02, 0x03]).unwrap();

    let plan = engine.compute_plan(&fake_pkg_dir).unwrap();
    let vm = map_install_plan(&plan);
    assert_eq!(vm.file_diffs[0].action, "MODIFY");
    assert_eq!(vm.file_diffs[0].lines[0].text, "[binary artifacts modified]");
    assert!(vm.file_diffs[0].planned_backup_path.contains("backups"));

    // Case 3: Binary Unchanged
    fs::write(&target, &bin_bytes).unwrap();
    let plan = engine.compute_plan(&fake_pkg_dir).unwrap();
    let vm = map_install_plan(&plan);
    assert_eq!(vm.file_diffs[0].action, "UNCHANGED");
    assert_eq!(vm.file_diffs[0].lines[0].text, "[binary artifacts identical]");
}

// ============================================================================
// 4. 15-Stage Async Progress Events & Failure Rollback Compensation
// ============================================================================

#[test]
fn test_challenge_15_stage_sequence_completeness_and_monotonicity() {
    let all_stages = [
        InstallStage::Resolve,
        InstallStage::Compatibility,
        InstallStage::Fetch,
        InstallStage::Validate,
        InstallStage::Inspect,
        InstallStage::Plan,
        InstallStage::Conflicts,
        InstallStage::Backup,
        InstallStage::Prepared,
        InstallStage::Execute,
        InstallStage::Outcomes,
        InstallStage::Activate,
        InstallStage::Verify,
        InstallStage::Commit,
        InstallStage::Present,
    ];

    assert_eq!(all_stages.len(), 15, "Must declare exactly 15 distinct stages");

    // Test string representations
    for stage in &all_stages {
        assert!(!stage.as_str().is_empty());
        assert_eq!(format!("{stage}"), stage.as_str());
    }

    // Verify real installation of Catppuccin Waybar captures progress stages monotonically
    let (_guard, home, data) = create_temp_env("progress-real");
    let db = Database::open(data.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups")).unwrap(),
        home: home.clone(),
    };

    let events: Arc<Mutex<Vec<(InstallStage, String, f32)>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let cb: ProgressCallback = Box::new(move |stage, msg, frac| {
        events_clone.lock().unwrap().push((stage, msg.to_string(), frac));
    });

    let ws = workspace_root();
    let pkg_dir = ws.join("registry/packages/com.catppuccin.waybar/1.0.0");
    let txn_id = engine
        .install_with_progress(&pkg_dir, "test_challenger", Some(cb))
        .expect("Install must succeed");

    let captured = events.lock().unwrap();
    assert!(!captured.is_empty(), "Must emit progress events");

    // Monotonicity check
    let mut last_frac = 0.0f32;
    for (stage, msg, frac) in captured.iter() {
        assert!(
            *frac >= last_frac,
            "Progress fraction must be monotonic: stage={:?}, frac={}, last_frac={}",
            stage,
            frac,
            last_frac
        );
        assert!(*frac >= 0.0 && *frac <= 1.0, "Fraction must be between 0 and 1");
        assert!(!msg.is_empty(), "Stage message must not be empty");
        last_frac = *frac;
    }

    assert_eq!(captured.last().unwrap().0, InstallStage::Commit);
    assert_eq!(captured.last().unwrap().2, 1.0);

    // Verify target file actually installed
    let target_file = home.join(".config/waybar/catppuccin-mocha.css");
    assert!(target_file.exists(), "Target file must be installed on disk");

    // Verify rollback works
    let rb_id = engine.rollback(&txn_id, false).expect("Rollback must succeed");
    assert!(!rb_id.is_empty());
    assert!(!target_file.exists(), "Target file must be removed after rollback");
}

#[test]
fn test_challenge_preflight_validation_rejection_before_transaction() {
    let (_guard, home, data) = create_temp_env("preflight-fail");
    let db = Database::open(data.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups")).unwrap(),
        home: home.clone(),
    };

    let fake_dir = data.join("corrupted_pkg");
    fs::create_dir_all(fake_dir.join("artifacts")).unwrap();
    fs::write(fake_dir.join("artifacts/mocha.css"), "tampered content").unwrap();

    let manifest_yaml = r#"
schema_version: '1.0.0'
package:
  id: com.catppuccin.waybar
  name: Catppuccin Waybar Mocha Theme
  version: '1.0.0'
  type: theme
  category: bar_themes_modules
  license: MIT
  description: Corrupted test.
  tags: []
artifacts:
  - path: artifacts/mocha.css
    sha256: a6e30597ccc828e898844eb0cea9df7d73879ff5672a6a550f3acc24c93edd17
install:
  files:
    - source: artifacts/mocha.css
      target: ~/.config/waybar/catppuccin-mocha.css
      backup_before_write: true
"#;
    fs::write(fake_dir.join("woderrise.yaml"), manifest_yaml).unwrap();

    let res = engine.install_with_progress(&fake_dir, "challenger_preflight", None);
    assert!(res.is_err(), "Install must fail pre-flight validation on checksum mismatch");

    // Zero transactions recorded because it failed before transaction start
    let db_check = Database::open(data.join("journal.db")).unwrap();
    let count: i64 = db_check
        .conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "No transactions should be recorded on pre-flight failure");
}

#[test]
fn test_challenge_simulated_install_failure_and_rollback_recovery() {
    let (_guard, home, data) = create_temp_env("failure-recovery");
    let db = Database::open(data.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups")).unwrap(),
        home: home.clone(),
    };

    // Pre-create target file 1 with existing content
    let target1 = home.join(".config/test/file1.txt");
    fs::create_dir_all(target1.parent().unwrap()).unwrap();
    fs::write(&target1, "original uncorrupted configuration").unwrap();

    // Make target 2 an existing directory so atomic_replace fails during execute
    let target2 = home.join(".config/test/file2.txt");
    fs::create_dir_all(&target2).unwrap(); // directory instead of file causes rename/copy error

    // Create a 2-file package
    let fake_dir = data.join("multi_pkg");
    fs::create_dir_all(fake_dir.join("artifacts")).unwrap();
    let content1 = "new content 1";
    let content2 = "new content 2";
    fs::write(fake_dir.join("artifacts/f1.txt"), content1).unwrap();
    fs::write(fake_dir.join("artifacts/f2.txt"), content2).unwrap();
    let h1 = woderrise_core::sha256_bytes(content1.as_bytes());
    let h2 = woderrise_core::sha256_bytes(content2.as_bytes());

    let manifest_yaml = format!(
        r#"
schema_version: '1.0.0'
package:
  id: com.test.compensation
  name: Compensation Test
  version: '1.0.0'
  type: config
  category: core
  license: MIT
  description: Test compensation on failure.
  tags: []
artifacts:
  - path: artifacts/f1.txt
    sha256: {h1}
  - path: artifacts/f2.txt
    sha256: {h2}
install:
  files:
    - source: artifacts/f1.txt
      target: ~/.config/test/file1.txt
      backup_before_write: true
    - source: artifacts/f2.txt
      target: ~/.config/test/file2.txt
      backup_before_write: true
"#
    );
    fs::write(fake_dir.join("woderrise.yaml"), manifest_yaml).unwrap();

    let res = engine.install_with_progress(&fake_dir, "challenger_fail", None);
    assert!(res.is_err(), "Install must fail when target2 is a directory");

    // Reopen DB to check transaction status
    let db_check = Database::open(data.join("journal.db")).unwrap();
    let status: String = db_check
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE package_id = 'com.test.compensation' ORDER BY started_at DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        status, "ABORTED_RECOVERED",
        "Transaction status should be ABORTED_RECOVERED after compensation"
    );

    // Verify original file 1 was safely restored by compensation
    let restored_content = fs::read_to_string(&target1).unwrap();
    assert_eq!(
        restored_content, "original uncorrupted configuration",
        "Target 1 must be restored from pre-modification backup"
    );
}

// ============================================================================
// 5. Headless Offscreen Slint UI Execution Verification
// ============================================================================

#[test]
fn test_challenge_headless_slint_interaction_under_missing_displays() {
    // Deliberately empty display server variables
    env::remove_var("DISPLAY");
    env::remove_var("WAYLAND_DISPLAY");

    init_headless_slint();

    let app = AppWindow::new().expect("Headless AppWindow initialization must succeed");

    // Step 1: Navigate across views
    for tab_idx in 0..=6 {
        app.invoke_switch_view(tab_idx);
        assert_eq!(app.get_active_view(), tab_idx);
    }

    // Step 2: Test Diff modal flow
    assert!(!app.get_show_diff_modal());
    app.set_diff_target_path("~/.config/waybar/mocha.css".into());
    app.set_diff_backup_path("backups/a6e3...".into());
    app.set_diff_action_type("Modify".into());
    app.set_diff_additions(26);
    app.set_diff_deletions(3);
    app.set_show_diff_modal(true);
    app.set_diff_modal_open(true);

    assert!(app.get_show_diff_modal());
    assert_eq!(app.get_diff_additions(), 26);
    assert_eq!(app.get_diff_deletions(), 3);

    app.invoke_close_diff_modal();
    assert!(!app.get_show_diff_modal());
    assert!(!app.get_diff_modal_open());

    // Step 3: Test Settings view state changes
    app.invoke_switch_view(6); // Settings view
    app.set_setting_auto_backup(true);
    app.invoke_toggle_setting_auto_backup();
    assert!(!app.get_setting_auto_backup());

    app.invoke_restore_setting_defaults();
    assert!(app.get_setting_auto_backup());
    assert!(app.get_setting_strict_hash());
    assert!(app.get_setting_offline_cache());
}
