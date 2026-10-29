//! Adversarial Challenge Test Suite for Milestone M1 (Design System, Tokens, Navigation & Theme Management)
//!
//! Empirically stress-tests:
//! 1. WCAG 2.1 contrast ratios across all foreground/background pairings in dark and light modes.
//! 2. Mathematical boundaries of `relative_luminance` and `contrast_ratio` (black, white, identical, extremes).
//! 3. Built-in Iced theme mapping exhaustiveness, stability, and lack of panics across all variants.
//! 4. State transitions and idempotence of `Message::ToggleDarkMode` and `EngineEvent::PortalThemeChanged`.
//! 5. Component styling helper closure invocation across all button/input/container states.
//! 6. Full App view rendering under both themes ensuring zero layout crashes.

use iced::widget::{button, text_input as text_input_widget};
use iced::{Color, Theme};
use tokio::sync::mpsc;
use woderrise_gui::app::{App, EngineEvent, Message};
use woderrise_gui::theme::{
    badge, button_primary, button_secondary, button_tab, container_base, container_card,
    container_surface, contrast_ratio, map_color_scheme_to_theme, map_raw_scheme_value, radius,
    relative_luminance, spacing, text_input, tokens, ThemeTokens,
};

#[test]
fn test_adversarial_wcag_contrast_matrix_dark_mode() {
    let tokens = ThemeTokens::dark();

    let backgrounds = [
        ("bg_base", tokens.bg_base),
        ("bg_surface", tokens.bg_surface),
        ("bg_card", tokens.bg_card),
        ("bg_card_hover", tokens.bg_card_hover),
        ("bg_sidebar", tokens.bg_sidebar),
        ("bg_input", tokens.bg_input),
    ];

    // Primary text must achieve WCAG AA normal text (>= 4.5:1) and AAA (>= 7.0:1) on key backgrounds
    for (name, bg) in &backgrounds {
        let ratio = contrast_ratio(tokens.text_primary, *bg);
        assert!(
            ratio >= 7.0,
            "Dark mode primary text on {} contrast ({:.2}) failed WCAG AAA (7.0:1)",
            name,
            ratio
        );
    }

    // Secondary text must achieve WCAG AA normal text (>= 4.5:1) on primary backgrounds
    let key_backgrounds = [
        ("bg_base", tokens.bg_base),
        ("bg_surface", tokens.bg_surface),
        ("bg_sidebar", tokens.bg_sidebar),
    ];
    for (name, bg) in &key_backgrounds {
        let ratio = contrast_ratio(tokens.text_secondary, *bg);
        assert!(
            ratio >= 4.5,
            "Dark mode secondary text on {} contrast ({:.2}) failed WCAG AA (4.5:1)",
            name,
            ratio
        );
    }

    // Secondary text on card must meet at least WCAG AA large text (>= 3.0:1)
    let card_ratio = contrast_ratio(tokens.text_secondary, tokens.bg_card);
    assert!(
        card_ratio >= 3.5,
        "Dark mode secondary text on card contrast ({:.2}) was too low",
        card_ratio
    );

    // Button text (WHITE) against accent_primary and accent_hover must meet >= 3.0:1
    let btn_primary_ratio = contrast_ratio(Color::WHITE, tokens.accent_primary);
    assert!(
        btn_primary_ratio >= 3.0,
        "Dark primary button text contrast ({:.2}) failed minimum 3.0:1",
        btn_primary_ratio
    );

    let btn_hover_ratio = contrast_ratio(Color::WHITE, tokens.accent_hover);
    assert!(
        btn_hover_ratio >= 4.0,
        "Dark hover button text contrast ({:.2}) failed minimum 4.0:1",
        btn_hover_ratio
    );

    // Warning and Danger text on base background
    let danger_ratio = contrast_ratio(tokens.danger, tokens.bg_base);
    assert!(
        danger_ratio >= 4.0,
        "Dark mode danger contrast on base ({:.2}) was too low",
        danger_ratio
    );

    let warning_ratio = contrast_ratio(tokens.warning, tokens.bg_base);
    assert!(
        warning_ratio >= 6.0,
        "Dark mode warning contrast on base ({:.2}) was too low",
        warning_ratio
    );
}

