//! Adversarial Stress Test Suite for `woderrise-github`
//!
//! Evaluates boundary conditions, rate limit locks, Fastly CDN limits,
//! Moka RAM cache behavior, and ETag 304 TTL extensions.

use std::time::Duration;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use woderrise_core::error::{SecurityError, WoderriseError};
use woderrise_github::{GitHubClient, RateLimitResource, MAX_CONFIG_BYTES, MAX_WALLPAPER_BYTES};

#[tokio::test]
async fn test_cdn_config_file_exceeding_10mb_content_length() {
    let mock_server = MockServer::start().await;
    let over_limit = (MAX_CONFIG_BYTES + 1) as usize;

    Mock::given(method("GET"))
        .and(path("/owner/repo/commit1/hypr/hyprland.conf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'#'; over_limit]))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .cdn_base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let err = client
        .stream_blob("owner", "repo", "commit1", "hypr/hyprland.conf", None)
        .await
        .expect_err("Config file with content-length > 10MB must be rejected");

    match err {
        WoderriseError::Security(SecurityError::ResourceLimitExceeded {
            resource,
            limit_bytes,
            actual_bytes,
        }) => {
            assert_eq!(resource, "hypr/hyprland.conf");
            assert_eq!(limit_bytes, MAX_CONFIG_BYTES);
            assert_eq!(actual_bytes, over_limit as u64);
        }
        other => panic!("Expected ResourceLimitExceeded, got {:?}", other),
    }
}

#[tokio::test]
async fn test_cdn_config_file_exceeding_10mb_streaming_chunks() {
    let mock_server = MockServer::start().await;
    let over_limit = (MAX_CONFIG_BYTES + 512) as usize;

    // Content-length header omitted, streaming body directly
    Mock::given(method("GET"))
        .and(path("/owner/repo/commit1/waybar/config.jsonc"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'{'; over_limit]))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .cdn_base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let err = client
        .stream_blob("owner", "repo", "commit1", "waybar/config.jsonc", None)
        .await
        .expect_err("Config stream exceeding 10MB chunk accumulation must be rejected");

    match err {
        WoderriseError::Security(SecurityError::ResourceLimitExceeded {
            resource,
            limit_bytes,
            ..
        }) => {
            assert_eq!(resource, "waybar/config.jsonc");
            assert_eq!(limit_bytes, MAX_CONFIG_BYTES);
        }
        other => panic!("Expected ResourceLimitExceeded, got {:?}", other),
    }
}

#[tokio::test]
async fn test_cdn_wallpaper_within_50mb_allowed() {
    let mock_server = MockServer::start().await;
    // 15 MB image: exceeds config limit (10MB) but within wallpaper limit (50MB)
    let wallpaper_size = 15 * 1024 * 1024;
    let payload = vec![0x89; wallpaper_size];

    Mock::given(method("GET"))
        .and(path("/owner/repo/commit1/wallpapers/neon_cyber.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("content-length", wallpaper_size.to_string())
                .set_body_bytes(payload.clone()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .cdn_base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let (bytes, sha256) = client
        .stream_blob(
            "owner",
            "repo",
            "commit1",
            "wallpapers/neon_cyber.png",
            None,
        )
        .await
        .expect("Wallpaper <= 50MB must be permitted");

    assert_eq!(bytes.len(), wallpaper_size);
    assert!(!sha256.is_empty());
}

#[tokio::test]
async fn test_cdn_wallpaper_exceeding_50mb_rejected() {
    let mock_server = MockServer::start().await;
    let over_limit = (MAX_WALLPAPER_BYTES + 1024) as usize;

    Mock::given(method("GET"))
        .and(path("/owner/repo/commit1/wallpapers/ultra_raw.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0xFF; over_limit]))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .cdn_base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let err = client
        .stream_blob("owner", "repo", "commit1", "wallpapers/ultra_raw.jpg", None)
        .await
        .expect_err("Wallpaper exceeding 50MB must be rejected");

    match err {
        WoderriseError::Security(SecurityError::ResourceLimitExceeded {
            resource,
            limit_bytes,
            actual_bytes,
        }) => {
            assert_eq!(resource, "wallpapers/ultra_raw.jpg");
            assert_eq!(limit_bytes, MAX_WALLPAPER_BYTES);
            assert_eq!(actual_bytes, over_limit as u64);
        }
        other => panic!("Expected ResourceLimitExceeded, got {:?}", other),
    }
}

#[tokio::test]
async fn test_rate_limit_lockout_429_core_parsed() {
    let mock_server = MockServer::start().await;
    let future_reset = (chrono::Utc::now() + chrono::Duration::minutes(45)).timestamp();

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(429)
                .append_header("x-ratelimit-resource", "core")
                .append_header("x-ratelimit-limit", "60")
                .append_header("x-ratelimit-remaining", "0")
                .append_header("x-ratelimit-reset", future_reset.to_string()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let err = client
        .get_tree("user", "repo", "main")
        .await
        .expect_err("HTTP 429 with 0 remaining must return RateLimitExceeded");

    match err {
        WoderriseError::RateLimitExceeded { reset_time } => {
            assert_eq!(reset_time.timestamp(), future_reset);
        }
        other => panic!("Expected RateLimitExceeded, got {:?}", other),
    }
}

#[tokio::test]
async fn test_rate_limit_lockout_429_search_parsed() {
    let mock_server = MockServer::start().await;
    let future_reset = (chrono::Utc::now() + chrono::Duration::minutes(2)).timestamp();

    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(
            ResponseTemplate::new(429)
                .append_header("x-ratelimit-resource", "search")
                .append_header("x-ratelimit-limit", "10")
                .append_header("x-ratelimit-remaining", "0")
                .append_header("x-ratelimit-reset", future_reset.to_string()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let err = client
        .search_repositories("catppuccin")
        .await
        .expect_err("Search HTTP 429 with 0 remaining must return RateLimitExceeded");

    match err {
        WoderriseError::RateLimitExceeded { reset_time } => {
            assert_eq!(reset_time.timestamp(), future_reset);
        }
        other => panic!("Expected RateLimitExceeded, got {:?}", other),
    }
}

#[tokio::test]
async fn test_search_proactive_throttling_remaining_le_2() {
    let client = GitHubClient::builder().build().unwrap();

    // Fabricate headers setting search remaining to 2
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-ratelimit-resource", "search".parse().unwrap());
    headers.insert("x-ratelimit-remaining", "2".parse().unwrap());
    let future_reset = (chrono::Utc::now() + chrono::Duration::seconds(45)).timestamp();
    headers.insert(
        "x-ratelimit-reset",
        future_reset.to_string().parse().unwrap(),
    );

    client
        .rate_limiter()
        .update_from_headers(&headers, RateLimitResource::Search);

    let err = client
        .rate_limiter()
        .check_search_quota()
        .expect_err("Remaining <= 2 must proactively throttle");

    match err {
        WoderriseError::RateLimitExceeded { reset_time } => {
            assert_eq!(reset_time.timestamp(), future_reset);
        }
        other => panic!("Expected RateLimitExceeded, got {:?}", other),
    }
}

#[tokio::test]
async fn test_cache_zero_network_requests_on_repeated_queries_within_10m() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "sha": "sha_10m",
                    "tree": [
                        {
                            "path": "hypr/hyprland.conf",
                            "mode": "100644",
                            "type": "blob",
                            "sha": "b1",
                            "size": 100
                        }
                    ],
                    "truncated": false
                }))
                .append_header("x-ratelimit-remaining", "50"),
        )
        .expect(1) // STRICT: Exactly 1 network request allowed!
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .cache_ttl(Duration::from_secs(600)) // 10 minutes
        .build()
        .expect("Client build failed");

    // Perform 10 consecutive requests within the 10m TTL window
    for _ in 0..10 {
        let tree = client.get_tree("user", "repo", "main").await.unwrap();
        assert_eq!(tree.sha, "sha_10m");
    }

    // Verify key is marked active in cache
    let key = woderrise_github::GitHubCache::tree_key("user", "repo", "main");
    assert!(client.cache().is_ttl_active(&key).await);
}

#[tokio::test]
async fn test_etag_304_cache_ttl_extension() {
    let mock_server = MockServer::start().await;

    // Step 1: Initial 200 response with ETag "etag-v1"
    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "sha": "initial_sha",
                    "tree": [
                        {
                            "path": "hypr/hyprland.conf",
                            "mode": "100644",
                            "type": "blob",
                            "sha": "b1"
                        }
                    ],
                    "truncated": false
                }))
                .append_header("etag", "\"etag-v1\"")
                .append_header("x-ratelimit-remaining", "55"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .cache_ttl(Duration::from_millis(50)) // Short TTL
        .build()
        .expect("Client build failed");

    let t1 = client.get_tree("user", "repo", "main").await.unwrap();
    assert_eq!(t1.sha, "initial_sha");

    // Wait 60ms for initial RAM TTL to expire
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Step 2: Reset mock server and expect conditional 304 request
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .and(header("If-None-Match", "\"etag-v1\""))
        .respond_with(
            ResponseTemplate::new(304)
                .append_header("etag", "\"etag-v1\"")
                .append_header("x-ratelimit-remaining", "54"),
        )
        .expect(1) // Exactly 1 request for the 304 revalidation
        .mount(&mock_server)
        .await;

    let t2 = client.get_tree("user", "repo", "main").await.unwrap();
    assert_eq!(t2.sha, "initial_sha");

    // Step 3: EMPIRICAL CHALLENGE: Verify TTL extension!
    // Immediately perform a 3rd request without sleeping.
    // The 304 response should have refreshed the TTL, so this 3rd request MUST HIT RAM
    // and emit ZERO additional requests to the mock server!
    let t3 = client.get_tree("user", "repo", "main").await.unwrap();
    assert_eq!(t3.sha, "initial_sha");

    // Check that TTL is currently active
    let key = woderrise_github::GitHubCache::tree_key("user", "repo", "main");
    assert!(client.cache().is_ttl_active(&key).await);
}

// =========================================================================
// M6 EMPIRICAL CHALLENGER ADVERSARIAL TESTS (R4 Fuzzy Search & R5 Curated Feed)
// =========================================================================

use woderrise_core::models::{
    Catalog, CatalogRepository, ConfigComponent, DetectedComponent, SyntaxFormat,
};
use woderrise_github::search::{filter_local_catalog, score_repository};

fn make_adversarial_repo(
    id: u64,
    owner: &str,
    repo: &str,
    desc: Option<&str>,
    stars: u64,
    topics: Vec<&str>,
    components: Vec<ConfigComponent>,
) -> CatalogRepository {
    CatalogRepository {
        id,
        owner: owner.to_string(),
        repo: repo.to_string(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: desc.map(String::from),
        stargazers_count: stars,
        default_branch: "main".to_string(),
        topics: topics.into_iter().map(String::from).collect(),
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: components
            .into_iter()
            .map(|c| DetectedComponent {
                component: c,
                syntax_format: SyntaxFormat::LegacyConf,
                root_path: "config".to_string(),
                entrypoint: "config.conf".to_string(),
                style_file: None,
                file_count: 5,
                total_size_bytes: 10000,
            })
            .collect(),
    }
}

#[test]
fn test_adversarial_fuzzy_typo_matrix_swapped_missing_extra_letters() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    // Matrix of adversarial typos for Hyprland:
    // 1. Swapped adjacent letters: 'hyprladn'
    // 2. Missing letters: 'hyprlnd', 'hprland'
    // 3. Extra duplicated letters: 'hyyprland', 'hyprrland'
    let hypr_typos = ["hyprladn", "hyprlnd", "hprland", "hyyprland", "hyprrland"];

    for typo in hypr_typos {
        let results = filter_local_catalog(&catalog, typo);
        assert!(
            !results.is_empty(),
            "Adversarial typo '{}' must match Hyprland packages",
            typo
        );

        let top = &results[0];
        let matches = top.resolved_full_name().to_lowercase().contains("hypr")
            || top.topics.iter().any(|t| t.contains("hypr"))
            || top
                .detected_components
                .iter()
                .any(|c| c.component.as_str().contains("hypr"));
        assert!(
            matches,
            "Top result '{}' for typo '{}' must relate to Hyprland",
            top.resolved_full_name(),
            typo
        );
    }

    // Matrix of adversarial typos for Waybar:
    // 1. Swapped letters: 'waybra'
    // 2. Missing letters: 'waybr', 'wybar'
    // 3. Extra letters: 'wayybar'
    let waybar_typos = ["waybra", "waybr", "wybar", "wayybar"];

    for typo in waybar_typos {
        let results = filter_local_catalog(&catalog, typo);
        assert!(
            !results.is_empty(),
            "Adversarial typo '{}' must match Waybar packages",
            typo
        );

        let top = &results[0];
        let matches = top.resolved_full_name().to_lowercase().contains("waybar")
            || top.topics.iter().any(|t| t.contains("waybar"))
            || top
                .detected_components
                .iter()
                .any(|c| c.component == ConfigComponent::Waybar);
        assert!(
            matches,
            "Top result '{}' for typo '{}' must feature Waybar",
            top.resolved_full_name(),
            typo
        );
    }
}

#[test]
fn test_adversarial_abbreviations_and_subsequence_acronyms() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    // Community abbreviations
    let ml4_res = filter_local_catalog(&catalog, "ml4");
    assert!(!ml4_res.is_empty(), "Abbreviation 'ml4' must resolve");
    assert_eq!(ml4_res[0].owner, "mylinuxforwork");

    let ml4w_res = filter_local_catalog(&catalog, "ml4w");
    assert!(!ml4w_res.is_empty(), "Abbreviation 'ml4w' must resolve");
    assert_eq!(ml4w_res[0].owner, "mylinuxforwork");

    let hl_res = filter_local_catalog(&catalog, "hl");
    assert!(!hl_res.is_empty(), "Abbreviation 'hl' must resolve");
    assert!(hl_res
        .iter()
        .any(|r| r.resolved_full_name().contains("hyprland")));

    let wb_res = filter_local_catalog(&catalog, "wb");
    assert!(!wb_res.is_empty(), "Abbreviation 'wb' must resolve");
    assert!(wb_res.iter().any(|r| r
        .detected_components
        .iter()
        .any(|c| c.component == ConfigComponent::Waybar)));

    let dots_res = filter_local_catalog(&catalog, "dots");
    assert!(!dots_res.is_empty(), "Abbreviation 'dots' must resolve");
    assert!(dots_res
        .iter()
        .any(|r| r.resolved_full_name().contains("dots")
            || r.resolved_full_name().contains("dotfiles")));

    // Subsequence acronyms via sublime_fuzzy
    // 'jkl' for JaKooLit
    let jkl_res = filter_local_catalog(&catalog, "jkl");
    assert!(
        !jkl_res.is_empty(),
        "Subsequence acronym 'jkl' must resolve to JaKooLit"
    );
    assert_eq!(jkl_res[0].owner, "JaKooLit");

    // 'sdt' for SolDoesTech
    let sdt_res = filter_local_catalog(&catalog, "sdt");
    assert!(
        !sdt_res.is_empty(),
        "Subsequence acronym 'sdt' must resolve to SolDoesTech"
    );
    assert_eq!(sdt_res[0].owner, "SolDoesTech");
}

#[test]
fn test_adversarial_multi_token_typo_combinations() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    // Multi-token query where both tokens have typos: "hyprlnd waybr"
    let dual_typo_res = filter_local_catalog(&catalog, "hyprlnd waybr");
    assert!(
        !dual_typo_res.is_empty(),
        "Dual typo query 'hyprlnd waybr' must match dual-component packages"
    );

    // Repositories returned MUST satisfy BOTH tokens
    for repo in &dual_typo_res {
        let has_hypr = repo.resolved_full_name().to_lowercase().contains("hypr")
            || repo.topics.iter().any(|t| t.contains("hypr"))
            || repo
                .detected_components
                .iter()
                .any(|c| c.component.as_str().contains("hypr"));
        let has_waybar = repo.resolved_full_name().to_lowercase().contains("waybar")
            || repo.topics.iter().any(|t| t.contains("waybar"))
            || repo
                .detected_components
                .iter()
                .any(|c| c.component == ConfigComponent::Waybar)
            || repo
                .description
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains("waybar");

        assert!(
            has_hypr,
            "Repo {} must match hyprland",
            repo.resolved_full_name()
        );
        assert!(
            has_waybar,
            "Repo {} must match waybar",
            repo.resolved_full_name()
        );
    }

    // catppuccin/hyprland only has hyprland (no waybar) -> MUST NOT match "hyprlnd waybr"
    assert!(
        !dual_typo_res
            .iter()
            .any(|r| r.resolved_full_name() == "catppuccin/hyprland"),
        "catppuccin/hyprland has no waybar and must be excluded by AND semantics"
    );

    // Multi-token query with non-existent token: "hyprland nonexistentspamterm999"
    let impossible_res = filter_local_catalog(&catalog, "hyprland nonexistentspamterm999");
    assert!(
        impossible_res.is_empty(),
        "Multi-token query with impossible token must return empty list"
    );
}

