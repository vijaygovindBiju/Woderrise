//! XDG path resolution, dotfile destination mapping, and traversal sanitization.

use crate::error::{PathError, Result, WoderriseError};
use crate::models::ConfigComponent;
use camino::{Utf8Path, Utf8PathBuf};
use directories::BaseDirs;
use std::env;

/// Complete XDG directory layout for Woderrise and target desktop environments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdgPaths {
    pub home_dir: Utf8PathBuf,
    pub config_home: Utf8PathBuf,
    pub data_home: Utf8PathBuf,
    pub cache_home: Utf8PathBuf,
    pub state_home: Utf8PathBuf,
}

impl XdgPaths {
    /// Construct XDG paths by querying environment variables with standard Linux fallbacks.
    pub fn from_env() -> Result<Self> {
        let home_str = if let Ok(val) = env::var("HOME") {
            if !val.trim().is_empty() {
                val
            } else {
                let base = BaseDirs::new().ok_or_else(|| PathError::BaseDirectoryNotFound {
                    dir_type: "HOME".to_string(),
                })?;
                base.home_dir().to_string_lossy().to_string()
            }
        } else {
            let base = BaseDirs::new().ok_or_else(|| PathError::BaseDirectoryNotFound {
                dir_type: "HOME".to_string(),
            })?;
            base.home_dir().to_string_lossy().to_string()
        };

        let home_dir =
            Utf8PathBuf::from_path_buf(home_str.into()).map_err(|p| PathError::NonUtf8Path {
                path: format!("{:?}", p),
            })?;

        let config_home = if let Ok(val) = env::var("XDG_CONFIG_HOME") {
            if !val.trim().is_empty() {
                Utf8PathBuf::from_path_buf(val.into()).map_err(|p| PathError::NonUtf8Path {
                    path: format!("{:?}", p),
                })?
            } else {
                home_dir.join(".config")
            }
        } else {
            home_dir.join(".config")
        };

        let data_home = if let Ok(val) = env::var("XDG_DATA_HOME") {
            if !val.trim().is_empty() {
                Utf8PathBuf::from_path_buf(val.into()).map_err(|p| PathError::NonUtf8Path {
                    path: format!("{:?}", p),
                })?
            } else {
                home_dir.join(".local/share")
            }
        } else {
            home_dir.join(".local/share")
        };

        let cache_home = if let Ok(val) = env::var("XDG_CACHE_HOME") {
            if !val.trim().is_empty() {
                Utf8PathBuf::from_path_buf(val.into()).map_err(|p| PathError::NonUtf8Path {
                    path: format!("{:?}", p),
                })?
            } else {
                home_dir.join(".cache")
            }
        } else {
            home_dir.join(".cache")
        };

        let state_home = if let Ok(val) = env::var("XDG_STATE_HOME") {
            if !val.trim().is_empty() {
                Utf8PathBuf::from_path_buf(val.into()).map_err(|p| PathError::NonUtf8Path {
                    path: format!("{:?}", p),
                })?
            } else {
                home_dir.join(".local/state")
            }
        } else {
            home_dir.join(".local/state")
        };

        Ok(Self {
            home_dir,
            config_home,
            data_home,
            cache_home,
            state_home,
        })
    }

    /// Construct isolated XDG paths rooted in a custom base directory (crucial for unit tests).
    pub fn custom(base_dir: &Utf8Path) -> Self {
        Self {
            home_dir: base_dir.to_path_buf(),
            config_home: base_dir.join(".config"),
            data_home: base_dir.join(".local/share"),
            cache_home: base_dir.join(".cache"),
            state_home: base_dir.join(".local/state"),
        }
    }

    // --- Woderrise Application Directories ---

    /// Path to `~/.config/woderrise` (application preferences)
    pub fn woderrise_config_dir(&self) -> Utf8PathBuf {
        self.config_home.join("woderrise")
    }

    /// Path to `~/.config/woderrise/settings.json`
    pub fn settings_file(&self) -> Utf8PathBuf {
        self.woderrise_config_dir().join("settings.json")
    }

    /// Path to `~/.local/share/woderrise` (persistent data)
    pub fn woderrise_data_dir(&self) -> Utf8PathBuf {
        self.data_home.join("woderrise")
    }

    /// Path to `~/.local/share/woderrise/catalog.json`
    pub fn catalog_file(&self) -> Utf8PathBuf {
        self.woderrise_data_dir().join("catalog.json")
    }

    /// Path to `~/.local/share/woderrise/journal.json` (WAL transaction journal)
    pub fn journal_file(&self) -> Utf8PathBuf {
        self.woderrise_data_dir().join("journal.json")
    }

    /// Path to `~/.cache/woderrise` (ephemeral caching & staging)
    pub fn woderrise_cache_dir(&self) -> Utf8PathBuf {
        self.cache_home.join("woderrise")
    }

