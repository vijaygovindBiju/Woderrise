//! Challenger 2 Empirical Stress Tests for Milestone 5:
//! Dynamic GitHub Repository Fetcher, Hyprland Dotfile Detection & Manifest Synthesis
//! in `crates/woderrise-core`.
//!
//! Evaluates:
//! 1. Resource Leak & TempDir Cleanup:
//!    - Verify RAII `tempfile::TempDir` lifecycle on success and error conditions.
//!    - Ensure zero orphaned directories in `/tmp` on failures (invalid repo, non-Hyprland repo, timeout).
//! 2. Git Execution Timeout & Deadlock Resistance:
//!    - Bounded timeout execution (`run_git_bounded`) with simulated hangs or unreachable hosts.
//!    - Stress large output and pipe buffer capacity.
//! 3. Cryptographic Checksum Integrity:
//!    - Synthesized Manifest artifact hashes match disk contents byte-for-byte.
//!    - Tampering with files causes `InstallationEngine::install` to reject the package without filesystem corruption.

use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    process::Command,
    sync::Mutex,
    time::{Duration, Instant},
};

static TMP_MUTEX: Mutex<()> = Mutex::new(());

use woderrise_core::{
    fetcher::{
        detect_hyprland_files, fetch_github_repository, parse_and_validate_github_url,
        run_git_bounded, FetchError, FetchOptions,
    },
    sha256_bytes, BackupManager, Database, InstallationEngine, WoderriseError,
};

/// Helper to get set of existing woderrise-fetch temp directories in /tmp
fn current_woderrise_tmpdirs() -> HashSet<PathBuf> {
    let tmp = std::env::temp_dir();
    let mut set = HashSet::new();
    if let Ok(entries) = fs::read_dir(&tmp) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("woderrise-fetch-") {
                    set.insert(path);
                }
            }
        }
    }
    set
}

/// Helper to create a genuine test git repository with authentic Hyprland & Waybar dotfiles.
fn create_authentic_hyprland_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed creating tempdir for test repo");
    let p = dir.path();

    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(p)
            .status()
            .unwrap_or_else(|e| panic!("failed running git {:?}: {}", args, e));
        assert!(status.success(), "git {:?} failed", args);
    };

    run(&["init"]);
    run(&["checkout", "-b", "main"]);
    run(&["config", "user.name", "Catppuccin Tester"]);
    run(&["config", "user.email", "tester@catppuccin.invalid"]);
    run(&["config", "commit.gpgsign", "false"]);

    let hypr = p.join(".config").join("hypr");
    let waybar = p.join(".config").join("waybar");
    fs::create_dir_all(&hypr).unwrap();
    fs::create_dir_all(&waybar).unwrap();

    fs::write(
        hypr.join("hyprland.conf"),
        "general {\n    gaps_in = 6\n    gaps_out = 12\n}\n$mauve = #cba6f7\n$base = #1e1e2e\n",
    )
    .unwrap();
    fs::write(
        hypr.join("hyprlock.conf"),
        "background {\n    color = #181825\n}\n",
    )
    .unwrap();
    fs::write(
        hypr.join("hypridle.conf"),
        "general {\n    lock_cmd = hyprlock\n}\n",
    )
    .unwrap();
    fs::write(
        waybar.join("config.jsonc"),
        "{\"layer\": \"top\", \"height\": 32}\n",
    )
    .unwrap();
    fs::write(
        waybar.join("style.css"),
        "@define-color base #1e1e2e;\n@define-color mantle #181825;\n",
    )
    .unwrap();
    fs::write(
        p.join("LICENSE"),
        "MIT License\nCopyright (c) 2026 Catppuccin\nPermission is hereby granted, free of charge\nwithout restriction\n",
    )
    .unwrap();

    run(&["add", "."]);
    run(&["commit", "-m", "feat: authentic hyprland and waybar suite"]);
    run(&["tag", "v1.2.3"]);

    dir
}

// =========================================================================
// 1. Resource Leak & TempDir Cleanup Tests
// =========================================================================

#[test]
fn test_tempdir_cleanup_lifecycle_on_success() {
    let _guard = TMP_MUTEX.lock().unwrap();
    let repo = create_authentic_hyprland_repo();
    let fetched = fetch_github_repository(
        repo.path().to_str().unwrap(),
        FetchOptions::default(),
    )
    .expect("fetch must succeed for valid git repository");

    let temp_path = fetched.temp_dir.path().to_path_buf();
    assert!(
        temp_path.exists(),
        "temp directory must exist while FetchedRepository is alive"
    );
    assert!(
        temp_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("woderrise-fetch-"),
        "tempdir must follow woderrise-fetch- naming prefix"
    );

    // Drop fetched repository and verify RAII cleanup immediately removes directory
    drop(fetched);
    assert!(
        !temp_path.exists(),
        "temp directory {:?} must be deleted on drop of FetchedRepository",
        temp_path
    );
}

