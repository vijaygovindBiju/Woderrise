//! Two-tier search engine (Tier 1 instant local catalog filter + Tier 2 remote GitHub Search API).

use serde::{Deserialize, Serialize};
use tracing::debug;
use woderrise_core::models::{Catalog, CatalogRepository};

/// Response payload from GitHub Search Repositories API (`/search/repositories`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchRepositoriesResponse {
    pub total_count: u64,
    #[serde(default)]
    pub incomplete_results: bool,
    pub items: Vec<CatalogRepository>,
}

/// Instant in-memory search over the local `Catalog` (Tier 1).
///
/// Filters the provided catalog by tokenizing the query string and matching
/// each token case-insensitively across:
/// - Repository full name (`owner/repo`)
/// - Description text
/// - Topic tags
/// - Detected component names and display names
///
/// Consumes 0ms latency, 0 network requests, and 0 rate limit quota.
pub fn filter_local_catalog(catalog: &Catalog, query: &str) -> Vec<CatalogRepository> {
    let query_trimmed = query.trim();
    if query_trimmed.is_empty() {
        return catalog.repositories.clone();
    }

    let tokens: Vec<String> = query_trimmed
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .collect();

    let mut matched = Vec::new();

    for repo in &catalog.repositories {
        let full_name_lower = repo.resolved_full_name().to_lowercase();
        let desc_lower = repo
            .description
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        let topics_lower: Vec<String> = repo.topics.iter().map(|t| t.to_lowercase()).collect();
        let components_lower: Vec<String> = repo
            .detected_components
            .iter()
            .flat_map(|c| {
                vec![
                    c.component.as_str().to_lowercase(),
                    c.component.display_name().to_lowercase(),
                ]
            })
            .collect();

        let all_match = tokens.iter().all(|token| {
            full_name_lower.contains(token)
                || desc_lower.contains(token)
                || topics_lower.iter().any(|t| t.contains(token))
                || components_lower.iter().any(|c| c.contains(token))
        });

        if all_match {
            matched.push(repo.clone());
        }
    }

    debug!(
        query = query_trimmed,
        matches = matched.len(),
        "Tier 1 local catalog search completed"
    );
    matched
}

/// Helper to enrich a user search query for Hyprland/Wayland dotfile discovery.
///
/// If the query doesn't already contain explicit topic qualifiers, appends
/// topic filters targeting Wayland and Hyprland configurations.
pub fn build_enriched_search_query(user_query: &str) -> String {
    let trimmed = user_query.trim();
    if trimmed.is_empty() {
        return "topic:hyprland-dotfiles topic:hyprland".to_string();
    }

    if trimmed.contains("topic:") {
        trimmed.to_string()
    } else {
        format!("{} topic:hyprland", trimmed)
    }
}
