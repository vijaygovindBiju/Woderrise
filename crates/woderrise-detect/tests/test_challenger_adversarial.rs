//! Adversarial Stress Test Suite for `woderrise-detect`
//!
//! Evaluates malformed Waybar JSONC resilience, dual Hyprland precedence determinism,
//! hardware monitor protection guarantees, and Git symlink mode 120000 rejection.

use camino::Utf8Path;
use tempfile::tempdir;
use woderrise_core::error::{PathError, WoderriseError};
use woderrise_core::models::{ConfigComponent, ConflictStatus, SyntaxFormat};
use woderrise_core::paths::XdgPaths;
use woderrise_detect::{
    detect_inline_monitor_directives, is_hardware_bound_filename, is_hardware_bound_path,
    validate_waybar_jsonc, DetectionEngine, TreeEntry, HARDWARE_BOUND_FILENAMES,
};

#[test]
fn test_waybar_jsonc_trailing_commas_and_comments() {
    let complex_jsonc = r#"{
        // Top-level comment
        "layer": "top", /* inline comment */
        "position": "bottom",
        "margin-top": 0, // trailing comma on next line
        "modules-left": [
            "hyprland/workspaces",
            "tray",
            /* trailing comma in array */
        ],
        "nested": {
            "key1": "val1",
            "key2": "val2", // trailing comma in object
        },
        "url_test": {
            "format": "https://api.github.com/v1/status", // must preserve slashes
            "escapes": "hello\nworld\t\"quoted\"",
        },
    }"#;

    let res = validate_waybar_jsonc("config.jsonc", complex_jsonc);
    assert!(
        res.valid,
        "Valid JSONC with comments, trailing commas, and URLs must succeed: {:?}",
        res.error_message
    );
}

#[test]
fn test_waybar_jsonc_syntax_error_reporting() {
    // 1. Unterminated string literal
    let bad_string = r#"{ "layer": "top, "position": "top" }"#;
    let res = validate_waybar_jsonc("config.jsonc", bad_string);
    assert!(!res.valid, "Unterminated string must fail validation");
    assert!(res.error_message.is_some());

    // 2. Unterminated block comment
    let bad_comment = r#"{ "layer": "top" /* unterminated comment "#;
    let res = validate_waybar_jsonc("config.jsonc", bad_comment);
    assert!(
        !res.valid,
        "Unterminated block comment must fail validation"
    );
    assert!(res.error_message.is_some());

    // 3. Invalid unquoted token
    let bad_syntax = r#"{ "layer": top }"#;
    let res = validate_waybar_jsonc("config.jsonc", bad_syntax);
    assert!(!res.valid, "Invalid unquoted token must fail validation");
    assert!(res.error_message.is_some());

    // 4. Primitive root (not object or array)
    let primitive_root = r#""plain string root""#;
    let res = validate_waybar_jsonc("config.jsonc", primitive_root);
    assert!(!res.valid, "String primitive root must fail validation");
    assert!(res
        .error_message
        .unwrap()
        .contains("root must be a JSON object or array"));

    // 5. Array of objects (valid multi-bar)
    let multi_bar = r#"[
        { "name": "top-bar", "position": "top", },
        { "name": "bottom-bar", "position": "bottom", },
    ]"#;
    let res = validate_waybar_jsonc("config.jsonc", multi_bar);
    assert!(res.valid, "Array of bar objects must be valid");
}

#[test]
fn test_dual_hyprland_precedence_determinism() {
    let engine = DetectionEngine::new();

    // Scenario A: lua candidate comes BEFORE conf candidate in slice
    let tree_lua_first = vec![
        TreeEntry::new(
            ".config/hypr/hyprland.lua",
            "100644",
            "blob",
            "lua_sha",
            Some(2000),
        ),
        TreeEntry::new(
            ".config/hypr/hyprland.conf",
            "100644",
            "blob",
            "conf_sha",
            Some(1500),
        ),
        TreeEntry::new(
            ".config/hypr/binds.conf",
            "100644",
            "blob",
            "b_sha",
            Some(500),
        ),
    ];

    let det_a = engine
        .detect_component(&tree_lua_first, ConfigComponent::Hyprland)
        .expect("Hyprland must be detected");
    assert_eq!(
        det_a.syntax_format,
        SyntaxFormat::LegacyConf,
        "Legacy conf must take precedence even when lua appears first in tree"
    );
    assert_eq!(det_a.entrypoint, ".config/hypr/hyprland.conf");

    // Scenario B: conf candidate comes BEFORE lua candidate in slice
    let tree_conf_first = vec![
        TreeEntry::new(
            ".config/hypr/hyprland.conf",
            "100644",
            "blob",
            "conf_sha",
            Some(1500),
        ),
        TreeEntry::new(
            ".config/hypr/hyprland.lua",
            "100644",
            "blob",
            "lua_sha",
            Some(2000),
        ),
    ];

    let det_b = engine
        .detect_component(&tree_conf_first, ConfigComponent::Hyprland)
        .expect("Hyprland must be detected");
    assert_eq!(
        det_b.syntax_format,
        SyntaxFormat::LegacyConf,
        "Legacy conf must take precedence when conf appears first"
    );
    assert_eq!(det_b.entrypoint, ".config/hypr/hyprland.conf");

    // Scenario C: Only lua exists -> Lua format must be selected
    let tree_lua_only = vec![TreeEntry::new(
        ".config/hypr/hyprland.lua",
        "100644",
        "blob",
        "lua_sha",
        Some(2000),
    )];
    let det_c = engine
        .detect_component(&tree_lua_only, ConfigComponent::Hyprland)
        .expect("Hyprland must be detected");
    assert_eq!(det_c.syntax_format, SyntaxFormat::Lua);
    assert_eq!(det_c.entrypoint, ".config/hypr/hyprland.lua");
}

