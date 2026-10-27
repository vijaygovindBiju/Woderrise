//! Empirical challenger adversarial test suite for `woderrise-gui`.
//!
//! Stress-tests view models, boundaries, edge cases, and arithmetic invariants:
//! 1. GitHub rate limit gauge math: limit 0 (division by zero), 100%, 20%, 5%, 0%, u32::MAX, overflow.
//! 2. Debounce timing: rapid query changes within 300ms, backwards clock skew, query cancellation, empty fallback.
//! 3. Category filtering: multi-category composites, case insensitivity, regex/special chars, empty states.
//! 4. Diff view edge cases: empty files, completely new files, deleted files, CRLF vs LF, 5000+ line diffs.
//! 5. Inspect & Install states: component toggle invariants, invalid progress ratios, directive approvals.
//! 6. History & Rollback LIFO ordering: non-committed entry resilience, dirty state modals.

use camino::Utf8PathBuf;
use chrono::Utc;
use woderrise_core::models::{
    CatalogRepository, ConfigComponent, DetectedComponent, JournalEntry, SyntaxFormat,
    TransactionStatus,
};
use woderrise_gui::screens::history::HistoryState;
use woderrise_gui::screens::inspect::InspectState;
use woderrise_gui::screens::install::{InstallPhase, InstallState};
use woderrise_gui::screens::search::{
    CategoryFilter, RateLimitColorTier, RateLimitGaugeState, SearchState,
};
use woderrise_gui::widgets::diff::{view_diff, FileDiffReport};
use woderrise_gui::widgets::gauge::view_gauge;

