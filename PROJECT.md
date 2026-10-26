# Project: Woderrise Desktop Configuration Manager

## Architecture
Woderrise is organized as a multi-crate Rust workspace:
- **`crates/woderrise-core`**: Core domain logic, SQLite WAL journal database (`journal.db`), transaction manager, content-addressed backup manager (`~/.local/share/woderrise/backups/<sha256>`), package manifest parser, catalog indexer (`catalog.rs` with FTS5), and source-versus-target diff engine (`diff.rs`).
- **`crates/woderrise-gui`**: Polished Ratatui / Crossterm TUI application (`main.rs`, `lib.rs`). Presents Home/Package Browser, Search, Detail modals, Multi-file Diff Preview with planned backup locations, async progress indicators/modals, journal view, and keyboard navigation.
- **`crates/woderrise-cli`**: Command-line interface with subcommands for `install`, `rollback`, `list`, `lint`, `diff`, `search`, `info`, and `recover`.
- **`registry/`**: Package repository containing real verified packages (`registry/packages/com.catppuccin.waybar/1.0.0`) with cryptographically verified SHA-256 artifacts and genuine metadata.

Data Flow:
```
Registry (YAML/Artifacts)
         │
         ▼
Catalog Indexer (SQLite + FTS5) ───► GUI Package Browser / CLI Search
         │
         ▼
Diff Engine (InstallPlan & Unified Diff) ───► GUI Diff Modal / CLI Diff Preview
         │
         ▼
Installation Engine (Pre-backup ─► Atomic Replace ─► SQLite WAL Journal) ───► Progress Callback / UI
         │
         ▼
Target Filesystem (~/.config/...) + Backup Store (~/.local/share/woderrise/backups)
```

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | Real Registry Indexing & Ingestion | Index genuine packages from `registry/packages`, extract name, version, description, license, upstream repo, palette, compatibility without invented defaults | M1 | Survey / R1 |
| 2 | Core Diff & Impact Inspection Engine | Dedicated `InstallPlan` and text diff module in `woderrise-core` computing unified diff lines, created/modified actions, target paths, and planned backup destinations | M1 | Survey / R2 |
| 3 | Transactional Safety & Rollback Integrity | Atomic file swaps, pre-modification backups, cleanup of `installed_packages` table on rollback, rollback status set to `COMMITTED`, interrupted transaction recovery | M1 | Survey / R3 |
| 4 | Asynchronous Installation & Granular Progress | `InstallationEngine::install` progress observer callback with `InstallStage` lifecycle events, async channel streaming | M1 | Survey / R3 |
| 5 | Test Workspace Hygiene | Fix test harness directory allocation in `woderrise-core` to use `std::env::temp_dir()` instead of polluting crate root | M1 | Survey / R5 |
| 6 | Ratatui TUI Real Metadata Display | Display genuine package metadata (license, authors, upstream repository, and colorized palette swatches) in browser and detail views | M2 | Survey / R1 |
| 7 | TUI Multi-File Diff Preview & Backup Inspection | Interactive multi-file diff view with vertical scrolling, true unified/LCS diff markers, target destination display, and planned backup snapshot paths | M2 | Survey / R2 |
| 8 | TUI Asynchronous Progress Feedback | Render `app.status` and progress gauge/modal during asynchronous installations, refresh operations, and rollbacks | M2 | Survey / R3 |
| 9 | TUI Keyboard Navigation & Offline Resilience | Implement Ctrl+C exit, search across tabs, `KeyEventKind::Press` filter, panic recovery hook, and truthful offline status indicator | M2 | Survey / R4 |
| 10 | CLI Capabilities Expansion | Add `diff`, `search`, `info`, and `recover` subcommands to `woderrise-cli` with unit tests | M3 | Survey / R1, R2, R3 |
| 11 | Transaction Recovery Mechanism | Implement recovery execution for interrupted transactions (`PLANNED`, `PREPARED`, `EXECUTING`, `FAILED`) in core and UI/CLI | M3 | Survey / R3 |
| 12 | Requirement-Driven E2E Test Suite | Opaque-box E2E test runner and 4-tier test cases covering package discovery, diff preview, transactions, rollback, offline resilience, and CLI | M4 | Dual Track / Acceptance Criteria |
| 13 | Final 100% E2E Test Pass & Adversarial Hardening | Verification that 100% of E2E tests pass across all tiers, followed by Tier 5 adversarial coverage hardening | M4 | Final Milestone |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Core Engine & Diff/Transaction Infrastructure | Diff/Plan engine in `woderrise-core`, rollback state sync fix, progress observer, test hygiene | none | DONE |
| M2 | Ratatui TUI Views & Keyboard Interaction | Genuine metadata display, multi-file diff viewer, visible progress modal/status, keyboard navigation fixes in `woderrise-gui` | M1 | DONE |
| M3 | CLI Subcommands & Transaction Recovery | CLI commands (`diff`, `search`, `info`, `recover`), interrupted transaction recovery logic | M1 | DONE |
| M4 | E2E Testing Suite & Adversarial Hardening | Opaque-box E2E tests (Tiers 1-4), 100% pass verification, and Tier 5 adversarial testing | M1, M2, M3 | IN_PROGRESS |

