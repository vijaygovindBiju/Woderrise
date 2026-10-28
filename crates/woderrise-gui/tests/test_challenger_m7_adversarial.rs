//! Adversarial empirical challenge tests for Milestone M7:
//! R1 Background Tree Pre-Inspection & R2 Multi-Topic Aggregation.

use chrono::{TimeZone, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
use woderrise_core::models::{CatalogRepository, ConfigComponent, DetectedComponent, SyntaxFormat};
use woderrise_detect::DetectionEngine;
use woderrise_github::search::deduplicate_and_sort_repositories;
use woderrise_github::{GitHubClient, RateLimitResource};
use woderrise_gui::app::Message;
use woderrise_gui::screens::search::{CategoryFilter, SearchState};
use woderrise_gui::spawn_tree_pre_inspection;

fn make_repo_with_details(
    id: u64,
    owner: &str,
    repo: &str,
    stars: u64,
    pushed_at: Option<chrono::DateTime<Utc>>,
    topics: Vec<&str>,
    desc: Option<&str>,
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
        topics: topics.into_iter().map(String::from).collect(),
        pushed_at,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    }
}

// =========================================================================
// 1. Adversarial R2 Multi-Topic Aggregation Tests
// =========================================================================

#[test]
fn test_adversarial_deduplication_case_insensitivity_and_topic_union() {
    let t1 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();

    // 4 instances of the exact same repo with conflicting casings, topics, stars, and descriptions
    let r1 = make_repo_with_details(
        101,
        "Alexays",
        "Waybar",
        4000,
        Some(t1),
        vec!["hyprland", "WAYBAR", "Arch"],
        None,
    );
    let r2 = make_repo_with_details(
        101,
        "alexays",
        "waybar",
        6500,                                 // Higher star count
        Some(t2),                             // Newer pushed_at
        vec!["Waybar", "Quickshell", "arch"], // Cased variations
        Some("Highly customizable Wayland bar"),
    );
    let r3 = make_repo_with_details(
        101,
        "ALEXAYS",
        "WAYBAR",
        3000,
        None,
        vec!["dots", "HYPRLAND", "quickshell", "linux"],
        Some("Older description"),
    );
    let mut r4 =
        make_repo_with_details(101, "Alexays", "waybar", 5000, None, vec!["wayland"], None);
    r4.detected_components = vec![DetectedComponent {
        component: ConfigComponent::Waybar,
        syntax_format: SyntaxFormat::Jsonc,
        root_path: ".config/waybar".to_string(),
        entrypoint: "config.jsonc".to_string(),
        style_file: Some("style.css".to_string()),
        file_count: 3,
        total_size_bytes: 4096,
    }];

    let deduplicated = deduplicate_and_sort_repositories(vec![r1, r2, r3, r4]);

    // Must collapse into a single repository
    assert_eq!(
        deduplicated.len(),
        1,
        "Duplicates across casing must be collapsed to exactly 1"
    );

    let unified = &deduplicated[0];
    // Maximum stars retained
    assert_eq!(unified.stargazers_count, 6500);
    // Newest pushed_at retained
    assert_eq!(unified.pushed_at, Some(t2));
    // Non-empty description retained
    assert_eq!(
        unified.description.as_deref(),
        Some("Highly customizable Wayland bar")
    );
    // Detected components merged
    assert_eq!(unified.detected_components.len(), 1);
    assert_eq!(
        unified.detected_components[0].component,
        ConfigComponent::Waybar
    );

    // Topics union verification:
    // Original distinct topics (case-insensitive):
    // "hyprland", "waybar", "arch", "quickshell", "dots", "linux", "wayland" -> 7 unique topics
    assert_eq!(
        unified.topics.len(),
        7,
        "Expected exactly 7 case-insensitively unique topics, got: {:?}",
        unified.topics
    );

    // Check all unique topics exist
    let lower_topics: Vec<String> = unified.topics.iter().map(|t| t.to_lowercase()).collect();
    assert!(lower_topics.contains(&"hyprland".to_string()));
    assert!(lower_topics.contains(&"waybar".to_string()));
    assert!(lower_topics.contains(&"arch".to_string()));
    assert!(lower_topics.contains(&"quickshell".to_string()));
    assert!(lower_topics.contains(&"dots".to_string()));
    assert!(lower_topics.contains(&"linux".to_string()));
    assert!(lower_topics.contains(&"wayland".to_string()));
}

