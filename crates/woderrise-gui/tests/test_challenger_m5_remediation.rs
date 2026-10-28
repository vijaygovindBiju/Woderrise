//! Authoritative empirical challenger stress tests for Milestone M5 Remediation:
//! 1. Unicode code points (CJK, Cyrillic, emojis, accents, RTL, symbols) and boundary lengths in `mask_token`
//! 2. POSIX 0600 file permissions across full CLI auth lifecycle (--token, --status, --clear, overwrite, remediation)
//! 3. Dynamic rate limit elevation (Core 60->5,000, Search 10->30) and downgrade on clear
//! 4. Non-panicking invariant testing over randomized generators

use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;
use woderrise_core::models::load_settings;
use woderrise_core::paths::XdgPaths;
use woderrise_github::rate_limit::GitHubRateLimitResponse;
use woderrise_github::RateLimitTracker;
use woderrise_gui::cli::handle_auth_command_with_paths;
use woderrise_gui::screens::settings::mask_token;

/// Objective 1: Empirical stress testing on `mask_token` across Unicode code points and extreme inputs.
#[test]
fn test_stress_mask_token_unicode_code_points() {
    // 1. CJK characters (3-byte UTF-8 each)
    assert_eq!(mask_token("你好世界"), "••••••••"); // 4 chars <= 8
    assert_eq!(mask_token("你好世界1234"), "••••••••"); // 8 chars <= 8
    assert_eq!(mask_token("你好世界12345"), "你好世界••••••••2345"); // 9 chars > 8

    // 2. Cyrillic characters (2-byte UTF-8 each)
    assert_eq!(mask_token("Привет"), "••••••••"); // 6 chars <= 8
    assert_eq!(mask_token("ПриветМи"), "••••••••"); // 8 chars <= 8
    assert_eq!(mask_token("ПриветМир"), "Прив••••••••тМир"); // 9 chars > 8
    assert_eq!(mask_token("ПриветМир123"), "Прив••••••••р123"); // 12 chars > 8

    // 3. Emojis (4-byte UTF-8 each)
    assert_eq!(mask_token("🦀🦀🦀"), "••••••••"); // 3 chars (12 bytes)
    assert_eq!(mask_token("🔥🚀🌟💻🎉"), "••••••••"); // 5 chars (20 bytes)
    assert_eq!(mask_token("🦀🔥🚀🌟💻🎉⚡🎯"), "••••••••"); // 8 chars (32 bytes)
    assert_eq!(mask_token("🦀🔥🚀🌟💻🎉⚡🎯✨"), "🦀🔥🚀🌟••••••••🎉⚡🎯✨"); // 9 chars (36 bytes)

    // 4. Accented characters
    assert_eq!(mask_token("éàîôü"), "••••••••"); // 5 chars <= 8
    assert_eq!(mask_token("éàîôü123"), "••••••••"); // 8 chars <= 8
    assert_eq!(mask_token("éàîôü1234"), "éàîô••••••••1234"); // 9 chars > 8

    // 5. Mixed scripts (Hebrew, Arabic, Devanagari, Japanese, Accents, ASCII)
    let mixed = "שלום_مرحبا_नमस्ते_日本語_éà_12345";
    let masked_mixed = mask_token(mixed);
    let mixed_chars: Vec<char> = mixed.chars().collect();
    let expected_prefix: String = mixed_chars[..4].iter().collect();
    let expected_suffix: String = mixed_chars[mixed_chars.len() - 4..].iter().collect();
    assert_eq!(
        masked_mixed,
        format!("{}••••••••{}", expected_prefix, expected_suffix)
    );
}

