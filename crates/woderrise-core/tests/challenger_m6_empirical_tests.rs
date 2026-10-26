//! Empirical Adversarial Challenge Suite for Milestone M6
//! Rigorous stress testing of `com.catppuccin.hyprland`:
//! 1. Corrupt payload & SHA-256 tampering detection matrix
//! 2. Target path traversal & injection defense
//! 3. Palette hex validation & malformed previews matrix
//! 4. FTS5 query injection & adversarial search stress
//! 5. Rollback conflict detection & 2-pass transactional safety
//! 6. Cross-package overwrite & rollback preservation
//! 7. Manifest schema & privilege escalation defense

use std::{
    fs,
    path::{Path, PathBuf},
};
use woderrise_core::{
    catalog, diff::FileDiffAction, expand_target, sha256_bytes, BackupManager,
    Database, InstallationEngine, Manifest, WoderriseError,
};

fn registry_packages_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages")
}

fn hyprland_pkg_dir() -> PathBuf {
    registry_packages_dir().join("com.catppuccin.hyprland/1.0.0")
}

fn create_temp_env(name: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join(format!("home_{name}"));
    let backups = tmp.path().join(format!("backups_{name}"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&backups).unwrap();
    (tmp, home, backups)
}

fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name())).unwrap();
        }
    }
}

// ============================================================================
// 1. Corrupt Payload & SHA-256 Tampering Detection Matrix
// ============================================================================

#[test]
fn test_hyprland_payload_tampering_matrix() {
    let original_pkg = hyprland_pkg_dir();
    let manifest_path = original_pkg.join("woderrise.yaml");
    let manifest = Manifest::from_file(&manifest_path).expect("parse original manifest");

    // Verify baseline passes
    catalog::verify_artifacts(&manifest, &original_pkg)
        .expect("genuine package must pass artifact verification");

    let tmp = tempfile::tempdir().expect("tempdir");
    let test_pkg = tmp.path().join("pkg");

    // Test tampering on each of the 9 artifacts individually
    for (idx, artifact) in manifest.artifacts.iter().enumerate() {
        // Copy clean package to test dir
        if test_pkg.exists() {
            fs::remove_dir_all(&test_pkg).unwrap();
        }
        copy_dir_all(&original_pkg, &test_pkg);

        let target_artifact_path = test_pkg.join(&artifact.path);
        assert!(
            target_artifact_path.is_file(),
            "Artifact file {} must exist",
            artifact.path
        );

        // Sub-scenario A: Tamper by appending malicious payload
        let mut content = fs::read(&target_artifact_path).unwrap();
        content.extend_from_slice(b"\n# ADVERSARIAL INJECTION PAYLOAD\nexec = rm -rf ~\n");
        fs::write(&target_artifact_path, &content).unwrap();

        let tampered_hash = sha256_bytes(&content);
        assert_ne!(
            tampered_hash, artifact.sha256,
            "Tampered content must produce distinct hash"
        );

        // Verify catalog::verify_artifacts catches the tampering
        let err = catalog::verify_artifacts(&manifest, &test_pkg)
            .expect_err(&format!("artifact {} tampering must be detected", artifact.path));
        match err {
            WoderriseError::Checksum {
                path,
                expected,
                actual,
            } => {
                assert_eq!(path, artifact.path);
                assert_eq!(expected, artifact.sha256);
                assert_eq!(actual, tampered_hash);
            }
            other => panic!(
                "Expected Checksum error for artifact {}, got {:?}",
                artifact.path, other
            ),
        }

        // Verify InstallationEngine refuses to install the tampered package
        let (_env_tmp, home, backups) = create_temp_env(&format!("tamper_{idx}"));
        let db = Database::in_memory().unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(&backups).unwrap(),
            home: home.clone(),
        };

        let install_err = engine
            .install(&test_pkg, "adversarial-tester")
            .expect_err("install must abort on tampered artifact");
        match install_err {
            WoderriseError::Checksum { path, .. } => {
                assert_eq!(path, artifact.path);
            }
            other => panic!(
                "Expected Checksum error on install for {}, got {:?}",
                artifact.path, other
            ),
        }

        // Verify no files were installed in user home
        assert!(
            !home.join(".config/hypr/hyprland.conf").exists(),
            "Installation must abort cleanly without writing to target home"
        );

        // Sub-scenario B: Truncate artifact to 0 bytes
        fs::write(&target_artifact_path, b"").unwrap();
        let zero_hash = sha256_bytes(b"");
        let err_trunc = catalog::verify_artifacts(&manifest, &test_pkg)
            .expect_err("Truncated artifact must fail");
        match err_trunc {
            WoderriseError::Checksum {
                path,
                expected,
                actual,
            } => {
                assert_eq!(path, artifact.path);
                assert_eq!(expected, artifact.sha256);
                assert_eq!(actual, zero_hash);
            }
            other => panic!("Expected Checksum error on truncated file, got {:?}", other),
        }

        // Sub-scenario C: Delete artifact file entirely
        fs::remove_file(&target_artifact_path).unwrap();
        let err_del = catalog::verify_artifacts(&manifest, &test_pkg)
            .expect_err("Missing artifact must fail verification");
        assert!(
            matches!(err_del, WoderriseError::Io(_) | WoderriseError::Registry(_)),
            "Expected Io or Registry error on missing file, got {:?}",
            err_del
        );
    }
}

