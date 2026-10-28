use base64::Engine;
use sha2::Digest;
use std::time::Duration;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
use woderrise_core::error::{SecurityError, WoderriseError};
use woderrise_core::models::{
    Catalog, CatalogRepository, ConfigComponent, DetectedComponent, SyntaxFormat,
};
use woderrise_github::{
    GitHubClient, RateLimitResource, MAX_CONFIG_BYTES, MODE_DIRECTORY, MODE_EXECUTABLE,
    MODE_REGULAR_FILE, MODE_SYMLINK,
};

#[tokio::test]
async fn test_client_default_headers() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .and(header(
            "User-Agent",
            format!("Woderrise/{}", env!("CARGO_PKG_VERSION")).as_str(),
        ))
        .and(header("Accept", "application/vnd.github+json"))
        .and(header("X-GitHub-Api-Version", "2022-11-28"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "sha": "main_sha",
                    "tree": [],
                    "truncated": false
                }))
                .append_header("x-ratelimit-resource", "core")
                .append_header("x-ratelimit-limit", "60")
                .append_header("x-ratelimit-remaining", "59")
                .append_header("x-ratelimit-reset", "1761500000"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .cdn_base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let tree = client
        .get_tree("user", "repo", "main")
        .await
        .expect("Failed to get tree");
    assert_eq!(tree.sha, "main_sha");
}

#[tokio::test]
async fn test_client_auth_bearer_token() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .and(header("Authorization", "Bearer ghp_secret_test_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": "auth_sha",
            "tree": [],
            "truncated": false
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .auth_token(Some("ghp_secret_test_token".to_string()))
        .build()
        .expect("Client build failed");

    let tree = client
        .get_tree("user", "repo", "main")
        .await
        .expect("Failed to get tree");
    assert_eq!(tree.sha, "auth_sha");
}

#[tokio::test]
async fn test_rate_limit_headers_core() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "sha": "tree1",
                    "tree": [],
                    "truncated": false
                }))
                .append_header("x-ratelimit-resource", "core")
                .append_header("x-ratelimit-limit", "60")
                .append_header("x-ratelimit-remaining", "42")
                .append_header("x-ratelimit-reset", "1761500000")
                .append_header("x-ratelimit-used", "18"),
        )
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let _ = client.get_tree("user", "repo", "main").await.unwrap();

    let status = client.rate_limiter().status(RateLimitResource::Core);
    assert_eq!(status.limit, 60);
    assert_eq!(status.remaining, 42);
    assert_eq!(status.used, 18);
    assert_eq!(status.reset_time.timestamp(), 1761500000);
}

#[tokio::test]
async fn test_rate_limit_headers_search() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "total_count": 1,
                    "incomplete_results": false,
                    "items": []
                }))
                .append_header("x-ratelimit-resource", "search")
                .append_header("x-ratelimit-limit", "10")
                .append_header("x-ratelimit-remaining", "8")
                .append_header("x-ratelimit-reset", "1761500060")
                .append_header("x-ratelimit-used", "2"),
        )
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let _ = client.search_repositories("hyprland").await.unwrap();

    let status = client.rate_limiter().status(RateLimitResource::Search);
    assert_eq!(status.limit, 10);
    assert_eq!(status.remaining, 8);
    assert_eq!(status.used, 2);
    assert_eq!(status.reset_time.timestamp(), 1761500060);
}

#[tokio::test]
async fn test_rate_limit_exhaustion_403() {
    let mock_server = MockServer::start().await;

    // Reset timestamp far in the future
    let future_reset = (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp();

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(403)
                .append_header("x-ratelimit-resource", "core")
                .append_header("x-ratelimit-limit", "60")
                .append_header("x-ratelimit-remaining", "0")
                .append_header("x-ratelimit-reset", future_reset.to_string()),
        )
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let err = client.get_tree("user", "repo", "main").await.unwrap_err();
    match err {
        WoderriseError::RateLimitExceeded { reset_time } => {
            assert_eq!(reset_time.timestamp(), future_reset);
        }
        other => panic!("Expected RateLimitExceeded, got {:?}", other),
    }
}