#[test]
fn test_adversarial_ranking_invariants_exact_vs_typo_vs_unrelated() {
    let repo_exact = make_adversarial_repo(
        1,
        "author",
        "hyprland",
        Some("Exact hyprland match with 0 stars"),
        0, // 0 stars!
        vec!["hyprland"],
        vec![ConfigComponent::Hyprland],
    );

    let repo_typo = make_adversarial_repo(
        2,
        "popular",
        "hyprladn-dots",
        Some("Typo hyprladn with 1,000,000 stars"),
        1_000_000, // 1M stars!
        vec!["desktop"],
        vec![],
    );

    let repo_unrelated = make_adversarial_repo(
        3,
        "other",
        "alacritty-terminal",
        Some("Irrelevant terminal emulator"),
        500_000,
        vec!["terminal"],
        vec![],
    );

    let catalog = Catalog {
        repositories: vec![
            repo_unrelated.clone(),
            repo_typo.clone(),
            repo_exact.clone(),
        ],
        ..Default::default()
    };

    // Query: "hyprland"
    let results = filter_local_catalog(&catalog, "hyprland");
    assert!(!results.is_empty());

    // Invariant 1: Unrelated package MUST NOT appear in results
    assert!(
        !results.iter().any(|r| r.id == 3),
        "Unrelated repo (id=3) must be filtered out"
    );

    // Invariant 2: Exact match on name/repo (id=1, 0 stars) MUST strictly outrank
    // typo match (id=2, 1M stars)
    assert_eq!(
        results[0].id, 1,
        "Exact match repo (id=1, 0 stars) must rank higher than typo match (id=2, 1M stars)"
    );

    let score_exact = score_repository(&repo_exact, "hyprland").expect("Exact must score");
    let score_typo = score_repository(&repo_typo, "hyprland").expect("Typo must score");
    assert!(
        score_exact > score_typo,
        "Exact match score ({}) must strictly exceed typo score ({})",
        score_exact,
        score_typo
    );

    // Query: "hyprladn" (reversed query)
    // Now repo 2 is the exact match for "hyprladn" and repo 1 is the typo match!
    let rev_results = filter_local_catalog(&catalog, "hyprladn");
    assert_eq!(
        rev_results[0].id, 2,
        "When query is 'hyprladn', repo 2 with exact name substring must rank #1"
    );
}

