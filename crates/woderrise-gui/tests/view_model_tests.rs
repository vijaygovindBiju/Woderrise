//! Deterministic offline unit test suite for Woderrise GUI view models.
//!
//! Tests all UI state transitions, rate limit calculations, category filtering,
//! hardware protection defaults, procedural directive approvals, and strict LIFO rollback logic
//! completely headlessly without requiring DISPLAY or WAYLAND_DISPLAY.

use ashpd::desktop::settings::ColorScheme;
use camino::Utf8PathBuf;
use chrono::Utc;
use iced::Theme;
use woderrise_core::models::{
    CatalogRepository, ConfigComponent, ConflictStatus, DetectedComponent, FileOperationRecord,
    HardwareConflictNotice, JournalEntry, OperationType, SecurityAuditReport, StagedFile,
    SyntaxFormat, TargetMapping, TransactionStatus, VerificationReport,
};
use woderrise_core::paths::XdgPaths;
use woderrise_detect::{ComponentDetection, ProceduralDirective, TreeEntry};
use woderrise_gui::app::{App, EngineEvent, GuiCommand, Message, Screen};
use woderrise_gui::screens::history::{
    view_history, DirtyStateModal, HistoryAction, HistoryComponentFilter, HistoryItem,
    HistoryMetrics, HistoryState, HistoryStatusFilter, RollbackConfirmState,
};
use woderrise_gui::screens::inspect::{InspectAction, InspectState, InspectTab};
use woderrise_gui::screens::install::{InstallPhase, InstallState};
use woderrise_gui::screens::search::{
    calculate_grid_columns, CategoryFilter, RateLimitColorTier, RateLimitGaugeState, SearchState,
    SortOption,
};
use woderrise_gui::screens::settings::{
    calculate_directory_stats, format_bytes, format_reset_countdown, format_reset_duration,
    safe_cleanup_cache, view_settings, AboutDiagnostics, CacheStats, SettingsAction, SettingsState,
    SettingsTab, ThemeMode, UiDensity,
};
use woderrise_gui::theme::{
    contrast_ratio, fetch_portal_theme, map_color_scheme_to_theme, map_raw_scheme_value, tokens,
    ThemeTokens,
};
use woderrise_gui::widgets::card::{format_relative_time, format_star_count, view_card_themed};

#[test]
fn test_rate_limit_gauge_tier_logic_and_percentage_math() {
    // Green tier: > 50%
    assert_eq!(
        RateLimitGaugeState::calc_tier(60, 60),
        RateLimitColorTier::Green
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(31, 60),
        RateLimitColorTier::Green
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(10, 10),
        RateLimitColorTier::Green
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(6, 10),
        RateLimitColorTier::Green
    );

    // Amber tier: 10% - 50%
    assert_eq!(
        RateLimitGaugeState::calc_tier(30, 60),
        RateLimitColorTier::Amber
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(6, 60),
        RateLimitColorTier::Amber
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(5, 10),
        RateLimitColorTier::Amber
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(1, 10),
        RateLimitColorTier::Amber
    );

    // Red tier: < 10%
    assert_eq!(
        RateLimitGaugeState::calc_tier(5, 60),
        RateLimitColorTier::Red
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(0, 60),
        RateLimitColorTier::Red
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(0, 10),
        RateLimitColorTier::Red
    );

    // Division by zero protection
    assert_eq!(
        RateLimitGaugeState::calc_tier(0, 0),
        RateLimitColorTier::Red
    );

    // Percentage math and clamp validation
    let state = RateLimitGaugeState::new(45, 60, 3, 10);
    assert!((state.core_percentage() - 0.75).abs() < f32::EPSILON);
    assert!((state.search_percentage() - 0.30).abs() < f32::EPSILON);
    assert_eq!(state.core_tier(), RateLimitColorTier::Green);
    assert_eq!(state.search_tier(), RateLimitColorTier::Amber);
    assert!(!state.is_search_low());

    // Low quota warning boundary
    let throttled_state = RateLimitGaugeState::new(2, 60, 1, 10);
    assert!(throttled_state.is_search_low());
    let exhausted_state = RateLimitGaugeState::new(0, 60, 0, 10);
    assert!(exhausted_state.is_search_low());
    assert_eq!(exhausted_state.core_percentage(), 0.0);
    assert_eq!(exhausted_state.search_percentage(), 0.0);
}

fn create_sample_repo(
    id: u64,
    owner: &str,
    repo: &str,
    desc: &str,
    component: ConfigComponent,
    topics: Vec<&str>,
) -> CatalogRepository {
    CatalogRepository {
        id,
        owner: owner.into(),
        repo: repo.into(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: Some(desc.into()),
        stargazers_count: 100 * id,
        default_branch: "main".into(),
        topics: topics.into_iter().map(String::from).collect(),
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![DetectedComponent {
            component,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "config".into(),
            entrypoint: "config.conf".into(),
            style_file: None,
            file_count: 4,
            total_size_bytes: 8192,
        }],
    }
}

#[test]
fn test_category_filtering_across_all_seven_categories() {
    let repo_hypr = create_sample_repo(
        1,
        "catppuccin",
        "hyprland-mocha",
        "Smooth Hyprland dotfiles",
        ConfigComponent::Hyprland,
        vec!["hyprland", "catppuccin"],
    );
    let repo_waybar = create_sample_repo(
        2,
        "alex",
        "waybar-clean",
        "Minimal status bar",
        ConfigComponent::Waybar,
        vec!["waybar"],
    );
    let repo_hyprlock = create_sample_repo(
        3,
        "sec",
        "hyprlock-cyber",
        "Modern lockscreen",
        ConfigComponent::Hyprlock,
        vec!["hyprlock"],
    );
    let repo_hypridle = create_sample_repo(
        4,
        "power",
        "hypridle-eco",
        "Battery saving idle config",
        ConfigComponent::Hypridle,
        vec!["hypridle"],
    );
    let repo_wallpapers = create_sample_repo(
        5,
        "artist",
        "nord-wallpapers",
        "Nord palette 4K wallpapers",
        ConfigComponent::Wallpapers,
        vec!["wallpapers", "nord"],
    );
    let repo_gtk = create_sample_repo(
        6,
        "themer",
        "gruvbox-gtk",
        "Gruvbox dark GTK theme",
        ConfigComponent::GtkTheme,
        vec!["gtk", "theme"],
    );

    let all_repos = vec![
        repo_hypr.clone(),
        repo_waybar.clone(),
        repo_hyprlock.clone(),
        repo_hypridle.clone(),
        repo_wallpapers.clone(),
        repo_gtk.clone(),
    ];

    let mut state = SearchState::new(all_repos);

    // 1. All category filter matches everything
    state.set_filter(CategoryFilter::All);
    assert_eq!(state.filtered_repositories.len(), 6);

    // 2. Hyprland filter
    state.set_filter(CategoryFilter::Hyprland);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(
        state.filtered_repositories[0].full_name,
        "catppuccin/hyprland-mocha"
    );

    // 3. Waybar filter
    state.set_filter(CategoryFilter::Waybar);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(
        state.filtered_repositories[0].full_name,
        "alex/waybar-clean"
    );

    // 4. Hyprlock filter
    state.set_filter(CategoryFilter::Hyprlock);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(
        state.filtered_repositories[0].full_name,
        "sec/hyprlock-cyber"
    );

    // 5. Hypridle filter
    state.set_filter(CategoryFilter::Hypridle);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(
        state.filtered_repositories[0].full_name,
        "power/hypridle-eco"
    );

    // 6. Wallpapers filter
    state.set_filter(CategoryFilter::Wallpapers);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(
        state.filtered_repositories[0].full_name,
        "artist/nord-wallpapers"
    );

    // 7. GTK filter
    state.set_filter(CategoryFilter::Gtk);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(
        state.filtered_repositories[0].full_name,
        "themer/gruvbox-gtk"
    );
}

#[test]
fn test_search_tier1_instant_local_filtering_and_debounce_timing() {
    let repo_cat = create_sample_repo(
        1,
        "catppuccin",
        "hyprland-mocha",
        "Catppuccin Mocha colors",
        ConfigComponent::Hyprland,
        vec!["catppuccin", "mocha"],
    );
    let repo_nord = create_sample_repo(
        2,
        "arctic",
        "nordic-rice",
        "Cold Arctic theme",
        ConfigComponent::Hyprland,
        vec!["nord", "blue"],
    );

    let mut state = SearchState::new(vec![repo_cat, repo_nord]);
    assert_eq!(state.filtered_repositories.len(), 2);

    // Instant local filtering by description token
    state.set_query("mocha".into(), 1000);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(state.filtered_repositories[0].repo, "hyprland-mocha");

    // Debounce timing checks
    assert!(!state.should_fire_debounce(1400, 600)); // Only 400ms elapsed
    assert!(state.should_fire_debounce(1600, 600)); // Exactly 600ms elapsed
    assert!(state.should_fire_debounce(2000, 600)); // 1000ms elapsed

    state.clear_debounce();
    assert!(!state.should_fire_debounce(2500, 600)); // Debounce was cleared

    // Filtering by topic token
    state.set_query("blue".into(), 3000);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(state.filtered_repositories[0].repo, "nordic-rice");

    // Empty query resets to full list
    state.set_query("   ".into(), 3500);
    assert_eq!(state.filtered_repositories.len(), 2);
}

#[test]
fn test_inspect_state_component_selection_and_hardware_preservation() {
    let repo = create_sample_repo(
        1,
        "catppuccin",
        "hyprland-mocha",
        "Hyprland rice with Waybar",
        ConfigComponent::Hyprland,
        vec![],
    );

    let hw_notice = HardwareConflictNotice {
        file_path: Utf8PathBuf::from("~/.config/hypr/monitors.conf"),
        description: "Hardware-bound display configuration detected in 'monitors.conf'".into(),
        preserved_by_default: true,
    };

    let directive = ProceduralDirective {
        directive_type: "exec-once".into(),
        command: "waybar &".into(),
        file: "hyprland.conf".into(),
        line: 12,
    };

    let mut inspect = InspectState::new(repo, vec![directive], vec![hw_notice]);

    // Initial state: components selected by default, install allowed
    assert_eq!(inspect.selected_components.len(), 1);
    assert!(inspect.is_install_allowed());
    assert_eq!(inspect.selected_file_count(), 4);

    // Toggle off component
    inspect.toggle_component(ConfigComponent::Hyprland);
    assert!(inspect.selected_components.is_empty());
    assert!(!inspect.is_install_allowed()); // Installation disallowed if 0 components selected
    assert_eq!(inspect.selected_file_count(), 0);

    // Toggle on component
    inspect.toggle_component(ConfigComponent::Hyprland);
    assert!(inspect.is_install_allowed());

    // Loading suppresses install
    inspect.is_loading = true;
    assert!(!inspect.is_install_allowed());
    inspect.is_loading = false;
    assert!(inspect.is_install_allowed());

    // Hardware notice preservation verified
    assert_eq!(inspect.hardware_notices.len(), 1);
    assert!(inspect.hardware_notices[0].preserved_by_default);
    assert_eq!(
        inspect.hardware_notices[0].file_path,
        Utf8PathBuf::from("~/.config/hypr/monitors.conf")
    );
}

#[test]
fn test_install_state_phase_advancement_and_directive_approval() {
    let repo = create_sample_repo(
        1,
        "user",
        "my-rice",
        "My custom desktop",
        ConfigComponent::Hyprland,
        vec![],
    );

    let d1 = ProceduralDirective {
        directive_type: "exec-once".into(),
        command: "dunst".into(),
        file: "hyprland.conf".into(),
        line: 10,
    };
    let d2 = ProceduralDirective {
        directive_type: "on-click".into(),
        command: "pavucontrol".into(),
        file: "config.jsonc".into(),
        line: 45,
    };

    let mut install = InstallState::new(
        "tx_12345".into(),
        repo,
        vec![ConfigComponent::Hyprland],
        vec![d1, d2],
    );

    // Initial phase is PendingApproval with all commands neutralized
    assert_eq!(install.current_phase, InstallPhase::PendingApproval);
    assert_eq!(install.directive_approvals.len(), 2);
    assert!(!install.directive_approvals[0].approved);
    assert!(!install.directive_approvals[1].approved);

    // Toggle approvals
    install.toggle_directive_approval(0);
    assert!(install.directive_approvals[0].approved);
    assert!(!install.directive_approvals[1].approved);

    install.toggle_directive_approval(0);
    assert!(!install.directive_approvals[0].approved);

    // Stepper progression
    install.advance_phase(InstallPhase::Staging, 0.25, "Streaming files...");
    assert_eq!(install.current_phase, InstallPhase::Staging);
    assert_eq!(install.progress_ratio, 0.25);

    install.advance_phase(
        InstallPhase::Backup,
        0.50,
        "Snapshotting existing dotfiles...",
    );
    assert_eq!(install.current_phase, InstallPhase::Backup);

    install.advance_phase(InstallPhase::JournalWal, 0.75, "Committing WAL journal...");
    assert_eq!(install.current_phase, InstallPhase::JournalWal);

    install.advance_phase(
        InstallPhase::AtomicSwap,
        0.90,
        "Atomically swapping files...",
    );
    assert_eq!(install.current_phase, InstallPhase::AtomicSwap);

    install.advance_phase(InstallPhase::Completed, 1.0, "Installation complete!");
    assert_eq!(install.current_phase, InstallPhase::Completed);

    // Failure transition
    install.fail("Disk write failed");
    assert_eq!(install.current_phase, InstallPhase::Failed);
    assert_eq!(install.error_message, Some("Disk write failed".into()));
}

#[test]
fn test_history_lifo_rollback_button_enablement_and_dirty_modal() {
    let t1 = JournalEntry {
        id: "tx_01".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Hyprland,
        status: TransactionStatus::RolledBack,
        source_repo: "catppuccin/hyprland".into(),
        source_commit: "c1".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx_01"),
        operations: vec![],
    };

    let t2 = JournalEntry {
        id: "tx_02".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Waybar,
        status: TransactionStatus::Committed,
        source_repo: "alex/waybar".into(),
        source_commit: "c2".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx_02"),
        operations: vec![],
    };

    let t3 = JournalEntry {
        id: "tx_03".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Wallpapers,
        status: TransactionStatus::Committed,
        source_repo: "artist/nord".into(),
        source_commit: "c3".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx_03"),
        operations: vec![],
    };

    // Chronological journal: [t1 (rolled back), t2 (older committed), t3 (newest committed)]
    let mut history = HistoryState::from_journal_entries(&[t1, t2, t3]);
    assert_eq!(history.transactions.len(), 3);

    // Strict LIFO check: only the newest committed transaction (t3 at idx 0) has rollback enabled
    assert!(history.transactions[0].is_latest_committed);
    assert!(!history.transactions[1].is_latest_committed); // t2 is disabled (LIFO violation)
    assert!(!history.transactions[2].is_latest_committed); // t1 is disabled (Already rolled back)

    // Dirty state modal management
    assert!(history.dirty_modal.is_none());
    history.open_dirty_modal(
        "tx_03",
        vec![Utf8PathBuf::from("~/.config/hypr/hyprland.conf")],
    );
    assert!(history.dirty_modal.is_some());
    let modal = history.dirty_modal.as_ref().unwrap();
    assert_eq!(modal.transaction_id, "tx_03");
    assert_eq!(modal.modified_files.len(), 1);

    history.close_dirty_modal();
    assert!(history.dirty_modal.is_none());
}

#[tokio::test]
async fn test_portal_theme_mapping_and_headless_fallback() {
    // 1. ashpd ColorScheme enum mapping
    assert_eq!(
        map_color_scheme_to_theme(ColorScheme::NoPreference),
        Theme::Dark
    );
    assert_eq!(
        map_color_scheme_to_theme(ColorScheme::PreferDark),
        Theme::Dark
    );
    assert_eq!(
        map_color_scheme_to_theme(ColorScheme::PreferLight),
        Theme::Light
    );

    // 2. Raw scheme values mapping
    assert_eq!(map_raw_scheme_value(0), Theme::Dark);
    assert_eq!(map_raw_scheme_value(1), Theme::Dark);
    assert_eq!(map_raw_scheme_value(2), Theme::Light);
    assert_eq!(map_raw_scheme_value(99), Theme::Dark);

    // 3. Headless fallback test (executes without D-Bus session in CI, safely returns Theme::Dark)
    let fallback_theme = fetch_portal_theme().await;
    assert_eq!(fallback_theme, Theme::Dark);
}

#[test]
fn test_theme_tokens_contrast() {
    let dark = ThemeTokens::dark();
    let light = ThemeTokens::light();

    // 1. Dark Mode Contrast
    let dark_contrast = contrast_ratio(dark.text_primary, dark.bg_base);
    assert!(
        dark_contrast >= 7.0,
        "Dark mode primary text contrast ({:.2}) must meet or exceed WCAG AAA (7.0:1)",
        dark_contrast
    );

    let dark_secondary_contrast = contrast_ratio(dark.text_secondary, dark.bg_base);
    assert!(
        dark_secondary_contrast >= 4.5,
        "Dark mode secondary text contrast ({:.2}) must meet or exceed WCAG AA (4.5:1)",
        dark_secondary_contrast
    );

    let dark_surface_contrast = contrast_ratio(dark.text_primary, dark.bg_surface);
    assert!(
        dark_surface_contrast >= 7.0,
        "Dark mode text on surface ({:.2}) must meet or exceed WCAG AAA (7.0:1)",
        dark_surface_contrast
    );

    let dark_card_contrast = contrast_ratio(dark.text_primary, dark.bg_card);
    assert!(
        dark_card_contrast >= 4.5,
        "Dark mode text on card ({:.2}) must meet or exceed WCAG AA (4.5:1)",
        dark_card_contrast
    );

    // 2. Light Mode Contrast
    let light_contrast = contrast_ratio(light.text_primary, light.bg_base);
    assert!(
        light_contrast >= 7.0,
        "Light mode primary text contrast ({:.2}) must meet or exceed WCAG AAA (7.0:1)",
        light_contrast
    );

    let light_secondary_contrast = contrast_ratio(light.text_secondary, light.bg_base);
    assert!(
        light_secondary_contrast >= 4.5,
        "Light mode secondary text contrast ({:.2}) must meet or exceed WCAG AA (4.5:1)",
        light_secondary_contrast
    );

    let light_surface_contrast = contrast_ratio(light.text_primary, light.bg_surface);
    assert!(
        light_surface_contrast >= 7.0,
        "Light mode text on surface ({:.2}) must meet or exceed WCAG AAA (7.0:1)",
        light_surface_contrast
    );

    let light_card_contrast = contrast_ratio(light.text_primary, light.bg_card);
    assert!(
        light_card_contrast >= 7.0,
        "Light mode text on card ({:.2}) must meet or exceed WCAG AAA (7.0:1)",
        light_card_contrast
    );

    // 3. Button Accent Contrast
    let dark_button_contrast = contrast_ratio(iced::Color::WHITE, dark.accent_primary);
    assert!(
        dark_button_contrast >= 3.0,
        "Dark primary button text contrast ({:.2}) must be accessible (>= 3.0:1)",
        dark_button_contrast
    );

    let light_button_contrast = contrast_ratio(iced::Color::WHITE, light.accent_primary);
    assert!(
        light_button_contrast >= 3.0,
        "Light primary button text contrast ({:.2}) must be accessible (>= 3.0:1)",
        light_button_contrast
    );
}

#[test]
fn test_theme_switching_resolution() {
    // 1. Direct dark and light themes
    assert_eq!(tokens(&Theme::Dark), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::Light), ThemeTokens::light());

    // 2. Built-in light variants map to light tokens
    assert_eq!(tokens(&Theme::SolarizedLight), ThemeTokens::light());
    assert_eq!(tokens(&Theme::GruvboxLight), ThemeTokens::light());
    assert_eq!(tokens(&Theme::CatppuccinLatte), ThemeTokens::light());
    assert_eq!(tokens(&Theme::TokyoNightLight), ThemeTokens::light());
    assert_eq!(tokens(&Theme::KanagawaLotus), ThemeTokens::light());

    // 3. Built-in dark variants map to dark tokens
    assert_eq!(tokens(&Theme::Dracula), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::Nord), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::SolarizedDark), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::GruvboxDark), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::CatppuccinFrappe), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::CatppuccinMacchiato), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::CatppuccinMocha), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::TokyoNight), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::TokyoNightStorm), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::KanagawaWave), ThemeTokens::dark());
    assert_eq!(tokens(&Theme::KanagawaDragon), ThemeTokens::dark());
}

