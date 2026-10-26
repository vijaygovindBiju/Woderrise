//! Empirical Adversarial Challenge Suite for Milestone M5
//! Dynamic GitHub Repository Fetcher, Hyprland Dotfile Detection & Manifest Synthesis

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use woderrise_core::{
    detect_hyprland_files, fetch_github_repository, parse_and_validate_github_url,
    BackupManager, Database, FetchError, FetchOptions, InstallationEngine,
};

fn init_local_git_repo(root: &Path) {
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command failed");
        assert!(status.success(), "git {:?} failed", args);
    };

    run(&["init"]);
    run(&["checkout", "-b", "main"]);
    run(&["config", "user.name", "Adversarial Tester"]);
    run(&["config", "user.email", "challenger@example.com"]);
    run(&["config", "commit.gpgsign", "false"]);
}

fn commit_all(root: &Path, msg: &str, tag: Option<&str>) {
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command failed");
        assert!(status.success(), "git {:?} failed", args);
    };

    run(&["add", "-A"]);
    run(&["commit", "-m", msg]);
    if let Some(t) = tag {
        run(&["tag", t]);
    }
}

// ============================================================================
// 1. URL Validation Adversarial Suite
// ============================================================================

#[test]
fn test_url_command_injection_syntax_rejected() {
    let injection_payloads = [
        "; rm -rf /",
        "| sh",
        "$(whoami)",
        "`whoami`",
        "https://github.com/owner/repo; rm -rf /",
        "https://github.com/owner/repo | sh",
        "https://github.com/owner/repo && whoami",
        "https://github.com/owner/repo $(whoami)",
        "https://github.com/owner/repo `whoami`",
        "https://github.com/owner/repo' OR '1'='1",
        "https://github.com/owner/repo\" --help",
        "--upload-pack=evil",
        "  --upload-pack=evil",
        "-u /tmp/evil",
        "https://github.com/--upload-pack/repo",
        "https://github.com/owner/--upload-pack",
        "https://github.com/-invalid-owner/repo",
        "https://github.com/owner/-invalid-repo",
    ];

    for payload in &injection_payloads {
        let res = parse_and_validate_github_url(payload);
        assert!(
            res.is_err(),
            "Expected payload to be rejected, but got Ok: {:?}",
            payload
        );
    }
}

#[test]
fn test_url_malformed_schemes_and_hosts() {
    let invalid_urls = [
        "",
        "   ",
        "http://github.com/owner/repo",
        "ftp://github.com/owner/repo",
        "ssh://git@github.com/owner/repo",
        "git://github.com/owner/repo",
        "git@github.com:owner/repo.git",
        "https://gitlab.com/owner/repo",
        "https://bitbucket.org/owner/repo",
        "https://github.com.evil.com/owner/repo",
        "https://evil-github.com/owner/repo",
        "https://notgithub.com/owner/repo",
        "https://github.com",
        "https://github.com/",
        "https://github.com/onlyowner",
        "https://github.com/onlyowner/",
        "https://github.com/owner/repo/extra/path",
        "https://github.com/owner/repo/pull/123",
        "https://github.com/owner/repo/issues/456",
        "https://github.com/owner/repo/blob/main/hyprland.conf",
    ];

    for url in &invalid_urls {
        let res = parse_and_validate_github_url(url);
        assert!(
            res.is_err(),
            "Expected URL to be rejected, but got Ok: {:?}",
            url
        );
    }
}

/// Adversarial Challenge: Embedded CRLF/newlines should not bypass URL validation.
#[test]
fn test_url_crlf_and_whitespace_injection() {
    let crlf_payloads = [
        "https://github.com/owner/repo\nrm -rf /",
        "https://github.com/owner/repo\r\nwhoami",
        "https://github.com/owner/repo\n--upload-pack",
    ];

    for payload in &crlf_payloads {
        let res = parse_and_validate_github_url(payload);
        // WHATWG Url::parse strips \r and \n, potentially causing silent parsing anomalies
        assert!(
            res.is_err(),
            "URLs with embedded newlines/CRLF MUST be rejected, got Ok: {:?}",
            res
        );
    }
}

