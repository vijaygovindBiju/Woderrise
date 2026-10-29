# `woderrise-github`

> High-efficiency GitHub API client, commit-pinned Fastly CDN raw streaming pipeline, in-memory TTL caching, and quota-aware search engine for Woderrise.

[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](../../Cargo.toml)

---

## Overview

Traditional dotfile utilities force users to `git clone` entire multi-gigabyte repositories just to extract a few kilobytes of configuration. `woderrise-github` eliminates cloning overhead by querying GitHub's **Git Trees API** (`recursive=1`) and streaming specific targeted files directly through **commit-pinned Fastly CDN edge endpoints** (`raw.githubusercontent.com`), consuming **0 REST API rate limit quota**.

### Key Architectural Features

- **Zero-Clone Fastly CDN Streaming (`cdn`)**:
  - Fetches targeted files via commit-pinned raw CDN URLs: `https://raw.githubusercontent.com/{owner}/{repo}/{commit_sha}/{path}`.
  - Consumes zero GitHub REST API quota while benefiting from global edge caching.
  - Bounded resource streaming with strict file size caps (`MAX_CONFIG_BYTES = 10 MB`, `MAX_WALLPAPER_BYTES = 50 MB`).
  - Automatic fallback to REST blob retrieval (`/git/blobs/{blob_sha}`) if CDN is blocked or unreachable.
- **Git Trees Resolution (`tree`)**:
  - Resolves entire repository file trees with a single metadata call (`GET /repos/{owner}/{repo}/git/trees/{branch}?recursive=1`).
  - Enforces tree size limits (`MAX_TREE_ENTRIES = 100,000`).
  - Extracts accurate POSIX mode bits (`100644`, `100755`, `120000` symlink detection for CVE-2021-21300 defense) directly from git metadata, bypassing NTFS/FAT mount mode degradation.
- **In-Memory TTL Caching (`cache`)**:
  - Thread-safe caching powered by `moka` with configurable TTL (default 10 minutes).
  - Eliminates redundant network queries on repeated inspects, navigation, and theme browsing.
- **Two-Tier Search & Quota Tracking (`rate_limit`, `search`)**:
  - Dedicated rate limit tracking for both `core` and `search` pools (`X-RateLimit-Resource: search`).
  - Tier 1: Instant local catalog filtering across name, description, and topics with 0ms latency and 0 quota cost.
  - Tier 2: Explicit remote GitHub Search querying when user presses <kbd>Enter</kbd> or clicks search.
  - Typo-tolerant fuzzy search scoring using `sublime_fuzzy` and Jaro-Winkler string similarity (`strsim`).
  - Multi-topic aggregation (`hyprland-dots`, `hyprland-rice`, `waybar-themes`, etc.) with intelligent deduplication and star-based scoring.

---

## Usage Example

```rust
use woderrise_github::{GitHubClient, GitHubClientBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = GitHubClientBuilder::new()
        .user_agent("woderrise-desktop/0.1.0")
        .build()?;

    // Fetch repository tree without cloning
    let tree = client.fetch_tree("prasanthrangan", "hyprdots", "main").await?;
    println!("Found {} files in remote tree", tree.tree.len());

    // Stream a specific config file via Fastly CDN (0 REST quota consumed)
    let content = client
        .fetch_file_content("prasanthrangan", "hyprdots", &tree.sha, "Configs/.config/waybar/config.jsonc")
        .await?;
    println!("Fetched waybar config ({} bytes)", content.len());

    Ok(())
}
```

---

## Testing

Run unit and integration tests for `woderrise-github`:

```bash
cargo test --package woderrise-github
```
