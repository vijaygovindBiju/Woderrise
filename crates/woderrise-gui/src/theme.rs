//! XDG Desktop Portal theme listener and color palette mapping for Woderrise.
//!
//! Maps portal color-scheme preferences (`PreferLight` -> `Light`, `PreferDark`/`NoPreference` -> `Dark`)
//! with fail-safe dark fallback for headless/CI environments.

use ashpd::desktop::settings::ColorScheme;
use iced::Theme;
use tracing::{info, warn};

/// Catppuccin Mocha color palette accents for Wayland ricing aesthetics.
pub mod palette {
    use iced::Color;

    pub const BASE: Color = Color::from_rgb(0.12, 0.12, 0.18); // #1e1e2e
    pub const MANTLE: Color = Color::from_rgb(0.09, 0.09, 0.15); // #181825
    pub const CRUST: Color = Color::from_rgb(0.07, 0.07, 0.11); // #11111b
    pub const SURFACE0: Color = Color::from_rgb(0.19, 0.20, 0.27); // #313244
    pub const SURFACE1: Color = Color::from_rgb(0.27, 0.28, 0.35); // #45475a
    pub const TEXT: Color = Color::from_rgb(0.80, 0.84, 0.96); // #cdd6f4
    pub const SUBTEXT: Color = Color::from_rgb(0.65, 0.68, 0.82); // #a6adc8
    pub const BLUE: Color = Color::from_rgb(0.54, 0.71, 0.98); // #89b4fa
    pub const GREEN: Color = Color::from_rgb(0.65, 0.89, 0.63); // #a6e3a1
    pub const YELLOW: Color = Color::from_rgb(0.98, 0.89, 0.69); // #f9e2af
    pub const RED: Color = Color::from_rgb(0.96, 0.58, 0.66); // #f38ba8
    pub const MAUVE: Color = Color::from_rgb(0.79, 0.69, 0.97); // #cba6f7
    pub const PEACH: Color = Color::from_rgb(0.98, 0.70, 0.53); // #fab387
}

/// Maps an `ashpd` `ColorScheme` to an Iced `Theme`.
pub fn map_color_scheme_to_theme(scheme: ColorScheme) -> Theme {
    match scheme {
        ColorScheme::NoPreference => Theme::Dark,
        ColorScheme::PreferDark => Theme::Dark,
        ColorScheme::PreferLight => Theme::Light,
    }
}

/// Maps a raw integer value (as returned by D-Bus portal) to an Iced `Theme`.
pub fn map_raw_scheme_value(value: u32) -> Theme {
    match value {
        1 => Theme::Dark,
        2 => Theme::Light,
        _ => Theme::Dark, // 0 (NoPreference) or unknown defaults to Dark
    }
}

/// Fetches the initial theme from the XDG Desktop Portal Settings.
/// If portal is unavailable (headless / CI / no D-Bus session), safely returns `Theme::Dark`.
pub async fn fetch_portal_theme() -> Theme {
    match ashpd::desktop::settings::Settings::new().await {
        Ok(settings) => match settings.color_scheme().await {
            Ok(scheme) => {
                info!("XDG Desktop Portal detected color scheme: {:?}", scheme);
                map_color_scheme_to_theme(scheme)
            }
            Err(e) => {
                warn!(
                    "Failed to read color scheme from portal: {:?}; using default Dark",
                    e
                );
                Theme::Dark
            }
        },
        Err(e) => {
            warn!(
                "Could not connect to XDG Desktop Portal Settings: {:?}; using default Dark",
                e
            );
            Theme::Dark
        }
    }
}
