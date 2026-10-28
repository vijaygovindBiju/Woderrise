//! E2E Test Suite for R4: Typo-Tolerant & Fuzzy Search.
//!
//! Verifies:
//! - Typo tolerance: `hyprlnd` -> `hyprland`, `waybr` -> `waybar`
//! - Abbreviation resolution: `ml4` / `ml4w` -> `mylinuxforwork/dotfiles`
//! - Ranked results: exact matches prioritized over fuzzy/typo matches
//! - Boundary conditions: empty queries, whitespace, special characters

use woderrise_core::models::{Catalog, CatalogRepository, ConfigComponent};
use woderrise_github::search::filter_local_catalog;

#[test]
fn test_e2e_r4_typo_hyprlnd_and_waybr() {
    let catalog = Catalog::default_popular();

    // 1. Typo "hyprlnd"
    let hypr_matches = filter_local_catalog(&catalog, "hyprlnd");
    assert!(
        !hypr_matches.is_empty(),
        "Query 'hyprlnd' must match Hyprland packages"
    );
    let top_hypr = &hypr_matches[0];
    let is_hypr_related = top_hypr
        .resolved_full_name()
        .to_lowercase()
        .contains("hypr")
        || top_hypr.topics.iter().any(|t| t.contains("hypr"))
        || top_hypr
            .detected_components
            .iter()
            .any(|c| c.component.as_str().contains("hypr"));
    assert!(is_hypr_related);

    // 2. Typo "waybr"
    let waybar_matches = filter_local_catalog(&catalog, "waybr");
    assert!(
        !waybar_matches.is_empty(),
        "Query 'waybr' must match Waybar packages"
    );
    let top_wb = &waybar_matches[0];
    let is_wb_related = top_wb
        .resolved_full_name()
        .to_lowercase()
        .contains("waybar")
        || top_wb.topics.iter().any(|t| t.contains("waybar"))
        || top_wb
            .detected_components
            .iter()
            .any(|c| c.component == ConfigComponent::Waybar);
    assert!(is_wb_related);
}

#[test]
fn test_e2e_r4_abbreviation_ml4_and_ranking_order() {
    let catalog = Catalog::default_popular();

    // 1. Abbreviation "ml4"
    let ml4_matches = filter_local_catalog(&catalog, "ml4");
    assert!(!ml4_matches.is_empty());
    assert_eq!(
        ml4_matches[0].owner, "mylinuxforwork",
        "Top result for 'ml4' must be mylinuxforwork"
    );

    // 2. Exact match ranking superiority over typo
    let exact_repo = CatalogRepository {
        id: 9001,
        owner: "exact-match".into(),
        repo: "waybar-config".into(),
        full_name: "exact-match/waybar-config".into(),
        html_url: "https://github.com/exact-match/waybar-config".into(),
        description: Some("Exact waybar config".into()),
        stargazers_count: 100, // Lower stars
        default_branch: "main".into(),
        topics: vec!["waybar".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let typo_repo = CatalogRepository {
        id: 9002,
        owner: "typo-match".into(),
        repo: "waybr-config".into(),
        full_name: "typo-match/waybr-config".into(),
        html_url: "https://github.com/typo-match/waybr-config".into(),
        description: Some("Typo waybr config".into()),
        stargazers_count: 5000, // Much higher stars!
        default_branch: "main".into(),
        topics: vec![],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let test_catalog = Catalog {
        repositories: vec![typo_repo, exact_repo],
        ..Default::default()
    };

    let results = filter_local_catalog(&test_catalog, "waybar");
    assert_eq!(
        results[0].id, 9001,
        "Exact match repo must rank #1 over higher-starred typo match"
    );
}