/// Objective 1: Empirical stress testing on `mask_token` across extreme lengths and whitespace.
#[test]
fn test_stress_mask_token_lengths_and_whitespace() {
    // Empty and pure whitespace
    assert_eq!(mask_token(""), "");
    assert_eq!(mask_token(" "), "");
    assert_eq!(mask_token("   \t\r\n\t  "), "");

    // 1 character
    assert_eq!(mask_token("x"), "••••••••");
    assert_eq!(mask_token("🦀"), "••••••••");
    assert_eq!(mask_token("中"), "••••••••");

    // Boundary lengths: 7, 8, 9 characters
    // 7 chars
    assert_eq!(mask_token("1234567"), "••••••••");
    assert_eq!(mask_token("🦀🦀🦀🦀🦀🦀🦀"), "••••••••");
    // 8 chars
    assert_eq!(mask_token("12345678"), "••••••••");
    assert_eq!(mask_token("🦀🦀🦀🦀🦀🦀🦀🦀"), "••••••••");
    // 9 chars
    assert_eq!(mask_token("123456789"), "1234••••••••6789");
    assert_eq!(mask_token("🦀🦀🦀🦀🦀🦀🦀🦀🦀"), "🦀🦀🦀🦀••••••••🦀🦀🦀🦀");

    // 1,000 characters ASCII
    let ascii_1000 = "a".repeat(1000);
    let masked_ascii = mask_token(&ascii_1000);
    assert_eq!(masked_ascii, "aaaa••••••••aaaa");
    assert_eq!(masked_ascii.chars().count(), 16);

    // 1,000 characters multi-byte Unicode (4,000 bytes)
    let crab_1000 = "🦀".repeat(1000);
    let masked_crab = mask_token(&crab_1000);
    assert_eq!(masked_crab, "🦀🦀🦀🦀••••••••🦀🦀🦀🦀");
    assert_eq!(masked_crab.chars().count(), 16);
    assert_eq!(masked_crab.len(), 4 * 4 + 8 * 3 + 4 * 4); // 40 bytes
}

/// Objective 1: Fuzz generator invariant check across 5,000 random Unicode sequences.
#[test]
fn test_stress_mask_token_randomized_invariants() {
    let sample_pool: Vec<char> = vec![
        'a', 'Z', '0', '_', '-', '.', ' ', '\t', '\n', '你', '好', '世', '界', '日', '本', '語',
        'П', 'р', 'и', 'в', 'е', 'т', '🦀', '🔥', '🚀', '🌟', '💻', '🎉', 'é', 'à', 'ô', 'ü', 'ñ',
        'ç', '\u{200B}', // zero-width space
        '\u{FEFF}', // byte order mark
    ];

    let mut seed: u64 = 0xDEADBEEF;
    let mut pseudo_rand = || -> usize {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 32) as usize
    };

    for _ in 0..5_000 {
        let len = pseudo_rand() % 40;
        let s: String = (0..len)
            .map(|_| sample_pool[pseudo_rand() % sample_pool.len()])
            .collect();

        // Must NEVER panic
        let res = std::panic::catch_unwind(|| mask_token(&s));
        assert!(res.is_ok(), "Panic encountered for input: {:?}", s);
        let masked = res.unwrap();

        let trimmed = s.trim();
        let char_count = trimmed.chars().count();

        if trimmed.is_empty() {
            assert_eq!(masked, "");
        } else if char_count <= 8 {
            assert_eq!(masked, "••••••••");
        } else {
            assert_eq!(masked.chars().count(), 16);
            let trimmed_chars: Vec<char> = trimmed.chars().collect();
            let prefix: String = trimmed_chars[..4].iter().collect();
            let suffix: String = trimmed_chars[char_count - 4..].iter().collect();
            assert!(masked.starts_with(&prefix));
            assert!(masked.ends_with(&suffix));
            assert_eq!(masked, format!("{}••••••••{}", prefix, suffix));
        }
    }
}

/// Objective 2: Stress test POSIX `0600` permissions on settings files across CLI auth lifecycle.
#[test]
fn test_stress_posix_0600_permissions_cli_lifecycle() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let settings_file = xdg.settings_file();

    // Helper closure to verify 0600 permissions
    let assert_mode_0600 = || {
        let meta = fs::metadata(&settings_file).expect("Failed to stat settings file");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "Security violation: settings file permissions must be 0600, got {:o}",
            mode
        );
    };

    // 1. Initial status query (no file yet)
    let res = handle_auth_command_with_paths(&["--status".into()], &xdg);
    assert!(res.is_ok());

    // 2. Save ASCII token
    let res = handle_auth_command_with_paths(
        &["--token".into(), "ghp_ascii_test_token_12345678".into()],
        &xdg,
    );
    assert!(res.is_ok());
    assert!(settings_file.exists());
    assert_mode_0600();

    let settings = load_settings(&settings_file).expect("Failed to load settings");
    assert_eq!(
        settings.github_token.as_deref(),
        Some("ghp_ascii_test_token_12345678")
    );

    // 3. Inspect status with token configured
    let res = handle_auth_command_with_paths(&["--status".into()], &xdg);
    assert!(res.is_ok());
    assert_mode_0600();

    // 4. Overwrite with Unicode token (CJK + Emojis)
    let unicode_token = "ghp_你好世界_🦀🚀_123456789";
    let res = handle_auth_command_with_paths(&["--token".into(), unicode_token.into()], &xdg);
    assert!(res.is_ok());
    assert_mode_0600();

    let settings = load_settings(&settings_file).expect("Failed to load settings");
    assert_eq!(settings.github_token.as_deref(), Some(unicode_token));

    // 5. Status again
    let res = handle_auth_command_with_paths(&["--status".into()], &xdg);
    assert!(res.is_ok());
    assert_mode_0600();

    // 6. Clear token
    let res = handle_auth_command_with_paths(&["--clear".into()], &xdg);
    assert!(res.is_ok());
    assert_mode_0600();

    let settings = load_settings(&settings_file).expect("Failed to load settings");
    assert_eq!(settings.github_token, None);

    // 7. Remediate externally compromised permissions:
    // If an external process changes permissions to 0666 or 0644,
    // subsequent `save_settings_durably` MUST restore 0600!
    fs::set_permissions(&settings_file, fs::Permissions::from_mode(0o666)).expect("chmod 0666");
    let compromised_mode = fs::metadata(&settings_file).unwrap().permissions().mode() & 0o777;
    assert_eq!(compromised_mode, 0o666);

    let res = handle_auth_command_with_paths(
        &["--token".into(), "ghp_restored_security_0000".into()],
        &xdg,
    );
    assert!(res.is_ok());
    assert_mode_0600(); // Restored to 0600!
}

