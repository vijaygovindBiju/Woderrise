//! Adversarial stress test suite for Slint Desktop GUI:
//! 1. Desktop GUI model bindings for imported Hyprland setups (palette swatches, compatibility badges, metadata)
//! 2. Multi-file diff modal Slint model bindings on imported dotfiles (CREATE, MODIFY, UNCHANGED, binary diffs)
//! 3. Import modal interactive state machine, adversarial input bindings, and rapid toggle lifecycle
//! 4. Catalog cards model consistency under dynamic package imports and selection tracking

use slint::Model;
use std::{collections::HashSet, path::PathBuf};
use woderrise_core::{
    diff::{DiffLine, DiffLineKind, FileDiff, FileDiffAction, InstallPlan},
    Manifest,
};
use woderrise_desktop::{
    catalog_entries_to_cards_model, install_plan_to_slint_files,
    test_helpers::init_headless_slint,
    view_model::PackageDetailViewModel,
    AppWindow,
};

fn setup_headless_env() {
    std::env::remove_var("DISPLAY");
    std::env::remove_var("WAYLAND_DISPLAY");
    init_headless_slint();
}

fn create_synthetic_hyprland_manifest(
    id: &str,
    name: &str,
    palette_yaml: &str,
) -> Manifest {
    let yaml = format!(
        r#"
schema_version: '1.0.0'
package:
  id: {id}
  name: {name}
  version: '1.0.0'
  type: config
  category: window_manager_hyprland
  license: MIT
  description: High performance dynamic Hyprland setup
  tags: [hyprland, wayland, mocha]
  repository: https://github.com/hyprland-community/rices
  authors:
    - name: Hyprland Rice Community
      email: rice@hyprland.community
      url: https://github.com/hyprland-community/rices
compatibility:
  display_server: wayland_only
  compositors:
    - name: hyprland
      version: '>=0.40.0'
  architectures: [x86_64, aarch64]
artifacts: []
install:
  files: []
{palette_yaml}
"#
    );
    serde_yaml::from_str(&yaml).expect("Failed to deserialize manifest YAML")
}

// ============================================================================
// 1. DESKTOP GUI MODEL BINDINGS FOR IMPORTED HYPRLAND PACKAGES
// ============================================================================

#[test]
fn test_adversarial_gui_bindings_across_palette_permutations() {
    setup_headless_env();
    let app = AppWindow::new().expect("Headless AppWindow creation failed");

    // Case A: 10 Swatches (Catppuccin Style)
    let ten_yaml = r##"
previews:
  thumbnail: artifacts/hyprland.conf
  palette:
    - name: mocha_base
      hex: "#1e1e2e"
    - name: mocha_mauve
      hex: "#cba6f7"
    - name: mocha_blue
      hex: "#89b4fa"
    - name: mocha_text
      hex: "#cdd6f4"
    - name: macchiato_base
      hex: "#24273a"
    - name: macchiato_mauve
      hex: "#c6a0f6"
    - name: frappe_base
      hex: "#303446"
    - name: frappe_mauve
      hex: "#ca9ee6"
    - name: latte_base
      hex: "#eff1f5"
    - name: latte_mauve
      hex: "#8839ef"
"##;
    let m10 = create_synthetic_hyprland_manifest("io.github.test.rice_10", "Rice 10 Colors", ten_yaml);
    let vm10 = PackageDetailViewModel::from_manifest(&m10, false);
    vm10.apply_to_window(&app);

    assert_eq!(app.get_sel_id(), "io.github.test.rice_10");
    assert_eq!(app.get_sel_name(), "Rice 10 Colors");
    assert_eq!(app.get_sel_display_server(), "Wayland Only");
    assert_eq!(app.get_sel_architectures(), "x86_64, aarch64");
    assert!(!app.get_sel_installed());

    let p10 = app.get_sel_palette();
    assert_eq!(p10.row_count(), 10);
    assert_eq!(p10.row_data(0).unwrap().name, "mocha_base");
    assert_eq!(p10.row_data(0).unwrap().hex, "#1e1e2e");
    assert_eq!(p10.row_data(9).unwrap().name, "latte_mauve");
    assert_eq!(p10.row_data(9).unwrap().hex, "#8839ef");

    // Case B: 0 Swatches (Manifest without color palette)
    let m0 = create_synthetic_hyprland_manifest("io.github.test.rice_0", "Rice No Colors", "");
    let vm0 = PackageDetailViewModel::from_manifest(&m0, true);
    vm0.apply_to_window(&app);

    assert_eq!(app.get_sel_id(), "io.github.test.rice_0");
    assert!(app.get_sel_installed());
    let p0 = app.get_sel_palette();
    assert_eq!(p0.row_count(), 0, "Empty palette should bind cleanly with 0 rows");

    // Case C: Corrupted/Adversarial Hex Colors (must not crash)
    let corrupt_yaml = r##"
previews:
  thumbnail: artifacts/hyprland.conf
  palette:
    - name: empty
      hex: ""
    - name: hash_only
      hex: "#"
    - name: short
      hex: "#12"
    - name: bad_chars
      hex: "#ZZZZZZ"
    - name: spaces
      hex: "#1e 1e 2e"
    - name: valid_short
      hex: "#abc"
    - name: valid_long
      hex: "#89b4faff"
"##;
    let m_corrupt = create_synthetic_hyprland_manifest("io.github.test.corrupt", "Corrupted Hex Rice", corrupt_yaml);
    let vm_corrupt = PackageDetailViewModel::from_manifest(&m_corrupt, false);
    vm_corrupt.apply_to_window(&app);

    let p_corrupt = app.get_sel_palette();
    assert_eq!(p_corrupt.row_count(), 7, "All swatch entries must bind even with fallback colors");
}

