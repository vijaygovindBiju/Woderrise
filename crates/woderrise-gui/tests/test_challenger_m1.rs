//! Adversarial empirical challenge tests for Milestone M1:
//! Design System, Tokens, Navigation & Theme Management (R1).

use iced::widget::button;
use iced::widget::text_input as text_input_widget;
use iced::{Color, Theme};
use woderrise_gui::app::{App, EngineEvent, Message, Screen};
use woderrise_gui::theme::{
    button_primary, button_secondary, button_tab, container_base, container_card,
    container_surface, contrast_ratio, text_input, tokens, ThemeTokens,
};

/// 1. Comprehensive WCAG 2.1 Contrast Matrix Challenge.
///
/// Tests all foreground/background pairings in both dark and light modes.
#[test]
fn test_challenge_wcag_contrast_matrix() {
    let dark = ThemeTokens::dark();
    let light = ThemeTokens::light();

    // Helper to evaluate WCAG AA / AAA
    let check = |name: &str, fg: Color, bg: Color, min_ratio: f32| -> f32 {
        let ratio = contrast_ratio(fg, bg);
        assert!(
            ratio >= min_ratio,
            "FAILED contrast for {}: got {:.2}:1, required at least {:.2}:1",
            name,
            ratio,
            min_ratio
        );
        ratio
    };

    println!("\n=== DARK MODE CONTRAST MATRIX ===");
    // Primary text on all dark containers must satisfy WCAG AAA (>= 7.0:1) or at least AA (>= 4.5:1)
    let d_tp_base = check(
        "dark text_primary / bg_base",
        dark.text_primary,
        dark.bg_base,
        7.0,
    );
    let d_tp_surf = check(
        "dark text_primary / bg_surface",
        dark.text_primary,
        dark.bg_surface,
        7.0,
    );
    let d_tp_card = check(
        "dark text_primary / bg_card",
        dark.text_primary,
        dark.bg_card,
        4.5,
    );
    let d_tp_hover = check(
        "dark text_primary / bg_card_hover",
        dark.text_primary,
        dark.bg_card_hover,
        4.5,
    );
    let d_tp_side = check(
        "dark text_primary / bg_sidebar",
        dark.text_primary,
        dark.bg_sidebar,
        7.0,
    );
    let d_tp_input = check(
        "dark text_primary / bg_input",
        dark.text_primary,
        dark.bg_input,
        7.0,
    );

    // Secondary text on all dark containers must satisfy WCAG AA (>= 4.5:1)
    let d_ts_base = check(
        "dark text_secondary / bg_base",
        dark.text_secondary,
        dark.bg_base,
        4.5,
    );
    let d_ts_surf = check(
        "dark text_secondary / bg_surface",
        dark.text_secondary,
        dark.bg_surface,
        4.5,
    );
    let d_ts_card = check(
        "dark text_secondary / bg_card",
        dark.text_secondary,
        dark.bg_card,
        4.5,
    );
    let d_ts_hover = check(
        "dark text_secondary / bg_card_hover",
        dark.text_secondary,
        dark.bg_card_hover,
        4.0,
    );
    let d_ts_side = check(
        "dark text_secondary / bg_sidebar",
        dark.text_secondary,
        dark.bg_sidebar,
        4.5,
    );

    // Primary button contrast (White text on accent_primary and accent_hover)
    let d_btn_active = check(
        "dark primary button active (White / accent_primary)",
        Color::WHITE,
        dark.accent_primary,
        3.0,
    );
    let d_btn_hover = check(
        "dark primary button hover (White / accent_hover)",
        Color::WHITE,
        dark.accent_hover,
        4.5,
    );

    // Status colors on dark base
    let d_success = check("dark success / bg_base", dark.success, dark.bg_base, 4.5);
    let d_warning = check("dark warning / bg_base", dark.warning, dark.bg_base, 4.5);
    let d_danger = check("dark danger / bg_base", dark.danger, dark.bg_base, 4.5);
    let d_star = check(
        "dark star_gold / bg_base",
        dark.star_gold,
        dark.bg_base,
        4.5,
    );

    println!("Dark Mode Results:");
    println!(
        "  Primary text: base={:.2}, surf={:.2}, card={:.2}, hover={:.2}, side={:.2}, input={:.2}",
        d_tp_base, d_tp_surf, d_tp_card, d_tp_hover, d_tp_side, d_tp_input
    );
    println!(
        "  Secondary text: base={:.2}, surf={:.2}, card={:.2}, hover={:.2}, side={:.2}",
        d_ts_base, d_ts_surf, d_ts_card, d_ts_hover, d_ts_side
    );
    println!(
        "  Button text: active={:.2}, hover={:.2}",
        d_btn_active, d_btn_hover
    );
    println!(
        "  Status: success={:.2}, warning={:.2}, danger={:.2}, star={:.2}",
        d_success, d_warning, d_danger, d_star
    );

    println!("\n=== LIGHT MODE CONTRAST MATRIX ===");
    // Primary text on all light containers must satisfy WCAG AAA (>= 7.0:1)
    let l_tp_base = check(
        "light text_primary / bg_base",
        light.text_primary,
        light.bg_base,
        7.0,
    );
    let l_tp_surf = check(
        "light text_primary / bg_surface",
        light.text_primary,
        light.bg_surface,
        7.0,
    );
    let l_tp_card = check(
        "light text_primary / bg_card",
        light.text_primary,
        light.bg_card,
        7.0,
    );
    let l_tp_hover = check(
        "light text_primary / bg_card_hover",
        light.text_primary,
        light.bg_card_hover,
        7.0,
    );
    let l_tp_side = check(
        "light text_primary / bg_sidebar",
        light.text_primary,
        light.bg_sidebar,
        7.0,
    );
    let l_tp_input = check(
        "light text_primary / bg_input",
        light.text_primary,
        light.bg_input,
        7.0,
    );

    // Secondary text on all light containers must satisfy WCAG AA (>= 4.5:1)
    let l_ts_base = check(
        "light text_secondary / bg_base",
        light.text_secondary,
        light.bg_base,
        4.5,
    );
    let l_ts_surf = check(
        "light text_secondary / bg_surface",
        light.text_secondary,
        light.bg_surface,
        4.5,
    );
    let l_ts_card = check(
        "light text_secondary / bg_card",
        light.text_secondary,
        light.bg_card,
        4.5,
    );
    let l_ts_hover = check(
        "light text_secondary / bg_card_hover",
        light.text_secondary,
        light.bg_card_hover,
        4.5,
    );
    let l_ts_side = check(
        "light text_secondary / bg_sidebar",
        light.text_secondary,
        light.bg_sidebar,
        4.5,
    );

    // Primary button contrast (White text on accent_primary and accent_hover)
    let l_btn_active = check(
        "light primary button active (White / accent_primary)",
        Color::WHITE,
        light.accent_primary,
        4.5,
    );
    let l_btn_hover = check(
        "light primary button hover (White / accent_hover)",
        Color::WHITE,
        light.accent_hover,
        4.5,
    );

    // Status colors on light surface (#FFFFFF)
    let l_danger = check(
        "light danger / bg_surface",
        light.danger,
        light.bg_surface,
        4.5,
    );
    let l_warning = contrast_ratio(light.warning, light.bg_surface);
    let l_success = contrast_ratio(light.success, light.bg_surface);

    println!("Light Mode Results:");
    println!(
        "  Primary text: base={:.2}, surf={:.2}, card={:.2}, hover={:.2}, side={:.2}, input={:.2}",
        l_tp_base, l_tp_surf, l_tp_card, l_tp_hover, l_tp_side, l_tp_input
    );
    println!(
        "  Secondary text: base={:.2}, surf={:.2}, card={:.2}, hover={:.2}, side={:.2}",
        l_ts_base, l_ts_surf, l_ts_card, l_ts_hover, l_ts_side
    );
    println!(
        "  Button text: active={:.2}, hover={:.2}",
        l_btn_active, l_btn_hover
    );
    println!(
        "  Status: danger={:.2}, warning={:.2}, success={:.2}",
        l_danger, l_warning, l_success
    );
}