fn dummy_repo(
    id: u64,
    owner: &str,
    repo: &str,
    desc: &str,
    components: Vec<ConfigComponent>,
    topics: Vec<&str>,
) -> CatalogRepository {
    CatalogRepository {
        id,
        owner: owner.into(),
        repo: repo.into(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: Some(desc.into()),
        stargazers_count: 50 * id,
        default_branch: "main".into(),
        topics: topics.into_iter().map(String::from).collect(),
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: components
            .into_iter()
            .map(|c| DetectedComponent {
                component: c,
                syntax_format: SyntaxFormat::LegacyConf,
                root_path: "config".into(),
                entrypoint: "config.conf".into(),
                style_file: None,
                file_count: 2,
                total_size_bytes: 4096,
            })
            .collect(),
    }
}

// =========================================================================
// 1. RATE LIMIT ARITHMETIC & BOUNDARY ADVERSARIAL MATRIX
// =========================================================================

#[test]
fn test_rate_limit_zero_limit_and_division_safety() {
    // Zero limit with zero remaining
    assert_eq!(
        RateLimitGaugeState::calc_tier(0, 0),
        RateLimitColorTier::Red
    );

    // Zero limit with positive remaining (anomalous upstream header)
    assert_eq!(
        RateLimitGaugeState::calc_tier(5, 0),
        RateLimitColorTier::Red
    );

    // Constructor max(1) protection against division by zero
    let state_zero_limit = RateLimitGaugeState::new(0, 0, 0, 0);
    assert_eq!(state_zero_limit.core_limit, 1);
    assert_eq!(state_zero_limit.search_limit, 1);
    assert_eq!(state_zero_limit.core_percentage(), 0.0);
    assert_eq!(state_zero_limit.search_percentage(), 0.0);

    // Verify view_gauge renders cleanly without panic or NaN
    let _element: iced::Element<'_, ()> = view_gauge(&state_zero_limit);
}

#[test]
fn test_rate_limit_percentage_and_tier_threshold_boundaries() {
    // 100% full
    assert_eq!(
        RateLimitGaugeState::calc_tier(60, 60),
        RateLimitColorTier::Green
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(10, 10),
        RateLimitColorTier::Green
    );

    // Just above 50% threshold (> 0.50)
    assert_eq!(
        RateLimitGaugeState::calc_tier(31, 60),
        RateLimitColorTier::Green
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(501, 1000),
        RateLimitColorTier::Green
    );

    // Exactly 50% boundary (pct == 0.50 -> Amber)
    assert_eq!(
        RateLimitGaugeState::calc_tier(30, 60),
        RateLimitColorTier::Amber
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(5, 10),
        RateLimitColorTier::Amber
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(500, 1000),
        RateLimitColorTier::Amber
    );

    // Between 10% and 50% (e.g. 20%)
    assert_eq!(
        RateLimitGaugeState::calc_tier(12, 60),
        RateLimitColorTier::Amber
    ); // 20%
    assert_eq!(
        RateLimitGaugeState::calc_tier(2, 10),
        RateLimitColorTier::Amber
    ); // 20%

    // Exactly 10% boundary (pct == 0.10 -> Amber)
    assert_eq!(
        RateLimitGaugeState::calc_tier(6, 60),
        RateLimitColorTier::Amber
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(1, 10),
        RateLimitColorTier::Amber
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(100, 1000),
        RateLimitColorTier::Amber
    );

    // Just below 10% threshold (< 0.10 -> Red)
    assert_eq!(
        RateLimitGaugeState::calc_tier(5, 60),
        RateLimitColorTier::Red
    ); // 8.33%
    assert_eq!(
        RateLimitGaugeState::calc_tier(99, 1000),
        RateLimitColorTier::Red
    ); // 9.9%
    assert_eq!(
        RateLimitGaugeState::calc_tier(3, 60),
        RateLimitColorTier::Red
    ); // 5%

    // 0% remaining
    assert_eq!(
        RateLimitGaugeState::calc_tier(0, 60),
        RateLimitColorTier::Red
    );
    assert_eq!(
        RateLimitGaugeState::calc_tier(0, 10),
        RateLimitColorTier::Red
    );
}

#[test]
fn test_rate_limit_overflow_and_large_values() {
    // Remaining > Limit (e.g., elevated quota after token add or clock reset)
    let overflow_state = RateLimitGaugeState::new(120, 60, 20, 10);
    assert_eq!(overflow_state.core_percentage(), 1.0);
    assert_eq!(overflow_state.search_percentage(), 1.0);
    assert_eq!(overflow_state.core_tier(), RateLimitColorTier::Green);
    assert_eq!(overflow_state.search_tier(), RateLimitColorTier::Green);

    // u32::MAX limit and remaining
    let max_state = RateLimitGaugeState::new(u32::MAX, u32::MAX, 0, u32::MAX);
    assert_eq!(max_state.core_tier(), RateLimitColorTier::Green);
    assert_eq!(max_state.search_tier(), RateLimitColorTier::Red);
    assert!((max_state.core_percentage() - 1.0).abs() < 1e-4);
    assert_eq!(max_state.search_percentage(), 0.0);

    // Search throttling threshold (< 2)
    assert!(RateLimitGaugeState::new(60, 60, 0, 10).is_search_low());
    assert!(RateLimitGaugeState::new(60, 60, 1, 10).is_search_low());
    assert!(!RateLimitGaugeState::new(60, 60, 2, 10).is_search_low());
    assert!(!RateLimitGaugeState::new(60, 60, 5, 10).is_search_low());

    // Gauge render verification with max values
    let _element: iced::Element<'_, ()> = view_gauge(&max_state);
}

// =========================================================================
// 2. DEBOUNCE TIMING & QUERY CANCELLATION STRESS TESTS
// =========================================================================

#[test]
fn test_debounce_rapid_query_changes_within_300ms() {
    let mut state = SearchState::new(Vec::new());

    // Rapid keystrokes: "w", "wo", "wod", "wode", "woderrise" within 50ms intervals
    state.set_query("w".into(), 100);
    assert!(!state.should_fire_debounce(140, 300));

    state.set_query("wo".into(), 150);
    assert!(!state.should_fire_debounce(190, 300));

    state.set_query("wod".into(), 200);
    assert!(!state.should_fire_debounce(240, 300));

    state.set_query("wode".into(), 250);
    assert!(!state.should_fire_debounce(290, 300));

    state.set_query("woderrise".into(), 300);
    // At t = 599ms (299ms elapsed), debounce must NOT fire
    assert!(!state.should_fire_debounce(599, 300));

    // At t = 600ms (exactly 300ms elapsed), debounce MUST fire
    assert!(state.should_fire_debounce(600, 300));

    // At t = 700ms (400ms elapsed), debounce still fires until cleared
    assert!(state.should_fire_debounce(700, 300));

    // After clearing, must not fire again
    state.clear_debounce();
    assert!(!state.should_fire_debounce(750, 300));
}

#[test]
fn test_debounce_backwards_clock_skew_and_zero_window() {
    let mut state = SearchState::new(Vec::new());
    state.set_query("query".into(), 5000);

    // Time moving backwards (e.g., NTP adjustment during keystrokes)
    // saturating_sub should return 0 without underflow panic
    assert!(!state.should_fire_debounce(4000, 300));
    assert!(!state.should_fire_debounce(0, 300));

    // Debounce window of 0ms should fire immediately on next check
    assert!(state.should_fire_debounce(5000, 0));
}

#[test]
fn test_empty_and_whitespace_query_fallback() {
    let repo1 = dummy_repo(
        1,
        "author1",
        "rice1",
        "Hyprland config",
        vec![ConfigComponent::Hyprland],
        vec!["hyprland"],
    );
    let repo2 = dummy_repo(
        2,
        "author2",
        "bar2",
        "Waybar setup",
        vec![ConfigComponent::Waybar],
        vec!["waybar"],
    );

    let mut state = SearchState::new(vec![repo1, repo2]);
    assert_eq!(state.filtered_repositories.len(), 2);

    // Filter to single result
    state.set_query("bar2".into(), 100);
    assert_eq!(state.filtered_repositories.len(), 1);

    // Whitespace query falls back to full local catalog
    state.set_query("   \t  \n ".into(), 200);
    assert_eq!(state.filtered_repositories.len(), 2);

    // Empty query falls back to full local catalog
    state.set_query("".into(), 300);
    assert_eq!(state.filtered_repositories.len(), 2);
}

// =========================================================================
// 3. CATEGORY FILTERING & ADVERSARIAL QUERY MATRIX
// =========================================================================

#[test]
fn test_multi_component_composite_category_matching() {
    // Complete rice suite with Hyprland, Waybar, Hyprlock, and Wallpapers
    let suite = dummy_repo(
        1,
        "catppuccin",
        "complete-suite",
        "Full desktop environment suite",
        vec![
            ConfigComponent::HyprlandLegacy,
            ConfigComponent::Waybar,
            ConfigComponent::Hyprlock,
            ConfigComponent::Wallpapers,
        ],
        vec!["suite", "catppuccin"],
    );

    let standalone_bar = dummy_repo(
        2,
        "alex",
        "waybar-only",
        "Just a waybar config",
        vec![ConfigComponent::Waybar],
        vec!["waybar"],
    );

    let mut state = SearchState::new(vec![suite, standalone_bar]);

    // All matches both
    state.set_filter(CategoryFilter::All);
    assert_eq!(state.filtered_repositories.len(), 2);

    // Hyprland matches only the suite
    state.set_filter(CategoryFilter::Hyprland);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(state.filtered_repositories[0].repo, "complete-suite");

    // Waybar matches BOTH
    state.set_filter(CategoryFilter::Waybar);
    assert_eq!(state.filtered_repositories.len(), 2);

    // Wallpapers matches only the suite
    state.set_filter(CategoryFilter::Wallpapers);
    assert_eq!(state.filtered_repositories.len(), 1);
    assert_eq!(state.filtered_repositories[0].repo, "complete-suite");

    // Hypridle matches NEITHER
    state.set_filter(CategoryFilter::Hypridle);
    assert_eq!(state.filtered_repositories.len(), 0);

    // GTK matches NEITHER
    state.set_filter(CategoryFilter::Gtk);
    assert_eq!(state.filtered_repositories.len(), 0);
}

#[test]
fn test_category_query_case_insensitivity_and_special_chars() {
    let repo = dummy_repo(
        1,
        "User-Name",
        "C++-Waybar_Widget.v2",
        "Cool [Nord] theme with (dots) & symbols!",
        vec![ConfigComponent::Waybar],
        vec!["C++", "Nord-Dark", "GUI.Tool"],
    );

    let mut state = SearchState::new(vec![repo]);

    // Case variations
    state.set_query("USER-NAME".into(), 10);
    assert_eq!(state.filtered_repositories.len(), 1);

    state.set_query("c++-waybar".into(), 20);
    assert_eq!(state.filtered_repositories.len(), 1);

    state.set_query("NORD-DARK".into(), 30);
    assert_eq!(state.filtered_repositories.len(), 1);

    // Punctuation and regex-like syntax
    state.set_query("[nord]".into(), 40);
    assert_eq!(state.filtered_repositories.len(), 1);

    state.set_query("(dots)".into(), 50);
    assert_eq!(state.filtered_repositories.len(), 1);

    state.set_query(".*".into(), 60);
    assert_eq!(state.filtered_repositories.len(), 0); // Must be literal substring, not regex match-all
}

#[test]
fn test_unicode_and_emoji_search_queries() {
    let repo = dummy_repo(
        1,
        "global",
        "tokyo-night",
        "東京の夜の美しさ 🌸 Japanese aesthetic rice",
        vec![ConfigComponent::Hyprland],
        vec!["sakura", "東京", "aesthetic"],
    );

    let mut state = SearchState::new(vec![repo]);

    state.set_query("東京".into(), 10);
    assert_eq!(state.filtered_repositories.len(), 1);

    state.set_query("🌸".into(), 20);
    assert_eq!(state.filtered_repositories.len(), 1);

    state.set_query("Kyoto".into(), 30);
    assert_eq!(state.filtered_repositories.len(), 0);
}

// =========================================================================
// 4. DIFF VIEW EDGE CASES & STRESS HARNESS
// =========================================================================

#[test]
fn test_diff_empty_files_both_sides() {
    let report = FileDiffReport::compute("empty.conf", "", "");
    assert_eq!(report.lines.len(), 0);
    assert_eq!(report.additions, 0);
    assert_eq!(report.deletions, 0);

    let _element: iced::Element<'_, ()> = view_diff(&report);
}

#[test]
fn test_diff_completely_new_file() {
    let new_content = "exec-once = waybar\nwindowrule = float, ^(kitty)$\n";
    let report = FileDiffReport::compute("hyprland.conf", "", new_content);

    assert_eq!(report.additions, 2);
    assert_eq!(report.deletions, 0);
    assert_eq!(report.lines.len(), 2);

    for (idx, line) in report.lines.iter().enumerate() {
        assert_eq!(line.tag, similar::ChangeTag::Insert);
        assert_eq!(line.old_line, None);
        assert_eq!(line.new_line, Some(idx + 1));
    }

    let _element: iced::Element<'_, ()> = view_diff(&report);
}

#[test]
fn test_diff_completely_deleted_file() {
    let old_content = "line1\nline2\nline3\n";
    let report = FileDiffReport::compute("obsolete.conf", old_content, "");

    assert_eq!(report.additions, 0);
    assert_eq!(report.deletions, 3);
    assert_eq!(report.lines.len(), 3);

    for (idx, line) in report.lines.iter().enumerate() {
        assert_eq!(line.tag, similar::ChangeTag::Delete);
        assert_eq!(line.old_line, Some(idx + 1));
        assert_eq!(line.new_line, None);
    }

    let _element: iced::Element<'_, ()> = view_diff(&report);
}

#[test]
fn test_diff_identical_files() {
    let content = "same line 1\nsame line 2\nsame line 3\n";
    let report = FileDiffReport::compute("config.jsonc", content, content);

    assert_eq!(report.additions, 0);
    assert_eq!(report.deletions, 0);
    assert_eq!(report.lines.len(), 3);

    for (idx, line) in report.lines.iter().enumerate() {
        assert_eq!(line.tag, similar::ChangeTag::Equal);
        assert_eq!(line.old_line, Some(idx + 1));
        assert_eq!(line.new_line, Some(idx + 1));
    }

    let _element: iced::Element<'_, ()> = view_diff(&report);
}

#[test]
fn test_diff_no_trailing_newlines_and_crlf_handling() {
    let old_content = "line1\r\nline2";
    let new_content = "line1\nline2_modified\nline3";

    let report = FileDiffReport::compute("hyprlock.conf", old_content, new_content);
    // Verified compute does not panic and trims trailing CR/LF properly
    for line in &report.lines {
        assert!(!line.content.ends_with('\r'));
        assert!(!line.content.ends_with('\n'));
    }

    let _element: iced::Element<'_, ()> = view_diff(&report);
}

#[test]
fn test_diff_large_payload_stress_harness() {
    // Generate 5,000 lines diff
    let mut old_buf = String::with_capacity(100_000);
    let mut new_buf = String::with_capacity(100_000);

    for i in 0..5_000 {
        use std::fmt::Write;
        let _ = writeln!(old_buf, "key_{} = value_{};", i, i);
        if i % 5 == 0 {
            let _ = writeln!(new_buf, "key_{} = MODIFIED_value_{};", i, i);
        } else {
            let _ = writeln!(new_buf, "key_{} = value_{};", i, i);
        }
    }

    let start = std::time::Instant::now();
    let report = FileDiffReport::compute("huge_config.conf", &old_buf, &new_buf);
    let elapsed = start.elapsed();

    // Must compute in reasonable time (< 500ms)
    assert!(
        elapsed.as_millis() < 500,
        "Diff took too long: {:?}",
        elapsed
    );
    assert_eq!(report.additions, 1000);
    assert_eq!(report.deletions, 1000);

    // Verify Iced view generation on 5,000+ line diff doesn't panic
    let _element: iced::Element<'_, ()> = view_diff(&report);
}

// =========================================================================
// 5. INSPECT & INSTALL VIEW STATE RESILIENCE
// =========================================================================

#[test]
fn test_inspect_state_component_toggle_idempotency() {
    let repo = dummy_repo(
        1,
        "tester",
        "toggle-test",
        "Toggle test",
        vec![ConfigComponent::Waybar, ConfigComponent::Hyprlock],
        vec![],
    );

    let mut inspect = InspectState::new(repo, Vec::new(), Vec::new());
    assert_eq!(inspect.selected_components.len(), 2);
    assert!(inspect.is_install_allowed());

    // Toggle Waybar off
    inspect.toggle_component(ConfigComponent::Waybar);
    assert_eq!(inspect.selected_components.len(), 1);
    assert!(inspect
        .selected_components
        .contains(&ConfigComponent::Hyprlock));
    assert!(inspect.is_install_allowed());

    // Toggle Hyprlock off -> 0 selected -> disallowed
    inspect.toggle_component(ConfigComponent::Hyprlock);
    assert_eq!(inspect.selected_components.len(), 0);
    assert!(!inspect.is_install_allowed());

    // Toggle Waybar on again -> allowed
    inspect.toggle_component(ConfigComponent::Waybar);
    assert_eq!(inspect.selected_components.len(), 1);
    assert!(inspect.is_install_allowed());
}

#[test]
fn test_install_state_progress_ratio_clamp() {
    let repo = dummy_repo(1, "t", "r", "d", vec![ConfigComponent::Hyprland], vec![]);
    let mut install = InstallState::new(
        "tx_clamp".into(),
        repo,
        vec![ConfigComponent::Hyprland],
        vec![],
    );

    // Progress above 1.0 clamped to 1.0
    install.advance_phase(InstallPhase::Staging, 1.85, "Testing overflow");
    assert_eq!(install.progress_ratio, 1.0);

    // Progress below 0.0 clamped to 0.0
    install.advance_phase(InstallPhase::Backup, -0.42, "Testing underflow");
    assert_eq!(install.progress_ratio, 0.0);
}

// =========================================================================
// 6. HISTORY & STRICT LIFO ROLLBACK CONSTRAINTS
// =========================================================================

#[test]
fn test_history_interleaved_failed_and_rolled_back_transactions() {
    // History contains non-committed transactions mixed with committed ones
    let tx1 = JournalEntry {
        id: "tx_1".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Hyprland,
        status: TransactionStatus::Committed,
        source_repo: "a/b".into(),
        source_commit: "c1".into(),
        backup_directory: Utf8PathBuf::from("/b1"),
        operations: vec![],
    };
    let tx2 = JournalEntry {
        id: "tx_2".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Waybar,
        status: TransactionStatus::Failed, // Failed transaction in between
        source_repo: "a/b".into(),
        source_commit: "c2".into(),
        backup_directory: Utf8PathBuf::from("/b2"),
        operations: vec![],
    };
    let tx3 = JournalEntry {
        id: "tx_3".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Wallpapers,
        status: TransactionStatus::RolledBack, // Already rolled back
        source_repo: "a/b".into(),
        source_commit: "c3".into(),
        backup_directory: Utf8PathBuf::from("/b3"),
        operations: vec![],
    };
    let tx4 = JournalEntry {
        id: "tx_4".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::GtkTheme,
        status: TransactionStatus::Committed, // Latest committed
        source_repo: "a/b".into(),
        source_commit: "c4".into(),
        backup_directory: Utf8PathBuf::from("/b4"),
        operations: vec![],
    };

    // In reverse chronological order: [tx4 (Committed), tx3 (RolledBack), tx2 (Failed), tx1 (Committed)]
    let history = HistoryState::from_journal_entries(&[tx4, tx3, tx2, tx1]);

    assert_eq!(history.transactions.len(), 4);
    assert!(history.transactions[0].is_latest_committed); // tx4 is eligible
    assert!(!history.transactions[1].is_latest_committed); // tx3 is RolledBack
    assert!(!history.transactions[2].is_latest_committed); // tx2 is Failed
    assert!(!history.transactions[3].is_latest_committed); // tx1 is an older committed transaction (LIFO blocked)
}
