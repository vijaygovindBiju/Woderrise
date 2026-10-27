//! Pre-flight security auditor for dotfile transaction staging.
//!
//! Enforces defensive security boundaries:
//! - Rejection of Git tree symlink objects (mode `120000`, CVE-2021-21300 directory traversal).
//! - Rejection of executable file modes (`100755` or ending in `755`).
//! - Rejection of executable script extensions (`.sh`, `.bash`, `.zsh`, `.py`, `.pl`, `.rb`, `.bin`).
//! - Path traversal rejection (`..`, null bytes, absolute paths).
//! - Strict component root pinning.
//! - Bounded resource limits (10MB configs, 50MB assets).
//! - Hardware-bound file protection (`monitors.conf`, `workspaces.conf`).
//! - Host symlink inspection via `symlink_metadata()` to resolve canonical targets
//!   and preserve GNU Stow / Chezmoi management workflows.

use camino::{Utf8Path, Utf8PathBuf};
use woderrise_core::error::{PathError, Result, SecurityError};
use woderrise_core::models::{
    ConfigComponent, ConflictStatus, HardwareConflictNotice, SecurityAuditReport, TargetMapping,
};
use woderrise_core::paths::{sanitize_relative_path, XdgPaths};
use woderrise_detect::{is_hardware_bound_filename, TreeEntry};

/// Maximum allowable size for standard desktop configuration files (10 MB).
pub const MAX_CONFIG_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum allowable size for wallpaper or image assets (50 MB).
pub const MAX_ASSET_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Security auditor enforcing defensive checks on incoming dotfile repository entries.
pub struct SecurityAuditor;

impl SecurityAuditor {
    /// Audits a single tree entry against security policies and determines local filesystem conflict status.
    pub fn audit_entry(
        entry: &TreeEntry,
        component: ConfigComponent,
        xdg: &XdgPaths,
    ) -> Result<TargetMapping> {
        let clean_rel = sanitize_relative_path(entry.utf8_path())?;

        // 1. Rejection of Git tree symlink objects (mode 120000) - CVE-2021-21300 protection
        if entry.is_symlink() || entry.mode == "120000" || entry.mode.starts_with("120") {
            return Err(PathError::ForbiddenSymlink {
                path: clean_rel,
                target: Utf8PathBuf::from("mode_120000_symlink_object"),
            }
            .into());
        }

        // 2. Rejection of executable permissions (100755, ends with 755)
        if entry.is_executable() || entry.mode == "100755" || entry.mode.ends_with("755") {
            return Err(SecurityError::ScriptExecutionBlocked {
                path: clean_rel,
                script_type: format!("executable_mode_{}", entry.mode),
            }
            .into());
        }

        // 3. Rejection of script file extensions (sh, bash, zsh, py, pl, rb, bin)
        if let Some(ext) = clean_rel.extension() {
            let lower_ext = ext.to_ascii_lowercase();
            if matches!(
                lower_ext.as_str(),
                "sh" | "bash" | "zsh" | "py" | "pl" | "rb" | "bin"
            ) {
                return Err(SecurityError::ScriptExecutionBlocked {
                    path: clean_rel,
                    script_type: format!("script_extension_{}", lower_ext),
                }
                .into());
            }
        }

        // 4. Resource limit audit
        if let Some(size) = entry.size {
            let limit = if component == ConfigComponent::Wallpapers {
                MAX_ASSET_FILE_SIZE
            } else {
                MAX_CONFIG_FILE_SIZE
            };
            if size > limit {
                return Err(SecurityError::ResourceLimitExceeded {
                    resource: clean_rel.to_string(),
                    limit_bytes: limit,
                    actual_bytes: size,
                }
                .into());
            }
        }

        // 5. Destination resolution and component root containment
        let target_dest = xdg.resolve_destination(component, &clean_rel)?;
        xdg.validate_target_containment(&target_dest, component)?;

        // 6. Hardware monitor preservation check
        let is_hw = is_hardware_bound_filename(clean_rel.file_name().unwrap_or_default());

        // 7. Host filesystem inspection using symlink_metadata
        let (effective_dest, conflict_status) = if is_hw {
            (target_dest, ConflictStatus::HardwareProtected)
        } else if let Ok(meta) = std::fs::symlink_metadata(&target_dest) {
            if meta.file_type().is_symlink() {
                // GNU Stow / Chezmoi symlink: resolve canonical target to preserve symlink hierarchy
                match std::fs::canonicalize(&target_dest) {
                    Ok(canonical) => {
                        let canonical_utf8 =
                            Utf8PathBuf::from_path_buf(canonical).map_err(|p| {
                                PathError::NonUtf8Path {
                                    path: format!("{:?}", p),
                                }
                            })?;
                        (canonical_utf8, ConflictStatus::ExistingSymlink)
                    }
                    Err(_) => (target_dest, ConflictStatus::OverwritingExisting),
                }
            } else {
                (target_dest, ConflictStatus::OverwritingExisting)
            }
        } else {
            (target_dest, ConflictStatus::NewFile)
        };

        Ok(TargetMapping {
            component,
            relative_path: clean_rel,
            blob_sha: entry.sha.clone(),
            target_destination: effective_dest,
            git_file_mode: entry.mode.clone(),
            expected_sha256: String::new(),
            is_hardware_protected: is_hw,
            conflict_status,
        })
    }

    /// Audit a batch of tree entries, generating target mappings and audit reports.
    pub fn audit_entries(
        entries: &[TreeEntry],
        component: ConfigComponent,
        xdg: &XdgPaths,
    ) -> Result<(
        Vec<TargetMapping>,
        SecurityAuditReport,
        Vec<HardwareConflictNotice>,
    )> {
        let mut mappings = Vec::with_capacity(entries.len());
        let mut hardware_notices = Vec::new();

        for entry in entries {
            let mapping = Self::audit_entry(entry, component, xdg)?;
            if mapping.is_hardware_protected {
                hardware_notices.push(HardwareConflictNotice {
                    file_path: mapping.target_destination.clone(),
                    description: format!(
                        "Hardware-bound display configuration detected in '{}'",
                        mapping.relative_path
                    ),
                    preserved_by_default: true,
                });
            }
            mappings.push(mapping);
        }

        let report = SecurityAuditReport {
            scripts_detected: 0,
            procedural_directives_neutralized: 0,
            path_traversal_violations: 0,
            symlinks_rejected: 0,
            passed: true,
        };

        Ok((mappings, report, hardware_notices))
    }

    /// Extracts procedural directives from configuration content using detectors.
    pub fn extract_procedural_directives(
        component: ConfigComponent,
        file_path: &Utf8Path,
        content: &str,
    ) -> Vec<woderrise_detect::ProceduralDirective> {
        let file_str = file_path.as_str();
        match component {
            ConfigComponent::Hyprland
            | ConfigComponent::HyprlandLegacy
            | ConfigComponent::HyprlandLua => {
                woderrise_detect::extract_hyprland_directives(content, file_str)
            }
            ConfigComponent::Waybar => {
                woderrise_detect::extract_waybar_directives(content, file_str)
            }
            ConfigComponent::Hypridle => {
                woderrise_detect::extract_hypridle_directives(content, file_str)
            }
            _ => Vec::new(),
        }
    }
}
