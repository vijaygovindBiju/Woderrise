//! Target mapping, conflict detection, and verification report models.

use super::component::ConfigComponent;
use super::manifest::SecurityAuditReport;
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

/// Conflict classification of a target file against the active local filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStatus {
    /// File does not exist locally; will be created fresh.
    NewFile,
    /// File exists locally with identical SHA-256; no overwrite needed.
    Unmodified,
    /// File exists locally with differing content; pre-install backup required.
    OverwritingExisting,
    /// File exists as a symlink (GNU Stow / Chezmoi); requires canonical resolution.
    ExistingSymlink,
    /// Target is hardware-bound (e.g. monitors.conf) and preserved by default.
    HardwareProtected,
}

/// Mapping connecting a staged repository file to its local target destination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetMapping {
    pub component: ConfigComponent,
    pub relative_path: Utf8PathBuf,
    pub blob_sha: String,
    pub target_destination: Utf8PathBuf,
    /// Git Tree mode attribute ('100644' regular, '100755' executable).
    pub git_file_mode: String,
    pub expected_sha256: String,
    pub is_hardware_protected: bool,
    pub conflict_status: ConflictStatus,
}

/// Severity level of a security audit message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditLevel {
    Info,
    Warning,
    Violation,
}

/// An individual audit message logged during security inspection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditMessage {
    pub level: AuditLevel,
    pub file: Utf8PathBuf,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub line: Option<usize>,
    pub message: String,
}

/// Notice indicating a hardware-bound file that is preserved by default.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareConflictNotice {
    pub file_path: Utf8PathBuf,
    pub description: String,
    pub preserved_by_default: bool,
}

/// Overall pre-flight verification report generated after staging and security audit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationReport {
    pub transaction_id: String,
    pub component: ConfigComponent,
    pub total_files: usize,
    pub total_bytes: u64,
    pub target_mappings: Vec<TargetMapping>,
    pub security_audit: SecurityAuditReport,
    pub hardware_conflicts: Vec<HardwareConflictNotice>,
    pub passed: bool,
}
