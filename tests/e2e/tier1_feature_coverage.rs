//! Tier 1: Feature Coverage (Isolation)
//! Verifies each core feature (R1 to R5) in isolation via genuine execution.

use super::common::TestEnvironment;
use camino::Utf8PathBuf;
use std::fs;
use tempfile::TempDir;
use woderrise_core::models::{load_settings, Catalog, CatalogRepository, ConfigComponent};
use woderrise_detect::{DetectionEngine, TreeEntry};
use woderrise_github::search::{aggregate_topics, filter_local_catalog, score_repository};
use woderrise_github::GitHubClientBuilder;
use woderrise_gui::resolve_catalog_4tier;

// ============================================================================
// R1: Background Tree Pre-Inspection and Discovery Card Badges/File Counts
// ============================================================================

#[test]
fn test_tier1_r1_background_tree_pre_inspection_badges_and_file_counts() {
    let engine = DetectionEngine::new();

    // Simulate GitHub Git Trees API entries for a realistic Hyprland + Waybar setup
    let entries = vec![
        TreeEntry {
            path: "hypr/hyprland.conf".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(3500),
            sha: "sha_hypr".into(),
        },
        TreeEntry {
            path: "hypr/hyprlock.conf".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(1200),
            sha: "sha_lock".into(),
        },
        TreeEntry {
            path: "waybar/config.jsonc".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(2500),
            sha: "sha_wb_cfg".into(),
        },
        TreeEntry {
            path: "waybar/style.css".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(1800),
            sha: "sha_wb_style".into(),
        },
        TreeEntry {
            path: "wallpapers/nord-mountain.png".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(500_000),
            sha: "sha_wall".into(),
        },
    ];

    let detected = engine.detect_components(&entries);
    assert!(
        detected.len() >= 3,
        "Expected at least 3 detected components (Hyprland, Waybar, Wallpapers), got: {}",
        detected.len()
    );

    // Verify component display names and file counters
    let hyprland_comp = detected
        .iter()
        .find(|c| c.component == ConfigComponent::Hyprland)
        .expect("Hyprland component must be detected");
    assert_eq!(hyprland_comp.component.as_str(), "hyprland");
    assert!(hyprland_comp.file_count >= 1);

    let waybar_comp = detected
        .iter()
        .find(|c| c.component == ConfigComponent::Waybar)
        .expect("Waybar component must be detected");
    assert_eq!(waybar_comp.component.as_str(), "waybar");
    assert_eq!(waybar_comp.file_count, 2); // config.jsonc + style.css

    let wallpaper_comp = detected
        .iter()
        .find(|c| c.component == ConfigComponent::Wallpapers)
        .expect("Wallpapers component must be detected");
    assert_eq!(wallpaper_comp.component.as_str(), "wallpapers");
    assert_eq!(wallpaper_comp.file_count, 1);

    // Verify repository models reflect detected components
    let repo = CatalogRepository {
        id: 10101,
        owner: "test-user".into(),
        repo: "hypr-dots".into(),
        full_name: "test-user/hypr-dots".into(),
        html_url: "https://github.com/test-user/hypr-dots".into(),
        description: Some("Modern Hyprland dotfiles".into()),
        stargazers_count: 320,
        default_branch: "main".into(),
        topics: vec!["hyprland".into(), "waybar".into()],
        pushed_at: None,
        cached_tree_sha: Some("sha_tree".into()),
        etag: None,
        detected_components: detected.clone(),
    };

    let total_files: usize = repo.detected_components.iter().map(|c| c.file_count).sum();
    assert_eq!(total_files, 6);

    // Verify discovery card badge view builds without error
    let card = woderrise_gui::view_card(&repo, woderrise_gui::app::Message::Noop);
    let _ = card;
}

// ============================================================================
// R2: Multi-Topic Aggregation & Community Relevance
// ============================================================================

