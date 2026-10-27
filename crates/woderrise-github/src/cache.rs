//! Multi-layer in-memory TTL caching with ETag support.

use crate::search::SearchRepositoriesResponse;
use crate::tree::GitTree;
use moka::future::Cache;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::debug;

/// Default time-to-live for cached responses (10 minutes).
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(600);
pub const DEFAULT_MAX_CAPACITY: u64 = 1_000;

type TreeStore = Arc<RwLock<HashMap<String, (GitTree, Option<String>)>>>;

/// Multi-tier in-memory cache shielding the application from rate-limit decrements
/// on redundant tab navigations or repeated queries.
#[derive(Clone)]
pub struct GitHubCache {
    /// Tracks TTL expiration window (10 minutes default).
    ttl_cache: Cache<String, ()>,
    /// Persistent in-memory store for trees and their ETags across revalidations.
    tree_store: TreeStore,
    /// Search results cache.
    search_cache: Cache<String, SearchRepositoriesResponse>,
    ttl: Duration,
}

impl GitHubCache {
    /// Create a new cache with default 10-minute TTL.
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_CACHE_TTL)
    }

    /// Create a new cache with custom TTL.
    pub fn with_ttl(ttl: Duration) -> Self {
        let ttl_cache = Cache::builder()
            .time_to_live(ttl)
            .max_capacity(DEFAULT_MAX_CAPACITY)
            .build();

        let search_cache = Cache::builder()
            .time_to_live(ttl)
            .max_capacity(DEFAULT_MAX_CAPACITY)
            .build();

        Self {
            ttl_cache,
            tree_store: Arc::new(RwLock::new(HashMap::new())),
            search_cache,
            ttl,
        }
    }

    /// Cache key for Git Trees.
    pub fn tree_key(owner: &str, repo: &str, tree_sha: &str) -> String {
        format!("{}/{}/tree/{}", owner, repo, tree_sha)
    }

    /// Check if the TTL window is currently active for this key.
    pub async fn is_ttl_active(&self, key: &str) -> bool {
        self.ttl_cache.get(key).await.is_some()
    }

    /// Retrieve a cached GitTree if the 10-minute TTL window is active.
    ///
    /// Consumes 0 network requests and 0 rate limit quota points.
    pub async fn get_tree_if_active(&self, key: &str) -> Option<GitTree> {
        if self.is_ttl_active(key).await {
            if let Ok(store) = self.tree_store.read() {
                if let Some((tree, _)) = store.get(key) {
                    debug!(
                        key = key,
                        "Git tree RAM cache hit within TTL (0 quota used)"
                    );
                    return Some(tree.clone());
                }
            }
        }
        None
    }

    /// Retrieve the stored GitTree and optional ETag regardless of TTL expiration.
    pub fn get_stored_tree(&self, key: &str) -> Option<(GitTree, Option<String>)> {
        self.tree_store.read().ok()?.get(key).cloned()
    }

    /// Retrieve the stored ETag for a given tree key.
    pub fn get_etag(&self, key: &str) -> Option<String> {
        self.tree_store
            .read()
            .ok()?
            .get(key)
            .and_then(|(_, etag)| etag.clone())
    }

    /// Insert or update a GitTree with an optional ETag header, activating the TTL timer.
    pub async fn put_tree(&self, key: String, tree: GitTree, etag: Option<String>) {
        debug!(key = %key, etag = ?etag, "Inserting Git tree into RAM cache and activating TTL");
        if let Ok(mut store) = self.tree_store.write() {
            store.insert(key.clone(), (tree, etag));
        }
        self.ttl_cache.insert(key, ()).await;
    }

    /// Refresh the TTL window for a known cached entry (e.g. after receiving HTTP 304).
    pub async fn refresh_ttl(&self, key: &str) {
        debug!(key = key, "Refreshing TTL window for cached Git tree");
        self.ttl_cache.insert(key.to_string(), ()).await;
    }

    /// Invalidate a specific GitTree cache entry.
    pub async fn invalidate_tree(&self, key: &str) {
        if let Ok(mut store) = self.tree_store.write() {
            store.remove(key);
        }
        self.ttl_cache.invalidate(key).await;
    }

    /// Retrieve cached search results.
    pub async fn get_search(&self, query: &str) -> Option<SearchRepositoriesResponse> {
        let res = self.search_cache.get(query).await;
        if res.is_some() {
            debug!(query = query, "Search RAM cache hit (0 quota used)");
        }
        res
    }

    /// Insert search results.
    pub async fn put_search(&self, query: String, response: SearchRepositoriesResponse) {
        debug!(query = %query, items = response.items.len(), "Inserting search results into RAM cache");
        self.search_cache.insert(query, response).await;
    }

    /// Invalidate all cached entries.
    pub fn invalidate_all(&self) {
        if let Ok(mut store) = self.tree_store.write() {
            store.clear();
        }
        self.ttl_cache.invalidate_all();
        self.search_cache.invalidate_all();
    }

    /// Returns the configured TTL.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }
}

impl Default for GitHubCache {
    fn default() -> Self {
        Self::new()
    }
}
