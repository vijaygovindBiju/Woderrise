//! Adversarial empirical challenge tests for Milestone M5:
//! PAT Settings Integration & Dynamic Rate Limits (R3).

use std::fs;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use woderrise_core::models::{
    load_settings, save_settings_durably, Settings, CURRENT_SETTINGS_VERSION,
};
use woderrise_core::paths::XdgPaths;
use woderrise_detect::DetectionEngine;
use woderrise_github::GitHubClientBuilder;
use woderrise_gui::app::{App, Message, Screen};
use woderrise_gui::cli::handle_auth_command_with_paths;
use woderrise_gui::run_worker;
use woderrise_gui::screens::settings::mask_token;
use woderrise_gui::{EngineEvent, GuiCommand};
use woderrise_transaction::TransactionEngine;

/// 1. Adversarial test: Token masking must NOT panic on non-ASCII multi-byte UTF-8 inputs.
///
/// Current implementation in `mask_token`:
/// `let prefix = &t[..4];`
/// `let suffix = &t[t.len() - 4..];`
/// Slices by raw byte offset instead of UTF-8 character boundaries.
///
/// When byte 4 or byte `len - 4` falls within a multi-byte UTF-8 sequence,
/// Rust panics with "byte index is not a char boundary".
#[test]
fn test_challenge_mask_token_unicode_slice_panics() {
    let cases = [
        ("你好世界", "••••••••"),
        ("日本語のトークン1234", "日本語の••••••••1234"),
        ("a🦀🦀🦀", "••••••••"),
    ];

    for (case, expected) in cases {
        let res = std::panic::catch_unwind(|| mask_token(case));
        assert!(
            res.is_ok(),
            "mask_token must NOT panic on UTF-8 case: {}",
            case
        );
        assert_eq!(res.unwrap(), expected);
    }
}

/// 2. Adversarial test: Short tokens (<= 8 characters) must be completely masked as "••••••••".
///
/// In `mask_token`:
/// `if t.len() <= 8`
/// Uses byte length instead of character count (`.chars().count()`).
/// A 3-character emoji string `🦀🦀🦀` has byte length 12 (> 8), so it bypasses `t.len() <= 8`
/// and reveals the first and last emoji!
#[test]
fn test_challenge_mask_token_char_count_vs_byte_length() {
    let three_emoji = "🦀🦀🦀"; // 3 characters, 12 bytes
    assert_eq!(three_emoji.chars().count(), 3);
    assert_eq!(three_emoji.len(), 12);

    let res = mask_token(three_emoji);
    // Tokens with <= 8 characters must be completely masked as "••••••••"
    assert_eq!(
        res, "••••••••",
        "Tokens with <=8 characters must be completely masked"
    );
}

/// 3. Adversarial test: Extreme lengths and whitespace edge cases.
#[test]
fn test_challenge_mask_token_extreme_lengths_and_whitespace() {
    // Empty & whitespace
    assert_eq!(mask_token(""), "");
    assert_eq!(mask_token("   \t\n  "), "");

    // Exactly 1 char
    assert_eq!(mask_token("a"), "••••••••");
    // Exactly 8 chars
    assert_eq!(mask_token("12345678"), "••••••••");
    // Exactly 9 chars
    assert_eq!(mask_token("123456789"), "1234••••••••6789");

    // Extreme 10,000 character token
    let long_token = "a".repeat(10_000);
    let masked_long = mask_token(&long_token);
    // In chars: 4 prefix + 8 bullets + 4 suffix = 16 chars
    assert_eq!(masked_long.chars().count(), 16);
    assert_eq!(masked_long, "aaaa••••••••aaaa");
}

