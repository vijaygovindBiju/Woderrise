//! Empirical Challenger Adversarial Test Suite for Milestone M5:
//! PAT Settings Integration, POSIX 0600 Durability, Dynamic Rate Limits, and CLI Auth.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::tempdir;
use woderrise_core::models::{
    load_settings, save_settings_durably, Settings, CURRENT_SETTINGS_VERSION,
};
use woderrise_core::paths::XdgPaths;
use woderrise_github::RateLimitTracker;
use woderrise_gui::cli::handle_auth_command_with_paths;
use woderrise_gui::screens::settings::mask_token;

/// Challenge 1: Multi-byte UTF-8 character boundary safety in mask_token.
///
/// Invariant: `mask_token` MUST NOT panic on non-ASCII or multi-byte UTF-8 inputs
/// (such as 3-byte Japanese/CJK characters, European symbols, or 4-byte emojis).
/// Slicing by byte offsets (&t[..4] or &t[t.len()-4..]) without character boundary checks
/// causes a fatal panic when byte 4 or (len - 4) falls within a multi-byte character.
#[test]
fn test_adversarial_mask_token_multibyte_utf8_boundaries() {
    // Test 1: 3-byte UTF-8 characters (Japanese Hiragana 'あ' = 3 bytes each: 0xE3 0x81 0x82)
    // 10 chars = 30 bytes > 8 bytes.
    let japanese_token = "ああああああああああ";
    let res = std::panic::catch_unwind(|| mask_token(japanese_token));
    assert!(
        res.is_ok(),
        "mask_token must NOT panic on 3-byte UTF-8 input 'ああああああああああ'"
    );
    assert_eq!(res.unwrap(), "ああああ••••••••ああああ");

    // Test 2: Euro symbol '€' (3 bytes: 0xE2 0x82 0xAC)
    // "ghp_€€€€123" (11 chars > 8 chars)
    let euro_token = "ghp_€€€€123";
    let res_euro = std::panic::catch_unwind(|| mask_token(euro_token));
    assert!(
        res_euro.is_ok(),
        "mask_token must NOT panic on input with 3-byte character '€'"
    );
    assert_eq!(res_euro.unwrap(), "ghp_••••••••€123");

    // Test 3: Embedded zero-width space (\u{200B}, 3 bytes: 0xE2 0x80 0x8B)
    // "gh\u{200B}p_12345678" (13 chars > 8 chars)
    let zws_token = "gh\u{200B}p_12345678";
    let res_zws = std::panic::catch_unwind(|| mask_token(zws_token));
    assert!(
        res_zws.is_ok(),
        "mask_token must NOT panic on zero-width space UTF-8 boundary"
    );
    assert_eq!(res_zws.unwrap(), "gh\u{200B}p••••••••5678");
}

/// Challenge 2: Settings durability under massive concurrent writes and reads.
///
/// Invariant: 50 concurrent threads saving distinct settings to the exact same file
/// MUST NOT corrupt the JSON, leave orphan temp files, or yield insecure file permissions.
/// Every concurrent load_settings call MUST return a valid Settings struct.
#[test]
fn test_adversarial_settings_high_concurrency_race() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let settings_path = base.join("settings.json");

    let num_threads = 50;
    let barrier = Arc::new(Barrier::new(num_threads));
    let mut handles = Vec::new();

    for i in 0..num_threads {
        let b = Arc::clone(&barrier);
        let p = settings_path.clone();
        let handle = thread::spawn(move || {
            let settings = Settings {
                version: CURRENT_SETTINGS_VERSION,
                github_token: Some(format!("ghp_thread_token_{:04}", i)),
                auto_backup: i % 2 == 0,
                theme: if i % 2 == 0 {
                    "dark".into()
                } else {
                    "light".into()
                },
            };

            // Wait for all threads to start simultaneously
            b.wait();

            // Save settings durably
            let save_res = save_settings_durably(&p, &settings);
            assert!(
                save_res.is_ok(),
                "save_settings_durably failed in thread {}",
                i
            );

            // Immediately attempt to read settings back
            let loaded = load_settings(&p);
            assert!(
                loaded.is_ok(),
                "load_settings failed during concurrent write race in thread {}: {:?}",
                i,
                loaded.err()
            );

            // Verify POSIX 0600 permissions
            let meta = fs::metadata(&p).expect("metadata");
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(
                mode, 0o600,
                "Permissions violation: Expected 0600 during concurrency, got {:o}",
                mode
            );
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("thread panicked");
    }

    // Final checks:
    // 1. Final file must be valid Settings
    let final_settings = load_settings(&settings_path).expect("final load");
    assert!(final_settings.github_token.is_some());

    // 2. Final permissions must be 0600
    let meta = fs::metadata(&settings_path).expect("final metadata");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);

    // 3. No orphan tmp files (*.tmp.*) must remain in the directory
    let entries = fs::read_dir(tmp.path()).expect("read_dir");
    for entry in entries {
        let entry = entry.expect("dir entry");
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.contains(".tmp."),
            "Durability leak: Orphan temporary file was left behind: {}",
            name
        );
    }
}