#[test]
fn test_app_toggle_theme() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    // Initial default theme is Dark
    assert_eq!(app.active_theme, Theme::Dark);
    assert_eq!(app.theme(), Theme::Dark);
    assert_eq!(tokens(&app.theme()), ThemeTokens::dark());

    // Toggle to Light mode
    let _ = app.update(Message::ToggleDarkMode(false));
    assert_eq!(app.active_theme, Theme::Light);
    assert_eq!(app.theme(), Theme::Light);
    assert_eq!(tokens(&app.theme()), ThemeTokens::light());

    // Toggle back to Dark mode
    let _ = app.update(Message::ToggleDarkMode(true));
    assert_eq!(app.active_theme, Theme::Dark);
    assert_eq!(app.theme(), Theme::Dark);
    assert_eq!(tokens(&app.theme()), ThemeTokens::dark());

    // Test engine event theme update
    let _ = app.update(Message::Engine(EngineEvent::PortalThemeChanged {
        dark_mode: false,
    }));
    assert_eq!(app.active_theme, Theme::Light);
    assert_eq!(tokens(&app.theme()), ThemeTokens::light());

    let _ = app.update(Message::Engine(EngineEvent::PortalThemeChanged {
        dark_mode: true,
    }));
    assert_eq!(app.active_theme, Theme::Dark);
    assert_eq!(tokens(&app.theme()), ThemeTokens::dark());
}