/// 4. Adversarial test: SettingsTestConnection must NOT falsely save uncommitted draft tokens.
///
/// In `crates/woderrise-gui/src/app.rs`:
/// When `SettingsTestConnection` is clicked, `settings_state.is_busy` is set to `true`.
/// When `EngineEvent::RateLimitsUpdated` arrives, `app.rs` checks `if self.settings_state.is_busy`
/// and immediately calls `self.settings_state.set_saved(token)` with `self.settings_state.token_input`!
/// This causes unsaved, unverified drafts in the input field to be marked as saved, while never
/// actually saving to disk or calling `SetGitHubToken`.
#[test]
fn test_challenge_settings_test_connection_does_not_corrupt_saved_token() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let initial_token = "ghp_committed_valid_token_1234".to_string();
    let mut app = App::new(cmd_tx, None);
    app.settings_state.set_saved(Some(initial_token.clone()));

    // Drain startup commands
    let _ = cmd_rx.try_recv();
    let _ = cmd_rx.try_recv();
    let _ = cmd_rx.try_recv();

    // User navigates to Settings
    let _ = app.update(Message::NavigateTo(Screen::Settings));
    let _ = cmd_rx.try_recv(); // FetchRateLimits on enter

    // User starts typing a new draft token in the input box, but does NOT click Save
    let draft_token = "ghp_uncommitted_draft_token_9999".to_string();
    let _ = app.update(Message::SettingsTokenChanged(draft_token.clone()));
    assert_eq!(app.settings_state.token_input, draft_token);
    assert_eq!(app.settings_state.saved_token, Some(initial_token.clone()));

    // User clicks "Test Connection"
    let _ = app.update(Message::SettingsTestConnection);
    assert!(app.settings_state.is_busy);
    match cmd_rx.try_recv() {
        Ok(GuiCommand::FetchRateLimits) => {}
        other => panic!("Expected FetchRateLimits command, got {:?}", other),
    }

    // Engine responds with RateLimitsUpdated
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Core,
            limit: 5000,
            remaining: 4999,
            reset_time: chrono::Utc::now(),
            used: 1,
        },
        search: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Search,
            limit: 30,
            remaining: 30,
            reset_time: chrono::Utc::now(),
            used: 0,
        },
    }));

    // Invariant: TestConnection must NOT overwrite saved_token with uncommitted draft
    let retained_saved_token = app.settings_state.saved_token.clone();
    assert_eq!(
        retained_saved_token,
        Some(initial_token.clone()),
        "TestConnection must retain saved_token intact and not overwrite with uncommitted draft!"
    );

    // Verify notice does NOT falsely claim token was saved
    let notice = app.settings_state.status_notice.as_ref().unwrap();
    assert!(
        !notice
            .0
            .contains("Personal Access Token saved and verified"),
        "UI must NOT display 'saved and verified' banner on TestConnection!"
    );
    assert!(
        notice.0.contains("Connection verified") || notice.0.contains("Active limits"),
        "UI should display connection verified notice"
    );
    assert!(!app.settings_state.is_busy);
}

/// 5. CLI Verification: Auth commands lifecycle, isolated filesystem, and POSIX 0600 mode.
#[test]
fn test_challenge_cli_auth_lifecycle_and_security() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let settings_file = xdg.settings_file();

    // 1. Initial status: unauthenticated
    let res_status1 = handle_auth_command_with_paths(&["--status".into()], &xdg);
    assert!(res_status1.is_ok());

    // 2. Save valid PAT
    let pat = "ghp_securepatforwoderrise0123456789";
    let res_save = handle_auth_command_with_paths(&["--token".into(), pat.into()], &xdg);
    assert!(res_save.is_ok());

    // Check POSIX 0600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Failed to read metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "Settings file permissions MUST be 0600, got {:o}",
            mode
        );
    }

    // Verify disk content
    let settings = load_settings(&settings_file).expect("Failed to load settings");
    assert_eq!(settings.github_token.as_deref(), Some(pat));

    // 3. Status with PAT active
    let res_status2 = handle_auth_command_with_paths(&["--status".into()], &xdg);
    assert!(res_status2.is_ok());

    // 4. Clear PAT
    let res_clear = handle_auth_command_with_paths(&["--clear".into()], &xdg);
    assert!(res_clear.is_ok());

    let cleared = load_settings(&settings_file).expect("Failed to load settings");
    assert_eq!(cleared.github_token, None);

    // Permissions must remain 0600 after clear
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Failed to read metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

