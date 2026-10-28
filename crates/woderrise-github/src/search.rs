//! Two-tier search engine (Tier 1 instant local catalog filter + Tier 2 remote GitHub Search API).

use serde::{Deserialize, Serialize};
use strsim::{jaro_winkler, normalized_damerau_levenshtein};
use sublime_fuzzy::best_match;
use tracing::debug;
use woderrise_core::error::WoderriseError;
use woderrise_core::models::{Catalog, CatalogRepository};

/// Default raw GitHub CDN URL for the curated static catalog feed.
pub const DEFAULT_CATALOG_FEED_URL: &str =
    "https://raw.githubusercontent.com/vijaygovindBiju/Woderrise/main/registry/catalog.v1.json";

/// Default topics and query feeds for multi-topic catalog curation.
pub const DEFAULT_CURATION_TOPICS: &[&str] = &[
    "topic:hyprland",
    "topic:waybar",
    "quickshell",
    "caelestia-dots",
];

/// Response payload from GitHub Search Repositories API (`/search/repositories`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchRepositoriesResponse {
    pub total_count: u64,
    #[serde(default)]
    pub incomplete_results: bool,
    pub items: Vec<CatalogRepository>,
}

/// Score a repository candidate against a search query.
///
/// Higher scores represent closer relevance. Returns `None` if the repository
/// does not match the query with sufficient confidence.
///
/// Scoring breakdown:
/// - Exact substring / identity matches in name, topics, components, and description (500.0 - 2000.0)
/// - Abbreviation / alias expansion (e.g. `ml4` / `ml4w` -> `mylinuxforwork`, `hl` -> `hyprland`, `wb` -> `waybar`) (700.0 - 950.0)
/// - Typo-tolerance via Jaro-Winkler and Damerau-Levenshtein distances (200.0 - 600.0)
/// - Subsequence matching via `sublime_fuzzy` for acronyms and camelCase tokens
/// - Logarithmic stargazer count tie-breaker bonus (up to 50.0 points)
pub fn score_repository(repo: &CatalogRepository, query: &str) -> Option<f64> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(repo.stargazers_count as f64);
    }

    let tokens: Vec<&str> = q.split_whitespace().take(16).collect();
    let full_name_lower = repo.resolved_full_name().to_lowercase();
    let owner_lower = repo.resolved_owner().to_lowercase();
    let repo_lower = repo.resolved_repo().to_lowercase();
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

    // Collect candidate words for word-level typo distance evaluation
    let mut candidate_words: Vec<&str> = full_name_lower
        .split(&['/', '-', '_', '.', ' '][..])
        .filter(|w| !w.is_empty())
        .collect();
    for t in &topics_lower {
        for w in t.split(&['-', '_', ' '][..]).filter(|w| !w.is_empty()) {
            candidate_words.push(w);
        }
    }
    for c in &components_lower {
        for w in c.split(&['-', '_', ' '][..]).filter(|w| !w.is_empty()) {
            candidate_words.push(w);
        }
    }
    for w in desc_lower
        .split(&['/', '-', '_', ' '][..])
        .filter(|w| !w.is_empty())
        .take(64)
    {
        let clean =
            w.trim_matches(&['(', ')', '[', ']', '{', '}', ',', '.', '!', '?', '"', '\''][..]);
        if clean.len() >= 3 {
            candidate_words.push(clean);
        }
    }

    let mut total_score: f64 = 0.0;

    for token in &tokens {
        let mut token_score: f64 = 0.0;

        // 1. Exact Substring / Equality Matches (Highest Priority: 500.0 - 2000.0)
        if full_name_lower == *token {
            token_score = token_score.max(2000.0);
        } else if repo_lower == *token {
            token_score = token_score.max(1800.0);
        } else if owner_lower == *token {
            token_score = token_score.max(1600.0);
        } else if full_name_lower.contains(token) {
            token_score = token_score.max(1200.0);
        }

        if topics_lower.iter().any(|t| t == *token) {
            token_score = token_score.max(1000.0);
        } else if topics_lower.iter().any(|t| t.contains(token)) {
            token_score = token_score.max(800.0);
        }

        if components_lower.iter().any(|c| c == *token) {
            token_score = token_score.max(900.0);
        } else if components_lower.iter().any(|c| c.contains(token)) {
            token_score = token_score.max(700.0);
        }

        if desc_lower.contains(token) {
            token_score = token_score.max(500.0);
        }

        // 2. Abbreviation / Alias Expansion
        let is_ml4 = (*token == "ml4" || *token == "ml4w")
            && (full_name_lower.contains("mylinuxforwork")
                || desc_lower.contains("ml4w")
                || desc_lower.contains("ml4"));
        let is_hl = (*token == "hl" || *token == "hypr")
            && (full_name_lower.contains("hyprland")
                || topics_lower.iter().any(|t| t == "hyprland")
                || components_lower.iter().any(|c| c == "hyprland"));
        let is_wb = *token == "wb"
            && (full_name_lower.contains("waybar")
                || topics_lower.iter().any(|t| t == "waybar")
                || components_lower.iter().any(|c| c == "waybar"));
        let is_dots = *token == "dots"
            && (full_name_lower.contains("dotfiles")
                || full_name_lower.contains("dots")
                || topics_lower.iter().any(|t| t == "dotfiles" || t == "dots"));

        if is_ml4 {
            token_score = token_score.max(950.0);
        }
        if is_hl || is_wb {
            token_score = token_score.max(900.0);
        }
        if is_dots {
            token_score = token_score.max(700.0);
        }

        // 3. Typo-Tolerance via Jaro-Winkler & Normalized Levenshtein (200.0 - 600.0)
        if token.len() >= 3 && token.len() <= 64 {
            let mut best_sim = 0.0f64;
            for word in &candidate_words {
                if (word.len() as isize - token.len() as isize).abs() <= 3 {
                    let jw = jaro_winkler(token, word);
                    let ndl = normalized_damerau_levenshtein(token, word);
                    let sim = jw.max(ndl);
                    if sim > best_sim {
                        best_sim = sim;
                    }
                }
            }
            if best_sim >= 0.82 {
                token_score = token_score.max(best_sim * 600.0);
            }
        }

        // 4. Subsequence Matching via sublime_fuzzy
        if token.len() >= 3 && token.len() <= 32 {
            if token.len() <= full_name_lower.len() {
                if let Some(m) = best_match(token, &full_name_lower) {
                    if m.score() > 0 {
                        token_score = token_score.max((m.score() as f64).min(50.0) * 8.0);
                    }
                }
            }
            // Only run sublime_fuzzy on the first 256 characters of description
            let desc_slice = if desc_lower.len() > 256 {
                let end = desc_lower
                    .char_indices()
                    .map(|(i, _)| i)
                    .nth(256)
                    .unwrap_or(desc_lower.len());
                &desc_lower[..end]
            } else {
                &desc_lower[..]
            };
            if token.len() <= desc_slice.len() {
                if let Some(m) = best_match(token, desc_slice) {
                    if m.score() > 0 {
                        token_score = token_score.max((m.score() as f64).min(50.0) * 4.0);
                    }
                }
            }
        }

        if token_score <= 0.0 {
            return None;
        }

        total_score += token_score;
    }

    // Stargazer tie-breaker bonus (up to 50.0 points)
    let star_bonus = ((repo.stargazers_count as f64 + 1.0).ln() * 5.0).min(50.0);
    Some(total_score + star_bonus)
}

