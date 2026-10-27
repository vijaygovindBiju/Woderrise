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
