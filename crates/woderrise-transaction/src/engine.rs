//! Unified TransactionEngine coordinating the complete 4-phase transaction lifecycle.
//!
//! Provides the top-level API for staging, pre-flight security auditing,
//! snapshot backups, Write-Ahead Logging, atomic file replacements,
//! 1-click LIFO rollback, and startup crash recovery.

use woderrise_core::error::Result;
use woderrise_core::models::{
    ActiveTransaction, ConfigComponent, Journal, JournalEntry, OperationType, StagedFile,
    StagingManifest, VerificationReport,
};
use woderrise_core::paths::XdgPaths;
use woderrise_detect::TreeEntry;

use crate::atomic::AtomicSwapper;
use crate::audit::SecurityAuditor;
use crate::backup::BackupSnapshotEngine;
use crate::recovery::{RecoveryOutcome, StartupRecovery};
use crate::rollback::{RollbackEngine, RollbackReport};
use crate::staging::StagingWorkspace;
use crate::wal::JournalManager;

/// High-level engine orchestrating dotfile transactions, staging, atomic replacement, and rollback.
#[derive(Debug, Clone)]
pub struct TransactionEngine {
    pub xdg: XdgPaths,
    pub journal_mgr: JournalManager,
}

impl TransactionEngine {
    /// Creates a new TransactionEngine bound to the provided XDG paths.
    pub fn new(xdg: XdgPaths) -> Result<Self> {
        let journal_mgr = JournalManager::new(&xdg)?;
        Ok(Self { xdg, journal_mgr })
    }

    /// Creates a TransactionEngine using default environment XDG paths.
    pub fn from_env() -> Result<Self> {
        let xdg = XdgPaths::from_env()?;
        Self::new(xdg)
    }

    /// Phase 1: Pre-flight security audit and sandboxed staging.
    ///
    /// Downloads/stages incoming files into an isolated sandbox, audits file permissions,
    /// Git modes, script extensions, path safety, and resource limits, and writes the
    /// staging manifest.
    pub fn stage_transaction(
        &self,
        tx_id: &str,
        component: ConfigComponent,
        repo: &str,
        commit: &str,
        entries: &[(TreeEntry, Vec<u8>)],
    ) -> Result<VerificationReport> {
        let workspace = StagingWorkspace::new(&self.xdg, tx_id)?;
        let mut target_mappings = Vec::with_capacity(entries.len());
        let mut staged_files = Vec::with_capacity(entries.len());
        let mut total_bytes = 0u64;

        let tree_entries: Vec<TreeEntry> = entries.iter().map(|(e, _)| e.clone()).collect();
        let (audited_mappings, mut audit_report, hardware_notices) =
            SecurityAuditor::audit_entries(&tree_entries, component, &self.xdg)?;

        for ((tree_entry, content), mut mapping) in entries.iter().zip(audited_mappings) {
            let staged = workspace.stage_file(
                tree_entry.utf8_path(),
                content,
                Some(mapping.target_destination.clone()),
                &tree_entry.mode,
                Some(tree_entry.sha.clone()),
            )?;

            mapping.expected_sha256 = staged.sha256.clone();
            total_bytes += staged.file_size;
            staged_files.push(staged);
            target_mappings.push(mapping);
        }

        audit_report.passed = true;

        let manifest = StagingManifest {
            schema: None,
            version: 1,
            transaction_id: tx_id.to_string(),
            component,
            source_repository: repo.to_string(),
            source_commit_sha: commit.to_string(),
            created_at: chrono::Utc::now(),
            staging_dir: workspace.root_dir.clone(),
            target_dir: self.xdg.component_root(component),
            files: staged_files,
            security_audit: audit_report.clone(),
        };

        workspace.commit_manifest(&manifest)?;

        Ok(VerificationReport {
            transaction_id: tx_id.to_string(),
            component,
            total_files: target_mappings.len(),
            total_bytes,
            target_mappings,
            security_audit: audit_report,
            hardware_conflicts: hardware_notices,
            passed: true,
        })
    }