## Interface Contracts

### `woderrise-core::diff` ↔ `woderrise-gui` / `woderrise-cli`
- Types:
  ```rust
  pub struct FileDiff {
      pub source_path: PathBuf,
      pub target_path: PathBuf,
      pub planned_backup_path: Option<PathBuf>,
      pub action: FileDiffAction, // Create, Modify, Unchanged
      pub lines: Vec<DiffLine>,
  }
  pub enum FileDiffAction {
      Create,
      Modify,
      Unchanged,
  }
  pub struct DiffLine {
      pub kind: DiffLineKind, // Context, Addition, Deletion
      pub left_line_no: Option<usize>,
      pub right_line_no: Option<usize>,
      pub text: String,
  }
  pub struct InstallPlan {
      pub package_id: String,
      pub version: String,
      pub file_diffs: Vec<FileDiff>,
      pub will_backup_count: usize,
  }
  ```
- Signatures:
  ```rust
  impl InstallationEngine {
      pub fn compute_plan(&self, package_dir: impl AsRef<Path>) -> Result<InstallPlan>;
  }
  ```

### `woderrise-core::InstallationEngine` ↔ Callers (GUI / CLI)
- Progress Callback:
  ```rust
  pub type ProgressCallback = Box<dyn Fn(InstallStage, &str, f32) + Send + Sync>;
  impl InstallationEngine {
      pub fn install_with_progress(
          &self,
          package_dir: impl AsRef<Path>,
          initiator: &str,
          progress: Option<ProgressCallback>,
      ) -> Result<String>;
  }
  ```

### Rollback State Contract
- When `rollback(txn_id)` executes:
  1. Files are reverted from backup snapshots.
  2. `installed_packages` entry for the package is deleted.
  3. New rollback transaction record is written with `status = 'COMMITTED'`.
  4. Original transaction status is updated to `ROLLED_BACK`.

## Code Layout
- `crates/woderrise-core/src/`:
  - `lib.rs`: Storage, TransactionManager, InstallationEngine, BackupManager
  - `diff.rs`: InstallPlan, unified text diff computation, backup path prediction
  - `catalog.rs`: Registry importer, manifest models, FTS5 catalog
- `crates/woderrise-gui/src/`:
  - `main.rs`: Ratatui application state, event loop, view rendering
  - `lib.rs`: Reusable GUI models, helper utilities
- `crates/woderrise-cli/src/`:
  - `main.rs`: CommandRunner, CLI argument parsing, subcommands
- `tests/e2e/`:
  - E2E opaque-box test runner and test cases (Tiers 1-4)