#[test]
fn test_hyprland_manifest_hash_tampering() {
    let original_pkg = hyprland_pkg_dir();
    let manifest_path = original_pkg.join("woderrise.yaml");
    let mut manifest = Manifest::from_file(&manifest_path).unwrap();

    // Tamper with sha256 in manifest definition
    manifest.artifacts[0].sha256 =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();

    let err = catalog::verify_artifacts(&manifest, &original_pkg)
        .expect_err("tampered manifest hash must fail verification");
    assert!(matches!(err, WoderriseError::Checksum { .. }));
}

// ============================================================================
// 2. Target Path Traversal & Injection Defense
// ============================================================================

#[test]
fn test_hyprland_path_traversal_rejection() {
    let original_pkg = hyprland_pkg_dir();
    let manifest_yaml = fs::read_to_string(original_pkg.join("woderrise.yaml")).unwrap();

    // Adversarial traversal vectors targeting sensitive files
    let dangerous_targets = [
        "~/.config/hypr/../../../../etc/shadow",
        "~/.config/hypr/../../etc/passwd",
        "~/.config/../.bashrc",
        "~/../../../root/.ssh/authorized_keys",
        "~/.config/hypr/../../../tmp/evil",
        "~/.config/hypr/..",
    ];

    for dangerous in &dangerous_targets {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.install.files[0].target = dangerous.to_string();
        let err = manifest
            .validate()
            .expect_err(&format!("Target traversal '{dangerous}' must be rejected"));
        assert!(
            matches!(err, WoderriseError::Validation(_)),
            "Expected Validation error for '{}', got {:?}",
            dangerous,
            err
        );
    }

    // Dangerous artifact paths
    let dangerous_artifact_paths = [
        "../../outside_artifact.conf",
        "artifacts/../../../etc/shadow",
        "/etc/shadow",
        "artifacts/../../secrets.txt",
    ];

    for dangerous in &dangerous_artifact_paths {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.artifacts[0].path = dangerous.to_string();
        let err = manifest
            .validate()
            .expect_err(&format!("Artifact path traversal '{dangerous}' must be rejected"));
        assert!(
            matches!(err, WoderriseError::Validation(_)),
            "Expected Validation error for '{}', got {:?}",
            dangerous,
            err
        );
    }

    // Dangerous install source paths
    for dangerous in &dangerous_artifact_paths {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.install.files[0].source = dangerous.to_string();
        let err = manifest.validate().expect_err(&format!(
            "Install source traversal '{dangerous}' must be rejected"
        ));
        assert!(
            matches!(err, WoderriseError::Validation(_)),
            "Expected Validation error for '{}', got {:?}",
            dangerous,
            err
        );
    }

    // Dangerous preview thumbnail path
    let dangerous_thumbnails = [
        "../../outside_thumb.png",
        "/etc/passwd",
        "artifacts/../../../root/.ssh/id_rsa",
    ];
    for dangerous in &dangerous_thumbnails {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.previews.as_mut().unwrap().thumbnail = dangerous.to_string();
        let err = manifest.validate().expect_err(&format!(
            "Preview thumbnail traversal '{dangerous}' must be rejected"
        ));
        assert!(
            matches!(err, WoderriseError::Validation(_)),
            "Expected Validation error for '{}', got {:?}",
            dangerous,
            err
        );
    }

    // Expand target traversal defense verification
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    let traversal_expansions = [
        "~/.config/hypr/../../etc/shadow",
        "~/../etc/passwd",
        "/etc/sudoers",
        "../relative",
        "~/.config/../../../",
    ];
    for t in &traversal_expansions {
        let err = expand_target(t, &home).expect_err(&format!("expand_target must reject '{t}'"));
        assert!(
            matches!(err, WoderriseError::UnsafePath(_)),
            "Expected UnsafePath for '{t}', got {:?}",
            err
        );
    }
}