#[tokio::test]
async fn test_etag_304_cache_hit() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "sha": "tree_etag_sha",
                    "tree": [
                        {
                            "path": "hyprland.conf",
                            "mode": "100644",
                            "type": "blob",
                            "sha": "blob123",
                            "size": 512
                        }
                    ],
                    "truncated": false
                }))
                .append_header("etag", "\"tag123\"")
                .append_header("x-ratelimit-remaining", "50"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .cache_ttl(Duration::from_millis(50)) // short TTL for test
        .build()
        .expect("Client build failed");

    let tree1 = client.get_tree("user", "repo", "main").await.unwrap();
    assert_eq!(tree1.sha, "tree_etag_sha");

    // Wait for RAM TTL to expire
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Reset mock to return 304 Not Modified
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(304)
                .append_header("x-ratelimit-remaining", "49")
                .append_header("etag", "\"tag123\""),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let tree2 = client.get_tree("user", "repo", "main").await.unwrap();
    assert_eq!(tree2.sha, "tree_etag_sha");
    assert_eq!(tree2.tree[0].path, "hyprland.conf");
}

#[tokio::test]
async fn test_moka_ttl_suppresses_network() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": "cached_sha",
            "tree": [],
            "truncated": false
        })))
        .expect(1) // STRICT EXPECTATION: exactly 1 network request
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .cache_ttl(Duration::from_secs(600)) // 10 minutes
        .build()
        .expect("Client build failed");

    // 5 calls within TTL window
    for _ in 0..5 {
        let tree = client.get_tree("user", "repo", "main").await.unwrap();
        assert_eq!(tree.sha, "cached_sha");
    }
}

#[tokio::test]
async fn test_git_trees_recursive_parsing() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "tree_full",
                "tree": [
                    { "path": "hypr/hyprland.conf", "mode": MODE_REGULAR_FILE, "type": "blob", "sha": "b1", "size": 100 },
                    { "path": "scripts/launch.sh", "mode": MODE_EXECUTABLE, "type": "blob", "sha": "b2", "size": 200 },
                    { "path": "sublink", "mode": MODE_SYMLINK, "type": "blob", "sha": "b3", "size": 10 },
                    { "path": "hypr", "mode": MODE_DIRECTORY, "type": "tree", "sha": "t1" }
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

    let tree = client.get_tree("user", "repo", "main").await.unwrap();
    assert_eq!(tree.tree.len(), 4);

    let f = tree.find_entry("hypr/hyprland.conf").unwrap();
    assert!(f.is_file());
    assert!(!f.is_executable());
    assert!(!f.is_symlink());

    let s = tree.find_entry("scripts/launch.sh").unwrap();
    assert!(s.is_file());
    assert!(s.is_executable());

    let link = tree.find_entry("sublink").unwrap();
    assert!(link.is_symlink());

    let dir = tree.find_entry("hypr").unwrap();
    assert!(dir.is_directory());
}

#[tokio::test]
async fn test_git_trees_truncated_handling() {
    let mock_server = MockServer::start().await;

    // Main tree returns truncated: true with an unexpanded hypr directory
    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/main"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "truncated_tree_sha",
                "tree": [
                    { "path": "README.md", "mode": "100644", "type": "blob", "sha": "readme1", "size": 50 },
                    { "path": "hypr", "mode": "040000", "type": "tree", "sha": "hypr_subtree_sha" }
                ],
                "truncated": true
            })),
        )
        .mount(&mock_server)
        .await;

    // Subtree request for hypr_subtree_sha
    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/trees/hypr_subtree_sha"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "hypr_subtree_sha",
                "tree": [
                    { "path": "hyprland.conf", "mode": "100644", "type": "blob", "sha": "hl_sha", "size": 1024 }
                ],
                "truncated": false
            })),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let tree = client.get_tree("user", "repo", "main").await.unwrap();
    // Verify that hypr/hyprland.conf was merged into the tree!
    let merged = tree.find_entry("hypr/hyprland.conf");
    assert!(
        merged.is_some(),
        "Expected hypr/hyprland.conf to be resolved from subtree"
    );
    assert_eq!(merged.unwrap().sha, "hl_sha");
}

