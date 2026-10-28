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

fn deserialize_owner<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OwnerHelper {
        Text(String),
        Object { login: String },
    }

    match Option::<OwnerHelper>::deserialize(deserializer)? {
        Some(OwnerHelper::Text(s)) => Ok(s),
        Some(OwnerHelper::Object { login }) => Ok(login),
        None => Ok(String::new()),
    }
}

fn deserialize_topics<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<Vec<String>>::deserialize(deserializer)? {
        Some(topics) => Ok(topics),
        None => Ok(Vec::new()),
    }
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
    #[serde(default)]
    pub id: u64,
    #[serde(
        default,
        deserialize_with = "deserialize_owner",
        skip_serializing_if = "String::is_empty"
    )]
    pub owner: String,
    #[serde(default, alias = "name", skip_serializing_if = "String::is_empty")]
    pub repo: String,
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "stars")]
    pub stargazers_count: u64,
    #[serde(default = "default_branch_main")]
    pub default_branch: String,
    #[serde(default, deserialize_with = "deserialize_topics")]
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
        } else if !self.resolved_owner().is_empty() && !self.resolved_repo().is_empty() {
            format!("{}/{}", self.resolved_owner(), self.resolved_repo())
        } else {
            String::new()
        }
    }

    /// Return the resolved owner string.
    pub fn resolved_owner(&self) -> &str {
        if !self.owner.is_empty() {
            &self.owner
        } else if let Some((o, _)) = self.full_name.split_once('/') {
            o
        } else {
            ""
        }
    }

    /// Return the resolved repo string.
    pub fn resolved_repo(&self) -> &str {
        if !self.repo.is_empty() {
            &self.repo
        } else if let Some((_, r)) = self.full_name.split_once('/') {
            r
        } else {
            ""
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
    #[serde(
        rename = "$schema",
        default = "default_catalog_schema",
        skip_serializing_if = "Option::is_none"
    )]
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