/// Instant in-memory search over the local `Catalog` (Tier 1).
///
/// Filters and ranks the provided catalog by relevance score using:
/// - Exact substring matching
/// - Abbreviation / community alias recognition
/// - Typo-tolerance via Jaro-Winkler / Damerau-Levenshtein distances
/// - Subsequence matching for acronyms
/// - Stargazer count tie-breaker bonus
///
/// If query is empty or whitespace-only, returns all repositories sorted
/// descending by stargazer count.
///
/// Consumes 0ms latency, 0 network requests, and 0 rate limit quota.
pub fn filter_local_catalog(catalog: &Catalog, query: &str) -> Vec<CatalogRepository> {
    let query_trimmed = query.trim();
    if query_trimmed.is_empty() {
        let mut repos = catalog.repositories.clone();
        repos.sort_by_key(|r| std::cmp::Reverse(r.stargazers_count));
        return repos;
    }

    let mut scored: Vec<(f64, CatalogRepository)> = catalog
        .repositories
        .iter()
        .filter_map(|repo| score_repository(repo, query_trimmed).map(|s| (s, repo.clone())))
        .collect();

    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.1.stargazers_count.cmp(&a.1.stargazers_count))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    debug!(
        query = query_trimmed,
        matches = scored.len(),
        "Tier 1 fuzzy local catalog search completed"
    );

    scored.into_iter().map(|(_, repo)| repo).collect()
}

