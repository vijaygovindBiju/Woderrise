//! Empirical Adversarial Challenge Test Suite for Milestone M5:
//! Screen 4 — Settings, LiveConnectionState, Countdown Timer Arithmetic, and Byte Formatting.
//!
//! Vectors tested:
//! 1. `format_reset_countdown` chronological spectrum (past, present, sub-second, far future, leap seconds, fuzz)
//! 2. `CacheStats::format_bytes` boundaries, powers of 2, u64::MAX, and precision rounding
//! 3. `SettingsState` state transitions: rapid tab switching with dirty uncommitted tokens, Unicode/CJK/emoji masking
//! 4. `LiveConnectionState` & `RateLimitGaugeState` synchronization and simulated clock ticks
//! 5. `safe_cleanup_cache` security invariants under directory symlinks and strict containment of non-cache XDG paths
//! 6. Headless `view_settings` rendering stress across all tabs, themes, and degenerate quota states

use chrono::{DateTime, TimeDelta, Utc};
use iced::Theme;
use std::fs;
use std::os::unix::fs::symlink;
use tempfile::tempdir;

use woderrise_core::paths::XdgPaths;
use woderrise_gui::app::{format_reset_countdown, App, EngineEvent, GuiCommand, Message, Screen};
use woderrise_gui::screens::settings::{
    calculate_directory_stats, format_bytes, mask_token, safe_cleanup_cache, view_settings,
    AboutDiagnostics, CacheStats, SettingsState, SettingsTab,
};
use woderrise_gui::theme::{contrast_ratio, ThemeTokens};
use woderrise_gui::widgets::gauge::{RateLimitColorTier, RateLimitGaugeState};

// =========================================================================
// Vector 1: `format_reset_countdown` Chronological Spectrum & Stress
// =========================================================================