/// 6. Error handling test: Saving an invalid token must display error notice and error banner.
#[test]
fn test_challenge_token_save_verification_failure_displays_error_banner() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let initial_token = "ghp_committed_valid_token_1234".to_string();
    let mut app = App::new(cmd_tx, None);
    app.settings_state.set_saved(Some(initial_token.clone()));

    // Drain startup commands
    let _ = cmd_rx.try_recv();
    let _ = cmd_rx.try_recv();
    let _ = cmd_rx.try_recv();

    let _ = app.update(Message::NavigateTo(Screen::Settings));
    let _ = cmd_rx.try_recv();

    // User types an invalid PAT
    let invalid_token = "ghp_revoked_or_invalid_pat_0000".to_string();
    let _ = app.update(Message::SettingsTokenChanged(invalid_token.clone()));

    // User clicks "Save Token"
    let _ = app.update(Message::SettingsSaveToken);
    assert!(app.settings_state.is_busy);
    match cmd_rx.try_recv() {
        Ok(GuiCommand::SetGitHubToken { token }) => {
            assert_eq!(token, Some(invalid_token));
        }
        other => panic!("Expected SetGitHubToken command, got {:?}", other),
    }

    // Engine responds with TokenVerificationFailed
    let _ = app.update(Message::Engine(EngineEvent::TokenVerificationFailed {
        error: "GitHub API HTTP 401 Unauthorized: Bad credentials".to_string(),
    }));

    // Invariants:
    // 1. App displays error banner
    assert!(
        app.error_banner.is_some(),
        "App must display error banner on token verification failure"
    );
    assert!(app
        .error_banner
        .as_ref()
        .unwrap()
        .contains("401 Unauthorized"));

    // 2. Settings notice displays error
    let notice = app
        .settings_state
        .status_notice
        .as_ref()
        .expect("Notice must be set");
    assert!(
        notice.1,
        "Status notice must be flagged as error (is_error = true)"
    );
    assert!(notice.0.contains("401 Unauthorized"));

    // 3. Saved token is NOT updated to invalid token
    assert_eq!(
        app.settings_state.saved_token,
        Some(initial_token),
        "saved_token must remain original and NOT be set to invalid token"
    );

    // 4. is_busy is cleared
    assert!(!app.settings_state.is_busy);
}

/// 7. Adversarial test: Clearing token_input and clicking Test Connection must NOT reset saved_token to None.
#[test]
fn test_challenge_settings_test_connection_cleared_input_preserves_saved_token() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let initial_token = "ghp_committed_valid_token_1234".to_string();
    let mut app = App::new(cmd_tx, None);
    app.settings_state.set_saved(Some(initial_token.clone()));

    // Drain startup commands
    let _ = cmd_rx.try_recv();
    let _ = cmd_rx.try_recv();
    let _ = cmd_rx.try_recv();

    // User navigates to Settings
    let _ = app.update(Message::NavigateTo(Screen::Settings));
    let _ = cmd_rx.try_recv(); // FetchRateLimits on enter

    // User clears token_input to empty string
    let _ = app.update(Message::SettingsTokenChanged(String::new()));
    assert_eq!(app.settings_state.token_input, "");
    assert_eq!(app.settings_state.saved_token, Some(initial_token.clone()));

    // User clicks "Test Connection"
    let _ = app.update(Message::SettingsTestConnection);
    assert!(app.settings_state.is_busy);
    assert!(app.is_testing_connection);
    assert_eq!(app.pending_token_save, None);

    match cmd_rx.try_recv() {
        Ok(GuiCommand::FetchRateLimits) => {}
        other => panic!("Expected FetchRateLimits command, got {:?}", other),
    }

    // Engine responds with RateLimitsUpdated
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Core,
            limit: 5000,
            remaining: 4800,
            reset_time: chrono::Utc::now(),
            used: 200,
        },
        search: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Search,
            limit: 30,
            remaining: 25,
            reset_time: chrono::Utc::now(),
            used: 5,
        },
    }));

    // Invariants:
    // 1. saved_token must remain initial_token and NOT be reset to None!
    assert_eq!(
        app.settings_state.saved_token,
        Some(initial_token),
        "Clearing input and testing connection must NOT reset saved_token to None!"
    );
    // 2. is_busy must be cleared
    assert!(!app.settings_state.is_busy);
    // 3. is_testing_connection must be cleared
    assert!(!app.is_testing_connection);
    // 4. Status notice must reflect connection verification without claiming save or clear
    let notice = app
        .settings_state
        .status_notice
        .as_ref()
        .expect("Notice must be present");
    assert!(!notice.1, "Notice should not be an error");
    assert!(
        notice.0.contains("Connection verified"),
        "Notice must contain connection verified, got: {}",
        notice.0
    );
    assert!(
        !notice.0.contains("cleared"),
        "Notice must NOT claim token was cleared"
    );
    assert!(
        !notice.0.contains("saved and verified"),
        "Notice must NOT claim token was saved"
    );
}