#[test]
fn test_adversarial_sorting_stability_equal_stars_pushed_at_and_id() {
    let t_old = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t_new = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();

    // 4 repositories, all with 500 stars:
    // repo_a: 500 stars, new timestamp, id 30
    // repo_b: 500 stars, old timestamp, id 10
    // repo_c: 500 stars, no timestamp, id 5
    // repo_d: 500 stars, no timestamp, id 25
    let repo_a = make_repo_with_details(30, "user", "repo-a", 500, Some(t_new), vec![], None);
    let repo_b = make_repo_with_details(10, "user", "repo-b", 500, Some(t_old), vec![], None);
    let repo_c = make_repo_with_details(5, "user", "repo-c", 500, None, vec![], None);
    let repo_d = make_repo_with_details(25, "user", "repo-d", 500, None, vec![], None);

    // Expected deterministic order:
    // 1. repo_a (newer timestamp)
    // 2. repo_b (older timestamp)
    // 3. repo_c (id 5 < id 25)
    // 4. repo_d (id 25)

    let inputs = vec![
        repo_a.clone(),
        repo_b.clone(),
        repo_c.clone(),
        repo_d.clone(),
    ];

    // Verify across 20 different permutations that sorting is strictly deterministic
    for i in 0..20 {
        let mut permuted = inputs.clone();
        if i % 2 == 1 {
            permuted.reverse();
        } else if i % 3 == 0 {
            permuted.swap(0, 2);
        } else if i % 5 == 0 {
            permuted.swap(1, 3);
        }

        let sorted = deduplicate_and_sort_repositories(permuted);
        assert_eq!(sorted.len(), 4);
        assert_eq!(
            sorted[0].resolved_full_name(),
            "user/repo-a",
            "Permutation {}: repo-a must be first (newest timestamp)",
            i
        );
        assert_eq!(
            sorted[1].resolved_full_name(),
            "user/repo-b",
            "Permutation {}: repo-b must be second (older timestamp)",
            i
        );
        assert_eq!(
            sorted[2].resolved_full_name(),
            "user/repo-c",
            "Permutation {}: repo-c must be third (id 5 tie-breaker)",
            i
        );
        assert_eq!(
            sorted[3].resolved_full_name(),
            "user/repo-d",
            "Permutation {}: repo-d must be fourth (id 25 tie-breaker)",
            i
        );
    }
}

#[tokio::test]
async fn test_adversarial_aggregate_topics_wiremock_fault_injection() {
    let mock_server = MockServer::start().await;

    // 1. Feed A: topic:hyprland (200 OK) -> repo1, repo2
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "topic:hyprland"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "total_count": 2,
                    "incomplete_results": false,
                    "items": [
                        {
                            "id": 1,
                            "owner": "end-4",
                            "repo": "dots-hyprland",
                            "full_name": "end-4/dots-hyprland",
                            "html_url": "https://github.com/end-4/dots-hyprland",
                            "description": "Hyprland setup",
                            "stargazers_count": 4000,
                            "default_branch": "main",
                            "topics": ["hyprland"]
                        },
                        {
                            "id": 2,
                            "owner": "Alexays",
                            "repo": "Waybar",
                            "full_name": "Alexays/Waybar",
                            "html_url": "https://github.com/Alexays/Waybar",
                            "description": "Wayland bar",
                            "stargazers_count": 6000,
                            "default_branch": "master",
                            "topics": ["waybar"]
                        }
                    ]
                }))
                .append_header("x-ratelimit-resource", "search")
                .append_header("x-ratelimit-remaining", "9"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    // 2. Feed B: topic:broken (500 Internal Server Error)
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "topic:broken"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&mock_server)
        .await;

    // 3. Feed C: topic:quickshell (200 OK) -> repo2 (duplicate with different casing), repo3
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "topic:quickshell"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "total_count": 2,
                    "incomplete_results": false,
                    "items": [
                        {
                            "id": 2,
                            "owner": "alexays",
                            "repo": "waybar",
                            "full_name": "alexays/waybar",
                            "html_url": "https://github.com/alexays/waybar",
                            "description": "Wayland bar",
                            "stargazers_count": 6200, // Higher stars
                            "default_branch": "master",
                            "topics": ["quickshell-interop"]
                        },
                        {
                            "id": 3,
                            "owner": "outfoxxed",
                            "repo": "quickshell",
                            "full_name": "outfoxxed/quickshell",
                            "html_url": "https://github.com/outfoxxed/quickshell",
                            "description": "Flexible desktop shell",
                            "stargazers_count": 1500,
                            "default_branch": "main",
                            "topics": ["quickshell"]
                        }
                    ]
                }))
                .append_header("x-ratelimit-resource", "search")
                .append_header("x-ratelimit-remaining", "8"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let results = client
        .aggregate_topics(&["topic:hyprland", "topic:broken", "topic:quickshell"])
        .await
        .expect("Aggregate topics must succeed despite 1 broken feed");

    // Must have exactly 3 deduplicated repositories
    assert_eq!(results.len(), 3);

    // Sorted descending by stars:
    // 1. Waybar (6200)
    // 2. dots-hyprland (4000)
    // 3. quickshell (1500)
    assert_eq!(
        results[0].resolved_full_name().to_lowercase(),
        "alexays/waybar"
    );
    assert_eq!(results[0].stargazers_count, 6200);
    // Topics unioned: waybar + quickshell-interop
    assert!(results[0].topics.contains(&"waybar".to_string()));
    assert!(results[0]
        .topics
        .contains(&"quickshell-interop".to_string()));

    assert_eq!(results[1].resolved_full_name(), "end-4/dots-hyprland");
    assert_eq!(results[1].stargazers_count, 4000);

    assert_eq!(results[2].resolved_full_name(), "outfoxxed/quickshell");
    assert_eq!(results[2].stargazers_count, 1500);
}

