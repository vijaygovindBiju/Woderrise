//! Comprehensive Slint Desktop GUI integration tests for Hyprland package display,
//! GitHub import modal lifecycle, property bindings, and multi-file diff mapping.

use slint::Model;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use woderrise_core::{
    catalog,
    diff::{DiffLine, DiffLineKind, FileDiff, FileDiffAction, InstallPlan},
    Database, Manifest,
};
use woderrise_desktop::{
    catalog_entries_to_cards_model, install_plan_to_slint_files,
    test_helpers::init_headless_slint,
    view_model::PackageDetailViewModel,
    AppWindow,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn test_hyprland_manifest_ingestion_and_detail_view_model() {
    let ws = workspace_root();
    let manifest_path = ws.join("registry/packages/com.catppuccin.hyprland/1.0.0/woderrise.yaml");
    assert!(manifest_path.exists(), "Hyprland manifest must exist at {:?}", manifest_path);

    let manifest = Manifest::from_file(&manifest_path).expect("Failed to parse Hyprland manifest");
    let vm = PackageDetailViewModel::from_manifest(&manifest, false);

    assert_eq!(vm.id, "com.catppuccin.hyprland");
    assert_eq!(vm.name, "Catppuccin Wayland Hyprland Suite");
    assert_eq!(vm.version, "1.0.0");
    assert_eq!(vm.category, "desktop_setups");
    assert_eq!(vm.license, "MIT");
    assert_eq!(vm.repo_url, "https://github.com/catppuccin/hyprland");
    assert_eq!(vm.display_server, "Wayland Only");
    assert_eq!(vm.architectures, "x86_64, aarch64");
    assert!(!vm.installed);

    // Verify all 10 genuine Catppuccin swatches
    assert_eq!(vm.palette.len(), 10, "Expected 10 Catppuccin palette swatches");
    assert_eq!(vm.palette[0].name, "mocha_base");
    assert_eq!(vm.palette[0].hex, "#1e1e2e");
    assert_eq!(vm.palette[1].name, "mocha_mauve");
    assert_eq!(vm.palette[1].hex, "#cba6f7");
    assert_eq!(vm.palette[9].name, "latte_mauve");
    assert_eq!(vm.palette[9].hex, "#8839ef");
}

#[test]
fn test_hyprland_view_model_binds_to_headless_slint_window() {
    init_headless_slint();
    let app = AppWindow::new().expect("Failed to create headless AppWindow");

    let ws = workspace_root();
    let manifest_path = ws.join("registry/packages/com.catppuccin.hyprland/1.0.0/woderrise.yaml");
    let manifest = Manifest::from_file(&manifest_path).expect("Failed to parse manifest");
    let vm = PackageDetailViewModel::from_manifest(&manifest, false);

    vm.apply_to_window(&app);

    assert_eq!(app.get_sel_id(), "com.catppuccin.hyprland");
    assert_eq!(app.get_sel_name(), "Catppuccin Wayland Hyprland Suite");
    assert_eq!(app.get_sel_version(), "1.0.0");
    assert_eq!(app.get_sel_category(), "desktop_setups");
    assert_eq!(app.get_sel_license(), "MIT");
    assert_eq!(app.get_sel_display_server(), "Wayland Only");
    assert_eq!(app.get_sel_architectures(), "x86_64, aarch64");
    assert!(!app.get_sel_installed());

    let palette_model = app.get_sel_palette();
    assert_eq!(palette_model.row_count(), 10, "Slint window model should hold 10 swatches");
    let first = palette_model.row_data(0).unwrap();
    assert_eq!(first.name, "mocha_base");
    assert_eq!(first.hex, "#1e1e2e");
    let last = palette_model.row_data(9).unwrap();
    assert_eq!(last.name, "latte_mauve");
    assert_eq!(last.hex, "#8839ef");
}

#[test]
fn test_headless_import_dialog_lifecycle() {
    init_headless_slint();
    let app = AppWindow::new().expect("Failed to create headless AppWindow");

    // Initially modal is closed
    assert!(!app.get_show_import_dialog());
    assert_eq!(app.get_import_url_input(), "");
    assert_eq!(app.get_import_error(), "");
    assert!(!app.get_import_is_loading());

    // Trigger open
    app.invoke_open_import_dialog();
    assert!(app.get_show_import_dialog());
    assert_eq!(app.get_import_error(), "");

    // Set error and loading
    app.set_import_error("Test error".into());
    app.set_import_is_loading(true);
    assert_eq!(app.get_import_error(), "Test error");
    assert!(app.get_import_is_loading());

    // Close dialog clears error and loading
    app.invoke_close_import_dialog();
    assert!(!app.get_show_import_dialog());
    assert_eq!(app.get_import_error(), "");
    assert!(!app.get_import_is_loading());
}

#[test]
fn test_headless_import_dialog_property_bindings() {
    init_headless_slint();
    let app = AppWindow::new().expect("Failed to create headless AppWindow");

    app.set_import_url_input("https://github.com/catppuccin/hyprland".into());
    assert_eq!(app.get_import_url_input(), "https://github.com/catppuccin/hyprland");

    app.set_import_error("Repository missing hyprland.conf".into());
    assert_eq!(app.get_import_error(), "Repository missing hyprland.conf");

    app.set_import_is_loading(true);
    assert!(app.get_import_is_loading());

    app.set_import_is_loading(false);
    assert!(!app.get_import_is_loading());
}

#[test]
fn test_catalog_cards_hyprland_indexing() {
    let ws = workspace_root();
    let reg_dir = ws.join("registry/packages");

    let db = Database::in_memory().expect("Failed to initialize database");
    catalog::migrate(&db).expect("Failed to migrate catalog");
    let count = catalog::import_registry(&db, &reg_dir).expect("Failed to import registry");
    assert_eq!(count, 2, "Expected exactly 2 packages imported into catalog");

    let entries = catalog::search(&db, "", 10).expect("Search failed");
    assert_eq!(entries.len(), 2);

    let cards = catalog_entries_to_cards_model(&entries, &HashSet::new());
    assert_eq!(cards.row_count(), 2);

    let mut found_hyprland = false;
    for i in 0..cards.row_count() {
        let card = cards.row_data(i).unwrap();
        if card.id == "com.catppuccin.hyprland" {
            found_hyprland = true;
            assert_eq!(card.name, "Catppuccin Wayland Hyprland Suite");
            assert_eq!(card.version, "1.0.0");
            assert_eq!(card.category, "desktop_setups");
            assert_eq!(card.license, "MIT");
            assert!(!card.installed);
        }
    }
    assert!(found_hyprland, "Hyprland package card was not present in cards model");
}

#[test]
fn test_diff_mapping_for_hyprland_multi_file_install_plan() {
    let plan = InstallPlan {
        package_id: "com.catppuccin.hyprland".into(),
        version: "1.0.0".into(),
        file_diffs: vec![
            FileDiff {
                source_path: PathBuf::from("artifacts/hyprland.conf"),
                target_path: PathBuf::from("~/.config/hypr/hyprland.conf"),
                action: FileDiffAction::Create,
                planned_backup_path: None,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        left_line_no: None,
                        right_line_no: Some(1),
                        text: "general { gaps_in = 5 }".into(),
                    },
                ],
            },
            FileDiff {
                source_path: PathBuf::from("artifacts/waybar-config.jsonc"),
                target_path: PathBuf::from("~/.config/waybar/config.jsonc"),
                action: FileDiffAction::Modify,
                planned_backup_path: Some(PathBuf::from("/home/user/.local/share/woderrise/backups/abc123")),
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Deletion,
                        left_line_no: Some(1),
                        right_line_no: None,
                        text: "old waybar config".into(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        left_line_no: None,
                        right_line_no: Some(1),
                        text: "new waybar config".into(),
                    },
                ],
            },
        ],
        will_backup_count: 1,
    };

    let files_model = install_plan_to_slint_files(&plan);
    assert_eq!(files_model.row_count(), 2);

    let file0 = files_model.row_data(0).unwrap();
    assert_eq!(file0.target_path.as_str(), "~/.config/hypr/hyprland.conf");
    assert_eq!(file0.action.as_str(), "Create");
    assert_eq!(file0.additions, 1);
    assert_eq!(file0.deletions, 0);

    let file1 = files_model.row_data(1).unwrap();
    assert_eq!(file1.target_path.as_str(), "~/.config/waybar/config.jsonc");
    assert_eq!(file1.action.as_str(), "Modify");
    assert_eq!(file1.additions, 1);
    assert_eq!(file1.deletions, 1);
    assert!(file1.backup_path.as_str().contains("backups/abc123"));
}