/// 8. Adversarial test: Typing whitespace in token_input and clicking Test Connection preserves saved_token.
#[test]
fn test_challenge_settings_test_connection_whitespace_input_preserves_saved_token() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let initial_token = "ghp_committed_valid_token_5678".to_string();
    let mut app = App::new(cmd_tx, None);
    app.settings_state.set_saved(Some(initial_token.clone()));

    // Drain startup commands
    let _ = cmd_rx.try_recv();
    let _ = cmd_rx.try_recv();
    let _ = cmd_rx.try_recv();

    let _ = app.update(Message::NavigateTo(Screen::Settings));
    let _ = cmd_rx.try_recv();

    // User types whitespace into token_input
    let _ = app.update(Message::SettingsTokenChanged("   \t  ".to_string()));
    assert_eq!(app.settings_state.token_input, "   \t  ");
    assert_eq!(app.settings_state.saved_token, Some(initial_token.clone()));

    // User clicks "Test Connection"
    let _ = app.update(Message::SettingsTestConnection);
    assert!(app.settings_state.is_busy);
    assert!(app.is_testing_connection);

    let _ = cmd_rx.try_recv();

    // Engine responds with RateLimitsUpdated
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Core,
            limit: 5000,
            remaining: 4500,
            reset_time: chrono::Utc::now(),
            used: 500,
        },
        search: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Search,
            limit: 30,
            remaining: 20,
            reset_time: chrono::Utc::now(),
            used: 10,
        },
    }));

    // Invariant: saved_token must still be Some(initial_token)
    assert_eq!(
        app.settings_state.saved_token,
        Some(initial_token),
        "Whitespace input and Test Connection must not reset saved_token"
    );
    assert!(!app.settings_state.is_busy);
}

/// 9. Empirical test: run_worker receiving invalid token (HTTP 401 Unauthorized from API)
/// must revert client token, NOT overwrite disk settings.json, and emit TokenVerificationFailed.
#[tokio::test]
async fn test_challenge_run_worker_token_save_mocked_401_unauthorized() {
    // 1. Spin up mock HTTP server responding 401 Unauthorized to /rate_limit
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", port);

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let response = "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 35\r\nConnection: close\r\n\r\n{\"message\": \"Bad credentials\"}";
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.flush().await;
        }
    });

    // 2. Setup isolated temporary XDG environment
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let settings_path = xdg.settings_file();

    // Pre-seed settings.json with prior verified token
    let prior_token = "ghp_prior_verified_token_1111".to_string();
    let prior_settings = Settings {
        version: CURRENT_SETTINGS_VERSION,
        github_token: Some(prior_token.clone()),
        auto_backup: true,
        theme: "dark".into(),
    };
    save_settings_durably(&settings_path, &prior_settings).expect("save initial settings");

    // Check permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&settings_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // 3. Build GitHubClient pointing to mock server
    let github_client = GitHubClientBuilder::new()
        .base_url(&mock_url)
        .auth_token(Some(prior_token.clone()))
        .build()
        .expect("build client");

    let tx_engine = TransactionEngine::new(xdg.clone()).expect("tx engine");
    let detection_engine = DetectionEngine::new();

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(10);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(10);

    // 4. Run worker in background
    let worker_handle = tokio::spawn(async move {
        run_worker(cmd_rx, event_tx, github_client, detection_engine, tx_engine).await;
    });

    // 5. Send SetGitHubToken with invalid token
    let invalid_token = "ghp_invalid_token_rejected_by_api_9999".to_string();
    cmd_tx
        .send(GuiCommand::SetGitHubToken {
            token: Some(invalid_token.clone()),
        })
        .await
        .expect("send command");

    // 6. Receive emitted event
    let event = event_rx.recv().await.expect("receive engine event");
    match &event {
        EngineEvent::TokenVerificationFailed { error } => {
            assert!(
                error.contains("401")
                    || error.contains("Unauthorized")
                    || error.contains("Bad credentials"),
                "Error message must mention 401/Unauthorized, got: {}",
                error
            );
        }
        other => panic!("Expected TokenVerificationFailed, got {:?}", other),
    }

    // 7. Disk verification: settings.json must NOT be modified or corrupted
    let loaded = load_settings(&settings_path).expect("load settings from disk");
    assert_eq!(
        loaded.github_token,
        Some(prior_token.clone()),
        "settings.json must NOT be updated with the invalid token!"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&settings_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "Permissions must remain 0600");
    }

    // 8. GUI App state verification with this event
    let (gui_cmd_tx, _gui_cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(gui_cmd_tx, None);
    app.settings_state.set_saved(Some(prior_token.clone()));
    app.settings_state.is_busy = true;
    app.pending_token_save = Some(Some(invalid_token.clone()));

    let _ = app.update(Message::Engine(event));

    // App error banner must be populated
    assert!(
        app.error_banner.is_some(),
        "App must display error banner on token verification failure"
    );
    assert!(
        app.error_banner.as_ref().unwrap().contains("401")
            || app.error_banner.as_ref().unwrap().contains("Unauthorized")
    );

    // Status notice must indicate error
    let notice = app
        .settings_state
        .status_notice
        .as_ref()
        .expect("Notice must be set");
    assert!(notice.1, "is_error must be true");

    // saved_token must remain prior_token
    assert_eq!(app.settings_state.saved_token, Some(prior_token));
    assert!(!app.settings_state.is_busy);
    assert_eq!(app.pending_token_save, None);

    // Shutdown worker
    drop(cmd_tx);
    let _ = worker_handle.await;
}