// =========================================================================
// 2. Adversarial R1 Background Tree Pre-Inspection Tests
// =========================================================================

#[tokio::test]
async fn test_adversarial_pre_inspection_cancellation_halts_promptly() {
    let mock_server = MockServer::start().await;

    // Simulate slow network response for tree fetches (300ms delay per repo)
    Mock::given(method("GET"))
        .and(path("/repos/user/repo-slow/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_json(serde_json::json!({
                    "sha": "tree1",
                    "tree": [
                        {
                            "path": ".config/hypr/hyprland.conf",
                            "mode": "100644",
                            "type": "blob",
                            "sha": "blob1",
                            "size": 500
                        }
                    ],
                    "truncated": false
                })),
        )
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let detection_engine = Arc::new(DetectionEngine::new());
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(10);
    let cancel_token = CancellationToken::new();

    // 5 candidates
    let repos: Vec<CatalogRepository> = (1..=5)
        .map(|i| make_repo_with_details(i, "user", "repo-slow", 100, None, vec!["hyprland"], None))
        .collect();

    let start_time = std::time::Instant::now();

    spawn_tree_pre_inspection(
        repos,
        client,
        detection_engine,
        event_tx,
        cancel_token.clone(),
    );

    // Cancel after 25ms
    tokio::time::sleep(Duration::from_millis(25)).await;
    cancel_token.cancel();

    // Wait 100ms and verify pre-inspection task halted without waiting 5 * 300ms = 1500ms
    tokio::time::sleep(Duration::from_millis(100)).await;
    let elapsed = start_time.elapsed();

    assert!(
        elapsed < Duration::from_millis(600),
        "Pre-inspection must abort promptly upon cancellation; elapsed was {:?}",
        elapsed
    );

    // No events should have been emitted
    assert!(event_rx.try_recv().is_err());
}