/// Helper to enrich a user search query for Hyprland/Wayland dotfile discovery.
///
/// If the query is empty, defaults to searching for popular Hyprland dotfile repositories.
/// If the user provides a query, uses the user's query directly.
pub fn build_enriched_search_query(user_query: &str) -> String {
    let trimmed = user_query.trim();
    if trimmed.is_empty() {
        "hyprland dotfiles".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Fetches a static `Catalog` feed from a CDN or raw URL without consuming GitHub REST or Search API rate limits.
pub async fn fetch_static_catalog(
    url: &str,
    timeout: std::time::Duration,
) -> Result<Catalog, WoderriseError> {
    let client = reqwest::Client::builder()
        .user_agent("woderrise-desktop")
        .build()
        .map_err(|e| WoderriseError::Validation(e.to_string()))?;

    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| WoderriseError::Validation(format!("CDN request failed: {}", e)))?;

    if !resp.status().is_success() {
        return Err(WoderriseError::GitHubApi {
            status: resp.status().as_u16(),
            message: format!("CDN feed returned HTTP status {}", resp.status()),
            reset_epoch: None,
        });
    }

    let text = resp
        .text()
        .await
        .map_err(|e| WoderriseError::Validation(format!("Failed to read response body: {}", e)))?;

    let catalog = serde_json::from_str::<Catalog>(&text).map_err(|e| {
        WoderriseError::Serialization(woderrise_core::error::SerializationError::JsonDeserialize {
            path: None,
            source: e,
        })
    })?;

    Ok(catalog)
}

/// Deduplicates a list of catalog repositories by repository full name (`owner/repo` lowercase),
/// merging topics vectors across duplicates and sorting results descending by `stargazers_count`
/// (with secondary tie-breaking on `pushed_at` descending, then repository `id`).
pub fn deduplicate_and_sort_repositories(repos: Vec<CatalogRepository>) -> Vec<CatalogRepository> {
    let mut map: std::collections::HashMap<String, CatalogRepository> =
        std::collections::HashMap::new();

    for repo in repos {
        let key = {
            let full = repo.resolved_full_name().to_lowercase();
            if full.is_empty() {
                format!("id:{}", repo.id)
            } else {
                full
            }
        };

        match map.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(repo);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let existing = entry.get_mut();
                // Merge topics without duplicates (case-insensitive)
                for topic in repo.topics {
                    if !existing
                        .topics
                        .iter()
                        .any(|t| t.eq_ignore_ascii_case(&topic))
                    {
                        existing.topics.push(topic);
                    }
                }
                // Retain maximum stars
                if repo.stargazers_count > existing.stargazers_count {
                    existing.stargazers_count = repo.stargazers_count;
                }
                // Retain newer pushed_at
                if let Some(new_pushed) = repo.pushed_at {
                    match existing.pushed_at {
                        Some(curr) if new_pushed > curr => existing.pushed_at = Some(new_pushed),
                        None => existing.pushed_at = Some(new_pushed),
                        _ => {}
                    }
                }
                // Retain description if existing is None
                if existing.description.is_none() && repo.description.is_some() {
                    existing.description = repo.description;
                }
                // Merge detected components if existing is empty
                if existing.detected_components.is_empty() && !repo.detected_components.is_empty() {
                    existing.detected_components = repo.detected_components;
                }
            }
        }
    }

    let mut result: Vec<CatalogRepository> = map.into_values().collect();
    result.sort_by(|a, b| {
        b.stargazers_count
            .cmp(&a.stargazers_count)
            .then_with(|| b.pushed_at.cmp(&a.pushed_at))
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| a.resolved_full_name().cmp(&b.resolved_full_name()))
    });

    result
}

/// Aggregates multiple topic search results, deduplicating repositories across topics
/// and sorting them in descending order by community relevance (stargazers count).
pub fn aggregate_topics(topic_results: Vec<Vec<CatalogRepository>>) -> Vec<CatalogRepository> {
    let all: Vec<CatalogRepository> = topic_results.into_iter().flatten().collect();
    deduplicate_and_sort_repositories(all)
}