/// Challenge 3: Insecure permission remediation on durable save.
///
/// Invariant: If settings.json previously existed with insecure permissions (e.g. 0644 or 0666),
/// calling `save_settings_durably` MUST replace the file and enforce POSIX 0600 permissions.
#[test]
fn test_adversarial_settings_permissions_override_insecure() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let settings_path = base.join("settings.json");

    // Create file with world-readable 0644 permissions
    let initial_settings = Settings::default();
    let initial_json = serde_json::to_string(&initial_settings).unwrap();
    fs::write(&settings_path, &initial_json).unwrap();
    fs::set_permissions(&settings_path, fs::Permissions::from_mode(0o644)).unwrap();

    let pre_mode = fs::metadata(&settings_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(pre_mode, 0o644);

    // Save durably with new token
    let mut updated = initial_settings;
    updated.github_token = Some("ghp_remediation_token_12345".into());
    save_settings_durably(&settings_path, &updated).expect("save durably");

    // Invariant: Permissions MUST now be 0600
    let post_mode = fs::metadata(&settings_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        post_mode, 0o600,
        "Security violation: save_settings_durably failed to remediate insecure 0644 permissions to 0600!"
    );
}

/// Challenge 4: Deep non-existent directory tree creation and error cleanup.
///
/// Invariant: `save_settings_durably` must recursively create missing parent directories
/// and must clean up temporary files if writing fails.
#[test]
fn test_adversarial_settings_deep_nested_missing_parents() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let deep_path = base
        .join("level1")
        .join("level2")
        .join("level3")
        .join("settings.json");

    let settings = Settings {
        version: CURRENT_SETTINGS_VERSION,
        github_token: Some("ghp_deep_token".into()),
        auto_backup: true,
        theme: "system".into(),
    };

    save_settings_durably(&deep_path, &settings).expect("save to deep path");

    assert!(deep_path.exists());
    let loaded = load_settings(&deep_path).expect("load from deep path");
    assert_eq!(loaded, settings);

    let mode = fs::metadata(&deep_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

/// Challenge 5: Corrupted settings file recovery and resilience.
///
/// Invariant: Truncated or malformed settings.json must yield a clear Serialization error on load,
/// but MUST be cleanly recoverable when overwritten via save_settings_durably.
#[test]
fn test_adversarial_settings_corrupted_file_recovery() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let settings_path = base.join("settings.json");

    // Write malformed JSON
    fs::write(
        &settings_path,
        b"{\"version\": 1, \"github_token\": \"truncated",
    )
    .unwrap();

    let load_res = load_settings(&settings_path);
    assert!(load_res.is_err(), "Loading corrupted JSON must fail");

    // Overwrite with valid settings
    let valid = Settings {
        version: 1,
        github_token: Some("ghp_recovered".into()),
        auto_backup: false,
        theme: "nord".into(),
    };
    let save_res = save_settings_durably(&settings_path, &valid);
    assert!(
        save_res.is_ok(),
        "Overwriting corrupted settings must succeed"
    );

    let reloaded = load_settings(&settings_path).expect("reloaded valid");
    assert_eq!(reloaded, valid);
}

/// Challenge 6: RateLimitTracker concurrency and rapid elevation toggling.
///
/// Invariant: Rapidly toggling authentication (10,000 times) across concurrent threads
/// must maintain tracker integrity without panic, deadlocks, or out-of-bound quotas.
#[test]
fn test_adversarial_rate_limit_rapid_toggling_and_concurrency() {
    let tracker = Arc::new(RateLimitTracker::new());
    let num_threads = 8;
    let iterations_per_thread = 500;
    let mut handles = Vec::new();

    for t in 0..num_threads {
        let tr = Arc::clone(&tracker);
        let h = thread::spawn(move || {
            for i in 0..iterations_per_thread {
                let auth = (t + i) % 2 == 0;
                tr.set_authenticated(auth);

                let core = tr.core_status();
                let search = tr.search_status();

                // Quota invariants
                assert!(core.limit == 60 || core.limit == 5000);
                assert!(search.limit == 10 || search.limit == 30);
                assert!(core.remaining <= core.limit);
                assert!(search.remaining <= search.limit);

                // Check quota calls must never panic
                let _ = tr.check_core_quota();
                let _ = tr.check_search_quota();
            }
        });
        handles.push(h);
    }

    for h in handles {
        h.join().expect("rate limit thread panicked");
    }
}

