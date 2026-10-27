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
    #[serde(skip_serializing_if = "Option::is_none", default, alias = "original_backup_path")]
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
    #[serde(rename = "$schema", default = "default_journal_schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default = "default_journal_version")]
    pub version: u32,
    #[serde(default)]
    pub active_transaction: Option<ActiveTransaction>,
    #[serde(default)]
    pub transactions: Vec<JournalEntry>,
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
