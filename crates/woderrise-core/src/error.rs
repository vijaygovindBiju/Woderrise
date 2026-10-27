//! Unified error hierarchy for Woderrise.

use camino::Utf8PathBuf;
use std::io;
use thiserror::Error;

/// Top-level unified error type for Woderrise operations.
#[derive(Debug, Error)]
pub enum WoderriseError {
    #[error("Domain error: {0}")]
    Domain(#[from] DomainError),

    #[error("Path error: {0}")]
    Path(#[from] PathError),

    #[error("I/O error: {0}")]
    Io(#[from] IoError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] SerializationError),

    #[error("Transaction error: {0}")]
    Transaction(#[from] TransactionError),

    #[error("Security violation: {0}")]
    Security(#[from] SecurityError),

    #[error("GitHub API error ({status}): {message}")]
    GitHubApi {
        status: u16,
        message: String,
        reset_epoch: Option<u64>,
    },

    #[error("Rate limit exceeded. Resets at {reset_time}")]
    RateLimitExceeded {
        reset_time: chrono::DateTime<chrono::Utc>,
    },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Direct I/O failure: {0}")]
    RawIo(#[from] io::Error),
}

/// Domain-specific errors relating to package and component lifecycle.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("Package '{id}' was not found in catalog")]
    PackageNotFound { id: String },

    #[error("Component '{component}' not found in package '{package_id}'")]
    ComponentNotFound {
        component: String,
        package_id: String,
    },

    #[error("Invalid package metadata for field '{field}': {reason}")]
    InvalidMetadata { field: String, reason: String },

    #[error("Incompatible version: required '{required}', actual '{actual}'")]
    IncompatibleVersion { required: String, actual: String },

    #[error("Missing entrypoint '{expected}' for component '{component}'")]
    MissingEntrypoint { component: String, expected: String },

    #[error("Syntax validation failed for {component} in '{file}': {details}")]
    SyntaxValidation {
        component: String,
        file: String,
        details: String,
    },

    #[error("Hardware configuration conflict in '{file}': {reason}")]
    HardwareConflict { file: String, reason: String },
}

/// Errors occurring during path resolution, traversal validation, or sanitization.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathError {
    #[error("Path traversal attempted in '{path}': forbidden token '{token}'")]
    PathTraversalAttempted { path: String, token: String },

    #[error("Path contains null byte: '{path}'")]
    ContainsNullByte { path: String },

    #[error("Absolute path '{path}' is forbidden for relative component dotfile")]
    AbsolutePathNotAllowed { path: String },

    #[error("Path '{path}' escapes designated component root '{allowed_root}'")]
    ComponentRootEscape {
        path: Utf8PathBuf,
        allowed_root: Utf8PathBuf,
    },

    #[error("Forbidden symlink at '{path}' pointing to '{target}' (CVE-2021-21300 protection)")]
    ForbiddenSymlink {
        path: Utf8PathBuf,
        target: Utf8PathBuf,
    },

    #[error("Non-UTF8 path encountered: '{path}'")]
    NonUtf8Path { path: String },

    #[error("Base XDG directory '{dir_type}' could not be resolved")]
    BaseDirectoryNotFound { dir_type: String },
}

/// Rich I/O error providing path context for filesystem failures.
#[derive(Debug, Error)]
pub enum IoError {
    #[error("Failed to read file at '{path}': {source}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Failed to write file at '{path}': {source}")]
    Write {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Failed to create directory at '{path}': {source}")]
    CreateDir {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Failed to remove file or directory at '{path}': {source}")]
    Remove {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Failed to rename '{from}' to '{to}': {source}")]
    Rename {
        from: Utf8PathBuf,
        to: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Failed to copy '{from}' to '{to}': {source}")]
    Copy {
        from: Utf8PathBuf,
        to: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Failed to fsync file or directory at '{path}': {source}")]
    Fsync {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Failed to read metadata for '{path}': {source}")]
    Metadata {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Contextual I/O failure: {0}")]
    Other(#[source] io::Error),
}

/// Errors related to serialization, deserialization, or schema compliance.
#[derive(Debug, Error)]
pub enum SerializationError {
    #[error("JSON serialization failed: {source}")]
    JsonSerialize {
        #[source]
        source: serde_json::Error,
    },

    #[error("JSON deserialization failed from '{}': {source}", match path { Some(p) => p.as_str(), None => "<unknown>" })]
    JsonDeserialize {
        path: Option<Utf8PathBuf>,
        #[source]
        source: serde_json::Error,
    },

    #[error("Schema version mismatch: expected '{expected}', found '{found}'")]
    SchemaVersionMismatch { expected: String, found: String },

    #[error("Invalid schema format: {0}")]
    InvalidFormat(String),
}

impl From<serde_json::Error> for SerializationError {
    fn from(source: serde_json::Error) -> Self {
        SerializationError::JsonSerialize { source }
    }
}

/// Errors encountered during the 4-phase transaction lifecycle or WAL recovery.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransactionError {
    #[error("Invalid state transition from '{from}' to '{to}'")]
    InvalidStateTransition { from: String, to: String },

    #[error("Active transaction '{active_tx_id}' is already in progress")]
    TransactionInProgress { active_tx_id: String },

    #[error("Transaction '{tx_id}' was not found in journal")]
    TransactionNotFound { tx_id: String },

    #[error("Staging phase failed for '{component}': {reason}")]
    StagingFailed { component: String, reason: String },

    #[error("Pre-installation backup failed for '{path}' in tx '{tx_id}': {reason}")]
    BackupFailed {
        tx_id: String,
        path: Utf8PathBuf,
        reason: String,
    },

    #[error("Write-Ahead Log commit failed for tx '{tx_id}': {reason}")]
    WalCommitFailed { tx_id: String, reason: String },

    #[error("Atomic sibling swap failed for target '{file}': {reason}")]
    AtomicSwapFailed { file: Utf8PathBuf, reason: String },

    #[error("Dirty state detected in '{file}': expected SHA-256 '{expected_sha256}', found '{actual_sha256}'")]
    DirtyStateDetected {
        file: Utf8PathBuf,
        expected_sha256: String,
        actual_sha256: String,
    },

    #[error("LIFO order violation: cannot rollback transaction '{tx_id}' because newer transaction '{top_tx_id}' exists")]
    LifoViolation { tx_id: String, top_tx_id: String },

    #[error("Crash recovery failed for tx '{tx_id}': {reason}")]
    RecoveryFailed { tx_id: String, reason: String },

    #[error("Rollback failed for tx '{tx_id}': {reason}")]
    RollbackFailed { tx_id: String, reason: String },
}

/// Security violations detected during staging audits.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SecurityError {
    #[error("Untrusted executable script blocked at '{path}' (type: {script_type})")]
    ScriptExecutionBlocked {
        path: Utf8PathBuf,
        script_type: String,
    },

    #[error("Interpreter bypass detected in command '{command}'")]
    InterpreterBypassDetected { command: String },

    #[error("Procedural execution directive '{directive}' neutralized in '{file}:{line}'")]
    ProceduralDirectiveNeutralized {
        directive: String,
        file: String,
        line: usize,
    },

    #[error("Resource limit exceeded for '{resource}': limit {limit_bytes} bytes, got {actual_bytes} bytes")]
    ResourceLimitExceeded {
        resource: String,
        limit_bytes: u64,
        actual_bytes: u64,
    },
}

pub type Result<T> = std::result::Result<T, WoderriseError>;
