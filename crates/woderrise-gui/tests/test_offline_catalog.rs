//! Integration tests for curated static catalog feed & 4-tier offline fallback (R5).

use camino::Utf8PathBuf;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use woderrise_core::models::{
    Catalog, CatalogRepository, ConfigComponent, DetectedComponent, SyntaxFormat,
};
use woderrise_core::schema::{read_json_file, write_json_file_durably};
use woderrise_github::{GitHubClientBuilder, RateLimitResource};
use woderrise_gui::resolve_catalog_4tier;

fn create_sample_catalog(id: u64, name: &str) -> Catalog {
    Catalog {
        schema: Some("https://woderrise.dev/schemas/catalog.v1.json".to_string()),
        version: 1,
        last_synced: chrono::Utc::now(),
        repositories: vec![CatalogRepository {
            id,
            owner: "testowner".to_string(),
            repo: name.to_string(),
            full_name: format!("testowner/{}", name),
            html_url: format!("https://github.com/testowner/{}", name),
            description: Some("Test catalog repository".to_string()),
            stargazers_count: 500,
            default_branch: "main".to_string(),
            topics: vec!["hyprland".to_string(), "waybar".to_string()],
            pushed_at: None,
            cached_tree_sha: None,
            etag: None,
            detected_components: vec![DetectedComponent {
                component: ConfigComponent::Hyprland,
                syntax_format: SyntaxFormat::LegacyConf,
                root_path: "hypr".to_string(),
                entrypoint: "hyprland.conf".to_string(),
                style_file: None,
                file_count: 5,
                total_size_bytes: 12000,
            }],
        }],
    }
}

