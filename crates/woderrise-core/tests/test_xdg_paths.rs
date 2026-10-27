//! Tests for XDG path resolution, custom test isolation, and component containment.

use camino::{Utf8Path, Utf8PathBuf};
use tempfile::TempDir;
use woderrise_core::error::{PathError, WoderriseError};
use woderrise_core::models::ConfigComponent;
use woderrise_core::paths::{validate_identifier, XdgPaths};

#[test]
fn test_xdg_custom_isolated_paths() {
    let temp = TempDir::new().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let paths = XdgPaths::custom(&base);

    assert_eq!(paths.home_dir, base);
    assert_eq!(paths.config_home, base.join(".config"));
    assert_eq!(paths.data_home, base.join(".local/share"));
    assert_eq!(paths.cache_home, base.join(".cache"));
    assert_eq!(paths.state_home, base.join(".local/state"));

    assert_eq!(paths.woderrise_config_dir(), base.join(".config/woderrise"));
    assert_eq!(
        paths.settings_file(),
        base.join(".config/woderrise/settings.json")
    );
    assert_eq!(
        paths.woderrise_data_dir(),
        base.join(".local/share/woderrise")
    );
    assert_eq!(
        paths.catalog_file(),
        base.join(".local/share/woderrise/catalog.json")
    );
    assert_eq!(
        paths.journal_file(),
        base.join(".local/share/woderrise/journal.json")
    );
    assert_eq!(paths.woderrise_cache_dir(), base.join(".cache/woderrise"));
    assert_eq!(
        paths.staging_dir("tx_12345").unwrap(),
        base.join(".cache/woderrise/staging/tx_12345")
    );
    assert_eq!(paths.woderrise_state_dir(), base.join(".local/state/woderrise"));
    assert_eq!(
        paths.backup_dir("tx_12345").unwrap(),
        base.join(".local/state/woderrise/backups/tx_12345")
    );
}

#[test]
fn test_xdg_component_roots() {
    let temp = TempDir::new().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let paths = XdgPaths::custom(&base);

    assert_eq!(
        paths.component_root(ConfigComponent::Hyprland),
        base.join(".config/hypr")
    );
    assert_eq!(
        paths.component_root(ConfigComponent::HyprlandLegacy),
        base.join(".config/hypr")
    );
    assert_eq!(
        paths.component_root(ConfigComponent::HyprlandLua),
        base.join(".config/hypr")
    );
    assert_eq!(
        paths.component_root(ConfigComponent::Waybar),
        base.join(".config/waybar")
    );
    assert_eq!(
        paths.component_root(ConfigComponent::Hyprlock),
        base.join(".config/hypr")
    );
    assert_eq!(
        paths.component_root(ConfigComponent::Hypridle),
        base.join(".config/hypr")
    );
    assert_eq!(
        paths.component_root(ConfigComponent::GtkTheme),
        base.join(".config/gtk-3.0")
    );
    assert_eq!(
        paths.component_root(ConfigComponent::Hyprcursor),
        base.join(".local/share/icons")
    );
    assert_eq!(
        paths.component_root(ConfigComponent::Wallpapers),
        base.join(".local/share/wallpapers")
    );
    assert_eq!(
        paths.component_root(ConfigComponent::CompleteSuite),
        base.join(".config")
    );
}

#[test]
fn test_resolve_destination_success() {
    let temp = TempDir::new().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let paths = XdgPaths::custom(&base);

    let resolved = paths
        .resolve_destination(ConfigComponent::Hyprland, Utf8Path::new("hyprland.conf"))
        .unwrap();
    assert_eq!(resolved, base.join(".config/hypr/hyprland.conf"));

    let resolved_sub = paths
        .resolve_destination(
            ConfigComponent::Waybar,
            Utf8Path::new("modules/clock.jsonc"),
        )
        .unwrap();
    assert_eq!(resolved_sub, base.join(".config/waybar/modules/clock.jsonc"));
}

#[test]
fn test_resolve_destination_escape_rejected() {
    let temp = TempDir::new().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let paths = XdgPaths::custom(&base);

    // Parent directory traversal is caught during sanitization
    let err = paths
        .resolve_destination(
            ConfigComponent::Waybar,
            Utf8Path::new("../hypr/hyprland.conf"),
        )
        .unwrap_err();

    match err {
        WoderriseError::Path(PathError::PathTraversalAttempted { token, .. }) => {
            assert_eq!(token, "..");
        }
        other => panic!("Expected PathTraversalAttempted, got: {:?}", other),
    }
}

#[test]
fn test_target_containment_valid_and_invalid() {
    let temp = TempDir::new().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let paths = XdgPaths::custom(&base);

    let valid_target = base.join(".config/hypr/hyprland.conf");
    assert!(paths
        .validate_target_containment(&valid_target, ConfigComponent::Hyprland)
        .is_ok());

    let invalid_target = base.join(".config/autostart/malicious.desktop");
    let err = paths
        .validate_target_containment(&invalid_target, ConfigComponent::Hyprland)
        .unwrap_err();

    match err {
        WoderriseError::Path(PathError::ComponentRootEscape { path, allowed_root }) => {
            assert_eq!(path, invalid_target);
            assert_eq!(allowed_root, base.join(".config/hypr"));
        }
        other => panic!("Expected ComponentRootEscape, got: {:?}", other),
    }
}

#[test]
fn test_validate_identifier_rules() {
    assert!(validate_identifier("tx_20261027_141500_hyprland_7d8e9f").is_ok());
    assert!(validate_identifier("simple-id-123").is_ok());
    assert!(validate_identifier("VALID_ID_99").is_ok());

    assert!(validate_identifier("").is_err());
    assert!(validate_identifier("../escaped").is_err());
    assert!(validate_identifier("has spaces").is_err());
    assert!(validate_identifier("bad$char").is_err());
    assert!(validate_identifier("slash/in/id").is_err());
}

#[test]
fn test_target_containment_component_root_escape_traversal() {
    let temp = TempDir::new().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let paths = XdgPaths::custom(&base);

    // Adversarial test: Starts with root prefix lexically, but has ParentDir .. escaping to autostart
    let evil_target = base.join(".config/hypr/../autostart/bad.desktop");
    let err = paths
        .validate_target_containment(&evil_target, ConfigComponent::Hyprland)
        .unwrap_err();

    match err {
        WoderriseError::Path(PathError::PathTraversalAttempted { token, .. }) => {
            assert_eq!(token, "..");
        }
        other => panic!("Expected PathTraversalAttempted, got: {:?}", other),
    }

    // Direct path test
    let direct_evil = Utf8Path::new("/home/user/.config/hypr/../autostart/bad.desktop");
    assert!(paths.validate_target_containment(direct_evil, ConfigComponent::Hyprland).is_err());
}
