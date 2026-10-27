//! Unit and integration test suite for `woderrise-detect`.

use camino::Utf8Path;
use tempfile::tempdir;
use woderrise_core::error::{PathError, WoderriseError};
use woderrise_core::models::{ConfigComponent, ConflictStatus, SyntaxFormat};
use woderrise_core::paths::XdgPaths;
use woderrise_detect::{
    detect_inline_monitor_directives, extract_hypridle_directives, extract_hyprland_directives,
    extract_waybar_directives, remap_hyprpaper_paths, validate_gtk_css, validate_hypridle_conf,
    validate_hyprland_conf, validate_waybar_jsonc, DetectionEngine, TreeEntry,
};

fn sample_classic_hyprland_tree() -> Vec<TreeEntry> {
    vec![
        TreeEntry::new(
            ".config/hypr/hyprland.conf",
            "100644",
            "blob",
            "hl_sha",
            Some(2048),
        ),
        TreeEntry::new(
            ".config/hypr/monitors.conf",
            "100644",
            "blob",
            "mon_sha",
            Some(512),
        ),
        TreeEntry::new(
            ".config/hypr/workspaces.conf",
            "100644",
            "blob",
            "ws_sha",
            Some(512),
        ),
        TreeEntry::new(
            ".config/hypr/keybinds.conf",
            "100644",
            "blob",
            "bind_sha",
            Some(1024),
        ),
        TreeEntry::new(
            ".config/hypr/animations.conf",
            "100644",
            "blob",
            "anim_sha",
            Some(512),
        ),
    ]
}

#[test]
fn test_detect_hyprland_classic() {
    let engine = DetectionEngine::new();
    let tree = sample_classic_hyprland_tree();

    let detection = engine
        .detect_component(&tree, ConfigComponent::Hyprland)
        .expect("Hyprland component must be detected");

    assert_eq!(detection.component, ConfigComponent::Hyprland);
    assert_eq!(detection.syntax_format, SyntaxFormat::LegacyConf);
    assert_eq!(detection.entrypoint, ".config/hypr/hyprland.conf");
    assert_eq!(detection.root_path, ".config/hypr");
    assert_eq!(detection.file_count, 5);

    // Verify hardware preservation notice generated for monitors.conf and workspaces.conf
    assert_eq!(detection.hardware_notices.len(), 2);
    assert!(detection
        .hardware_notices
        .iter()
        .any(|n| n.file_path.as_str().contains("monitors.conf")));
    assert!(detection
        .hardware_notices
        .iter()
        .any(|n| n.file_path.as_str().contains("workspaces.conf")));
}

#[test]
fn test_detect_waybar_json5_parsing() {
    let jsonc_content = r#"{
        // Top-level status bar configuration
        "layer": "top",
        "position": "top",
        /* Module positioning */
        "modules-left": [
            "hyprland/workspaces",
            "clock", // Trailing comma supported
        ],
        "modules-center": [
            "hyprland/window",
        ],
        "modules-right": [
            "battery",
        ],
        "clock": {
            "format": "https://example.com/time/{:%H:%M}", // URL slashes preserved
            "tooltip": true,
        }
    }"#;

    let res = validate_waybar_jsonc("config.jsonc", jsonc_content);
    assert!(
        res.valid,
        "Valid JSONC with comments and trailing commas must parse: {:?}",
        res.error_message
    );
}

#[test]
fn test_waybar_procedural_directive_extraction() {
    let jsonc_content = r#"{
        "custom/media": {
            "format": "{icon} {}",
            "exec": "python3 ~/.config/waybar/scripts/mediaplayer.py",
            "on-click": "playerctl play-pause",
            "on-scroll-up": "playerctl next"
        }
    }"#;

    let directives = extract_waybar_directives("config.jsonc", jsonc_content);
    assert_eq!(directives.len(), 3);

    assert!(directives.iter().any(|d| d.directive_type == "exec"
        && d.command == "python3 ~/.config/waybar/scripts/mediaplayer.py"));
    assert!(directives
        .iter()
        .any(|d| d.directive_type == "on-click" && d.command == "playerctl play-pause"));
    assert!(directives
        .iter()
        .any(|d| d.directive_type == "on-scroll-up" && d.command == "playerctl next"));
}

#[test]
fn test_hyprland_dual_legacy_lua_precedence() {
    let engine = DetectionEngine::new();
    let tree = vec![
        TreeEntry::new(
            ".config/hypr/hyprland.conf",
            "100644",
            "blob",
            "conf_sha",
            Some(1024),
        ),
        TreeEntry::new(
            ".config/hypr/hyprland.lua",
            "100644",
            "blob",
            "lua_sha",
            Some(1024),
        ),
    ];

    let detection = engine
        .detect_component(&tree, ConfigComponent::Hyprland)
        .expect("Hyprland component must be detected");

    // Legacy conf must take precedence over Lua by default
    assert_eq!(detection.syntax_format, SyntaxFormat::LegacyConf);
    assert_eq!(detection.entrypoint, ".config/hypr/hyprland.conf");
}