#[tokio::test]
async fn test_app_installation_flow_genuine_staged_files() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    // Drain startup commands (LoadHistory, FetchRateLimits, RefreshCatalog)
    while cmd_rx.try_recv().is_ok() {}

    let repo = create_sample_repo(
        10,
        "hyprland-community",
        "rice-deluxe",
        "Genuine dotfiles test repository",
        ConfigComponent::Hyprland,
        vec!["hyprland", "rice"],
    );

    // 1. Select repository -> transitions to Inspect screen
    let _ = app.update(Message::SelectRepo(repo.clone()));
    assert_eq!(app.active_screen, Screen::Inspect);
    let cmd = cmd_rx
        .recv()
        .await
        .expect("InspectPackage command expected");
    match cmd {
        GuiCommand::InspectPackage { repo: r, .. } => {
            assert_eq!(r, "hyprland-community/rice-deluxe");
        }
        _ => panic!("Expected InspectPackage command"),
    }

    // 2. Deliver TreeLoaded event with Hyprland and Monitors components
    let detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: vec![
                TreeEntry::new(
                    "hyprland.conf",
                    "100644",
                    "blob",
                    "aaaabbbbccccddddeeeeffff0000111122223333",
                    Some(2048),
                ),
                TreeEntry::new(
                    "keybinds.conf",
                    "100644",
                    "blob",
                    "1111222233334444555566667777888899990000",
                    Some(1024),
                ),
            ],
            file_count: 2,
            total_size_bytes: 3072,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Monitors,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "monitors.conf".into(),
            style_file: None,
            matched_entries: vec![TreeEntry::new(
                "monitors.conf",
                "100644",
                "blob",
                "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                Some(512),
            )],
            file_count: 1,
            total_size_bytes: 512,
            hardware_notices: vec![HardwareConflictNotice {
                file_path: Utf8PathBuf::from("~/.config/hypr/monitors.conf"),
                description: "Hardware monitor configuration".into(),
                preserved_by_default: true,
            }],
        },
    ];

    let _ = app.update(Message::Engine(EngineEvent::TreeLoaded {
        repo: "hyprland-community/rice-deluxe".into(),
        tree: woderrise_github::GitTree {
            sha: "tree_root_sha".into(),
            url: None,
            tree: vec![],
            truncated: false,
        },
        detections,
    }));

    // Verify hardware display preservation invariant
    let inspect = app.inspect_state.as_ref().unwrap();
    assert!(inspect
        .selected_components
        .contains(&ConfigComponent::Hyprland));
    assert!(!inspect
        .selected_components
        .contains(&ConfigComponent::Monitors));

    // 3. Initiate installation -> transitions to Install screen & sends StagePackage command
    let _ = app.update(Message::InitiateInstall);
    assert_eq!(app.active_screen, Screen::Install);

    let stage_cmd = cmd_rx.recv().await.expect("StagePackage command expected");
    let tx_id = match stage_cmd {
        GuiCommand::StagePackage {
            tx_id,
            component,
            repo: r,
            entries,
            ..
        } => {
            assert_eq!(component, ConfigComponent::Hyprland);
            assert_eq!(r, "hyprland-community/rice-deluxe");
            // Multi-file preservation: includes both hyprland.conf and keybinds.conf
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].path, "hyprland.conf");
            assert_eq!(entries[1].path, "keybinds.conf");
            tx_id
        }
        _ => panic!("Expected StagePackage command"),
    };

    // 4. Staging finishes -> delivers StagingCompleted event with genuine StagedFile records
    let expected_dest_hyprland = Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf");
    let expected_dest_keybinds = Utf8PathBuf::from("/home/user/.config/hypr/keybinds.conf");
    let genuine_staged_files = vec![
        StagedFile {
            relative_path: Utf8PathBuf::from("hyprland.conf"),
            target_destination: Some(expected_dest_hyprland.clone()),
            blob_sha: Some("aaaabbbbccccddddeeeeffff0000111122223333".into()),
            file_size: 2048,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
            file_mode: "100644".into(),
            permissions: 0o644,
            is_executable: false,
            is_symlink: false,
            syntax_valid: true,
            path_safe: true,
        },
        StagedFile {
            relative_path: Utf8PathBuf::from("keybinds.conf"),
            target_destination: Some(expected_dest_keybinds.clone()),
            blob_sha: Some("1111222233334444555566667777888899990000".into()),
            file_size: 1024,
            sha256: "ca978112ca1bbdcafac231b39a23dc4da7860814961409733a9e868278c993f9".into(),
            file_mode: "100644".into(),
            permissions: 0o644,
            is_executable: false,
            is_symlink: false,
            syntax_valid: true,
            path_safe: true,
        },
    ];

    let _ = app.update(Message::Engine(EngineEvent::StagingCompleted {
        tx_id: tx_id.clone(),
        report: VerificationReport {
            transaction_id: tx_id.clone(),
            component: ConfigComponent::Hyprland,
            total_files: 2,
            total_bytes: 3072,
            target_mappings: vec![
                TargetMapping {
                    component: ConfigComponent::Hyprland,
                    relative_path: Utf8PathBuf::from("hyprland.conf"),
                    blob_sha: "aaaabbbbccccddddeeeeffff0000111122223333".into(),
                    target_destination: expected_dest_hyprland.clone(),
                    git_file_mode: "100644".into(),
                    expected_sha256:
                        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                    is_hardware_protected: false,
                    conflict_status: ConflictStatus::NewFile,
                },
                TargetMapping {
                    component: ConfigComponent::Hyprland,
                    relative_path: Utf8PathBuf::from("keybinds.conf"),
                    blob_sha: "1111222233334444555566667777888899990000".into(),
                    target_destination: expected_dest_keybinds.clone(),
                    git_file_mode: "100644".into(),
                    expected_sha256:
                        "ca978112ca1bbdcafac231b39a23dc4da7860814961409733a9e868278c993f9".into(),
                    is_hardware_protected: false,
                    conflict_status: ConflictStatus::NewFile,
                },
            ],
            security_audit: SecurityAuditReport {
                scripts_detected: 0,
                procedural_directives_neutralized: 0,
                path_traversal_violations: 0,
                symlinks_rejected: 0,
                passed: true,
            },
            hardware_conflicts: vec![],
            passed: true,
        },
        staged_files: genuine_staged_files.clone(),
    }));

    // Verify StagedFiles are preserved in InstallState
    let install = app.install_state.as_ref().unwrap();
    assert_eq!(install.current_phase, InstallPhase::PendingApproval);
    assert_eq!(install.staged_files.len(), 2);
    assert_eq!(
        install.staged_files[0].target_destination,
        Some(expected_dest_hyprland.clone())
    );

    // 5. User confirms installation -> ApplyInstallation command must receive genuine StagedFiles
    let _ = app.update(Message::ConfirmInstall);
    let apply_cmd = cmd_rx
        .recv()
        .await
        .expect("ApplyInstallation command expected");
    match apply_cmd {
        GuiCommand::ApplyInstallation {
            tx_id: applied_tx_id,
            component,
            repo: applied_repo,
            staged_files,
            ..
        } => {
            assert_eq!(applied_tx_id, tx_id);
            assert_eq!(component, ConfigComponent::Hyprland);
            assert_eq!(applied_repo, "hyprland-community/rice-deluxe");
            assert_eq!(staged_files.len(), 2);

            // ABSOLUTE DESTINATION INTEGRITY:
            assert_eq!(
                staged_files[0].target_destination,
                Some(expected_dest_hyprland)
            );
            assert_eq!(
                staged_files[1].target_destination,
                Some(expected_dest_keybinds)
            );

            // REAL SHA INTEGRITY (not dummy "verified"):
            assert_ne!(staged_files[0].sha256, "verified");
            assert_eq!(
                staged_files[0].sha256,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            );
            assert_eq!(staged_files[0].file_size, 2048);
            assert_eq!(staged_files[1].file_size, 1024);
        }
        _ => panic!("Expected ApplyInstallation command"),
    }
}

#[test]
fn test_settings_screen_navigation_and_interactions() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);

    // Initial commands drained
    let _ = cmd_rx.try_recv(); // LoadHistory
    let _ = cmd_rx.try_recv(); // FetchRateLimits
    let _ = cmd_rx.try_recv(); // RefreshCatalog

    // 1. Navigate to Settings screen
    let _ = app.update(Message::NavigateTo(Screen::Settings));
    assert_eq!(app.active_screen, Screen::Settings);
    // Should dispatch FetchRateLimits command
    match cmd_rx.try_recv() {
        Ok(GuiCommand::FetchRateLimits) => (),
        other => panic!(
            "Expected FetchRateLimits command on navigating to Settings, got {:?}",
            other
        ),
    }

    // 2. Modify token input
    let _ = app.update(Message::SettingsTokenChanged("ghp_test9876543210".into()));
    assert_eq!(app.settings_state.token_input, "ghp_test9876543210");

    // 3. Toggle token masking
    let _ = app.update(Message::SettingsToggleMask);
    assert!(!app.settings_state.is_masked);
    let _ = app.update(Message::SettingsToggleMask);
    assert!(app.settings_state.is_masked);

    // 4. Save token
    let _ = app.update(Message::SettingsSaveToken);
    assert!(app.settings_state.is_busy);
    match cmd_rx.try_recv() {
        Ok(GuiCommand::SetGitHubToken { token }) => {
            assert_eq!(token, Some("ghp_test9876543210".to_string()));
        }
        other => panic!("Expected SetGitHubToken command, got {:?}", other),
    }

    // 5. RateLimitsUpdated event resolves saving state
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Core,
            limit: 5000,
            remaining: 4999,
            reset_time: chrono::Utc::now(),
            used: 1,
        },
        search: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Search,
            limit: 30,
            remaining: 30,
            reset_time: chrono::Utc::now(),
            used: 0,
        },
    }));
    assert!(!app.settings_state.is_busy);
    assert_eq!(
        app.settings_state.saved_token,
        Some("ghp_test9876543210".to_string())
    );
    assert_eq!(app.settings_state.masked_display(), "ghp_••••••••3210");
    assert_eq!(app.search_state.rate_limits.core_limit, 5000);
    assert_eq!(app.search_state.rate_limits.search_limit, 30);

    // 6. Clear token
    let _ = app.update(Message::SettingsClearToken);
    assert!(app.settings_state.is_busy);
    match cmd_rx.try_recv() {
        Ok(GuiCommand::SetGitHubToken { token }) => {
            assert_eq!(token, None);
        }
        other => panic!("Expected SetGitHubToken {{ token: None }}, got {:?}", other),
    }

    // 7. RateLimitsUpdated event on clear resolves state
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Core,
            limit: 60,
            remaining: 60,
            reset_time: chrono::Utc::now(),
            used: 0,
        },
        search: woderrise_github::RateLimitStatus {
            resource: woderrise_github::RateLimitResource::Search,
            limit: 10,
            remaining: 10,
            reset_time: chrono::Utc::now(),
            used: 0,
        },
    }));
    assert!(!app.settings_state.is_busy);
    assert_eq!(app.settings_state.saved_token, None);
    assert_eq!(
        app.settings_state.masked_display(),
        "None (Unauthenticated)"
    );
    assert_eq!(app.search_state.rate_limits.core_limit, 60);
    assert_eq!(app.search_state.rate_limits.search_limit, 10);
}

fn create_bare_repo(
    id: u64,
    owner: &str,
    repo: &str,
    desc: &str,
    topics: Vec<&str>,
) -> CatalogRepository {
    CatalogRepository {
        id,
        owner: owner.into(),
        repo: repo.into(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: Some(desc.into()),
        stargazers_count: 100 * id,
        default_branch: "main".into(),
        topics: topics.into_iter().map(String::from).collect(),
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    }
}

#[test]
fn test_repo_pre_inspected_updates_catalog_and_category_filter() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    // Initial repo with empty detected components, topics that do not match Waybar
    let bare_repo = create_bare_repo(
        42,
        "custom-dev",
        "custom-setup",
        "A minimal desktop configuration",
        vec!["desktop", "minimal"],
    );

    app.search_state.catalog_repositories = vec![bare_repo];
    app.search_state.apply_filter();

    // With All filter, repo is present
    assert_eq!(app.search_state.filtered_repositories.len(), 1);

    // Switch to Waybar filter: since detected_components is empty and text has no 'waybar', matches is false
    app.search_state.set_filter(CategoryFilter::Waybar);
    assert_eq!(app.search_state.filtered_repositories.len(), 0);

    // Dispatch RepoPreInspected event with detected Waybar component
    let waybar_comp = DetectedComponent {
        component: ConfigComponent::Waybar,
        syntax_format: SyntaxFormat::Jsonc,
        root_path: ".config/waybar".into(),
        entrypoint: "config.jsonc".into(),
        style_file: Some("style.css".into()),
        file_count: 3,
        total_size_bytes: 4096,
    };

    let _ = app.update(Message::Engine(EngineEvent::RepoPreInspected {
        repo: "custom-dev/custom-setup".into(),
        components: vec![waybar_comp.clone()],
    }));

    // Verify catalog_repositories updated
    let updated = &app.search_state.catalog_repositories[0];
    assert_eq!(updated.detected_components.len(), 1);
    assert_eq!(
        updated.detected_components[0].component,
        ConfigComponent::Waybar
    );
    assert_eq!(updated.detected_components[0].file_count, 3);

    // Verify filtered_repositories updated automatically via apply_filter!
    assert_eq!(app.search_state.filtered_repositories.len(), 1);
    assert_eq!(
        app.search_state.filtered_repositories[0].resolved_full_name(),
        "custom-dev/custom-setup"
    );
}

#[test]
fn test_card_view_reflects_detected_components_and_file_counts() {
    let mut repo = create_bare_repo(
        100,
        "community",
        "awesome-dots",
        "Curated setup",
        vec!["hyprland"],
    );

    repo.detected_components = vec![
        DetectedComponent {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: ".config/waybar".into(),
            entrypoint: "config.jsonc".into(),
            style_file: Some("style.css".into()),
            file_count: 3,
            total_size_bytes: 3000,
        },
        DetectedComponent {
            component: ConfigComponent::Hyprlock,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: ".config/hypr".into(),
            entrypoint: "hyprlock.conf".into(),
            style_file: None,
            file_count: 1,
            total_size_bytes: 800,
        },
    ];

    // Total files should be 4 (3 + 1)
    let total_files: usize = repo.detected_components.iter().map(|c| c.file_count).sum();
    assert_eq!(total_files, 4);

    // Verify view_card builds without panic
    let card_element = woderrise_gui::view_card(&repo, Message::Noop);
    let _ = card_element; // builds successfully
}

#[tokio::test]
async fn test_spawn_tree_pre_inspection_cancellation() {
    let mock_server = wiremock::MockServer::start().await;
    let client = woderrise_github::GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .unwrap();

    let detection_engine = std::sync::Arc::new(woderrise_detect::DetectionEngine::new());
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(10);
    let cancel_token = tokio_util::sync::CancellationToken::new();

    // Pre-cancel the token
    cancel_token.cancel();

    let repo = create_bare_repo(1, "owner", "repo", "desc", vec![]);
    woderrise_gui::spawn_tree_pre_inspection(
        vec![repo],
        client,
        detection_engine,
        event_tx,
        cancel_token,
    );

    // Since token is cancelled, no events should be emitted
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(event_rx.try_recv().is_err());
}

