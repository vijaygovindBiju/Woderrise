//! Empirical Challenger Adversarial Test Suite for Milestone M5 (UI Settings & Cache Management):
//! 1. Symlink escape resistance and directory traversal safety in `safe_cleanup_cache`
//! 2. Preservation of $XDG_CONFIG_HOME, $XDG_STATE_HOME, and $XDG_DATA_HOME under adversarial directory structures
//! 3. Resilient directory stats calculation against symlink loops, broken links, and recursion traps
//! 4. Headless `view_settings` rendering across 4 tabs x 8 themes x 6 rate limit tiers x extreme edge states
//! 5. Numerical boundaries and time calculation safety in `format_reset_countdown` and `format_bytes`

use camino::Utf8PathBuf;
use chrono::{Duration, Utc};
use iced::Theme;
use std::fs;
use std::os::unix::fs::symlink;
use tempfile::tempdir;
use woderrise_core::paths::XdgPaths;
use woderrise_gui::app::Message;
use woderrise_gui::screens::search::RateLimitGaugeState;
use woderrise_gui::screens::settings::{
    calculate_directory_stats, format_bytes, format_reset_countdown, safe_cleanup_cache,
    view_settings, CacheStats, SettingsState, SettingsTab, ThemeMode, UiDensity,
};

/// 1. ADVERSARIAL STRESS TEST: Symlinks inside staging must NEVER cause deletion of outside targets.
#[test]
fn test_adversarial_symlink_inside_staging_targets_never_deleted() {
    let tmp = tempdir().expect("create temp dir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    // Create outside sensitive directory with valuable files
    let sensitive_dir = base.join("sensitive_outside_data");
    fs::create_dir_all(&sensitive_dir).unwrap();
    let secret_file = sensitive_dir.join("id_rsa");
    let secret_config = sensitive_dir.join("config.toml");
    fs::write(&secret_file, b"SUPER_SECRET_PRIVATE_KEY").unwrap();
    fs::write(&secret_config, b"[user]\nname = \"target\"").unwrap();

    // Create cache directory structure
    let cache_dir = base.join("cache");
    let staging_dir = cache_dir.join("staging");
    fs::create_dir_all(&staging_dir).unwrap();

    // In staging, place:
    // a) A symlink pointing directly to the outside sensitive directory
    let symlink_dir_path = staging_dir.join("link_to_outside_dir");
    symlink(sensitive_dir.as_std_path(), symlink_dir_path.as_std_path()).unwrap();

    // b) A symlink pointing directly to the outside sensitive file
    let symlink_file_path = staging_dir.join("link_to_secret_file");
    symlink(secret_file.as_std_path(), symlink_file_path.as_std_path()).unwrap();

    // c) A legitimate subdirectory containing a symlink to outside data
    let legitimate_sub = staging_dir.join("legit_tx_123");
    fs::create_dir_all(&legitimate_sub).unwrap();
    fs::write(legitimate_sub.join("temp.blob"), b"junk data").unwrap();
    let nested_link = legitimate_sub.join("nested_link_to_secret");
    symlink(secret_file.as_std_path(), nested_link.as_std_path()).unwrap();

    // d) Loose cache blob in cache root
    fs::write(cache_dir.join("loose.blob"), b"loose cache blob").unwrap();

    // Execute safe_cleanup_cache
    let res = safe_cleanup_cache(&cache_dir);
    assert!(res.is_ok(), "safe_cleanup_cache failed: {:?}", res);

    // CRITICAL SECURITY ASSERTIONS:
    // Outside sensitive directory and its files MUST REMAIN 100% INTACT AND UNMODIFIED!
    assert!(
        secret_file.exists(),
        "CRITICAL VULNERABILITY: Outside target secret_file was deleted through symlink traversal!"
    );
    assert_eq!(
        fs::read(&secret_file).unwrap(),
        b"SUPER_SECRET_PRIVATE_KEY",
        "Outside target content was modified or corrupted!"
    );

    assert!(
        secret_config.exists(),
        "CRITICAL VULNERABILITY: Outside target secret_config was deleted through symlink traversal!"
    );
    assert_eq!(
        fs::read(&secret_config).unwrap(),
        b"[user]\nname = \"target\"",
        "Outside target config was modified or corrupted!"
    );

    // Ephemeral staging items inside cache should have been cleaned
    assert!(
        !legitimate_sub.exists(),
        "Legitimate staging subdirectory should have been cleaned"
    );
    assert!(
        !cache_dir.join("loose.blob").exists(),
        "Loose cache blob should have been removed"
    );
}

/// 2. ADVERSARIAL STRESS TEST: When staging itself is a symlink pointing outside, outside files must NOT be deleted.
#[test]
fn test_adversarial_staging_itself_is_symlink_does_not_destroy_target() {
    let tmp = tempdir().expect("create temp dir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    // Create outside directory simulating user's home or config
    let outside_dir = base.join("user_config");
    fs::create_dir_all(&outside_dir).unwrap();
    let important_file = outside_dir.join("important.conf");
    fs::write(&important_file, b"important settings").unwrap();

    // Cache directory where "staging" is an adversarial symlink pointing to outside_dir
    let cache_dir = base.join("cache");
    fs::create_dir_all(&cache_dir).unwrap();
    let staging_symlink = cache_dir.join("staging");
    symlink(outside_dir.as_std_path(), staging_symlink.as_std_path()).unwrap();

    // Run safe_cleanup_cache
    let _ = safe_cleanup_cache(&cache_dir);

    // Outside important file must NOT be deleted or purged!
    assert!(
        important_file.exists(),
        "CRITICAL VULNERABILITY: Staging symlink allowed deletion of outside directory contents!"
    );
    assert_eq!(
        fs::read(&important_file).unwrap(),
        b"important settings",
        "Target file content was modified!"
    );
}

/// 2b. ADVERSARIAL STRESS TEST: When staging is symlinked to $XDG_CONFIG_HOME, settings.json must NOT be destroyed.
#[test]
fn test_adversarial_staging_symlink_to_xdg_config_destroys_settings() {
    let tmp = tempdir().expect("create temp dir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);

    let config_dir = xdg.woderrise_config_dir();
    fs::create_dir_all(&config_dir).unwrap();
    let settings_file = xdg.settings_file();
    fs::write(
        &settings_file,
        b"{\"version\":1,\"github_token\":\"ghp_secret\"}",
    )
    .unwrap();

    let cache_dir = xdg.woderrise_cache_dir();
    fs::create_dir_all(&cache_dir).unwrap();

    // Adversarial symlink: staging points to config_dir
    let staging_symlink = cache_dir.join("staging");
    symlink(config_dir.as_std_path(), staging_symlink.as_std_path()).unwrap();

    let _ = safe_cleanup_cache(&cache_dir);

    // settings.json MUST NOT be destroyed!
    assert!(
        settings_file.exists(),
        "CRITICAL VULNERABILITY: settings.json was deleted because cache/staging was symlinked to config directory!"
    );
}

/// 3. ADVERSARIAL STRESS TEST: Loose symlinks in cache root pointing to outside files/directories.
#[test]
fn test_adversarial_loose_symlinks_in_cache_root() {
    let tmp = tempdir().expect("create temp dir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let outside_root = base.join("outside_system");
    fs::create_dir_all(&outside_root).unwrap();
    let system_target_file = outside_root.join("system_hosts");
    fs::write(&system_target_file, b"127.0.0.1 localhost").unwrap();

    let system_target_dir = outside_root.join("system_lib");
    fs::create_dir_all(&system_target_dir).unwrap();
    fs::write(system_target_dir.join("libtest.so"), b"binary bytes").unwrap();

    let cache_dir = base.join("cache");
    fs::create_dir_all(&cache_dir).unwrap();

    // Create symlink to outside file in cache root
    let symlink_file = cache_dir.join("symlink_to_hosts");
    symlink(system_target_file.as_std_path(), symlink_file.as_std_path()).unwrap();

    // Create symlink to outside directory in cache root
    let symlink_dir = cache_dir.join("symlink_to_lib");
    symlink(system_target_dir.as_std_path(), symlink_dir.as_std_path()).unwrap();

    // Run safe_cleanup_cache
    let res = safe_cleanup_cache(&cache_dir);
    assert!(res.is_ok(), "safe_cleanup_cache failed: {:?}", res);

    // Verify system target file is NOT deleted
    assert!(
        system_target_file.exists(),
        "Outside target file was deleted when loose symlink was removed!"
    );
    assert_eq!(
        fs::read(&system_target_file).unwrap(),
        b"127.0.0.1 localhost"
    );

    // Verify system target directory is NOT deleted
    assert!(
        system_target_dir.join("libtest.so").exists(),
        "Outside target directory contents were deleted!"
    );
}

/// 4. ADVERSARIAL STRESS TEST: Circular symlinks, broken/dangling symlinks, and directory stats calculations.
#[test]
fn test_adversarial_circular_and_broken_symlinks() {
    let tmp = tempdir().expect("create temp dir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let cache_dir = base.join("cache");
    let staging_dir = cache_dir.join("staging");
    fs::create_dir_all(&staging_dir).unwrap();

    // Broken / dangling symlink pointing to non-existent location
    let dangling = staging_dir.join("dangling_link");
    symlink(
        base.join("non_existent_path_xyz").as_std_path(),
        dangling.as_std_path(),
    )
    .unwrap();

    // Circular symlink pointing back to parent staging directory
    let circular = staging_dir.join("circular_link");
    symlink(staging_dir.as_std_path(), circular.as_std_path()).unwrap();

    // Legitimate file
    fs::write(staging_dir.join("real_file.bin"), b"12345678").unwrap();

    // 1. calculate_directory_stats must NOT get caught in an infinite loop or panic on broken/circular symlinks
    let stats = calculate_directory_stats(&cache_dir);
    assert!(
        stats.is_ok(),
        "calculate_directory_stats must handle symlinks safely: {:?}",
        stats
    );
    let s = stats.unwrap();
    // Only the real file should be counted (symlinks skipped for safety)
    assert_eq!(s.file_count, 1);
    assert_eq!(s.total_bytes, 8);

    // 2. safe_cleanup_cache must handle dangling and circular links cleanly without panic
    let cleanup_res = safe_cleanup_cache(&cache_dir);
    assert!(
        cleanup_res.is_ok(),
        "safe_cleanup_cache must handle dangling links: {:?}",
        cleanup_res
    );
}

/// 5. ADVERSARIAL STRESS TEST: Complete preservation of XDG directories under corrupt/readonly files.
#[test]
fn test_adversarial_strict_xdg_preservation_with_corrupt_and_readonly_files() {
    let tmp = tempdir().expect("create temp dir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);

    // Setup XDG directories
    let config_dir = xdg.woderrise_config_dir();
    let data_dir = xdg.woderrise_data_dir();
    let state_dir = xdg.woderrise_state_dir();
    let cache_dir = xdg.woderrise_cache_dir();

    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&data_dir).unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    // Corrupted non-UTF8 binary data in config
    let config_file = xdg.settings_file();
    let binary_garbage = vec![0xFF, 0xFE, 0x00, 0x12, 0x7F, 0xAA];
    fs::write(&config_file, &binary_garbage).unwrap();

    // Corrupted JSON in data
    let journal_file = xdg.journal_file();
    fs::write(&journal_file, b"{\"broken_json\": [").unwrap();

    // Backup file in state
    let backup_dir = state_dir.join("backups");
    fs::create_dir_all(&backup_dir).unwrap();
    let backup_tar = backup_dir.join("snapshot_1.tar");
    fs::write(&backup_tar, b"TAR_HEADER_CONTENT").unwrap();

    // Fill cache with ephemeral staging data
    let staging = cache_dir.join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("chunk.bin"), b"chunk").unwrap();

    // Run safe_cleanup_cache on cache_dir
    let res = safe_cleanup_cache(&cache_dir);
    assert!(res.is_ok(), "safe_cleanup_cache failed: {:?}", res);

    // Verify all XDG files remain 100% bit-for-bit unchanged
    assert_eq!(
        fs::read(&config_file).unwrap(),
        binary_garbage,
        "Config file was modified or corrupted!"
    );
    assert_eq!(
        fs::read(&journal_file).unwrap(),
        b"{\"broken_json\": [",
        "Journal file was modified or corrupted!"
    );
    assert_eq!(
        fs::read(&backup_tar).unwrap(),
        b"TAR_HEADER_CONTENT",
        "Backup tar was modified or corrupted!"
    );
}

/// 6. ADVERSARIAL STRESS TEST: Headless view_settings rendering across 4 tabs x 8 themes x 6 rate limit tiers x edge cases.
#[test]
fn test_adversarial_view_settings_exhaustive_matrix() {
    let tmp = tempdir().expect("create temp dir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);

    let tabs = [
        SettingsTab::GitHub,
        SettingsTab::Appearance,
        SettingsTab::NetworkCache,
        SettingsTab::About,
    ];

    let themes = [
        Theme::Dark,
        Theme::Light,
        Theme::SolarizedDark,
        Theme::SolarizedLight,
        Theme::CatppuccinMocha,
        Theme::CatppuccinLatte,
        Theme::Dracula,
        Theme::Nord,
    ];

    let rate_limit_scenarios = [
        // Normal tier
        RateLimitGaugeState::new(4900, 5000, 29, 30),
        // Moderate tier
        RateLimitGaugeState::new(2500, 5000, 15, 30),
        // Low quota tier
        RateLimitGaugeState::new(200, 5000, 3, 30),
        // Throttled / Exhausted
        RateLimitGaugeState::new(0, 5000, 0, 30),
        // Zero limit edge case (must not divide by zero)
        RateLimitGaugeState::new(0, 0, 0, 0),
        // Saturated (remaining > limit)
        RateLimitGaugeState::new(6000, 5000, 50, 30),
    ];

    for tab in tabs {
        for theme in &themes {
            for rate_limits in &rate_limit_scenarios {
                // Test 1: Full diagnostics, authenticated, masked
                {
                    let mut state =
                        SettingsState::with_xdg(Some("ghp_test_token_1234".into()), Some(&xdg));
                    state.active_tab = tab;
                    state.username = Some("test_user".into());
                    state.status_notice = Some(("Operation succeeded.".into(), false));
                    state.core_reset = Some(Utc::now() + Duration::minutes(45));
                    state.search_reset = Some(Utc::now() + Duration::seconds(25));
                    state.cache_stats = CacheStats {
                        file_count: 142,
                        total_bytes: 45_890_123,
                    };

                    let _el = view_settings(&state, rate_limits, theme, |_| Message::Noop);
                }

                // Test 2: Error status notice, busy state, unmasked token
                {
                    let mut state =
                        SettingsState::with_xdg(Some("ghp_test_token_1234".into()), Some(&xdg));
                    state.active_tab = tab;
                    state.is_busy = true;
                    state.is_masked = false;
                    state.status_notice = Some(("401 Unauthorized: Invalid PAT.".into(), true));
                    state.core_reset = Some(Utc::now() - Duration::seconds(10)); // past reset
                    state.search_reset = None;

                    let _el2 = view_settings(&state, rate_limits, theme, |_| Message::Noop);
                }

                // Test 3: Unauthenticated, no diagnostics, clearing cache state
                {
                    let mut unauth_state = SettingsState::with_xdg(None, None);
                    unauth_state.active_tab = tab;
                    unauth_state.is_clearing_cache = true;
                    unauth_state.theme_mode = ThemeMode::Light;
                    unauth_state.ui_density = UiDensity::Compact;

                    let _el3 = view_settings(&unauth_state, rate_limits, theme, |_| Message::Noop);
                }

                // Test 4: Extreme unicode status notice and extreme cache stats
                {
                    let mut extreme_state = SettingsState::with_xdg(None, Some(&xdg));
                    extreme_state.active_tab = tab;
                    extreme_state.status_notice = Some((
                        "🔥⚠️ Unicode multi-line error \n\t with tabs and \u{200B}zero-width characters!".into(),
                        true,
                    ));
                    extreme_state.cache_stats = CacheStats {
                        file_count: 1_000_000,
                        total_bytes: u64::MAX,
                    };
                    extreme_state.ui_density = UiDensity::Comfortable;

                    let _el4 = view_settings(&extreme_state, rate_limits, theme, |_| Message::Noop);
                }
            }
        }
    }
}

/// 7. ADVERSARIAL STRESS TEST: Countdown time formatting edge cases and boundaries.
#[test]
fn test_adversarial_format_reset_countdown_boundaries() {
    let now = Utc::now();

    // Reset in the past
    assert_eq!(
        format_reset_countdown(now - Duration::seconds(10)),
        "resets now"
    );
    assert_eq!(
        format_reset_countdown(now - Duration::minutes(5)),
        "resets now"
    );

    // Reset right now (0 seconds)
    assert_eq!(format_reset_countdown(now), "resets now");

    // Reset in less than 60 seconds (with 100ms jitter buffer)
    assert_eq!(
        format_reset_countdown(now + Duration::milliseconds(1100)),
        "resets in 1s"
    );
    assert_eq!(
        format_reset_countdown(now + Duration::milliseconds(30100)),
        "resets in 30s"
    );
    assert_eq!(
        format_reset_countdown(now + Duration::milliseconds(59100)),
        "resets in 59s"
    );

    // Reset in minutes
    assert_eq!(
        format_reset_countdown(now + Duration::seconds(65)),
        "resets in 1 min"
    );
    assert_eq!(
        format_reset_countdown(now + Duration::seconds(3599)),
        "resets in 59 min"
    );

    // Reset in over 1 hour (with jitter buffer)
    assert_eq!(
        format_reset_countdown(now + Duration::seconds(3610)),
        "resets in 1h 0m"
    );
    assert_eq!(
        format_reset_countdown(now + Duration::seconds(7200 + 190)),
        "resets in 2h 3m"
    );
}

/// 8. ADVERSARIAL STRESS TEST: Byte formatting boundaries and extreme numbers.
#[test]
fn test_adversarial_format_bytes_boundaries() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(1), "1 B");
    assert_eq!(format_bytes(1023), "1023 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(1024 * 1024 - 1), "1024.0 KB");
    assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
    assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.00 GB");

    // Extreme number should not panic or overflow
    let formatted_max = format_bytes(u64::MAX);
    assert!(formatted_max.contains("GB"));
}