#[test]
fn test_hyprlock_and_hypridle_independent_detection() {
    let engine = DetectionEngine::new();
    let tree = vec![
        TreeEntry::new(
            ".config/hypr/hyprlock.conf",
            "100644",
            "blob",
            "hl_sha",
            Some(512),
        ),
        TreeEntry::new(
            ".config/hypr/hypridle.conf",
            "100644",
            "blob",
            "hi_sha",
            Some(512),
        ),
    ];

    let lock = engine.detect_component(&tree, ConfigComponent::Hyprlock);
    assert!(lock.is_some(), "Hyprlock must be independently detected");
    assert_eq!(lock.unwrap().component, ConfigComponent::Hyprlock);

    let idle = engine.detect_component(&tree, ConfigComponent::Hypridle);
    assert!(idle.is_some(), "Hypridle must be independently detected");
    assert_eq!(idle.unwrap().component, ConfigComponent::Hypridle);

    // Test hypridle directive extraction
    let idle_conf = r#"
    general {
        lock_cmd = pidof hyprlock || hyprlock
    }
    listener {
        timeout = 300
        on-timeout = "hyprlock"
        on-resume = 'notify-send "Welcome back"'
    }
    "#;
    let dirs = extract_hypridle_directives("hypridle.conf", idle_conf);
    assert_eq!(dirs.len(), 2);
    assert_eq!(dirs[0].directive_type, "on-timeout");
    assert_eq!(dirs[0].command, "hyprlock");
    assert_eq!(dirs[1].directive_type, "on-resume");
    assert_eq!(dirs[1].command, "notify-send \"Welcome back\"");
}

#[test]
fn test_wallpaper_path_remapping() {
    let hyprpaper_content = r#"
    preload = /home/bob/Pictures/Wallpapers/neon_city.png
    wallpaper = eDP-1,/home/bob/Pictures/Wallpapers/neon_city.png
    "#;

    let repo_images = vec![TreeEntry::new(
        "wallpapers/neon_city.png",
        "100644",
        "blob",
        "img_sha",
        Some(1_000_000),
    )];

    let remapped = remap_hyprpaper_paths(
        hyprpaper_content,
        "/home/alice/.local/share/wallpapers",
        &repo_images,
    );

    assert!(remapped.contains("preload = /home/alice/.local/share/wallpapers/neon_city.png"));
    assert!(
        remapped.contains("wallpaper = eDP-1,/home/alice/.local/share/wallpapers/neon_city.png")
    );
}

#[test]
fn test_hyprcursor_detection() {
    let engine = DetectionEngine::new();
    let tree = vec![
        TreeEntry::new(
            "icons/Breeze-Hyprcursor/manifest.hl",
            "100644",
            "blob",
            "man_sha",
            Some(128),
        ),
        TreeEntry::new(
            "icons/Breeze-Hyprcursor/hyprcursors/arrow/meta.hl",
            "100644",
            "blob",
            "meta_sha",
            Some(64),
        ),
        TreeEntry::new(
            "icons/Breeze-Hyprcursor/hyprcursors/arrow/arrow.svg",
            "100644",
            "blob",
            "svg_sha",
            Some(1024),
        ),
    ];

    let detection = engine
        .detect_component(&tree, ConfigComponent::Hyprcursor)
        .expect("Hyprcursor must be detected from manifest.hl");

    assert_eq!(detection.component, ConfigComponent::Hyprcursor);
    assert_eq!(detection.entrypoint, "icons/Breeze-Hyprcursor/manifest.hl");
    assert_eq!(detection.root_path, "icons/Breeze-Hyprcursor");
    assert_eq!(detection.file_count, 3);
}

#[test]
fn test_hardware_monitor_preservation_flag() {
    let temp = tempdir().unwrap();
    let base_path = Utf8Path::from_path(temp.path()).unwrap();
    let xdg = XdgPaths::custom(base_path);
    let engine = DetectionEngine::new();

    let tree = sample_classic_hyprland_tree();
    let (mappings, notices) = engine
        .generate_target_mappings(&tree, ConfigComponent::Hyprland, &xdg)
        .expect("Target mappings must generate successfully");

    // Check that monitors.conf mapping is flagged
    let mon_mapping = mappings
        .iter()
        .find(|m| m.target_destination.as_str().contains("monitors.conf"))
        .expect("monitors.conf mapping must exist");

    assert!(mon_mapping.is_hardware_protected);
    assert_eq!(
        mon_mapping.conflict_status,
        ConflictStatus::HardwareProtected
    );

    // Regular file like animations.conf should NOT be hardware protected
    let anim_mapping = mappings
        .iter()
        .find(|m| m.target_destination.as_str().contains("animations.conf"))
        .expect("animations.conf mapping must exist");

    assert!(!anim_mapping.is_hardware_protected);
    assert_eq!(anim_mapping.conflict_status, ConflictStatus::NewFile);

    assert_eq!(notices.len(), 2);
}

