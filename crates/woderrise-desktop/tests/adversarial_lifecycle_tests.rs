//! Adversarial tests for Slint component lifecycle, modal transitions, and settings persistence.
//!
//! Specifically tests:
//! 1. Rapid 7-view switching across all permutations and invalid index boundaries.
//! 2. Modal open/close lifecycle, rapid 100-cycle toggles, clean dismissals, and trap avoidance.
//! 3. Overlapping modal states (DiffModal + ProgressModal) and modal interaction integrity.
//! 4. Settings toggle mutations, offline-cache/offline-first link, and reset-to-defaults fidelity.
//! 5. SettingsState disk serialization, deserialization, corrupted JSON resilience, and fallback.
//! 6. Controller lifecycle with SQLite catalog, package selection, and callback dispatch in headless mode.

use std::{
    env,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use slint::ComponentHandle;
use woderrise_core::{
    catalog::{self, CatalogEntry},
    Database,
};
use woderrise_desktop::{
    catalog_entries_to_cards_model,
    test_helpers::init_headless_slint,
    AppWindow, NavTab, SettingsState,
};

fn setup_headless_env() {
    env::remove_var("DISPLAY");
    env::remove_var("WAYLAND_DISPLAY");
    init_headless_slint();
}

// ============================================================================
// 1. Seven-View Switching & Navigation State Lifecycle
// ============================================================================

#[test]
fn test_adversarial_rapid_7_view_switching_forward_backward_and_ping_pong() {
    setup_headless_env();
    let app = AppWindow::new().expect("Failed to instantiate AppWindow");

    // Initial state check
    assert_eq!(app.get_active_view(), 0);
    assert_eq!(app.get_current_tab(), NavTab::Home);

    // Forward sweep 0..=6
    let expected_tabs = [
        NavTab::Home,
        NavTab::Browse,
        NavTab::Categories,
        NavTab::Installed,
        NavTab::Updates,
        NavTab::Registry,
        NavTab::Settings,
    ];

    for (idx, expected_tab) in expected_tabs.iter().enumerate() {
        app.invoke_switch_view(idx as i32);
        assert_eq!(app.get_active_view(), idx as i32);
        assert_eq!(app.get_current_tab(), *expected_tab);
    }

    // Backward sweep 6..=0
    for idx in (0..=6).rev() {
        app.invoke_switch_view(idx as i32);
        assert_eq!(app.get_active_view(), idx as i32);
        assert_eq!(app.get_current_tab(), expected_tabs[idx]);
    }

    // Rapid Ping-Pong (100 switches between Home and Settings)
    for _ in 0..50 {
        app.invoke_switch_view(0);
        assert_eq!(app.get_active_view(), 0);
        assert_eq!(app.get_current_tab(), NavTab::Home);

        app.invoke_switch_view(6);
        assert_eq!(app.get_active_view(), 6);
        assert_eq!(app.get_current_tab(), NavTab::Settings);
    }

    // Pseudo-random view walk across 200 view transitions
    let walk = [
        2, 5, 1, 4, 0, 6, 3, 1, 5, 2, 0, 4, 6, 3, 2, 1, 0, 5, 4, 6, 3, 0, 1, 2,
    ];
    for &target in walk.iter().cycle().take(200) {
        app.invoke_switch_view(target);
        assert_eq!(app.get_active_view(), target);
        assert_eq!(app.get_current_tab(), expected_tabs[target as usize]);
    }
}

#[test]
fn test_adversarial_view_switching_out_of_bounds_handling() {
    setup_headless_env();
    let app = AppWindow::new().expect("Failed to instantiate AppWindow");

    // Establish baseline at Home (0)
    app.invoke_switch_view(0);
    assert_eq!(app.get_active_view(), 0);
    assert_eq!(app.get_current_tab(), NavTab::Home);

    // Negative view index: does not crash, preserves prior current_tab
    app.invoke_switch_view(-1);
    assert_eq!(app.get_active_view(), -1);
    assert_eq!(app.get_current_tab(), NavTab::Home);

    // Out-of-range positive view index: does not crash, preserves prior current_tab
    app.invoke_switch_view(7);
    assert_eq!(app.get_active_view(), 7);
    assert_eq!(app.get_current_tab(), NavTab::Home);

    app.invoke_switch_view(9999);
    assert_eq!(app.get_active_view(), 9999);
    assert_eq!(app.get_current_tab(), NavTab::Home);

    // Return to valid view (Browse) recovers clean state immediately
    app.invoke_switch_view(1);
    assert_eq!(app.get_active_view(), 1);
    assert_eq!(app.get_current_tab(), NavTab::Browse);
}

// ============================================================================
// 2. Modal Open/Close States & Dismissal Trap Avoidance
// ============================================================================

#[test]
fn test_adversarial_diff_modal_rapid_open_close_cycles() {
    setup_headless_env();
    let app = AppWindow::new().expect("Failed to instantiate AppWindow");

    // Verify modal starts closed
    assert!(!app.get_show_diff_modal());
    assert!(!app.get_diff_modal_open());

    // Rapid 100 cycles of open -> close via invoke_close_diff_modal
    for cycle in 0..100 {
        app.set_diff_modal_open(true);
        app.set_show_diff_modal(true);
        assert!(app.get_show_diff_modal(), "Failed open at cycle {}", cycle);
        assert!(app.get_diff_modal_open(), "Failed open at cycle {}", cycle);

        app.invoke_close_diff_modal();
        assert!(!app.get_show_diff_modal(), "Modal trapped at cycle {}", cycle);
        assert!(!app.get_diff_modal_open(), "Modal trapped at cycle {}", cycle);
    }
}

#[test]
fn test_adversarial_progress_modal_lifecycle_and_stage_transitions() {
    setup_headless_env();
    let app = AppWindow::new().expect("Failed to instantiate AppWindow");

    assert!(!app.get_show_progress_modal());
    assert!(!app.get_progress_active());

    // 15 Standard stages during installation
    let stages = [
        ("Resolve", "Resolving package manifest and dependencies", 0.05, 5),
        ("VerifyHash", "Verifying SHA-256 package checksums", 0.15, 15),
        ("Extract", "Extracting package payload files", 0.25, 25),
        ("InspectPlan", "Validating pre-execution target paths", 0.35, 35),
        ("PreBackup", "Creating content-addressed rollback backups", 0.45, 45),
        ("SwapFiles", "Applying target files into ~/.config", 0.65, 65),
        ("JournalCommit", "Recording transaction into SQLite WAL", 0.85, 85),
        ("Complete", "Installation complete and active", 1.0, 100),
    ];

    app.set_progress_active(true);
    app.set_show_progress_modal(true);

    for (stage, msg, frac, pct) in stages {
        app.set_progress_stage(stage.into());
        app.set_progress_message(msg.into());
        app.set_progress_fraction(frac);
        app.set_progress_percentage(pct);

        assert_eq!(app.get_progress_stage(), stage);
        assert_eq!(app.get_progress_message(), msg);
        assert!((app.get_progress_fraction() - frac).abs() < 1e-4);
        assert_eq!(app.get_progress_percentage(), pct);
    }

    // Dismissal
    app.set_progress_active(false);
    app.set_show_progress_modal(false);
    assert!(!app.get_show_progress_modal());
    assert!(!app.get_progress_active());
}

#[test]
fn test_adversarial_overlapping_modals_do_not_interfere() {
    setup_headless_env();
    let app = AppWindow::new().expect("Failed to instantiate AppWindow");

    // Open Diff Modal first
    app.set_diff_modal_open(true);
    app.set_show_diff_modal(true);
    assert!(app.get_show_diff_modal());

    // Now trigger Progress Modal (e.g. user clicked Apply Install)
    app.set_progress_active(true);
    app.set_show_progress_modal(true);
    app.set_progress_title("Installing Waybar".into());

    // Close diff modal as expected during install initiation
    app.invoke_close_diff_modal();
    assert!(!app.get_show_diff_modal());
    assert!(!app.get_diff_modal_open());

    // Progress modal must remain open and unaffected
    assert!(app.get_show_progress_modal());
    assert!(app.get_progress_active());
    assert_eq!(app.get_progress_title(), "Installing Waybar");

    // Close progress modal
    app.set_progress_active(false);
    app.set_show_progress_modal(false);
    assert!(!app.get_show_progress_modal());
    assert!(!app.get_progress_active());
}

#[test]
fn test_adversarial_view_switching_while_diff_modal_is_open() {
    setup_headless_env();
    let app = AppWindow::new().expect("Failed to instantiate AppWindow");

    // In browse tab, open diff modal
    app.invoke_switch_view(1);
    app.set_diff_modal_open(true);
    app.set_show_diff_modal(true);
    assert!(app.get_show_diff_modal());

    // User switches view to Settings (6)
    app.invoke_switch_view(6);
    assert_eq!(app.get_active_view(), 6);
    assert_eq!(app.get_current_tab(), NavTab::Settings);

    // Diff modal can still be cleanly dismissed from settings view
    app.invoke_close_diff_modal();
    assert!(!app.get_show_diff_modal());
    assert!(!app.get_diff_modal_open());
}

// ============================================================================
// 3. Settings Toggles, Synchronization & Reset to Defaults
// ============================================================================

#[test]
fn test_adversarial_settings_toggles_and_reset_state_machine() {
    setup_headless_env();
    let app = AppWindow::new().expect("Failed to instantiate AppWindow");

    // Ensure default initial settings
    assert!(app.get_setting_auto_backup());
    assert!(app.get_setting_strict_hash());
    assert!(app.get_setting_offline_cache());
    assert!(app.get_setting_offline_first());

    // Multiple toggles on each setting (10 toggles)
    for cycle in 0..10 {
        // Initial was true. After 1st toggle (cycle 0), value should be false.
        // After 2nd toggle (cycle 1), value should be true.
        let expected = cycle % 2 == 1; // true when odd cycle, false when even cycle

        app.invoke_toggle_setting_auto_backup();
        assert_eq!(app.get_setting_auto_backup(), expected);

        app.invoke_toggle_setting_strict_hash();
        assert_eq!(app.get_setting_strict_hash(), expected);

        app.invoke_toggle_setting_offline_cache();
        assert_eq!(app.get_setting_offline_cache(), expected);
        assert_eq!(app.get_setting_offline_first(), expected);
    }

    // Inverted state: turn all off
    if app.get_setting_auto_backup() {
        app.invoke_toggle_setting_auto_backup();
    }
    if app.get_setting_strict_hash() {
        app.invoke_toggle_setting_strict_hash();
    }
    if app.get_setting_offline_cache() {
        app.invoke_toggle_setting_offline_cache();
    }

    assert!(!app.get_setting_auto_backup());
    assert!(!app.get_setting_strict_hash());
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
fn test_adversarial_settings_state_disk_persistence_and_corruption_resilience() {
    let temp_dir = std::env::temp_dir().join(format!("woderrise_test_settings_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    // 1. Missing file falls back to default
    let loaded = SettingsState::load_from_dir(&temp_dir);
    assert_eq!(loaded, SettingsState::default());

    // 2. Save modified settings
    let modified = SettingsState {
        auto_backup: false,
        strict_hash: false,
        offline_first: false,
        compositor: "Sway (Wayland)".to_string(),
        shell: "Rofi (~/.config/rofi)".to_string(),
    };
    modified.save_to_dir(&temp_dir).expect("save settings");

    // 3. Reload and verify equality
    let reloaded = SettingsState::load_from_dir(&temp_dir);
    assert_eq!(reloaded, modified);

    // 4. Corrupted JSON file: verify safe fallback to default without panic
    let settings_file = temp_dir.join("settings.json");
    fs::write(&settings_file, "{ malformed: json, syntax: error !@#$% }").expect("write bad json");
    let fallback = SettingsState::load_from_dir(&temp_dir);
    assert_eq!(fallback, SettingsState::default());

    // 5. Truncated empty file: verify safe fallback
    fs::write(&settings_file, "").expect("write empty file");
    let fallback_empty = SettingsState::load_from_dir(&temp_dir);
    assert_eq!(fallback_empty, SettingsState::default());

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

// ============================================================================
// 4. Controller Callback Integration & State Machine
// ============================================================================

#[allow(dead_code)]
struct MockControllerState {
    all_entries: Vec<CatalogEntry>,
    filtered_entries: Vec<CatalogEntry>,
    selected_index: usize,
    settings: SettingsState,
}

#[test]
fn test_adversarial_controller_state_and_callbacks_integration() {
    setup_headless_env();
    let app = AppWindow::new().expect("Failed to instantiate AppWindow");

    let temp_data = std::env::temp_dir().join(format!("woderrise_test_ctrl_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_data);
    fs::create_dir_all(&temp_data).expect("create temp dir");

    // Open real SQLite database in temp dir
    let db = Database::open(temp_data.join("journal.db")).expect("open db");
    catalog::migrate(&db).expect("migrate");

    let reg_dir = PathBuf::from("registry/packages");
    if reg_dir.is_dir() {
        let _ = catalog::import_registry(&db, &reg_dir);
    }

    let all_entries = catalog::search(&db, "", 100).expect("search");
    let initial_count = all_entries.len();

    let state = Arc::new(Mutex::new(MockControllerState {
        all_entries: all_entries.clone(),
        filtered_entries: all_entries.clone(),
        selected_index: 0,
        settings: SettingsState::default(),
    }));

    // Bind cards model
    let installed_ids = std::collections::HashSet::new();
    let cards = catalog_entries_to_cards_model(&all_entries, &installed_ids);
    app.set_packages(cards);
    app.set_total_packages_count(initial_count as i32);

    // Test selection callback with bounds checks
    {
        let state_rc = Arc::clone(&state);
        let app_weak = app.as_weak();
        app.on_package_selected(move |idx: i32| {
            let Some(app) = app_weak.upgrade() else { return; };
            let mut st = state_rc.lock().unwrap();
            if idx < 0 || (idx as usize) >= st.filtered_entries.len() {
                return; // Bounds protected
            }
            st.selected_index = idx as usize;
            let entry = &st.filtered_entries[st.selected_index];
            app.set_sel_id(entry.id.clone().into());
            app.set_sel_name(entry.name.clone().into());
        });
    }

    // Select valid package
    if initial_count > 0 {
        app.invoke_package_selected(0);
        assert!(!app.get_sel_id().is_empty());

        // Negative index -> safely ignored
        let prev_id = app.get_sel_id().to_string();
        app.invoke_package_selected(-1);
        assert_eq!(app.get_sel_id(), prev_id);

        // Out-of-bounds positive index -> safely ignored
        app.invoke_package_selected(9999);
        assert_eq!(app.get_sel_id(), prev_id);
    }

    // Test toggle_setting callback
    {
        let state_rc = Arc::clone(&state);
        let data_dir = temp_data.clone();
        app.on_toggle_setting(move |key, val| {
            let mut st = state_rc.lock().unwrap();
            match key.as_str() {
                "auto_backup" => st.settings.auto_backup = val,
                "strict_hash" => st.settings.strict_hash = val,
                "offline_first" => st.settings.offline_first = val,
                _ => {}
            }
            let _ = st.settings.save_to_dir(&data_dir);
        });
    }

    app.invoke_toggle_setting("strict_hash".into(), false);
    assert!(!state.lock().unwrap().settings.strict_hash);

    // Verify saved to disk
    let loaded_from_disk = SettingsState::load_from_dir(&temp_data);
    assert!(!loaded_from_disk.strict_hash);

    // Cleanup
    let _ = fs::remove_dir_all(&temp_data);
}
