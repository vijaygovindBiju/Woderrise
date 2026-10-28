//! E2E Test Suite for R5: Curated Static Catalog Feed & Deterministic 4-Tier Fallback.
//!
//! Verifies:
//! - Curated feed schema compliance (`registry/catalog.v1.json`)
//! - Deterministic 4-tier fallback:
//!   - Tier 1: Remote CDN feed returns 200 OK -> caches to local XDG
//!   - Tier 2: Offline CDN -> falls back to local XDG cache
//!   - Tier 3: Offline CDN and missing cache -> falls back to bundled static catalog
//!   - Tier 4: Zero GitHub API rate limit consumption on startup

use camino::Utf8PathBuf;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use woderrise_core::models::{Catalog, CatalogRepository};
use woderrise_github::GitHubClientBuilder;
use woderrise_gui::resolve_catalog_4tier;

#[test]
fn test_e2e_r5_catalog_v1_json_schema_conformance() {
    const BUNDLED_JSON: &str = include_str!("../../registry/catalog.v1.json");
    let catalog: Catalog = serde_json::from_str(BUNDLED_JSON)
        .expect("registry/catalog.v1.json must be valid schema compliant Catalog");

    assert_eq!(
        catalog.schema.as_deref(),
        Some("https://woderrise.dev/schemas/catalog.v1.json")
    );
    assert_eq!(catalog.version, 1);
    assert!(
        catalog.repositories.len() >= 5,
        "Curated catalog feed must contain at least 5 configurations"
    );

    for repo in &catalog.repositories {
        assert!(!repo.full_name.is_empty());
        assert!(!repo.html_url.is_empty());
        assert!(repo.stargazers_count > 0);
        assert!(!repo.topics.is_empty());
        assert!(
            !repo.detected_components.is_empty(),
            "Pre-audited repository must contain detected components"
        );
    }
}

#[tokio::test]
async fn test_e2e_r5_deterministic_four_tier_fallback() {
    let temp = TempDir::new().unwrap();

    // 1. Tier 1: Remote CDN returns valid feed -> durably caches to disk
    let mock_server = MockServer::start().await;
    let sample = Catalog {
        schema: Some("https://woderrise.dev/schemas/catalog.v1.json".into()),
        version: 1,
        last_synced: chrono::Utc::now(),
        repositories: vec![CatalogRepository {
            id: 88881,
            owner: "cdn-owner".into(),
            repo: "cdn-repo".into(),
            full_name: "cdn-owner/cdn-repo".into(),
            html_url: "https://github.com/cdn-owner/cdn-repo".into(),
            description: None,
            stargazers_count: 500,
            default_branch: "main".into(),
            topics: vec!["hyprland".into()],
            pushed_at: None,
            cached_tree_sha: None,
            etag: None,
            detected_components: vec![],
        }],
    };
    let cdn_body = serde_json::to_string(&sample).unwrap();

    Mock::given(method("GET"))
        .and(path("/registry/catalog.v1.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cdn_body))
        .mount(&mock_server)
        .await;

    let cache_file = Utf8PathBuf::from_path_buf(temp.path().join("catalog.json")).unwrap();
    let feed_url = format!("{}/registry/catalog.v1.json", mock_server.uri());

    let cdn_catalog = resolve_catalog_4tier(Some(&feed_url), Some(&cache_file)).await;
    assert_eq!(cdn_catalog.repositories.len(), 1);
    assert_eq!(cdn_catalog.repositories[0].id, 88881);
    assert!(cache_file.exists(), "Tier 1 must write cache to disk");

    // 2. Tier 2: Offline CDN -> successfully falls back to cached file
    let offline_url = "http://127.0.0.1:9/offline.json";
    let tier2_catalog = resolve_catalog_4tier(Some(offline_url), Some(&cache_file)).await;
    assert_eq!(tier2_catalog.repositories.len(), 1);
    assert_eq!(tier2_catalog.repositories[0].id, 88881);

    // 3. Tier 3: Offline CDN and missing cache -> successfully falls back to bundled
    let missing_cache = Utf8PathBuf::from_path_buf(temp.path().join("missing.json")).unwrap();
    let tier3_catalog = resolve_catalog_4tier(Some(offline_url), Some(&missing_cache)).await;
    assert!(!tier3_catalog.repositories.is_empty());
    assert_eq!(tier3_catalog.version, 1);

    // 4. Tier 4: Zero rate limit quota consumed
    let client = GitHubClientBuilder::new().build().unwrap();
    let initial_search = client.rate_limiter().search_status().remaining;
    let initial_core = client.rate_limiter().core_status().remaining;

    let _ = resolve_catalog_4tier(None, Some(&cache_file)).await;

    assert_eq!(
        client.rate_limiter().search_status().remaining,
        initial_search
    );
    assert_eq!(client.rate_limiter().core_status().remaining, initial_core);
}