// ============================================================================
// 3. Palette Hex Validation & Malformed Previews Matrix
// ============================================================================

#[test]
fn test_hyprland_palette_hex_validation_matrix() {
    let original_pkg = hyprland_pkg_dir();
    let manifest_yaml = fs::read_to_string(original_pkg.join("woderrise.yaml")).unwrap();

    // Adversarial / invalid hex color values
    let invalid_hex_cases = [
        ("1e1e2e", "missing hash prefix"),
        ("#1e1e2g", "non-hex character g"),
        ("#zzzzzz", "non-hex characters"),
        ("#12345!", "symbol character"),
        ("#1e1e2", "too short (5 chars after #)"),
        ("#fff", "3-char shorthand not permitted"),
        ("#1e1e2eee", "8-char RGBA hex"),
        ("#1e1e2e9999", "too long hex"),
        ("#", "hash only"),
        ("", "empty string"),
        ("# 1e1e2", "embedded space"),
        (" #1e1e2e", "leading space"),
        ("#1e1e2e ", "trailing space"),
        ("rgb(30,30,46)", "CSS rgb function"),
        ("rgba(30,30,46,1)", "CSS rgba function"),
        ("hsl(240, 21%, 15%)", "CSS hsl function"),
        ("catppuccin-mocha", "color name string"),
    ];

    for (bad_hex, reason) in &invalid_hex_cases {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.previews.as_mut().unwrap().palette[0].hex = bad_hex.to_string();
        let err = manifest.validate().expect_err(&format!(
            "Invalid hex '{bad_hex}' ({reason}) must fail validation"
        ));
        assert!(
            matches!(err, WoderriseError::Validation(_)),
            "Expected Validation error for '{}' ({}), got {:?}",
            bad_hex,
            reason,
            err
        );
    }

    // Valid hex cases (both lowercase and uppercase standard #RRGGBB)
    let valid_hex_cases = [
        "#1e1e2e", "#cba6f7", "#89b4fa", "#cdd6f4", "#24273a", "#c6a0f6", "#303446", "#ca9ee6",
        "#eff1f5", "#8839ef", "#CBA6F7", "#89B4FA", "#CDD6F4", "#FFFFFF", "#000000",
    ];
    for good_hex in &valid_hex_cases {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.previews.as_mut().unwrap().palette[0].hex = good_hex.to_string();
        if let Err(e) = manifest.validate() {
            panic!("Valid hex '{good_hex}' must pass validation: {e}");
        }
    }
}

// ============================================================================
// 4. FTS5 Query Injection & Adversarial Search Stress
// ============================================================================

