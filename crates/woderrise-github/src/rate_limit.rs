//! Dual rate limit state tracking for GitHub API "core" and "search" resource pools.

use chrono::{DateTime, TimeZone, Utc};
use reqwest::header::HeaderMap;
use serde::Deserialize;
use std::sync::RwLock;
use tracing::{debug, warn};
use woderrise_core::error::{Result, WoderriseError};

/// Partitioned GitHub rate limit resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateLimitResource {
    Core,
    Search,
}

impl RateLimitResource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Search => "search",
        }
    }
}

/// Instantaneous status of a rate limit resource bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitStatus {
    pub resource: RateLimitResource,
    pub limit: u32,
    pub remaining: u32,
    pub reset_time: DateTime<Utc>,
    pub used: u32,
}

impl RateLimitStatus {
    pub fn new_default(resource: RateLimitResource, authenticated: bool) -> Self {
        let (limit, remaining) = match (resource, authenticated) {
            (RateLimitResource::Core, false) => (60, 60),
            (RateLimitResource::Core, true) => (5000, 5000),
            (RateLimitResource::Search, false) => (10, 10),
            (RateLimitResource::Search, true) => (30, 30),
        };
        Self {
            resource,
            limit,
            remaining,
            reset_time: Utc::now() + chrono::Duration::hours(1),
            used: 0,
        }
    }

    /// Whether this resource bucket is currently exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.remaining == 0 && Utc::now() < self.reset_time
    }
}

/// Thread-safe rate limit tracker maintaining independent pools for core and search.
#[derive(Debug)]
pub struct RateLimitTracker {
    core: RwLock<RateLimitStatus>,
    search: RwLock<RateLimitStatus>,
}

impl RateLimitTracker {
    /// Create a new tracker with default unauthenticated limits.
    pub fn new() -> Self {
        Self::with_auth(false)
    }

    /// Create a new tracker specifying whether authentication is active.
    pub fn with_auth(authenticated: bool) -> Self {
        Self {
            core: RwLock::new(RateLimitStatus::new_default(
                RateLimitResource::Core,
                authenticated,
            )),
            search: RwLock::new(RateLimitStatus::new_default(
                RateLimitResource::Search,
                authenticated,
            )),
        }
    }

    /// Returns a snapshot of the current Core rate limit status.
    pub fn core_status(&self) -> RateLimitStatus {
        self.core.read().unwrap().clone()
    }

    /// Returns a snapshot of the current Search rate limit status.
    pub fn search_status(&self) -> RateLimitStatus {
        self.search.read().unwrap().clone()
    }

    /// Returns a snapshot for the requested resource.
    pub fn status(&self, resource: RateLimitResource) -> RateLimitStatus {
        match resource {
            RateLimitResource::Core => self.core_status(),
            RateLimitResource::Search => self.search_status(),
        }
    }

    /// Check if the core pool has remaining quota.
    ///
    /// If remaining is 0 and the current time is before the reset time,
    /// returns `WoderriseError::RateLimitExceeded`.
    pub fn check_core_quota(&self) -> Result<()> {
        let status = self.core_status();
        if status.remaining == 0 && Utc::now() < status.reset_time {
            warn!(
                reset = %status.reset_time,
                "GitHub Core rate limit exhausted"
            );
            return Err(WoderriseError::RateLimitExceeded {
                reset_time: status.reset_time,
            });
        }
        Ok(())
    }

    /// Check if the search pool has remaining quota.
    ///
    /// Proactively throttles when remaining <= 2 to prevent hard API lockout,
    /// returning `WoderriseError::RateLimitExceeded`.
    pub fn check_search_quota(&self) -> Result<()> {
        let status = self.search_status();
        if status.remaining <= 2 && Utc::now() < status.reset_time {
            warn!(
                remaining = status.remaining,
                reset = %status.reset_time,
                "GitHub Search rate limit near exhaustion (remaining <= 2). Proactively throttling."
            );
            return Err(WoderriseError::RateLimitExceeded {
                reset_time: status.reset_time,
            });
        }
        Ok(())
    }