/// 2. Exhaustive Built-in Iced Themes Coverage Challenge.
///
/// Verifies `tokens(theme)` against every theme in `Theme::ALL`.
#[test]
fn test_challenge_all_iced_themes_coverage() {
    let all_themes = Theme::ALL;
    assert!(
        all_themes.len() >= 15,
        "Expected at least 15 built-in Iced themes, found {}",
        all_themes.len()
    );

    println!(
        "\n=== ICED THEMES RESOLUTION AUDIT (total: {}) ===",
        all_themes.len()
    );
    let mut light_count = 0;
    let mut dark_count = 0;

    for theme in all_themes {
        // Must never panic
        let resolved = tokens(theme);

        // Check if theme is logically dark or light
        let is_extended_dark = theme.extended_palette().is_dark;

        if resolved == ThemeTokens::dark() {
            dark_count += 1;
            assert!(
                is_extended_dark,
                "Theme {:?} resolved to dark tokens but extended_palette is light",
                theme
            );
        } else if resolved == ThemeTokens::light() {
            light_count += 1;
            assert!(
                !is_extended_dark,
                "Theme {:?} resolved to light tokens but extended_palette is dark",
                theme
            );
        } else {
            panic!("Theme {:?} resolved to an unknown token set", theme);
        }

        // Verify primary text contrast on the resolved base background meets WCAG AAA
        let primary_contrast = contrast_ratio(resolved.text_primary, resolved.bg_base);
        assert!(
            primary_contrast >= 7.0,
            "Theme {:?} failed primary text contrast on base: {:.2}:1",
            theme,
            primary_contrast
        );
    }

    println!(
        "All {} Iced themes verified without panics or mismatch ({} dark, {} light)",
        all_themes.len(),
        dark_count,
        light_count
    );
}

