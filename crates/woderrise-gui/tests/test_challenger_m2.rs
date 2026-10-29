//! Adversarial empirical challenge tests for Milestone M2:
//! Discover Screen Redesign & Responsive Grid (R2).
//!
//! Empirically challenges:
//! 1. `calculate_grid_columns` on all boundary widths (<960, 960, 1400, >1400, 0, negative, extremely large, NaN, Inf).
//! 2. `SortOption` on empty catalogs, single elements, ties in stars, ties in dates, None dates, special characters and Unicode in repo names.
//! 3. `Message::SearchClear` resetting query, filter, sort, debounce, in-flight cancellation token, and dispatching catalog refresh.
//! 4. Debounce timing, stale debounce rejection, and non-blocking Task execution.
//! 5. `view_search` and `view_card_themed` robustness against extreme text lengths, zero stars, many components, and layout paddings.

use chrono::{Duration, Utc};
use iced::Theme;
use tokio::sync::mpsc;
use woderrise_core::models::{CatalogRepository, ConfigComponent, DetectedComponent, SyntaxFormat};
use woderrise_gui::app::{App, GuiCommand, Message};
use woderrise_gui::screens::search::{
    calculate_grid_columns, view_search, CategoryFilter, SearchState, SortOption,
};
use woderrise_gui::widgets::card::{format_relative_time, format_star_count, view_card_themed};