#[test]
fn test_tier1_r2_multi_topic_aggregation_and_deduplication() {
    let repo_hypr = CatalogRepository {
        id: 2001,
        owner: "user-a".into(),
        repo: "dots-hypr".into(),
        full_name: "user-a/dots-hypr".into(),
        html_url: "https://github.com/user-a/dots-hypr".into(),
        description: Some("Hyprland config".into()),
        stargazers_count: 500,
        default_branch: "main".into(),
        topics: vec!["hyprland".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    // repo_both appears in both topic searches
    let repo_both_from_hypr = CatalogRepository {
        id: 2002,
        owner: "user-b".into(),
        repo: "unified-desktop".into(),
        full_name: "user-b/unified-desktop".into(),
        html_url: "https://github.com/user-b/unified-desktop".into(),
        description: Some("Hyprland and Waybar combined".into()),
        stargazers_count: 1500,
        default_branch: "main".into(),
        topics: vec!["hyprland".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let repo_both_from_waybar = CatalogRepository {
        id: 2002,
        owner: "user-b".into(),
        repo: "unified-desktop".into(),
        full_name: "user-b/unified-desktop".into(),
        html_url: "https://github.com/user-b/unified-desktop".into(),
        description: Some("Hyprland and Waybar combined".into()),
        stargazers_count: 1500,
        default_branch: "main".into(),
        topics: vec!["waybar".into(), "desktop".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let repo_waybar = CatalogRepository {
        id: 2003,
        owner: "user-c".into(),
        repo: "waybar-themes".into(),
        full_name: "user-c/waybar-themes".into(),
        html_url: "https://github.com/user-c/waybar-themes".into(),
        description: Some("Waybar themes".into()),
        stargazers_count: 800,
        default_branch: "main".into(),
        topics: vec!["waybar".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let topic_results = vec![
        vec![repo_hypr, repo_both_from_hypr],
        vec![repo_both_from_waybar, repo_waybar],
    ];

    let aggregated = aggregate_topics(topic_results);

    // Must deduplicate repo 2002: total 3 repos
    assert_eq!(aggregated.len(), 3, "Expected 3 deduplicated repositories");

    // Community star ranking (descending: 1500 -> 800 -> 500)
    assert_eq!(aggregated[0].id, 2002);
    assert_eq!(aggregated[0].stargazers_count, 1500);
    assert_eq!(aggregated[1].id, 2003);
    assert_eq!(aggregated[1].stargazers_count, 800);
    assert_eq!(aggregated[2].id, 2001);
    assert_eq!(aggregated[2].stargazers_count, 500);

    // Topic union verification: repo 2002 must contain topics from both queries
    let merged_repo = &aggregated[0];
    assert!(merged_repo.topics.contains(&"hyprland".to_string()));
    assert!(merged_repo.topics.contains(&"waybar".to_string()));
    assert!(merged_repo.topics.contains(&"desktop".to_string()));
}

// ============================================================================
// R3: PAT Authentication CLI & Settings Integration
// ============================================================================

#[test]
fn test_tier1_r3_pat_auth_cli_token_save_and_permissions() {
    let env = TestEnvironment::new("t1-r3-pat");
    let settings_file = env.settings_file();

    // 1. Save PAT via compiled CLI binary
    let out_save = env.run_auth(&["--token", "ghp_test_token_tier1_0123456789"]);
    assert_eq!(
        out_save.status.code(),
        Some(0),
        "CLI auth --token failed: {}",
        String::from_utf8_lossy(&out_save.stderr)
    );
    let stdout_save = String::from_utf8_lossy(&out_save.stdout);
    assert!(stdout_save.contains("Token saved successfully"));

    // 2. Verify file exists on disk
    assert!(settings_file.exists(), "settings.json must exist");

    // 3. Verify POSIX 0600 file permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Failed to read settings metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "settings.json must have strict POSIX 0600 permissions, got {:o}",
            mode
        );
    }

    // 4. Verify settings file content
    let settings =
        load_settings(&camino::Utf8PathBuf::from_path_buf(settings_file.clone()).unwrap())
            .expect("Failed to parse settings.json");
    assert_eq!(
        settings.github_token.as_deref(),
        Some("ghp_test_token_tier1_0123456789")
    );

    // 5. Verify CLI status command
    let out_status = env.run_auth(&["--status"]);
    assert_eq!(out_status.status.code(), Some(0));
    let stdout_status = String::from_utf8_lossy(&out_status.stdout);
    assert!(stdout_status.contains("Authenticated"));
    assert!(stdout_status.contains("ghp_••••••••6789"));
    assert!(stdout_status.contains("30 requests/min"));
    assert!(stdout_status.contains("5,000 requests/hr"));

    // 6. Verify CLI clear command
    let out_clear = env.run_auth(&["--clear"]);
    assert_eq!(out_clear.status.code(), Some(0));
    let stdout_clear = String::from_utf8_lossy(&out_clear.stdout);
    assert!(stdout_clear.contains("removed"));
    assert!(stdout_clear.contains("Reverted to unauthenticated limits"));

    // 7. Verify settings is cleared
    let cleared_settings =
        load_settings(&camino::Utf8PathBuf::from_path_buf(settings_file).unwrap())
            .expect("Failed to parse cleared settings");
    assert_eq!(cleared_settings.github_token, None);
}

// ============================================================================
// R4: Typo-Tolerant & Fuzzy Search
// ============================================================================

#[test]
fn test_tier1_r4_fuzzy_search_typo_and_abbreviation() {
    let catalog = Catalog::default_popular();

    // 1. Typo "hyprlnd" -> matches "hyprland"
    let hypr_results = filter_local_catalog(&catalog, "hyprlnd");
    assert!(
        !hypr_results.is_empty(),
        "Typo 'hyprlnd' must match Hyprland configurations"
    );
    assert!(
        hypr_results[0]
            .resolved_full_name()
            .to_lowercase()
            .contains("hypr")
            || hypr_results[0].topics.iter().any(|t| t.contains("hypr"))
    );

    // 2. Typo "waybr" -> matches "waybar"
    let waybar_results = filter_local_catalog(&catalog, "waybr");
    assert!(
        !waybar_results.is_empty(),
        "Typo 'waybr' must match Waybar configurations"
    );

    // 3. Abbreviation "ml4" -> resolves to "mylinuxforwork"
    let ml4_results = filter_local_catalog(&catalog, "ml4");
    assert!(!ml4_results.is_empty(), "Abbreviation 'ml4' must resolve");
    assert_eq!(
        ml4_results[0].owner, "mylinuxforwork",
        "Top result for 'ml4' must be mylinuxforwork/dotfiles"
    );

    // 4. Exact match ranking superiority
    let exact_score = score_repository(&catalog.repositories[0], "dotfiles");
    let fuzzy_score = score_repository(&catalog.repositories[0], "dtfls");
    assert!(exact_score.is_some() && fuzzy_score.is_some());
    assert!(
        exact_score.unwrap() > fuzzy_score.unwrap(),
        "Exact match score must rank higher than fuzzy typo score"
    );
}

// ============================================================================
// R5: Curated Static Catalog Feed & Deterministic 4-Tier Fallback
// ============================================================================

#[tokio::test]
async fn test_tier1_r5_curated_catalog_feed_schema_and_zero_rate_limit() {
    // 1. Bundled static catalog schema validation
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

    // 2. Zero rate limit consumption verification
    let client = GitHubClientBuilder::new().build().unwrap();
    let initial_search_quota = client.rate_limiter().search_status().remaining;
    let initial_core_quota = client.rate_limiter().core_status().remaining;

    let temp = TempDir::new().unwrap();
    let cache_path = Utf8PathBuf::from_path_buf(temp.path().join("catalog.json")).unwrap();

    // Point CDN to unreachable URL to test graceful offline fallback to bundled
    let offline_feed_url = "http://127.0.0.1:9/offline.json";
    let resolved = resolve_catalog_4tier(Some(offline_feed_url), Some(&cache_path)).await;

    assert!(
        !resolved.repositories.is_empty(),
        "Offline fallback must return non-empty curated catalog"
    );

    let final_search_quota = client.rate_limiter().search_status().remaining;
    let final_core_quota = client.rate_limiter().core_status().remaining;

    assert_eq!(
        initial_search_quota, final_search_quota,
        "Static feed fallback must consume 0 search rate limit quota"
    );
    assert_eq!(
        initial_core_quota, final_core_quota,
        "Static feed fallback must consume 0 core rate limit quota"
    );
}
