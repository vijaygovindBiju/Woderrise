//! Unified Design System Tokens and Theme Palette for Woderrise.
//!
//! Provides semantic color tokens (`ThemeTokens`), light/dark themes,
//! reusable Iced styling helpers, layout constants, and XDG Desktop Portal
//! theme listeners.

use ashpd::desktop::settings::ColorScheme;
use iced::widget::{button, container, text_input as text_input_widget};
use iced::{Border, Color, Theme};
use tracing::{info, warn};

/// Semantic color tokens for the Woderrise UI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeTokens {
    pub bg_base: Color,
    pub bg_surface: Color,
    pub bg_card: Color,
    pub bg_card_hover: Color,
    pub bg_sidebar: Color,
    pub bg_input: Color,

    pub border_subtle: Color,
    pub border_card: Color,
    pub border_focus: Color,

    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub text_inverse: Color,

    pub accent_primary: Color,
    pub accent_hover: Color,
    pub accent_subtle: Color,

    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub star_gold: Color,
}

impl ThemeTokens {
    /// Polished dark slate default with restrained blue accents (#3B82F6), subtle borders,
    /// and accessible typography contrast.
    pub fn dark() -> Self {
        Self {
            bg_base: Color::from_rgb8(11, 15, 23),       // #0B0F17
            bg_surface: Color::from_rgb8(19, 27, 42),    // #131B2A
            bg_card: Color::from_rgb8(26, 35, 56),       // #1A2338
            bg_card_hover: Color::from_rgb8(34, 46, 72), // #222E48
            bg_sidebar: Color::from_rgb8(15, 22, 35),    // #0F1623
            bg_input: Color::from_rgb8(16, 24, 38),      // #101826

            border_subtle: Color::from_rgba8(45, 60, 88, 0.45),
            border_card: Color::from_rgba8(51, 68, 100, 0.55),
            border_focus: Color::from_rgb8(59, 130, 246), // #3B82F6

            text_primary: Color::from_rgb8(248, 250, 252), // #F8FAFC
            text_secondary: Color::from_rgb8(148, 163, 184), // #94A3B8
            text_muted: Color::from_rgb8(100, 116, 139),   // #64748B
            text_inverse: Color::from_rgb8(15, 23, 42),    // #0F172A

            accent_primary: Color::from_rgb8(59, 130, 246), // #3B82F6
            accent_hover: Color::from_rgb8(37, 99, 235),    // #2563EB
            accent_subtle: Color::from_rgba8(59, 130, 246, 0.15),

            success: Color::from_rgb8(16, 185, 129), // #10B981
            warning: Color::from_rgb8(245, 158, 11), // #F59E0B
            danger: Color::from_rgb8(239, 68, 68),   // #EF4444
            star_gold: Color::from_rgb8(245, 158, 11), // #F59E0B
        }
    }

    /// Clean light mode with #F8FAFC base, #FFFFFF cards, and dark slate text #0F172A.
    pub fn light() -> Self {
        Self {
            bg_base: Color::from_rgb8(248, 250, 252),       // #F8FAFC
            bg_surface: Color::from_rgb8(255, 255, 255),    // #FFFFFF
            bg_card: Color::from_rgb8(255, 255, 255),       // #FFFFFF
            bg_card_hover: Color::from_rgb8(241, 245, 249), // #F1F5F9
            bg_sidebar: Color::from_rgb8(241, 245, 249),    // #F1F5F9
            bg_input: Color::from_rgb8(255, 255, 255),      // #FFFFFF

            border_subtle: Color::from_rgba8(203, 213, 225, 0.6),
            border_card: Color::from_rgba8(203, 213, 225, 0.8),
            border_focus: Color::from_rgb8(37, 99, 235), // #2563EB

            text_primary: Color::from_rgb8(15, 23, 42), // #0F172A
            text_secondary: Color::from_rgb8(71, 85, 105), // #475569
            text_muted: Color::from_rgb8(148, 163, 184), // #94A3B8
            text_inverse: Color::from_rgb8(255, 255, 255), // #FFFFFF

            accent_primary: Color::from_rgb8(37, 99, 235), // #2563EB
            accent_hover: Color::from_rgb8(29, 78, 216),   // #1D4ED8
            accent_subtle: Color::from_rgba8(37, 99, 235, 0.12),

            success: Color::from_rgb8(16, 185, 129),
            warning: Color::from_rgb8(217, 119, 6),
            danger: Color::from_rgb8(220, 38, 38),
            star_gold: Color::from_rgb8(217, 119, 6),
        }
    }
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self::dark()
    }
}

