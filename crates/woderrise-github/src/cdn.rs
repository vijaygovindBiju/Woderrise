//! Fastly CDN raw blob streaming with size limits and SHA-256 calculation.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use reqwest::header::HeaderMap;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};
use woderrise_core::error::{Result, SecurityError, WoderriseError};

/// Maximum allowed size for text configuration files (10 MB).
pub const MAX_CONFIG_BYTES: u64 = 10 * 1024 * 1024;

/// Maximum allowed size for wallpaper and graphical image assets (50 MB).
pub const MAX_WALLPAPER_BYTES: u64 = 50 * 1024 * 1024;

/// Determine maximum permitted download size based on file extension.
pub fn max_bytes_for_path(path: &str) -> u64 {
    let lower = path.to_lowercase();
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".svg")
        || lower.ends_with(".bmp")
        || lower.ends_with(".gif")
    {
        MAX_WALLPAPER_BYTES
    } else {
        MAX_CONFIG_BYTES
    }
}

/// Git Blob response payload from GitHub REST API fallback.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitBlobResponse {
    pub sha: String,
    pub size: Option<u64>,
    pub content: String,
    pub encoding: String,
}

/// Stream a raw blob from Fastly CDN (`raw.githubusercontent.com`).
///
/// Bypasses the GitHub REST API and consumes 0 rate limit quota.
/// Enforces streaming size limits and computes SHA-256 on the fly.
pub async fn stream_raw_cdn(
    client: &Client,
    cdn_base_url: &str,
    owner: &str,
    repo: &str,
    commit_sha: &str,
    path: &str,
    max_bytes: u64,
) -> Result<(Vec<u8>, String)> {
    let clean_path = path.trim_start_matches('/');
    let url = format!(
        "{}/{}/{}/{}/{}",
        cdn_base_url.trim_end_matches('/'),
        owner,
        repo,
        commit_sha,
        clean_path
    );

    debug!(url = %url, max_bytes = max_bytes, "Streaming raw dotfile from Fastly CDN");

    let mut response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| WoderriseError::GitHubApi {
            status: 500,
            message: format!("CDN request network error: {}", e),
            reset_epoch: None,
        })?;

    let status = response.status();
    if !status.is_success() {
        return Err(WoderriseError::GitHubApi {
            status: status.as_u16(),
            message: format!("CDN returned HTTP status {}", status),
            reset_epoch: None,
        });
    }

    // Early Content-Length check
    if let Some(content_length) = response.content_length() {
        if content_length > max_bytes {
            warn!(
                content_length = content_length,
                max_bytes = max_bytes,
                path = path,
                "CDN blob Content-Length exceeds allowed limit"
            );
            return Err(WoderriseError::Security(
                SecurityError::ResourceLimitExceeded {
                    resource: path.to_string(),
                    limit_bytes: max_bytes,
                    actual_bytes: content_length,
                },
            ));
        }
    }

    let mut buffer = Vec::new();
    let mut hasher = Sha256::new();
    let mut total_bytes: u64 = 0;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| WoderriseError::GitHubApi {
            status: 500,
            message: format!("CDN chunk streaming error: {}", e),
            reset_epoch: None,
        })?
    {
        total_bytes += chunk.len() as u64;
        if total_bytes > max_bytes {
            warn!(
                total_bytes = total_bytes,
                max_bytes = max_bytes,
                path = path,
                "CDN blob stream aborted: exceeded size limit"
            );
            return Err(WoderriseError::Security(
                SecurityError::ResourceLimitExceeded {
                    resource: path.to_string(),
                    limit_bytes: max_bytes,
                    actual_bytes: total_bytes,
                },
            ));
        }
        hasher.update(&chunk);
        buffer.extend_from_slice(&chunk);
    }

    let sha256_hex = format!("{:x}", hasher.finalize());
    debug!(
        path = path,
        bytes = buffer.len(),
        sha256 = %sha256_hex,
        "Fastly CDN raw streaming completed successfully"
    );

    Ok((buffer, sha256_hex))
}

/// Fallback to GitHub REST API `/repos/{owner}/{repo}/git/blobs/{blob_sha}`.
///
/// Used when Fastly CDN raw returns 404 or when fetching from private repositories.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_blob_rest(
    client: &Client,
    base_url: &str,
    auth_token: Option<&str>,
    headers_out: Option<&mut HeaderMap>,
    owner: &str,
    repo: &str,
    blob_sha: &str,
    max_bytes: u64,
    path_hint: &str,
) -> Result<(Vec<u8>, String)> {
    let url = format!(
        "{}/repos/{}/{}/git/blobs/{}",
        base_url.trim_end_matches('/'),
        owner,
        repo,
        blob_sha
    );

    debug!(url = %url, blob_sha = blob_sha, "Fetching blob via GitHub REST API fallback");

    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");

    if let Some(token) = auth_token {
        req = req.bearer_auth(token);
    }

    let response = req.send().await.map_err(|e| WoderriseError::GitHubApi {
        status: 500,
        message: format!("REST blob request error: {}", e),
        reset_epoch: None,
    })?;

    if let Some(out) = headers_out {
        *out = response.headers().clone();
    }

    let status = response.status();
    if !status.is_success() {
        return Err(WoderriseError::GitHubApi {
            status: status.as_u16(),
            message: format!("GitHub REST blob returned HTTP status {}", status),
            reset_epoch: None,
        });
    }

    let blob: GitBlobResponse = response
        .json()
        .await
        .map_err(|e| WoderriseError::GitHubApi {
            status: 500,
            message: format!("Failed to parse blob JSON: {}", e),
            reset_epoch: None,
        })?;

    if blob.encoding != "base64" {
        return Err(WoderriseError::GitHubApi {
            status: 400,
            message: format!("Unsupported blob encoding: {}", blob.encoding),
            reset_epoch: None,
        });
    }

    let cleaned_content: String = blob
        .content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let decoded = BASE64_STANDARD
        .decode(cleaned_content.as_bytes())
        .map_err(|e| WoderriseError::GitHubApi {
            status: 500,
            message: format!("Base64 decoding failed for blob: {}", e),
            reset_epoch: None,
        })?;

    if decoded.len() as u64 > max_bytes {
        return Err(WoderriseError::Security(
            SecurityError::ResourceLimitExceeded {
                resource: path_hint.to_string(),
                limit_bytes: max_bytes,
                actual_bytes: decoded.len() as u64,
            },
        ));
    }

    let mut hasher = Sha256::new();
    hasher.update(&decoded);
    let sha256_hex = format!("{:x}", hasher.finalize());

    Ok((decoded, sha256_hex))
}