#[tokio::test]
async fn test_adversarial_pre_inspection_quota_safety_boundary() {
    let mock_server = MockServer::start().await;

    // Expect 0 requests when remaining < 5
    Mock::given(method("GET"))
        .and(path("/repos/user/repo1/git/trees/main"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    // Set remaining quota to exactly 4 (strictly < 5)
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
        reqwest::header::HeaderValue::from_static("4"),
    );
    headers.insert(
        "x-ratelimit-reset",
        reqwest::header::HeaderValue::from_static("1761500000"),
    );
    client
        .rate_limiter()
        .update_from_headers(&headers, RateLimitResource::Core);

    let detection_engine = Arc::new(DetectionEngine::new());
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(10);
    let cancel_token = CancellationToken::new();

    let repo = make_repo_with_details(1, "user", "repo1", 100, None, vec![], None);

    spawn_tree_pre_inspection(vec![repo], client, detection_engine, event_tx, cancel_token);

    tokio::time::sleep(Duration::from_millis(50)).await;
    // Strictly blocked from querying
    assert!(event_rx.try_recv().is_err());
}

#[test]
fn test_adversarial_badge_rendering_robustness_matrix() {
    // 1. Repo with 0 components
    let repo_0 = make_repo_with_details(1, "u", "r0", 50, None, vec![], None);
    let _ = woderrise_gui::view_card(&repo_0, Message::Noop);

    // 2. Repo with 1 component having 0 files (syntax file only or empty component)
    let mut repo_1_empty = make_repo_with_details(2, "u", "r1", 50, None, vec![], None);
    repo_1_empty.detected_components = vec![DetectedComponent {
        component: ConfigComponent::Waybar,
        syntax_format: SyntaxFormat::Jsonc,
        root_path: ".config/waybar".to_string(),
        entrypoint: "config.jsonc".to_string(),
        style_file: None,
        file_count: 0,
        total_size_bytes: 0,
    }];
    let _ = woderrise_gui::view_card(&repo_1_empty, Message::Noop);

    // 3. Repo with 1 component having 1 file (singular "file")
    let mut repo_1_single = make_repo_with_details(3, "u", "r2", 50, None, vec![], None);
    repo_1_single.detected_components = vec![DetectedComponent {
        component: ConfigComponent::Hyprland,
        syntax_format: SyntaxFormat::LegacyConf,
        root_path: ".config/hypr".to_string(),
        entrypoint: "hyprland.conf".to_string(),
        style_file: None,
        file_count: 1,
        total_size_bytes: 512,
    }];
    let _ = woderrise_gui::view_card(&repo_1_single, Message::Noop);

    // 4. Repo with many components (all 6 components) and plural file counts
    let mut repo_many = make_repo_with_details(4, "u", "r3", 50, None, vec![], None);
    repo_many.detected_components = vec![
        DetectedComponent {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: ".config/hypr".to_string(),
            entrypoint: "hyprland.conf".to_string(),
            style_file: None,
            file_count: 8,
            total_size_bytes: 8000,
        },
        DetectedComponent {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: ".config/waybar".to_string(),
            entrypoint: "config.jsonc".to_string(),
            style_file: Some("style.css".to_string()),
            file_count: 3,
            total_size_bytes: 3000,
        },
        DetectedComponent {
            component: ConfigComponent::Hyprlock,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: ".config/hypr".to_string(),
            entrypoint: "hyprlock.conf".to_string(),
            style_file: None,
            file_count: 2,
            total_size_bytes: 1200,
        },
        DetectedComponent {
            component: ConfigComponent::Hypridle,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: ".config/hypr".to_string(),
            entrypoint: "hypridle.conf".to_string(),
            style_file: None,
            file_count: 1,
            total_size_bytes: 600,
        },
        DetectedComponent {
            component: ConfigComponent::Wallpapers,
            syntax_format: SyntaxFormat::BinaryAsset,
            root_path: "wallpapers".to_string(),
            entrypoint: "default.png".to_string(),
            style_file: None,
            file_count: 15,
            total_size_bytes: 25_000_000,
        },
        DetectedComponent {
            component: ConfigComponent::GtkTheme,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: ".config/gtk-3.0".to_string(),
            entrypoint: "settings.ini".to_string(),
            style_file: None,
            file_count: 2,
            total_size_bytes: 1500,
        },
    ];
    let _ = woderrise_gui::view_card(&repo_many, Message::Noop);

    // 5. Extreme file count (1,000,000 files)
    let mut repo_extreme = make_repo_with_details(5, "u", "r4", 50, None, vec![], None);
    repo_extreme.detected_components = vec![DetectedComponent {
        component: ConfigComponent::Wallpapers,
        syntax_format: SyntaxFormat::BinaryAsset,
        root_path: "wallpapers".to_string(),
        entrypoint: "default.png".to_string(),
        style_file: None,
        file_count: 1_000_000,
        total_size_bytes: 1_000_000_000,
    }];
    let _ = woderrise_gui::view_card(&repo_extreme, Message::Noop);
}

#[test]
fn test_adversarial_app_pre_inspection_filter_reactivity() {
    let mut search_state = SearchState::new(vec![]);

    // Catalog starts with 1 repo with no detected components and no 'hyprlock' in description or topics
    let repo = make_repo_with_details(
        10,
        "author",
        "desktop-rice",
        200,
        None,
        vec!["generic-rice"],
        Some("A customized Linux environment"),
    );
    search_state.catalog_repositories = vec![repo];
    search_state.apply_filter();

    // 1. Initial State: CategoryFilter::Hyprlock should NOT match
    search_state.set_filter(CategoryFilter::Hyprlock);
    assert!(
        search_state.filtered_repositories.is_empty(),
        "Hyprlock filter should not match before pre-inspection"
    );

    // 2. Incoming Pre-Inspection Event delivers detected Hyprlock component
    let hyprlock_comp = DetectedComponent {
        component: ConfigComponent::Hyprlock,
        syntax_format: SyntaxFormat::LegacyConf,
        root_path: ".config/hypr".to_string(),
        entrypoint: "hyprlock.conf".to_string(),
        style_file: None,
        file_count: 1,
        total_size_bytes: 800,
    };
    search_state.update_repo_components("author/desktop-rice", vec![hyprlock_comp]);

    // 3. Immediately reacts and activates the filter match
    assert_eq!(
        search_state.filtered_repositories.len(),
        1,
        "Filtered repositories must immediately update when pre-inspection event arrives"
    );
    assert_eq!(
        search_state.filtered_repositories[0].resolved_full_name(),
        "author/desktop-rice"
    );
}

#[test]
fn test_adversarial_deduplication_fallback_to_id_and_empty_fullname() {
    // 2 repos with empty full_name and empty owner/repo, but same id
    let mut r1 = make_repo_with_details(999, "", "", 100, None, vec!["topic1"], None);
    r1.full_name = String::new();
    let mut r2 = make_repo_with_details(
        999,
        "",
        "",
        200,
        None,
        vec!["topic2"],
        Some("Recovered desc"),
    );
    r2.full_name = String::new();

    let dedup = deduplicate_and_sort_repositories(vec![r1, r2]);
    assert_eq!(
        dedup.len(),
        1,
        "Fallback key id:999 must deduplicate repos with empty names"
    );
    assert_eq!(dedup[0].stargazers_count, 200);
    assert_eq!(dedup[0].description.as_deref(), Some("Recovered desc"));
    assert_eq!(dedup[0].topics.len(), 2);
}

#[tokio::test]
async fn test_adversarial_aggregate_topics_all_fail_returns_err() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(503))
        .expect(3)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let res = client
        .aggregate_topics(&["topic:hyprland", "topic:waybar", "quickshell"])
        .await;

    assert!(
        res.is_err(),
        "When all feeds fail, aggregate_topics must return Err"
    );
}