#[test]
fn test_adversarial_high_token_count_query() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    let multi_token = "hyprland ".repeat(1_000);
    let start = std::time::Instant::now();
    let res_multi = filter_local_catalog(&catalog, &multi_token);
    let elapsed_multi = start.elapsed();
    assert!(
        elapsed_multi < Duration::from_millis(500),
        "1,000 tokens query must finish within 500ms, took {:?}",
        elapsed_multi
    );
    assert!(!res_multi.is_empty());
}

#[test]
fn test_adversarial_unicode_and_emojis() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    let unicode_queries = [
        "🦀",
        "🚀🔥✨",
        "🇨🇦",
        "こんにちは",
        "Привет",
        "مرحبا",
        "\u{FFFF}\u{10FFFF}",
    ];
    for uq in unicode_queries {
        let res_u = filter_local_catalog(&catalog, uq);
        let _ = res_u.len();
    }
}

#[test]
fn test_adversarial_special_and_injection_payloads() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    let special_queries = [
        "",
        "   \t\r\n   ",
        "\0\0\0",
        "\x01\x02\x03\x1b\x7f",
        "'; DROP TABLE repositories; --",
        "<script>alert('xss')</script>",
        "../../../../../../etc/passwd",
        "\\\\\\\\",
        "~!@#$%^&*()_+=-`{}|[]\\:\";'<>?,./",
    ];
    for sq in special_queries {
        let res_s = filter_local_catalog(&catalog, sq);
        if sq.trim().is_empty() {
            assert_eq!(
                res_s.len(),
                catalog.repositories.len(),
                "Empty/whitespace query must return all repositories"
            );
        }
    }
}

