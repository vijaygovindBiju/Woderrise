//! E2E Test Suite for R3: Personal Access Token (PAT) Settings Integration & CLI.
//!
//! Verifies:
//! - Opaque-box execution of the compiled `woderrise` CLI executable
//! - `auth --token <TOKEN>` persists to settings.json with POSIX 0600 permissions
//! - `auth --status` outputs masked token and elevated quotas
//! - `auth --clear` removes saved credentials and reverts quotas
//! - Dynamic rate limit updates in `RateLimitTracker` and client

use super::common::TestEnvironment;
use std::fs;
use woderrise_core::models::load_settings;
use woderrise_github::RateLimitTracker;

#[test]
fn test_e2e_r3_cli_auth_lifecycle_and_posix_permissions() {
    let env = TestEnvironment::new("e2e-r3-cli");
    let settings_file = env.settings_file();

    // 1. Initial status: unauthenticated (10/min search, 60/hr core)
    let out_status1 = env.run_auth(&["--status"]);
    assert_eq!(out_status1.status.code(), Some(0));
    let s_init = String::from_utf8_lossy(&out_status1.stdout);
    assert!(s_init.contains("Unauthenticated"));
    assert!(s_init.contains("10 requests/min"));
    assert!(s_init.contains("60 requests/hr"));

    // 2. Set token via CLI
    let token = "ghp_e2e_r3_dedicated_pat_1122334455";
    let out_token = env.run_auth(&["--token", token]);
    assert_eq!(out_token.status.code(), Some(0));
    let s_token = String::from_utf8_lossy(&out_token.stdout);
    assert!(s_token.contains("saved successfully"));

    // 3. Verify POSIX 0600 file permissions on ~/.config/woderrise/settings.json
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Failed to read settings metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "settings.json must have strict POSIX 0600 permissions, got: {:o}",
            mode
        );
    }

    // 4. Verify deserialized content
    let settings =
        load_settings(&camino::Utf8PathBuf::from_path_buf(settings_file.clone()).unwrap())
            .expect("Load settings");
    assert_eq!(settings.github_token.as_deref(), Some(token));

    // 5. Verify masked status and elevated quotas
    let out_status2 = env.run_auth(&["--status"]);
    assert_eq!(out_status2.status.code(), Some(0));
    let s_auth = String::from_utf8_lossy(&out_status2.stdout);
    assert!(s_auth.contains("Authenticated"));
    assert!(s_auth.contains("ghp_••••••••4455"));
    assert!(s_auth.contains("30 requests/min"));
    assert!(s_auth.contains("5,000 requests/hr"));

    // 6. Verify clear command
    let out_clear = env.run_auth(&["--clear"]);
    assert_eq!(out_clear.status.code(), Some(0));
    let s_clear = String::from_utf8_lossy(&out_clear.stdout);
    assert!(s_clear.contains("removed"));

    // 7. Verify settings cleared and permissions maintained
    let cleared =
        load_settings(&camino::Utf8PathBuf::from_path_buf(settings_file.clone()).unwrap()).unwrap();
    assert_eq!(cleared.github_token, None);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Failed to read settings metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn test_e2e_r3_dynamic_rate_limit_updates() {
    let tracker = RateLimitTracker::new();

    // Default: unauthenticated limits
    assert_eq!(tracker.search_status().limit, 10);
    assert_eq!(tracker.core_status().limit, 60);

    // Set authenticated -> dynamic elevation
    tracker.set_authenticated(true);
    assert_eq!(tracker.search_status().limit, 30);
    assert_eq!(tracker.core_status().limit, 5000);

    // Clear authenticated -> dynamic reversion
    tracker.set_authenticated(false);
    assert_eq!(tracker.search_status().limit, 10);
    assert_eq!(tracker.core_status().limit, 60);
}

#[test]
fn test_e2e_r3_hermetic_isolation_with_ambient_xdg_config_home() {
    let decoy_guard = super::common::TempDirGuard(std::env::temp_dir().join(format!(
            "woderrise-decoy-xdg-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir_all(&decoy_guard.0).expect("failed to create decoy dir");

    let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    // Simulate ambient XDG_CONFIG_HOME in the environment
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &decoy_guard.0);
    }

    let env = TestEnvironment::new("e2e-r3-hermetic");
    let token = "ghp_hermetic_isolation_token_9988776655";

    let out = env.run_auth(&["--token", token]);

    // Restore previous environment state
    unsafe {
        match prev_xdg {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    assert_eq!(out.status.code(), Some(0));

    // Verify decoy directory remains completely untouched
    let decoy_entries = fs::read_dir(&decoy_guard.0)
        .expect("read decoy dir")
        .count();
    assert_eq!(
        decoy_entries, 0,
        "Decoy directory must remain completely untouched; found entries: {}",
        decoy_entries
    );
    assert!(
        !decoy_guard.0.join("woderrise").exists(),
        "Decoy directory should not contain woderrise directory"
    );

    // Verify sandbox's settings_file() contains the token with mode 0600
    let settings_file = env.settings_file();
    assert!(
        settings_file.exists(),
        "Settings file must exist in hermetic test environment: {:?}",
        settings_file
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Failed to read settings metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "settings.json must have strict POSIX 0600 permissions, got: {:o}",
            mode
        );
    }

    let settings = load_settings(&camino::Utf8PathBuf::from_path_buf(settings_file).unwrap())
        .expect("Load settings");
    assert_eq!(settings.github_token.as_deref(), Some(token));
}