#[test]
fn test_hardware_monitor_protection_exhaustive() {
    let temp = tempdir().unwrap();
    let base_path = Utf8Path::from_path(temp.path()).unwrap();
    let xdg = XdgPaths::custom(base_path);
    let engine = DetectionEngine::new();

    // Verify all defined hardware-bound filenames
    for fname in HARDWARE_BOUND_FILENAMES {
        assert!(
            is_hardware_bound_filename(fname),
            "{} must be recognized by is_hardware_bound_filename",
            fname
        );
        let path = format!(".config/hypr/{}", fname);
        assert!(
            is_hardware_bound_path(&path),
            "{} must be recognized by is_hardware_bound_path",
            path
        );
    }

    let tree = vec![
        TreeEntry::new(
            ".config/hypr/hyprland.conf",
            "100644",
            "blob",
            "hl_sha",
            Some(1000),
        ),
        TreeEntry::new(
            ".config/hypr/monitors.conf",
            "100644",
            "blob",
            "mon_sha",
            Some(400),
        ),
        TreeEntry::new(
            ".config/hypr/workspaces.conf",
            "100644",
            "blob",
            "ws_sha",
            Some(400),
        ),
        TreeEntry::new(
            ".config/hypr/hardware.conf",
            "100644",
            "blob",
            "hw_sha",
            Some(300),
        ),
        TreeEntry::new(
            ".config/hypr/general.conf",
            "100644",
            "blob",
            "gen_sha",
            Some(500),
        ),
    ];

    let (mappings, notices) = engine
        .generate_target_mappings(&tree, ConfigComponent::Hyprland, &xdg)
        .expect("Target mappings must generate successfully");

    // monitors.conf, workspaces.conf, and hardware.conf must ALL be protected
    let protected_files: Vec<&str> = mappings
        .iter()
        .filter(|m| m.is_hardware_protected)
        .map(|m| m.relative_path.file_name().unwrap())
        .collect();

    assert!(protected_files.contains(&"monitors.conf"));
    assert!(protected_files.contains(&"workspaces.conf"));
    assert!(protected_files.contains(&"hardware.conf"));
    assert!(!protected_files.contains(&"general.conf"));
    assert!(!protected_files.contains(&"hyprland.conf"));

    // Check ConflictStatus::HardwareProtected
    for m in mappings.iter().filter(|m| m.is_hardware_protected) {
        assert_eq!(m.conflict_status, ConflictStatus::HardwareProtected);
    }

    // Verify notices generated with preserved_by_default = true
    assert_eq!(notices.len(), 3);
    for n in &notices {
        assert!(n.preserved_by_default);
    }

    // Verify inline monitor detection
    let inline_conf = r#"
    # Commented monitor line:
    # monitor = eDP-1, 1920x1080@60, 0x0, 1
    autogenerated = 0
    monitor = DP-2, 3840x2160@144, 0x0, 1.5 # 4K Display
    monitor = HDMI-A-1, 1920x1080@60, 2560x0, 1
    "#;
    let inline_notices = detect_inline_monitor_directives("hyprland.conf", inline_conf);
    assert_eq!(
        inline_notices.len(),
        2,
        "Only uncommented monitor directives must be detected"
    );
    assert!(inline_notices[0].description.contains("DP-2"));
    assert!(inline_notices[1].description.contains("HDMI-A-1"));
    assert!(inline_notices[0].preserved_by_default);
}

#[test]
fn test_symlink_rejection_mode_120000() {
    let temp = tempdir().unwrap();
    let base_path = Utf8Path::from_path(temp.path()).unwrap();
    let xdg = XdgPaths::custom(base_path);
    let engine = DetectionEngine::new();

    // Adversarial tree with symlink entry disguised as a normal config
    let adversarial_tree = vec![
        TreeEntry::new(
            ".config/waybar/config.jsonc",
            "100644",
            "blob",
            "wb_sha",
            Some(1000),
        ),
        TreeEntry::new(
            ".config/waybar/style.css",
            "120000", // Git symlink mode
            "blob",
            "symlink_target_sha",
            Some(40),
        ),
    ];

    let err = engine
        .generate_target_mappings(&adversarial_tree, ConfigComponent::Waybar, &xdg)
        .expect_err("Tree with mode 120000 symlink must be rejected unconditionally");

    match err {
        WoderriseError::Path(PathError::ForbiddenSymlink { path, target }) => {
            assert!(path.as_str().contains("style.css"));
            assert_eq!(target.as_str(), "<git-symlink-traversal>");
        }
        other => panic!("Expected ForbiddenSymlink error, got {:?}", other),
    }

    // Ensure tree with regular files (100644) and executables (100755) is NOT rejected
    let clean_tree = vec![
        TreeEntry::new(
            ".config/waybar/config.jsonc",
            "100644",
            "blob",
            "wb_sha",
            Some(1000),
        ),
        TreeEntry::new(
            ".config/waybar/style.css",
            "100644",
            "blob",
            "css_sha",
            Some(500),
        ),
    ];

    let clean_res = engine.generate_target_mappings(&clean_tree, ConfigComponent::Waybar, &xdg);
    assert!(clean_res.is_ok(), "Clean tree must succeed");
}