#[test]
fn test_adversarial_10k_char_query_dos_and_performance() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    let huge_query = "a".repeat(10_000);
    let start = std::time::Instant::now();
    let res = filter_local_catalog(&catalog, &huge_query);
    let elapsed = start.elapsed();
    assert!(res.is_empty(), "10k 'a' string must not match anything");
    assert!(
        elapsed < Duration::from_millis(100),
        "10k query must finish within 100ms for responsiveness on GUI thread, took {:?}",
        elapsed
    );
}

#[test]
fn test_adversarial_empty_catalog_and_stargazer_tie_breakers() {
    // 1. Empty catalog must not panic
    let empty_catalog = Catalog {
        repositories: vec![],
        ..Default::default()
    };
    let empty_res = filter_local_catalog(&empty_catalog, "hyprland");
    assert!(empty_res.is_empty());
    let empty_res_blank = filter_local_catalog(&empty_catalog, "");
    assert!(empty_res_blank.is_empty());

    // 2. Stargazer tie breaker on identical scores
    let r1 = make_adversarial_repo(10, "a", "match", None, 100, vec!["match"], vec![]);
    let r2 = make_adversarial_repo(20, "b", "match", None, 500, vec!["match"], vec![]);
    let r3 = make_adversarial_repo(5, "c", "match", None, 500, vec!["match"], vec![]);

    let test_catalog = Catalog {
        repositories: vec![r1.clone(), r2.clone(), r3.clone()],
        ..Default::default()
    };

    let ranked = filter_local_catalog(&test_catalog, "match");
    assert_eq!(ranked.len(), 3);
    // r3 and r2 have 500 stars, r1 has 100 stars.
    // Between r3 and r2: id 5 < id 20 -> r3 ranks before r2.
    assert_eq!(ranked[0].id, 5, "Rank 0 must be r3 (500 stars, id 5)");
    assert_eq!(ranked[1].id, 20, "Rank 1 must be r2 (500 stars, id 20)");
    assert_eq!(ranked[2].id, 10, "Rank 2 must be r1 (100 stars, id 10)");
}

