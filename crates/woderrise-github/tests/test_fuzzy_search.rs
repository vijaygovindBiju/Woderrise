//! Comprehensive unit tests for typo-tolerant fuzzy search engine (R4).

use woderrise_core::models::{
    Catalog, CatalogRepository, ConfigComponent, DetectedComponent, SyntaxFormat,
};
use woderrise_github::search::{filter_local_catalog, score_repository};

fn create_test_repo(
    id: u64,
    owner: &str,
    repo: &str,
    desc: &str,
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
        description: Some(desc.to_string()),
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
fn test_fuzzy_search_typo_hyprlnd_matches_hyprland() {
    let catalog = Catalog::default_popular();
    let results = filter_local_catalog(&catalog, "hyprlnd");

    assert!(
        !results.is_empty(),
        "Typo 'hyprlnd' must return Hyprland repositories"
    );

    // All returned repositories should relate to Hyprland
    for repo in &results {
        let matches_hypr = repo.resolved_full_name().to_lowercase().contains("hypr")
            || repo.topics.iter().any(|t| t.contains("hypr"))
            || repo
                .detected_components
                .iter()
                .any(|c| c.component.as_str().contains("hypr"));
        assert!(
            matches_hypr,
            "Repo {} should match hyprland",
            repo.resolved_full_name()
        );
    }
}

#[test]
fn test_fuzzy_search_typo_waybr_matches_waybar() {
    let catalog = Catalog::default_popular();
    let results = filter_local_catalog(&catalog, "waybr");

    assert!(
        !results.is_empty(),
        "Typo 'waybr' must match Waybar repositories"
    );

    // The top result should feature a Waybar component or topic
    let top = &results[0];
    let has_waybar = top
        .detected_components
        .iter()
        .any(|c| c.component == ConfigComponent::Waybar)
        || top.topics.iter().any(|t| t == "waybar");
    assert!(
        has_waybar,
        "Top match {} should feature Waybar",
        top.resolved_full_name()
    );
}

#[test]
fn test_fuzzy_search_abbreviation_ml4_and_ml4w_matches_mylinuxforwork() {
    let catalog = Catalog::default_popular();

    // Test 'ml4'
    let results_ml4 = filter_local_catalog(&catalog, "ml4");
    assert!(
        !results_ml4.is_empty(),
        "Abbreviation 'ml4' must return results"
    );
    assert_eq!(
        results_ml4[0].owner, "mylinuxforwork",
        "Top result for 'ml4' should be mylinuxforwork/dotfiles"
    );

    // Test 'ml4w'
    let results_ml4w = filter_local_catalog(&catalog, "ml4w");
    assert!(
        !results_ml4w.is_empty(),
        "Abbreviation 'ml4w' must return results"
    );
    assert_eq!(
        results_ml4w[0].owner, "mylinuxforwork",
        "Top result for 'ml4w' should be mylinuxforwork/dotfiles"
    );
}

#[test]
fn test_fuzzy_search_exact_matches_rank_higher_than_fuzzy_matches() {
    let repo_exact = create_test_repo(
        1,
        "custom",
        "waybar-config",
        "Exact waybar configuration",
        100,
        vec!["waybar"],
        vec![ConfigComponent::Waybar],
    );
    let repo_typo = create_test_repo(
        2,
        "other",
        "waybr-setup",
        "Waybar setup with slight typo",
        500, // Higher stars, but only typo match for query "waybar" if name is waybr
        vec!["desktop"],
        vec![],
    );

    let catalog = Catalog {
        repositories: vec![repo_typo.clone(), repo_exact.clone()],
        ..Default::default()
    };

    let results = filter_local_catalog(&catalog, "waybar");
    assert!(!results.is_empty());
    assert_eq!(
        results[0].id, 1,
        "Exact match repo (id=1) must rank higher than fuzzy/typo match (id=2) despite lower star count"
    );

    let score_exact = score_repository(&repo_exact, "waybar").expect("Must score");
    let score_typo = score_repository(&repo_typo, "waybar").expect("Must score");
    assert!(
        score_exact > score_typo,
        "Exact match score ({}) must exceed typo score ({})",
        score_exact,
        score_typo
    );
}

#[test]
fn test_fuzzy_search_empty_and_whitespace_returns_all_sorted_by_stars() {
    let repo1 = create_test_repo(1, "a", "repo1", "first", 50, vec![], vec![]);
    let repo2 = create_test_repo(2, "b", "repo2", "second", 5000, vec![], vec![]);
    let repo3 = create_test_repo(3, "c", "repo3", "third", 200, vec![], vec![]);

    let catalog = Catalog {
        repositories: vec![repo1, repo2, repo3],
        ..Default::default()
    };

    // Empty query
    let empty_res = filter_local_catalog(&catalog, "");
    assert_eq!(empty_res.len(), 3);
    assert_eq!(empty_res[0].id, 2); // 5000 stars
    assert_eq!(empty_res[1].id, 3); // 200 stars
    assert_eq!(empty_res[2].id, 1); // 50 stars

    // Whitespace-only query
    let ws_res = filter_local_catalog(&catalog, "   \t\n  ");
    assert_eq!(ws_res.len(), 3);
    assert_eq!(ws_res[0].id, 2);
    assert_eq!(ws_res[1].id, 3);
    assert_eq!(ws_res[2].id, 1);
}

#[test]
fn test_fuzzy_search_bogus_query_returns_empty() {
    let catalog = Catalog::default_popular();
    let results = filter_local_catalog(&catalog, "zzzyyyxxx12345nonexistent");
    assert!(
        results.is_empty(),
        "Completely irrelevant query must return empty list"
    );
}

#[test]
fn test_score_repository_api_matches_and_exclusions() {
    let catalog = Catalog::default_popular();
    let ml4w = &catalog.repositories[0];

    // Relevant query produces Some(score) with positive value
    let score = score_repository(ml4w, "hyprland");
    assert!(score.is_some());
    assert!(score.unwrap() > 1000.0);

    // Abbreviation produces high score
    let score_abbr = score_repository(ml4w, "ml4");
    assert!(score_abbr.is_some());
    assert!(score_abbr.unwrap() > 900.0);

    // Bogus query produces None
    let score_bogus = score_repository(ml4w, "qwertyuiopasdfghjkl");
    assert!(score_bogus.is_none());
}
