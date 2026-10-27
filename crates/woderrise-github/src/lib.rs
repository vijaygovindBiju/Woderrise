//! GitHub API client and blob streaming for Woderrise.
//!
//! Provides the Git Trees API consumer, Fastly CDN raw blob streamer,
//! in-memory TTL caching with Moka, and two-tier search integration.

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