    /// Path to `~/.cache/woderrise/staging/<tx_id>`
    pub fn staging_dir(&self, tx_id: &str) -> Result<Utf8PathBuf> {
        validate_identifier(tx_id)?;
        Ok(self.woderrise_cache_dir().join("staging").join(tx_id))
    }

    /// Path to `~/.local/state/woderrise` (pre-install snapshot backups)
    pub fn woderrise_state_dir(&self) -> Utf8PathBuf {
        self.state_home.join("woderrise")
    }

    /// Path to `~/.local/state/woderrise/backups/<tx_id>`
    pub fn backup_dir(&self, tx_id: &str) -> Result<Utf8PathBuf> {
        validate_identifier(tx_id)?;
        Ok(self.woderrise_state_dir().join("backups").join(tx_id))
    }

    // --- Target Desktop Dotfile Roots ---

    /// The allowed component root on the user's filesystem where files for this component MUST reside.
    pub fn component_root(&self, component: ConfigComponent) -> Utf8PathBuf {
        match component {
            ConfigComponent::Hyprland
            | ConfigComponent::HyprlandLegacy
            | ConfigComponent::HyprlandLua
            | ConfigComponent::Hyprlock
            | ConfigComponent::Hypridle
            | ConfigComponent::Hyprpaper
            | ConfigComponent::Monitors => self.config_home.join("hypr"),

            ConfigComponent::Waybar => self.config_home.join("waybar"),

            ConfigComponent::GtkTheme => self.config_home.join("gtk-3.0"),

            ConfigComponent::Hyprcursor => self.data_home.join("icons"),

            ConfigComponent::Wallpapers => self.data_home.join("wallpapers"),

            ConfigComponent::CompleteSuite => self.config_home.clone(),
        }
    }

    /// Resolve a sanitized relative dotfile path to its final absolute destination,
    /// verifying containment within the allowed component root.
    pub fn resolve_destination(
        &self,
        component: ConfigComponent,
        relative_path: &Utf8Path,
    ) -> Result<Utf8PathBuf> {
        let clean_rel = sanitize_relative_path(relative_path)?;
        let root = self.component_root(component);
        let destination = root.join(&clean_rel);

        // Enforce strict component root pinning
        if !destination.starts_with(&root) {
            return Err(PathError::ComponentRootEscape {
                path: destination,
                allowed_root: root,
            }
            .into());
        }

        Ok(destination)
    }

    /// Verify that an absolute target path is fully contained within the component root.
    pub fn validate_target_containment(
        &self,
        target: &Utf8Path,
        component: ConfigComponent,
    ) -> Result<()> {
        let target_str = target.as_str();

        // 1. Check for null bytes
        if target_str.contains('\0') {
            return Err(PathError::ContainsNullByte {
                path: target_str.to_string(),
            }
            .into());
        }

        // 2. Reject parent directory traversal (..) in any component
        for comp in target.components() {
            match comp {
                camino::Utf8Component::ParentDir => {
                    return Err(PathError::PathTraversalAttempted {
                        path: target.to_string(),
                        token: "..".to_string(),
                    }
                    .into());
                }
                camino::Utf8Component::Normal(segment)
                    if segment == ".." || segment.contains("..") =>
                {
                    return Err(PathError::PathTraversalAttempted {
                        path: target.to_string(),
                        token: segment.to_string(),
                    }
                    .into());
                }
                _ => {}
            }
        }

        // 3. Enforce strict component root pinning
        let root = self.component_root(component);
        if !target.starts_with(&root) {
            return Err(PathError::ComponentRootEscape {
                path: target.to_path_buf(),
                allowed_root: root,
            }
            .into());
        }
        Ok(())
    }
}

/// Standalone convenience functions for default environment paths.
pub fn xdg_config_dir() -> Result<Utf8PathBuf> {
    Ok(XdgPaths::from_env()?.config_home)
}

pub fn xdg_data_dir() -> Result<Utf8PathBuf> {
    Ok(XdgPaths::from_env()?.data_home)
}

pub fn xdg_cache_dir() -> Result<Utf8PathBuf> {
    Ok(XdgPaths::from_env()?.cache_home)
}

pub fn xdg_state_dir() -> Result<Utf8PathBuf> {
    Ok(XdgPaths::from_env()?.state_home)
}

pub fn settings_path() -> Result<Utf8PathBuf> {
    Ok(XdgPaths::from_env()?.settings_file())
}

pub fn catalog_path() -> Result<Utf8PathBuf> {
    Ok(XdgPaths::from_env()?.catalog_file())
}

pub fn journal_path() -> Result<Utf8PathBuf> {
    Ok(XdgPaths::from_env()?.journal_file())
}

pub fn staging_dir(tx_id: &str) -> Result<Utf8PathBuf> {
    XdgPaths::from_env()?.staging_dir(tx_id)
}