#[test]
fn test_adversarial_wcag_contrast_matrix_light_mode() {
    let tokens = ThemeTokens::light();

    let backgrounds = [
        ("bg_base", tokens.bg_base),
        ("bg_surface", tokens.bg_surface),
        ("bg_card", tokens.bg_card),
        ("bg_card_hover", tokens.bg_card_hover),
        ("bg_sidebar", tokens.bg_sidebar),
        ("bg_input", tokens.bg_input),
    ];

    // Primary text must achieve WCAG AAA (>= 7.0:1) on all light backgrounds
    for (name, bg) in &backgrounds {
        let ratio = contrast_ratio(tokens.text_primary, *bg);
        assert!(
            ratio >= 7.0,
            "Light mode primary text on {} contrast ({:.2}) failed WCAG AAA (7.0:1)",
            name,
            ratio
        );
    }

    // Secondary text must achieve WCAG AA normal text (>= 4.5:1) on all light backgrounds
    for (name, bg) in &backgrounds {
        let ratio = contrast_ratio(tokens.text_secondary, *bg);
        assert!(
            ratio >= 4.5,
            "Light mode secondary text on {} contrast ({:.2}) failed WCAG AA (4.5:1)",
            name,
            ratio
        );
    }

    // Button text (WHITE) against accent_primary and accent_hover must meet >= 3.0:1
    let btn_primary_ratio = contrast_ratio(Color::WHITE, tokens.accent_primary);
    assert!(
        btn_primary_ratio >= 4.0,
        "Light primary button text contrast ({:.2}) failed minimum 4.0:1",
        btn_primary_ratio
    );

    let btn_hover_ratio = contrast_ratio(Color::WHITE, tokens.accent_hover);
    assert!(
        btn_hover_ratio >= 5.0,
        "Light hover button text contrast ({:.2}) failed minimum 5.0:1",
        btn_hover_ratio
    );

    // Warning and Danger text on white surface
    let danger_ratio = contrast_ratio(tokens.danger, tokens.bg_surface);
    assert!(
        danger_ratio >= 4.5,
        "Light mode danger contrast on surface ({:.2}) failed WCAG AA (4.5:1)",
        danger_ratio
    );

    let warning_ratio = contrast_ratio(tokens.warning, tokens.bg_surface);
    assert!(
        warning_ratio >= 3.0,
        "Light mode warning contrast on surface ({:.2}) failed WCAG large text (3.0:1)",
        warning_ratio
    );
}

#[test]
fn test_adversarial_luminance_and_contrast_boundaries() {
    let black = Color::from_rgb(0.0, 0.0, 0.0);
    let white = Color::from_rgb(1.0, 1.0, 1.0);

    // Boundary luminances
    let lum_black = relative_luminance(black);
    let lum_white = relative_luminance(white);

    assert_eq!(lum_black, 0.0);
    assert!((lum_white - 1.0).abs() < 1e-4);

    // Max contrast ratio between black and white is exactly 21:1
    let max_contrast = contrast_ratio(black, white);
    assert!(
        (max_contrast - 21.0).abs() < 1e-2,
        "Black to white contrast must be 21:1, got {}",
        max_contrast
    );

    // Reverse parameter order must yield identical result (symmetric)
    let max_contrast_rev = contrast_ratio(white, black);
    assert_eq!(max_contrast, max_contrast_rev);

    // Identical colors must yield exactly 1.0:1
    let self_contrast = contrast_ratio(black, black);
    assert!((self_contrast - 1.0).abs() < 1e-4);

    let white_self = contrast_ratio(white, white);
    assert!((white_self - 1.0).abs() < 1e-4);

    // Low luminance non-linear threshold boundary at 0.04045
    let low_c = Color::from_rgb(0.04, 0.04, 0.04);
    let lum_low = relative_luminance(low_c);
    assert!(lum_low > 0.0 && lum_low < 0.01);
}

#[test]
fn test_adversarial_all_iced_built_in_themes() {
    // Test all built-in themes from iced::Theme::ALL
    for theme in Theme::ALL {
        let t = tokens(theme);
        // None of the tokens should be zero/default empty colors
        assert_ne!(t.bg_base, Color::TRANSPARENT);
        assert_ne!(t.bg_surface, Color::TRANSPARENT);
        assert_ne!(t.text_primary, Color::TRANSPARENT);
        assert_ne!(t.accent_primary, Color::TRANSPARENT);

        // Every built-in theme must map to either dark or light tokens
        assert!(t == ThemeTokens::dark() || t == ThemeTokens::light());

        // Verify contrast for every mapped theme
        let contrast = contrast_ratio(t.text_primary, t.bg_base);
        assert!(
            contrast >= 7.0,
            "Theme {:?} resolved to tokens with insufficient contrast {:.2}",
            theme,
            contrast
        );
    }
}

#[test]
fn test_adversarial_portal_scheme_mapping() {
    use ashpd::desktop::settings::ColorScheme;

    // Direct mapping
    assert_eq!(
        map_color_scheme_to_theme(ColorScheme::PreferDark),
        Theme::Dark
    );
    assert_eq!(
        map_color_scheme_to_theme(ColorScheme::PreferLight),
        Theme::Light
    );
    assert_eq!(
        map_color_scheme_to_theme(ColorScheme::NoPreference),
        Theme::Dark
    );

    // Raw scheme values
    assert_eq!(map_raw_scheme_value(1), Theme::Dark);
    assert_eq!(map_raw_scheme_value(2), Theme::Light);
    assert_eq!(map_raw_scheme_value(0), Theme::Dark); // NoPreference
    assert_eq!(map_raw_scheme_value(99), Theme::Dark); // Corrupted / unknown value
    assert_eq!(map_raw_scheme_value(u32::MAX), Theme::Dark); // Max boundary
}

