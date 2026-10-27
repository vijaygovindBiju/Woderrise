//! Startup crash recovery engine.
//!
//! Detects interrupted or incomplete transactions (`Applying`, `Staged`, or `RollingBack`)
//! recorded in `journal.json` upon application initialization. Safely rolls back
//! partial modifications using snapshot backups, removes abandoned temporary files,
//! and transitions transaction status to `RecoveredRollback`.

use camino::Utf8PathBuf;
use std::fs;
use tracing::{info, warn};
use woderrise_core::error::Result;
use woderrise_core::models::{ConfigComponent, JournalEntry, OperationType, TransactionStatus};
use woderrise_core::paths::XdgPaths;

use crate::atomic::AtomicSwapper;
use crate::wal::JournalManager;

/// Detailed outcome of a startup crash recovery run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryOutcome {
    pub tx_id: String,
    pub recovered_status: TransactionStatus,
    pub restored_files: Vec<Utf8PathBuf>,
}

/// Engine for recovering interrupted transactions on startup.
pub struct StartupRecovery;

impl StartupRecovery {
    /// Inspects the WAL journal on startup, restores any in-flight transaction,
    /// and cleans up abandoned temporary files.
    pub fn scan_and_recover(
        journal_mgr: &JournalManager,
        xdg: &XdgPaths,
    ) -> Result<Option<RecoveryOutcome>> {
        let _lock = journal_mgr.acquire_lock()?;

        // 1. Clean up stray .woderrise_tmp.* files across all known component roots
        for component in &[
            ConfigComponent::Hyprland,
            ConfigComponent::Waybar,
            ConfigComponent::GtkTheme,
            ConfigComponent::Hyprcursor,
            ConfigComponent::Wallpapers,
        ] {
            let root = xdg.component_root(*component);
            let _ = AtomicSwapper::prune_stray_tmp_files(&root);
        }

        let mut journal = journal_mgr.load_journal()?;

        // 2. Discover interrupted transactions in active_transaction
        let active = match journal.active_transaction.take() {
            Some(a) if a.status.is_in_flight() => a,
            _ => return Ok(None),
        };

        warn!(
            "Crash recovery: discovered interrupted transaction '{}' with status '{:?}'",
            active.id, active.status
        );

        let component_root = xdg.component_root(active.component);
        let mut restored = Vec::new();

        // 3. Roll back partial modifications in reverse LIFO order
        for op in active.operations.iter().rev() {
            match op.op_type {
                OperationType::Overwrite => {
                    if let Some(ref backup) = op.backup_file {
                        if backup.exists() {
                            let _ = AtomicSwapper::swap_file(backup, &op.target_file);
                            restored.push(op.target_file.clone());
                        }
                    }
                }
                OperationType::Create => {
                    if op.target_file.exists() {
                        let _ = fs::remove_file(&op.target_file);

                        // Prune empty parent directories up to component root
                        let mut curr = op.target_file.parent();
                        while let Some(dir) = curr {
                            if dir == component_root || !dir.starts_with(&component_root) {
                                break;
                            }
                            if let Ok(mut entries) = fs::read_dir(dir) {
                                if entries.next().is_none() {
                                    if fs::remove_dir(dir).is_err() {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                            curr = dir.parent();
                        }
                    }
                }
                OperationType::DeactivateLegacy => {
                    let disabled_path =
                        Utf8PathBuf::from(format!("{}.disabled_by_woderrise", op.target_file));
                    if disabled_path.exists() {
                        let _ = fs::rename(&disabled_path, &op.target_file);
                        restored.push(op.target_file.clone());
                    } else if let Some(ref backup) = op.backup_file {
                        if backup.exists() {
                            let _ = AtomicSwapper::swap_file(backup, &op.target_file);
                            restored.push(op.target_file.clone());
                        }
                    }
                }
                OperationType::Delete => {
                    if let Some(ref backup) = op.backup_file {
                        if backup.exists() {
                            let _ = AtomicSwapper::swap_file(backup, &op.target_file);
                            restored.push(op.target_file.clone());
                        }
                    }
                }
            }
        }

        // 4. Archive transaction as RecoveredRollback
        journal.transactions.push(JournalEntry {
            id: active.id.clone(),
            timestamp: active.timestamp,
            component: active.component,
            status: TransactionStatus::RecoveredRollback,
            source_repo: active.source_repo,
            source_commit: active.source_commit,
            backup_directory: active.backup_directory,
            operations: active.operations,
        });

        journal.active_transaction = None;
        journal_mgr.save_journal(&journal)?;

        // 5. Final stray temporary file sweep in component root
        let _ = AtomicSwapper::prune_stray_tmp_files(&component_root);

        info!(
            "Crash recovery successfully completed for transaction '{}'",
            active.id
        );

        Ok(Some(RecoveryOutcome {
            tx_id: active.id,
            recovered_status: TransactionStatus::RecoveredRollback,
            restored_files: restored,
        }))
    }
}