pub fn backup_dir(tx_id: &str) -> Result<Utf8PathBuf> {
    XdgPaths::from_env()?.backup_dir(tx_id)
}

pub fn allowed_component_root(component: ConfigComponent) -> Result<Utf8PathBuf> {
    Ok(XdgPaths::from_env()?.component_root(component))
}

pub fn validate_target_containment(target: &Utf8Path, component: ConfigComponent) -> Result<()> {
    XdgPaths::from_env()?.validate_target_containment(target, component)
}

/// Validate that an identifier (such as transaction_id) contains only safe characters.
pub fn validate_identifier(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(WoderriseError::Validation(
            "Identifier cannot be empty".into(),
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(PathError::PathTraversalAttempted {
            path: id.to_string(),
            token: "invalid_identifier_characters".to_string(),
        }
        .into());
    }
    Ok(())
}

/// Sanitize an untrusted relative path extracted from a Git Tree.
///
/// Rules:
/// 1. Rejects null bytes (`\0`).
/// 2. Rejects empty paths.
/// 3. Rejects absolute paths (leading `/`, `\`, or Windows drive letter prefix).
/// 4. Rejects parent directory traversal tokens (`..`).
/// 5. Strips redundant `.` components.
pub fn sanitize_relative_path(path: &Utf8Path) -> Result<Utf8PathBuf> {
    let path_str = path.as_str();

    // 1. Check for empty
    if path_str.trim().is_empty() {
        return Err(PathError::PathTraversalAttempted {
            path: path_str.to_string(),
            token: "empty_path".to_string(),
        }
        .into());
    }

    // 2. Check for null bytes
    if path_str.contains('\0') {
        return Err(PathError::ContainsNullByte {
            path: path_str.to_string(),
        }
        .into());
    }

    // 3. Detect and reject percent-encoded traversal tokens (%2e, %2f, %5c, %00)
    let lower = path_str.to_ascii_lowercase();
    for token in &["%2e", "%2f", "%5c", "%00"] {
        if lower.contains(token) {
            return Err(PathError::PathTraversalAttempted {
                path: path_str.to_string(),
                token: (*token).to_string(),
            }
            .into());
        }
    }

    // 4. Reject absolute paths and Windows drive letters
    if path.is_absolute() || path_str.starts_with('/') || path_str.starts_with('\\') {
        return Err(PathError::AbsolutePathNotAllowed {
            path: path_str.to_string(),
        }
        .into());
    }
    if path_str.len() >= 2 {
        let first = path_str.chars().next().unwrap();
        let second = path_str.chars().nth(1).unwrap();
        if first.is_ascii_alphabetic() && second == ':' {
            return Err(PathError::AbsolutePathNotAllowed {
                path: path_str.to_string(),
            }
            .into());
        }
    }

    // 5. Check for traversal tokens in path components
    let mut clean_components = Vec::new();
    for comp in path.components() {
        match comp {
            camino::Utf8Component::ParentDir => {
                return Err(PathError::PathTraversalAttempted {
                    path: path_str.to_string(),
                    token: "..".to_string(),
                }
                .into());
            }
            camino::Utf8Component::CurDir => {
                // Skip '.'
                continue;
            }
            camino::Utf8Component::Normal(segment) => {
                if segment == ".." || segment.contains("..") {
                    return Err(PathError::PathTraversalAttempted {
                        path: path_str.to_string(),
                        token: segment.to_string(),
                    }
                    .into());
                }
                clean_components.push(segment);
            }
            camino::Utf8Component::RootDir | camino::Utf8Component::Prefix(_) => {
                return Err(PathError::AbsolutePathNotAllowed {
                    path: path_str.to_string(),
                }
                .into());
            }
        }
    }

    if clean_components.is_empty() {
        return Err(PathError::PathTraversalAttempted {
            path: path_str.to_string(),
            token: "empty_path".to_string(),
        }
        .into());
    }

    let result = clean_components.iter().collect::<Utf8PathBuf>();
    Ok(result)
}

/// Strip well-known repository root prefixes to resolve to component-relative paths.
/// E.g. `Configs/.config/hypr/hyprland.conf` -> `hyprland.conf` when component is Hyprland.
/// Only strips the first matching component prefix to preserve nested directories
/// of the same name (e.g. `dotfiles/hypr/themes/hypr/custom.conf` -> `themes/hypr/custom.conf`).
pub fn strip_dotfile_repo_prefix<'a>(path: &'a Utf8Path, component_subpath: &str) -> &'a Utf8Path {
    let s = path.as_str();
    let marker_start = format!("{}/", component_subpath);
    let marker_slash = format!("/{}", marker_start);

    if s.starts_with(&marker_start) {
        return Utf8Path::new(&s[marker_start.len()..]);
    }
    if let Some(pos) = s.find(&marker_slash) {
        let remainder = &s[pos + marker_slash.len()..];
        return Utf8Path::new(remainder);
    }
    path
}