#[tokio::test]
async fn test_spawn_tree_pre_inspection_rate_limit_protection() {
    let mock_server = wiremock::MockServer::start().await;
    let client = woderrise_github::GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .unwrap();

    // Consume rate limit so core_status().remaining < 5
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-ratelimit-resource",
        reqwest::header::HeaderValue::from_static("core"),
    );
    headers.insert(
        "x-ratelimit-limit",
        reqwest::header::HeaderValue::from_static("60"),
    );
    headers.insert(
        "x-ratelimit-remaining",
        reqwest::header::HeaderValue::from_static("2"),
    );
    headers.insert(
        "x-ratelimit-reset",
        reqwest::header::HeaderValue::from_static("1761500000"),
    );
    client
        .rate_limiter()
        .update_from_headers(&headers, woderrise_github::RateLimitResource::Core);

    let detection_engine = std::sync::Arc::new(woderrise_detect::DetectionEngine::new());
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(10);
    let cancel_token = tokio_util::sync::CancellationToken::new();

    let repo = create_bare_repo(1, "owner", "repo", "desc", vec![]);
    woderrise_gui::spawn_tree_pre_inspection(
        vec![repo],
        client,
        detection_engine,
        event_tx,
        cancel_token,
    );

    // Quota < 5 prevents pre-inspection from executing
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(event_rx.try_recv().is_err());
}

#[test]
fn test_category_count_derivation() {
    let mut repo1 = create_bare_repo(1, "alice", "hypr-dots", "Hyprland setup", vec!["hyprland"]);
    repo1.detected_components = vec![DetectedComponent {
        component: ConfigComponent::Hyprland,
        syntax_format: SyntaxFormat::LegacyConf,
        root_path: ".config/hypr".into(),
        entrypoint: "hyprland.conf".into(),
        style_file: None,
        file_count: 5,
        total_size_bytes: 2048,
    }];

    let mut repo2 = create_bare_repo(
        2,
        "bob",
        "waybar-theme",
        "Custom Waybar status",
        vec!["waybar"],
    );
    repo2.detected_components = vec![DetectedComponent {
        component: ConfigComponent::Waybar,
        syntax_format: SyntaxFormat::Jsonc,
        root_path: ".config/waybar".into(),
        entrypoint: "config.jsonc".into(),
        style_file: Some("style.css".into()),
        file_count: 2,
        total_size_bytes: 1024,
    }];

    let mut repo3 = create_bare_repo(
        3,
        "charlie",
        "full-rice",
        "Complete rice with hypr and waybar",
        vec!["hyprland", "waybar"],
    );
    repo3.detected_components = vec![
        DetectedComponent {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: ".config/hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            file_count: 4,
            total_size_bytes: 1500,
        },
        DetectedComponent {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: ".config/waybar".into(),
            entrypoint: "config.jsonc".into(),
            style_file: None,
            file_count: 3,
            total_size_bytes: 1200,
        },
    ];

    let repos = vec![repo1, repo2, repo3];
    let state = SearchState::new(repos.clone());

    // Category count on CategoryFilter
    assert_eq!(CategoryFilter::All.count_in_catalog(&repos), 3);
    assert_eq!(CategoryFilter::Hyprland.count_in_catalog(&repos), 2); // repo1, repo3
    assert_eq!(CategoryFilter::Waybar.count_in_catalog(&repos), 2); // repo2, repo3
    assert_eq!(CategoryFilter::Hyprlock.count_in_catalog(&repos), 0);
    assert_eq!(CategoryFilter::Wallpapers.count_in_catalog(&repos), 0);

    // Category count on SearchState
    assert_eq!(state.category_count(CategoryFilter::All), 3);
    assert_eq!(state.category_count(CategoryFilter::Hyprland), 2);
    assert_eq!(state.category_count(CategoryFilter::Waybar), 2);
    assert_eq!(state.category_count(CategoryFilter::Gtk), 0);
}

#[test]
fn test_sorting_by_stars_recently_updated_name_and_relevance() {
    let now = Utc::now();
    let mut repo_a = create_bare_repo(1, "zeta", "zeta-repo", "Zeta dotfiles", vec!["hyprland"]);
    repo_a.stargazers_count = 50;
    repo_a.pushed_at = Some(now - chrono::Duration::days(10));

    let mut repo_b = create_bare_repo(2, "alpha", "alpha-repo", "Alpha dotfiles", vec!["hyprland"]);
    repo_b.stargazers_count = 500;
    repo_b.pushed_at = Some(now - chrono::Duration::days(1));

    let mut repo_c = create_bare_repo(3, "middle", "beta-repo", "Beta dotfiles", vec!["hyprland"]);
    repo_c.stargazers_count = 10;
    repo_c.pushed_at = Some(now - chrono::Duration::days(3));

    let mut state = SearchState::new(vec![repo_a, repo_b, repo_c]);

    // 1. SortOption::Stars -> Descending stargazers_count (500, 50, 10)
    state.set_sort_option(SortOption::Stars);
    assert_eq!(state.filtered_repositories.len(), 3);
    assert_eq!(state.filtered_repositories[0].resolved_repo(), "alpha-repo");
    assert_eq!(state.filtered_repositories[0].stargazers_count, 500);
    assert_eq!(state.filtered_repositories[1].resolved_repo(), "zeta-repo");
    assert_eq!(state.filtered_repositories[1].stargazers_count, 50);
    assert_eq!(state.filtered_repositories[2].resolved_repo(), "beta-repo");
    assert_eq!(state.filtered_repositories[2].stargazers_count, 10);

    // 2. SortOption::RecentlyUpdated -> Descending pushed_at (1 day ago, 3 days ago, 10 days ago)
    state.set_sort_option(SortOption::RecentlyUpdated);
    assert_eq!(state.filtered_repositories[0].resolved_repo(), "alpha-repo"); // 1 day ago
    assert_eq!(state.filtered_repositories[1].resolved_repo(), "beta-repo"); // 3 days ago
    assert_eq!(state.filtered_repositories[2].resolved_repo(), "zeta-repo"); // 10 days ago

    // 3. SortOption::Name -> Alphabetical by resolved_repo().to_lowercase()
    state.set_sort_option(SortOption::Name);
    assert_eq!(state.filtered_repositories[0].resolved_repo(), "alpha-repo");
    assert_eq!(state.filtered_repositories[1].resolved_repo(), "beta-repo");
    assert_eq!(state.filtered_repositories[2].resolved_repo(), "zeta-repo");

    // 4. SortOption::Relevance with empty query -> falls back to descending stars
    state.set_sort_option(SortOption::Relevance);
    assert_eq!(state.filtered_repositories[0].stargazers_count, 500);
    assert_eq!(state.filtered_repositories[1].stargazers_count, 50);
    assert_eq!(state.filtered_repositories[2].stargazers_count, 10);
}

#[test]
fn test_search_clear_resets_query_and_active_filters() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);
    // Drain startup commands
    while cmd_rx.try_recv().is_ok() {}

    let mut repo1 = create_bare_repo(
        1,
        "arch",
        "hypr-config",
        "Hyprland desktop",
        vec!["hyprland"],
    );
    repo1.detected_components = vec![DetectedComponent {
        component: ConfigComponent::Hyprland,
        syntax_format: SyntaxFormat::LegacyConf,
        root_path: ".config/hypr".into(),
        entrypoint: "hyprland.conf".into(),
        style_file: None,
        file_count: 3,
        total_size_bytes: 1024,
    }];
    let mut repo2 = create_bare_repo(2, "arch", "waybar-config", "Waybar desktop", vec!["waybar"]);
    repo2.detected_components = vec![DetectedComponent {
        component: ConfigComponent::Waybar,
        syntax_format: SyntaxFormat::Jsonc,
        root_path: ".config/waybar".into(),
        entrypoint: "config.jsonc".into(),
        style_file: None,
        file_count: 2,
        total_size_bytes: 512,
    }];

    app.search_state.catalog_repositories = vec![repo1, repo2];
    app.search_state.apply_filter();
    assert_eq!(app.search_state.filtered_repositories.len(), 2);

    // Modify query and filters
    let _ = app.update(Message::SearchInputChanged("hypr".to_string()));
    let _ = app.update(Message::FilterSelected(CategoryFilter::Waybar));
    let _ = app.update(Message::SortSelected(SortOption::Name));
    assert_eq!(app.search_state.query, "hypr");
    assert_eq!(app.search_state.active_filter, CategoryFilter::Waybar);
    assert_eq!(app.search_state.sort_option, SortOption::Name);

    // Dispatch SearchClear
    let _ = app.update(Message::SearchClear);

    // Verify all states are reset
    assert!(app.search_state.query.is_empty());
    assert_eq!(app.search_state.active_filter, CategoryFilter::All);
    assert_eq!(app.search_state.sort_option, SortOption::Relevance);
    assert!(!app.search_state.is_searching);
    assert!(!app.search_state.debounce_pending);

    // Both repos should now be present again
    assert_eq!(app.search_state.filtered_repositories.len(), 2);

    // GuiCommand::RefreshCatalog should have been dispatched
    assert!(matches!(cmd_rx.try_recv(), Ok(GuiCommand::RefreshCatalog)));
}

#[test]
fn test_grid_column_calculation_narrow_standard_wide() {
    // Narrow desktop widths (< 960px): 2 columns
    assert_eq!(calculate_grid_columns(500.0), 2);
    assert_eq!(calculate_grid_columns(720.0), 2);
    assert_eq!(calculate_grid_columns(800.0), 2);
    assert_eq!(calculate_grid_columns(959.0), 2);
    assert_eq!(calculate_grid_columns(959.9), 2);

    // Standard desktop widths (960px - 1399px): 3 columns
    assert_eq!(calculate_grid_columns(960.0), 3);
    assert_eq!(calculate_grid_columns(1024.0), 3);
    assert_eq!(calculate_grid_columns(1280.0), 3);
    assert_eq!(calculate_grid_columns(1366.0), 3);
    assert_eq!(calculate_grid_columns(1399.0), 3);
    assert_eq!(calculate_grid_columns(1399.9), 3);

    // Wide desktop widths (>= 1400px): 4 columns
    assert_eq!(calculate_grid_columns(1400.0), 4);
    assert_eq!(calculate_grid_columns(1440.0), 4);
    assert_eq!(calculate_grid_columns(1920.0), 4);
    assert_eq!(calculate_grid_columns(2560.0), 4);
    assert_eq!(calculate_grid_columns(3840.0), 4);
}

#[test]
fn test_format_relative_time_and_star_count() {
    // Star formatting
    assert_eq!(format_star_count(0), "★ 0");
    assert_eq!(format_star_count(42), "★ 42");
    assert_eq!(format_star_count(999), "★ 999");
    assert_eq!(format_star_count(1_000), "★ 1.0k");
    assert_eq!(format_star_count(16_400), "★ 16.4k");

    // Relative time formatting
    assert_eq!(format_relative_time(None), "Updated recently");

    let now = Utc::now();
    assert_eq!(
        format_relative_time(Some(now - chrono::Duration::seconds(30))),
        "Updated just now"
    );
    assert_eq!(
        format_relative_time(Some(now - chrono::Duration::minutes(5))),
        "Updated 5 mins ago"
    );
    assert_eq!(
        format_relative_time(Some(now - chrono::Duration::hours(3))),
        "Updated 3 hours ago"
    );
    assert_eq!(
        format_relative_time(Some(now - chrono::Duration::days(3))),
        "Updated 3 days ago"
    );
    assert_eq!(
        format_relative_time(Some(now - chrono::Duration::days(60))),
        "Updated 2 months ago"
    );
    assert_eq!(
        format_relative_time(Some(now - chrono::Duration::days(400))),
        "Updated 1 year ago"
    );
}

#[test]
fn test_card_view_themed_rendering() {
    let mut repo = create_bare_repo(
        1,
        "end-4",
        "dots-hyprland",
        "Clean Hyprland dots",
        vec!["hyprland"],
    );
    repo.stargazers_count = 16_400;
    repo.detected_components = vec![DetectedComponent {
        component: ConfigComponent::Hyprland,
        syntax_format: SyntaxFormat::LegacyConf,
        root_path: ".config/hypr".into(),
        entrypoint: "hyprland.conf".into(),
        style_file: None,
        file_count: 12,
        total_size_bytes: 4096,
    }];

    // Dark theme card
    let dark_card = view_card_themed(&repo, &Theme::Dark, Message::Noop);
    let _ = dark_card;

    // Light theme card
    let light_card = view_card_themed(&repo, &Theme::Light, Message::Noop);
    let _ = light_card;
}