#[test]
fn test_adversarial_50k_char_query_stress() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    // Case 1: 50,000 characters contiguous without whitespace (single gigantic token)
    let huge_single_token = "z".repeat(50_000);
    let start1 = std::time::Instant::now();
    let res1 = filter_local_catalog(&catalog, &huge_single_token);
    let elapsed1 = start1.elapsed();
    assert!(res1.is_empty(), "50k 'z' query must not match anything");
    assert!(
        elapsed1 < Duration::from_millis(100),
        "50k single-token query must complete in <100ms, took {:?}",
        elapsed1
    );

    // Case 2: 50,000 characters with 2-char tokens (25,000 tokens)
    let huge_token_stream = "ab ".repeat(16_666);
    let start2 = std::time::Instant::now();
    let res2 = filter_local_catalog(&catalog, &huge_token_stream);
    let elapsed2 = start2.elapsed();
    assert!(
        elapsed2 < Duration::from_millis(100),
        "50k char stream query must complete in <100ms, took {:?}",
        elapsed2
    );
    let _ = res2.len();
}

#[test]
fn test_adversarial_5k_tokens_query_stress() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    // 5,000 distinct whitespace-separated tokens
    let mut tokens = Vec::with_capacity(5000);
    for i in 0..5000 {
        tokens.push(format!("tok{:04x}", i));
    }
    let query_5k = tokens.join(" ");
    let start = std::time::Instant::now();
    let res = filter_local_catalog(&catalog, &query_5k);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(200),
        "5,000 distinct tokens query must complete in <200ms, took {:?}",
        elapsed
    );
    assert!(res.is_empty());
}