/// Objective 3: Stress test dynamic rate limit elevation (Core 60->5,000, Search 10->30) and downgrade on clear.
#[test]
fn test_stress_dynamic_rate_limit_elevation_and_downgrade() {
    let tracker = RateLimitTracker::new();

    // 1. Initial unauthenticated defaults
    let initial_core = tracker.core_status();
    let initial_search = tracker.search_status();
    assert_eq!(initial_core.limit, 60);
    assert_eq!(initial_core.remaining, 60);
    assert_eq!(initial_search.limit, 10);
    assert_eq!(initial_search.remaining, 10);

    // Simulate GitHub GET /rate_limit response consuming unauthenticated quota
    let resp_unauth: GitHubRateLimitResponse = serde_json::from_value(serde_json::json!({
        "resources": {
            "core": { "limit": 60, "remaining": 58, "reset": 1761500000, "used": 2 },
            "search": { "limit": 10, "remaining": 9, "reset": 1761500000, "used": 1 }
        }
    }))
    .unwrap();
    tracker.update_from_response(&resp_unauth);

    let c1 = tracker.core_status();
    let s1 = tracker.search_status();
    assert_eq!(c1.remaining, 58);
    assert_eq!(s1.remaining, 9);

    // 2. Elevate rate limits (authenticating with PAT)
    tracker.set_authenticated(true);
    let c_elevated = tracker.core_status();
    let s_elevated = tracker.search_status();

    assert_eq!(c_elevated.limit, 5000, "Core limit must elevate to 5,000");
    assert_eq!(s_elevated.limit, 30, "Search limit must elevate to 30");
    // Elevation math: 58 + (5000 - 60) = 58 + 4940 = 4998
    assert_eq!(c_elevated.remaining, 4998);
    // Search elevation: 9 + (30 - 10) = 9 + 20 = 29
    assert_eq!(s_elevated.remaining, 29);

    // Simulate authenticated usage response
    let resp_auth: GitHubRateLimitResponse = serde_json::from_value(serde_json::json!({
        "resources": {
            "core": { "limit": 5000, "remaining": 4997, "reset": 1761500000, "used": 3 },
            "search": { "limit": 30, "remaining": 28, "reset": 1761500000, "used": 2 }
        }
    }))
    .unwrap();
    tracker.update_from_response(&resp_auth);

    assert_eq!(tracker.core_status().remaining, 4997);
    assert_eq!(tracker.search_status().remaining, 28);

    // 3. Downgrade rate limits (clearing PAT)
    tracker.set_authenticated(false);
    let c_downgraded = tracker.core_status();
    let s_downgraded = tracker.search_status();

    assert_eq!(c_downgraded.limit, 60, "Core limit must return to 60");
    assert_eq!(s_downgraded.limit, 10, "Search limit must return to 10");
    // Downgrade math: (4997 + (60 - 5000)).clamp(0, 60) = (4997 - 4940).clamp(0, 60) = 57
    assert_eq!(c_downgraded.remaining, 57);
    // Search downgrade: (28 + (10 - 30)).clamp(0, 10) = (28 - 20).clamp(0, 10) = 8
    assert_eq!(s_downgraded.remaining, 8);

    // 4. Repeated rapid toggling stability
    for i in 0..100 {
        tracker.set_authenticated(i % 2 == 0);
        let c = tracker.core_status();
        let s = tracker.search_status();
        assert!(c.remaining <= c.limit);
        assert!(s.remaining <= s.limit);
    }
}
