//! Pre-installation backup snapshot engine.
//!
//! Creates isolated pre-installation backup snapshots in `$XDG_STATE_HOME/woderrise/backups/<tx_id>/`
//! before any host configuration files are modified or overwritten.
//! Tracks pre/post SHA-256 hashes and coordinates legacy configuration deactivation.

use camino::Utf8PathBuf;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use woderrise_core::error::{IoError, Result, TransactionError, WoderriseError};
use woderrise_core::models::{ConfigComponent, FileOperationRecord, OperationType, StagedFile};
use woderrise_core::paths::{validate_identifier, XdgPaths};

/// Engine for creating pre-installation backup snapshots of host configurations.
#[derive(Debug, Clone)]
pub struct BackupSnapshotEngine {
    pub tx_id: String,
    pub backup_dir: Utf8PathBuf,
}

impl BackupSnapshotEngine {
    /// Initializes the backup engine and creates the snapshot directory.
    pub fn new(xdg: &XdgPaths, tx_id: &str) -> Result<Self> {
        validate_identifier(tx_id)?;
        let backup_dir = xdg.backup_dir(tx_id)?;
        if !backup_dir.exists() {
            fs::create_dir_all(&backup_dir).map_err(|source| {
                WoderriseError::Io(IoError::CreateDir {
                    path: backup_dir.clone(),
                    source,
                })
            })?;
        }

        Ok(Self {
            tx_id: tx_id.to_string(),
            backup_dir,
        })
    }

    /// Performs pre-installation backups for all files in the staged manifest.
    ///
    /// Copies existing files that will be modified into the snapshot directory,
    /// computes pre-installation SHA-256 hashes, handles GNU Stow/Chezmoi canonical
    /// symlink resolution, and detects legacy Hyprland configs when installing Lua.
    pub fn create_snapshot(
        &self,
        staged_files: &[StagedFile],
        component: ConfigComponent,
        xdg: &XdgPaths,
    ) -> Result<Vec<FileOperationRecord>> {
        let mut operations = Vec::with_capacity(staged_files.len() + 1);

        for file in staged_files {
            let target_path = match &file.target_destination {
                Some(p) => p.clone(),
                None => xdg.resolve_destination(component, &file.relative_path)?,
            };

            // Inspect host target file using symlink_metadata
            let effective_path = match fs::symlink_metadata(&target_path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    // Resolve canonical location for symlinks to prevent breaking GNU Stow / Chezmoi
                    match fs::canonicalize(&target_path) {
                        Ok(canonical) => Utf8PathBuf::from_path_buf(canonical)
                            .unwrap_or_else(|_| target_path.clone()),
                        Err(_) => target_path.clone(),
                    }
                }
                _ => target_path.clone(),
            };

            if effective_path.exists() && effective_path.is_file() {
                // Compute pre-installation SHA-256
                let mut f = File::open(&effective_path).map_err(|source| {
                    WoderriseError::Io(IoError::Read {
                        path: effective_path.clone(),
                        source,
                    })
                })?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).map_err(|source| {
                    WoderriseError::Io(IoError::Read {
                        path: effective_path.clone(),
                        source,
                    })
                })?;
                let mut hasher = Sha256::new();
                hasher.update(&buf);
                let pre_sha = format!("{:x}", hasher.finalize());

                // Snapshot destination inside backup directory
                let snap_path = self.backup_dir.join(&file.relative_path);
                if let Some(parent) = snap_path.parent() {
                    if !parent.exists() {
                        fs::create_dir_all(parent).map_err(|source| {
                            WoderriseError::Io(IoError::CreateDir {
                                path: parent.to_path_buf(),
                                source,
                            })
                        })?;
                    }
                }

                fs::copy(&effective_path, &snap_path).map_err(|source| {
                    WoderriseError::Transaction(TransactionError::BackupFailed {
                        tx_id: self.tx_id.clone(),
                        path: effective_path.clone(),
                        reason: source.to_string(),
                    })
                })?;

                operations.push(FileOperationRecord {
                    op_type: OperationType::Overwrite,
                    target_file: effective_path,
                    backup_file: Some(snap_path),
                    pre_sha256: Some(pre_sha),
                    post_sha256: Some(file.sha256.clone()),
                    mode: Some(file.file_mode.clone()),
                });
            } else {
                operations.push(FileOperationRecord {
                    op_type: OperationType::Create,
                    target_file: effective_path,
                    backup_file: None,
                    pre_sha256: None,
                    post_sha256: Some(file.sha256.clone()),
                    mode: Some(file.file_mode.clone()),
                });
            }
        }

        // Special handling: Hyprland Lua precedence over legacy conf
        if matches!(
            component,
            ConfigComponent::Hyprland
                | ConfigComponent::HyprlandLua
                | ConfigComponent::CompleteSuite
        ) {
            let has_lua = staged_files.iter().any(|f| {
                f.relative_path.file_name() == Some("hyprland.lua")
                    || f.relative_path.as_str().ends_with("hyprland.lua")
            });
            let legacy_conf = xdg
                .component_root(ConfigComponent::Hyprland)
                .join("hyprland.conf");

            if has_lua && legacy_conf.exists() && legacy_conf.is_file() {
                // Ensure we haven't already scheduled legacy_conf in operations
                let already_managed = operations.iter().any(|op| {
                    op.target_file == legacy_conf && op.op_type == OperationType::Overwrite
                });

                if !already_managed {
                    let mut f = File::open(&legacy_conf).map_err(|source| {
                        WoderriseError::Io(IoError::Read {
                            path: legacy_conf.clone(),
                            source,
                        })
                    })?;
                    let mut buf = Vec::new();
                    f.read_to_end(&mut buf).map_err(|source| {
                        WoderriseError::Io(IoError::Read {
                            path: legacy_conf.clone(),
                            source,
                        })
                    })?;
                    let mut hasher = Sha256::new();
                    hasher.update(&buf);
                    let pre_sha = format!("{:x}", hasher.finalize());

                    let snap_legacy = self.backup_dir.join("hyprland.conf.legacy_backup");
                    fs::copy(&legacy_conf, &snap_legacy).map_err(|source| {
                        WoderriseError::Transaction(TransactionError::BackupFailed {
                            tx_id: self.tx_id.clone(),
                            path: legacy_conf.clone(),
                            reason: source.to_string(),
                        })
                    })?;

                    operations.push(FileOperationRecord {
                        op_type: OperationType::DeactivateLegacy,
                        target_file: legacy_conf,
                        backup_file: Some(snap_legacy),
                        pre_sha256: Some(pre_sha),
                        post_sha256: None,
                        mode: None,
                    });
                }
            }
        }

        Ok(operations)
    }
}
