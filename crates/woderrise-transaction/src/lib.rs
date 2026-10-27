//! Transaction lifecycle, staging sandbox, WAL, and rollback engine for Woderrise.
//!
//! Enforces the 4-phase atomic installation pipeline with Write-Ahead Logging (WAL),
//! pre-installation backup snapshots, EXDEV-safe sibling temporary swaps with fsync,
//! 1-click LIFO rollback with dirty-state verification, and startup crash recovery.

pub mod atomic;
pub mod audit;
pub mod backup;
pub mod engine;
pub mod recovery;
pub mod rollback;
pub mod staging;
pub mod wal;

pub use atomic::AtomicSwapper;
pub use audit::{SecurityAuditor, MAX_ASSET_FILE_SIZE, MAX_CONFIG_FILE_SIZE};
pub use backup::BackupSnapshotEngine;
pub use engine::TransactionEngine;
pub use recovery::{RecoveryOutcome, StartupRecovery};
pub use rollback::{RollbackEngine, RollbackReport};
pub use staging::StagingWorkspace;
pub use wal::JournalManager;

/// Re-export core models and errors for convenience.
pub use woderrise_core::error::{Result, TransactionError, WoderriseError};
pub use woderrise_core::models::{
    ActiveTransaction, ConfigComponent, ConflictStatus, FileOperationRecord, Journal, JournalEntry,
    OperationType, StagedFile, StagingManifest, TargetMapping, TransactionStatus,
    VerificationReport,
};
pub use woderrise_core::paths::XdgPaths;

/// Returns the crate version string.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
