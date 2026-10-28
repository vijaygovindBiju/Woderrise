//! Core GitHub client with User-Agent, custom base URLs, timeouts, and quota tracking.

use crate::cache::GitHubCache;
use crate::cdn::{fetch_blob_rest, max_bytes_for_path, stream_raw_cdn};
use crate::rate_limit::{RateLimitResource, RateLimitStatus, RateLimitTracker};
use crate::search::{
    build_enriched_search_query, deduplicate_and_sort_repositories, filter_local_catalog,
    SearchRepositoriesResponse,
};
use crate::tree::{GitTree, GitTreeEntry, MAX_TREE_ENTRIES};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};
use woderrise_core::error::{Result, WoderriseError};
use woderrise_core::models::{Catalog, CatalogRepository};

/// Default GitHub REST API base URL.
pub const DEFAULT_GITHUB_API_BASE_URL: &str = "https://api.github.com";

/// Default Fastly CDN raw blob base URL.
pub const DEFAULT_CDN_BASE_URL: &str = "https://raw.githubusercontent.com";

/// Client configuration.
#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub base_url: String,
    pub cdn_base_url: String,
    pub auth_token: Option<String>,
    pub user_agent: String,
    pub timeout: Duration,
    pub cache_ttl: Duration,
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_GITHUB_API_BASE_URL.to_string(),
            cdn_base_url: DEFAULT_CDN_BASE_URL.to_string(),
            auth_token: None,
            user_agent: format!("Woderrise/{}", env!("CARGO_PKG_VERSION")),
            timeout: Duration::from_secs(30),
            cache_ttl: Duration::from_secs(600),
        }
    }
}

/// Builder for constructing a configured `GitHubClient`.
#[derive(Debug, Default)]
pub struct GitHubClientBuilder {
    config: GitHubConfig,
}

impl GitHubClientBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.config.base_url = url.into();
        self
    }

    pub fn cdn_base_url(mut self, url: impl Into<String>) -> Self {
        self.config.cdn_base_url = url.into();
        self
    }

    pub fn auth_token(mut self, token: Option<String>) -> Self {
        self.config.auth_token = token;
        self
    }

    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.config.user_agent = ua.into();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    pub fn cache_ttl(mut self, ttl: Duration) -> Self {
        self.config.cache_ttl = ttl;
        self
    }

    pub fn build(self) -> Result<GitHubClient> {
        GitHubClient::new(self.config)
    }
}

/// Asynchronous GitHub API and Fastly CDN client.
#[derive(Clone)]
pub struct GitHubClient {
    config: GitHubConfig,
    http: Client,
    rate_limiter: Arc<RateLimitTracker>,
    cache: Arc<GitHubCache>,
}

