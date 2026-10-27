//! Config component and syntax format definitions.

use serde::{Deserialize, Serialize};

/// Supported Wayland configuration components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigComponent {
    /// Generic or default Hyprland component
    Hyprland,
    /// Hyprland compositor configuration (legacy hyprland.conf)
    #[serde(alias = "hyprland_legacy")]
    HyprlandLegacy,
    /// Modern Hyprland Lua configuration (hyprland.lua)
    #[serde(alias = "hyprland_lua")]
    HyprlandLua,
    /// Waybar status bar (config.jsonc + style.css)
    Waybar,
    /// Hyprlock screen locker (hyprlock.conf)
    Hyprlock,
    /// Hypridle idle daemon (hypridle.conf)
    Hypridle,
    /// Hyprpaper configuration and directives
    Hyprpaper,
    /// Wallpaper assets and directory bundles
    Wallpapers,
    /// Hyprcursor cursor themes
    Hyprcursor,
    /// GTK 3/4 settings and icon themes
    #[serde(alias = "gtk", alias = "gtk_theme")]
    GtkTheme,
    /// Hardware monitor configurations (monitors.conf)
    Monitors,
    /// Composite suite containing multiple components
    #[serde(alias = "complete_suite")]
    CompleteSuite,
}

impl ConfigComponent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Hyprland => "hyprland",
            Self::HyprlandLegacy => "hyprland_legacy",
            Self::HyprlandLua => "hyprland_lua",
            Self::Waybar => "waybar",
            Self::Hyprlock => "hyprlock",
            Self::Hypridle => "hypridle",
            Self::Hyprpaper => "hyprpaper",
            Self::Wallpapers => "wallpapers",
            Self::Hyprcursor => "hyprcursor",
            Self::GtkTheme => "gtk_theme",
            Self::Monitors => "monitors",
            Self::CompleteSuite => "complete_suite",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Hyprland => "Hyprland",
            Self::HyprlandLegacy => "Hyprland (Legacy Conf)",
            Self::HyprlandLua => "Hyprland (Lua)",
            Self::Waybar => "Waybar",
            Self::Hyprlock => "Hyprlock",
            Self::Hypridle => "Hypridle",
            Self::Hyprpaper => "Hyprpaper",
            Self::Wallpapers => "Wallpapers",
            Self::Hyprcursor => "Hyprcursor",
            Self::GtkTheme => "GTK & Icons",
            Self::Monitors => "Monitors",
            Self::CompleteSuite => "Complete Suite",
        }
    }

    pub fn default_config_subpath(&self) -> &'static str {
        match self {
            Self::Hyprland
            | Self::HyprlandLegacy
            | Self::HyprlandLua
            | Self::Hyprlock
            | Self::Hypridle
            | Self::Hyprpaper
            | Self::Monitors => "hypr",
            Self::Waybar => "waybar",
            Self::GtkTheme => "gtk-3.0",
            Self::Hyprcursor => "icons",
            Self::Wallpapers => "wallpapers",
            Self::CompleteSuite => "",
        }
    }

    pub fn default_entrypoint(&self) -> &'static str {
        match self {
            Self::Hyprland | Self::HyprlandLegacy => "hyprland.conf",
            Self::HyprlandLua => "hyprland.lua",
            Self::Waybar => "config.jsonc",
            Self::Hyprlock => "hyprlock.conf",
            Self::Hypridle => "hypridle.conf",
            Self::Hyprpaper => "hyprpaper.conf",
            Self::Wallpapers => "",
            Self::Hyprcursor => "manifest.hl",
            Self::GtkTheme => "settings.ini",
            Self::Monitors => "monitors.conf",
            Self::CompleteSuite => "hyprland.conf",
        }
    }
}

/// Syntax format of configuration files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxFormat {
    LegacyConf,
    Lua,
    Jsonc,
    Css,
    Hl,
    MetaHl,
    Ini,
    BinaryAsset,
}