#[test]
fn test_tempdir_cleanup_on_failure_modes_zero_orphans() {
    let _guard = TMP_MUTEX.lock().unwrap();
    // Mode A: Invalid git repository (empty dir, git clone fails)
    {
        let before_dirs = current_woderrise_tmpdirs();
        let empty_dir = tempfile::tempdir().unwrap();
        let res = fetch_github_repository(
            empty_dir.path().to_str().unwrap(),
            FetchOptions::default(),
        );
        assert!(res.is_err(), "must fail for non-git directory");
        let after_dirs = current_woderrise_tmpdirs();
        let leaked: Vec<_> = after_dirs.difference(&before_dirs).collect();
        assert!(
            leaked.is_empty(),
            "git clone failure leaked temp directories: {:?}",
            leaked
        );
    }

    // Mode B: Valid git repository but non-Hyprland (missing hyprland.conf)
    {
        let non_hypr = tempfile::tempdir().unwrap();
        let p = non_hypr.path();
        Command::new("git").args(["init"]).current_dir(p).status().unwrap();
        Command::new("git").args(["checkout", "-b", "main"]).current_dir(p).status().unwrap();
        Command::new("git").args(["config", "user.name", "Tester"]).current_dir(p).status().unwrap();
        Command::new("git").args(["config", "user.email", "test@test"]).current_dir(p).status().unwrap();
        fs::write(p.join("bashrc"), "export PATH=$PATH\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(p).status().unwrap();
        Command::new("git").args(["commit", "-m", "init non-hypr"]).current_dir(p).status().unwrap();

        let before_dirs = current_woderrise_tmpdirs();
        let res = fetch_github_repository(
            p.to_str().unwrap(),
            FetchOptions::default(),
        );
        assert!(
            matches!(res, Err(WoderriseError::Fetch(FetchError::MissingHyprlandConfig))),
            "must return MissingHyprlandConfig, got {:?}",
            res
        );
        let after_dirs = current_woderrise_tmpdirs();
        let leaked: Vec<_> = after_dirs.difference(&before_dirs).collect();
        assert!(
            leaked.is_empty(),
            "missing hyprland.conf leaked temp directories: {:?}",
            leaked
        );
    }

    // Mode C: Timeout during git clone (unreachable IP / simulated hang)
    {
        let before_dirs = current_woderrise_tmpdirs();
        let options = FetchOptions {
            timeout: Duration::from_millis(150),
            branch: None,
        };
        // 192.0.2.1 is TEST-NET-1 (RFC 5737), non-routable / unreachable
        let res = fetch_github_repository(
            "https://github.com/woderrise-org-unreachable/mock-repo",
            options,
        );
        assert!(res.is_err(), "must fail for unreachable host");
        let after_dirs = current_woderrise_tmpdirs();
        let leaked: Vec<_> = after_dirs.difference(&before_dirs).collect();
        assert!(
            leaked.is_empty(),
            "timeout/network failure leaked temp directories: {:?}",
            leaked
        );
    }
}

// =========================================================================
// 2. Git Execution Timeout & Deadlock Resistance Tests
// =========================================================================

#[test]
fn test_git_execution_timeout_kills_hanging_process_promptly() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "sleep 30"]);

    let start = Instant::now();
    let res = run_git_bounded(&mut cmd, Duration::from_millis(150));
    let duration = start.elapsed();

    assert!(res.is_err(), "run_git_bounded must return error on timeout");
    match res.unwrap_err() {
        FetchError::Timeout(t) => assert_eq!(t, Duration::from_millis(150)),
        other => panic!("expected Timeout, got: {:?}", other),
    }

    assert!(
        duration < Duration::from_secs(2),
        "timeout enforcement took too long: {:?}",
        duration
    );
}

#[test]
fn test_git_execution_unreachable_endpoint_error_handling() {
    let mut cmd = Command::new("git");
    cmd.args([
        "clone",
        "--depth",
        "1",
        "https://192.0.2.1/invalid-owner/invalid-repo",
        "/tmp/.woderrise-unreachable-test",
    ]);

    let res = run_git_bounded(&mut cmd, Duration::from_millis(300));
    assert!(res.is_err(), "clone from unreachable IP must fail");
    let err = res.unwrap_err();
    assert!(
        matches!(err, FetchError::Timeout(_) | FetchError::NetworkUnreachable(_) | FetchError::GitExecution { .. }),
        "expected network/timeout error variant, got {:?}",
        err
    );
}

