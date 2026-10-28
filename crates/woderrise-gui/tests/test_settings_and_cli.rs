//! Unit and integration tests for Settings screen state and CLI auth subcommands.

use std::fs;
use tempfile::tempdir;
use woderrise_core::models::load_settings;
use woderrise_core::paths::XdgPaths;
use woderrise_gui::cli::{handle_auth_command, handle_auth_command_with_paths, is_cli_invocation};
use woderrise_gui::screens::settings::{mask_token, SettingsState};

#[test]
fn test_mask_token_invariants() {
    // Empty token
    assert_eq!(mask_token(""), "");
    assert_eq!(mask_token("   "), "");

    // Short tokens (<= 8 characters)
    assert_eq!(mask_token("1234"), "••••••••");
    assert_eq!(mask_token("12345678"), "••••••••");

    // Standard GitHub PATs (> 8 characters)
    assert_eq!(mask_token("ghp_1234567890abcdef"), "ghp_••••••••cdef");
    assert_eq!(mask_token("ghp_abcdefghijkl1234"), "ghp_••••••••1234");
    assert_eq!(
        mask_token("github_pat_11ABCDEF_secret9876"),
        "gith••••••••9876"
    );

    // Multi-byte Unicode tokens (CJK, Emojis, Symbols)
    assert_eq!(mask_token("你好世界"), "••••••••");
    assert_eq!(mask_token("日本語テスト文字列"), "日本語テ••••••••ト文字列");
    assert_eq!(mask_token("ghp_🦀123"), "••••••••");
    assert_eq!(mask_token("ghp_🦀1234"), "ghp_••••••••1234");
    assert_eq!(mask_token("ghp_🦀123456789"), "ghp_••••••••6789");
}

#[test]
fn test_settings_state_transitions() {
    // 1. Initial unauthenticated state
    let mut state = SettingsState::new(None);
    assert!(!state.is_authenticated());
    assert!(state.token_input.is_empty());
    assert!(state.is_masked);
    assert!(!state.is_busy);
    assert!(state.status_notice.is_none());
    assert_eq!(state.masked_display(), "None (Unauthenticated)");

    // 2. Input change
    state.set_token_input("ghp_newtoken123456".to_string());
    assert_eq!(state.token_input, "ghp_newtoken123456");
    assert!(state.status_notice.is_none());

    // 3. Toggle masking
    state.toggle_mask();
    assert!(!state.is_masked);
    state.toggle_mask();
    assert!(state.is_masked);

    // 4. Set status notices
    state.set_notice("Testing error", true);
    assert_eq!(
        state.status_notice,
        Some(("Testing error".to_string(), true))
    );
    state.clear_notice();
    assert!(state.status_notice.is_none());

    // 5. Set saved authenticated token
    state.set_saved(Some("ghp_savedtoken9999".to_string()));
    assert!(state.is_authenticated());
    assert_eq!(state.saved_token.as_deref(), Some("ghp_savedtoken9999"));
    assert_eq!(state.masked_display(), "ghp_••••••••9999");
    assert!(!state.is_busy);
    assert!(state.status_notice.is_some());
    assert!(!state.status_notice.as_ref().unwrap().1); // not error

    // 6. Set saved None (clear token)
    state.set_saved(None);
    assert!(!state.is_authenticated());
    assert!(state.token_input.is_empty());
    assert_eq!(state.masked_display(), "None (Unauthenticated)");
}

#[test]
fn test_is_cli_invocation_logic() {
    // GUI launches
    assert!(!is_cli_invocation(&["woderrise".into()]));
    assert!(!is_cli_invocation(&["woderrise".into(), "gui".into()]));

    // CLI subcommands & flags
    assert!(is_cli_invocation(&["woderrise".into(), "auth".into()]));
    assert!(is_cli_invocation(&["woderrise".into(), "config".into()]));
    assert!(is_cli_invocation(&["woderrise".into(), "--help".into()]));
    assert!(is_cli_invocation(&["woderrise".into(), "-h".into()]));
    assert!(is_cli_invocation(&["woderrise".into(), "help".into()]));
    assert!(is_cli_invocation(&["woderrise".into(), "--version".into()]));
    assert!(is_cli_invocation(&["woderrise".into(), "-V".into()]));
    assert!(is_cli_invocation(&["woderrise".into(), "version".into()]));

    // Unknown arguments do not trigger CLI mode
    assert!(!is_cli_invocation(&["woderrise".into(), "browse".into()]));

    // Auth help invocation
    assert!(handle_auth_command(&["--help".into()]).is_ok());
}

#[test]
fn test_cli_auth_token_save_and_permissions() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let settings_file = xdg.settings_file();

    // 1. Save token via auth CLI
    let args = vec!["--token".to_string(), "ghp_testing0123456789".to_string()];
    let res = handle_auth_command_with_paths(&args, &xdg);
    assert!(
        res.is_ok(),
        "handle_auth_command_with_paths failed: {:?}",
        res
    );

    // 2. Verify file exists on disk
    assert!(
        settings_file.exists(),
        "Settings file was not created: {}",
        settings_file
    );

    // 3. Verify POSIX 0600 permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Failed to read metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "Expected POSIX 0600 permissions on settings.json, got {:o}",
            mode
        );
    }

    // 4. Verify deserialized content
    let settings = load_settings(&settings_file).expect("Failed to load saved settings");
    assert_eq!(
        settings.github_token.as_deref(),
        Some("ghp_testing0123456789")
    );

    // 5. Test --status command with token present
    let status_res = handle_auth_command_with_paths(&["--status".to_string()], &xdg);
    assert!(status_res.is_ok());

    // 6. Test --clear command
    let clear_res = handle_auth_command_with_paths(&["--clear".to_string()], &xdg);
    assert!(clear_res.is_ok());

    let cleared_settings = load_settings(&settings_file).expect("Failed to load cleared settings");
    assert_eq!(cleared_settings.github_token, None);

    // 7. Verify permissions remain 0600 after clear
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Failed to read metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn test_cli_auth_validation_errors() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);

    // Missing token argument value
    let res_missing = handle_auth_command_with_paths(&["--token".to_string()], &xdg);
    assert!(res_missing.is_err());

    // Empty token string
    let res_empty =
        handle_auth_command_with_paths(&["--token".to_string(), "   ".to_string()], &xdg);
    assert!(res_empty.is_err());

    // Unknown option
    let res_unknown = handle_auth_command_with_paths(&["--invalid-flag".to_string()], &xdg);
    assert!(res_unknown.is_err());
}
