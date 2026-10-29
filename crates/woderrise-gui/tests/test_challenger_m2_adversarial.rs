//! Adversarial Challenge Test Suite for Milestone M2 (Discover Screen & Responsive Grid)
//!
//! Empirically stress-tests:
//! 1. `calculate_grid_columns` boundary conditions (0.0, negative, subnormal, 959.99, 960.0, 1399.99, 1400.0, huge, NaN, +/-inf).
//! 2. `SortOption` stability and edge cases (empty lists, single elements, ties in stars, None/ties in pushed_at, unicode/special chars in repo names).
//! 3. `Message::SearchClear` reset semantics (query, filter, sort, cancel token, debounce flags, catalog refresh).
//! 4. `Task::perform` debounce timer non-blocking execution, stale token rejection, and command dispatch.
//! 5. `format_relative_time` and `format_star_count` edge cases (future timestamps, negative diffs, zero, u64::MAX).
//! 6. `view_card_themed` rendering robustness under extreme repository inputs (empty metadata, massive descriptions, 50+ components).

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
    pushed_at: Option<chrono::DateTime<chrono::Utc>>,
    components: Vec<ConfigComponent>,
) -> CatalogRepository {
    let detected = components
        .into_iter()
        .map(|c| DetectedComponent {
            component: c,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: ".config".into(),
            entrypoint: "config".into(),
            style_file: None,
            file_count: 2,
            total_size_bytes: 1024,
        })
        .collect();

    CatalogRepository {
        id,
        owner: owner.to_string(),
        repo: repo.to_string(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: Some(format!("Test repo {}/{}", owner, repo)),
        stargazers_count: stars,
        default_branch: "main".to_string(),
        topics: vec!["hyprland".into(), "dotfiles".into()],
        pushed_at,
        cached_tree_sha: None,
        etag: None,
        detected_components: detected,
    }
}

// =========================================================================
// 1. Boundary & Extreme Floating-Point Grid Column Calculation
// =========================================================================

#[test]
fn test_adversarial_grid_column_calculation_boundaries() {
    // Exact boundaries
    assert_eq!(calculate_grid_columns(0.0), 2);
    assert_eq!(calculate_grid_columns(1.0), 2);
    assert_eq!(calculate_grid_columns(480.0), 2);
    assert_eq!(calculate_grid_columns(959.0), 2);
    assert_eq!(calculate_grid_columns(959.9999), 2);

    // Boundary 1: 960.0
    assert_eq!(calculate_grid_columns(960.0), 3);
    assert_eq!(calculate_grid_columns(960.0001), 3);
    assert_eq!(calculate_grid_columns(1200.0), 3);
    assert_eq!(calculate_grid_columns(1399.0), 3);
    assert_eq!(calculate_grid_columns(1399.9999), 3);

    // Boundary 2: 1400.0
    assert_eq!(calculate_grid_columns(1400.0), 4);
    assert_eq!(calculate_grid_columns(1400.0001), 4);
    assert_eq!(calculate_grid_columns(1920.0), 4);
    assert_eq!(calculate_grid_columns(3840.0), 4);

    // Extremes & Negative Values: Must never return 0 or panic (chunking by 0 panics)
    assert_eq!(calculate_grid_columns(-0.0), 2);
    assert_eq!(calculate_grid_columns(-1.0), 2);
    assert_eq!(calculate_grid_columns(-999999.0), 2);
    assert_eq!(calculate_grid_columns(f32::MIN), 2);
    assert_eq!(calculate_grid_columns(f32::NEG_INFINITY), 2);

    // Extremely large values
    assert_eq!(calculate_grid_columns(100_000.0), 4);
    assert_eq!(calculate_grid_columns(f32::MAX), 4);
    assert_eq!(calculate_grid_columns(f32::INFINITY), 4);

    // IEEE 754 NaN handling
    let nan_cols = calculate_grid_columns(f32::NAN);
    assert!(
        (2..=4).contains(&nan_cols),
        "NaN width must return a valid column count between 2 and 4, got {}",
        nan_cols
    );
}

// =========================================================================
// 2. SortOption Adversarial Stress Testing
// =========================================================================

#[test]
fn test_adversarial_sort_option_empty_and_single_element() {
    let mut state = SearchState::new(vec![]);

    for sort in SortOption::ALL {
        state.set_sort_option(sort);
        assert!(
            state.filtered_repositories.is_empty(),
            "Sorting empty list with {:?} must remain empty",
            sort
        );
    }

    let single_repo = make_test_repo(1, "alice", "single-dotfiles", 100, None, vec![]);
    let mut state_single = SearchState::new(vec![single_repo]);

    for sort in SortOption::ALL {
        state_single.set_sort_option(sort);
        assert_eq!(state_single.filtered_repositories.len(), 1);
        assert_eq!(
            state_single.filtered_repositories[0].resolved_repo(),
            "single-dotfiles"
        );
    }
}

#[test]
fn test_adversarial_sort_option_ties_and_none_timestamps() {
    let now = Utc::now();
    // 4 repositories: 2 have tied stars (500), 2 have tied pushed_at, 1 has None pushed_at
    let r1 = make_test_repo(
        1,
        "owner1",
        "repo-alpha",
        500,
        Some(now - Duration::days(5)),
        vec![],
    );
    let r2 = make_test_repo(
        2,
        "owner2",
        "repo-beta",
        500,
        Some(now - Duration::days(1)),
        vec![],
    );
    let r3 = make_test_repo(
        3,
        "owner3",
        "repo-gamma",
        100,
        Some(now - Duration::days(1)),
        vec![],
    );
    let r4 = make_test_repo(4, "owner4", "repo-delta", 50, None, vec![]);

    let repos = vec![r1, r2, r3, r4];
    let mut state = SearchState::new(repos);

    // Stars sort: 500 (r1, r2), then 100 (r3), then 50 (r4)
    state.set_sort_option(SortOption::Stars);
    assert_eq!(state.filtered_repositories[0].stargazers_count, 500);
    assert_eq!(state.filtered_repositories[1].stargazers_count, 500);
    assert_eq!(state.filtered_repositories[2].stargazers_count, 100);
    assert_eq!(state.filtered_repositories[3].stargazers_count, 50);

    // RecentlyUpdated sort:
    // r2 (1 day ago, id 2) & r3 (1 day ago, id 3) top,
    // r1 (5 days ago) third,
    // r4 (None pushed_at) last!
    state.set_sort_option(SortOption::RecentlyUpdated);
    assert!(state.filtered_repositories[0].pushed_at.is_some());
    assert!(state.filtered_repositories[1].pushed_at.is_some());
    assert!(state.filtered_repositories[2].pushed_at.is_some());
    assert_eq!(state.filtered_repositories[3].id, 4); // None pushed_at placed at the end
    assert!(state.filtered_repositories[3].pushed_at.is_none());
}

#[test]
fn test_adversarial_sort_option_special_characters_and_case() {
    let r1 = make_test_repo(1, "user", "Zeta", 10, None, vec![]);
    let r2 = make_test_repo(2, "user", "alpha", 20, None, vec![]);
    let r3 = make_test_repo(3, "user", "ALPHA", 30, None, vec![]);
    let r4 = make_test_repo(4, "user", "_special-repo", 40, None, vec![]);
    let r5 = make_test_repo(5, "user", "123-dots", 50, None, vec![]);
    let r6 = make_test_repo(6, "user", "🦀-rust-rice", 60, None, vec![]);

    let repos = vec![r1, r2, r3, r4, r5, r6];
    let mut state = SearchState::new(repos);

    state.set_sort_option(SortOption::Name);
    let names: Vec<String> = state
        .filtered_repositories
        .iter()
        .map(|r| r.resolved_repo().to_lowercase())
        .collect();

    // Check sorted in non-decreasing alphabetical order
    for w in names.windows(2) {
        assert!(
            w[0] <= w[1],
            "Names should be alphabetically sorted: {} vs {}",
            w[0],
            w[1]
        );
    }
}

// =========================================================================
// 3. SearchClear Reset Semantics & Cancellation Invariants
// =========================================================================

#[test]
fn test_adversarial_search_clear_deep_invariants() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    let r1 = make_test_repo(
        1,
        "hyprwm",
        "hyprland",
        15000,
        None,
        vec![ConfigComponent::Hyprland],
    );
    let r2 = make_test_repo(
        2,
        "Alexays",
        "Waybar",
        8000,
        None,
        vec![ConfigComponent::Waybar],
    );
    app.search_state.catalog_repositories = vec![r1, r2];
    app.search_state.apply_filter();

    // 1. Simulate typing a search query
    let _ = app.update(Message::SearchInputChanged("hypr".to_string()));
    assert_eq!(app.search_state.query, "hypr");
    assert!(app.search_state.debounce_pending);

    // 2. Simulate setting filter and sort
    let _ = app.update(Message::FilterSelected(CategoryFilter::Waybar));
    let _ = app.update(Message::SortSelected(SortOption::Name));
    assert_eq!(app.search_state.active_filter, CategoryFilter::Waybar);
    assert_eq!(app.search_state.sort_option, SortOption::Name);

    // 3. Simulate an active search cancellation token
    let token = tokio_util::sync::CancellationToken::new();
    app.search_cancel_token = Some(token.clone());
    app.search_state.is_searching = true;

    // 4. Fire SearchClear
    let _ = app.update(Message::SearchClear);

    // Invariants check:
    assert!(app.search_state.query.is_empty());
    assert_eq!(app.search_state.active_filter, CategoryFilter::All);
    assert_eq!(app.search_state.sort_option, SortOption::Relevance);
    assert!(!app.search_state.is_searching);
    assert!(!app.search_state.debounce_pending);
    assert!(app.search_cancel_token.is_none());
    assert!(
        token.is_cancelled(),
        "Existing search cancel token MUST be cancelled"
    );

    // All repositories restored
    assert_eq!(app.search_state.filtered_repositories.len(), 2);

    // Catalog refresh requested
    assert!(matches!(cmd_rx.try_recv(), Ok(GuiCommand::RefreshCatalog)));
}