#[test]
fn test_adversarial_format_reset_countdown_chronological_spectrum() {
    let now = Utc::now();

    // 1. Past Timestamps: MUST return "resets now" without underflow or negative numbers
    let past_cases = [
        now - TimeDelta::try_days(365 * 100).unwrap(), // 100 years ago
        now - TimeDelta::try_days(30).unwrap(),        // 30 days ago
        now - TimeDelta::try_days(1).unwrap(),         // yesterday
        now - TimeDelta::try_hours(5).unwrap(),        // 5 hours ago
        now - TimeDelta::try_minutes(10).unwrap(),     // 10 minutes ago
        now - TimeDelta::try_minutes(1).unwrap(),      // 1 minute ago
        now - TimeDelta::try_seconds(5).unwrap(),      // 5 seconds ago
        now - TimeDelta::try_seconds(1).unwrap(),      // 1 second ago
        now - TimeDelta::try_milliseconds(100).unwrap(), // 100ms ago
        now - TimeDelta::try_milliseconds(1).unwrap(), // 1ms ago
        now,                                           // exact current instant
    ];

    for past in past_cases {
        let result = format_reset_countdown(past);
        assert_eq!(
            result, "resets now",
            "Past or current timestamp {:?} must return 'resets now', got '{}'",
            past, result
        );
    }

    // 2. Sub-Second Future Timestamps: MUST return "resets in 1s" (never 0s or negative)
    let subsecond_cases = [
        now + TimeDelta::try_milliseconds(1).unwrap(),
        now + TimeDelta::try_milliseconds(250).unwrap(),
        now + TimeDelta::try_milliseconds(500).unwrap(),
        now + TimeDelta::try_milliseconds(999).unwrap(),
    ];

    for sub in subsecond_cases {
        let result = format_reset_countdown(sub);
        assert_eq!(
            result, "resets in 1s",
            "Sub-second future delta {:?} must clamp to 'resets in 1s', got '{}'",
            sub, result
        );
    }

    // 3. Seconds Range (1s to 59s)
    for s in 1..=59 {
        let now_tick = Utc::now();
        let target = now_tick + TimeDelta::try_milliseconds(s * 1000 + 50).unwrap();
        let result = format_reset_countdown(target);
        assert_eq!(
            result,
            format!("resets in {}s", s),
            "Expected 'resets in {}s', got '{}'",
            s,
            result
        );
    }

    // 4. Minutes Range (60s to 3599s)
    let min_cases = [
        (60, "resets in 1 min"),
        (61, "resets in 1 min"),
        (119, "resets in 1 min"),
        (120, "resets in 2 min"),
        (300, "resets in 5 min"),
        (1800, "resets in 30 min"),
        (3540, "resets in 59 min"),
        (3599, "resets in 59 min"),
    ];

    for (secs, expected) in min_cases {
        let now_tick = Utc::now();
        let target = now_tick + TimeDelta::try_milliseconds(secs * 1000 + 500).unwrap();
        let result = format_reset_countdown(target);
        assert_eq!(
            result, expected,
            "For delta {}s, expected '{}', got '{}'",
            secs, expected, result
        );
    }

    // 5. Hours Range (>= 60 minutes)
    let hour_cases = [
        (3600, "resets in 1h 0m"),
        (3660, "resets in 1h 1m"),
        (5400, "resets in 1h 30m"),
        (7200, "resets in 2h 0m"),
        (86400, "resets in 24h 0m"),
        (90000, "resets in 25h 0m"),
        (360000, "resets in 100h 0m"),
    ];

    for (secs, expected) in hour_cases {
        let now_tick = Utc::now();
        let target = now_tick + TimeDelta::try_milliseconds(secs * 1000 + 500).unwrap();
        let result = format_reset_countdown(target);
        assert_eq!(
            result, expected,
            "For delta {}s, expected '{}', got '{}'",
            secs, expected, result
        );
    }

    // 6. Extreme Far Future (Year 2038+, 2050, 3000, and MAX_UTC)
    let far_future_cases = [
        DateTime::parse_from_rfc3339("2038-01-19T03:14:08Z")
            .unwrap()
            .with_timezone(&Utc),
        DateTime::parse_from_rfc3339("2050-12-31T23:59:59Z")
            .unwrap()
            .with_timezone(&Utc),
        DateTime::parse_from_rfc3339("3000-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        DateTime::<Utc>::MAX_UTC,
    ];

    for future in far_future_cases {
        let res = std::panic::catch_unwind(|| format_reset_countdown(future));
        assert!(
            res.is_ok(),
            "format_reset_countdown panicked on far future date {:?}",
            future
        );
        let s = res.unwrap();
        assert!(
            s.starts_with("resets in ") && s.contains('h'),
            "Output must format as hours: {}",
            s
        );
    }

    // 7. High Frequency Stress Loop (10,000 iterations over varied second deltas)
    for delta_secs in -5000..5000 {
        let t = now + TimeDelta::try_seconds(delta_secs).unwrap();
        let formatted = format_reset_countdown(t);
        assert!(
            formatted.starts_with("resets"),
            "Formatted string must start with 'resets', got '{}'",
            formatted
        );
        assert!(
            !formatted.contains('-'),
            "Formatted string must never contain negative numbers, got '{}' for delta {}",
            formatted,
            delta_secs
        );
    }
}

// =========================================================================
// Vector 2: `CacheStats::format_bytes` Boundaries, Powers of 2, u64::MAX
// =========================================================================

#[test]
fn test_adversarial_format_bytes_boundaries_and_precision() {
    // 1. Exact boundaries
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(1), "1 B");
    assert_eq!(format_bytes(1023), "1023 B");

    // 1024 bytes = 1.0 KB
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1025), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");

    // 1 MB boundaries (1048576)
    assert_eq!(format_bytes(1048575), "1024.0 KB"); // Rounding boundary
    assert_eq!(format_bytes(1048576), "1.0 MB");
    assert_eq!(format_bytes(1048577), "1.0 MB");
    assert_eq!(format_bytes(1048576 * 2), "2.0 MB");
    assert_eq!(format_bytes(1048576 * 15 + 204800), "15.2 MB");

    // 1 GB boundaries (1073741824)
    assert_eq!(format_bytes(1073741823), "1024.0 MB");
    assert_eq!(format_bytes(1073741824), "1.00 GB");
    assert_eq!(format_bytes(1073741824 * 5), "5.00 GB");
    assert_eq!(format_bytes(1073741824 * 1024), "1024.00 GB"); // 1 TB

    // Extreme u64 boundaries
    let u64_max_formatted = format_bytes(u64::MAX);
    assert!(
        u64_max_formatted.ends_with(" GB"),
        "u64::MAX must format with GB suffix, got '{}'",
        u64_max_formatted
    );
    assert!(!u64_max_formatted.contains("NaN"), "Must not contain NaN");
    assert!(!u64_max_formatted.contains("inf"), "Must not contain inf");

    // 2. CacheStats struct wrapper integration
    let stats_empty = CacheStats::default();
    assert_eq!(stats_empty.formatted_size(), "0 B");

    let stats_gigabyte = CacheStats {
        file_count: 500,
        total_bytes: 3 * 1073741824 + 536870912, // 3.5 GB
    };
    assert_eq!(stats_gigabyte.formatted_size(), "3.50 GB");

    // 3. Powers of 2 Sweep (2^0 to 2^63)
    for power in 0..64 {
        let val = 1u64.checked_shl(power).unwrap_or(u64::MAX);
        let res = format_bytes(val);
        assert!(!res.is_empty());
        assert!(
            res.ends_with(" B")
                || res.ends_with(" KB")
                || res.ends_with(" MB")
                || res.ends_with(" GB"),
            "Unexpected unit in '{}' for 2^{}",
            res,
            power
        );
    }
}

