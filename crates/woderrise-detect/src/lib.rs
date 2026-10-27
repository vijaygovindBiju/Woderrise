//! Component detection and syntax parsing for Woderrise.
//!
//! Provides independent parsers and heuristics for Hyprland (conf and Lua),
//! Waybar JSONC, Hyprlock, Hypridle, wallpapers, Hyprcursor, and GTK themes.

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
