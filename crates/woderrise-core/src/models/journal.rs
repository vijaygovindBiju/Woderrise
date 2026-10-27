//! Journal and WAL models for journal.json.

use super::component::ConfigComponent;
use camino::Utf8PathBuf;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const DEFAULT_JOURNAL_SCHEMA: &str = "https://woderrise.dev/schemas/journal.v1.json";
pub const DEFAULT_JOURNAL_VERSION: u32 = 1;

fn default_journal_schema() -> Option<String> {
    Some(DEFAULT_JOURNAL_SCHEMA.to_string())
}

fn default_journal_version() -> u32 {
    DEFAULT_JOURNAL_VERSION
}

/// Status of an active or committed transaction in the WAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionStatus {
    Staged,
    Applying,
    Committed,
    RollingBack,
    RolledBack,
    RecoveredRollback,
    Failed,
}

impl TransactionStatus {
    /// Check whether a transition from `self` to `next` is valid under the WAL state machine.
    pub fn can_transition_to(&self, next: TransactionStatus) -> bool {
        match self {
            TransactionStatus::Staged => {
                matches!(
                    next,
                    TransactionStatus::Applying | TransactionStatus::Failed
                )
            }
            TransactionStatus::Applying => {
                matches!(
                    next,
                    TransactionStatus::Committed
                        | TransactionStatus::Failed
                        | TransactionStatus::RollingBack
                        | TransactionStatus::RecoveredRollback
                )
            }
            TransactionStatus::Committed => {
                matches!(next, TransactionStatus::RollingBack)
            }
            TransactionStatus::RollingBack => {
                matches!(
                    next,
                    TransactionStatus::RolledBack
                        | TransactionStatus::Failed
                        | TransactionStatus::RecoveredRollback
                )
            }
            TransactionStatus::RolledBack
            | TransactionStatus::RecoveredRollback
            | TransactionStatus::Failed => false,
        }
    }

    /// Returns true if this status represents an in-flight (non-terminal) transaction state.
    pub fn is_in_flight(&self) -> bool {
        matches!(
            self,
            TransactionStatus::Staged
                | TransactionStatus::Applying
                | TransactionStatus::RollingBack
        )
    }

    /// Returns true if this status represents a terminal state.
    pub fn is_terminal(&self) -> bool {
        !self.is_in_flight()
    }
}

/// Alias for transaction status in WAL state transitions.
pub type ActiveTransactionState = TransactionStatus;

/// Kinds of file mutations recorded in the transaction journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    Overwrite,
    Create,
    Delete,
    DeactivateLegacy,
}

/// Alias for operation type.
pub type FileOperationKind = OperationType;

/// Individual file operation record tracking pre/post hashes and backup paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileOperationRecord {
    #[serde(rename = "type", alias = "operation")]
    pub op_type: OperationType,
    #[serde(alias = "path")]
    pub target_file: Utf8PathBuf,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        alias = "original_backup_path"
    )]
    pub backup_file: Option<Utf8PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default, alias = "new_sha256")]
    pub post_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mode: Option<String>,
}

/// Alias for FileOperationRecord.
pub type FileOperation = FileOperationRecord;
/// Alias for FileOperationRecord.
pub type ModifiedFileEntry = FileOperationRecord;

/// In-flight transaction tracked in the Write-Ahead Log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveTransaction {
    #[serde(alias = "tx_id")]
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub component: ConfigComponent,
    pub status: TransactionStatus,
    #[serde(alias = "source_repository")]
    pub source_repo: String,
    #[serde(alias = "source_commit_sha")]
    pub source_commit: String,
    #[serde(alias = "backup_dir", alias = "backups_snapshot_dir")]
    pub backup_directory: Utf8PathBuf,
    #[serde(alias = "modified_files")]
    pub operations: Vec<FileOperationRecord>,
}

impl ActiveTransaction {
    /// Transition the active transaction to a new status according to WAL state machine rules.
    pub fn transition_to(&mut self, next: TransactionStatus) -> crate::error::Result<()> {
        if self.status.can_transition_to(next) {
            self.status = next;
            Ok(())
        } else {
            Err(crate::error::TransactionError::InvalidStateTransition {
                from: format!("{:?}", self.status),
                to: format!("{:?}", next),
            }
            .into())
        }
    }
}

/// Historical transaction entry in the journal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEntry {
    #[serde(alias = "tx_id")]
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub component: ConfigComponent,
    pub status: TransactionStatus,
    #[serde(alias = "source_repository")]
    pub source_repo: String,
    #[serde(alias = "source_commit_sha")]
    pub source_commit: String,
    #[serde(alias = "backup_dir", alias = "backups_snapshot_dir")]
    pub backup_directory: Utf8PathBuf,
    #[serde(alias = "modified_files")]
    pub operations: Vec<FileOperationRecord>,
}

/// Persistent transaction journal storing active WAL and transaction history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Journal {
    #[serde(
        rename = "$schema",
        default = "default_journal_schema",
        skip_serializing_if = "Option::is_none"
    )]
    pub schema: Option<String>,
    #[serde(default = "default_journal_version")]
    pub version: u32,
    #[serde(default)]
    pub active_transaction: Option<ActiveTransaction>,
    #[serde(default)]
    pub transactions: Vec<JournalEntry>,
}

impl Journal {
    /// Validate WAL invariants:
    /// If `active_transaction` is present, it must be in an in-flight state (`Staged`, `Applying`, or `RollingBack`).
    /// Terminal states (`Committed`, `RolledBack`, `RecoveredRollback`, `Failed`) must not be held in `active_transaction`.
    pub fn validate_wal_invariants(&self) -> Result<(), String> {
        if let Some(ref active) = self.active_transaction {
            if active.status.is_terminal() {
                return Err(format!(
                    "Active transaction '{}' has terminal status '{:?}'; terminal transactions must be archived in history and active_transaction cleared",
                    active.id, active.status
                ));
            }
        }
        Ok(())
    }
}

impl Default for Journal {
    fn default() -> Self {
        Self {
            schema: default_journal_schema(),
            version: default_journal_version(),
            active_transaction: None,
            transactions: Vec::new(),
        }
    }
}