/// Adversarial Challenge: Traversal sequences (e.g. `../../etc/passwd`) should be rejected.
#[test]
fn test_url_traversal_detection() {
    let traversal_urls = [
        "https://github.com/../../etc/passwd",
        "https://github.com/owner/repo/../../etc/passwd",
        "https://github.com/../owner/repo",
    ];

    for url in &traversal_urls {
        let res = parse_and_validate_github_url(url);
        assert!(
            res.is_err(),
            "Traversal URL {:?} MUST be rejected, but was accepted: {:?}",
            url,
            res
        );
    }
}

/// Adversarial Challenge: Local and file:// paths must reject git CLI option flags.
#[test]
fn test_file_protocol_and_local_path_flag_injection() {
    let flag_payloads = [
        "file://--upload-pack=evil",
        "file://-u",
        "file:///tmp/--upload-pack=evil",
        "/--upload-pack=evil",
        "/-u",
        "./--upload-pack=evil",
    ];

    for payload in &flag_payloads {
        let res = parse_and_validate_github_url(payload);
        assert!(
            res.is_err(),
            "Local or file:// path starting with '-' or containing flags MUST be rejected, got: {:?}",
            res
        );
    }
}

/// Adversarial Challenge: Malformed ports and credentials should be rejected.
#[test]
fn test_url_ports_and_credentials_rejection() {
    let candidate_urls = [
        "https://user:pass@github.com/owner/repo",
        "https://token@github.com/owner/repo",
        "https://github.com:8080/owner/repo",
        "https://github.com:22/owner/repo",
    ];

    for url in &candidate_urls {
        let res = parse_and_validate_github_url(url);
        assert!(
            res.is_err(),
            "URL with non-standard port or credentials MUST be rejected, got: {:?}",
            res
        );
    }
}

// ============================================================================
// 2. Dotfile Detection Edge Cases
// ============================================================================

#[test]
fn test_dotfile_detection_deeply_nested_beyond_max_depth() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let deep_dir = root.join("a/b/c/d/e/f/g");
    fs::create_dir_all(&deep_dir).unwrap();
    fs::write(deep_dir.join("hyprland.conf"), "monitor=,preferred,auto,1\n").unwrap();

    let err = detect_hyprland_files(root).unwrap_err();
    assert!(
        matches!(err, FetchError::MissingHyprlandConfig),
        "Depth > 6 anchor should be excluded, got: {:?}",
        err
    );
}

#[test]
fn test_dotfile_detection_subconfig_nested_beyond_hypr_depth() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let hypr_dir = root.join("hypr");
    fs::create_dir_all(&hypr_dir).unwrap();
    fs::write(hypr_dir.join("hyprland.conf"), "general { gaps_in = 5 }\n").unwrap();

    let sub2 = hypr_dir.join("sub1/sub2");
    fs::create_dir_all(&sub2).unwrap();
    fs::write(sub2.join("colors.conf"), "$color = #112233\n").unwrap();

    let sub6 = hypr_dir.join("d1/d2/d3/d4/d5/d6");
    fs::create_dir_all(&sub6).unwrap();
    fs::write(sub6.join("deep.conf"), "$deep = #445566\n").unwrap();

    let detected = detect_hyprland_files(root).unwrap();
    assert!(detected.iter().any(|d| d.target_path == "~/.config/hypr/hyprland.conf"));
    assert!(detected.iter().any(|d| d.target_path == "~/.config/hypr/sub1/sub2/colors.conf"));
    assert!(
        !detected.iter().any(|d| d.target_path.contains("deep.conf")),
        "Deep subconfig at depth 6 under hypr_dir should be excluded by max_depth(5)"
    );
}