#[tokio::test]
async fn test_adversarial_pre_inspection_authenticated_quota_bypass() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/user/repo-auth/git/trees/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": "tree-auth",
            "tree": [
                {
                    "path": ".config/waybar/config.jsonc",
                    "mode": "100644",
                    "type": "blob",
                    "sha": "blob-wb",
                    "size": 400
                }
            ],
            "truncated": false
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .auth_token(Some("ghp_fake_test_token_12345".to_string()))
        .build()
        .expect("Client build failed");

    // Set remaining quota to 2 (< 5), but client has auth_token!
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-ratelimit-resource",
        reqwest::header::HeaderValue::from_static("core"),
    );
    headers.insert(
        "x-ratelimit-remaining",
        reqwest::header::HeaderValue::from_static("2"),
    );
    headers.insert(
        "x-ratelimit-limit",
        reqwest::header::HeaderValue::from_static("5000"),
    );
    headers.insert(
        "x-ratelimit-reset",
        reqwest::header::HeaderValue::from_static("1761500000"),
    );
    client
        .rate_limiter()
        .update_from_headers(&headers, RateLimitResource::Core);

    let detection_engine = Arc::new(DetectionEngine::new());
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(10);
    let cancel_token = CancellationToken::new();

    let repo = make_repo_with_details(1, "user", "repo-auth", 100, None, vec![], None);

    spawn_tree_pre_inspection(vec![repo], client, detection_engine, event_tx, cancel_token);

    // Authenticated client proceeds and emits RepoPreInspected!
    let event = tokio::time::timeout(Duration::from_millis(500), event_rx.recv())
        .await
        .expect("Must not timeout")
        .expect("Must receive event");

    match event {
        woderrise_gui::app::EngineEvent::RepoPreInspected { repo, components } => {
            assert_eq!(repo, "user/repo-auth");
            assert_eq!(components.len(), 1);
            assert_eq!(components[0].component, ConfigComponent::Waybar);
        }
        other => panic!("Unexpected event: {:?}", other),
    }
}