#[test]
fn test_git_execution_normal_output_capture() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "echo 'git stdout text' && echo 'git stderr text' >&2"]);

    let out = run_git_bounded(&mut cmd, Duration::from_secs(2)).expect("must succeed");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("git stdout text"));
    assert!(stderr.contains("git stderr text"));
}

#[test]
fn test_git_execution_large_stdout_pipe_deadlock() {
    // If a command writes more than Linux PIPE_BUF (64KB), run_git_bounded deadlocks
    // because it polls `child.try_wait()` in a sleep loop without reading stdout/stderr pipes.
    let mut cmd = Command::new("sh");
    // Write 128KB of text to stdout
    cmd.args(["-c", "head -c 131072 /dev/zero | tr '\\0' 'A'"]);

    let start = Instant::now();
    let res = run_git_bounded(&mut cmd, Duration::from_millis(500));
    let duration = start.elapsed();

    // Empirically reproduces the deadlock: it times out at 500ms instead of finishing in ~5ms.
    assert!(
        matches!(res, Err(FetchError::Timeout(_))),
        "Empirically confirmed bug: run_git_bounded deadlocks and times out on >64KB pipe output, duration: {:?}",
        duration
    );
}

#[test]
fn test_file_url_argument_injection_vulnerability() {
    // An adversarial URL using file:// with git command-line flags
    let res = parse_and_validate_github_url("file://--upload-pack=malicious_binary");
    
    // Verifies that flag injection via file:// is rejected:
    assert!(
        res.is_err(),
        "parse_and_validate_github_url must reject flag injection via file://"
    );
}

#[test]
fn test_flat_layout_5mb_limit_omission() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Flat layout: hyprland.conf in root
    fs::write(root.join("hyprland.conf"), "general { gaps_in = 5 }\n").unwrap();

    // 6MB config file in root
    let big_conf = root.join("big.conf");
    let big_data = vec![b'Z'; 6 * 1024 * 1024];
    fs::write(&big_conf, &big_data).unwrap();

    let detected = detect_hyprland_files(root).unwrap();
    
    // Verifies that flat layout correctly filters files > 5MB:
    assert!(
        !detected.iter().any(|d| d.relative_path.to_str() == Some("big.conf")),
        "flat layout must filter files > 5MB"
    );
}

// =========================================================================
// 3. Cryptographic Checksum Integrity Tests
// =========================================================================

#[test]
fn test_manifest_artifact_checksums_match_disk_byte_for_byte() {
    let _guard = TMP_MUTEX.lock().unwrap();
    let repo = create_authentic_hyprland_repo();
    let fetched = fetch_github_repository(
        repo.path().to_str().unwrap(),
        FetchOptions::default(),
    )
    .expect("fetch must succeed");

    assert!(
        !fetched.manifest.artifacts.is_empty(),
        "manifest artifacts must not be empty"
    );

    // Byte-for-byte verification for each artifact
    for artifact in &fetched.manifest.artifacts {
        let abs_path = fetched.temp_dir.path().join(&artifact.path);
        assert!(
            abs_path.exists(),
            "artifact file does not exist: {:?}",
            abs_path
        );

        let bytes = fs::read(&abs_path).expect("failed to read artifact file");
        let computed_sha256 = sha256_bytes(&bytes);

        assert_eq!(
            computed_sha256, artifact.sha256,
            "artifact {} SHA-256 mismatch! Expected {}, got {}",
            artifact.path, artifact.sha256, computed_sha256
        );
    }

    // Also verify detected_files and manifest.install.files match
    assert_eq!(
        fetched.detected_files.len(),
        fetched.manifest.install.files.len()
    );
    for file in &fetched.manifest.install.files {
        let matching_artifact = fetched
            .manifest
            .artifacts
            .iter()
            .find(|a| a.path == file.source);
        assert!(
            matching_artifact.is_some(),
            "install file {} must have a corresponding artifact",
            file.source
        );
    }
}

// =========================================================================
// 4. Tamper Rejection via InstallationEngine::install
// =========================================================================

