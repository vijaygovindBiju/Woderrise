//! E2E Test Suite for R2: Multi-Topic Aggregation & Community Relevance.
//!
//! Verifies:
//! - Aggregation across multiple topic queries (topic:hyprland, topic:waybar, quickshell, etc.)
//! - Repository deduplication by ID
//! - Preservation of topic unions
//! - Strict descending sorting by community stars

use woderrise_core::models::CatalogRepository;
use woderrise_github::search::aggregate_topics;

#[test]
fn test_e2e_r2_multi_topic_aggregation_dedup_and_star_ranking() {
    let repo_a = CatalogRepository {
        id: 101,
        owner: "dev-a".into(),
        repo: "hypr-setup".into(),
        full_name: "dev-a/hypr-setup".into(),
        html_url: "https://github.com/dev-a/hypr-setup".into(),
        description: None,
        stargazers_count: 350,
        default_branch: "main".into(),
        topics: vec!["hyprland".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let repo_b_query1 = CatalogRepository {
        id: 102,
        owner: "dev-b".into(),
        repo: "super-desktop".into(),
        full_name: "dev-b/super-desktop".into(),
        html_url: "https://github.com/dev-b/super-desktop".into(),
        description: None,
        stargazers_count: 2400,
        default_branch: "main".into(),
        topics: vec!["hyprland".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let repo_b_query2 = CatalogRepository {
        id: 102,
        owner: "dev-b".into(),
        repo: "super-desktop".into(),
        full_name: "dev-b/super-desktop".into(),
        html_url: "https://github.com/dev-b/super-desktop".into(),
        description: None,
        stargazers_count: 2400,
        default_branch: "main".into(),
        topics: vec!["waybar".into(), "quickshell".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let repo_c = CatalogRepository {
        id: 103,
        owner: "dev-c".into(),
        repo: "quickshell-widgets".into(),
        full_name: "dev-c/quickshell-widgets".into(),
        html_url: "https://github.com/dev-c/quickshell-widgets".into(),
        description: None,
        stargazers_count: 1100,
        default_branch: "main".into(),
        topics: vec!["quickshell".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: vec![],
    };

    let queries = vec![vec![repo_a, repo_b_query1], vec![repo_b_query2, repo_c]];

    let results = aggregate_topics(queries);

    // 1. Deduplication: repo 102 must only appear once
    assert_eq!(results.len(), 3, "Total deduplicated repos must be 3");

    // 2. Star ranking: 2400 (repo 102) -> 1100 (repo 103) -> 350 (repo 101)
    assert_eq!(results[0].id, 102);
    assert_eq!(results[0].stargazers_count, 2400);

    assert_eq!(results[1].id, 103);
    assert_eq!(results[1].stargazers_count, 1100);

    assert_eq!(results[2].id, 101);
    assert_eq!(results[2].stargazers_count, 350);

    // 3. Topic union: repo 102 must contain topics from both queries
    let deduplicated_b = &results[0];
    assert!(deduplicated_b.topics.contains(&"hyprland".to_string()));
    assert!(deduplicated_b.topics.contains(&"waybar".to_string()));
    assert!(deduplicated_b.topics.contains(&"quickshell".to_string()));
}