/// 10. Empirical test: run_worker receiving invalid token when network drops abruptly
/// must NOT corrupt disk settings.json and must emit TokenVerificationFailed.
#[tokio::test]
async fn test_challenge_run_worker_token_save_network_drop() {
    // Mock TCP listener that immediately drops the socket
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", port);

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            drop(stream); // abrupt network drop
        }
    });

    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let settings_path = xdg.settings_file();

    let prior_token = "ghp_prior_token_before_drop".to_string();
    let prior_settings = Settings {
        version: CURRENT_SETTINGS_VERSION,
        github_token: Some(prior_token.clone()),
        auto_backup: true,
        theme: "system".into(),
    };
    save_settings_durably(&settings_path, &prior_settings).expect("save initial settings");

    let github_client = GitHubClientBuilder::new()
        .base_url(&mock_url)
        .auth_token(Some(prior_token.clone()))
        .build()
        .expect("build client");

    let tx_engine = TransactionEngine::new(xdg.clone()).expect("tx engine");
    let detection_engine = DetectionEngine::new();

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(10);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(10);

    let worker_handle = tokio::spawn(async move {
        run_worker(cmd_rx, event_tx, github_client, detection_engine, tx_engine).await;
    });

    cmd_tx
        .send(GuiCommand::SetGitHubToken {
            token: Some("ghp_network_drop_token".into()),
        })
        .await
        .expect("send command");

    let event = event_rx.recv().await.expect("receive engine event");
    match &event {
        EngineEvent::TokenVerificationFailed { error } => {
            assert!(!error.is_empty());
        }
        other => panic!(
            "Expected TokenVerificationFailed on network drop, got {:?}",
            other
        ),
    }

    // Disk settings must be untouched
    let loaded = load_settings(&settings_path).expect("load settings");
    assert_eq!(loaded.github_token, Some(prior_token));

    // Shutdown worker
    drop(cmd_tx);
    let _ = worker_handle.await;
}

/// 11. Empirical test: run_worker receiving valid token (HTTP 200 OK from API)
/// must durably persist settings.json with 0600 permissions and emit RateLimitsUpdated.
#[tokio::test]
async fn test_challenge_run_worker_token_save_valid_token_persists_durably() {
    // Mock HTTP server responding 200 OK with valid rate limit JSON
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", port);

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = r#"{"resources":{"core":{"limit":5000,"remaining":4995,"reset":1761500000,"used":5},"search":{"limit":30,"remaining":29,"reset":1761500000,"used":1}}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.flush().await;
        }
    });

    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let settings_path = xdg.settings_file();

    let github_client = GitHubClientBuilder::new()
        .base_url(&mock_url)
        .build()
        .expect("build client");

    let tx_engine = TransactionEngine::new(xdg.clone()).expect("tx engine");
    let detection_engine = DetectionEngine::new();

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(10);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(10);

    let worker_handle = tokio::spawn(async move {
        run_worker(cmd_rx, event_tx, github_client, detection_engine, tx_engine).await;
    });

    let new_token = "ghp_genuine_valid_token_12345".to_string();
    cmd_tx
        .send(GuiCommand::SetGitHubToken {
            token: Some(new_token.clone()),
        })
        .await
        .expect("send command");

    let event = event_rx.recv().await.expect("receive engine event");
    match &event {
        EngineEvent::RateLimitsUpdated { core, search } => {
            assert_eq!(core.limit, 5000);
            assert_eq!(core.remaining, 4995);
            assert_eq!(search.limit, 30);
            assert_eq!(search.remaining, 29);
        }
        other => panic!("Expected RateLimitsUpdated, got {:?}", other),
    }

    // Disk settings must have new token and 0600 mode
    let loaded = load_settings(&settings_path).expect("load settings");
    assert_eq!(loaded.github_token, Some(new_token.clone()));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&settings_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // GUI App state update
    let (gui_cmd_tx, _gui_cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(gui_cmd_tx, None);
    app.settings_state.is_busy = true;
    app.pending_token_save = Some(Some(new_token.clone()));

    let _ = app.update(Message::Engine(event));

    assert_eq!(app.settings_state.saved_token, Some(new_token));
    assert!(!app.settings_state.is_busy);
    assert_eq!(app.error_banner, None);

    drop(cmd_tx);
    let _ = worker_handle.await;
}
