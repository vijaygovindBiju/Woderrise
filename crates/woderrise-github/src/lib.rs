//! GitHub API discovery, Fastly CDN streaming, and caching for Woderrise.
//!
//! Provides an efficient, selective data ingestion pipeline that fetches only targeted
//! configuration files from GitHub's Git Trees API and Fastly CDN without full repository cloning.

pub mod cache;
pub mod cdn;
pub mod client;
pub mod rate_limit;
pub mod search;
pub mod tree;

pub use cache::{GitHubCache, DEFAULT_CACHE_TTL};
pub use cdn::{
    fetch_blob_rest, max_bytes_for_path, stream_raw_cdn, MAX_CONFIG_BYTES, MAX_WALLPAPER_BYTES,
};
pub use client::{
    GitHubClient, GitHubClientBuilder, GitHubConfig, DEFAULT_CDN_BASE_URL,
    DEFAULT_GITHUB_API_BASE_URL,
};
pub use rate_limit::{RateLimitResource, RateLimitStatus, RateLimitTracker};
pub use search::{
    aggregate_topics, build_enriched_search_query, deduplicate_and_sort_repositories,
    fetch_static_catalog, filter_local_catalog, score_repository, SearchRepositoriesResponse,
    DEFAULT_CATALOG_FEED_URL, DEFAULT_CURATION_TOPICS,
};
pub use tree::{
    GitTree, GitTreeEntry, MAX_TREE_ENTRIES, MODE_DIRECTORY, MODE_EXECUTABLE, MODE_REGULAR_FILE,
    MODE_SYMLINK,
};

/// Returns the crate version string.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
