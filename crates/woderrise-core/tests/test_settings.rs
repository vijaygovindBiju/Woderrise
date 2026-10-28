//! Integration tests for Settings model, serialization, durability, and POSIX 0600 permissions.

use camino::Utf8PathBuf;
use std::fs;
use tempfile::TempDir;
use woderrise_core::models::{
    load_settings, save_settings_durably, Settings, CURRENT_SETTINGS_VERSION,
};

#[test]
fn test_settings_default_values() {
    let settings = Settings::default();
    assert_eq!(settings.version, CURRENT_SETTINGS_VERSION);
    assert_eq!(settings.github_token, None);
    assert!(settings.auto_backup);
    assert_eq!(settings.theme, "system");
}

#[test]
fn test_settings_serde_roundtrip() {
    let settings = Settings {
        version: CURRENT_SETTINGS_VERSION,
        github_token: Some("ghp_test1234567890abcdef".to_string()),
        auto_backup: false,
        theme: "dark".to_string(),
    };

    let serialized = serde_json::to_string_pretty(&settings).expect("serialize settings");
    let deserialized: Settings = serde_json::from_str(&serialized).expect("deserialize settings");

    assert_eq!(settings, deserialized);
}

#[test]
fn test_save_settings_durably_creates_file_with_0600_permissions() {
    let temp = TempDir::new().expect("create temp dir");
    let settings_path =
        Utf8PathBuf::from_path_buf(temp.path().join("config/woderrise/settings.json"))
            .expect("valid utf8");

    let settings = Settings {
        version: CURRENT_SETTINGS_VERSION,
        github_token: Some("ghp_secret_token_12345".to_string()),
        auto_backup: true,
        theme: "system".to_string(),
    };

    save_settings_durably(&settings_path, &settings).expect("save settings");
    assert!(settings_path.exists());

    // Check POSIX 0600 permissions on Unix platforms
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_path).expect("read metadata");
        let mode = metadata.permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "Settings file permissions must be 0600 (owner read/write only)"
        );
    }

    // Load and verify content matches
    let loaded = load_settings(&settings_path).expect("load settings");
    assert_eq!(settings, loaded);
}

#[test]
fn test_load_settings_missing_returns_default() {
    let temp = TempDir::new().expect("create temp dir");
    let non_existent =
        Utf8PathBuf::from_path_buf(temp.path().join("missing.json")).expect("valid utf8");

    let loaded = load_settings(&non_existent).expect("load missing settings");
    assert_eq!(loaded, Settings::default());
}

#[test]
fn test_save_settings_durably_overwrites_atomically() {
    let temp = TempDir::new().expect("create temp dir");
    let settings_path =
        Utf8PathBuf::from_path_buf(temp.path().join("settings.json")).expect("valid utf8");

    let initial = Settings {
        version: 1,
        github_token: Some("token_1".to_string()),
        auto_backup: true,
        theme: "light".to_string(),
    };
    save_settings_durably(&settings_path, &initial).expect("save initial");

    let updated = Settings {
        version: 1,
        github_token: None,
        auto_backup: false,
        theme: "dark".to_string(),
    };
    save_settings_durably(&settings_path, &updated).expect("save updated");

    let loaded = load_settings(&settings_path).expect("load updated");
    assert_eq!(loaded, updated);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_path).expect("read metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
}