    /// Phases 2, 3, & 4: Backup Snapshot, WAL Applying, Atomic Swap, and Commit.
    ///
    /// Safely applies a staged transaction:
    /// - Creates a backup snapshot in `$XDG_STATE_HOME/woderrise/backups/<tx_id>/`
    /// - Writes the WAL active transaction with status `Applying`
    /// - Atomically swaps staged files into target destinations (EXDEV-safe with parent fsync)
    /// - Deactivates legacy configs if modern Lua alternatives are installed
    /// - Commits the transaction in the WAL and removes the staging sandbox
    pub fn apply_transaction(
        &self,
        tx_id: &str,
        component: ConfigComponent,
        source_repo: &str,
        source_commit: &str,
        staged_files: &[StagedFile],
    ) -> Result<JournalEntry> {
        let _lock = self.journal_mgr.acquire_lock()?;
        let staging = StagingWorkspace::new(&self.xdg, tx_id)?;

        // Phase 2: Backup snapshot
        let backup = BackupSnapshotEngine::new(&self.xdg, tx_id)?;
        let operations = backup.create_snapshot(staged_files, component, &self.xdg)?;

        // Phase 3: WAL commit to Applying
        self.journal_mgr.begin_applying(
            tx_id,
            component,
            source_repo,
            source_commit,
            &backup.backup_dir,
            operations.clone(),
        )?;

        // Phase 4: Atomic file replacement
        for file in staged_files {
            let staged_source = staging.files_dir.join(&file.relative_path);
            let target_dest = match &file.target_destination {
                Some(p) => p.clone(),
                None => self
                    .xdg
                    .resolve_destination(component, &file.relative_path)?,
            };

            AtomicSwapper::swap_file(&staged_source, &target_dest)?;
        }

        // Deactivate any legacy files specified in operations
        for op in &operations {
            if op.op_type == OperationType::DeactivateLegacy {
                AtomicSwapper::deactivate_legacy_file(&op.target_file)?;
            }
        }

        // Commit transaction in WAL
        let entry = self.journal_mgr.commit_transaction(tx_id)?;

        // Clean up ephemeral staging sandbox
        let _ = staging.cleanup();

        Ok(entry)
    }

    /// Alias for `apply_transaction`.
    pub fn execute_installation(
        &self,
        tx_id: &str,
        component: ConfigComponent,
        source_repo: &str,
        source_commit: &str,
        staged_files: &[StagedFile],
    ) -> Result<JournalEntry> {
        self.apply_transaction(tx_id, component, source_repo, source_commit, staged_files)
    }

    /// Reverses the latest committed transaction using 1-Click LIFO rollback.
    pub fn rollback_latest(&self, force_dirty: bool) -> Result<RollbackReport> {
        RollbackEngine::execute_rollback(&self.journal_mgr, &self.xdg, None, force_dirty)
    }

    /// Reverses a specific committed transaction, enforcing linear LIFO ordering.
    pub fn rollback_transaction(&self, tx_id: &str, force_dirty: bool) -> Result<RollbackReport> {
        RollbackEngine::execute_rollback(&self.journal_mgr, &self.xdg, Some(tx_id), force_dirty)
    }

    /// Checks for and recovers any interrupted in-flight transaction on application startup.
    pub fn recover_on_startup(&self) -> Result<Option<RecoveryOutcome>> {
        StartupRecovery::scan_and_recover(&self.journal_mgr, &self.xdg)
    }

    /// Loads the entire journal.
    pub fn load_journal(&self) -> Result<Journal> {
        self.journal_mgr.load_journal()
    }

    /// Returns historical transaction records.
    pub fn get_history(&self) -> Result<Vec<JournalEntry>> {
        let journal = self.load_journal()?;
        Ok(journal.transactions)
    }

    /// Returns the currently active in-flight transaction if any.
    pub fn get_active_transaction(&self) -> Result<Option<ActiveTransaction>> {
        let journal = self.load_journal()?;
        Ok(journal.active_transaction)
    }
}