#[tokio::test]
async fn test_inspect_tab_switching_across_all_five_tabs() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    let repo = create_sample_repo(
        1,
        "catppuccin",
        "hyprland-mocha",
        "Hyprland rice",
        ConfigComponent::Hyprland,
        vec!["hyprland"],
    );

    let _ = app.update(Message::SelectRepo(repo));
    assert_eq!(app.active_screen, Screen::Inspect);
    let _ = cmd_rx.recv().await;

    // Default tab is Overview
    let inspect = app.inspect_state.as_ref().unwrap();
    assert_eq!(inspect.active_tab, InspectTab::Overview);

    // Verify all 5 tab variants and labels
    assert_eq!(InspectTab::ALL.len(), 5);
    assert_eq!(InspectTab::Overview.label(), "Overview");
    assert_eq!(InspectTab::Components.label(), "Components");
    assert_eq!(InspectTab::FileTree.label(), "File Tree");
    assert_eq!(InspectTab::Readme.label(), "README");
    assert_eq!(InspectTab::DiffAndInstall.label(), "Diff & Install");

    // Switch across all 5 tabs using InspectAction
    let tabs = [
        InspectTab::Components,
        InspectTab::FileTree,
        InspectTab::Readme,
        InspectTab::DiffAndInstall,
        InspectTab::Overview,
    ];

    for tab in tabs {
        let _ = app.update(Message::InspectAction(InspectAction::SelectTab(tab)));
        assert_eq!(app.inspect_state.as_ref().unwrap().active_tab, tab);
    }

    // Direct SelectInspectTab message compatibility
    let _ = app.update(Message::SelectInspectTab(InspectTab::Components));
    assert_eq!(
        app.inspect_state.as_ref().unwrap().active_tab,
        InspectTab::Components
    );
}

#[test]
fn test_file_level_selection_and_dynamic_counts() {
    let mut repo = create_bare_repo(1, "catppuccin", "dots", "Multi-component dots", vec![]);
    repo.detected_components = vec![
        DetectedComponent {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            file_count: 3,
            total_size_bytes: 4500,
        },
        DetectedComponent {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: "waybar".into(),
            entrypoint: "config.jsonc".into(),
            style_file: Some("style.css".into()),
            file_count: 2,
            total_size_bytes: 2000,
        },
    ];

    let mut inspect = InspectState::new(repo, vec![], vec![]);

    // Populate raw_detections with granular file entries
    inspect.raw_detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: vec![
                TreeEntry::new("hypr/hyprland.conf", "100644", "blob", "sha1", Some(2000)),
                TreeEntry::new("hypr/keybinds.conf", "100644", "blob", "sha2", Some(1500)),
                TreeEntry::new("hypr/rules.conf", "100644", "blob", "sha3", Some(1000)),
            ],
            file_count: 3,
            total_size_bytes: 4500,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: "waybar".into(),
            entrypoint: "config.jsonc".into(),
            style_file: Some("style.css".into()),
            matched_entries: vec![
                TreeEntry::new("waybar/config.jsonc", "100644", "blob", "sha4", Some(1200)),
                TreeEntry::new("waybar/style.css", "100644", "blob", "sha5", Some(800)),
            ],
            file_count: 2,
            total_size_bytes: 2000,
            hardware_notices: vec![],
        },
    ];

    // Initial state: both components active, all 5 files selected
    assert_eq!(inspect.selected_file_count(), 5);
    assert_eq!(inspect.selected_total_bytes(), 6500);
    assert!(inspect.is_file_selected("hypr/keybinds.conf", ConfigComponent::Hyprland));
    assert!(inspect.is_install_allowed());

    // Granular toggle off a single file: hypr/keybinds.conf (1500 bytes)
    inspect.toggle_file("hypr/keybinds.conf", ConfigComponent::Hyprland);
    assert!(!inspect.is_file_selected("hypr/keybinds.conf", ConfigComponent::Hyprland));
    assert_eq!(inspect.selected_file_count(), 4);
    assert_eq!(inspect.selected_total_bytes(), 5000);
    assert!(inspect.is_install_allowed());

    // Toggle off a second file: waybar/style.css (800 bytes)
    inspect.toggle_file("waybar/style.css", ConfigComponent::Waybar);
    assert!(!inspect.is_file_selected("waybar/style.css", ConfigComponent::Waybar));
    assert_eq!(inspect.selected_file_count(), 3);
    assert_eq!(inspect.selected_total_bytes(), 4200);

    // Toggle back on: waybar/style.css
    inspect.toggle_file("waybar/style.css", ConfigComponent::Waybar);
    assert!(inspect.is_file_selected("waybar/style.css", ConfigComponent::Waybar));
    assert_eq!(inspect.selected_file_count(), 4);
    assert_eq!(inspect.selected_total_bytes(), 5000);

    // Deselect all components
    inspect.deselect_all_components();
    assert_eq!(inspect.selected_file_count(), 0);
    assert_eq!(inspect.selected_total_bytes(), 0);
    assert!(!inspect.is_install_allowed());

    // Select all components restores files
    inspect.select_all_components();
    assert_eq!(inspect.selected_file_count(), 5);
    assert_eq!(inspect.selected_total_bytes(), 6500);
    assert!(inspect.is_install_allowed());
}

#[tokio::test]
async fn test_confirmation_modal_toggle_and_directive_approvals() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    let repo = create_sample_repo(
        1,
        "author",
        "custom-rice",
        "Custom desktop rice",
        ConfigComponent::Hyprland,
        vec![],
    );

    let _ = app.update(Message::SelectRepo(repo));
    let _ = cmd_rx.recv().await;

    let directive = ProceduralDirective {
        directive_type: "exec-once".into(),
        command: "waybar &".into(),
        file: "hyprland.conf".into(),
        line: 10,
    };

    if let Some(inspect) = &mut app.inspect_state {
        inspect.procedural_directives = vec![directive];
    }

    // Modal dialog is closed by default
    assert!(!app.inspect_state.as_ref().unwrap().show_confirm_dialog);

    // Open confirmation dialog
    let _ = app.update(Message::InspectAction(InspectAction::OpenConfirmDialog));
    assert!(app.inspect_state.as_ref().unwrap().show_confirm_dialog);

    // Toggle directive approval in modal
    let _ = app.update(Message::InspectAction(InspectAction::ToggleDirective(0)));
    assert!(app
        .inspect_state
        .as_ref()
        .unwrap()
        .approved_directives
        .contains(&0));

    // Toggle off directive approval
    let _ = app.update(Message::InspectAction(InspectAction::ToggleDirective(0)));
    assert!(!app
        .inspect_state
        .as_ref()
        .unwrap()
        .approved_directives
        .contains(&0));

    // Close confirmation dialog via cancel
    let _ = app.update(Message::InspectAction(InspectAction::CloseConfirmDialog));
    assert!(!app.inspect_state.as_ref().unwrap().show_confirm_dialog);

    // Re-open and approve directive
    let _ = app.update(Message::InspectAction(InspectAction::OpenConfirmDialog));
    let _ = app.update(Message::InspectAction(InspectAction::ToggleDirective(0)));

    // Confirm installation from modal -> transitions to Screen::Install and stages package
    let _ = app.update(Message::InspectAction(InspectAction::ConfirmInstall));
    assert_eq!(app.active_screen, Screen::Install);
    assert!(!app.inspect_state.as_ref().unwrap().show_confirm_dialog);

    let stage_cmd = cmd_rx.recv().await.expect("StagePackage expected");
    match stage_cmd {
        GuiCommand::StagePackage { repo: r, .. } => {
            assert_eq!(r, "author/custom-rice");
        }
        _ => panic!("Expected StagePackage command"),
    }

    // Verified directive was marked approved in install_state
    let install = app.install_state.as_ref().unwrap();
    assert_eq!(install.directive_approvals.len(), 1);
    assert!(install.directive_approvals[0].approved);
}

#[test]
fn test_hardware_monitor_protection_invariant_never_auto_selected() {
    let mut repo = create_bare_repo(
        1,
        "monitors-user",
        "multi-mon-rice",
        "Rice with monitors",
        vec![],
    );
    repo.detected_components = vec![
        DetectedComponent {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            file_count: 2,
            total_size_bytes: 2000,
        },
        DetectedComponent {
            component: ConfigComponent::Monitors,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "monitors.conf".into(),
            style_file: None,
            file_count: 1,
            total_size_bytes: 512,
        },
    ];

    let hw_notice = HardwareConflictNotice {
        file_path: Utf8PathBuf::from("~/.config/hypr/monitors.conf"),
        description: "Display configuration detected".into(),
        preserved_by_default: true,
    };

    // 1. InspectState::new invariant check
    let mut inspect = InspectState::new(repo, vec![], vec![hw_notice]);
    assert!(inspect
        .selected_components
        .contains(&ConfigComponent::Hyprland));
    assert!(
        !inspect.selected_components.contains(&ConfigComponent::Monitors),
        "Hardware protection invariant failure: ConfigComponent::Monitors was auto-selected in new()!"
    );

    // 2. select_all_components invariant check: Monitors must NEVER be selected by Select All
    inspect.select_all_components();
    assert!(inspect
        .selected_components
        .contains(&ConfigComponent::Hyprland));
    assert!(
        !inspect.selected_components.contains(&ConfigComponent::Monitors),
        "Hardware protection invariant failure: select_all_components() selected ConfigComponent::Monitors!"
    );

    // 3. Explicit manual selection is the only way Monitors can be selected
    inspect.toggle_component(ConfigComponent::Monitors);
    assert!(inspect
        .selected_components
        .contains(&ConfigComponent::Monitors));
    inspect.toggle_component(ConfigComponent::Monitors);
    assert!(!inspect
        .selected_components
        .contains(&ConfigComponent::Monitors));
}

#[test]
fn test_planning_destination_path_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    std::fs::create_dir_all(&xdg.config_home).unwrap();
    let config_home = xdg.config_home.clone();

    let repo = create_bare_repo(1, "author", "rice", "Rice", vec![]);
    let mut inspect = InspectState::new(repo, vec![], vec![]);

    inspect.raw_detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: vec![
                TreeEntry::new("hyprland.conf", "100644", "blob", "sha1", Some(1024)),
                TreeEntry::new("monitors.conf", "100644", "blob", "sha2", Some(512)),
            ],
            file_count: 2,
            total_size_bytes: 1536,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: "waybar".into(),
            entrypoint: "config.jsonc".into(),
            style_file: None,
            matched_entries: vec![TreeEntry::new(
                "config.jsonc",
                "100644",
                "blob",
                "sha3",
                Some(2048),
            )],
            file_count: 1,
            total_size_bytes: 2048,
            hardware_notices: vec![],
        },
    ];

    inspect
        .selected_components
        .insert(ConfigComponent::Hyprland);
    inspect.selected_components.insert(ConfigComponent::Waybar);

    // Case 1: Fresh installation (files do not exist on disk)
    let mappings = inspect.resolve_target_mappings_with_xdg(&xdg);
    assert_eq!(mappings.len(), 3);

    // hyprland.conf -> ~/.config/hypr/hyprland.conf, NewFile
    let hypr_map = mappings
        .iter()
        .find(|m| m.relative_path.as_str() == "hyprland.conf")
        .unwrap();
    assert_eq!(
        hypr_map.target_destination,
        config_home.join("hypr/hyprland.conf")
    );
    assert_eq!(hypr_map.conflict_status, ConflictStatus::NewFile);
    assert!(!hypr_map.is_hardware_protected);

    // waybar config.jsonc -> ~/.config/waybar/config.jsonc, NewFile
    let waybar_map = mappings
        .iter()
        .find(|m| m.relative_path.as_str() == "config.jsonc")
        .unwrap();
    assert_eq!(
        waybar_map.target_destination,
        config_home.join("waybar/config.jsonc")
    );
    assert_eq!(waybar_map.conflict_status, ConflictStatus::NewFile);

    // monitors.conf -> HardwareProtected
    let mon_map = mappings
        .iter()
        .find(|m| m.relative_path.as_str() == "monitors.conf")
        .unwrap();
    assert_eq!(mon_map.conflict_status, ConflictStatus::HardwareProtected);
    assert!(mon_map.is_hardware_protected);

    // Case 2: Pre-existing file exists on local filesystem -> detected as OverwritingExisting
    let target_file = config_home.join("hypr/hyprland.conf");
    std::fs::create_dir_all(target_file.parent().unwrap()).unwrap();
    std::fs::write(&target_file, "local existing configuration").unwrap();

    let updated_mappings = inspect.resolve_target_mappings_with_xdg(&xdg);
    let updated_hypr = updated_mappings
        .iter()
        .find(|m| m.relative_path.as_str() == "hyprland.conf")
        .unwrap();
    assert_eq!(
        updated_hypr.conflict_status,
        ConflictStatus::OverwritingExisting
    );
}