fn make_test_repo(
    id: u64,
    owner: &str,
    repo: &str,
    stars: u64,
    pushed_at: Option<chrono::DateTime<Utc>>,
    desc: Option<&str>,
    comps: Vec<ConfigComponent>,
) -> CatalogRepository {
    CatalogRepository {
        id,
        owner: owner.to_string(),
        repo: repo.to_string(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: desc.map(|s| s.to_string()),
        stargazers_count: stars,
        default_branch: "main".to_string(),
        topics: vec!["hyprland".to_string(), "dotfiles".to_string()],
        pushed_at,
        cached_tree_sha: None,
        etag: None,
        detected_components: comps
            .into_iter()
            .map(|c| DetectedComponent {
                component: c,
                syntax_format: SyntaxFormat::LegacyConf,
                root_path: ".config".to_string(),
                entrypoint: "config.conf".to_string(),
                style_file: None,
                file_count: 5,
                total_size_bytes: 4096,
            })
            .collect(),
    }
}

/// 1. EMPIRICAL CHALLENGE: `calculate_grid_columns` Boundary Stress Testing
#[test]
fn test_challenge_grid_column_calculation_boundaries() {
    // Exact threshold boundaries
    assert_eq!(calculate_grid_columns(0.0), 2);
    assert_eq!(calculate_grid_columns(1.0), 2);
    assert_eq!(calculate_grid_columns(959.0), 2);
    assert_eq!(calculate_grid_columns(959.999), 2);
    assert_eq!(calculate_grid_columns(960.0), 3);
    assert_eq!(calculate_grid_columns(960.001), 3);
    assert_eq!(calculate_grid_columns(1399.0), 3);
    assert_eq!(calculate_grid_columns(1399.999), 3);
    assert_eq!(calculate_grid_columns(1400.0), 4);
    assert_eq!(calculate_grid_columns(1400.001), 4);
    assert_eq!(calculate_grid_columns(3840.0), 4);

    // Negative widths (e.g. uninitialized, minimized, or collapsed window states)
    assert_eq!(calculate_grid_columns(-0.0), 2);
    assert_eq!(calculate_grid_columns(-1.0), 2);
    assert_eq!(calculate_grid_columns(-999.0), 2);
    assert_eq!(calculate_grid_columns(f32::MIN), 2);
    assert_eq!(calculate_grid_columns(f32::NEG_INFINITY), 2);

    // Extremely large widths
    assert_eq!(calculate_grid_columns(100_000.0), 4);
    assert_eq!(calculate_grid_columns(f32::MAX), 4);
    assert_eq!(calculate_grid_columns(f32::INFINITY), 4);

    // NaN boundary behavior: In Rust `NaN < 960.0` is false, `NaN < 1400.0` is false => 4
    assert_eq!(calculate_grid_columns(f32::NAN), 4);

    // INVARIANT CHECK: Columns must ALWAYS be >= 1 to prevent chunk(0) panic
    let test_widths = [
        -1000.0,
        -1.0,
        0.0,
        0.1,
        500.0,
        959.9,
        960.0,
        1024.0,
        1399.9,
        1400.0,
        2560.0,
        10000.0,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];

    let dummy_repos = [
        make_test_repo(1, "o1", "r1", 10, None, None, vec![]),
        make_test_repo(2, "o2", "r2", 20, None, None, vec![]),
        make_test_repo(3, "o3", "r3", 30, None, None, vec![]),
    ];

    for &w in &test_widths {
        let cols = calculate_grid_columns(w);
        assert!(cols >= 1, "calculate_grid_columns({}) returned 0!", w);

        // Verify chunking does not panic and remainder math does not underflow
        for chunk in dummy_repos.chunks(cols) {
            assert!(cols >= chunk.len());
            let remainder = cols - chunk.len();
            assert!(remainder < cols);
        }
    }
}

/// 2. EMPIRICAL CHALLENGE: `SortOption` Edge Cases (Empty, Single, Star Ties, Date Ties, Special Characters)
#[test]
fn test_challenge_sort_option_edge_cases() {
    // 2.1 Empty list across all sort options
    let mut empty_state = SearchState::new(Vec::new());
    for sort_opt in SortOption::ALL {
        empty_state.set_sort_option(sort_opt);
        assert!(empty_state.filtered_repositories.is_empty());
    }

    // 2.2 Single element across all sort options
    let single_repo = make_test_repo(
        1,
        "lone",
        "repo",
        42,
        Some(Utc::now()),
        Some("Single"),
        vec![],
    );
    let mut single_state = SearchState::new(vec![single_repo.clone()]);
    for sort_opt in SortOption::ALL {
        single_state.set_sort_option(sort_opt);
        assert_eq!(single_state.filtered_repositories.len(), 1);
        assert_eq!(single_state.filtered_repositories[0].id, 1);
    }

    // 2.3 Ties in stars
    let now = Utc::now();
    let r_tie1 = make_test_repo(
        10,
        "alpha",
        "tie-a",
        100,
        Some(now - Duration::days(5)),
        None,
        vec![],
    );
    let r_tie2 = make_test_repo(
        20,
        "beta",
        "tie-b",
        100,
        Some(now - Duration::days(2)),
        None,
        vec![],
    );
    let r_tie3 = make_test_repo(
        30,
        "gamma",
        "tie-c",
        100,
        Some(now - Duration::days(1)),
        None,
        vec![],
    );
    let r_higher = make_test_repo(
        40,
        "delta",
        "tie-high",
        500,
        Some(now - Duration::days(10)),
        None,
        vec![],
    );
    let r_lower = make_test_repo(
        50,
        "epsilon",
        "tie-low",
        10,
        Some(now - Duration::days(8)),
        None,
        vec![],
    );

    let mut tie_state = SearchState::new(vec![
        r_tie1.clone(),
        r_tie2.clone(),
        r_tie3.clone(),
        r_higher.clone(),
        r_lower.clone(),
    ]);

    tie_state.set_sort_option(SortOption::Stars);
    assert_eq!(tie_state.filtered_repositories[0].id, 40); // 500 stars
    assert_eq!(tie_state.filtered_repositories[4].id, 50); // 10 stars

    // In-between are the 3 tied repos (100 stars). Rust's sort_by_key is stable:
    let middle_ids: Vec<u64> = tie_state.filtered_repositories[1..4]
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(middle_ids, vec![10, 20, 30]);

    // 2.4 Ties in pushed_at and handling of None pushed_at
    let r_date_none1 = make_test_repo(60, "none", "no-date-1", 10, None, None, vec![]);
    let r_date_none2 = make_test_repo(61, "none", "no-date-2", 20, None, None, vec![]);
    let mut date_state = SearchState::new(vec![
        r_date_none1.clone(),
        r_tie1.clone(),
        r_date_none2.clone(),
        r_tie3.clone(),
    ]);

    date_state.set_sort_option(SortOption::RecentlyUpdated);
    // r_tie3 is 1 day ago (most recent)
    assert_eq!(date_state.filtered_repositories[0].id, 30);
    // r_tie1 is 5 days ago
    assert_eq!(date_state.filtered_repositories[1].id, 10);
    // None pushed_at must be placed at the end safely without crashing
    assert!(date_state.filtered_repositories[2].pushed_at.is_none());
    assert!(date_state.filtered_repositories[3].pushed_at.is_none());

    // 2.5 Special characters, unicode, uppercase/lowercase in repo names
    let special_repos = vec![
        make_test_repo(101, "u", "zebra-rice", 0, None, None, vec![]),
        make_test_repo(102, "u", "ALPHA-CONFIG", 0, None, None, vec![]),
        make_test_repo(103, "u", "!dotfiles-bang", 0, None, None, vec![]),
        make_test_repo(104, "u", "🦀-rust-rice", 0, None, None, vec![]),
        make_test_repo(105, "u", "_under_dot", 0, None, None, vec![]),
        make_test_repo(106, "u", "alpha-config-lower", 0, None, None, vec![]),
        make_test_repo(107, "u", "123-numeric", 0, None, None, vec![]),
    ];

    let mut name_state = SearchState::new(special_repos);
    name_state.set_sort_option(SortOption::Name);

    let names: Vec<String> = name_state
        .filtered_repositories
        .iter()
        .map(|r| r.resolved_repo().to_lowercase())
        .collect();

    // Verify ordering is strictly ascending lexicographical
    for i in 0..names.len() - 1 {
        assert!(
            names[i] <= names[i + 1],
            "SortOption::Name failed: '{}' > '{}'",
            names[i],
            names[i + 1]
        );
    }
}

/// 3. EMPIRICAL CHALLENGE: `Message::SearchClear` Comprehensive State Restoration
#[test]
fn test_challenge_search_clear_comprehensive_reset() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);

    // Drain initial commands
    while cmd_rx.try_recv().is_ok() {}

    let repo_a = make_test_repo(
        1,
        "user",
        "hypr-a",
        10,
        None,
        Some("hypr config"),
        vec![ConfigComponent::Hyprland],
    );
    let repo_b = make_test_repo(
        2,
        "user",
        "waybar-b",
        20,
        None,
        Some("waybar config"),
        vec![ConfigComponent::Waybar],
    );
    let repo_c = make_test_repo(
        3,
        "user",
        "other-c",
        30,
        None,
        Some("other config"),
        vec![ConfigComponent::Hyprlock],
    );

    app.search_state.catalog_repositories = vec![repo_a, repo_b, repo_c];
    app.search_state.apply_filter();
    assert_eq!(app.search_state.filtered_repositories.len(), 3);

    // Set dirty states: query, active filter, sort option, fake in-flight token, flags
    let _ = app.update(Message::SearchInputChanged("hypr".to_string()));
    let _ = app.update(Message::FilterSelected(CategoryFilter::Hyprland));
    let _ = app.update(Message::SortSelected(SortOption::Name));

    let cancel_token = tokio_util::sync::CancellationToken::new();
    app.search_cancel_token = Some(cancel_token.clone());
    app.search_state.is_searching = true;
    app.search_state.debounce_pending = true;

    assert_eq!(app.search_state.query, "hypr");
    assert_eq!(app.search_state.active_filter, CategoryFilter::Hyprland);
    assert_eq!(app.search_state.sort_option, SortOption::Name);
    assert!(!cancel_token.is_cancelled());

    // Send Message::SearchClear
    let _ = app.update(Message::SearchClear);

    // Verify complete state reset
    assert!(app.search_state.query.is_empty(), "Query was not cleared");
    assert_eq!(
        app.search_state.active_filter,
        CategoryFilter::All,
        "Filter was not reset to All"
    );
    assert_eq!(
        app.search_state.sort_option,
        SortOption::Relevance,
        "Sort was not reset to Relevance"
    );
    assert!(!app.search_state.is_searching, "is_searching was not reset");
    assert!(
        !app.search_state.debounce_pending,
        "debounce_pending was not reset"
    );

    // In-flight cancellation token must have been cancelled and taken
    assert!(
        app.search_cancel_token.is_none(),
        "search_cancel_token was not taken"
    );
    assert!(
        cancel_token.is_cancelled(),
        "in-flight search was not cancelled"
    );

    // Filtered repositories must be restored to all 3 catalog repositories
    assert_eq!(
        app.search_state.filtered_repositories.len(),
        3,
        "Catalog results were not restored"
    );

    // Command channel must have received GuiCommand::RefreshCatalog
    let mut refreshed = false;
    while let Ok(cmd) = cmd_rx.try_recv() {
        if matches!(cmd, GuiCommand::RefreshCatalog) {
            refreshed = true;
        }
    }
    assert!(
        refreshed,
        "GuiCommand::RefreshCatalog was not dispatched on SearchClear"
    );
}