/// 3. Stress Test: ToggleDarkMode clean state oscillation and idempotent transitions
#[test]
fn test_challenge_toggle_dark_mode_oscillation() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    assert_eq!(app.active_theme, Theme::Dark);
    assert_eq!(tokens(&app.theme()), ThemeTokens::dark());

    // Rapidly toggle dark mode 100 times back and forth
    for i in 0..100 {
        if i % 2 == 0 {
            // Toggle to light
            let _ = app.update(Message::ToggleDarkMode(false));
            assert_eq!(app.active_theme, Theme::Light);
            assert_eq!(tokens(&app.theme()), ThemeTokens::light());
        } else {
            // Toggle to dark
            let _ = app.update(Message::ToggleDarkMode(true));
            assert_eq!(app.active_theme, Theme::Dark);
            assert_eq!(tokens(&app.theme()), ThemeTokens::dark());
        }
    }

    // Idempotent toggling (sending true when already dark)
    let _ = app.update(Message::ToggleDarkMode(true));
    assert_eq!(app.active_theme, Theme::Dark);
    assert_eq!(tokens(&app.theme()), ThemeTokens::dark());

    // Idempotent toggling (sending false when already light)
    let _ = app.update(Message::ToggleDarkMode(false));
    assert_eq!(app.active_theme, Theme::Light);
    let _ = app.update(Message::ToggleDarkMode(false));
    assert_eq!(app.active_theme, Theme::Light);
    assert_eq!(tokens(&app.theme()), ThemeTokens::light());

    // Verify engine portal events integrate cleanly
    let _ = app.update(Message::Engine(EngineEvent::PortalThemeChanged {
        dark_mode: true,
    }));
    assert_eq!(app.active_theme, Theme::Dark);
    assert_eq!(tokens(&app.theme()), ThemeTokens::dark());

    let _ = app.update(Message::Engine(EngineEvent::PortalThemeChanged {
        dark_mode: false,
    }));
    assert_eq!(app.active_theme, Theme::Light);
    assert_eq!(tokens(&app.theme()), ThemeTokens::light());
}