#[test]
fn test_dotfile_detection_layout_variations_a_b_c_d() {
    // Layout A: .config/hypr/hyprland.conf + .config/waybar/config
    {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let hypr = root.join(".config/hypr");
        let waybar = root.join(".config/waybar");
        fs::create_dir_all(&hypr).unwrap();
        fs::create_dir_all(&waybar).unwrap();
        fs::write(hypr.join("hyprland.conf"), "autogenerated = 0\n").unwrap();
        fs::write(waybar.join("config"), "{\"position\": \"top\"}\n").unwrap();

        let files = detect_hyprland_files(root).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hyprland.conf" && f.is_primary));
        assert!(files.iter().any(|f| f.target_path == "~/.config/waybar/config" && !f.is_primary));
    }

    // Layout B: hypr/hyprland.conf + waybar/config.jsonc
    {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let hypr = root.join("hypr");
        let waybar = root.join("waybar");
        fs::create_dir_all(&hypr).unwrap();
        fs::create_dir_all(&waybar).unwrap();
        fs::write(hypr.join("hyprland.conf"), "autogenerated = 0\n").unwrap();
        fs::write(waybar.join("config.jsonc"), "{\"position\": \"top\"}\n").unwrap();

        let files = detect_hyprland_files(root).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hyprland.conf"));
        assert!(files.iter().any(|f| f.target_path == "~/.config/waybar/config.jsonc"));
    }

    // Layout C: Flat root
    {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("hyprland.conf"), "autogenerated = 0\n").unwrap();
        fs::write(root.join("hypridle.conf"), "general {}\n").unwrap();

        let files = detect_hyprland_files(root).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hyprland.conf"));
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hypridle.conf"));
    }

    // Layout D: Nested dots
    {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let hypr = root.join("dots/.config/hypr");
        let waybar = root.join("dots/.config/waybar");
        fs::create_dir_all(&hypr).unwrap();
        fs::create_dir_all(&waybar).unwrap();
        fs::write(hypr.join("hyprland.conf"), "autogenerated = 0\n").unwrap();
        fs::write(waybar.join("config"), "{\"position\": \"top\"}\n").unwrap();

        let files = detect_hyprland_files(root).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hyprland.conf"));
        assert!(files.iter().any(|f| f.target_path == "~/.config/waybar/config"));
    }
}

#[test]
fn test_dotfile_detection_symlinks_pointing_outside_repository() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let hypr = root.join("hypr");
    fs::create_dir_all(&hypr).unwrap();
    fs::write(hypr.join("hyprland.conf"), "autogenerated = 0\n").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    let outside_file = outside_dir.path().join("secret.conf");
    fs::write(&outside_file, "SECRET_TOKEN=12345\n").unwrap();

    let symlink_path = hypr.join("leaked.conf");
    std::os::unix::fs::symlink(&outside_file, &symlink_path).unwrap();

    let dir_symlink = hypr.join("outside_dir");
    std::os::unix::fs::symlink(outside_dir.path(), &dir_symlink).unwrap();

    let detected = detect_hyprland_files(root).unwrap();

    assert!(
        !detected.iter().any(|d| d.target_path.contains("leaked.conf")),
        "Symlink pointing outside repo MUST be ignored"
    );
    assert!(
        !detected.iter().any(|d| d.target_path.contains("secret.conf")),
        "Target of symlink outside repo MUST NOT be detected"
    );
    assert_eq!(detected.len(), 1, "Only regular hyprland.conf should be detected");
}

#[test]
fn test_dotfile_detection_multiple_hyprland_conf_precedence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let dotconfig = root.join(".config/hypr");
    let hypr = root.join("hypr");
    fs::create_dir_all(&dotconfig).unwrap();
    fs::create_dir_all(&hypr).unwrap();

    fs::write(root.join("hyprland.conf"), "# root flat\n").unwrap();
    fs::write(hypr.join("hyprland.conf"), "# hypr subdir\n").unwrap();
    fs::write(dotconfig.join("hyprland.conf"), "# .config/hypr canonical\n").unwrap();

    let detected = detect_hyprland_files(root).unwrap();
    let primary = detected.iter().find(|d| d.is_primary).expect("Primary must exist");
    assert_eq!(
        primary.relative_path,
        PathBuf::from(".config/hypr/hyprland.conf"),
        "Precedence 0 (.config/hypr/hyprland.conf) must win over other hyprland.conf files"
    );

    let primary_content = fs::read_to_string(root.join(&primary.relative_path)).unwrap();
    assert_eq!(primary_content, "# .config/hypr canonical\n");
}