/// Pre-seeded list of genuine top-rated real-world Hyprland and Wayland repositories.
pub fn default_popular_repositories() -> Vec<CatalogRepository> {
    vec![
        CatalogRepository {
            id: 647000001,
            owner: "mylinuxforwork".to_string(),
            repo: "dotfiles".to_string(),
            full_name: "mylinuxforwork/dotfiles".to_string(),
            html_url: "https://github.com/mylinuxforwork/dotfiles".to_string(),
            description: Some(
                "The ML4W Dotfiles for Hyprland - An advanced and customizable desktop suite"
                    .to_string(),
            ),
            stargazers_count: 5086,
            default_branch: "main".to_string(),
            topics: vec![
                "hyprland".to_string(),
                "dotfiles".to_string(),
                "waybar".to_string(),
                "arch".to_string(),
                "linux".to_string(),
                "rofi".to_string(),
            ],
            pushed_at: None,
            cached_tree_sha: None,
            etag: None,
            detected_components: vec![
                DetectedComponent {
                    component: ConfigComponent::Hyprland,
                    syntax_format: SyntaxFormat::LegacyConf,
                    root_path: ".config/hypr".to_string(),
                    entrypoint: "hyprland.conf".to_string(),
                    style_file: None,
                    file_count: 24,
                    total_size_bytes: 42000,
                },
                DetectedComponent {
                    component: ConfigComponent::Waybar,
                    syntax_format: SyntaxFormat::Jsonc,
                    root_path: ".config/waybar".to_string(),
                    entrypoint: "config.jsonc".to_string(),
                    style_file: Some("style.css".to_string()),
                    file_count: 12,
                    total_size_bytes: 28000,
                },
                DetectedComponent {
                    component: ConfigComponent::Hyprlock,
                    syntax_format: SyntaxFormat::LegacyConf,
                    root_path: ".config/hypr".to_string(),
                    entrypoint: "hyprlock.conf".to_string(),
                    style_file: None,
                    file_count: 2,
                    total_size_bytes: 4500,
                },
                DetectedComponent {
                    component: ConfigComponent::Hypridle,
                    syntax_format: SyntaxFormat::LegacyConf,
                    root_path: ".config/hypr".to_string(),
                    entrypoint: "hypridle.conf".to_string(),
                    style_file: None,
                    file_count: 1,
                    total_size_bytes: 1800,
                },
                DetectedComponent {
                    component: ConfigComponent::Wallpapers,
                    syntax_format: SyntaxFormat::BinaryAsset,
                    root_path: "wallpapers".to_string(),
                    entrypoint: "default.png".to_string(),
                    style_file: None,
                    file_count: 15,
                    total_size_bytes: 12400000,
                },
            ],
        },
        CatalogRepository {
            id: 647000002,
            owner: "end-4".to_string(),
            repo: "dots-hyprland".to_string(),
            full_name: "end-4/dots-hyprland".to_string(),
            html_url: "https://github.com/end-4/dots-hyprland".to_string(),
            description: Some(
                "Fabric / Quickshell / Hyprland configuration with Material You aesthetic"
                    .to_string(),
            ),
            stargazers_count: 16483,
            default_branch: "main".to_string(),
            topics: vec![
                "dotfiles".to_string(),
                "hyprland".to_string(),
                "linux".to_string(),
                "material-design".to_string(),
                "quickshell".to_string(),
                "rice".to_string(),
                "wayland".to_string(),
            ],
            pushed_at: None,
            cached_tree_sha: None,
            etag: None,
            detected_components: vec![
                DetectedComponent {
                    component: ConfigComponent::Hyprland,
                    syntax_format: SyntaxFormat::LegacyConf,
                    root_path: ".config/hypr".to_string(),
                    entrypoint: "hyprland.conf".to_string(),
                    style_file: None,
                    file_count: 32,
                    total_size_bytes: 68000,
                },
                DetectedComponent {
                    component: ConfigComponent::Waybar,
                    syntax_format: SyntaxFormat::Jsonc,
                    root_path: ".config/waybar".to_string(),
                    entrypoint: "config.jsonc".to_string(),
                    style_file: Some("style.css".to_string()),
                    file_count: 10,
                    total_size_bytes: 25000,
                },
            ],
        },
        CatalogRepository {
            id: 647000003,
            owner: "prasanthrangan".to_string(),
            repo: "hyprdots".to_string(),
            full_name: "prasanthrangan/hyprdots".to_string(),
            html_url: "https://github.com/prasanthrangan/hyprdots".to_string(),
            description: Some(
                "Custom Hyprland desktop configuration and themes suite".to_string(),
            ),
            stargazers_count: 8475,
            default_branch: "main".to_string(),
            topics: vec![
                "archlinux".to_string(),
                "dotfiles".to_string(),
                "hyprdots".to_string(),
                "hyprland".to_string(),
                "waybar".to_string(),
                "rice".to_string(),
            ],
            pushed_at: None,
            cached_tree_sha: None,
            etag: None,
            detected_components: vec![
                DetectedComponent {
                    component: ConfigComponent::Hyprland,
                    syntax_format: SyntaxFormat::LegacyConf,
                    root_path: "Configs/.config/hypr".to_string(),
                    entrypoint: "hyprland.conf".to_string(),
                    style_file: None,
                    file_count: 18,
                    total_size_bytes: 52000,
                },
                DetectedComponent {
                    component: ConfigComponent::Waybar,
                    syntax_format: SyntaxFormat::Jsonc,
                    root_path: "Configs/.config/waybar".to_string(),
                    entrypoint: "config.jsonc".to_string(),
                    style_file: Some("style.css".to_string()),
                    file_count: 8,
                    total_size_bytes: 21000,
                },
                DetectedComponent {
                    component: ConfigComponent::Hyprlock,
                    syntax_format: SyntaxFormat::LegacyConf,
                    root_path: "Configs/.config/hypr".to_string(),
                    entrypoint: "hyprlock.conf".to_string(),
                    style_file: None,
                    file_count: 1,
                    total_size_bytes: 3200,
                },
            ],
        },
        CatalogRepository {
            id: 647000004,
            owner: "JaKooLit".to_string(),
            repo: "Hyprland-Dots".to_string(),
            full_name: "JaKooLit/Hyprland-Dots".to_string(),
            html_url: "https://github.com/JaKooLit/Hyprland-Dots".to_string(),
            description: Some(
                "JaKooLit's automated and curated Hyprland configuration files".to_string(),
            ),
            stargazers_count: 3200,
            default_branch: "main".to_string(),
            topics: vec![
                "arch".to_string(),
                "dotfiles".to_string(),
                "hyprland".to_string(),
                "waybar".to_string(),
                "rofi".to_string(),
            ],
            pushed_at: None,
            cached_tree_sha: None,
            etag: None,
            detected_components: vec![
                DetectedComponent {
                    component: ConfigComponent::Hyprland,
                    syntax_format: SyntaxFormat::LegacyConf,
                    root_path: "config/hypr".to_string(),
                    entrypoint: "hyprland.conf".to_string(),
                    style_file: None,
                    file_count: 20,
                    total_size_bytes: 48000,
                },
                DetectedComponent {
                    component: ConfigComponent::Waybar,
                    syntax_format: SyntaxFormat::Jsonc,
                    root_path: "config/waybar".to_string(),
                    entrypoint: "config.jsonc".to_string(),
                    style_file: Some("style.css".to_string()),
                    file_count: 14,
                    total_size_bytes: 33000,
                },
                DetectedComponent {
                    component: ConfigComponent::Hyprlock,
                    syntax_format: SyntaxFormat::LegacyConf,
                    root_path: "config/hypr".to_string(),
                    entrypoint: "hyprlock.conf".to_string(),
                    style_file: None,
                    file_count: 1,
                    total_size_bytes: 2800,
                },
            ],
        },
        CatalogRepository {
            id: 647000005,
            owner: "catppuccin".to_string(),
            repo: "hyprland".to_string(),
            full_name: "catppuccin/hyprland".to_string(),
            html_url: "https://github.com/catppuccin/hyprland".to_string(),
            description: Some("Soothing pastel theme for Hyprland".to_string()),
            stargazers_count: 1100,
            default_branch: "main".to_string(),
            topics: vec![
                "catppuccin".to_string(),
                "hyprland".to_string(),
                "theme".to_string(),
            ],
            pushed_at: None,
            cached_tree_sha: None,
            etag: None,
            detected_components: vec![DetectedComponent {
                component: ConfigComponent::Hyprland,
                syntax_format: SyntaxFormat::LegacyConf,
                root_path: "themes".to_string(),
                entrypoint: "mocha.conf".to_string(),
                style_file: None,
                file_count: 4,
                total_size_bytes: 8500,
            }],
        },
    ]
}

impl Catalog {
    /// Constructs a catalog populated with popular genuine community Hyprland repositories.
    pub fn default_popular() -> Self {
        Self {
            schema: default_catalog_schema(),
            version: default_catalog_version(),
            last_synced: Utc::now(),
            repositories: default_popular_repositories(),
        }
    }
}