#[test]
fn test_hyprland_fts5_query_injection_and_fuzzing() {
    let reg_dir = registry_packages_dir();
    let db = Database::in_memory().expect("in-memory db");
    catalog::migrate(&db).expect("catalog migration");
    let count = catalog::import_registry(&db, &reg_dir).expect("registry import");
    assert!(count >= 2, "Must import at least 2 registry packages");

    // Comprehensive matrix of adversarial, malformed, and hostile FTS5 queries
    let adversarial_queries = [
        // SQL Injection vectors
        "' OR 1=1 --",
        "'; DROP TABLE catalog; --",
        "'; DROP TABLE catalog_fts; --",
        "' UNION SELECT id, version FROM catalog --",
        "\" OR \"\"=\"",
        "admin' --",
        "' OR '1'='1",
        "1; ATTACH DATABASE '/tmp/pwn.db' AS pwn; --",
        // FTS5 Special Syntax Exploitation
        "\"",
        "\"\"",
        "\"\"\"",
        "\"\"\"\"",
        "\"hyprland",
        "hyprland\"",
        "\"hyprland\"",
        "\"com.catppuccin.hyprland\"",
        "*",
        "**",
        "***",
        "^",
        "^^^",
        "NEAR",
        "NEAR/2",
        "NEAR/0(hyprland, mocha)",
        "AND",
        "OR",
        "NOT",
        "AND AND AND",
        "OR OR OR",
        "NOT NOT NOT",
        "name:hyprland",
        "description:mocha",
        "category:desktop_setups",
        "unknown_column:foo",
        "tags:mocha",
        // Punctuation and control symbols
        "!@#$%^&*()_+-=[]{}|;':,./<>?",
        "hyprland; ls -la",
        "hyprland | rm -rf",
        "`whoami`",
        "$(id)",
        // Unicode, zero-width, non-ASCII, emoji
        "hyprland \u{200B}\u{200C}\u{FEFF}",
        "🦀 hyprland 🚀",
        " hyprland \t\n\r ",
        "hyprland\0null",
        "",
        "   \t\n  ",
    ];

    for q in &adversarial_queries {
        let result = catalog::search(&db, q, 10);
        // Search must not panic; if SQLite rejects malformed input (e.g. embedded null), it returns structured error
        if q.contains('\0') {
            assert!(
                result.is_ok() || matches!(result, Err(WoderriseError::Database(_))),
                "Null byte query should return Ok or structured Database error: {:?}",
                result
            );
        } else {
            assert!(
                result.is_ok(),
                "catalog::search must return Ok on adversarial query {:?}: {:?}",
                q,
                result.err()
            );
        }
    }

    // Extreme query length test (10,000 characters repeating tokens)
    let long_query = "hyprland ".repeat(1000);
    let long_res = catalog::search(&db, &long_query, 10);
    assert!(long_res.is_ok(), "Long query must be handled without crash");
    assert!(
        long_res
            .unwrap()
            .iter()
            .any(|e| e.id == "com.catppuccin.hyprland"),
        "Repeating hyprland term should still match package"
    );

    // Verify database integrity is 100% intact after injection barrage
    let integrity: String = db
        .conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .expect("PRAGMA integrity_check");
    assert_eq!(integrity, "ok", "Database must remain intact");

    // Verify legitimate search queries reliably find com.catppuccin.hyprland
    let legitimate_keywords = [
        "hyprland",
        "catppuccin",
        "mocha",
        "macchiato",
        "frappe",
        "latte",
        "hyprlock",
        "hypridle",
        "desktop_setups",
        "wayland",
    ];
    for kw in &legitimate_keywords {
        let results = catalog::search(&db, kw, 10).unwrap();
        assert!(
            results.iter().any(|e| e.id == "com.catppuccin.hyprland"),
            "Search for legitimate keyword '{}' must find com.catppuccin.hyprland",
            kw
        );
    }
}

// ============================================================================
// 5. Rollback Conflict Detection & 2-Pass Transaction Safety
// ============================================================================

#[test]
fn test_hyprland_rollback_conflict_detection_and_resolution() {
    let pkg_dir = hyprland_pkg_dir();
    let (_tmp, home, backups) = create_temp_env("conflict-test");
    let db = Database::in_memory().unwrap();

    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(&backups).unwrap(),
        home: home.clone(),
    };

    // Clean install
    let txn = engine
        .install(&pkg_dir, "challenger-conflict")
        .expect("initial installation must succeed");

    let hypr_conf_path = home.join(".config/hypr/hyprland.conf");
    assert!(hypr_conf_path.is_file());

    // Conflict 1: Post-install target modification
    let user_custom_edit = b"# User edited custom hyprland configuration\nbind = SUPER, Q, killactive\n";
    fs::write(&hypr_conf_path, user_custom_edit).unwrap();

    // Rollback without force must detect conflict and ABORT
    let conflict_err = engine
        .rollback(&txn, false)
        .expect_err("Rollback without force must detect modified file conflict");
    match conflict_err {
        WoderriseError::Conflict(path) => {
            assert!(
                path.contains("hyprland.conf"),
                "Conflict must report hyprland.conf path: {}",
                path
            );
        }
        other => panic!("Expected Conflict error, got {:?}", other),
    }

    // Verify the user's custom edit was NOT wiped out by the aborted rollback
    assert_eq!(
        fs::read(&hypr_conf_path).unwrap(),
        user_custom_edit,
        "User file must remain intact after conflict rejection"
    );

    // Rollback with force = true must succeed and purge/restore state
    let forced_txn = engine
        .rollback(&txn, true)
        .expect("Forced rollback must succeed");
    assert!(!forced_txn.is_empty());

    // Since hyprland.conf did not exist before install, rollback removes it
    assert!(
        !hypr_conf_path.exists(),
        "Created file must be removed on rollback"
    );

    // Conflict 2: Double Rollback Defense
    let double_err = engine
        .rollback(&txn, false)
        .expect_err("Calling rollback twice must fail");
    assert!(
        matches!(double_err, WoderriseError::Other(_)),
        "Expected status error on already rolled-back transaction, got {:?}",
        double_err
    );

    // Conflict 3: Non-existent transaction rollback
    let nonexistent_err = engine
        .rollback("TXN-NONEXISTENT-9999", false)
        .expect_err("Nonexistent txn rollback must fail");
    assert!(
        matches!(nonexistent_err, WoderriseError::Other(_)),
        "Expected not found error, got {:?}",
        nonexistent_err
    );
}