#[test]
fn test_dotfile_detection_structured_large_file_exclusion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let hypr = root.join("hypr");
    let waybar = root.join("waybar");
    fs::create_dir_all(&hypr).unwrap();
    fs::create_dir_all(&waybar).unwrap();

    fs::write(hypr.join("hyprland.conf"), "autogenerated = 0\n").unwrap();
    fs::write(waybar.join("config"), "{\"layer\": \"top\"}\n").unwrap();

    let large_hypr = hypr.join("large_wallpaper.conf");
    let large_data = vec![b'A'; 6 * 1024 * 1024];
    fs::write(&large_hypr, &large_data).unwrap();

    let large_waybar = waybar.join("large_style.css");
    fs::write(&large_waybar, &large_data).unwrap();

    let detected = detect_hyprland_files(root).unwrap();
    assert!(
        !detected.iter().any(|d| d.target_path.contains("large_wallpaper.conf")),
        "Files > 5MB in hypr dir must be excluded"
    );
    assert!(
        !detected.iter().any(|d| d.target_path.contains("large_style.css")),
        "Files > 5MB in waybar dir must be excluded"
    );
    assert_eq!(detected.len(), 2, "Only legitimate files under 5MB should be detected");
}

/// Adversarial Challenge: Large file (> 5MB) exclusion must apply to flat layout repositories too.
#[test]
fn test_dotfile_detection_flat_layout_large_file_exclusion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(root.join("hyprland.conf"), "autogenerated = 0\n").unwrap();

    // Create a 6MB file in flat root
    let large_flat = root.join("large_config.conf");
    let large_data = vec![b'B'; 6 * 1024 * 1024];
    fs::write(&large_flat, &large_data).unwrap();

    let detected = detect_hyprland_files(root).unwrap();
    let has_large = detected.iter().any(|d| d.target_path.contains("large_config.conf"));
    assert!(
        !has_large,
        "Files > 5MB in flat layout MUST be excluded, but was included: {:?}",
        detected
    );
}

#[test]
fn test_dotfile_detection_unreadable_file_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let hypr = root.join("hypr");
    fs::create_dir_all(&hypr).unwrap();
    fs::write(hypr.join("hyprland.conf"), "autogenerated = 0\n").unwrap();

    let unreadable = hypr.join("unreadable.conf");
    fs::write(&unreadable, "secret\n").unwrap();
    let mut perms = fs::metadata(&unreadable).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&unreadable, perms).unwrap();

    let res = detect_hyprland_files(root);
    // Cleanup permissions so tempdir can be cleanly deleted
    let mut restore_perms = fs::metadata(&unreadable).unwrap().permissions();
    restore_perms.set_mode(0o644);
    let _ = fs::set_permissions(&unreadable, restore_perms);

    assert!(res.is_err(), "Unreadable file should return an error");
    assert!(
        matches!(res.unwrap_err(), FetchError::Io(_)),
        "Expected FetchError::Io error variant"
    );
}

// ============================================================================
// 3. End-to-End Stress Test: Layouts A, B, C, D Clone, Plan, Install, Rollback
// ============================================================================