// =========================================================================
// Vector 3: `SettingsState` State Transitions & Dirty Token Preservation
// =========================================================================

#[test]
fn test_adversarial_settings_state_rapid_tab_switching_preserves_dirty_input() {
    let mut state = SettingsState::new(None);
    assert_eq!(state.active_tab, SettingsTab::GitHub);
    assert!(state.token_input.is_empty());
    assert_eq!(state.saved_token, None);

    // User types an uncommitted token into the input field
    let dirty_token = "ghp_super_secret_dirty_token_in_progress_987654";
    state.set_token_input(dirty_token.to_string());
    state.set_notice("Draft token entered but not yet saved", false);
    assert_eq!(state.token_input, dirty_token);
    assert!(state.status_notice.is_some());

    // Rapidly switch tabs 1,000 times
    let tabs = [
        SettingsTab::Appearance,
        SettingsTab::NetworkCache,
        SettingsTab::About,
        SettingsTab::GitHub,
    ];

    for i in 0..1000 {
        let tab = tabs[i % tabs.len()];
        state.set_tab(tab);
        assert_eq!(state.active_tab, tab);

        // INVARIANT: Switching tabs MUST NEVER clear or alter uncommitted draft token!
        assert_eq!(
            state.token_input, dirty_token,
            "Dirty token input was corrupted or cleared on tab switch to {:?}",
            tab
        );

        // INVARIANT: Switching tabs clears stale notice banner
        assert_eq!(
            state.status_notice, None,
            "Status notice must be cleared on tab switch to {:?}",
            tab
        );
    }

    // Saved token must still be None
    assert_eq!(state.saved_token, None);
    assert!(!state.is_authenticated());
    assert_eq!(state.masked_display(), "None (Unauthenticated)");
}