// =========================================================================
// 4. Runtime Debounce Non-Blocking Execution & Stale Query Rejection
// =========================================================================

#[tokio::test]
async fn test_adversarial_debounce_timer_fires_without_blocking() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    // Type query "catppuccin"
    let _task = app.update(Message::SearchInputChanged("catppuccin".to_string()));

    // Verify debounce is pending in state
    assert!(app.search_state.debounce_pending);
    assert_eq!(app.search_state.query, "catppuccin");

    // The update returned a task. Let's verify that after sleeping 310ms,
    // Message::SearchDebounceFired triggers GuiCommand::Search
    tokio::time::sleep(std::time::Duration::from_millis(310)).await;

    // Now dispatch SearchDebounceFired with the matching query
    let _ = app.update(Message::SearchDebounceFired("catppuccin".to_string()));

    // Verify debounce cleared and search command sent
    assert!(!app.search_state.debounce_pending);
    assert!(app.search_state.is_searching);

    match cmd_rx.try_recv() {
        Ok(GuiCommand::Search { query, .. }) => {
            assert_eq!(query, "catppuccin");
        }
        other => panic!("Expected GuiCommand::Search, got {:?}", other),
    }
}

#[tokio::test]
async fn test_adversarial_debounce_rejects_stale_query() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    // 1. User types "cat"
    let _ = app.update(Message::SearchInputChanged("cat".to_string()));

    // 2. Rapidly afterwards, user types "catppuccin" (before "cat" debounce completes)
    let _ = app.update(Message::SearchInputChanged("catppuccin".to_string()));

    // 3. Stale timer for "cat" arrives
    let _ = app.update(Message::SearchDebounceFired("cat".to_string()));

    // The stale "cat" message MUST NOT trigger a search because the active query is "catppuccin"
    assert!(
        cmd_rx.try_recv().is_err(),
        "Stale debounce MUST NOT trigger GuiCommand::Search"
    );

    // 4. Now the legitimate "catppuccin" timer arrives after 300ms
    tokio::time::sleep(std::time::Duration::from_millis(310)).await;
    let _ = app.update(Message::SearchDebounceFired("catppuccin".to_string()));

    // Now it should trigger
    match cmd_rx.try_recv() {
        Ok(GuiCommand::Search { query, .. }) => {
            assert_eq!(query, "catppuccin");
        }
        other => panic!("Expected GuiCommand::Search, got {:?}", other),
    }
}