#[tokio::test]
async fn test_tier1_remote_cdn_success_and_caches_to_xdg() {
    let mock_server = MockServer::start().await;

    let sample_catalog = create_sample_catalog(99001, "tier1-cdn-package");
    let cdn_json = serde_json::to_string(&sample_catalog).expect("Must serialize sample catalog");

    Mock::given(method("GET"))
        .and(path("/registry/catalog.v1.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cdn_json))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().expect("Create temp dir");
    let cache_file = Utf8PathBuf::from_path_buf(temp_dir.path().join("catalog.json"))
        .expect("Convert to Utf8PathBuf");

    assert!(
        !cache_file.exists(),
        "Cache file must not exist before Tier 1 fetch"
    );

    let feed_url = format!("{}/registry/catalog.v1.json", mock_server.uri());
    let catalog = resolve_catalog_4tier(Some(&feed_url), Some(&cache_file)).await;

    // Verify repository was returned from Tier 1 CDN
    assert_eq!(catalog.repositories.len(), 1);
    assert_eq!(catalog.repositories[0].id, 99001);
    assert_eq!(catalog.repositories[0].repo, "tier1-cdn-package");

    // Verify Tier 1 durably wrote to local XDG cache
    assert!(
        cache_file.exists(),
        "Tier 1 must durably write fetched catalog to local XDG cache"
    );
    let cached: Catalog = read_json_file(&cache_file).expect("Must read cached JSON");
    assert_eq!(cached.repositories.len(), 1);
    assert_eq!(cached.repositories[0].id, 99001);
}

#[tokio::test]
async fn test_tier2_offline_cdn_fallback_to_xdg_cache() {
    let temp_dir = TempDir::new().expect("Create temp dir");
    let cache_file = Utf8PathBuf::from_path_buf(temp_dir.path().join("catalog.json"))
        .expect("Convert to Utf8PathBuf");

    // Pre-populate Tier 2 local XDG cache
    let cached_catalog = create_sample_catalog(99002, "tier2-cached-package");
    write_json_file_durably(&cache_file, &cached_catalog).expect("Write cache");
    assert!(cache_file.exists());

    // Point CDN URL to an unreachable / offline socket
    let offline_url = "http://127.0.0.1:9/unreachable_feed.json";
    let catalog = resolve_catalog_4tier(Some(offline_url), Some(&cache_file)).await;

    // Must successfully fall back to Tier 2 local cache
    assert_eq!(catalog.repositories.len(), 1);
    assert_eq!(catalog.repositories[0].id, 99002);
    assert_eq!(catalog.repositories[0].repo, "tier2-cached-package");
}

#[tokio::test]
async fn test_tier3_offline_and_no_cache_fallback_to_bundled() {
    let temp_dir = TempDir::new().expect("Create temp dir");
    let non_existent_cache =
        Utf8PathBuf::from_path_buf(temp_dir.path().join("missing_catalog.json"))
            .expect("Convert to Utf8PathBuf");

    assert!(!non_existent_cache.exists());

    // Both Tier 1 (offline socket) and Tier 2 (missing file) fail
    let offline_url = "http://127.0.0.1:9/offline.json";
    let catalog = resolve_catalog_4tier(Some(offline_url), Some(&non_existent_cache)).await;

    // Must gracefully fall back to Tier 3 bundled static catalog without panicking or hanging
    assert!(
        !catalog.repositories.is_empty(),
        "Tier 3 bundled catalog must return non-empty repositories"
    );

    // Verify presence of genuine curated packages in bundled catalog
    let has_ml4w = catalog
        .repositories
        .iter()
        .any(|r| r.resolved_full_name() == "mylinuxforwork/dotfiles");
    assert!(
        has_ml4w,
        "Bundled catalog must contain mylinuxforwork/dotfiles"
    );

    let has_end4 = catalog
        .repositories
        .iter()
        .any(|r| r.resolved_full_name() == "end-4/dots-hyprland");
    assert!(has_end4, "Bundled catalog must contain end-4/dots-hyprland");
}

#[test]
fn test_curated_static_catalog_json_schema_conformance() {
    const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
    let catalog: Catalog = serde_json::from_str(BUNDLED_JSON)
        .expect("registry/catalog.v1.json must deserialize cleanly as Catalog");

    assert_eq!(
        catalog.schema.as_deref(),
        Some("https://woderrise.dev/schemas/catalog.v1.json")
    );
    assert_eq!(catalog.version, 1);
    assert!(
        catalog.repositories.len() >= 5,
        "Curated feed must contain at least 5 top real-world packages"
    );

    for repo in &catalog.repositories {
        assert!(!repo.full_name.is_empty(), "full_name must be non-empty");
        assert!(!repo.html_url.is_empty(), "html_url must be non-empty");
        assert!(repo.stargazers_count > 0, "stargazers_count must be > 0");
        assert!(!repo.topics.is_empty(), "topics must be non-empty");
        assert!(
            !repo.detected_components.is_empty(),
            "detected_components must contain pre-audited components"
        );
    }
}

#[tokio::test]
async fn test_catalog_refresh_consumes_zero_search_api_quota() {
    let client = GitHubClientBuilder::new().build().expect("Build client");

    let initial_search_quota = client.rate_limiter().search_status();
    let initial_core_quota = client.rate_limiter().status(RateLimitResource::Core);

    // Run Tier 1/2/3 fallback
    let temp_dir = TempDir::new().expect("Create temp dir");
    let cache_file = Utf8PathBuf::from_path_buf(temp_dir.path().join("catalog.json"))
        .expect("Convert to Utf8PathBuf");

    let _catalog = resolve_catalog_4tier(None, Some(&cache_file)).await;

    // Verify rate limit tracker was not decremented
    let final_search_quota = client.rate_limiter().search_status();
    let final_core_quota = client.rate_limiter().status(RateLimitResource::Core);

    assert_eq!(
        initial_search_quota.remaining, final_search_quota.remaining,
        "Static catalog feed loading must consume 0 GitHub Search API quota"
    );
    assert_eq!(
        initial_core_quota.remaining, final_core_quota.remaining,
        "Static catalog feed loading must consume 0 GitHub Core REST API quota"
    );
}