/// Resolves semantic tokens for an Iced `Theme`.
pub fn tokens(theme: &Theme) -> ThemeTokens {
    match theme {
        Theme::Light
        | Theme::SolarizedLight
        | Theme::GruvboxLight
        | Theme::CatppuccinLatte
        | Theme::TokyoNightLight
        | Theme::KanagawaLotus => ThemeTokens::light(),
        Theme::Dark
        | Theme::Dracula
        | Theme::Nord
        | Theme::SolarizedDark
        | Theme::GruvboxDark
        | Theme::CatppuccinFrappe
        | Theme::CatppuccinMacchiato
        | Theme::CatppuccinMocha
        | Theme::TokyoNight
        | Theme::TokyoNightStorm
        | Theme::KanagawaWave
        | Theme::KanagawaDragon => ThemeTokens::dark(),
        other => {
            if other.extended_palette().is_dark {
                ThemeTokens::dark()
            } else {
                ThemeTokens::light()
            }
        }
    }
}

/// Reusable container styling for the root application background.
pub fn container_base(tokens: ThemeTokens) -> container::Style {
    container::Style {
        background: Some(tokens.bg_base.into()),
        text_color: Some(tokens.text_primary),
        ..Default::default()
    }
}

/// Reusable container styling for surface sections (navigation header, toolbars, sidebars).
pub fn container_surface(tokens: ThemeTokens) -> container::Style {
    container::Style {
        background: Some(tokens.bg_surface.into()),
        text_color: Some(tokens.text_primary),
        border: Border {
            color: tokens.border_subtle,
            width: 1.0,
            radius: radius::MD,
        },
        ..Default::default()
    }
}

/// Reusable container styling for content cards.
pub fn container_card(tokens: ThemeTokens) -> container::Style {
    container::Style {
        background: Some(tokens.bg_card.into()),
        text_color: Some(tokens.text_primary),
        border: Border {
            color: tokens.border_card,
            width: 1.0,
            radius: radius::LG,
        },
        ..Default::default()
    }
}

/// Reusable primary button styling.
pub fn button_primary(tokens: ThemeTokens, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(tokens.accent_hover.into()),
            text_color: Color::WHITE,
            border: Border {
                radius: radius::MD,
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(tokens.accent_subtle.into()),
            text_color: tokens.text_muted,
            border: Border {
                radius: radius::MD,
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Active => button::Style {
            background: Some(tokens.accent_primary.into()),
            text_color: Color::WHITE,
            border: Border {
                radius: radius::MD,
                ..Default::default()
            },
            ..Default::default()
        },
    }
}

/// Reusable secondary / outline button styling.
pub fn button_secondary(tokens: ThemeTokens, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered => button::Style {
            background: Some(tokens.bg_card_hover.into()),
            text_color: tokens.text_primary,
            border: Border {
                color: tokens.border_card,
                width: 1.0,
                radius: radius::MD,
            },
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(tokens.bg_surface.into()),
            text_color: tokens.text_primary,
            border: Border {
                color: tokens.border_focus,
                width: 1.0,
                radius: radius::MD,
            },
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(tokens.bg_base.into()),
            text_color: tokens.text_muted,
            border: Border {
                color: tokens.border_subtle,
                width: 1.0,
                radius: radius::MD,
            },
            ..Default::default()
        },
        button::Status::Active => button::Style {
            background: Some(tokens.bg_card.into()),
            text_color: tokens.text_primary,
            border: Border {
                color: tokens.border_subtle,
                width: 1.0,
                radius: radius::MD,
            },
            ..Default::default()
        },
    }
}

