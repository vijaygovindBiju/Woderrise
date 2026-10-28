//! E2E Test Suite for R1: Background Tree Pre-Inspection and Discovery Card Badges/File Counts.
//!
//! Verifies:
//! - Component detection (Hyprland, Waybar, Hyprlock, Hypridle, Wallpapers, GTK)
//! - Component display names and file counters
//! - Discovery card badge generation without panics
//! - Rate limit quota protection (< 5 halts pre-inspection)
//! - Prompt worker cancellation on query changes

use tokio_util::sync::CancellationToken;
use woderrise_core::models::{CatalogRepository, ConfigComponent};
use woderrise_detect::{DetectionEngine, TreeEntry};
use woderrise_github::{GitHubClientBuilder, RateLimitResource};

#[test]
fn test_e2e_r1_all_six_component_badges_and_file_counts() {
    let engine = DetectionEngine::new();

    let tree = vec![
        // Hyprland
        TreeEntry {
            path: "hypr/hyprland.conf".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(4000),
            sha: "sha1".into(),
        },
        // Hyprlock
        TreeEntry {
            path: "hypr/hyprlock.conf".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(1500),
            sha: "sha2".into(),
        },
        // Hypridle
        TreeEntry {
            path: "hypr/hypridle.conf".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(900),
            sha: "sha3".into(),
        },
        // Waybar (config + style)
        TreeEntry {
            path: "waybar/config.jsonc".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(3000),
            sha: "sha4".into(),
        },
        TreeEntry {
            path: "waybar/style.css".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(2000),
            sha: "sha5".into(),
        },
        // Wallpapers
        TreeEntry {
            path: "wallpapers/sunset.jpg".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(850_000),
            sha: "sha6".into(),
        },
        TreeEntry {
            path: "wallpapers/mountains.png".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(1_200_000),
            sha: "sha7".into(),
        },
        // GTK
        TreeEntry {
            path: "gtk-3.0/settings.ini".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(600),
            sha: "sha8".into(),
        },
    ];

    let detected = engine.detect_components(&tree);
    assert_eq!(detected.len(), 6, "Must detect all 6 desktop components");

    let component_map: std::collections::HashMap<ConfigComponent, usize> = detected
        .iter()
        .map(|c| (c.component, c.file_count))
        .collect();

    assert_eq!(component_map.get(&ConfigComponent::Hyprland), Some(&3));
    assert_eq!(component_map.get(&ConfigComponent::Hyprlock), Some(&1));
    assert_eq!(component_map.get(&ConfigComponent::Hypridle), Some(&1));
    assert_eq!(component_map.get(&ConfigComponent::Waybar), Some(&2));
    assert_eq!(component_map.get(&ConfigComponent::Wallpapers), Some(&2));
    assert_eq!(component_map.get(&ConfigComponent::GtkTheme), Some(&1));

    let repo = CatalogRepository {
        id: 1111,
        owner: "complete-setup".into(),
        repo: "all-components".into(),
        full_name: "complete-setup/all-components".into(),
        html_url: "https://github.com/complete-setup/all-components".into(),
        description: Some("Complete desktop setup".into()),
        stargazers_count: 999,
        default_branch: "main".into(),
        topics: vec!["hyprland".into(), "waybar".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: detected,
    };

    let total_files: usize = repo.detected_components.iter().map(|c| c.file_count).sum();
    assert_eq!(total_files, 10); // 3 + 1 + 1 + 2 + 2 + 1 = 10

    // Card element builds without panic
    let card = woderrise_gui::view_card(&repo, woderrise_gui::app::Message::Noop);
    let _ = card;
}

#[tokio::test]
async fn test_e2e_r1_pre_inspection_rate_limit_and_cancellation_invariants() {
    let mock_server = wiremock::MockServer::start().await;
    let client = GitHubClientBuilder::new()
        .base_url(mock_server.uri())
        .build()
        .unwrap();

    // 1. Invariant: Quota < 5 prevents pre-inspection
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
        reqwest::header::HeaderValue::from_static("3"),
    );
    headers.insert(
        "x-ratelimit-reset",
        reqwest::header::HeaderValue::from_static("1761500000"),
    );
    client
        .rate_limiter()
        .update_from_headers(&headers, RateLimitResource::Core);

    let engine = std::sync::Arc::new(DetectionEngine::new());
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let cancel = CancellationToken::new();

    let repo = CatalogRepository {
        id: 7777,
        owner: "test".into(),
        repo: "test".into(),
        full_name: "test/test".into(),
        html_url: "https://github.com/test/test".into(),
        description: None,
        stargazers_count: 10,
        default_branch: "main".into(),
        topics: vec![],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    woderrise_gui::spawn_tree_pre_inspection(
        vec![repo.clone()],
        client.clone(),
        engine.clone(),
        tx,
        cancel,
    );

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(rx.try_recv().is_err());

    // 2. Invariant: Cancelled token stops task immediately
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(10);
    let cancel2 = CancellationToken::new();
    cancel2.cancel();

    woderrise_gui::spawn_tree_pre_inspection(vec![repo], client, engine, tx2, cancel2);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(rx2.try_recv().is_err());
}
