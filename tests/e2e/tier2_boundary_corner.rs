//! Tier 2: Boundary & Corner Cases (Adversarial)
//! Exercises edge conditions, hostile queries, and boundary inputs across R1-R5.

use super::common::TestEnvironment;
use camino::Utf8PathBuf;
use std::fs;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use woderrise_core::models::{Catalog, CatalogRepository};
use woderrise_github::search::{aggregate_topics, filter_local_catalog};
use woderrise_github::{GitHubClientBuilder, RateLimitResource};
use woderrise_gui::resolve_catalog_4tier;

// ============================================================================
// 1. CLI Usage Boundaries & Standard Exit Codes (R3)
// ============================================================================

#[test]
fn test_tier2_auth_cli_invalid_arguments_and_error_codes() {
    let env = TestEnvironment::new("t2-auth-err");

    // 1. Missing token argument value -> non-zero exit status
    let out_missing = env.run_auth(&["--token"]);
    assert_ne!(out_missing.status.code(), Some(0));
    let stderr_missing = String::from_utf8_lossy(&out_missing.stderr);
    assert!(
        stderr_missing.contains("Missing argument") || stderr_missing.contains("Usage:"),
        "Expected error message for missing token argument, got: {}",
        stderr_missing
    );

    // 2. Empty token string -> non-zero exit status
    let out_empty = env.run_auth(&["--token", "   "]);
    assert_ne!(out_empty.status.code(), Some(0));
    let stderr_empty = String::from_utf8_lossy(&out_empty.stderr);
    assert!(
        stderr_empty.contains("empty") || stderr_empty.contains("Empty"),
        "Expected empty token error, got: {}",
        stderr_empty
    );

    // 3. Unrecognized flag -> non-zero exit status
    let out_unknown = env.run_auth(&["--bogus-unrecognized-flag"]);
    assert_ne!(out_unknown.status.code(), Some(0));

    // 4. Standard --help flag -> exit status 0
    let out_help = env.run_cli(&["--help"]);
    assert_eq!(out_help.status.code(), Some(0));
    let stdout_help = String::from_utf8_lossy(&out_help.stdout);
    assert!(stdout_help.contains("Woderrise"));
    assert!(stdout_help.contains("USAGE:"));

    // 5. Standard --version flag -> exit status 0
    let out_ver = env.run_cli(&["--version"]);
    assert_eq!(out_ver.status.code(), Some(0));
    let stdout_ver = String::from_utf8_lossy(&out_ver.stdout);
    assert!(stdout_ver.contains("woderrise"));
}

// ============================================================================
// 2. Adversarial Fuzzy Search Inputs & Boundary Limits (R4)
// ============================================================================

#[test]
fn test_tier2_fuzzy_search_adversarial_queries_and_boundaries() {
    let catalog = Catalog::default_popular();

    // 1. Empty and whitespace queries must return all packages ordered by stars
    let empty_results = filter_local_catalog(&catalog, "");
    assert_eq!(empty_results.len(), catalog.repositories.len());
    let ws_results = filter_local_catalog(&catalog, "   \t\n  ");
    assert_eq!(ws_results.len(), catalog.repositories.len());

    // 2. Adversarial queries must not panic or crash
    let adversarial_queries = [
        r#"""#,
        r#"""""#,
        r#"*"#,
        r#"***"#,
        r#"'"#,
        r#"''"#,
        r#"OR"#,
        r#"AND"#,
        r#"NOT"#,
        r#"AND OR NOT"#,
        r#"( )"#,
        r#"((()))"#,
        r#"^"#,
        r#":"#,
        r#"{ }"#,
        r#"%"#,
        r#"%%"#,
        r#";"#,
        r#"\*; DROP TABLE repositories; --"#,
        r#"SELECT * FROM catalog WHERE 1=1"#,
        r#"<script>alert(1)</script>"#,
        r#"🦀🦀🦀"#,
        r#"../"#,
        r#"../../../../etc/passwd"#,
        r#"///"#,
    ];

    for query in adversarial_queries {
        let results = filter_local_catalog(&catalog, query);
        let _ = results; // Assert safe termination without panic
    }

    // 3. Extremely long query (1024 characters) -> returns empty cleanly
    let long_query = "a".repeat(1024);
    let long_results = filter_local_catalog(&catalog, &long_query);
    assert!(long_results.is_empty());

    // 4. Completely irrelevant random gibberish -> returns empty cleanly
    let bogus_results = filter_local_catalog(&catalog, "zxqvwnmk_completely_absent_123");
    assert!(bogus_results.is_empty());
}