#[test]
fn test_hyprland_rollback_conflict_on_missing_target() {
    let pkg_dir = hyprland_pkg_dir();
    let (_tmp, home, backups) = create_temp_env("conflict-missing");
    let db = Database::in_memory().unwrap();

    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(&backups).unwrap(),
        home: home.clone(),
    };

    let txn = engine
        .install(&pkg_dir, "challenger-missing-file")
        .expect("install");

    // User or third party deleted one of the installed files
    let mocha_path = home.join(".config/hypr/mocha.conf");
    fs::remove_file(&mocha_path).unwrap();

    // Rollback without force must detect conflict because post_sha256 does not match missing file
    let conflict_err = engine
        .rollback(&txn, false)
        .expect_err("Rollback must detect missing file as conflict");
    assert!(matches!(conflict_err, WoderriseError::Conflict(_)));

    // Forced rollback must override and succeed
    let forced = engine.rollback(&txn, true).expect("Forced rollback");
    assert!(!forced.is_empty());
}

// ============================================================================
// 6. Cross-Package Overwrite & Rollback Preservation
// ============================================================================

#[test]
fn test_hyprland_cross_package_overwrite_and_rollback() {
    let pkg_dir = hyprland_pkg_dir();
    let (_tmp, home, backups) = create_temp_env("cross-pkg");
    let db = Database::in_memory().unwrap();

    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(&backups).unwrap(),
        home: home.clone(),
    };

    // Pre-populate home with pre-existing Waybar files (simulating installed Waybar package or custom setup)
    let pre_waybar_config_path = home.join(".config/waybar/config.jsonc");
    let pre_style_path = home.join(".config/waybar/style.css");
    fs::create_dir_all(pre_waybar_config_path.parent().unwrap()).unwrap();

    let pre_config_bytes = b"// Existing Waybar Config\n{\"layer\": \"top\"}\n";
    let pre_style_bytes = b"/* Existing Waybar Style */\n#waybar { background: #000; }\n";
    fs::write(&pre_waybar_config_path, pre_config_bytes).unwrap();
    fs::write(&pre_style_path, pre_style_bytes).unwrap();

    let pre_config_hash = sha256_bytes(pre_config_bytes);
    let pre_style_hash = sha256_bytes(pre_style_bytes);

    // Compute install plan for com.catppuccin.hyprland
    let plan = engine.compute_plan(&pkg_dir).expect("compute plan");
    assert_eq!(plan.package_id, "com.catppuccin.hyprland");
    assert_eq!(plan.file_diffs.len(), 9);
    assert_eq!(plan.will_backup_count, 2);

    let modify_count = plan
        .file_diffs
        .iter()
        .filter(|d| d.action == FileDiffAction::Modify)
        .count();
    let create_count = plan
        .file_diffs
        .iter()
        .filter(|d| d.action == FileDiffAction::Create)
        .count();
    assert_eq!(modify_count, 2, "Must plan modification for 2 waybar files");
    assert_eq!(create_count, 7, "Must plan creation for 7 hyprland files");

    // Execute install
    let txn = engine
        .install(&pkg_dir, "challenger-cross-pkg")
        .expect("install");

    // Verify all 9 files are now installed
    assert_ne!(
        fs::read(&pre_waybar_config_path).unwrap(),
        pre_config_bytes
    );
    assert_ne!(fs::read(&pre_style_path).unwrap(), pre_style_bytes);
    assert!(home.join(".config/hypr/hyprland.conf").is_file());
    assert!(home.join(".config/hypr/hyprlock.conf").is_file());

    // Verify backups were created with exact pre-existing hashes
    let backup_cfg = backups.join(&pre_config_hash);
    let backup_stl = backups.join(&pre_style_hash);
    assert!(backup_cfg.is_file(), "Waybar config backup must exist");
    assert!(backup_stl.is_file(), "Waybar style backup must exist");
    assert_eq!(fs::read(&backup_cfg).unwrap(), pre_config_bytes);
    assert_eq!(fs::read(&backup_stl).unwrap(), pre_style_bytes);

    // Rollback transaction cleanly
    let rb_txn = engine.rollback(&txn, false).expect("rollback");
    assert!(!rb_txn.is_empty());

    // Pre-existing Waybar files must be 100% restored bit-for-bit
    assert_eq!(
        fs::read(&pre_waybar_config_path).unwrap(),
        pre_config_bytes,
        "Waybar config must be completely restored"
    );
    assert_eq!(
        fs::read(&pre_style_path).unwrap(),
        pre_style_bytes,
        "Waybar style must be completely restored"
    );

    // The 7 hyprland files created during install must be completely removed
    assert!(
        !home.join(".config/hypr/hyprland.conf").exists(),
        "hyprland.conf must be removed"
    );
    assert!(
        !home.join(".config/hypr/hyprlock.conf").exists(),
        "hyprlock.conf must be removed"
    );
    assert!(
        !home.join(".config/hypr/hypridle.conf").exists(),
        "hypridle.conf must be removed"
    );
    assert!(
        !home.join(".config/hypr/mocha.conf").exists(),
        "mocha.conf must be removed"
    );
}

