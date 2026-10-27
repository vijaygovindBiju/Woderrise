//! Staging manifest models for staging/<tx_id>/manifest.json.

use super::component::ConfigComponent;
use camino::Utf8PathBuf;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MANIFEST_SCHEMA: &str = "https://woderrise.dev/schemas/manifest.v1.json";
pub const DEFAULT_MANIFEST_VERSION: u32 = 1;

fn default_manifest_schema() -> Option<String> {
    Some(DEFAULT_MANIFEST_SCHEMA.to_string())
}

fn default_manifest_version() -> u32 {
    DEFAULT_MANIFEST_VERSION
}

fn default_file_mode() -> String {
    "100644".to_string()
}

fn default_file_permissions() -> u32 {
    0o644
}

fn default_true() -> bool {
    true
}

/// Package reference linking component to source repository and commit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageReference {
    #[serde(alias = "source_repo", alias = "source_repository")]
    pub repository: String,
    #[serde(alias = "source_commit", alias = "source_commit_sha")]
    pub commit_sha: String,
    pub component: ConfigComponent,
}

/// A staged file verified during the sandboxed staging phase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StagedFile {
    pub relative_path: Utf8PathBuf,
    #[serde(skip_serializing_if = "Option::is_none", alias = "destination", alias = "target_file", default)]
    pub target_destination: Option<Utf8PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub blob_sha: Option<String>,
    pub file_size: u64,
    pub sha256: String,
    #[serde(default = "default_file_mode")]
    pub file_mode: String,
    #[serde(default = "default_file_permissions")]
    pub permissions: u32,
    #[serde(default)]
    pub is_executable: bool,
    #[serde(default)]
    pub is_symlink: bool,
    #[serde(default = "default_true")]
    pub syntax_valid: bool,
    #[serde(default = "default_true")]
    pub path_safe: bool,
}

/// Security audit outcome for staged package files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityAuditReport {
    pub scripts_detected: usize,
    pub procedural_directives_neutralized: usize,
    pub path_traversal_violations: usize,
    pub symlinks_rejected: usize,
    pub passed: bool,
}

/// Staging manifest generated before atomic file swap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StagingManifest {
    #[serde(rename = "$schema", default = "default_manifest_schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default = "default_manifest_version")]
    pub version: u32,
    #[serde(alias = "id", alias = "tx_id")]
    pub transaction_id: String,
    pub component: ConfigComponent,
    #[serde(alias = "source_repo")]
    pub source_repository: String,
    #[serde(alias = "source_commit")]
    pub source_commit_sha: String,
    pub created_at: DateTime<Utc>,
    pub staging_dir: Utf8PathBuf,
    pub target_dir: Utf8PathBuf,
    #[serde(alias = "staged_files")]
    pub files: Vec<StagedFile>,
    pub security_audit: SecurityAuditReport,
}

impl StagingManifest {
    /// Verify that security audit passed and all staged files adhere to security invariants.
    pub fn validate_security_invariants(&self) -> Result<(), String> {
        if !self.security_audit.passed {
            return Err("Security audit marked as failed".to_string());
        }
        if self.security_audit.symlinks_rejected > 0 || self.security_audit.path_traversal_violations > 0 {
            return Err("Security audit has unresolved violations".to_string());
        }
        for file in &self.files {
            if file.is_symlink {
                return Err(format!("Symlink detected in file: {}", file.relative_path));
            }
            if file.sha256.len() != 64 || !file.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("Invalid SHA-256 for file: {}", file.relative_path));
            }
            if file.relative_path.is_absolute() || file.relative_path.as_str().contains("..") {
                return Err(format!("Path traversal violation in: {}", file.relative_path));
            }
        }
        Ok(())
    }
}