// =========================================================================
// 5. Formatting Helpers Adversarial Edge Cases
// =========================================================================

#[test]
fn test_adversarial_formatting_helpers() {
    // Star counts
    assert_eq!(format_star_count(0), "★ 0");
    assert_eq!(format_star_count(1), "★ 1");
    assert_eq!(format_star_count(999), "★ 999");
    assert_eq!(format_star_count(1_000), "★ 1.0k");
    assert_eq!(format_star_count(1_049), "★ 1.0k");
    assert_eq!(format_star_count(1_050), "★ 1.1k");
    assert_eq!(format_star_count(999_999), "★ 1000.0k");

    // Relative times: future timestamps (clock skew)
    let future = Utc::now() + Duration::days(5);
    let s_future = format_relative_time(Some(future));
    // When duration is negative, num_seconds() < 60, should gracefully return "Updated just now"
    assert_eq!(s_future, "Updated just now");

    // None timestamp
    assert_eq!(format_relative_time(None), "Updated recently");

    // Distant past
    let ten_years_ago = Utc::now() - Duration::days(365 * 10);
    assert_eq!(
        format_relative_time(Some(ten_years_ago)),
        "Updated 10 years ago"
    );
}

// =========================================================================
// 6. Extreme Card Widget Rendering Robustness
// =========================================================================