// ============================================================================
// 7. Manifest Schema & Privilege Escalation Defense
// ============================================================================

#[test]
fn test_hyprland_schema_and_privilege_escalation_defense() {
    let original_pkg = hyprland_pkg_dir();
    let manifest_yaml = fs::read_to_string(original_pkg.join("woderrise.yaml")).unwrap();

    // Privilege escalation attempts via dangerous file permissions
    let forbidden_modes = [
        "04755", // setuid executable
        "02755", // setgid executable
        "06755", // setuid + setgid
        "4755",  // setuid without leading zero
        "2755",  // setgid without leading zero
    ];
    for mode in &forbidden_modes {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.install.files[0].mode = Some(mode.to_string());
        let err = manifest
            .validate()
            .expect_err(&format!("Forbidden mode '{mode}' must be rejected"));
        assert!(
            matches!(err, WoderriseError::Validation(_)),
            "Expected Validation error for mode '{}', got {:?}",
            mode,
            err
        );
    }

    // Invalid mode strings
    let invalid_modes = ["0999", "invalid", "root", "0888", "07778", "-0644"];
    for mode in &invalid_modes {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.install.files[0].mode = Some(mode.to_string());
        let err = manifest
            .validate()
            .expect_err(&format!("Invalid mode '{mode}' must fail validation"));
        assert!(matches!(err, WoderriseError::Validation(_)));
    }

    // Insecure artifact URLs (must enforce HTTPS)
    let insecure_urls = [
        "http://raw.githubusercontent.com/catppuccin/hyprland/main/example/hyprland.conf",
        "ftp://downloads.catppuccin.org/hyprland.conf",
        "file:///etc/shadow",
    ];
    for url in &insecure_urls {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.artifacts[0].url = Some(url.to_string());
        let err = manifest
            .validate()
            .expect_err(&format!("Insecure URL '{url}' must be rejected"));
        assert!(
            matches!(err, WoderriseError::Validation(_)),
            "Expected Validation error for URL '{}', got {:?}",
            url,
            err
        );
    }

    // Invalid reverse-DNS package IDs
    let invalid_ids = [
        "hyprland",                 // no dot
        "com.Catppuccin.Hyprland",  // uppercase letters
        "com.catppuccin.hyprland;", // semicolon
        "com catppuccin",           // space
        "com/catppuccin",           // slash
        "",                         // empty
    ];
    for bad_id in &invalid_ids {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.package.id = bad_id.to_string();
        let err = manifest
            .validate()
            .expect_err(&format!("Invalid package id '{bad_id}' must be rejected"));
        assert!(matches!(err, WoderriseError::Validation(_)));
    }

    // Unsupported schema versions
    let invalid_schemas = ["2.0.0", "0.9.0", "1.0", "1", ""];
    for schema in &invalid_schemas {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.schema_version = schema.to_string();
        let err = manifest
            .validate()
            .expect_err(&format!("Schema version '{schema}' must be rejected"));
        assert!(matches!(err, WoderriseError::Validation(_)));
    }

    // Invalid semver strings
    let invalid_versions = ["1.0", "v1.0.0", "1.0.0.0", "latest", "alpha", ""];
    for ver in &invalid_versions {
        let mut manifest = Manifest::parse(&manifest_yaml).unwrap();
        manifest.package.version = ver.to_string();
        let err = manifest
            .validate()
            .expect_err(&format!("Invalid semver '{ver}' must be rejected"));
        assert!(matches!(err, WoderriseError::Validation(_)));
    }
}