    /// Update rate limit state by parsing response headers.
    pub fn update_from_headers(&self, headers: &HeaderMap, fallback_resource: RateLimitResource) {
        let resource = headers
            .get("x-ratelimit-resource")
            .and_then(|val| val.to_str().ok())
            .map(|r| match r {
                "search" => RateLimitResource::Search,
                _ => RateLimitResource::Core,
            })
            .unwrap_or(fallback_resource);

        let limit = headers
            .get("x-ratelimit-limit")
            .and_then(|val| val.to_str().ok())
            .and_then(|val| val.parse::<u32>().ok());

        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|val| val.to_str().ok())
            .and_then(|val| val.parse::<u32>().ok());

        let reset = headers
            .get("x-ratelimit-reset")
            .and_then(|val| val.to_str().ok())
            .and_then(|val| val.parse::<i64>().ok())
            .and_then(|epoch| Utc.timestamp_opt(epoch, 0).single());

        let used = headers
            .get("x-ratelimit-used")
            .and_then(|val| val.to_str().ok())
            .and_then(|val| val.parse::<u32>().ok());

        let target = match resource {
            RateLimitResource::Core => &self.core,
            RateLimitResource::Search => &self.search,
        };

        if let Ok(mut current) = target.write() {
            if let Some(l) = limit {
                current.limit = l;
            }
            if let Some(r) = remaining {
                current.remaining = r;
            }
            if let Some(t) = reset {
                current.reset_time = t;
            }
            if let Some(u) = used {
                current.used = u;
            }
            debug!(
                resource = resource.as_str(),
                remaining = current.remaining,
                limit = current.limit,
                reset = %current.reset_time,
                "Updated rate limit status"
            );
        }
    }

    /// Dynamically update authentication status and elevate or revert rate limits proportionally.
    pub fn set_authenticated(&self, authenticated: bool) {
        if let Ok(mut core) = self.core.write() {
            let target_limit = if authenticated { 5000 } else { 60 };
            let diff = target_limit as i64 - core.limit as i64;
            core.limit = target_limit;
            core.remaining = (core.remaining as i64 + diff).clamp(0, target_limit as i64) as u32;
        }
        if let Ok(mut search) = self.search.write() {
            let target_limit = if authenticated { 30 } else { 10 };
            let diff = target_limit as i64 - search.limit as i64;
            search.limit = target_limit;
            search.remaining =
                (search.remaining as i64 + diff).clamp(0, target_limit as i64) as u32;
        }
    }

    /// Update rate limits directly from GitHub's GET /rate_limit response JSON.
    pub fn update_from_response(&self, resp: &GitHubRateLimitResponse) {
        if let Ok(mut core) = self.core.write() {
            core.limit = resp.resources.core.limit;
            core.remaining = resp.resources.core.remaining;
            core.used = resp.resources.core.used;
            if let Some(t) = Utc.timestamp_opt(resp.resources.core.reset, 0).single() {
                core.reset_time = t;
            }
        }
        if let Ok(mut search) = self.search.write() {
            search.limit = resp.resources.search.limit;
            search.remaining = resp.resources.search.remaining;
            search.used = resp.resources.search.used;
            if let Some(t) = Utc.timestamp_opt(resp.resources.search.reset, 0).single() {
                search.reset_time = t;
            }
        }
    }
}

/// GitHub API GET /rate_limit response payload.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRateLimitResponse {
    pub resources: GitHubRateLimitResources,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRateLimitResources {
    pub core: GitHubRateLimitItem,
    pub search: GitHubRateLimitItem,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRateLimitItem {
    pub limit: u32,
    pub remaining: u32,
    pub reset: i64,
    pub used: u32,
}

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}