#[test]
fn test_adversarial_toggle_dark_mode_state_machine() {
    let (cmd_tx, _cmd_rx) = mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    // Initial state: starts dark
    assert_eq!(app.theme(), Theme::Dark);
    assert_eq!(tokens(&app.theme()), ThemeTokens::dark());

    // Rapid toggle cycling 50 times
    for i in 0..50 {
        if i % 2 == 0 {
            let _ = app.update(Message::ToggleDarkMode(false));
            assert_eq!(app.theme(), Theme::Light);
            assert_eq!(tokens(&app.theme()), ThemeTokens::light());
        } else {
            let _ = app.update(Message::ToggleDarkMode(true));
            assert_eq!(app.theme(), Theme::Dark);
            assert_eq!(tokens(&app.theme()), ThemeTokens::dark());
        }
    }

    // Idempotent calls: toggling to same state produces no corruption
    let _ = app.update(Message::ToggleDarkMode(true));
    let _ = app.update(Message::ToggleDarkMode(true));
    assert_eq!(app.theme(), Theme::Dark);

    let _ = app.update(Message::ToggleDarkMode(false));
    let _ = app.update(Message::ToggleDarkMode(false));
    assert_eq!(app.theme(), Theme::Light);

    // Portal theme events override active theme cleanly
    let _ = app.update(Message::Engine(EngineEvent::PortalThemeChanged {
        dark_mode: true,
    }));
    assert_eq!(app.theme(), Theme::Dark);

    let _ = app.update(Message::Engine(EngineEvent::PortalThemeChanged {
        dark_mode: false,
    }));
    assert_eq!(app.theme(), Theme::Light);
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn test_adversarial_styling_helper_closures_and_layout_constants() {
    let dark_tokens = ThemeTokens::dark();
    let light_tokens = ThemeTokens::light();

    for t in [dark_tokens, light_tokens] {
        // Container styles
        let base_st = container_base(t);
        assert!(base_st.background.is_some());
        assert_eq!(base_st.text_color, Some(t.text_primary));

        let surf_st = container_surface(t);
        assert!(surf_st.background.is_some());
        assert_eq!(surf_st.text_color, Some(t.text_primary));
        assert_eq!(surf_st.border.color, t.border_subtle);

        let card_st = container_card(t);
        assert!(card_st.background.is_some());
        assert_eq!(card_st.text_color, Some(t.text_primary));
        assert_eq!(card_st.border.color, t.border_card);

        // Button styles across all statuses
        for status in [
            button::Status::Active,
            button::Status::Hovered,
            button::Status::Pressed,
            button::Status::Disabled,
        ] {
            let p_style = button_primary(t, status);
            assert!(p_style.background.is_some());

            let s_style = button_secondary(t, status);
            assert!(s_style.background.is_some());

            let tab_active = button_tab(t, true, status);
            assert_eq!(tab_active.background, Some(t.accent_primary.into()));

            let tab_inactive = button_tab(t, false, status);
            if status == button::Status::Hovered {
                assert_eq!(tab_inactive.background, Some(t.bg_card_hover.into()));
            } else {
                assert_eq!(tab_inactive.background, Some(Color::TRANSPARENT.into()));
            }
        }

        // Text input styles across statuses
        let statuses = [
            text_input_widget::Status::Active,
            text_input_widget::Status::Hovered,
            text_input_widget::Status::Focused { is_hovered: false },
            text_input_widget::Status::Focused { is_hovered: true },
            text_input_widget::Status::Disabled,
        ];
        for status in statuses {
            let ti_style = text_input(t, status);
            assert_eq!(ti_style.background, t.bg_input.into());
            assert_eq!(ti_style.value, t.text_primary);
            assert_eq!(ti_style.placeholder, t.text_muted);
        }

        // Badge styling
        let badge_st = badge(t.accent_subtle, t.accent_primary);
        assert_eq!(badge_st.background, Some(t.accent_subtle.into()));
        assert_eq!(badge_st.text_color, Some(t.accent_primary));
    }

    // Verify layout constants are non-zero and ordered
    assert!(spacing::XS < spacing::SM);
    assert!(spacing::SM < spacing::MD);
    assert!(spacing::MD < spacing::LG);
    assert!(spacing::LG < spacing::XL);
    assert!(spacing::XL < spacing::XXL);

    assert!(radius::SM.top_left < radius::MD.top_left);
    assert!(radius::MD.top_left < radius::LG.top_left);
    assert!(radius::LG.top_left < radius::PILL.top_left);
}

#[test]
fn test_adversarial_app_view_renders_cleanly_in_both_themes() {
    let (cmd_tx, _cmd_rx) = mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    // 1. Render in dark theme
    app.active_theme = Theme::Dark;
    {
        let _view_dark = app.view();
    }

    // 2. Render in light theme
    app.active_theme = Theme::Light;
    {
        let _view_light = app.view();
    }

    // 3. Render with startup notice and error banner in light mode
    app.startup_notice = Some("Test startup notification".into());
    app.error_banner = Some("Test critical error message".into());
    {
        let _view_with_banners_light = app.view();
    }

    // 4. Render with startup notice and error banner in dark mode
    app.active_theme = Theme::Dark;
    {
        let _view_with_banners_dark = app.view();
    }
}