#[tokio::test]
async fn test_fastly_cdn_streaming_success() {
    let mock_server = MockServer::start().await;

    let test_dotfile = "# Hyprland configuration\nautogenerated = 0\n";
    let expected_sha256 = format!("{:x}", sha2::Sha256::digest(test_dotfile.as_bytes()));

    Mock::given(method("GET"))
        .and(path("/user/repo/c0ffee1234/hypr/hyprland.conf"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(test_dotfile.as_bytes().to_vec())
                .append_header("content-length", test_dotfile.len().to_string()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .cdn_base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let (bytes, sha256) = client
        .stream_blob("user", "repo", "c0ffee1234", "hypr/hyprland.conf", None)
        .await
        .expect("Stream failed");

    assert_eq!(bytes, test_dotfile.as_bytes());
    assert_eq!(sha256, expected_sha256);
}

#[tokio::test]
async fn test_fastly_cdn_size_limit_rejection() {
    let mock_server = MockServer::start().await;

    let huge_size = (MAX_CONFIG_BYTES + 1024) as usize; // Exceeds 10MB limit

    Mock::given(method("GET"))
        .and(path("/user/repo/c0ffee/massive.conf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'A'; huge_size]))
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .cdn_base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let err = client
        .stream_blob("user", "repo", "c0ffee", "massive.conf", None)
        .await
        .unwrap_err();

    match err {
        WoderriseError::Security(SecurityError::ResourceLimitExceeded {
            resource,
            limit_bytes,
            ..
        }) => {
            assert_eq!(resource, "massive.conf");
            assert_eq!(limit_bytes, MAX_CONFIG_BYTES);
        }
        other => panic!("Expected ResourceLimitExceeded, got {:?}", other),
    }
}

#[tokio::test]
async fn test_fastly_cdn_fallback_to_rest() {
    let mock_server = MockServer::start().await;

    // CDN returns 404
    Mock::given(method("GET"))
        .and(path("/user/repo/c0ffee/private.conf"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    // REST Blobs API returns base64 content
    let raw_content = "fallback_secret_content";
    let encoded_base64 = base64::engine::general_purpose::STANDARD.encode(raw_content.as_bytes());
    let expected_sha256 = format!("{:x}", sha2::Sha256::digest(raw_content.as_bytes()));

    Mock::given(method("GET"))
        .and(path("/repos/user/repo/git/blobs/blob999"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": "blob999",
            "size": raw_content.len(),
            "content": encoded_base64,
            "encoding": "base64"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .cdn_base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let (bytes, sha256) = client
        .stream_blob("user", "repo", "c0ffee", "private.conf", Some("blob999"))
        .await
        .expect("Fallback to REST should succeed");

    assert_eq!(bytes, raw_content.as_bytes());
    assert_eq!(sha256, expected_sha256);
}

#[tokio::test]
async fn test_two_tier_search_tier1_local() {
    let mock_server = MockServer::start().await;

    // Zero requests expected on mock server
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&mock_server)
        .await;

    let mut catalog = Catalog::default();
    catalog.repositories.push(CatalogRepository {
        id: 1,
        owner: "prasanthrangan".to_string(),
        repo: "hyprdots".to_string(),
        full_name: "prasanthrangan/hyprdots".to_string(),
        html_url: "https://github.com/prasanthrangan/hyprdots".to_string(),
        description: Some("Hyprland Arch dotfiles setup".to_string()),
        stargazers_count: 5000,
        default_branch: "main".to_string(),
        topics: vec!["hyprland".to_string(), "dotfiles".to_string()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![DetectedComponent {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: ".config/hypr".to_string(),
            entrypoint: "hyprland.conf".to_string(),
            style_file: None,
            file_count: 10,
            total_size_bytes: 5000,
        }],
    });
    catalog.repositories.push(CatalogRepository {
        id: 2,
        owner: "end-4".to_string(),
        repo: "dots-hyprland".to_string(),
        full_name: "end-4/dots-hyprland".to_string(),
        html_url: "https://github.com/end-4/dots-hyprland".to_string(),
        description: Some("Material You themed Hyprland setup".to_string()),
        stargazers_count: 4000,
        default_branch: "main".to_string(),
        topics: vec!["hyprland".to_string()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    });
    catalog.repositories.push(CatalogRepository {
        id: 3,
        owner: "other".to_string(),
        repo: "hyprland-config".to_string(),
        full_name: "other/hyprland-config".to_string(),
        html_url: "https://github.com/other/hyprland-config".to_string(),
        description: Some("Minimal hyprland".to_string()),
        stargazers_count: 100,
        default_branch: "main".to_string(),
        topics: vec!["hyprland".to_string()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    });

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    // Tier 1 search with >= 3 local matches consumes ZERO network requests
    let results = client
        .search_two_tier(Some(&catalog), "hyprland")
        .await
        .unwrap();
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_two_tier_search_tier2_debounced() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("sort", "stars"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "total_count": 1,
                    "incomplete_results": false,
                    "items": [
                        {
                            "id": 99,
                            "owner": "remote_user",
                            "repo": "remote_dots",
                            "full_name": "remote_user/remote_dots",
                            "html_url": "https://github.com/remote_user/remote_dots",
                            "description": "Remote fetched dots",
                            "stargazers_count": 999,
                            "default_branch": "main",
                            "topics": ["hyprland"]
                        }
                    ]
                }))
                .append_header("x-ratelimit-resource", "search")
                .append_header("x-ratelimit-limit", "10")
                .append_header("x-ratelimit-remaining", "9")
                .append_header("x-ratelimit-reset", "1761500000"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    // Empty catalog -> forces Tier 2 remote query
    let empty_catalog = Catalog::default();
    let results = client
        .search_two_tier(Some(&empty_catalog), "catppuccin")
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].resolved_full_name(), "remote_user/remote_dots");
    assert_eq!(client.rate_limiter().search_status().remaining, 9);
}

#[tokio::test]
async fn test_github_search_real_api_response_deserialization() {
    let mock_server = MockServer::start().await;

    // Real GitHub Search API response snippet with owner as object and name alias
    let github_search_json = serde_json::json!({
        "total_count": 4365,
        "incomplete_results": false,
        "items": [
            {
                "id": 349947919,
                "name": "awesome-ml4co",
                "full_name": "Thinklab-SJTU/awesome-ml4co",
                "owner": {
                    "login": "Thinklab-SJTU",
                    "id": 55233859,
                    "avatar_url": "https://avatars.githubusercontent.com/u/55233859?v=4"
                },
                "html_url": "https://github.com/Thinklab-SJTU/awesome-ml4co",
                "description": "Awesome machine learning for combinatorial optimization papers.",
                "stargazers_count": 2175,
                "default_branch": "master",
                "topics": ["combinatorial-optimization", "machine-learning"],
                "pushed_at": "2026-10-26T09:38:07Z"
            },
            {
                "id": 5086001,
                "name": "dotfiles",
                "full_name": "mylinuxforwork/dotfiles",
                "owner": {
                    "login": "mylinuxforwork",
                    "id": 123456
                },
                "html_url": "https://github.com/mylinuxforwork/dotfiles",
                "description": "The ML4W Dotfiles for Hyprland",
                "stargazers_count": 5086,
                "default_branch": "main",
                "topics": ["hyprland", "dotfiles", "waybar"]
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(github_search_json)
                .append_header("x-ratelimit-resource", "search")
                .append_header("x-ratelimit-limit", "10")
                .append_header("x-ratelimit-remaining", "8"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    let resp = client
        .search_repositories("mL4")
        .await
        .expect("Search must succeed without deserialization error");

    assert_eq!(resp.total_count, 4365);
    assert_eq!(resp.items.len(), 2);
    assert_eq!(resp.items[0].resolved_full_name(), "Thinklab-SJTU/awesome-ml4co");
    assert_eq!(resp.items[0].owner, "Thinklab-SJTU");
    assert_eq!(resp.items[0].repo, "awesome-ml4co");
    assert_eq!(resp.items[0].stars(), 2175);
    assert_eq!(resp.items[1].resolved_full_name(), "mylinuxforwork/dotfiles");
    assert_eq!(resp.items[1].owner, "mylinuxforwork");
    assert_eq!(resp.items[1].repo, "dotfiles");
    assert_eq!(resp.items[1].stars(), 5086);
}