#[test]
fn test_e2e_stress_all_four_layouts_roundtrip() {
    let test_cases = [
        // Case 1: Layout A (.config/hypr)
        ("layout_a", "1.0.0", vec![
            (".config/hypr/hyprland.conf", "general { gaps_in = 3 }\n$accent = #ff00ff\n"),
            (".config/hypr/hyprlock.conf", "background { color = #000000 }\n"),
            (".config/waybar/config", "{\"layer\": \"top\"}\n"),
            (".config/waybar/style.css", "@define-color fg #ffffff;\n"),
        ]),
        // Case 2: Layout B (hypr/)
        ("layout_b", "2.1.0", vec![
            ("hypr/hyprland.conf", "general { gaps_in = 5 }\n$accent = #00ffff\n"),
            ("hypr/hypridle.conf", "general { lock_cmd = hyprlock }\n"),
            ("waybar/config.jsonc", "{\"layer\": \"bottom\"}\n"),
        ]),
        // Case 3: Layout C (Flat)
        ("layout_c", "0.2.0", vec![
            ("hyprland.conf", "monitor=,preferred,auto,1\n$accent = #ffaa00\n"),
            ("hyprpaper.conf", "preload = ~/bg.png\n"),
        ]),
        // Case 4: Layout D (Nested dots)
        ("layout_d", "3.0.0", vec![
            ("dots/.config/hypr/hyprland.conf", "input { kb_layout = us }\n$accent = #123456\n"),
            ("dots/.config/hypr/hyprpaper.conf", "wallpaper = ,~/wall.jpg\n"),
            ("dots/.config/waybar/config", "{\"position\": \"left\"}\n"),
        ]),
    ];

    for (name, ver, files) in test_cases {
        let repo_dir = tempfile::tempdir().unwrap();
        let repo_path = repo_dir.path();

        init_local_git_repo(repo_path);

        for (rel, content) in &files {
            let full = repo_path.join(rel);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(&full, content).unwrap();
        }
        fs::write(repo_path.join("LICENSE"), "MIT License\n").unwrap();

        commit_all(repo_path, &format!("feat: {name}"), Some(&format!("v{ver}")));

        // 1. Fetch via fetch_github_repository using local path
        let fetched = fetch_github_repository(
            repo_path.to_str().unwrap(),
            FetchOptions::default(),
        )
        .unwrap_or_else(|e| panic!("Fetch failed for {name}: {e}"));

        assert_eq!(fetched.version, ver, "Version mismatch for {name}");
        assert_eq!(fetched.license, "MIT", "License mismatch for {name}");
        assert_eq!(fetched.detected_files.len(), files.len(), "File count mismatch for {name}");
        assert!(fetched.manifest.validate().is_ok(), "Manifest validation failed for {name}");

        // 2. Setup isolated Home and Database for InstallationEngine
        let sim_home = tempfile::tempdir().unwrap();
        let db = Database::in_memory().unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(sim_home.path().join("backups")).unwrap(),
            home: sim_home.path().to_path_buf(),
        };

        // 3. Compute plan
        let plan = engine
            .compute_plan(fetched.temp_dir.path())
            .unwrap_or_else(|e| panic!("compute_plan failed for {name}: {e}"));
        assert_eq!(plan.file_diffs.len(), files.len(), "Diff count mismatch for {name}");

        // 4. Install
        let txn = engine
            .install(fetched.temp_dir.path(), "adversarial_e2e")
            .unwrap_or_else(|e| panic!("install failed for {name}: {e}"));

        // Verify target files exist and have exact matching content
        for dotfile in &fetched.detected_files {
            let target = sim_home.path().join(dotfile.target_path.trim_start_matches("~/"));
            assert!(target.exists(), "Target file {:?} was not created for {name}", target);
            let target_content = fs::read_to_string(&target).unwrap();
            let source_content = fs::read_to_string(fetched.temp_dir.path().join(&dotfile.relative_path)).unwrap();
            assert_eq!(target_content, source_content, "Content mismatch in {name} for {:?}", dotfile.target_path);
        }

        // 5. Rollback
        let rollback_res = engine.rollback(&txn, false);
        assert!(rollback_res.is_ok(), "Rollback failed for {name}: {:?}", rollback_res);

        // Verify target files are completely cleaned up and home is clean
        for dotfile in &fetched.detected_files {
            let target = sim_home.path().join(dotfile.target_path.trim_start_matches("~/"));
            assert!(!target.exists(), "Target file {:?} was not removed after rollback for {name}", target);
        }
    }
}