#[test]
fn test_syntax_validation_catches_unclosed_braces() {
    // 1. Waybar CSS unclosed brace
    let broken_css = r#"
    window#waybar {
        background-color: rgba(43, 48, 59, 0.5);
    /* Missing closing brace */
    "#;
    let css_res = validate_gtk_css("style.css", broken_css);
    assert!(!css_res.valid);
    assert!(css_res
        .error_message
        .unwrap()
        .contains("Unclosed curly braces"));

    // 2. Hyprland unclosed block
    let broken_hl = r#"
    general {
        gaps_in = 5
        gaps_out = 20
    # Missing }
    "#;
    let hl_res = validate_hyprland_conf("hyprland.conf", broken_hl);
    assert!(!hl_res.valid);
    assert!(hl_res
        .error_message
        .unwrap()
        .contains("Unclosed section block"));

    // 3. Hypridle invalid timeout
    let broken_idle = r#"
    listener {
        timeout = not_a_number
        on-timeout = notify-send "Idle"
    }
    "#;
    let idle_res = validate_hypridle_conf("hypridle.conf", broken_idle);
    assert!(!idle_res.valid);
    assert!(idle_res
        .error_message
        .unwrap()
        .contains("Invalid non-numeric timeout"));
}

#[test]
fn test_symlink_mode_120000_detection() {
    let temp = tempdir().unwrap();
    let base_path = Utf8Path::from_path(temp.path()).unwrap();
    let xdg = XdgPaths::custom(base_path);
    let engine = DetectionEngine::new();

    // Adversarial tree containing a mode 120000 Git symlink
    let tree = vec![
        TreeEntry::new(
            ".config/hypr/hyprland.conf",
            "100644",
            "blob",
            "hl_sha",
            Some(1024),
        ),
        TreeEntry::new(
            ".config/hypr/sublink",
            "120000",
            "blob",
            "link_sha",
            Some(32),
        ),
    ];

    let res = engine.generate_target_mappings(&tree, ConfigComponent::Hyprland, &xdg);
    assert!(
        res.is_err(),
        "Tree containing Git symlink mode 120000 must be rejected"
    );

    match res.unwrap_err() {
        WoderriseError::Path(PathError::ForbiddenSymlink { path, .. }) => {
            assert!(path.as_str().contains("sublink"));
        }
        other => panic!("Expected ForbiddenSymlink, got {:?}", other),
    }
}

#[test]
fn test_scan_catalog_components() {
    let engine = DetectionEngine::new();
    let tree = vec![
        TreeEntry::new(
            ".config/hypr/hyprland.conf",
            "100644",
            "blob",
            "hl",
            Some(1000),
        ),
        TreeEntry::new(
            ".config/waybar/config.jsonc",
            "100644",
            "blob",
            "wb",
            Some(2000),
        ),
        TreeEntry::new(
            ".config/waybar/style.css",
            "100644",
            "blob",
            "css",
            Some(500),
        ),
        TreeEntry::new("wallpapers/bg.png", "100644", "blob", "wall", Some(50000)),
    ];

    let summaries = engine.scan_catalog_components(&tree);
    assert_eq!(summaries.len(), 3); // Hyprland, Waybar, Wallpapers

    assert!(summaries
        .iter()
        .any(|s| s.component == ConfigComponent::Hyprland));
    assert!(summaries
        .iter()
        .any(|s| s.component == ConfigComponent::Waybar));
    assert!(summaries
        .iter()
        .any(|s| s.component == ConfigComponent::Wallpapers));
}

#[test]
fn test_inline_monitor_directives_detection() {
    let conf = r#"
    autogenerated = 0
    monitor = DP-1, 2560x1440@165, 0x0, 1
    monitor = HDMI-A-1, 1920x1080@60, 2560x0, 1
    bind = SUPER, Q, exec, kitty
    "#;

    let notices = detect_inline_monitor_directives("hyprland.conf", conf);
    assert_eq!(notices.len(), 2);
    assert!(notices[0].description.contains("DP-1"));
    assert!(notices[1].description.contains("HDMI-A-1"));
}

#[test]
fn test_hyprland_directive_extraction() {
    let conf = r#"
    exec-once = waybar
    exec-once = hyprpaper &
    exec = nm-applet
    bind = SUPER, RETURN, exec, kitty
    "#;

    let dirs = extract_hyprland_directives("hyprland.conf", conf);
    assert_eq!(dirs.len(), 3); // 2 exec-once, 1 exec
    assert_eq!(dirs[0].directive_type, "exec-once");
    assert_eq!(dirs[0].command, "waybar");
    assert_eq!(dirs[1].directive_type, "exec-once");
    assert_eq!(dirs[1].command, "hyprpaper &");
    assert_eq!(dirs[2].directive_type, "exec");
    assert_eq!(dirs[2].command, "nm-applet");
}
