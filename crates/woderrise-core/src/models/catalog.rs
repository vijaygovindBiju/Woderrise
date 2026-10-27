//! Catalog and repository link models for catalog.json.

use super::component::{ConfigComponent, SyntaxFormat};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CATALOG_SCHEMA: &str = "https://woderrise.dev/schemas/catalog.v1.json";
pub const DEFAULT_CATALOG_VERSION: u32 = 1;

fn default_catalog_schema() -> Option<String> {
    Some(DEFAULT_CATALOG_SCHEMA.to_string())
}

fn default_catalog_version() -> u32 {
    DEFAULT_CATALOG_VERSION
}

fn default_branch_main() -> String {
    "main".to_string()
}

/// Discovered component summary within a repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetectedComponent {
    pub component: ConfigComponent,
    pub syntax_format: SyntaxFormat,
    pub root_path: String,
    pub entrypoint: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub style_file: Option<String>,
    pub file_count: usize,
    pub total_size_bytes: u64,
}

/// Alias for compatibility with differing naming conventions.
pub type CompatibleComponent = DetectedComponent;

/// Discovered repository metadata in catalog.json.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogRepository {
    pub id: u64,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub full_name: String,
    pub html_url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(alias = "stars")]
    pub stargazers_count: u64,
    #[serde(default = "default_branch_main")]
    pub default_branch: String,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(alias = "updated_at", default)]
    pub pushed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cached_tree_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub etag: Option<String>,
    #[serde(alias = "compatible_components", default)]
    pub detected_components: Vec<DetectedComponent>,
}

impl CatalogRepository {
    /// Return the resolved full repository name in 'owner/repo' format.
    pub fn resolved_full_name(&self) -> String {
        if !self.full_name.is_empty() {
            self.full_name.clone()
        } else if !self.owner.is_empty() && !self.repo.is_empty() {
            format!("{}/{}", self.owner, self.repo)
        } else {
            String::new()
        }
    }

    /// Return stars count, providing ergonomic access.
    pub fn stars(&self) -> u64 {
        self.stargazers_count
    }
}

/// Complete catalog repository registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Catalog {
    #[serde(rename = "$schema", default = "default_catalog_schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default = "default_catalog_version")]
    pub version: u32,
    pub last_synced: DateTime<Utc>,
    #[serde(default)]
    pub repositories: Vec<CatalogRepository>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            schema: default_catalog_schema(),
            version: default_catalog_version(),
            last_synced: Utc::now(),
            repositories: Vec::new(),
        }
    }
}