#[test]
fn test_adversarial_settings_mask_toggling_unicode_and_emojis() {
    let mut state = SettingsState::new(None);
    assert!(state.is_masked); // Default masked

    // Toggle 100 times
    for _ in 0..100 {
        state.toggle_mask();
        assert!(!state.is_masked);
        state.toggle_mask();
        assert!(state.is_masked);
    }

    // Test masked_display under Unicode, RTL, CJK, Emojis
    let complex_tokens = [
        ("ghp_🦀🔥🚀🌟💻🎉⚡🎯", "ghp_••••••••💻🎉⚡🎯"),
        ("日本語_トークン_パスワード_12345", "日本語_••••••••2345"),
        ("ПриветМир_СекретныйТокен_67890", "Прив••••••••7890"),
        ("éàîôü_accent_pass_7890", "éàîô••••••••7890"),
        ("short", "••••••••"),
        ("12345678", "••••••••"),
        ("123456789", "1234••••••••6789"),
    ];

    for (raw, expected_mask) in complex_tokens {
        state.set_saved(Some(raw.to_string()));
        assert!(state.is_authenticated());
        assert_eq!(
            state.masked_display(),
            expected_mask,
            "Masking mismatch for token '{}'",
            raw
        );
    }

    // Clearing token reverts to unauthenticated state
    state.set_saved(None);
    assert!(!state.is_authenticated());
    assert_eq!(state.masked_display(), "None (Unauthenticated)");
    assert!(state.token_input.is_empty());
}

// =========================================================================
// Vector 4: `LiveConnectionState`, `RateLimitGaugeState` & Simulated Clock Ticks
// =========================================================================

#[test]
fn test_adversarial_live_connection_state_sync_and_clock_ticks() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(50);
    let mut app = App::new(cmd_tx, None);

    // Initial state invariants
    assert_eq!(app.active_screen, Screen::Search);
    assert_eq!(app.connection_state.core_remaining, 60);
    assert_eq!(app.connection_state.search_remaining, 10);
    assert!(!app.connection_state.is_authenticated);

    // Drain startup commands sent by App::new
    let mut initial_cmds = Vec::new();
    while let Ok(cmd) = cmd_rx.try_recv() {
        initial_cmds.push(cmd);
    }

    // Navigate to Settings
    let _ = app.update(Message::NavigateTo(Screen::Settings));
    assert_eq!(app.active_screen, Screen::Settings);

    // Drained FetchRateLimits command sent on Settings navigation
    let cmd = cmd_rx
        .try_recv()
        .expect("Expected command on Settings navigation");
    assert!(
        matches!(cmd, GuiCommand::FetchRateLimits),
        "Expected GuiCommand::FetchRateLimits, got {:?}",
        cmd
    );

    // Simulate 1,000 rapid ClockTicks
    for _ in 0..1000 {
        let _ = app.update(Message::ClockTick);
    }

    // Live update from Engine with elevated authenticated quotas
    let reset_core = Utc::now() + TimeDelta::try_hours(1).unwrap();
    let reset_search = Utc::now() + TimeDelta::try_minutes(1).unwrap();

    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Core,
            limit: 5000,
            remaining: 4850,
            reset_time: reset_core,
            used: 150,
        },
        search: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Search,
            limit: 30,
            remaining: 28,
            reset_time: reset_search,
            used: 2,
        },
    }));

    // Verify app.connection_state is synchronized
    assert_eq!(app.connection_state.core_limit, 5000);
    assert_eq!(app.connection_state.core_remaining, 4850);
    assert_eq!(app.connection_state.core_reset, reset_core);
    assert_eq!(app.connection_state.search_limit, 30);
    assert_eq!(app.connection_state.search_remaining, 28);
    assert_eq!(app.connection_state.search_reset, reset_search);
    assert_eq!(app.connection_state.last_error, None);

    // Verify settings_state reset times match
    assert_eq!(app.settings_state.core_reset, Some(reset_core));
    assert_eq!(app.settings_state.search_reset, Some(reset_search));

    // Verify rate_limits gauge reflects updated percentages & tiers
    assert_eq!(app.connection_state.rate_limits.core_remaining, 4850);
    assert_eq!(app.connection_state.rate_limits.core_limit, 5000);
    assert_eq!(
        app.connection_state.rate_limits.core_tier(),
        RateLimitColorTier::Green
    );
    assert_eq!(
        app.connection_state.rate_limits.search_tier(),
        RateLimitColorTier::Green
    );

    // Simulate connection failure event
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsFetchFailed {
        error: "HTTP 401 Bad credentials".into(),
    }));
    assert_eq!(
        app.connection_state.last_error,
        Some("HTTP 401 Bad credentials".into())
    );
}

// =========================================================================
// Vector 5: `safe_cleanup_cache` Directory Containment & Symlink Invariants
// =========================================================================