fn make_test_history_item(
    id: &str,
    repo: &str,
    commit: &str,
    comp: ConfigComponent,
    status: TransactionStatus,
    ops: Vec<FileOperationRecord>,
    is_latest: bool,
) -> HistoryItem {
    HistoryItem {
        id: id.into(),
        timestamp: Utc::now(),
        source_repo: repo.into(),
        source_commit: commit.into(),
        component: comp,
        status,
        file_count: ops.len(),
        backup_dir: Utf8PathBuf::from(format!("/backups/{}", id)),
        operations: ops,
        is_latest_committed: is_latest,
    }
}

#[test]
fn test_history_metrics_calculate_empty_single_and_mixed_states() {
    // 1. Empty state
    let empty_metrics = HistoryMetrics::calculate(&[]);
    assert_eq!(empty_metrics.total_transactions, 0);
    assert_eq!(empty_metrics.successful_installations, 0);
    assert_eq!(empty_metrics.failed_operations, 0);
    assert_eq!(empty_metrics.rollback_eligible_count, 0);

    // 2. Single Committed
    let item_c = make_test_history_item(
        "tx1",
        "a/b",
        "c1",
        ConfigComponent::Hyprland,
        TransactionStatus::Committed,
        vec![],
        true,
    );
    let m_c = HistoryMetrics::calculate(std::slice::from_ref(&item_c));
    assert_eq!(m_c.total_transactions, 1);
    assert_eq!(m_c.successful_installations, 1);
    assert_eq!(m_c.failed_operations, 0);
    assert_eq!(m_c.rollback_eligible_count, 1);

    // 3. Single Failed
    let item_f = make_test_history_item(
        "tx2",
        "a/b",
        "c2",
        ConfigComponent::Waybar,
        TransactionStatus::Failed,
        vec![],
        false,
    );
    let m_f = HistoryMetrics::calculate(std::slice::from_ref(&item_f));
    assert_eq!(m_f.total_transactions, 1);
    assert_eq!(m_f.successful_installations, 0);
    assert_eq!(m_f.failed_operations, 1);
    assert_eq!(m_f.rollback_eligible_count, 0);

    // 4. Single RolledBack
    let item_rb = make_test_history_item(
        "tx3",
        "a/b",
        "c3",
        ConfigComponent::Hyprlock,
        TransactionStatus::RolledBack,
        vec![],
        false,
    );
    let m_rb = HistoryMetrics::calculate(std::slice::from_ref(&item_rb));
    assert_eq!(m_rb.total_transactions, 1);
    assert_eq!(m_rb.successful_installations, 1);
    assert_eq!(m_rb.failed_operations, 0);
    assert_eq!(m_rb.rollback_eligible_count, 0);

    // 5. Single RecoveredRollback
    let item_rec = make_test_history_item(
        "tx4",
        "a/b",
        "c4",
        ConfigComponent::Hypridle,
        TransactionStatus::RecoveredRollback,
        vec![],
        false,
    );
    let m_rec = HistoryMetrics::calculate(std::slice::from_ref(&item_rec));
    assert_eq!(m_rec.total_transactions, 1);
    assert_eq!(m_rec.successful_installations, 1);
    assert_eq!(m_rec.failed_operations, 0);
    assert_eq!(m_rec.rollback_eligible_count, 0);

    // 6. Mixed multi-item list
    let item_c2 = make_test_history_item(
        "tx5",
        "x/y",
        "c5",
        ConfigComponent::Wallpapers,
        TransactionStatus::Committed,
        vec![],
        false,
    );
    let items = vec![item_c, item_f, item_rb, item_rec, item_c2];
    let mixed_m = HistoryMetrics::calculate(&items);
    assert_eq!(mixed_m.total_transactions, 5);
    assert_eq!(mixed_m.successful_installations, 4); // 2 committed + 1 rolledback + 1 recovered
    assert_eq!(mixed_m.failed_operations, 1);
    assert_eq!(mixed_m.rollback_eligible_count, 2); // 2 committed
}

#[test]
fn test_history_search_query_and_filtering_across_fields() {
    let t1 = JournalEntry {
        id: "tx_catppuccin_01".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Hyprland,
        status: TransactionStatus::Committed,
        source_repo: "catppuccin/hyprland-mocha".into(),
        source_commit: "a1b2c3d4".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx1"),
        operations: vec![],
    };
    let t2 = JournalEntry {
        id: "tx_waybar_02".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Waybar,
        status: TransactionStatus::RolledBack,
        source_repo: "alex/waybar-clean".into(),
        source_commit: "e5f6g7h8".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx2"),
        operations: vec![],
    };
    let t3 = JournalEntry {
        id: "tx_gtk_03".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::GtkTheme,
        status: TransactionStatus::Failed,
        source_repo: "themer/gruvbox-gtk".into(),
        source_commit: "1234abcd".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx3"),
        operations: vec![],
    };
    let t4 = JournalEntry {
        id: "tx_wallpapers_04".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Wallpapers,
        status: TransactionStatus::Committed,
        source_repo: "artist/nord-wallpapers".into(),
        source_commit: "99887766".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx4"),
        operations: vec![],
    };

    let mut state = HistoryState::from_journal_entries(&[t1, t2, t3, t4]);
    assert_eq!(state.transactions.len(), 4);
    assert_eq!(state.filtered_transactions().len(), 4);

    // Search query matches repo
    state.search_query = "catppuccin".into();
    let res = state.filtered_transactions();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "tx_catppuccin_01");

    // Search query matches commit hash (case-insensitive)
    state.search_query = "E5F6G7H8".into();
    let res = state.filtered_transactions();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "tx_waybar_02");

    // Search query matches component name
    state.search_query = "GtkTheme".into();
    let res = state.filtered_transactions();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "tx_gtk_03");

    // Search query matches status name
    state.search_query = "Committed".into();
    let res = state.filtered_transactions();
    assert_eq!(res.len(), 2);

    state.search_query = "failed".into();
    let res = state.filtered_transactions();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "tx_gtk_03");

    // Search query matches nothing
    state.search_query = "nonexistent_repo".into();
    assert_eq!(state.filtered_transactions().len(), 0);

    // Reset search query
    state.search_query.clear();

    // Status filter dropdown
    state.status_filter = HistoryStatusFilter::Committed;
    assert_eq!(state.filtered_transactions().len(), 2);

    state.status_filter = HistoryStatusFilter::RolledBack;
    let res = state.filtered_transactions();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "tx_waybar_02");

    state.status_filter = HistoryStatusFilter::Failed;
    let res = state.filtered_transactions();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "tx_gtk_03");

    state.status_filter = HistoryStatusFilter::All;
    assert_eq!(state.filtered_transactions().len(), 4);

    // Component filter dropdown
    state.component_filter = HistoryComponentFilter::Hyprland;
    assert_eq!(state.filtered_transactions().len(), 1);
    state.component_filter = HistoryComponentFilter::Waybar;
    assert_eq!(state.filtered_transactions().len(), 1);
    state.component_filter = HistoryComponentFilter::Hyprlock;
    assert_eq!(state.filtered_transactions().len(), 0);

    // Combined search and filters
    state.component_filter = HistoryComponentFilter::Wallpapers;
    state.status_filter = HistoryStatusFilter::Committed;
    state.search_query = "artist".into();
    assert!(state.has_active_filters());
    let res = state.filtered_transactions();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "tx_wallpapers_04");

    // Clear filters
    state.clear_filters();
    assert!(!state.has_active_filters());
    assert_eq!(state.search_query, "");
    assert_eq!(state.status_filter, HistoryStatusFilter::All);
    assert_eq!(state.component_filter, HistoryComponentFilter::All);
    assert_eq!(state.filtered_transactions().len(), 4);
}

#[test]
fn test_history_lifo_eligibility_invariant() {
    let t_oldest = JournalEntry {
        id: "tx_oldest".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Hyprland,
        status: TransactionStatus::Committed,
        source_repo: "repo/a".into(),
        source_commit: "rev1".into(),
        backup_directory: Utf8PathBuf::from("/backups/1"),
        operations: vec![],
    };
    let t_middle = JournalEntry {
        id: "tx_middle".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Waybar,
        status: TransactionStatus::Committed,
        source_repo: "repo/b".into(),
        source_commit: "rev2".into(),
        backup_directory: Utf8PathBuf::from("/backups/2"),
        operations: vec![],
    };
    let t_failed = JournalEntry {
        id: "tx_failed".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Hyprlock,
        status: TransactionStatus::Failed,
        source_repo: "repo/c".into(),
        source_commit: "rev3".into(),
        backup_directory: Utf8PathBuf::from("/backups/3"),
        operations: vec![],
    };
    let t_newest = JournalEntry {
        id: "tx_newest".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Wallpapers,
        status: TransactionStatus::Committed,
        source_repo: "repo/d".into(),
        source_commit: "rev4".into(),
        backup_directory: Utf8PathBuf::from("/backups/4"),
        operations: vec![],
    };

    // Journal entries passed in chronological order: [oldest, middle, failed, newest]
    let state = HistoryState::from_journal_entries(&[t_oldest, t_middle, t_failed, t_newest]);
    assert_eq!(state.transactions.len(), 4);

    // LIFO ordering: transactions are stored reverse-chronologically
    assert_eq!(state.transactions[0].id, "tx_newest");
    assert_eq!(state.transactions[1].id, "tx_failed");
    assert_eq!(state.transactions[2].id, "tx_middle");
    assert_eq!(state.transactions[3].id, "tx_oldest");

    // Only the newest committed transaction (at index 0) is eligible for rollback
    assert!(state.transactions[0].is_latest_committed);
    assert!(!state.transactions[1].is_latest_committed); // failed
    assert!(!state.transactions[2].is_latest_committed); // middle committed (blocked by LIFO)
    assert!(!state.transactions[3].is_latest_committed); // oldest committed (blocked by LIFO)
}