/// Reusable danger / destructive button styling.
pub fn button_danger(tokens: ThemeTokens, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(tokens.danger.into()),
            text_color: Color::WHITE,
            border: Border {
                radius: radius::MD,
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(
                Color::from_rgba(tokens.danger.r, tokens.danger.g, tokens.danger.b, 0.2).into(),
            ),
            text_color: tokens.text_muted,
            border: Border {
                radius: radius::MD,
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Active => button::Style {
            background: Some(
                Color::from_rgba(tokens.danger.r, tokens.danger.g, tokens.danger.b, 0.85).into(),
            ),
            text_color: Color::WHITE,
            border: Border {
                radius: radius::MD,
                ..Default::default()
            },
            ..Default::default()
        },
    }
}

/// Reusable navigation pill tab styling.
pub fn button_tab(tokens: ThemeTokens, is_active: bool, status: button::Status) -> button::Style {
    if is_active {
        button::Style {
            background: Some(tokens.accent_primary.into()),
            text_color: Color::WHITE,
            border: Border {
                radius: radius::PILL,
                ..Default::default()
            },
            ..Default::default()
        }
    } else {
        match status {
            button::Status::Hovered => button::Style {
                background: Some(tokens.bg_card_hover.into()),
                text_color: tokens.text_primary,
                border: Border {
                    radius: radius::PILL,
                    ..Default::default()
                },
                ..Default::default()
            },
            _ => button::Style {
                background: Some(Color::TRANSPARENT.into()),
                text_color: tokens.text_secondary,
                border: Border {
                    radius: radius::PILL,
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }
}

/// Reusable pill badge container styling.
pub fn badge(bg: Color, text_color: Color) -> container::Style {
    container::Style {
        background: Some(bg.into()),
        text_color: Some(text_color),
        border: Border {
            radius: radius::PILL,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Reusable text input styling with focus, hover, and active states.
pub fn text_input(
    tokens: ThemeTokens,
    status: text_input_widget::Status,
) -> text_input_widget::Style {
    let border = match status {
        text_input_widget::Status::Focused { .. } => Border {
            color: tokens.border_focus,
            width: 1.5,
            radius: radius::MD,
        },
        text_input_widget::Status::Hovered => Border {
            color: tokens.border_card,
            width: 1.0,
            radius: radius::MD,
        },
        text_input_widget::Status::Disabled => Border {
            color: tokens.border_subtle,
            width: 1.0,
            radius: radius::MD,
        },
        text_input_widget::Status::Active => Border {
            color: tokens.border_subtle,
            width: 1.0,
            radius: radius::MD,
        },
    };

    text_input_widget::Style {
        background: tokens.bg_input.into(),
        border,
        icon: tokens.text_muted,
        placeholder: tokens.text_muted,
        value: tokens.text_primary,
        selection: tokens.accent_subtle,
    }
}

/// Computes the relative luminance of an sRGB color per WCAG 2.1 specifications.
pub fn relative_luminance(color: Color) -> f32 {
    let to_linear = |c: f32| -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let r = to_linear(color.r);
    let g = to_linear(color.g);
    let b = to_linear(color.b);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Calculates the WCAG contrast ratio between two colors (ranging from 1.0 to 21.0).
pub fn contrast_ratio(c1: Color, c2: Color) -> f32 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);
    let (lighter, darker) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Common spacing scale (in pixels).
pub mod spacing {
    pub const XS: u16 = 4;
    pub const SM: u16 = 8;
    pub const MD: u16 = 12;
    pub const LG: u16 = 16;
    pub const XL: u16 = 20;
    pub const XXL: u16 = 24;
}

/// Common border radius presets.
pub mod radius {
    use iced::border::Radius;

    pub const SM: Radius = Radius {
        top_left: 4.0,
        top_right: 4.0,
        bottom_right: 4.0,
        bottom_left: 4.0,
    };
    pub const MD: Radius = Radius {
        top_left: 6.0,
        top_right: 6.0,
        bottom_right: 6.0,
        bottom_left: 6.0,
    };
    pub const LG: Radius = Radius {
        top_left: 10.0,
        top_right: 10.0,
        bottom_right: 10.0,
        bottom_left: 10.0,
    };
    pub const PILL: Radius = Radius {
        top_left: 9999.0,
        top_right: 9999.0,
        bottom_right: 9999.0,
        bottom_left: 9999.0,
    };
}

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