#[test]
fn test_adversarial_pathological_regex_and_backtracking_patterns() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    let pathological_patterns = [
        // Classic ReDoS backtracking bombs
        "((((((((a*)*)*)*)*)*)*)*",
        "(a+)+$",
        "([a-zA-Z]+)*",
        "^(a|a?)+$",
        "(a|aa)+$",
        "a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?a?aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        // Pathological lookaheads / quantifiers
        "(?=.*[a-z])(?=.*[A-Z])(?=.*\\d)(?=.*[@$!%*?&])[A-Za-z\\d@$!%*?&]{8,100000}",
        "*+?{9999999}",
        "\\p{L}+\\p{M}*",
        "\\Q\\E\\Q\\E\\Q\\E",
        // Unbalanced regex syntax
        "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[",
        "((((((((((((((((((((((((((((((((((((((((((",
        "??????????????????????????????????????????",
        "******************************************",
        "++++++++++++++++++++++++++++++++++++++++++",
        // Format string & memory corruption probes
        "%s%s%s%s%n%n%x%x%d%d",
        "\\x00\\xff\\xfe\\x7f",
    ];

    for pattern in &pathological_patterns {
        let start = std::time::Instant::now();
        let res = filter_local_catalog(&catalog, pattern);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(50),
            "Pattern '{}' took too long: {:?}",
            pattern,
            elapsed
        );
        let _ = res.len();
    }
}