#[test]
fn test_adversarial_safe_cache_cleanup_symlink_attack_and_directory_containment() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);

    // 1. Establish persistent non-cache directories
    let config_dir = xdg.woderrise_config_dir();
    let data_dir = xdg.woderrise_data_dir();
    let state_dir = xdg.woderrise_state_dir();
    let cache_dir = xdg.woderrise_cache_dir();

    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&data_dir).unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    // 2. Populate critical files
    let settings_file = xdg.settings_file();
    fs::write(
        &settings_file,
        b"{\"version\":1,\"token\":\"ghp_critical\"}",
    )
    .unwrap();

    let journal_file = xdg.journal_file();
    fs::write(&journal_file, b"{\"transactions\":[\"tx_001\"]}").unwrap();

    let catalog_file = xdg.catalog_file();
    fs::write(&catalog_file, b"{\"curated\":true}").unwrap();

    let backup_dir = xdg.backup_dir("tx_critical_snapshot").unwrap();
    fs::create_dir_all(&backup_dir).unwrap();
    let backup_file = backup_dir.join("waybar_config.json");
    fs::write(&backup_file, b"{\"layer\":\"top\"}").unwrap();

    // 3. Populate cache directory with staging transactions and loose blobs
    let staging_tx = xdg.staging_dir("tx_ephemeral_to_clean").unwrap();
    fs::create_dir_all(&staging_tx).unwrap();
    fs::write(staging_tx.join("staged_1.txt"), b"ephemeral payload 1").unwrap();
    fs::write(staging_tx.join("staged_2.txt"), b"ephemeral payload 2").unwrap();

    let loose_blob = cache_dir.join("blob_feed.json");
    fs::write(&loose_blob, b"blob cached data").unwrap();

    // 4. Create an adversarial symlink inside staging pointing to config_dir
    let symlink_path = staging_tx.join("evil_symlink_to_config");
    let _ = symlink(config_dir.as_std_path(), symlink_path.as_std_path());

    // Also an adversarial symlink inside cache root pointing to journal_file
    let loose_symlink = cache_dir.join("evil_symlink_to_journal");
    let _ = symlink(journal_file.as_std_path(), loose_symlink.as_std_path());

    // 5. Run safe_cleanup_cache
    let cleanup_result = safe_cleanup_cache(&cache_dir);
    assert!(
        cleanup_result.is_ok(),
        "safe_cleanup_cache must succeed: {:?}",
        cleanup_result
    );

    // 6. Verify ephemeral cache files are purged
    assert!(
        !staging_tx.exists(),
        "Staging transaction directory must be removed"
    );
    assert!(!loose_blob.exists(), "Loose cache blob must be removed");

    // 7. CRITICAL INVARIANT: Persistent non-cache files MUST NEVER BE DELETED OR CORRUPTED!
    assert!(
        settings_file.exists(),
        "CRITICAL SECURITY VIOLATION: settings.json was deleted by cache cleanup!"
    );
    assert_eq!(
        fs::read_to_string(&settings_file).unwrap(),
        "{\"version\":1,\"token\":\"ghp_critical\"}"
    );

    assert!(
        journal_file.exists(),
        "CRITICAL INTEGRITY VIOLATION: journal.json was deleted by cache cleanup!"
    );
    assert!(
        catalog_file.exists(),
        "CRITICAL INTEGRITY VIOLATION: catalog.json was deleted by cache cleanup!"
    );
    assert!(
        backup_file.exists(),
        "CRITICAL DATA LOSS: backup snapshot was deleted by cache cleanup!"
    );
}

// =========================================================================
// Vector 6: Headless `view_settings` Rendering Stress Matrix
// =========================================================================