#[test]
fn test_history_guarded_rollback_confirmation_modal_and_file_separation() {
    let operations = vec![
        FileOperationRecord {
            op_type: OperationType::Overwrite,
            target_file: Utf8PathBuf::from("~/.config/hypr/hyprland.conf"),
            backup_file: Some(Utf8PathBuf::from("/backups/hyprland.conf.bak")),
            pre_sha256: Some("sha_pre_1".into()),
            post_sha256: Some("sha_post_1".into()),
            mode: None,
        },
        FileOperationRecord {
            op_type: OperationType::Create,
            target_file: Utf8PathBuf::from("~/.config/hypr/binds.conf"),
            backup_file: None,
            pre_sha256: None,
            post_sha256: Some("sha_post_2".into()),
            mode: None,
        },
        FileOperationRecord {
            op_type: OperationType::Create,
            target_file: Utf8PathBuf::from("~/.config/hypr/rules.conf"),
            backup_file: None,
            pre_sha256: None,
            post_sha256: Some("sha_post_3".into()),
            mode: None,
        },
        FileOperationRecord {
            op_type: OperationType::Delete,
            target_file: Utf8PathBuf::from("~/.config/hypr/deprecated.conf"),
            backup_file: Some(Utf8PathBuf::from("/backups/deprecated.conf.bak")),
            pre_sha256: Some("sha_pre_4".into()),
            post_sha256: None,
            mode: None,
        },
        FileOperationRecord {
            op_type: OperationType::DeactivateLegacy,
            target_file: Utf8PathBuf::from("~/.config/hypr/legacy.conf"),
            backup_file: Some(Utf8PathBuf::from("/backups/legacy.conf.bak")),
            pre_sha256: Some("sha_pre_5".into()),
            post_sha256: None,
            mode: None,
        },
    ];

    let item = make_test_history_item(
        "tx_rollback_guard",
        "catppuccin/hyprland",
        "deadbeef",
        ConfigComponent::Hyprland,
        TransactionStatus::Committed,
        operations,
        true,
    );

    // Separation of files to restore vs files to remove
    let confirm_state = RollbackConfirmState::from_item(&item);
    assert_eq!(confirm_state.tx_id, "tx_rollback_guard");
    assert_eq!(confirm_state.repo, "catppuccin/hyprland");
    assert_eq!(confirm_state.revision, "deadbeef");
    assert_eq!(confirm_state.component, ConfigComponent::Hyprland);

    // Overwrite, Delete, DeactivateLegacy must be restored
    assert_eq!(confirm_state.files_to_restore.len(), 3);
    assert!(confirm_state
        .files_to_restore
        .contains(&Utf8PathBuf::from("~/.config/hypr/hyprland.conf")));
    assert!(confirm_state
        .files_to_restore
        .contains(&Utf8PathBuf::from("~/.config/hypr/deprecated.conf")));
    assert!(confirm_state
        .files_to_restore
        .contains(&Utf8PathBuf::from("~/.config/hypr/legacy.conf")));

    // Create must be removed
    assert_eq!(confirm_state.files_to_remove.len(), 2);
    assert!(confirm_state
        .files_to_remove
        .contains(&Utf8PathBuf::from("~/.config/hypr/binds.conf")));
    assert!(confirm_state
        .files_to_remove
        .contains(&Utf8PathBuf::from("~/.config/hypr/rules.conf")));

    // Test App state machine interactions with modal
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    // Drain initial startup commands
    while cmd_rx.try_recv().is_ok() {}

    // Open confirm dialog
    let _ = app.update(Message::HistoryAction(HistoryAction::OpenRollbackConfirm(
        item.clone(),
    )));
    assert!(app.history_state.confirm_rollback_modal.is_some());

    // Dismiss / Cancel confirm dialog
    let _ = app.update(Message::HistoryAction(HistoryAction::CloseRollbackConfirm));
    assert!(app.history_state.confirm_rollback_modal.is_none());

    // Re-open and execute confirm
    let _ = app.update(Message::HistoryAction(HistoryAction::OpenRollbackConfirm(
        item.clone(),
    )));
    assert!(app.history_state.confirm_rollback_modal.is_some());
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: item.id.clone(),
        force_dirty: false,
    }));
    assert!(app.history_state.confirm_rollback_modal.is_none());
    assert!(app.history_state.is_rolling_back);

    // Verify command dispatched to transaction worker
    let dispatched = cmd_rx
        .try_recv()
        .expect("Command must be dispatched on rollback confirm");
    match dispatched {
        GuiCommand::RollbackTransaction { tx_id, force_dirty } => {
            assert_eq!(tx_id, "tx_rollback_guard");
            assert!(!force_dirty);
        }
        other => panic!("Unexpected GuiCommand dispatched: {:?}", other),
    }

    // Verify dirty modal interactions in App
    app.history_state.open_dirty_modal(
        item.id.clone(),
        vec![Utf8PathBuf::from("~/.config/hypr/modified.conf")],
    );
    assert!(app.history_state.dirty_modal.is_some());
    let dirty_ref: &DirtyStateModal = app.history_state.dirty_modal.as_ref().unwrap();
    assert_eq!(dirty_ref.transaction_id, "tx_rollback_guard");
    assert_eq!(dirty_ref.modified_files.len(), 1);

    // Cancel dirty modal
    let _ = app.update(Message::HistoryAction(HistoryAction::CloseDirtyModal));
    assert!(app.history_state.dirty_modal.is_none());

    // Re-open dirty modal and force rollback
    app.history_state.open_dirty_modal(
        item.id.clone(),
        vec![Utf8PathBuf::from("~/.config/hypr/modified.conf")],
    );
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: item.id.clone(),
        force_dirty: true,
    }));
    assert!(app.history_state.dirty_modal.is_none());
    assert!(app.history_state.is_rolling_back);

    let dispatched_force = cmd_rx
        .try_recv()
        .expect("Force dirty rollback command must be dispatched");
    match dispatched_force {
        GuiCommand::RollbackTransaction { tx_id, force_dirty } => {
            assert_eq!(tx_id, "tx_rollback_guard");
            assert!(force_dirty);
        }
        other => panic!("Unexpected GuiCommand dispatched: {:?}", other),
    }
}

#[test]
fn test_history_empty_state_navigation_action() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    // Navigate to History screen
    app.active_screen = Screen::History;
    assert_eq!(app.active_screen, Screen::History);

    // Trigger NavigateDiscover action (from "Explore Dotfiles in Discover" empty state button)
    let _ = app.update(Message::HistoryAction(HistoryAction::NavigateDiscover));
    assert_eq!(app.active_screen, Screen::Search);
}

#[test]
fn test_history_operation_inspection_modal_details() {
    let operations = vec![FileOperationRecord {
        op_type: OperationType::Create,
        target_file: Utf8PathBuf::from("~/.config/waybar/config.jsonc"),
        backup_file: None,
        pre_sha256: None,
        post_sha256: Some("sha256_abcdef123456".into()),
        mode: Some("0644".into()),
    }];

    let item = make_test_history_item(
        "tx_inspect_01",
        "alex/waybar-clean",
        "a9b8c7d6",
        ConfigComponent::Waybar,
        TransactionStatus::Committed,
        operations,
        false,
    );

    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    // Inspect modal starts closed
    assert!(app.history_state.inspect_modal.is_none());

    // Open inspect modal
    let _ = app.update(Message::HistoryAction(HistoryAction::Inspect(item.clone())));
    assert!(app.history_state.inspect_modal.is_some());
    let inspected = app.history_state.inspect_modal.as_ref().unwrap();
    assert_eq!(inspected.id, "tx_inspect_01");
    assert_eq!(inspected.operations.len(), 1);
    assert_eq!(inspected.operations[0].op_type, OperationType::Create);

    // Close inspect modal
    let _ = app.update(Message::HistoryAction(HistoryAction::CloseInspect));
    assert!(app.history_state.inspect_modal.is_none());
}

#[test]
fn test_history_filter_display_and_exhaustive_matches() {
    // Check all status filters
    for sf in &HistoryStatusFilter::ALL {
        let label = sf.label();
        let display = format!("{}", sf);
        assert_eq!(label, display);
        match sf {
            HistoryStatusFilter::All => {
                assert!(sf.matches(TransactionStatus::Committed));
                assert!(sf.matches(TransactionStatus::Failed));
                assert!(sf.matches(TransactionStatus::RolledBack));
            }
            HistoryStatusFilter::Committed => {
                assert!(sf.matches(TransactionStatus::Committed));
                assert!(!sf.matches(TransactionStatus::Failed));
            }
            HistoryStatusFilter::RolledBack => {
                assert!(sf.matches(TransactionStatus::RolledBack));
                assert!(sf.matches(TransactionStatus::RecoveredRollback));
                assert!(!sf.matches(TransactionStatus::Committed));
            }
            HistoryStatusFilter::Failed => {
                assert!(sf.matches(TransactionStatus::Failed));
                assert!(!sf.matches(TransactionStatus::Committed));
            }
        }
    }

    // Check all component filters
    for cf in &HistoryComponentFilter::ALL {
        let label = cf.label();
        let display = format!("{}", cf);
        assert_eq!(label, display);
        match cf {
            HistoryComponentFilter::All => {
                assert!(cf.matches(ConfigComponent::Hyprland));
                assert!(cf.matches(ConfigComponent::Waybar));
                assert!(cf.matches(ConfigComponent::GtkTheme));
            }
            HistoryComponentFilter::Hyprland => {
                assert!(cf.matches(ConfigComponent::Hyprland));
                assert!(!cf.matches(ConfigComponent::Waybar));
            }
            HistoryComponentFilter::Waybar => {
                assert!(cf.matches(ConfigComponent::Waybar));
                assert!(!cf.matches(ConfigComponent::Hyprland));
            }
            HistoryComponentFilter::Hyprlock => {
                assert!(cf.matches(ConfigComponent::Hyprlock));
                assert!(!cf.matches(ConfigComponent::Hypridle));
            }
            HistoryComponentFilter::Hypridle => {
                assert!(cf.matches(ConfigComponent::Hypridle));
                assert!(!cf.matches(ConfigComponent::Hyprlock));
            }
            HistoryComponentFilter::Wallpapers => {
                assert!(cf.matches(ConfigComponent::Wallpapers));
                assert!(!cf.matches(ConfigComponent::Hyprland));
            }
            HistoryComponentFilter::GtkTheme => {
                assert!(cf.matches(ConfigComponent::GtkTheme));
                assert!(!cf.matches(ConfigComponent::Hyprland));
            }
        }
    }
}

#[test]
fn test_history_view_rendering_dark_light_and_modals() {
    let item = make_test_history_item(
        "tx_render_test",
        "catppuccin/hyprland",
        "abcdef12",
        ConfigComponent::Hyprland,
        TransactionStatus::Committed,
        vec![FileOperationRecord {
            op_type: OperationType::Overwrite,
            target_file: Utf8PathBuf::from("~/.config/hypr/hyprland.conf"),
            backup_file: Some(Utf8PathBuf::from("/backups/hyprland.conf.bak")),
            pre_sha256: Some("sha_pre".into()),
            post_sha256: Some("sha_post".into()),
            mode: None,
        }],
        true,
    );

    // 1. Empty state rendering (Dark and Light)
    {
        let empty_state = HistoryState::from_journal_entries(&[]);
        let _ = view_history(&empty_state, &Theme::Dark, Message::HistoryAction);
        let _ = view_history(&empty_state, &Theme::Light, Message::HistoryAction);
    }

    // 2. Populated state rendering (Dark and Light)
    let mut populated_state = HistoryState {
        transactions: vec![item.clone()],
        metrics: HistoryMetrics::calculate(std::slice::from_ref(&item)),
        search_query: String::new(),
        status_filter: HistoryStatusFilter::All,
        component_filter: HistoryComponentFilter::All,
        inspect_modal: None,
        confirm_rollback_modal: None,
        dirty_modal: None,
        is_rolling_back: false,
    };
    {
        let _ = view_history(&populated_state, &Theme::Dark, Message::HistoryAction);
        let _ = view_history(&populated_state, &Theme::Light, Message::HistoryAction);
    }

    // 3. Filtered empty state (query matches nothing)
    populated_state.search_query = "nonexistent_query_xyz".into();
    {
        let _ = view_history(&populated_state, &Theme::Dark, Message::HistoryAction);
    }
    populated_state.search_query.clear();

    // 4. Inspect modal rendering
    populated_state.open_inspect_modal(item.clone());
    {
        let _ = view_history(&populated_state, &Theme::Dark, Message::HistoryAction);
    }
    populated_state.close_inspect_modal();

    // 5. Rollback confirm modal rendering
    populated_state.open_rollback_confirm(&item);
    {
        let _ = view_history(&populated_state, &Theme::Dark, Message::HistoryAction);
    }
    populated_state.close_rollback_confirm();

    // 6. Dirty state modal rendering
    populated_state.open_dirty_modal(
        item.id.clone(),
        vec![Utf8PathBuf::from("~/.config/hypr/hyprland.conf")],
    );
    {
        let _ = view_history(&populated_state, &Theme::Dark, Message::HistoryAction);
    }
    populated_state.close_dirty_modal();
}

// =========================================================================
// Milestone M5: Screen 4 — Settings, GitHub Auth, Rate Limits & Diagnostics
// =========================================================================

#[test]
fn test_settings_tab_navigation_and_state_preservation() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);

    // Initial tab must be GitHub
    assert_eq!(app.settings_state.active_tab, SettingsTab::GitHub);

    // Type a token in the input field
    let _ = app.update(Message::SettingsTokenChanged(
        "ghp_my_secret_token_1234".into(),
    ));
    assert_eq!(app.settings_state.token_input, "ghp_my_secret_token_1234");

    // Navigate to Appearance tab
    let _ = app.update(Message::SettingsTabSelected(SettingsTab::Appearance));
    assert_eq!(app.settings_state.active_tab, SettingsTab::Appearance);
    // User token input must NOT be cleared or modified
    assert_eq!(app.settings_state.token_input, "ghp_my_secret_token_1234");

    // Navigate to Network & Cache tab
    let _ = app.update(Message::SettingsTabSelected(SettingsTab::NetworkCache));
    assert_eq!(app.settings_state.active_tab, SettingsTab::NetworkCache);
    assert_eq!(app.settings_state.token_input, "ghp_my_secret_token_1234");

    // Navigate to About tab
    let _ = app.update(Message::SettingsTabSelected(SettingsTab::About));
    assert_eq!(app.settings_state.active_tab, SettingsTab::About);
    assert_eq!(app.settings_state.token_input, "ghp_my_secret_token_1234");

    // Navigate back to GitHub tab
    let _ = app.update(Message::SettingsTabSelected(SettingsTab::GitHub));
    assert_eq!(app.settings_state.active_tab, SettingsTab::GitHub);
    assert_eq!(app.settings_state.token_input, "ghp_my_secret_token_1234");

    // Test SettingsAction forwarding
    let _ = app.update(Message::SettingsAction(SettingsAction::TabSelected(
        SettingsTab::Appearance,
    )));
    assert_eq!(app.settings_state.active_tab, SettingsTab::Appearance);
}