impl GitHubClient {
    /// Create a new GitHub client with the given configuration.
    pub fn new(config: GitHubConfig) -> Result<Self> {
        let mut default_headers = HeaderMap::new();
        default_headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&config.user_agent).map_err(|e| {
                WoderriseError::Validation(format!("Invalid User-Agent header: {}", e))
            })?,
        );
        default_headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        default_headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );

        let http = Client::builder()
            .default_headers(default_headers)
            .timeout(config.timeout)
            .build()
            .map_err(|e| WoderriseError::GitHubApi {
                status: 500,
                message: format!("Failed to initialize HTTP client: {}", e),
                reset_epoch: None,
            })?;

        let authenticated = config.auth_token.is_some();
        let rate_limiter = Arc::new(RateLimitTracker::with_auth(authenticated));
        let cache = Arc::new(GitHubCache::with_ttl(config.cache_ttl));

        Ok(Self {
            config,
            http,
            rate_limiter,
            cache,
        })
    }

    /// Builder for ergonomic configuration.
    pub fn builder() -> GitHubClientBuilder {
        GitHubClientBuilder::new()
    }

    /// Access rate limiter.
    pub fn rate_limiter(&self) -> &RateLimitTracker {
        &self.rate_limiter
    }

    /// Access multi-tier cache.
    pub fn cache(&self) -> &GitHubCache {
        &self.cache
    }

    /// Access current client configuration.
    pub fn config(&self) -> &GitHubConfig {
        &self.config
    }

    /// Dynamically update the Personal Access Token and adjust rate limits proportionally.
    pub fn set_auth_token(&mut self, token: Option<String>) {
        let authenticated = token.is_some();
        self.config.auth_token = token;
        self.rate_limiter.set_authenticated(authenticated);
    }

    /// Query GitHub's GET /rate_limit endpoint to fetch live rate limit allocations (consumes 0 API quota).
    pub async fn get_rate_limit(&self) -> Result<(RateLimitStatus, RateLimitStatus)> {
        let url = format!("{}/rate_limit", self.config.base_url.trim_end_matches('/'));
        let mut req = self.http.get(&url);
        if let Some(token) = &self.config.auth_token {
            req = req.bearer_auth(token);
        }

        let res = req.send().await.map_err(|e| WoderriseError::GitHubApi {
            status: 500,
            message: format!("Rate limit request error: {}", e),
            reset_epoch: None,
        })?;

        self.rate_limiter
            .update_from_headers(res.headers(), RateLimitResource::Core);

        let status = res.status();
        if status.is_success() {
            if let Ok(body) = res
                .json::<crate::rate_limit::GitHubRateLimitResponse>()
                .await
            {
                self.rate_limiter.update_from_response(&body);
            }
        } else if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
        {
            let error_body = res.text().await.unwrap_or_default();
            return Err(WoderriseError::GitHubApi {
                status: status.as_u16(),
                message: format!("GitHub authentication error: {}", error_body),
                reset_epoch: None,
            });
        }

        Ok((
            self.rate_limiter.core_status(),
            self.rate_limiter.search_status(),
        ))
    }

    /// Fetch a recursive Git Tree (`GET /repos/{owner}/{repo}/git/trees/{tree_sha}?recursive=1`).
    ///
    /// Multi-tier flow:
    /// 1. Check in-memory Moka cache (10-minute TTL). If hit, returns immediately (0 network, 0 quota).
    /// 2. If expired or missing, check core quota and send request with `If-None-Match: <etag>`.
    /// 3. Handle `HTTP 304 Not Modified`, updating rate limit headers and refreshing cache TTL.
    /// 4. On `HTTP 200 OK`, parse tree entries and handle `truncated: true` if needed.
    pub async fn get_tree(&self, owner: &str, repo: &str, tree_sha: &str) -> Result<GitTree> {
        let key = GitHubCache::tree_key(owner, repo, tree_sha);

        // 1. In-memory TTL RAM cache check
        if let Some(cached_tree) = self.cache.get_tree_if_active(&key).await {
            return Ok(cached_tree);
        }

        // 2. Pre-flight rate limit check
        self.rate_limiter.check_core_quota()?;

        let url = format!(
            "{}/repos/{}/{}/git/trees/{}?recursive=1",
            self.config.base_url.trim_end_matches('/'),
            owner,
            repo,
            tree_sha
        );

        debug!(url = %url, "Requesting recursive Git Tree from GitHub API");

        let mut req = self.http.get(&url);
        if let Some(token) = &self.config.auth_token {
            req = req.bearer_auth(token);
        }

        // Attach ETag for conditional revalidation if previously cached
        if let Some(etag) = self.cache.get_etag(&key) {
            req = req.header("If-None-Match", etag);
        }

        let res = req.send().await.map_err(|e| WoderriseError::GitHubApi {
            status: 500,
            message: format!("Tree request error: {}", e),
            reset_epoch: None,
        })?;

        // Update rate limits from headers
        self.rate_limiter
            .update_from_headers(res.headers(), RateLimitResource::Core);

        let status = res.status();

        // 3. HTTP 304 Not Modified
        if status == reqwest::StatusCode::NOT_MODIFIED {
            if let Some((stored_tree, _)) = self.cache.get_stored_tree(&key) {
                debug!(key = %key, "Refreshed cache TTL on HTTP 304 Not Modified");
                self.cache.refresh_ttl(&key).await;
                return Ok(stored_tree);
            }
        }

        // 4. Rate limit exhaustion HTTP 403 / 429
        if (status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS)
            && self.rate_limiter.core_status().is_exhausted()
        {
            let reset_time = self.rate_limiter.core_status().reset_time;
            return Err(WoderriseError::RateLimitExceeded { reset_time });
        }

        if !status.is_success() {
            return Err(WoderriseError::GitHubApi {
                status: status.as_u16(),
                message: format!("GitHub API returned HTTP status {}", status),
                reset_epoch: None,
            });
        }

        let etag = res
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let mut git_tree: GitTree = res.json().await.map_err(|e| WoderriseError::GitHubApi {
            status: 500,
            message: format!("Failed to parse GitTree JSON: {}", e),
            reset_epoch: None,
        })?;

        // Truncated tree handling: resolve targeted subtrees
        if git_tree.truncated {
            warn!(
                owner = owner,
                repo = repo,
                tree_sha = tree_sha,
                "GitHub API truncated Git Tree. Resolving unexpanded subtrees."
            );
            self.resolve_truncated_subtrees(owner, repo, &mut git_tree, 1)
                .await?;
        }

        if git_tree.tree.len() > MAX_TREE_ENTRIES {
            warn!(
                entries = git_tree.tree.len(),
                max = MAX_TREE_ENTRIES,
                "Tree entry count exceeded maximum allowable threshold"
            );
            return Err(WoderriseError::Validation(format!(
                "Repository tree contains {} entries, exceeding maximum allowable limit of {}",
                git_tree.tree.len(),
                MAX_TREE_ENTRIES
            )));
        }

        // Put into cache
        self.cache.put_tree(key, git_tree.clone(), etag).await;

        Ok(git_tree)
    }

    /// Resolve unexpanded subtrees for truncated trees up to max depth.
    async fn resolve_truncated_subtrees(
        &self,
        owner: &str,
        repo: &str,
        parent_tree: &mut GitTree,
        depth: usize,
    ) -> Result<()> {
        if depth > 2 {
            return Ok(());
        }

        let unexpanded: Vec<GitTreeEntry> = parent_tree
            .tree
            .iter()
            .filter(|e| e.is_tree())
            .cloned()
            .collect();

        for entry in unexpanded {
            let lower_path = entry.path.to_lowercase();
            // Focus on dotfile relevant directories
            let is_dotfile_candidate = lower_path.contains("hypr")
                || lower_path.contains("waybar")
                || lower_path.contains("config")
                || lower_path.contains("dots")
                || lower_path.contains("wallpapers")
                || lower_path.contains("icons")
                || lower_path.contains("gtk");

            if is_dotfile_candidate {
                let subtree_url = format!(
                    "{}/repos/{}/{}/git/trees/{}?recursive=1",
                    self.config.base_url.trim_end_matches('/'),
                    owner,
                    repo,
                    entry.sha
                );

                debug!(
                    path = %entry.path,
                    url = %subtree_url,
                    "Fetching targeted subtree for truncated directory"
                );

                let mut req = self.http.get(&subtree_url);
                if let Some(token) = &self.config.auth_token {
                    req = req.bearer_auth(token);
                }

                if let Ok(res) = req.send().await {
                    self.rate_limiter
                        .update_from_headers(res.headers(), RateLimitResource::Core);

                    if res.status().is_success() {
                        if let Ok(subtree) = res.json::<GitTree>().await {
                            for sub_entry in subtree.tree {
                                let mut merged_entry = sub_entry;
                                merged_entry.path = format!("{}/{}", entry.path, merged_entry.path);
                                if !parent_tree.tree.iter().any(|e| e.path == merged_entry.path) {
                                    parent_tree.tree.push(merged_entry);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Stream a dotfile raw blob directly from Fastly CDN.
    ///
    /// Consumes 0 REST API rate limit quota.
    /// Falls back to the GitHub REST Blobs endpoint if CDN returns 404 and `blob_sha` is available.
    pub async fn stream_blob(
        &self,
        owner: &str,
        repo: &str,
        commit_sha: &str,
        path: &str,
        blob_sha: Option<&str>,
    ) -> Result<(Vec<u8>, String)> {
        let max_bytes = max_bytes_for_path(path);

        // Try CDN first
        match stream_raw_cdn(
            &self.http,
            &self.config.cdn_base_url,
            owner,
            repo,
            commit_sha,
            path,
            max_bytes,
        )
        .await
        {
            Ok(result) => Ok(result),
            Err(e) => {
                // If 404 and blob_sha provided, fallback to REST
                let is_not_found = match &e {
                    WoderriseError::GitHubApi { status, .. } => *status == 404,
                    _ => false,
                };

                if is_not_found {
                    if let Some(sha) = blob_sha {
                        warn!(
                            path = path,
                            blob_sha = sha,
                            "Fastly CDN returned 404. Falling back to GitHub REST API blob endpoint"
                        );
                        self.rate_limiter.check_core_quota()?;
                        let mut headers = HeaderMap::new();
                        let res = fetch_blob_rest(
                            &self.http,
                            &self.config.base_url,
                            self.config.auth_token.as_deref(),
                            Some(&mut headers),
                            owner,
                            repo,
                            sha,
                            max_bytes,
                            path,
                        )
                        .await;
                        self.rate_limiter
                            .update_from_headers(&headers, RateLimitResource::Core);
                        return res;
                    }
                }
                Err(e)
            }
        }
    }

    /// Search GitHub repositories (`GET /search/repositories`).
    ///
    /// Governed by independent `X-RateLimit-Resource: search` pool.
    /// Checks in-memory cache and throttles when remaining <= 2.
    pub async fn search_repositories(&self, query: &str) -> Result<SearchRepositoriesResponse> {
        // Check search cache
        if let Some(cached) = self.cache.get_search(query).await {
            return Ok(cached);
        }

        // Check search quota
        self.rate_limiter.check_search_quota()?;

        let enriched = build_enriched_search_query(query);
        let url = format!(
            "{}/search/repositories",
            self.config.base_url.trim_end_matches('/')
        );

        debug!(url = %url, query = %enriched, "Querying GitHub Search API");

        let mut req = self.http.get(&url).query(&[
            ("q", enriched.as_str()),
            ("sort", "stars"),
            ("order", "desc"),
            ("per_page", "30"),
        ]);
        if let Some(token) = &self.config.auth_token {
            req = req.bearer_auth(token);
        }

        let res = req.send().await.map_err(|e| WoderriseError::GitHubApi {
            status: 500,
            message: format!("Search request error: {}", e),
            reset_epoch: None,
        })?;

        self.rate_limiter
            .update_from_headers(res.headers(), RateLimitResource::Search);

        let status = res.status();
        if (status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS)
            && self.rate_limiter.search_status().is_exhausted()
        {
            let reset_time = self.rate_limiter.search_status().reset_time;
            return Err(WoderriseError::RateLimitExceeded { reset_time });
        }

        if !status.is_success() {
            return Err(WoderriseError::GitHubApi {
                status: status.as_u16(),
                message: format!("GitHub Search API returned HTTP status {}", status),
                reset_epoch: None,
            });
        }

        let body_text = res.text().await.map_err(|e| WoderriseError::GitHubApi {
            status: 500,
            message: format!("Failed to read search response body: {}", e),
            reset_epoch: None,
        })?;

        let response: SearchRepositoriesResponse =
            serde_json::from_str(&body_text).map_err(|e| {
                let preview = if body_text.len() > 200 {
                    format!("{}...", &body_text[..200])
                } else {
                    body_text.clone()
                };
                warn!(error = %e, preview = %preview, "Failed to parse search response JSON");
                WoderriseError::GitHubApi {
                    status: 500,
                    message: format!("Failed to parse search response JSON: {}", e),
                    reset_epoch: None,
                }
            })?;

        self.cache
            .put_search(query.to_string(), response.clone())
            .await;

        Ok(response)
    }

    /// Two-tier search:
    /// Filters local catalog first (Tier 1). If local matches >= 3 or remote quota is low, returns local matches.
    /// Otherwise queries remote GitHub Search API (Tier 2) and merges results.
    pub async fn search_two_tier(
        &self,
        catalog: Option<&Catalog>,
        query: &str,
    ) -> Result<Vec<CatalogRepository>> {
        let mut results = Vec::new();

        if let Some(cat) = catalog {
            let local_matches = filter_local_catalog(cat, query);
            results.extend(local_matches);

            // If local matches are sufficient or query is empty, return Tier 1
            if results.len() >= 3 || query.trim().is_empty() {
                return Ok(results);
            }
        }

        // Try Tier 2 remote search if quota permits
        if self.rate_limiter.check_search_quota().is_ok() {
            if let Ok(remote) = self.search_repositories(query).await {
                for item in remote.items {
                    if !results
                        .iter()
                        .any(|r| r.resolved_full_name() == item.resolved_full_name())
                    {
                        results.push(item);
                    }
                }
            }
        }

        Ok(results)
    }

    /// Aggregates repositories across multiple topics and curation feeds.
    ///
    /// Deduplicates results by repository full name (`owner/repo` lowercase), merges `topics`
    /// vectors across duplicate matches, and sorts descending by `stargazers_count`
    /// (tie-breaking on `pushed_at` descending, then repository `id`).
    ///
    /// Resilient to partial failures: if one topic query fails or rate-limits,
    /// returns the results from the successful queries rather than aborting entirely.
    pub async fn aggregate_topics(&self, topics: &[&str]) -> Result<Vec<CatalogRepository>> {
        if topics.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_repos = Vec::new();
        let mut last_err = None;
        let mut success_count = 0;

        for topic in topics {
            match self.search_repositories(topic).await {
                Ok(resp) => {
                    success_count += 1;
                    all_repos.extend(resp.items);
                }
                Err(e) => {
                    warn!(
                        topic = %topic,
                        error = %e,
                        "Topic query feed failed; continuing with remaining feeds"
                    );
                    last_err = Some(e);
                }
            }
        }

        if success_count == 0 {
            if let Some(err) = last_err {
                return Err(err);
            }
        }

        Ok(deduplicate_and_sort_repositories(all_repos))
    }
}
