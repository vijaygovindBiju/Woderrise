//! 1-Click LIFO rollback engine.
//!
//! Reverses committed dotfile transactions in linear Last-In, First-Out (LIFO) stack order.
//! Enforces dirty-state verification (`current_sha256 == post_sha256`), restores pre-install
//! backup snapshots, deletes newly created files, and prunes empty parent directories.

use camino::Utf8PathBuf;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use woderrise_core::error::{IoError, Result, TransactionError, WoderriseError};
use woderrise_core::models::{JournalEntry, OperationType, TransactionStatus};
use woderrise_core::paths::XdgPaths;

use crate::atomic::AtomicSwapper;
use crate::wal::JournalManager;

/// Summary report of a completed rollback operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackReport {
    pub tx_id: String,
    pub restored_files: Vec<Utf8PathBuf>,
    pub removed_files: Vec<Utf8PathBuf>,
    pub pruned_directories: Vec<Utf8PathBuf>,
}

/// 1-Click LIFO rollback engine.
pub struct RollbackEngine;

impl RollbackEngine {
    /// Validates whether any target files have been modified externally post-installation.
    pub fn verify_dirty_state(entry: &JournalEntry) -> Result<()> {
        for op in &entry.operations {
            if let Some(ref post_sha) = op.post_sha256 {
                if op.target_file.exists() && op.target_file.is_file() {
                    let mut file = File::open(&op.target_file).map_err(|source| {
                        WoderriseError::Io(IoError::Read {
                            path: op.target_file.clone(),
                            source,
                        })
                    })?;
                    let mut content = Vec::new();
                    file.read_to_end(&mut content).map_err(|source| {
                        WoderriseError::Io(IoError::Read {
                            path: op.target_file.clone(),
                            source,
                        })
                    })?;
                    let mut hasher = Sha256::new();
                    hasher.update(&content);
                    let actual_sha = format!("{:x}", hasher.finalize());

                    if &actual_sha != post_sha {
                        return Err(TransactionError::DirtyStateDetected {
                            file: op.target_file.clone(),
                            expected_sha256: post_sha.clone(),
                            actual_sha256: actual_sha,
                        }
                        .into());
                    }
                }
            }
        }
        Ok(())
    }

    /// Executes 1-click rollback on the specified transaction (or latest committed if `None`).
    ///
    /// Validates LIFO stack ordering, checks for dirty state unless `force_dirty` is true,
    /// restores files from snapshot backups, removes newly created files, prunes empty
    /// subdirectories, and updates the journal to `RolledBack`.
    pub fn execute_rollback(
        journal_mgr: &JournalManager,
        xdg: &XdgPaths,
        target_tx_id: Option<&str>,
        force_dirty: bool,
    ) -> Result<RollbackReport> {
        let _lock = journal_mgr.acquire_lock()?;
        let journal = journal_mgr.load_journal()?;

        // 1. Identify target transaction and verify LIFO order
        let tx_idx = match target_tx_id {
            Some(id) => {
                let idx = journal
                    .transactions
                    .iter()
                    .position(|t| t.id == id)
                    .ok_or_else(|| TransactionError::TransactionNotFound {
                        tx_id: id.to_string(),
                    })?;

                // Enforce LIFO: no newer committed transaction may exist after this one
                if let Some(top) = journal
                    .transactions
                    .iter()
                    .skip(idx + 1)
                    .rfind(|t| t.status == TransactionStatus::Committed)
                {
                    return Err(TransactionError::LifoViolation {
                        tx_id: id.to_string(),
                        top_tx_id: top.id.clone(),
                    }
                    .into());
                }
                idx
            }
            None => journal
                .transactions
                .iter()
                .rposition(|t| t.status == TransactionStatus::Committed)
                .ok_or_else(|| TransactionError::TransactionNotFound {
                    tx_id: "latest".to_string(),
                })?,
        };

        let entry = journal.transactions[tx_idx].clone();
        if entry.status != TransactionStatus::Committed {
            return Err(TransactionError::InvalidStateTransition {
                from: format!("{:?}", entry.status),
                to: "RollingBack".to_string(),
            }
            .into());
        }

        // 2. Dirty-state check
        if !force_dirty {
            Self::verify_dirty_state(&entry)?;
        }

        // 3. Mark WAL as RollingBack
        journal_mgr.begin_rolling_back(&entry)?;

        let component_root = xdg.component_root(entry.component);
        let mut restored = Vec::new();
        let mut removed = Vec::new();
        let mut pruned = Vec::new();

        // 4. Reverse operations in LIFO order
        for op in entry.operations.iter().rev() {
            match op.op_type {
                OperationType::Overwrite => {
                    if let Some(ref backup_file) = op.backup_file {
                        if backup_file.exists() {
                            AtomicSwapper::swap_file(backup_file, &op.target_file)?;
                            restored.push(op.target_file.clone());
                        }
                    }
                }
                OperationType::Create => {
                    if op.target_file.exists() {
                        let _ = fs::remove_file(&op.target_file);
                        removed.push(op.target_file.clone());

                        // Prune empty parent directories up to component root
                        let mut curr = op.target_file.parent();
                        while let Some(dir) = curr {
                            if dir == component_root || !dir.starts_with(&component_root) {
                                break;
                            }
                            if let Ok(mut entries) = fs::read_dir(dir) {
                                if entries.next().is_none() {
                                    if fs::remove_dir(dir).is_ok() {
                                        pruned.push(dir.to_path_buf());
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
                    // Try to restore disabled legacy file
                    let disabled_path =
                        Utf8PathBuf::from(format!("{}.disabled_by_woderrise", op.target_file));
                    if disabled_path.exists() {
                        let _ = fs::rename(&disabled_path, &op.target_file);
                        restored.push(op.target_file.clone());
                    } else if let Some(ref backup_file) = op.backup_file {
                        if backup_file.exists() {
                            let _ = AtomicSwapper::swap_file(backup_file, &op.target_file);
                            restored.push(op.target_file.clone());
                        }
                    }
                }
                OperationType::Delete => {
                    if let Some(ref backup_file) = op.backup_file {
                        if backup_file.exists() {
                            AtomicSwapper::swap_file(backup_file, &op.target_file)?;
                            restored.push(op.target_file.clone());
                        }
                    }
                }
            }
        }

        // 5. Update journal entry status to RolledBack and reset active_transaction
        let mut updated_journal = journal_mgr.load_journal()?;
        if let Some(t) = updated_journal
            .transactions
            .iter_mut()
            .find(|t| t.id == entry.id)
        {
            t.status = TransactionStatus::RolledBack;
        }
        updated_journal.active_transaction = None;
        journal_mgr.save_journal(&updated_journal)?;

        Ok(RollbackReport {
            tx_id: entry.id,
            restored_files: restored,
            removed_files: removed,
            pruned_directories: pruned,
        })
    }
}