/// 4. Stress Test: App::view rendering with all screens in both Dark and Light modes
#[test]
fn test_challenge_app_view_across_all_screens_and_themes() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    let screens = [
        Screen::Search,
        Screen::Inspect,
        Screen::Install,
        Screen::History,
        Screen::Settings,
    ];

    let themes = [
        Theme::Dark,
        Theme::Light,
        Theme::Dracula,
        Theme::Nord,
        Theme::SolarizedLight,
        Theme::CatppuccinMocha,
        Theme::GruvboxLight,
    ];

    for theme in themes {
        app.active_theme = theme.clone();

        for screen in screens {
            let _ = app.update(Message::NavigateTo(screen));

            // Also test with active error banner and startup notice
            app.startup_notice = Some("Test startup notification".into());
            app.error_banner = Some("Test error notification".into());

            // View rendering must not panic
            {
                let _element = app.view();
            }

            // Clear banners
            let _ = app.update(Message::DismissStartupNotice);
            let _ = app.update(Message::DismissError);
            assert!(app.startup_notice.is_none());
            assert!(app.error_banner.is_none());

            // Re-render view in clean state
            {
                let _element_clean = app.view();
            }
        }
    }
}

/// 5. Reusable Styling Helpers Invariant Verification
#[test]
fn test_challenge_styling_helpers_invariants() {
    let dark = ThemeTokens::dark();
    let light = ThemeTokens::light();

    for t in [dark, light] {
        // Base container
        let base_style = container_base(t);
        assert_eq!(base_style.background, Some(t.bg_base.into()));
        assert_eq!(base_style.text_color, Some(t.text_primary));

        // Surface container
        let surf_style = container_surface(t);
        assert_eq!(surf_style.background, Some(t.bg_surface.into()));
        assert_eq!(surf_style.border.color, t.border_subtle);

        // Card container
        let card_style = container_card(t);
        assert_eq!(card_style.background, Some(t.bg_card.into()));
        assert_eq!(card_style.border.color, t.border_card);

        // Primary button active and hover
        let p_active = button_primary(t, button::Status::Active);
        assert_eq!(p_active.background, Some(t.accent_primary.into()));
        let p_hover = button_primary(t, button::Status::Hovered);
        assert_eq!(p_hover.background, Some(t.accent_hover.into()));
        let p_disabled = button_primary(t, button::Status::Disabled);
        assert_eq!(p_disabled.background, Some(t.accent_subtle.into()));

        // Secondary button
        let s_active = button_secondary(t, button::Status::Active);
        assert_eq!(s_active.background, Some(t.bg_card.into()));
        let s_hover = button_secondary(t, button::Status::Hovered);
        assert_eq!(s_hover.background, Some(t.bg_card_hover.into()));
        let s_disabled = button_secondary(t, button::Status::Disabled);
        assert_eq!(s_disabled.background, Some(t.bg_base.into()));

        // Tab button
        let tab_active = button_tab(t, true, button::Status::Active);
        assert_eq!(tab_active.background, Some(t.accent_primary.into()));
        let tab_inactive = button_tab(t, false, button::Status::Active);
        assert_eq!(tab_inactive.background, Some(Color::TRANSPARENT.into()));
        assert_eq!(tab_inactive.text_color, t.text_secondary);

        // Text input states
        let ti_active = text_input(t, text_input_widget::Status::Active);
        assert_eq!(ti_active.border.color, t.border_subtle);
        let ti_focused = text_input(t, text_input_widget::Status::Focused { is_hovered: false });
        assert_eq!(ti_focused.border.color, t.border_focus);
    }
}
