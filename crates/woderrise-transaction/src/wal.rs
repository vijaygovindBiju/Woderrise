//! Write-Ahead Logging (WAL) and persistent journal manager.
//!
//! Manages `journal.json` with advisory file locking via `fs2` and durable
//! atomic sibling file writes. Guarantees that any state modification is
//! recorded in the WAL before host configurations are changed.

use camino::{Utf8Path, Utf8PathBuf};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use woderrise_core::error::{Result, TransactionError, WoderriseError};
use woderrise_core::models::{
    ActiveTransaction, ConfigComponent, FileOperationRecord, Journal, JournalEntry,
    TransactionStatus,
};
use woderrise_core::paths::XdgPaths;
use woderrise_core::schema::{read_json_file, write_json_file_durably};

/// Manager for the Write-Ahead Log (WAL) and `journal.json`.
#[derive(Debug, Clone)]
pub struct JournalManager {
    pub journal_file: Utf8PathBuf,
    pub lock_file: Utf8PathBuf,
}

impl JournalManager {
    /// Creates a new JournalManager with paths resolved from XDG.
    pub fn new(xdg: &XdgPaths) -> Result<Self> {
        let data_dir = xdg.woderrise_data_dir();
        if !data_dir.exists() {
            fs::create_dir_all(&data_dir).ok();
        }

        Ok(Self {
            journal_file: xdg.journal_file(),
            lock_file: data_dir.join("woderrise.lock"),
        })
    }

    /// Acquires an exclusive advisory file lock to serialize multi-process modifications.
    pub fn acquire_lock(&self) -> Result<File> {
        let parent = self.lock_file.parent().unwrap_or(Utf8Path::new("."));
        if !parent.exists() {
            fs::create_dir_all(parent).ok();
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_file)
            .map_err(WoderriseError::RawIo)?;

        file.lock_exclusive().map_err(WoderriseError::RawIo)?;
        Ok(file)
    }

    /// Loads the current journal from disk, or returns an empty default journal.
    pub fn load_journal(&self) -> Result<Journal> {
        if !self.journal_file.exists() {
            return Ok(Journal::default());
        }

        let journal: Journal = read_json_file(&self.journal_file)?;
        journal.validate_wal_invariants().map_err(|e| {
            WoderriseError::Transaction(TransactionError::WalCommitFailed {
                tx_id: "load".to_string(),
                reason: e,
            })
        })?;

        Ok(journal)
    }

    /// Saves the journal durably to disk using atomic sibling replacement and directory fsync.
    pub fn save_journal(&self, journal: &Journal) -> Result<()> {
        journal.validate_wal_invariants().map_err(|e| {
            WoderriseError::Transaction(TransactionError::WalCommitFailed {
                tx_id: "save".to_string(),
                reason: e,
            })
        })?;
        write_json_file_durably(&self.journal_file, journal)
    }

    /// Commits an in-flight transaction with status `Applying` to the WAL.
    pub fn begin_applying(
        &self,
        tx_id: &str,
        component: ConfigComponent,
        source_repo: &str,
        source_commit: &str,
        backup_dir: &Utf8Path,
        operations: Vec<FileOperationRecord>,
    ) -> Result<()> {
        let mut journal = self.load_journal()?;
        if let Some(ref existing) = journal.active_transaction {
            if existing.status.is_in_flight() {
                return Err(TransactionError::TransactionInProgress {
                    active_tx_id: existing.id.clone(),
                }
                .into());
            }
        }

        let active = ActiveTransaction {
            id: tx_id.to_string(),
            timestamp: chrono::Utc::now(),
            component,
            status: TransactionStatus::Applying,
            source_repo: source_repo.to_string(),
            source_commit: source_commit.to_string(),
            backup_directory: backup_dir.to_path_buf(),
            operations,
        };

        journal.active_transaction = Some(active);
        self.save_journal(&journal)
    }

    /// Marks the active transaction as `RollingBack` in the WAL before executing rollback actions.
    pub fn begin_rolling_back(&self, entry: &JournalEntry) -> Result<()> {
        let mut journal = self.load_journal()?;

        let active = ActiveTransaction {
            id: entry.id.clone(),
            timestamp: chrono::Utc::now(),
            component: entry.component,
            status: TransactionStatus::RollingBack,
            source_repo: entry.source_repo.clone(),
            source_commit: entry.source_commit.clone(),
            backup_directory: entry.backup_directory.clone(),
            operations: entry.operations.clone(),
        };

        journal.active_transaction = Some(active);
        self.save_journal(&journal)
    }

    /// Finalizes a successful transaction: archives it as `Committed` in history
    /// and resets `active_transaction` to `None`.
    pub fn commit_transaction(&self, tx_id: &str) -> Result<JournalEntry> {
        let mut journal = self.load_journal()?;
        let active = match journal.active_transaction.take() {
            Some(a) if a.id == tx_id => a,
            Some(a) => {
                return Err(TransactionError::WalCommitFailed {
                    tx_id: tx_id.to_string(),
                    reason: format!(
                        "Active transaction ID mismatch: expected {}, got {}",
                        tx_id, a.id
                    ),
                }
                .into());
            }
            None => {
                return Err(TransactionError::TransactionNotFound {
                    tx_id: tx_id.to_string(),
                }
                .into());
            }
        };

        let entry = JournalEntry {
            id: active.id,
            timestamp: active.timestamp,
            component: active.component,
            status: TransactionStatus::Committed,
            source_repo: active.source_repo,
            source_commit: active.source_commit,
            backup_directory: active.backup_directory,
            operations: active.operations,
        };

        journal.transactions.push(entry.clone());
        self.save_journal(&journal)?;
        Ok(entry)
    }
}
