//! Adversarial empirical challenge tests for Milestone M6:
//! Fuzzy Search & Curated Static Feed / Offline Support (R4 & R5).

use camino::Utf8PathBuf;
use std::fs;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use woderrise_core::models::{
    Catalog, CatalogRepository, ConfigComponent, DetectedComponent, SyntaxFormat,
};
use woderrise_core::paths::XdgPaths;
use woderrise_core::schema::write_json_file_durably;
use woderrise_detect::DetectionEngine;
use woderrise_github::search::{filter_local_catalog, score_repository};
use woderrise_github::{GitHubClientBuilder, RateLimitResource};
use woderrise_gui::app::{EngineEvent, GuiCommand};
use woderrise_gui::{resolve_catalog_4tier, run_worker};
use woderrise_transaction::TransactionEngine;

fn make_dummy_repo(id: u64, owner: &str, repo: &str, stars: u64) -> CatalogRepository {
    CatalogRepository {
        id,
        owner: owner.to_string(),
        repo: repo.to_string(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: Some(format!("Test repository {}/{}", owner, repo)),
        stargazers_count: stars,
        default_branch: "main".to_string(),
        topics: vec!["hyprland".to_string(), "waybar".to_string()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![DetectedComponent {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: ".config/hypr".to_string(),
            entrypoint: "hyprland.conf".to_string(),
            style_file: None,
            file_count: 5,
            total_size_bytes: 10000,
        }],
    }
}

// =========================================================================
// 1. Adversarial Offline & CDN Failure Tests (Tier 1 Failure Modes)
// =========================================================================

#[tokio::test]
async fn test_adversarial_offline_broken_dns_fallback_to_bundled() {
    let temp_dir = TempDir::new().expect("tempdir");
    let non_existent_cache =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("missing.json")).expect("Utf8PathBuf");

    // Non-resolvable DNS domain name
    let broken_dns_url =
        "http://this-domain-does-not-exist-woderrise-offline-999.invalid/feed.json";
    let catalog = resolve_catalog_4tier(Some(broken_dns_url), Some(&non_existent_cache)).await;

    // Must gracefully fall back to Tier 3 bundled catalog
    assert!(
        !catalog.repositories.is_empty(),
        "Broken DNS must gracefully fall back to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_offline_connection_refused_fallback_to_bundled() {
    let temp_dir = TempDir::new().expect("tempdir");
    let non_existent_cache =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("missing.json")).expect("Utf8PathBuf");

    // Port 1 on loopback should refuse connection immediately
    let refused_url = "http://127.0.0.1:1/feed.json";
    let catalog = resolve_catalog_4tier(Some(refused_url), Some(&non_existent_cache)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "Connection refused must fall back to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_offline_http_500_internal_error() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/catalog.json"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().expect("tempdir");
    let non_existent_cache =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("missing.json")).expect("Utf8PathBuf");

    let cdn_url = format!("{}/catalog.json", mock_server.uri());
    let catalog = resolve_catalog_4tier(Some(&cdn_url), Some(&non_existent_cache)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "HTTP 500 must trigger fallback to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_offline_http_503_service_unavailable() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/catalog.json"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().expect("tempdir");
    let non_existent_cache =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("missing.json")).expect("Utf8PathBuf");

    let cdn_url = format!("{}/catalog.json", mock_server.uri());
    let catalog = resolve_catalog_4tier(Some(&cdn_url), Some(&non_existent_cache)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "HTTP 503 must trigger fallback to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_offline_http_404_not_found() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/catalog.json"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().expect("tempdir");
    let non_existent_cache =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("missing.json")).expect("Utf8PathBuf");

    let cdn_url = format!("{}/catalog.json", mock_server.uri());
    let catalog = resolve_catalog_4tier(Some(&cdn_url), Some(&non_existent_cache)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "HTTP 404 must trigger fallback to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_offline_cdn_malformed_json_response() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/catalog.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<<< NOT JSON AT ALL >>>"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().expect("tempdir");
    let non_existent_cache =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("missing.json")).expect("Utf8PathBuf");

    let cdn_url = format!("{}/catalog.json", mock_server.uri());
    let catalog = resolve_catalog_4tier(Some(&cdn_url), Some(&non_existent_cache)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "Malformed JSON response from CDN must trigger fallback to Tier 3"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_offline_cdn_empty_repositories_array() {
    let mock_server = MockServer::start().await;
    let empty_catalog_json = r#"{"$schema":"https://woderrise.dev/schemas/catalog.v1.json","version":1,"last_synced":"2026-10-28T00:00:00Z","repositories":[]}"#;

    Mock::given(method("GET"))
        .and(path("/catalog.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(empty_catalog_json))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().expect("tempdir");
    let non_existent_cache =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("missing.json")).expect("Utf8PathBuf");

    let cdn_url = format!("{}/catalog.json", mock_server.uri());
    let catalog = resolve_catalog_4tier(Some(&cdn_url), Some(&non_existent_cache)).await;

    // Empty catalog returned from CDN should not be accepted as valid; must fallback to Tier 3
    assert!(
        !catalog.repositories.is_empty(),
        "Empty repositories array from CDN must trigger fallback to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_offline_cdn_timeout_fallback_to_bundled() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/catalog.json"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(4500)))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().expect("tempdir");
    let non_existent_cache =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("missing.json")).expect("Utf8PathBuf");

    let cdn_url = format!("{}/catalog.json", mock_server.uri());
    let start = std::time::Instant::now();
    let catalog = resolve_catalog_4tier(Some(&cdn_url), Some(&non_existent_cache)).await;
    let elapsed = start.elapsed();

    assert!(
        !catalog.repositories.is_empty(),
        "CDN response exceeding 4s timeout must fall back to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
    assert!(
        elapsed >= Duration::from_millis(3800) && elapsed < Duration::from_millis(7000),
        "Timeout must trigger around 4 seconds, actual: {:?}",
        elapsed
    );
}

// =========================================================================
// 2. Adversarial Corrupted & Absent Cache Tests (Tier 2 Failure Modes)
// =========================================================================

#[tokio::test]
async fn test_adversarial_cache_syntax_error_fallback_to_bundled() {
    let temp_dir = TempDir::new().expect("tempdir");
    let cache_file =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("catalog.json")).expect("Utf8PathBuf");

    // Write syntax error / truncated JSON to cache file
    fs::write(&cache_file, r#"{"version": 1, "last_synced": "#).expect("write corrupt cache");

    let offline_url = "http://127.0.0.1:1/offline.json";
    let catalog = resolve_catalog_4tier(Some(offline_url), Some(&cache_file)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "Corrupted JSON syntax in cache must fall back to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_cache_invalid_schema_types_fallback_to_bundled() {
    let temp_dir = TempDir::new().expect("tempdir");
    let cache_file =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("catalog.json")).expect("Utf8PathBuf");

    // Valid JSON syntax, but invalid schema types (repositories is a number, version is string)
    fs::write(
        &cache_file,
        r#"{"version": "not-a-number", "repositories": 12345}"#,
    )
    .expect("write corrupt types");

    let offline_url = "http://127.0.0.1:1/offline.json";
    let catalog = resolve_catalog_4tier(Some(offline_url), Some(&cache_file)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "Invalid data types in cache must fall back to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_cache_zero_byte_empty_file() {
    let temp_dir = TempDir::new().expect("tempdir");
    let cache_file =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("catalog.json")).expect("Utf8PathBuf");

    // Create 0-byte file
    fs::write(&cache_file, b"").expect("write empty file");

    let offline_url = "http://127.0.0.1:1/offline.json";
    let catalog = resolve_catalog_4tier(Some(offline_url), Some(&cache_file)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "Zero-byte empty cache file must fall back to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_cache_empty_repositories_fallback() {
    let temp_dir = TempDir::new().expect("tempdir");
    let cache_file =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("catalog.json")).expect("Utf8PathBuf");

    // Valid catalog but 0 repositories
    let empty_cat = Catalog {
        repositories: Vec::new(),
        ..Default::default()
    };
    write_json_file_durably(&cache_file, &empty_cat).expect("write empty cat");

    let offline_url = "http://127.0.0.1:1/offline.json";
    let catalog = resolve_catalog_4tier(Some(offline_url), Some(&cache_file)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "Cache with 0 repositories must fall back to Tier 3 bundled catalog"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_cache_is_directory_instead_of_file() {
    let temp_dir = TempDir::new().expect("tempdir");
    let cache_dir =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("catalog.json")).expect("Utf8PathBuf");

    // Create a directory at the expected file path
    fs::create_dir_all(&cache_dir).expect("create dir");

    let offline_url = "http://127.0.0.1:1/offline.json";
    let catalog = resolve_catalog_4tier(Some(offline_url), Some(&cache_dir)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "Cache path being a directory must not panic and must fall back to Tier 3"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

#[tokio::test]
async fn test_adversarial_cache_completely_absent_nested_parent() {
    let temp_dir = TempDir::new().expect("tempdir");
    let deeply_nested_absent = Utf8PathBuf::from_path_buf(
        temp_dir
            .path()
            .join("deep")
            .join("sub")
            .join("dirs")
            .join("catalog.json"),
    )
    .expect("Utf8PathBuf");

    assert!(!deeply_nested_absent.exists());

    let offline_url = "http://127.0.0.1:1/offline.json";
    let catalog = resolve_catalog_4tier(Some(offline_url), Some(&deeply_nested_absent)).await;

    assert!(
        !catalog.repositories.is_empty(),
        "Absent cache in missing nested parent dir must fall back to Tier 3"
    );
    assert_eq!(catalog.repositories.len(), 8);
}

// =========================================================================
// 3. Rate Limit & Zero Quota Verification
// =========================================================================

#[tokio::test]
async fn test_adversarial_zero_search_quota_consumed_via_gui_worker() {
    let client = GitHubClientBuilder::new().build().expect("build client");

    let initial_search_status = client.rate_limiter().search_status();
    let initial_core_status = client.rate_limiter().status(RateLimitResource::Core);

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(16);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(16);

    let temp_dir = TempDir::new().expect("tempdir");
    let base = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let tx_engine = TransactionEngine::new(xdg).expect("tx engine");
    let detect_engine = DetectionEngine::new();

    // Spawn GUI worker background task
    let worker_handle = tokio::spawn(run_worker(
        cmd_rx,
        event_tx,
        client.clone(),
        detect_engine,
        tx_engine,
    ));

    // Send RefreshCatalog command (triggering resolve_catalog_4tier)
    cmd_tx
        .send(GuiCommand::RefreshCatalog)
        .await
        .expect("send RefreshCatalog");

    // Await SearchCompleted event
    let event = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("timed out waiting for EngineEvent")
        .expect("event channel closed");

    match event {
        EngineEvent::SearchCompleted {
            query,
            results,
            search_quota,
        } => {
            assert!(query.is_empty(), "RefreshCatalog query must be empty");
            assert!(
                !results.is_empty(),
                "RefreshCatalog must return populated catalog results"
            );
            assert_eq!(
                search_quota.remaining, initial_search_status.remaining,
                "Worker event must report unchanged search quota"
            );
        }
        other => panic!("Unexpected event received: {:?}", other),
    }

    // Verify rate limiter directly on client
    let final_search_status = client.rate_limiter().search_status();
    let final_core_status = client.rate_limiter().status(RateLimitResource::Core);

    assert_eq!(
        final_search_status.remaining, initial_search_status.remaining,
        "GitHub search quota remaining must remain strictly unchanged (0 quota consumed)"
    );
    assert_eq!(
        final_core_status.remaining, initial_core_status.remaining,
        "GitHub core quota remaining must remain strictly unchanged (0 quota consumed)"
    );

    // Drop cmd_tx to cleanly terminate run_worker loop
    drop(cmd_tx);
    let _ = worker_handle.await;
}

// =========================================================================
// 4. Curated Catalog Schema & Content Validation (registry/catalog.v1.json)
// =========================================================================

#[test]
fn test_adversarial_catalog_v1_json_strict_schema_and_packages() {
    const CATALOG_RAW: &str = include_str!("../../../registry/catalog.v1.json");

    // Strict serde deserialization
    let catalog: Catalog = serde_json::from_str(CATALOG_RAW)
        .expect("registry/catalog.v1.json must deserialize strictly into Catalog");

    assert_eq!(
        catalog.schema.as_deref(),
        Some("https://woderrise.dev/schemas/catalog.v1.json"),
        "$schema must match canonical URI"
    );
    assert_eq!(catalog.version, 1, "Catalog version must be 1");
    assert_eq!(
        catalog.repositories.len(),
        8,
        "Curated feed must contain exactly 8 audited packages"
    );

    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_full_names = std::collections::HashSet::new();

    for (idx, repo) in catalog.repositories.iter().enumerate() {
        // ID uniqueness and non-zero
        assert!(repo.id > 0, "Package #{} must have id > 0", idx);
        assert!(
            seen_ids.insert(repo.id),
            "Package #{} id {} is duplicated",
            idx,
            repo.id
        );

        // Owner and repo validity
        assert!(!repo.owner.is_empty(), "Package #{} owner is empty", idx);
        assert!(!repo.repo.is_empty(), "Package #{} repo is empty", idx);
        assert_eq!(
            repo.full_name,
            format!("{}/{}", repo.owner, repo.repo),
            "Package #{} full_name must match owner/repo",
            idx
        );
        assert!(
            seen_full_names.insert(repo.full_name.clone()),
            "Package #{} full_name {} is duplicated",
            idx,
            repo.full_name
        );

        // GitHub URL validity
        assert_eq!(
            repo.html_url,
            format!("https://github.com/{}", repo.full_name),
            "Package #{} html_url must point to GitHub",
            idx
        );

        // Description non-empty
        let desc = repo.description.as_deref().unwrap_or_default();
        assert!(
            !desc.is_empty() && desc.len() >= 10,
            "Package #{} description must be non-empty and descriptive: {:?}",
            idx,
            desc
        );

        // Stargazers and branch
        assert!(
            repo.stargazers_count > 0,
            "Package #{} stars must be > 0",
            idx
        );
        assert_eq!(
            repo.default_branch, "main",
            "Package #{} default branch must be 'main'",
            idx
        );

        // Topics
        assert!(
            !repo.topics.is_empty(),
            "Package #{} topics must be non-empty",
            idx
        );
        for topic in &repo.topics {
            assert!(
                !topic.is_empty(),
                "Package #{} contains empty topic string",
                idx
            );
        }

        // Audited components
        assert!(
            !repo.detected_components.is_empty(),
            "Package #{} detected_components must be non-empty",
            idx
        );

        for comp in &repo.detected_components {
            assert!(
                !comp.root_path.is_empty(),
                "Package #{} component root_path must be non-empty",
                idx
            );
            assert!(
                !comp.entrypoint.is_empty(),
                "Package #{} component entrypoint must be non-empty",
                idx
            );
            assert!(
                comp.file_count > 0,
                "Package #{} component file_count must be > 0",
                idx
            );
            assert!(
                comp.total_size_bytes > 0,
                "Package #{} component total_size_bytes must be > 0",
                idx
            );
        }
    }
}

// =========================================================================
// 5. Fuzzy Search Edge Cases & Hostile Inputs
// =========================================================================

#[test]
fn test_adversarial_fuzzy_search_hostile_inputs() {
    let catalog = Catalog::default_popular();

    let hostile_queries = [
        "",                               // Empty
        "   \t\n\r  ",                    // Whitespace only
        "🦀🦀🦀",                         // Multi-byte Unicode Emoji
        "日本語の検索",                   // CJK characters
        "русский текст",                  // Cyrillic
        "../../../etc/shadow",            // Directory traversal
        "'; DROP TABLE repositories; --", // SQL injection
        "<script>alert('xss')</script>",  // HTML/XSS injection
        "%s%s%s%s%s%n",                   // Format string specifiers
        "\0\0\0null_bytes\0\0",           // Null bytes
        &"A".repeat(5000),                // Extremely long token
        &"word ".repeat(500),             // Many tokens
    ];

    for query in hostile_queries {
        let results = filter_local_catalog(&catalog, query);
        // None of these hostile queries may panic, crash, or return inconsistent lengths
        if query.trim().is_empty() {
            assert_eq!(
                results.len(),
                catalog.repositories.len(),
                "Empty query must return all catalog repositories"
            );
        } else {
            // Non-matching hostile queries should return 0 results cleanly without error
            assert!(
                results.len() <= catalog.repositories.len(),
                "Filtered results must not exceed catalog repository count"
            );
        }
    }
}

#[test]
fn test_adversarial_fuzzy_search_ranking_determinism_and_tie_breaking() {
    // 3 repos with identical scores but different stars and IDs
    let repo_a = make_dummy_repo(10, "test", "identical-name", 100);
    let repo_b = make_dummy_repo(20, "test", "identical-name", 500);
    let repo_c = make_dummy_repo(30, "test", "identical-name", 500);

    let catalog = Catalog {
        repositories: vec![repo_a.clone(), repo_b.clone(), repo_c.clone()],
        ..Default::default()
    };

    let score_b = score_repository(&repo_b, "identical-name").expect("score repo_b");
    let score_c = score_repository(&repo_c, "identical-name").expect("score repo_c");
    assert!(
        (score_b - score_c).abs() < f64::EPSILON,
        "Base scores must be identical"
    );

    let results = filter_local_catalog(&catalog, "identical-name");
    assert_eq!(results.len(), 3);

    // Repos with 500 stars must rank ahead of repo with 100 stars
    assert_eq!(results[0].stargazers_count, 500);
    assert_eq!(results[1].stargazers_count, 500);
    assert_eq!(results[2].stargazers_count, 100);
    assert_eq!(results[2].id, 10);

    // Tie-breaker between repo_b and repo_c: lower ID (20 vs 30)
    assert_eq!(results[0].id, 20);
    assert_eq!(results[1].id, 30);
}