#[test]
fn test_adversarial_deeply_nested_substrings_and_pathological_descriptions() {
    // Measure scaling across varying description lengths: 300, 1000, 5000, 20000 chars
    let lengths = [300, 1000, 3000, 10000, 20000];
    let query_32 = "hyprlandhyprlandhyprlandhyprland";

    for &len in &lengths {
        let pattern = "hyprland config ";
        let repeat_count = (len / pattern.len()) + 1;
        let mut desc = pattern.repeat(repeat_count);
        desc.truncate(len);

        let pathological_repo = make_adversarial_repo(
            9999,
            "deeply-nested-owner",
            "deeply-nested-repo",
            Some(&desc),
            1000,
            vec!["hyprland", "waybar"],
            vec![ConfigComponent::Hyprland, ConfigComponent::Waybar],
        );

        let catalog = Catalog {
            repositories: vec![pathological_repo],
            ..Default::default()
        };

        let start = std::time::Instant::now();
        let res = filter_local_catalog(&catalog, query_32);
        let elapsed = start.elapsed();
        println!("Description length: {} chars, elapsed: {:?}", len, elapsed);
        let _ = res.len();

        // Each individual repository evaluation must finish within 100ms
        assert!(
            elapsed < Duration::from_millis(100),
            "Query on {}-char repo description took too long: {:?}",
            len,
            elapsed
        );
    }
}

#[test]
fn test_adversarial_length_boundary_guards() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog =
        serde_json::from_str(BUNDLED_JSON).expect("Curated catalog must deserialize");

    // Token length boundary test:
    // length 2: skipped for typo-tolerance (min 3) and subsequence (min 3)
    let res_2 = filter_local_catalog(&catalog, "xy");
    assert!(res_2.is_empty());

    // length 3: allowed for typo-tolerance (3..=64) and subsequence (3..=32)
    let res_3 = filter_local_catalog(&catalog, "hyp");
    assert!(
        !res_3.is_empty(),
        "3-char token 'hyp' must match via subsequence"
    );

    // length 32: max length for subsequence
    let token_32 = "h".repeat(32);
    let res_32 = filter_local_catalog(&catalog, &token_32);
    let _ = res_32.len();

    // length 33: exceeds subsequence limit (32), but within typo limit (64)
    let token_33 = "h".repeat(33);
    let res_33 = filter_local_catalog(&catalog, &token_33);
    let _ = res_33.len();

    // length 64: max length for typo distance
    let token_64 = "h".repeat(64);
    let res_64 = filter_local_catalog(&catalog, &token_64);
    assert!(res_64.is_empty());

    // length 65: exceeds typo distance limit (64)
    let token_65 = "h".repeat(65);
    let res_65 = filter_local_catalog(&catalog, &token_65);
    assert!(res_65.is_empty());

    // Token count boundary test: 16 tokens vs 17 tokens
    // First 16 tokens match "hyprland", 17th is non-matching "zzzzzzzz"
    // Since take(16) bounds token iteration, tokens after 16 are ignored
    let mut query_17 = "hyprland ".repeat(16);
    query_17.push_str("zzzzzzzzzzzzzzzz");
    let res_17 = filter_local_catalog(&catalog, &query_17);
    assert!(
        !res_17.is_empty(),
        "Query with 16 matching tokens + 1 trailing token beyond take(16) should match based on first 16 tokens"
    );
}
