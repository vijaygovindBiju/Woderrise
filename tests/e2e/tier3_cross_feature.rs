//! Tier 3: Cross-Feature Interactions & Combinatorial State Transitions
//! Verifies interactions between PAT elevation, fuzzy search, pre-inspection, and feed fallback.

use super::common::TestEnvironment;
use camino::Utf8PathBuf;
use std::fs;
use tempfile::TempDir;
use woderrise_core::models::{
    load_settings, CatalogRepository, ConfigComponent, DetectedComponent, SyntaxFormat,
};
use woderrise_detect::DetectionEngine;
use woderrise_github::search::aggregate_topics;
use woderrise_github::RateLimitTracker;
use woderrise_gui::app::{App, EngineEvent, Message};
use woderrise_gui::resolve_catalog_4tier;
use woderrise_gui::screens::search::CategoryFilter;

#[test]
fn test_tier3_pat_elevation_multi_topic_search_and_pre_inspection() {
    let env = TestEnvironment::new("t3-full-lifecycle");

    // Phase 1: Unauthenticated Initial State
    let out_s0 = env.run_auth(&["--status"]);
    assert_eq!(out_s0.status.code(), Some(0));
    let s0 = String::from_utf8_lossy(&out_s0.stdout);
    assert!(s0.contains("Unauthenticated"));
    assert!(s0.contains("10 requests/min"));
    assert!(s0.contains("60 requests/hr"));

    // Phase 2: Save GitHub PAT via CLI
    let token = "ghp_cross_feature_e2e_token_9876543210";
    let out_auth = env.run_auth(&["--token", token]);
    assert_eq!(out_auth.status.code(), Some(0));
    assert!(env.settings_file().exists());

    // Phase 3: Status reflects elevation
    let out_s1 = env.run_auth(&["--status"]);
    assert_eq!(out_s1.status.code(), Some(0));
    let s1 = String::from_utf8_lossy(&out_s1.stdout);
    assert!(s1.contains("Authenticated"));
    assert!(s1.contains("30 requests/min"));
    assert!(s1.contains("5,000 requests/hr"));

    // Verify rate limit tracker elevation logic
    let tracker = RateLimitTracker::new();
    tracker.set_authenticated(true);
    assert_eq!(tracker.search_status().limit, 30);
    assert_eq!(tracker.core_status().limit, 5000);

    // Phase 4: Multi-Topic Aggregation with elevated limits
    let repo_alpha = CatalogRepository {
        id: 4001,
        owner: "developer-a".into(),
        repo: "dotfiles-hypr".into(),
        full_name: "developer-a/dotfiles-hypr".into(),
        html_url: "https://github.com/developer-a/dotfiles-hypr".into(),
        description: Some("Hyprland dotfiles".into()),
        stargazers_count: 650,
        default_branch: "main".into(),
        topics: vec!["hyprland".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };
    let repo_beta = CatalogRepository {
        id: 4002,
        owner: "developer-b".into(),
        repo: "waybar-master".into(),
        full_name: "developer-b/waybar-master".into(),
        html_url: "https://github.com/developer-b/waybar-master".into(),
        description: Some("Waybar configurations".into()),
        stargazers_count: 1200,
        default_branch: "main".into(),
        topics: vec!["waybar".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let aggregated = aggregate_topics(vec![vec![repo_alpha], vec![repo_beta]]);
    assert_eq!(aggregated.len(), 2);
    assert_eq!(aggregated[0].id, 4002); // 1200 stars ranked first
    assert_eq!(aggregated[1].id, 4001); // 650 stars ranked second

    // Phase 5: Eager tree pre-inspection
    let engine = DetectionEngine::new();
    let entries = vec![
        woderrise_detect::TreeEntry {
            path: ".config/waybar/config".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(1500),
            sha: "sha_wb".into(),
        },
        woderrise_detect::TreeEntry {
            path: ".config/waybar/style.css".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(2500),
            sha: "sha_style".into(),
        },
    ];
    let components = engine.detect_components(&entries);
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].component, ConfigComponent::Waybar);
    assert_eq!(components[0].file_count, 2);

    // Phase 6: Clear PAT via CLI
    let out_clear = env.run_auth(&["--clear"]);
    assert_eq!(out_clear.status.code(), Some(0));

    let out_s2 = env.run_auth(&["--status"]);
    assert_eq!(out_s2.status.code(), Some(0));
    let s2 = String::from_utf8_lossy(&out_s2.stdout);
    assert!(s2.contains("Unauthenticated"));
    assert!(s2.contains("10 requests/min"));
    assert!(s2.contains("60 requests/hr"));
}

#[test]
fn test_tier3_fuzzy_search_to_pre_inspection_to_category_filter() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(100);
    let mut app = App::new(cmd_tx, None);

    // 1. User searches with typo "hyprlnd"
    let _ = app.update(Message::SearchInputChanged("hyprlnd".into()));

    // 2. Add an uninspected bare repository to catalog
    let bare_repo = CatalogRepository {
        id: 5001,
        owner: "custom-user".into(),
        repo: "custom-setup".into(),
        full_name: "custom-user/custom-setup".into(),
        html_url: "https://github.com/custom-user/custom-setup".into(),
        description: Some("Custom Wayland environment".into()),
        stargazers_count: 450,
        default_branch: "main".into(),
        topics: vec!["hyprland".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    app.search_state.catalog_repositories = vec![bare_repo];
    app.search_state.apply_filter();

    // Initially with All filter and query "hyprlnd", matches via topic "hyprland"
    assert_eq!(app.search_state.filtered_repositories.len(), 1);

    // 3. Switch category filter to Waybar (currently 0 matches because Waybar not yet detected)
    app.search_state.set_filter(CategoryFilter::Waybar);
    assert_eq!(app.search_state.filtered_repositories.len(), 0);

    // 4. Background tree pre-inspection finishes, detecting a Waybar component
    let waybar_detected = DetectedComponent {
        component: ConfigComponent::Waybar,
        syntax_format: SyntaxFormat::Jsonc,
        root_path: ".config/waybar".into(),
        entrypoint: "config.jsonc".into(),
        style_file: Some("style.css".into()),
        file_count: 2,
        total_size_bytes: 3500,
    };

    let _ = app.update(Message::Engine(EngineEvent::RepoPreInspected {
        repo: "custom-user/custom-setup".into(),
        components: vec![waybar_detected],
    }));

    // 5. Filtered repositories dynamically update to include the repo under Waybar filter
    assert_eq!(
        app.search_state.filtered_repositories.len(),
        1,
        "Pre-inspected Waybar component must cause repo to appear in Waybar category filter"
    );
    assert_eq!(
        app.search_state.filtered_repositories[0].resolved_full_name(),
        "custom-user/custom-setup"
    );
}

#[tokio::test]
async fn test_tier3_offline_transition_with_pat_and_curated_feed() {
    let env = TestEnvironment::new("t3-offline-feed");

    // Configure PAT in settings
    let _ = env.run_auth(&["--token", "ghp_offline_resilience_test_token_123"]);
    let settings_file = env.settings_file();
    assert!(settings_file.exists());

    // Simulate complete offline status by providing unreachable CDN URL and empty local cache
    let temp = TempDir::new().unwrap();
    let cache_file = Utf8PathBuf::from_path_buf(temp.path().join("offline_cache.json")).unwrap();
    let offline_cdn = "http://127.0.0.1:9/offline_catalog.json";

    // 4-tier fallback runs offline
    let catalog = resolve_catalog_4tier(Some(offline_cdn), Some(&cache_file)).await;

    // Must gracefully resolve bundled catalog without errors or hangs
    assert!(!catalog.repositories.is_empty());
    assert_eq!(catalog.version, 1);

    // Verify settings remains intact and secure (POSIX 0600)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Read metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
    let loaded =
        load_settings(&camino::Utf8PathBuf::from_path_buf(settings_file).unwrap()).unwrap();
    assert_eq!(
        loaded.github_token.as_deref(),
        Some("ghp_offline_resilience_test_token_123")
    );
}