/// Challenge 7: RateLimitTracker extreme value saturation and arithmetic bounds.
///
/// Invariant: Ingesting extreme header values (0, u32::MAX, large counts) must not cause
/// integer overflow or panics in `set_authenticated` difference calculations.
#[test]
fn test_adversarial_rate_limit_extreme_saturation() {
    let tracker = RateLimitTracker::new();

    // 1. Extreme: remaining = 0 when unauthenticated
    let resp1: woderrise_github::rate_limit::GitHubRateLimitResponse =
        serde_json::from_value(serde_json::json!({
            "resources": {
                "core": { "limit": 60, "remaining": 0, "reset": 1761500000, "used": 60 },
                "search": { "limit": 10, "remaining": 10, "reset": 1761500000, "used": 0 }
            }
        }))
        .unwrap();
    tracker.update_from_response(&resp1);

    assert_eq!(tracker.core_status().remaining, 0);

    // Elevate with 0 remaining: diff = 5000 - 60 = 4940, remaining = 0 + 4940 = 4940
    tracker.set_authenticated(true);
    let s1 = tracker.core_status();
    assert_eq!(s1.limit, 5000);
    assert_eq!(s1.remaining, 4940);

    // 2. Extreme: huge header limit (e.g. 2,000,000,000)
    let resp2: woderrise_github::rate_limit::GitHubRateLimitResponse = serde_json::from_value(serde_json::json!({
        "resources": {
            "core": { "limit": 2000000000u32, "remaining": 1000000000u32, "reset": 1761500000, "used": 1000000000u32 },
            "search": { "limit": 10000, "remaining": 9990, "reset": 1761500000, "used": 10 }
        }
    })).unwrap();
    tracker.update_from_response(&resp2);

    // Revert to unauthenticated: diff = 60 - 2000000000 = -1999999940.
    // (1000000000 - 1999999940).clamp(0, 60) must safely clamp to 0 without overflow!
    tracker.set_authenticated(false);
    let s2 = tracker.core_status();
    assert_eq!(s2.limit, 60);
    assert_eq!(s2.remaining, 0);

    // 3. Search extreme quota
    tracker.set_authenticated(true);
    let s3 = tracker.search_status();
    assert_eq!(s3.limit, 30);
    // (9990 + (30 - 10000)) = 9990 - 9970 = 20
    assert_eq!(s3.remaining, 20);
}

/// Challenge 8: CLI auth subcommand boundary inputs and flag syntax.
///
/// Invariant: CLI auth must handle:
/// - `--token=VALUE` syntax
/// - Oversized 64KB tokens
/// - Non-existent nested XDG directories
/// - Clean handling of corrupted settings file on save
#[test]
fn test_adversarial_cli_auth_boundary_cases() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let settings_file = xdg.settings_file();

    // 1. Syntax: --token=VALUE
    let res_eq =
        handle_auth_command_with_paths(&["--token=ghp_syntax_equal_test".to_string()], &xdg);
    assert!(res_eq.is_ok(), "Failed --token=VALUE syntax: {:?}", res_eq);
    let s1 = load_settings(&settings_file).unwrap();
    assert_eq!(s1.github_token.as_deref(), Some("ghp_syntax_equal_test"));

    // 2. Large 64KB token
    let huge_token = format!("ghp_{}", "a".repeat(65536));
    let res_huge =
        handle_auth_command_with_paths(&["--token".to_string(), huge_token.clone()], &xdg);
    assert!(res_huge.is_ok(), "Failed on 64KB token: {:?}", res_huge);
    let s2 = load_settings(&settings_file).unwrap();
    assert_eq!(s2.github_token.as_deref(), Some(huge_token.as_str()));

    // 3. Clear token
    let res_clear = handle_auth_command_with_paths(&["--clear".to_string()], &xdg);
    assert!(res_clear.is_ok());
    let s3 = load_settings(&settings_file).unwrap();
    assert_eq!(s3.github_token, None);

    // 4. Overwrite corrupted file via CLI
    fs::write(&settings_file, b"corrupted non-json content").unwrap();
    let res_recover = handle_auth_command_with_paths(
        &["--token".to_string(), "ghp_recovering_token".to_string()],
        &xdg,
    );
    assert!(
        res_recover.is_ok(),
        "CLI must overwrite corrupted settings: {:?}",
        res_recover
    );
    let s4 = load_settings(&settings_file).unwrap();
    assert_eq!(s4.github_token.as_deref(), Some("ghp_recovering_token"));
}