// ============================================================================
// 2. MULTI-FILE DIFF MODAL SLINT MODEL BINDINGS ON IMPORTED DOTFILES
// ============================================================================

#[test]
fn test_adversarial_multi_file_diff_modal_slint_bindings() {
    setup_headless_env();

    // Construct realistic multi-file install plan for imported Hyprland rice
    let plan = InstallPlan {
        package_id: "io.github.local.hypr-nordic".into(),
        version: "1.0.0".into(),
        file_diffs: vec![
            // File 1: hyprland.conf (MODIFY with additions & deletions)
            FileDiff {
                source_path: PathBuf::from("hypr/hyprland.conf"),
                target_path: PathBuf::from("~/.config/hypr/hyprland.conf"),
                action: FileDiffAction::Modify,
                planned_backup_path: Some(PathBuf::from("/home/user/.local/share/woderrise/backups/67e2a4b8")),
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Deletion,
                        left_line_no: Some(1),
                        right_line_no: None,
                        text: "monitor=DP-1,1920x1080@60,0x0,1".into(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        left_line_no: None,
                        right_line_no: Some(1),
                        text: "monitor=,preferred,auto,1".into(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        left_line_no: Some(2),
                        right_line_no: Some(2),
                        text: "general { gaps_in = 5 }".into(),
                    },
                ],
            },
            // File 2: hyprlock.conf (CREATE)
            FileDiff {
                source_path: PathBuf::from("hypr/hyprlock.conf"),
                target_path: PathBuf::from("~/.config/hypr/hyprlock.conf"),
                action: FileDiffAction::Create,
                planned_backup_path: None,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        left_line_no: None,
                        right_line_no: Some(1),
                        text: "background { color = rgba(30, 30, 46, 1.0) }".into(),
                    },
                ],
            },
            // File 3: waybar/config.jsonc (UNCHANGED)
            FileDiff {
                source_path: PathBuf::from("waybar/config.jsonc"),
                target_path: PathBuf::from("~/.config/waybar/config.jsonc"),
                action: FileDiffAction::Unchanged,
                planned_backup_path: None,
                lines: vec![],
            },
            // File 4: wallpaper.png (MODIFY binary)
            FileDiff {
                source_path: PathBuf::from("hypr/wallpaper.png"),
                target_path: PathBuf::from("~/.config/hypr/wallpaper.png"),
                action: FileDiffAction::Modify,
                planned_backup_path: Some(PathBuf::from("/home/user/.local/share/woderrise/backups/f301cc99")),
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Deletion,
                        left_line_no: None,
                        right_line_no: None,
                        text: "[binary artifacts modified]".into(),
                    },
                ],
            },
        ],
        will_backup_count: 2,
    };

    let files_model = install_plan_to_slint_files(&plan);
    assert_eq!(files_model.row_count(), 4, "Expected exactly 4 files in diff model");

    // Verify File 0 (MODIFY)
    let f0 = files_model.row_data(0).unwrap();
    assert_eq!(f0.target_path.as_str(), "~/.config/hypr/hyprland.conf");
    assert_eq!(f0.action.as_str(), "Modify");
    assert!(f0.backup_path.as_str().contains("backups/67e2a4b8"));
    assert_eq!(f0.additions, 1);
    assert_eq!(f0.deletions, 1);
    assert_eq!(f0.lines.row_count(), 3);
    assert_eq!(f0.lines.row_data(0).unwrap().kind.as_str(), "deletion");
    assert_eq!(f0.lines.row_data(1).unwrap().kind.as_str(), "addition");
    assert_eq!(f0.lines.row_data(2).unwrap().kind.as_str(), "context");

    // Verify File 1 (CREATE)
    let f1 = files_model.row_data(1).unwrap();
    assert_eq!(f1.target_path.as_str(), "~/.config/hypr/hyprlock.conf");
    assert_eq!(f1.action.as_str(), "Create");
    assert_eq!(f1.backup_path.as_str(), "None (clean install)");
    assert_eq!(f1.additions, 1);
    assert_eq!(f1.deletions, 0);

    // Verify File 2 (UNCHANGED)
    let f2 = files_model.row_data(2).unwrap();
    assert_eq!(f2.target_path.as_str(), "~/.config/waybar/config.jsonc");
    assert_eq!(f2.action.as_str(), "Unchanged");
    assert_eq!(f2.additions, 0);
    assert_eq!(f2.deletions, 0);

    // Verify File 3 (MODIFY binary)
    let f3 = files_model.row_data(3).unwrap();
    assert_eq!(f3.target_path.as_str(), "~/.config/hypr/wallpaper.png");
    assert_eq!(f3.action.as_str(), "Modify");
    assert!(f3.backup_path.as_str().contains("backups/f301cc99"));
    assert_eq!(f3.lines.row_count(), 1);
    assert_eq!(f3.lines.row_data(0).unwrap().text.as_str(), "[binary artifacts modified]");
}