/// 4. EMPIRICAL CHALLENGE: Debounce Timing & Stale Event Rejection
#[test]
fn test_challenge_debounce_runtime_logic() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    // 4.1 Empty input cancels any pending in-flight token, sets is_searching=false, and triggers RefreshCatalog
    let old_token = tokio_util::sync::CancellationToken::new();
    app.search_cancel_token = Some(old_token.clone());
    let _empty_task = app.update(Message::SearchInputChanged("   ".to_string()));
    assert!(!app.search_state.is_searching);
    assert!(old_token.is_cancelled());
    assert!(matches!(cmd_rx.try_recv(), Ok(GuiCommand::RefreshCatalog)));

    // When SearchClear is called, debounce_pending is explicitly cleared
    let _ = app.update(Message::SearchClear);
    assert!(!app.search_state.debounce_pending);

    let _task = app.update(Message::SearchInputChanged("hypr".to_string()));
    assert_eq!(app.search_state.query, "hypr");
    assert!(app.search_state.debounce_pending);
    assert!(old_token.is_cancelled());

    // 4.3 Stale debounce rejection:
    // If user changed query from "hypr" to "hyprland", a delayed debounce message for "hypr" must be dropped
    let _task2 = app.update(Message::SearchInputChanged("hyprland".to_string()));
    assert_eq!(app.search_state.query, "hyprland");

    // Simulate stale debounce event arriving for "hypr"
    let _ = app.update(Message::SearchDebounceFired("hypr".to_string()));
    // Should NOT have triggered GuiCommand::Search
    while let Ok(cmd) = cmd_rx.try_recv() {
        if let GuiCommand::Search { query, .. } = cmd {
            panic!("Stale query '{}' triggered search!", query);
        }
    }

    // 4.4 Premature debounce rejection (elapsed < 300ms)
    // Manually set last_input_time_ms to current timestamp
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    app.search_state.last_input_time_ms = now_ms;
    assert!(!app.search_state.should_fire_debounce(now_ms + 100, 300));
    assert!(app.search_state.should_fire_debounce(now_ms + 300, 300));
    assert!(app.search_state.should_fire_debounce(now_ms + 500, 300));

    // 4.5 Matching debounce event after delay triggers search
    // Simulate elapsed time by backdating last_input_time_ms
    app.search_state.last_input_time_ms = now_ms - 350;
    let _ = app.update(Message::SearchDebounceFired("hyprland".to_string()));

    assert!(app.search_state.is_searching);
    assert!(!app.search_state.debounce_pending);
    assert!(app.search_cancel_token.is_some());

    let mut found_search = false;
    while let Ok(cmd) = cmd_rx.try_recv() {
        if let GuiCommand::Search { query, .. } = cmd {
            assert_eq!(query, "hyprland");
            found_search = true;
        }
    }
    assert!(
        found_search,
        "Valid debounce event did not trigger GuiCommand::Search"
    );
}