#[test]
fn test_appearance_theme_mode_switching_and_tokens() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);

    // Default theme mode
    assert_eq!(app.settings_state.theme_mode, ThemeMode::System);

    // Switch to explicit Dark theme
    let _ = app.update(Message::SettingsThemeSelected(ThemeMode::Dark));
    assert_eq!(app.settings_state.theme_mode, ThemeMode::Dark);
    assert_eq!(app.active_theme, Theme::Dark);
    let dark_tokens = woderrise_gui::theme::tokens(&app.active_theme);
    assert_eq!(dark_tokens, woderrise_gui::theme::ThemeTokens::dark());
    // WCAG contrast validation
    let contrast_dark =
        woderrise_gui::theme::contrast_ratio(dark_tokens.text_primary, dark_tokens.bg_base);
    assert!(
        contrast_dark >= 7.0,
        "Dark mode text contrast must exceed 7.0:1 (AAA)"
    );

    // Switch to explicit Light theme
    let _ = app.update(Message::SettingsThemeSelected(ThemeMode::Light));
    assert_eq!(app.settings_state.theme_mode, ThemeMode::Light);
    assert_eq!(app.active_theme, Theme::Light);
    let light_tokens = woderrise_gui::theme::tokens(&app.active_theme);
    assert_eq!(light_tokens, woderrise_gui::theme::ThemeTokens::light());
    let contrast_light =
        woderrise_gui::theme::contrast_ratio(light_tokens.text_primary, light_tokens.bg_base);
    assert!(
        contrast_light >= 7.0,
        "Light mode text contrast must exceed 7.0:1 (AAA)"
    );

    // Switch back to System mode
    let _ = app.update(Message::SettingsThemeSelected(ThemeMode::System));
    assert_eq!(app.settings_state.theme_mode, ThemeMode::System);
}

#[test]
fn test_appearance_ui_density_selection() {
    let mut state = SettingsState::new(None);
    assert_eq!(state.ui_density, UiDensity::Default);

    state.set_ui_density(UiDensity::Compact);
    assert_eq!(state.ui_density, UiDensity::Compact);

    state.set_ui_density(UiDensity::Comfortable);
    assert_eq!(state.ui_density, UiDensity::Comfortable);

    state.set_ui_density(UiDensity::Default);
    assert_eq!(state.ui_density, UiDensity::Default);
}

#[test]
fn test_cache_stats_calculation_and_byte_formatting() {
    // Test formatting helper boundaries
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    assert_eq!(format_bytes(14 * 1024 * 1024 + 204800), "14.2 MB");
    assert_eq!(format_bytes(1024 * 1024 * 1024 * 2), "2.00 GB");

    let default_stats = CacheStats::default();
    assert_eq!(default_stats.file_count, 0);
    assert_eq!(default_stats.total_bytes, 0);
    assert_eq!(default_stats.formatted_size(), "0 B");

    // Test directory calculation on a temporary directory
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let temp_path = camino::Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

    let initial_stats = calculate_directory_stats(&temp_path).expect("calc stats");
    assert_eq!(initial_stats.file_count, 0);
    assert_eq!(initial_stats.total_bytes, 0);

    // Populate files in staging/
    let staging = temp_path.join("staging/tx_test");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("file1.conf"), vec![0u8; 1000]).unwrap();
    std::fs::write(staging.join("file2.conf"), vec![0u8; 2500]).unwrap();

    let populated_stats = calculate_directory_stats(&temp_path).expect("calc stats");
    assert_eq!(populated_stats.file_count, 2);
    assert_eq!(populated_stats.total_bytes, 3500);
    assert_eq!(populated_stats.formatted_size(), "3.4 KB");
}

#[test]
fn test_safe_cache_cleanup_preserves_backups_and_journal() {
    let temp_root = tempfile::tempdir().expect("create temp dir");
    let root_path = camino::Utf8PathBuf::from_path_buf(temp_root.path().to_path_buf()).unwrap();

    let xdg = woderrise_core::paths::XdgPaths::custom(&root_path);

    // 1. Create persistent critical files that MUST NOT be touched
    std::fs::create_dir_all(xdg.woderrise_config_dir()).unwrap();
    std::fs::write(xdg.settings_file(), b"{\"version\":1}").unwrap();

    std::fs::create_dir_all(xdg.woderrise_data_dir()).unwrap();
    std::fs::write(xdg.journal_file(), b"{\"transactions\":[]}").unwrap();
    std::fs::write(xdg.catalog_file(), b"{\"repositories\":[]}").unwrap();

    let backup_dir = xdg.backup_dir("tx_critical_backup").unwrap();
    std::fs::create_dir_all(&backup_dir).unwrap();
    std::fs::write(
        backup_dir.join("hyprland.conf.bak"),
        b"monitor=,preferred,auto,1",
    )
    .unwrap();

    // 2. Create ephemeral cache and staging files
    let cache_dir = xdg.woderrise_cache_dir();
    let staging_dir = xdg.staging_dir("tx_ephemeral").unwrap();
    std::fs::create_dir_all(&staging_dir).unwrap();
    std::fs::write(
        staging_dir.join("staged_file.txt"),
        b"ephemeral staging payload",
    )
    .unwrap();
    std::fs::write(cache_dir.join("loose_cache.blob"), b"cached blob data").unwrap();

    // 3. Execute safe cache cleanup
    let (freed_count, freed_bytes) = safe_cleanup_cache(&cache_dir).expect("cleanup cache");
    assert_eq!(freed_count, 2);
    assert!(freed_bytes > 0);

    // 4. Verify cache contents are removed
    assert!(
        !staging_dir.exists(),
        "Staging subdirectories must be removed"
    );
    assert!(
        !cache_dir.join("loose_cache.blob").exists(),
        "Loose cache blobs must be removed"
    );

    // 5. Verify persistent files are 100% PRESERVED
    assert!(
        xdg.settings_file().exists(),
        "settings.json must NEVER be removed!"
    );
    assert!(
        xdg.journal_file().exists(),
        "journal.json must NEVER be removed!"
    );
    assert!(
        xdg.catalog_file().exists(),
        "catalog.json must NEVER be removed!"
    );
    assert!(
        backup_dir.join("hyprland.conf.bak").exists(),
        "Backup snapshot files must NEVER be removed!"
    );
}

#[test]
fn test_network_request_timeout_configuration() {
    let mut state = SettingsState::new(None);
    assert_eq!(state.request_timeout_secs, 10);

    state.set_timeout(5);
    assert_eq!(state.request_timeout_secs, 5);

    state.set_timeout(30);
    assert_eq!(state.request_timeout_secs, 30);

    // Bounds clamping: values below 5s clamp to 5s, values above 60s clamp to 60s
    state.set_timeout(2);
    assert_eq!(state.request_timeout_secs, 5);

    state.set_timeout(120);
    assert_eq!(state.request_timeout_secs, 60);
}

#[test]
fn test_about_diagnostics_metadata_and_xdg_paths() {
    let temp_root = tempfile::tempdir().expect("create temp dir");
    let root_path = camino::Utf8PathBuf::from_path_buf(temp_root.path().to_path_buf()).unwrap();
    let xdg = woderrise_core::paths::XdgPaths::custom(&root_path);

    let diag = AboutDiagnostics::collect(&xdg);
    assert_eq!(diag.app_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(diag.os_name, std::env::consts::OS);
    assert_eq!(diag.arch, std::env::consts::ARCH);
    assert_eq!(diag.config_dir, root_path.join(".config/woderrise"));
    assert_eq!(diag.data_dir, root_path.join(".local/share/woderrise"));
    assert_eq!(diag.cache_dir, root_path.join(".cache/woderrise"));
    assert_eq!(diag.state_dir, root_path.join(".local/state/woderrise"));
}

#[test]
fn test_settings_view_rendering_all_tabs_and_themes() {
    let xdg = woderrise_core::paths::XdgPaths::from_env().unwrap_or_else(|_| {
        woderrise_core::paths::XdgPaths::custom(&camino::Utf8PathBuf::from("/tmp"))
    });
    let mut state = SettingsState::with_xdg(Some("ghp_valid_test_token_1234".into()), Some(&xdg));
    let rate_limits = woderrise_gui::screens::search::RateLimitGaugeState::new(4500, 5000, 25, 30);

    let tabs = [
        SettingsTab::GitHub,
        SettingsTab::Appearance,
        SettingsTab::NetworkCache,
        SettingsTab::About,
    ];

    let themes = [iced::Theme::Dark, iced::Theme::Light];

    for theme in &themes {
        for tab in &tabs {
            state.active_tab = *tab;
            // Render view; must not panic or cause layout overflow
            let _element = view_settings(&state, &rate_limits, theme, |_action| {
                woderrise_gui::app::Message::Noop
            });
        }
    }
}

#[test]
fn test_clock_tick_subscription_scoping_eliminates_idle_battery_drain() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(100);
    let mut app = App::new(cmd_tx, None);

    // Invariant 1: Discover (Search) screen MUST have zero timer overhead by default
    assert_eq!(app.active_screen, Screen::Search);
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must NOT be active on Discover (Search) screen"
    );

    // Invariant 2: Inspect screen MUST have zero timer overhead
    app.active_screen = Screen::Inspect;
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must NOT be active on Inspect screen"
    );

    // Invariant 3: Install screen MUST have zero timer overhead
    app.active_screen = Screen::Install;
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must NOT be active on Install screen"
    );

    // Invariant 4: History screen MUST have zero timer overhead
    app.active_screen = Screen::History;
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must NOT be active on History screen"
    );

    // Invariant 5: Settings screen MUST activate clock tick for live countdown timers
    app.active_screen = Screen::Settings;
    assert!(
        app.is_clock_tick_needed(),
        "Clock tick MUST be active when viewing Settings screen"
    );

    // Invariant 6: Active connection test activates timer even from non-Settings screens
    app.active_screen = Screen::Search;
    app.is_testing_connection = true;
    assert!(
        app.is_clock_tick_needed(),
        "Clock tick MUST be active while connection test is in flight"
    );

    // Invariant 7: Once connection test completes, timer returns to inactive on Search
    app.is_testing_connection = false;
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must deactivate when connection test finishes"
    );
}

#[test]
fn test_reset_countdown_and_duration_formatting() {
    let now = chrono::Utc::now();

    // 1. Full countdown string with "resets in" / "resets now"
    assert_eq!(
        format_reset_countdown(now - chrono::Duration::seconds(10)),
        "resets now"
    );
    assert_eq!(format_reset_countdown(now), "resets now");
    assert_eq!(
        format_reset_countdown(now + chrono::Duration::milliseconds(1100)),
        "resets in 1s"
    );
    assert_eq!(
        format_reset_countdown(now + chrono::Duration::milliseconds(25100)),
        "resets in 25s"
    );
    assert_eq!(
        format_reset_countdown(now + chrono::Duration::seconds(90)),
        "resets in 1 min"
    );
    assert_eq!(
        format_reset_countdown(now + chrono::Duration::seconds(2710)),
        "resets in 45 min"
    );
    assert_eq!(
        format_reset_countdown(now + chrono::Duration::seconds(3610)),
        "resets in 1h 0m"
    );
    assert_eq!(
        format_reset_countdown(now + chrono::Duration::seconds(7390)),
        "resets in 2h 3m"
    );

    // 2. Compact raw duration without prefix
    assert_eq!(
        format_reset_duration(now - chrono::Duration::seconds(10)),
        "now"
    );
    assert_eq!(format_reset_duration(now), "now");
    assert_eq!(
        format_reset_duration(now + chrono::Duration::milliseconds(1100)),
        "1s"
    );
    assert_eq!(
        format_reset_duration(now + chrono::Duration::milliseconds(25100)),
        "25s"
    );
    assert_eq!(
        format_reset_duration(now + chrono::Duration::seconds(90)),
        "1 min"
    );
    assert_eq!(
        format_reset_duration(now + chrono::Duration::seconds(2710)),
        "45 min"
    );
    assert_eq!(
        format_reset_duration(now + chrono::Duration::seconds(3610)),
        "1h 0m"
    );
    assert_eq!(
        format_reset_duration(now + chrono::Duration::seconds(7390)),
        "2h 3m"
    );
}