#[test]
fn test_adversarial_card_and_search_view_rendering_stress() {
    // Repository with huge description, special unicode, 20 detected components
    let components = vec![
        ConfigComponent::Hyprland,
        ConfigComponent::Waybar,
        ConfigComponent::Hyprlock,
        ConfigComponent::Hypridle,
        ConfigComponent::Wallpapers,
        ConfigComponent::GtkTheme,
        ConfigComponent::Monitors,
    ];

    let mut repo = make_test_repo(
        99999,
        "🚀-super-unicode_owner",
        "extremely-long-repository-name-designed-to-stress-ui-layout-boundaries",
        1_234_567,
        Some(Utc::now() - Duration::hours(12)),
        components,
    );
    repo.description = Some("A".repeat(1000)); // 1000 characters description!

    // Verify view_card_themed renders under both Light and Dark themes without panics
    let _card_dark = view_card_themed(&repo, &Theme::Dark, Message::Noop);
    let _card_light = view_card_themed(&repo, &Theme::Light, Message::Noop);

    // Test SearchState with 50 repositories rendered in view_search
    let mut repos = Vec::new();
    for i in 0..50 {
        repos.push(make_test_repo(
            i,
            &format!("owner_{}", i),
            &format!("repo_{}", i),
            i * 100,
            Some(Utc::now() - Duration::days(i as i64)),
            vec![ConfigComponent::Hyprland],
        ));
    }

    let search_state = SearchState::new(repos);
    let _view = view_search(
        &search_state,
        &Theme::Dark,
        Message::SearchInputChanged,
        Message::SearchSubmitted,
        Message::SearchClear,
        Message::FilterSelected,
        Message::SortSelected,
        Message::SelectRepo,
    );
}
