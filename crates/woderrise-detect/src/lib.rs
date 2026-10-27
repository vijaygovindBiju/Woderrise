//! Component detection and syntax parsing for Woderrise.
//!
//! Provides independent parsers and heuristics for Hyprland (legacy conf and modern Lua),
//! Waybar JSONC, Hyprlock, Hypridle, wallpapers, Hyprcursor, GTK themes, and hardware monitor preservation.

pub mod detector;
pub mod gtk;
pub mod hypridle;
pub mod hyprland;
pub mod hyprlock;
pub mod monitors;
pub mod types;
pub mod wallpaper;
pub mod waybar;

pub use detector::{ComponentDetector, DetectionEngine};
pub use gtk::{
    validate_gtk_settings_ini, validate_hyprcursor_manifest, GtkDetector, HyprcursorDetector,
};
pub use hypridle::{extract_hypridle_directives, validate_hypridle_conf, HypridleDetector};
pub use hyprland::{
    extract_hyprland_directives, extract_hyprland_sources, validate_hyprland_conf, HyprlandDetector,
};
pub use hyprlock::{validate_hyprlock_conf, HyprlockDetector};
pub use monitors::{
    create_hardware_conflict_notice, detect_inline_monitor_directives, is_hardware_bound_filename,
    is_hardware_bound_path, MonitorDetector, HARDWARE_BOUND_FILENAMES,
};
pub use types::{ComponentDetection, ProceduralDirective, SyntaxValidationResult, TreeEntry};
pub use wallpaper::{is_wallpaper_asset, remap_hyprpaper_paths, WallpaperDetector};
pub use waybar::{
    extract_waybar_directives, validate_gtk_css, validate_waybar_jsonc, WaybarDetector,
};

/// Returns the crate version string.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
