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
    CatalogRepository, ConfigComponent, DetectedComponent, HardwareConflictNotice, JournalEntry,
    SyntaxFormat, TransactionStatus,
};
use woderrise_detect::ProceduralDirective;
use woderrise_gui::screens::history::HistoryState;
use woderrise_gui::screens::inspect::InspectState;
use woderrise_gui::screens::install::{InstallPhase, InstallState};
use woderrise_gui::screens::search::{
    CategoryFilter, RateLimitColorTier, RateLimitGaugeState, SearchState,
};
use woderrise_gui::theme::{fetch_portal_theme, map_color_scheme_to_theme, map_raw_scheme_value};

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

    // LIFO reverse chronological: [t3 (newest committed), t2 (older committed), t1 (rolled back)]
    let mut history = HistoryState::from_journal_entries(&[t3, t2, t1]);
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