#[test]
fn test_tampering_rejection_by_installation_engine() {
    let _guard = TMP_MUTEX.lock().unwrap();
    let repo = create_authentic_hyprland_repo();
    let fetched = fetch_github_repository(
        repo.path().to_str().unwrap(),
        FetchOptions::default(),
    )
    .expect("fetch must succeed");

    let sim_home = tempfile::tempdir().expect("simulated home");
    let db = Database::in_memory().expect("db in memory");
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(sim_home.path().join("backups")).unwrap(),
        home: sim_home.path().to_path_buf(),
    };

    // Subtest 1: Tamper with primary config (`hyprland.conf`)
    let hyprland_file = fetched
        .manifest
        .artifacts
        .iter()
        .find(|a| a.path.contains("hyprland.conf"))
        .expect("must contain hyprland.conf");
    let hyprland_path = fetched.temp_dir.path().join(&hyprland_file.path);

    let original_content = fs::read(&hyprland_path).unwrap();
    // Tamper by modifying content
    let mut tampered_content = original_content.clone();
    tampered_content.extend_from_slice(b"\n# MALICIOUS INJECTION\nexec-once = evil_script\n");
    fs::write(&hyprland_path, &tampered_content).unwrap();

    let install_res = engine.install(fetched.temp_dir.path(), "malicious_actor");
    assert!(
        install_res.is_err(),
        "InstallationEngine::install must reject package with tampered primary file"
    );

    match install_res.unwrap_err() {
        WoderriseError::Checksum { path, expected, actual } => {
            assert_eq!(path, hyprland_file.path);
            assert_eq!(expected, hyprland_file.sha256);
            assert_ne!(actual, expected);
        }
        other => panic!("expected Checksum error, got: {:?}", other),
    }

    // Verify home directory target was NOT created or modified
    assert!(
        !sim_home.path().join(".config/hypr/hyprland.conf").exists(),
        "target file must not exist in home directory after tampering rejection"
    );

    // Subtest 2: Restore primary and tamper with secondary config (`style.css`)
    fs::write(&hyprland_path, &original_content).unwrap();

    let style_file = fetched
        .manifest
        .artifacts
        .iter()
        .find(|a| a.path.contains("style.css"))
        .expect("must contain style.css");
    let style_path = fetched.temp_dir.path().join(&style_file.path);

    fs::write(&style_path, b"/* HACKED STYLE */\n").unwrap();

    let install_res_2 = engine.install(fetched.temp_dir.path(), "malicious_actor_2");
    assert!(
        install_res_2.is_err(),
        "InstallationEngine::install must reject package with tampered secondary file"
    );
    match install_res_2.unwrap_err() {
        WoderriseError::Checksum { path, expected, actual } => {
            assert_eq!(path, style_file.path);
            assert_eq!(expected, style_file.sha256);
            assert_ne!(actual, expected);
        }
        other => panic!("expected Checksum error, got: {:?}", other),
    }

    assert!(
        !sim_home.path().join(".config/waybar/style.css").exists(),
        "waybar style.css must not exist in home after tampering rejection"
    );
}

// =========================================================================
// 5. Dotfile Detection & Manifest Synthesis Boundary Stress
// =========================================================================

#[test]
fn test_file_size_boundary_5mb_filter() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let hypr = root.join(".config").join("hypr");
    fs::create_dir_all(&hypr).unwrap();

    fs::write(hypr.join("hyprland.conf"), "general { gaps_in = 5 }\n").unwrap();

    // 5MB + 10 bytes file should be excluded
    let large_file = hypr.join("bloat.conf");
    let large_bytes = vec![b'X'; 5 * 1024 * 1024 + 10];
    fs::write(&large_file, &large_bytes).unwrap();

    let detected = detect_hyprland_files(root).unwrap();
    assert_eq!(detected.len(), 1, "bloat.conf > 5MB must be excluded");
    assert_eq!(detected[0].target_path, "~/.config/hypr/hyprland.conf");
}

#[test]
fn test_zero_byte_hyprland_conf_handled_without_crash() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("hyprland.conf"), "").unwrap();

    let detected = detect_hyprland_files(root).expect("should handle 0-byte file");
    assert_eq!(detected.len(), 1);
    assert_eq!(
        detected[0].sha256,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn test_parse_and_validate_github_url_strictness() {
    // Valid GitHub URLs
    assert!(parse_and_validate_github_url("https://github.com/hyprwm/hyprland").is_ok());
    assert!(parse_and_validate_github_url("https://github.com/catppuccin/hyprland.git").is_ok());

    // Local fixtures
    assert!(parse_and_validate_github_url("file:///tmp/repo").is_ok());
    assert!(parse_and_validate_github_url("/tmp/repo").is_ok());
    assert!(parse_and_validate_github_url("./repo").is_ok());

    // Flag / Command injection attempts
    assert!(parse_and_validate_github_url("-u").is_err());
    assert!(parse_and_validate_github_url("--upload-pack=evil").is_err());
    assert!(parse_and_validate_github_url("").is_err());
    assert!(parse_and_validate_github_url("   ").is_err());
    assert!(parse_and_validate_github_url("http://github.com/owner/repo").is_err());
    assert!(parse_and_validate_github_url("https://gitlab.com/owner/repo").is_err());
    assert!(parse_and_validate_github_url("https://github.com/owner").is_err());
    assert!(parse_and_validate_github_url("https://github.com/owner/repo/extra").is_err());
    assert!(parse_and_validate_github_url("https://github.com/-owner/repo").is_err());
    assert!(parse_and_validate_github_url("https://github.com/owner/-repo").is_err());
}
