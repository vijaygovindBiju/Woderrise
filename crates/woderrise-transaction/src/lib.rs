//! Transaction lifecycle, staging sandbox, WAL, and rollback engine for Woderrise.
//!
//! Enforces the 4-phase atomic installation pipeline with Write-Ahead Logging (WAL),
//! pre-installation backups, sibling temporary swaps with fsync, and LIFO rollback.

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