// ============================================================================
// 3. IMPORT MODAL INTERACTIVE STATE MACHINE & ADVERSARIAL INPUT BINDINGS
// ============================================================================

#[test]
fn test_adversarial_import_modal_state_machine_and_input_bindings() {
    setup_headless_env();
    let app = AppWindow::new().expect("Headless AppWindow creation failed");

    // Initial state invariants
    assert!(!app.get_show_import_dialog());
    assert_eq!(app.get_import_url_input(), "");
    assert_eq!(app.get_import_error(), "");
    assert!(!app.get_import_is_loading());

    // 100 rapid open/close toggle iterations to verify zero flag trapping or leaks
    for _ in 0..100 {
        app.invoke_open_import_dialog();
        assert!(app.get_show_import_dialog());
        app.invoke_close_import_dialog();
        assert!(!app.get_show_import_dialog());
    }

    // Interactive error string injection: unicode, SQL syntax, whitespace
    app.invoke_open_import_dialog();
    assert!(app.get_show_import_dialog());

    let injection_error = "Error: ' OR 1=1 -- 🚨 Unexpected token in hyprland.conf; failed at line 42";
    app.set_import_error(injection_error.into());
    assert_eq!(app.get_import_error(), injection_error);

    // URL input buffer bindings
    let test_url = "https://github.com/adversarial-tester/hypr-rice";
    app.set_import_url_input(test_url.into());
    assert_eq!(app.get_import_url_input(), test_url);

    // Loading flag transitions
    app.set_import_is_loading(true);
    assert!(app.get_import_is_loading());

    // Dismissal resets error and loading
    app.invoke_close_import_dialog();
    assert!(!app.get_show_import_dialog());
    assert_eq!(app.get_import_error(), "");
    assert!(!app.get_import_is_loading());
}

// ============================================================================
// 4. CATALOG CARDS MODEL CONSISTENCY UNDER DYNAMIC PACKAGE IMPORTS
// ============================================================================

#[test]
fn test_adversarial_catalog_cards_model_under_dynamic_imports() {
    let mut entries = Vec::new();
    for i in 0..5 {
        entries.push(woderrise_core::catalog::CatalogEntry {
            id: format!("io.github.imported.rice_{i}"),
            name: format!("Imported Rice {i}"),
            description: format!("Dynamic Hyprland rice import {i}"),
            version: "1.0.0".into(),
            package_type: "config".into(),
            category: "window_manager_hyprland".into(),
            tags: vec!["hyprland".into(), format!("tag_{i}")],
            license: "MIT".into(),
            repository: Some(format!("https://github.com/imported/rice_{i}")),
            manifest_path: PathBuf::from(format!("/tmp/imported/rice_{i}/woderrise.yaml")),
            manifest_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
            validated: true,
            yanked: false,
        });
    }

    let mut installed_set = HashSet::new();
    installed_set.insert("io.github.imported.rice_1".to_string());
    installed_set.insert("io.github.imported.rice_3".to_string());

    let cards_model = catalog_entries_to_cards_model(&entries, &installed_set);
    assert_eq!(cards_model.row_count(), 5);

    // Verify card 0 (not installed)
    let c0 = cards_model.row_data(0).unwrap();
    assert_eq!(c0.id.as_str(), "io.github.imported.rice_0");
    assert_eq!(c0.name.as_str(), "Imported Rice 0");
    assert_eq!(c0.category.as_str(), "window_manager_hyprland");
    assert!(!c0.installed);

    // Verify card 1 (installed)
    let c1 = cards_model.row_data(1).unwrap();
    assert_eq!(c1.id.as_str(), "io.github.imported.rice_1");
    assert!(c1.installed);

    // Verify card 3 (installed)
    let c3 = cards_model.row_data(3).unwrap();
    assert_eq!(c3.id.as_str(), "io.github.imported.rice_3");
    assert!(c3.installed);

    // Bind cards model to headless AppWindow
    setup_headless_env();
    let app = AppWindow::new().expect("Headless AppWindow creation failed");
    app.set_packages(cards_model);
    app.set_total_packages_count(5);
    app.set_selected_package_index(3);

    assert_eq!(app.get_total_packages_count(), 5);
    assert_eq!(app.get_selected_package_index(), 3);
    assert_eq!(app.get_packages().row_count(), 5);
}
