use std::fs;
use std::path::{Path, PathBuf};
use woderrise_core::catalog;
use woderrise_core::{Database, Manifest};
use woderrise_desktop::view_model::{manifest_to_view_model, PackageViewModel};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn test_real_catppuccin_waybar_manifest_ingestion_and_metadata_fidelity() {
    let ws = workspace_root();
    let manifest_path = ws.join("registry/packages/com.catppuccin.waybar/1.0.0/woderrise.yaml");
    assert!(
        manifest_path.exists(),
        "Catppuccin Waybar manifest must exist at {:?}",
        manifest_path
    );

    let content = fs::read_to_string(&manifest_path).expect("Failed to read manifest");
    let manifest: Manifest = serde_yaml::from_str(&content).expect("Failed to parse manifest YAML");

    let vm: PackageViewModel = manifest_to_view_model(&manifest, &manifest_path, false);

    // Assert exact metadata fidelity (zero placeholders)
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
    assert!(!vm.installed);

    // Verify 4-color palette swatches
    assert_eq!(vm.palette.len(), 4);
    assert_eq!(vm.palette[0].name, "base");
    assert_eq!(vm.palette[0].hex, "#1e1e2e");
    assert_eq!((vm.palette[0].r, vm.palette[0].g, vm.palette[0].b), (30, 30, 46));

    assert_eq!(vm.palette[1].name, "blue");
    assert_eq!(vm.palette[1].hex, "#89b4fa");
    assert_eq!((vm.palette[1].r, vm.palette[1].g, vm.palette[1].b), (137, 180, 250));

    assert_eq!(vm.palette[2].name, "mauve");
    assert_eq!(vm.palette[2].hex, "#cba6f7");
    assert_eq!((vm.palette[2].r, vm.palette[2].g, vm.palette[2].b), (203, 166, 247));

    assert_eq!(vm.palette[3].name, "text");
    assert_eq!(vm.palette[3].hex, "#cdd6f4");
    assert_eq!((vm.palette[3].r, vm.palette[3].g, vm.palette[3].b), (205, 214, 244));
}

#[test]
fn test_sqlite_catalog_indexing_and_view_model_population() {
    let ws = workspace_root();
    let reg_dir = ws.join("registry/packages");

    let db = Database::in_memory().expect("Failed to initialize in-memory SQLite database");
    catalog::migrate(&db).expect("Failed to run catalog migration");

    let count = catalog::import_registry(&db, &reg_dir).expect("Failed to import registry");
    assert!(count >= 1, "Must index at least 1 verified package");

    let entries = catalog::search(&db, "waybar", 10).expect("Catalog search failed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "com.catppuccin.waybar");
    assert_eq!(entries[0].version, "1.0.0");
    assert!(entries[0].validated);

    // Extract manifest through catalog API
    let manifest = catalog::manifest(&db, &entries[0].id, &entries[0].version)
        .expect("Failed to load manifest via catalog");
    let vm = manifest_to_view_model(&manifest, &entries[0].manifest_path, false);
    assert_eq!(vm.name, "Catppuccin Waybar Mocha Theme");
}

#[test]
fn test_installed_status_reflection_against_sqlite() {
    let db = Database::in_memory().expect("Failed to open SQLite");
    catalog::migrate(&db).expect("Migration failed");

    // Initially uninstalled
    let installed_count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.catppuccin.waybar'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(installed_count, 0);

    // Insert active install transaction
    db.conn
        .execute(
            "INSERT INTO installed_packages (package_id, active_version, package_type, installed_at, updated_at, install_txn_id)
             VALUES ('com.catppuccin.waybar', '1.0.0', 'theme', 1700000000, 1700000000, 'txn-001')",
            [],
        )
        .unwrap();

    let is_installed: bool = db
        .conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM installed_packages WHERE package_id = 'com.catppuccin.waybar' AND active_version = '1.0.0'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(is_installed);
}