// ============================================================================
// 3. Background Tree Pre-Inspection Protection & Cancellation (R1)
// ============================================================================

#[tokio::test]
async fn test_tier2_pre_inspection_rate_limit_protection_and_cancellation() {
    let mock_server = wiremock::MockServer::start().await;
    let client = GitHubClientBuilder::new()
        .base_url(mock_server.uri())
        .build()
        .unwrap();

    // 1. Exhaust rate limit: set core remaining to 2 (< 5 threshold)
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
        .update_from_headers(&headers, RateLimitResource::Core);

    let detection_engine = std::sync::Arc::new(woderrise_detect::DetectionEngine::new());
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(10);
    let cancel_token = CancellationToken::new();

    let repo = CatalogRepository {
        id: 3001,
        owner: "test-owner".into(),
        repo: "test-repo".into(),
        full_name: "test-owner/test-repo".into(),
        html_url: "https://github.com/test-owner/test-repo".into(),
        description: Some("Test".into()),
        stargazers_count: 100,
        default_branch: "main".into(),
        topics: vec![],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    // Spawn tree pre-inspection
    woderrise_gui::spawn_tree_pre_inspection(
        vec![repo.clone()],
        client.clone(),
        detection_engine.clone(),
        event_tx.clone(),
        cancel_token.clone(),
    );

    // Protected: No network requests emitted when quota < 5
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        event_rx.try_recv().is_err(),
        "Quota < 5 must protect user rate limits from being consumed by background workers"
    );

    // 2. Cancellation token test
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(10);
    let cancel_token2 = CancellationToken::new();
    cancel_token2.cancel(); // Pre-cancel

    woderrise_gui::spawn_tree_pre_inspection(
        vec![repo],
        client,
        detection_engine,
        tx2,
        cancel_token2,
    );

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        rx2.try_recv().is_err(),
        "Pre-cancelled worker must not dispatch events"
    );
}

// ============================================================================
// 4. Multi-Topic Aggregation Boundary Cases (R2)
// ============================================================================

#[test]
fn test_tier2_multi_topic_empty_and_disjoint_topics() {
    // Empty topics
    let empty_results: Vec<Vec<CatalogRepository>> = vec![];
    let agg_empty = aggregate_topics(empty_results);
    assert!(agg_empty.is_empty());

    // Single empty query result
    let single_empty = vec![vec![]];
    let agg_single = aggregate_topics(single_empty);
    assert!(agg_single.is_empty());

    // Deterministic tie-breaking on identical stars
    let repo1 = CatalogRepository {
        id: 10,
        owner: "alpha".into(),
        repo: "theme".into(),
        full_name: "alpha/theme".into(),
        html_url: "https://github.com/alpha/theme".into(),
        description: None,
        stargazers_count: 500,
        default_branch: "main".into(),
        topics: vec!["hyprland".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };
    let repo2 = CatalogRepository {
        id: 20,
        owner: "beta".into(),
        repo: "theme".into(),
        full_name: "beta/theme".into(),
        html_url: "https://github.com/beta/theme".into(),
        description: None,
        stargazers_count: 500,
        default_branch: "main".into(),
        topics: vec!["waybar".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let agg_ties = aggregate_topics(vec![vec![repo2, repo1]]);
    assert_eq!(agg_ties.len(), 2);
    // Both items preserved without loss
    assert!(agg_ties.iter().any(|r| r.id == 10));
    assert!(agg_ties.iter().any(|r| r.id == 20));
}

// ============================================================================
// 5. Offline Fallback Boundary & Corrupted Cache Matrix (R5)
// ============================================================================

#[tokio::test]
async fn test_tier2_offline_feed_corrupted_cache_matrix() {
    let temp = TempDir::new().unwrap();
    let corrupted_cache = Utf8PathBuf::from_path_buf(temp.path().join("corrupted.json")).unwrap();

    // Write completely invalid / truncated JSON bytes to cache
    fs::write(&corrupted_cache, b"{ \"truncated\": true, ").unwrap();

    // Point CDN URL to unreachable port
    let offline_url = "http://127.0.0.1:9/unreachable.json";

    // Fallback must skip corrupted cache and successfully load bundled static catalog
    let catalog = resolve_catalog_4tier(Some(offline_url), Some(&corrupted_cache)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "Corrupted cache + offline CDN must fall back gracefully to bundled static catalog"
    );
    assert_eq!(catalog.version, 1);
}