#[test]
fn test_adversarial_view_settings_rendering_stress_matrix() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);

    let degenerate_gauges = [
        // 1. Zero limits (limit == 0 edge case)
        RateLimitGaugeState {
            core_remaining: 0,
            core_limit: 0,
            search_remaining: 0,
            search_limit: 0,
        },
        // 2. Remaining > Limit
        RateLimitGaugeState {
            core_remaining: 9999,
            core_limit: 5000,
            search_remaining: 50,
            search_limit: 30,
        },
        // 3. Exhausted quotas (remaining == 0)
        RateLimitGaugeState {
            core_remaining: 0,
            core_limit: 5000,
            search_remaining: 0,
            search_limit: 30,
        },
        // 4. Maximum u32 boundaries
        RateLimitGaugeState {
            core_remaining: u32::MAX,
            core_limit: u32::MAX,
            search_remaining: u32::MAX,
            search_limit: u32::MAX,
        },
    ];

    let tabs = [
        SettingsTab::GitHub,
        SettingsTab::Appearance,
        SettingsTab::NetworkCache,
        SettingsTab::About,
    ];

    let themes = [Theme::Dark, Theme::Light];

    for theme in &themes {
        for tab in &tabs {
            for gauge in &degenerate_gauges {
                {
                    let mut state = SettingsState::with_xdg(
                        Some("ghp_valid_test_token_1234".into()),
                        Some(&xdg),
                    );
                    state.active_tab = *tab;
                    state.set_notice("Testing error banner rendering", true);
                    let _element = view_settings(&state, gauge, theme, |_| ());
                }

                {
                    let mut state = SettingsState::with_xdg(
                        Some("ghp_valid_test_token_1234".into()),
                        Some(&xdg),
                    );
                    state.active_tab = *tab;
                    state.set_notice("Testing success banner rendering", false);
                    let _element = view_settings(&state, gauge, theme, |_| ());
                }

                {
                    let mut state = SettingsState::with_xdg(
                        Some("ghp_valid_test_token_1234".into()),
                        Some(&xdg),
                    );
                    state.active_tab = *tab;
                    state.clear_notice();
                    let _element = view_settings(&state, gauge, theme, |_| ());
                }
            }
        }
    }
}

// =========================================================================
// Vector 7: `AboutDiagnostics` and UI Appearance Contrast
// =========================================================================

#[test]
fn test_adversarial_about_diagnostics_and_contrast() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);

    let diag = AboutDiagnostics::collect(&xdg);
    assert!(!diag.app_version.is_empty());
    assert!(!diag.os_name.is_empty());
    assert!(!diag.arch.is_empty());
    assert!(!diag.display_server.is_empty());

    // Contrast ratio testing on dark and light tokens
    let dark_tokens = ThemeTokens::dark();
    let light_tokens = ThemeTokens::light();

    assert!(contrast_ratio(dark_tokens.text_primary, dark_tokens.bg_base) >= 7.0);
    assert!(contrast_ratio(light_tokens.text_primary, light_tokens.bg_base) >= 7.0);
    assert!(contrast_ratio(dark_tokens.accent_primary, dark_tokens.bg_base) >= 3.0);
    assert!(contrast_ratio(light_tokens.accent_primary, light_tokens.bg_base) >= 3.0);
}

// =========================================================================
// Vector 8: Deep Directories, Cyclic Symlinks & Huge Files in Cache Stats
// =========================================================================

#[test]
fn test_adversarial_deep_directory_and_cyclic_symlinks_cache_stats() {
    let tmp = tempdir().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let cache_dir = base.join("cache");
    fs::create_dir_all(&cache_dir).unwrap();

    // 1. Create a 15-level deeply nested directory structure with files
    let mut current = cache_dir.clone();
    for depth in 0..15 {
        current = current.join(format!("level_{}", depth));
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("payload.txt"), vec![0xAA; 1024]).unwrap(); // 1 KB per file
    }

    // 2. Create cyclic directory symlink: current/loop -> cache_dir
    let cyclic_symlink = current.join("evil_loop");
    let _ = symlink(cache_dir.as_std_path(), cyclic_symlink.as_std_path());

    // 3. Create file symlink pointing to non-existent target (broken symlink)
    let broken_symlink = cache_dir.join("broken.symlink");
    let _ = symlink("/nonexistent/file/path", broken_symlink.as_std_path());

    // 4. Calculate directory stats: MUST NOT hang in infinite loop, MUST NOT panic on broken symlink
    let stats = calculate_directory_stats(&cache_dir).expect("calculate_directory_stats");
    assert_eq!(stats.file_count, 15);
    assert_eq!(stats.total_bytes, 15 * 1024);
    assert_eq!(stats.formatted_size(), "15.0 KB");
}

