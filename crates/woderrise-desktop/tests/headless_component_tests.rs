use std::env;
use woderrise_desktop::test_helpers::init_headless_slint;
use woderrise_desktop::{AppWindow, NavTab};

#[test]
fn test_headless_slint_instantiation_without_display_server() {
    // Deliberately empty display server variables to simulate pure headless CI
    env::remove_var("DISPLAY");
    env::remove_var("WAYLAND_DISPLAY");

    init_headless_slint();

    let app = AppWindow::new().expect("Failed to create headless AppWindow");

    // Assert initial layout and defaults
    assert_eq!(app.get_active_view(), 0); // 0 = Home view
    assert_eq!(app.get_current_tab(), NavTab::Home);
    assert_eq!(app.get_search_query(), "");
    assert!(!app.get_show_diff_modal());
    assert!(!app.get_diff_modal_open());
    assert!(!app.get_show_progress_modal());
    assert!(!app.get_progress_active());
}

#[test]
fn test_headless_sidebar_navigation_switching_across_7_views() {
    init_headless_slint();
    let app = AppWindow::new().unwrap();

    // 0: Home, 1: Browse, 2: Categories, 3: Installed, 4: Updates, 5: Registry, 6: Settings
    for target_view in 0..=6 {
        app.invoke_switch_view(target_view);
        assert_eq!(
            app.get_active_view(),
            target_view,
            "Failed switching to view index {}",
            target_view
        );
    }
}

#[test]
fn test_headless_search_query_property_binding() {
    init_headless_slint();
    let app = AppWindow::new().unwrap();

    app.set_search_query("waybar".into());
    assert_eq!(app.get_search_query(), "waybar");

    app.invoke_search_changed("catppuccin".into());
    assert_eq!(app.get_search_query(), "catppuccin");

    app.invoke_search_changed("".into());
    assert_eq!(app.get_search_query(), "");
}

#[test]
fn test_headless_diff_modal_open_and_close_toggle() {
    init_headless_slint();
    let app = AppWindow::new().unwrap();

    assert!(!app.get_show_diff_modal());
    assert!(!app.get_diff_modal_open());

    app.set_show_diff_modal(true);
    assert!(app.get_show_diff_modal());

    app.invoke_close_diff_modal();
    assert!(!app.get_show_diff_modal());
    assert!(!app.get_diff_modal_open());
}

#[test]
fn test_headless_settings_view_toggles_and_defaults_reset() {
    init_headless_slint();
    let app = AppWindow::new().unwrap();

    // Navigate to Settings
    app.invoke_switch_view(6);
    assert_eq!(app.get_active_view(), 6);

    // Initial defaults
    assert!(app.get_setting_auto_backup());
    assert!(app.get_setting_strict_hash());
    assert!(app.get_setting_offline_cache());
    assert!(app.get_setting_offline_first());

    // Toggle switches
    app.invoke_toggle_setting_auto_backup();
    assert!(!app.get_setting_auto_backup());

    app.invoke_toggle_setting_strict_hash();
    assert!(!app.get_setting_strict_hash());

    app.invoke_toggle_setting_offline_cache();
    assert!(!app.get_setting_offline_cache());
    assert!(!app.get_setting_offline_first());

    // Restore defaults
    app.invoke_restore_setting_defaults();
    assert!(app.get_setting_auto_backup());
    assert!(app.get_setting_strict_hash());
    assert!(app.get_setting_offline_cache());
    assert!(app.get_setting_offline_first());
}

#[test]
fn test_headless_progress_modal_lifecycle() {
    init_headless_slint();
    let app = AppWindow::new().unwrap();

    assert!(!app.get_show_progress_modal());
    assert!(!app.get_progress_active());

    app.set_show_progress_modal(true);
    app.set_progress_stage("Resolve".into());
    app.set_progress_percentage(15);
    app.set_progress_message("Resolving package manifest".into());

    assert!(app.get_show_progress_modal());
    assert_eq!(app.get_progress_stage(), "Resolve");
    assert_eq!(app.get_progress_percentage(), 15);
    assert_eq!(app.get_progress_message(), "Resolving package manifest");

    // Finish operation
    app.set_progress_stage("Commit".into());
    app.set_progress_percentage(100);
    app.set_show_progress_modal(false);
    assert!(!app.get_show_progress_modal());
}