/// 5. EMPIRICAL CHALLENGE: UI View and Card Robustness Under Extreme Data
#[test]
fn test_challenge_view_and_card_rendering_extremes() {
    // 5.1 Card with extreme values
    let extreme_repo = make_test_repo(
        999,
        "super-long-organization-name-that-exceeds-normal-length",
        "extremely-long-repository-name-with-nested-suffixes-and-extensions",
        1_234_567,
        Some(Utc::now() - Duration::days(400)),
        Some("A".repeat(500).as_str()),
        vec![
            ConfigComponent::Hyprland,
            ConfigComponent::Waybar,
            ConfigComponent::Hyprlock,
            ConfigComponent::Hypridle,
            ConfigComponent::Wallpapers,
            ConfigComponent::GtkTheme,
        ],
    );

    // Format helpers on extreme values
    assert_eq!(format_star_count(1_234_567), "★ 1234.6k");
    assert_eq!(format_star_count(0), "★ 0");
    assert_eq!(format_relative_time(None), "Updated recently");
    assert_eq!(
        format_relative_time(Some(Utc::now() - Duration::days(400))),
        "Updated 1 year ago"
    );
    assert_eq!(
        format_relative_time(Some(Utc::now() - Duration::seconds(10))),
        "Updated just now"
    );

    // Render cards in both themes
    let _dark_card = view_card_themed(&extreme_repo, &Theme::Dark, Message::Noop);
    let _light_card = view_card_themed(&extreme_repo, &Theme::Light, Message::Noop);

    // 5.2 Discover screen rendering: Empty state with search clear
    let empty_state = SearchState::new(Vec::new());
    let _empty_view = view_search(
        &empty_state,
        &Theme::Dark,
        |_| Message::Noop,
        Message::Noop,
        Message::SearchClear,
        |_| Message::Noop,
        |_| Message::Noop,
        |_| Message::Noop,
    );

    // 5.3 Discover screen rendering: Loading spinner state
    let mut loading_state = SearchState::new(Vec::new());
    loading_state.is_searching = true;
    let _loading_view = view_search(
        &loading_state,
        &Theme::Dark,
        |_| Message::Noop,
        Message::Noop,
        Message::SearchClear,
        |_| Message::Noop,
        |_| Message::Noop,
        |_| Message::Noop,
    );

    // 5.4 Discover screen rendering: Low rate limit banner state
    let mut low_quota_state = SearchState::new(vec![extreme_repo.clone()]);
    low_quota_state.set_rate_limits(10, 60, 1, 10); // search_remaining = 1 -> is_search_low() is true
    assert!(low_quota_state.rate_limits.is_search_low());
    let _low_quota_view = view_search(
        &low_quota_state,
        &Theme::Light,
        |_| Message::Noop,
        Message::Noop,
        Message::SearchClear,
        |_| Message::Noop,
        |_| Message::Noop,
        |_| Message::Noop,
    );

    // 5.5 Category counts consistency
    let repo_hypr = make_test_repo(1, "o", "r1", 0, None, None, vec![ConfigComponent::Hyprland]);
    let repo_waybar = make_test_repo(2, "o", "r2", 0, None, None, vec![ConfigComponent::Waybar]);
    let repo_both = make_test_repo(
        3,
        "o",
        "r3",
        0,
        None,
        None,
        vec![ConfigComponent::Hyprland, ConfigComponent::Waybar],
    );

    let state = SearchState::new(vec![repo_hypr, repo_waybar, repo_both]);
    assert_eq!(state.category_count(CategoryFilter::All), 3);
    assert_eq!(state.category_count(CategoryFilter::Hyprland), 2);
    assert_eq!(state.category_count(CategoryFilter::Waybar), 2);
    assert_eq!(state.category_count(CategoryFilter::Hyprlock), 0);
}