// =========================================================================
// Vector 9: Rapid Interleaved Token Actions & Unwedgeability
// =========================================================================

#[test]
fn test_adversarial_rapid_interleaved_token_actions_and_unwedgeability() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(100);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    // Rapidly interleave conflicting user actions
    let _ = app.update(Message::SettingsTokenChanged("ghp_first_attempt".into()));
    let _ = app.update(Message::SettingsSaveToken);
    assert!(app.settings_state.is_busy);

    // User immediately presses ClearToken before response arrives
    let _ = app.update(Message::SettingsClearToken);
    assert!(app.settings_state.is_busy);
    assert!(app.settings_state.token_input.is_empty());

    // User tests connection before response arrives
    let _ = app.update(Message::SettingsTestConnection);
    assert!(app.settings_state.is_busy);
    assert!(app.is_testing_connection);

    // Simulate RateLimitsFetchFailed arriving
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsFetchFailed {
        error: "Connection refused".into(),
    }));

    // Invariant: Failure must UNWEDGE the GUI (is_busy = false, is_testing_connection = false)
    assert!(
        !app.settings_state.is_busy,
        "GUI remained in busy state after connection test failure!"
    );
    assert!(
        !app.is_testing_connection,
        "is_testing_connection remained true after connection test failure!"
    );
    assert!(app.settings_state.status_notice.is_some());
    let (notice, is_err) = app.settings_state.status_notice.as_ref().unwrap();
    assert!(is_err, "Notice must indicate error");
    assert!(notice.contains("Connection test failed"));

    // Now test TokenVerificationFailed unwedgeability
    let _ = app.update(Message::SettingsTokenChanged("ghp_bad_pat".into()));
    let _ = app.update(Message::SettingsSaveToken);
    assert!(app.settings_state.is_busy);

    let _ = app.update(Message::Engine(EngineEvent::TokenVerificationFailed {
        error: "401 Unauthorized".into(),
    }));

    assert!(
        !app.settings_state.is_busy,
        "GUI remained in busy state after TokenVerificationFailed!"
    );
    assert_eq!(app.pending_token_save, None);
}

// =========================================================================
// Vector 10: Massive 1MB Token String & Timeout Clamping Invariants
// =========================================================================

#[test]
fn test_adversarial_massive_token_buffer_and_timeout_bounds() {
    let mut state = SettingsState::new(None);

    // 1. Timeout bounds clamping: [5, 60]
    let timeout_cases = [
        (0, 5),
        (1, 5),
        (4, 5),
        (5, 5),
        (30, 30),
        (60, 60),
        (61, 60),
        (100, 60),
        (u64::MAX, 60),
    ];

    for (input, expected) in timeout_cases {
        state.set_timeout(input);
        assert_eq!(
            state.request_timeout_secs, expected,
            "Timeout {} was not correctly clamped to {}",
            input, expected
        );
    }

    // 2. 1,000,000 character token string (1 MB)
    let huge_token = "a".repeat(1_000_000);
    state.set_token_input(huge_token.clone());
    assert_eq!(state.token_input.len(), 1_000_000);

    // Masking 1MB token
    let masked = mask_token(&huge_token);
    assert_eq!(masked.len(), 4 + 8 * 3 + 4); // 4 'a's + 8 bullets (3 bytes each) + 4 'a's = 32 bytes
    assert_eq!(masked, "aaaa••••••••aaaa");

    state.set_saved(Some(huge_token));
    assert_eq!(state.masked_display(), "aaaa••••••••aaaa");

    // Headless render with huge token saved
    let gauge = RateLimitGaugeState::default();
    let theme = Theme::Dark;
    let _element = view_settings(&state, &gauge, &theme, |_| ());
}
