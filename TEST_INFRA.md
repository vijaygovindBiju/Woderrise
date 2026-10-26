# Woderrise End-to-End Test Infrastructure Specification (`TEST_INFRA.md`)

## 1. Test Architecture & Runner Invocation

### Architecture Overview
Woderrise uses an **opaque-box, requirement-driven** End-to-End (E2E) testing architecture. The test suite interacts with the system exclusively through its compiled CLI executable (`woderrise`, via `CARGO_BIN_EXE_woderrise`), real filesystem boundaries, and real SQLite Write-Ahead Logging (WAL) databases.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                           E2E Test Runner                                 │
│  (tests/e2e_tests.rs -> tests/e2e/*.rs via `cargo test --test e2e_tests`) │
└──────────────────────┬────────────────────────────────────────────────────┘
                       │ Spawns subprocess with isolated env
                       ▼
┌───────────────────────────────────────────────────────────────────────────┐
│                       woderrise CLI Executable                            │
│                 (`target/debug/woderrise` binary)                         │
└──────────────┬─────────────────────────────┬──────────────────────────────┘
               │                             │
               ▼                             ▼
┌─────────────────────────────┐┌────────────────────────────────────────────┐
│   Isolated Temp Filesystem  ││           SQLite WAL Database              │
│ - $HOME (~/.config/...)     ││ - journal.db (`transactions`)              │
│ - WODERRISE_DATA_DIR        ││ - `installed_packages` table               │
│ - Content-addressed backups ││ - `catalog` FTS5 index                     │
└─────────────────────────────┘└────────────────────────────────────────────┘
```

### Runner Invocation
The test suite can be executed using standard Cargo commands:
```bash
# Run all E2E tests
cargo test --test e2e_tests

# Run all workspace tests (unit, integration, stress, and E2E)
cargo test --workspace

# Run a specific test tier
cargo test --test e2e_tests tier1_feature_coverage
cargo test --test e2e_tests tier2_boundary_corner
cargo test --test e2e_tests tier3_cross_feature
cargo test --test e2e_tests tier4_real_workload

# Run with verbose test output
cargo test --test e2e_tests -- --nocapture
```

---

## 2. Test Philosophy & Principles

1. **Opaque-Box Verification**: Tests interact strictly via public interfaces—CLI arguments, standard input/output (`stdout`/`stderr`), exit status codes (0 for success, 1 for runtime failure, 2 for CLI usage errors), and observable disk/database mutations.
2. **Requirement-Driven**: Every assertion maps directly to documented specifications in `PROJECT.md` and `ORIGINAL_REQUEST.md`.
3. **Zero Mock Fixtures**: The test suite operates on real verified registry artifacts (`registry/packages/com.catppuccin.waybar/1.0.0`), real SQLite transactions, and genuine SHA-256 content-addressed file hashes. No mock data or artificial shims are used.
4. **Hermetic Test Isolation & Hygiene**: Every test instantiates a dedicated `TestEnvironment` allocated inside `std::env::temp_dir()`. Environment variables (`HOME`, `WODERRISE_DATA_DIR`, `WODERRISE_REGISTRY_DIR`) are sandboxed per test process. Dropping the guard RAII cleanups the sandbox immediately, leaving zero residual files in the workspace or user home directory.
5. **Truthful Error Handling**: Error messages must adhere to the standard prefix `woderrise: <error>`. Missing packages, path traversal attacks, and permission errors must fail safely and deterministically without unhandled panics.

---

## 3. Feature Inventory Coverage Mapping

All 13 features defined in `PROJECT.md` are covered across the 4 testing tiers:

| # | Feature Name | Description | Test Tiers | Primary Test Functions |
|---|--------------|-------------|------------|------------------------|
| **F1** | Real Registry Indexing & Ingestion | Index genuine packages from `registry/packages`, extract name, version, license, upstream repo, palette without defaults | Tier 1, Tier 3, Tier 4 | `test_tier1_f1_registry_indexing_and_search`, `test_tier4_real_world_catppuccin_waybar_complete_lifecycle` |
| **F2** | Core Diff & Impact Inspection Engine | Dedicated `InstallPlan` and Myers unified text diff computing unified diff lines, `[CREATE]`, `[MODIFY]`, target paths, and planned backup destinations | Tier 1, Tier 3, Tier 4 | `test_tier1_f2_diff_clean_install_create_action`, `test_tier1_f2_diff_summary_mode`, `test_tier1_f7_diff_modify_and_backup_destination_prediction` |
| **F3** | Transactional Safety & Rollback Integrity | Atomic file swaps, pre-modification backups, cleanup of `installed_packages` table on rollback, rollback status set to `COMMITTED`, interrupted transaction recovery | Tier 1, Tier 3, Tier 4 | `test_tier1_f3_transactional_install_and_journal_record`, `test_tier1_f3_safe_rollback_restores_clean_filesystem`, `test_tier3_double_rollback_prevention` |
| **F4** | Asynchronous Installation & Granular Progress | `InstallationEngine::install` progress observer callback with `InstallStage` lifecycle events, async channel streaming | Tier 1, Tier 3 | `test_tier1_f4_progress_reporting_and_status`, `test_tier3_lifecycle_search_info_diff_install_diff_rollback` |
| **F5** | Test Workspace Hygiene | Test harness allocation in `std::env::temp_dir()` rather than repository root | Tier 1 | `test_tier1_f5_test_workspace_hygiene` |
| **F6** | Ratatui TUI Real Metadata Display | Genuine package metadata display (license, authors, upstream repository, colorized palette swatches) | Tier 1, Tier 4 | `test_tier1_f6_info_metadata_and_provenance_display`, `test_tier4_real_world_catppuccin_waybar_complete_lifecycle` |
| **F7** | TUI Multi-File Diff Preview & Backup Inspection | True unified/LCS diff markers, target destination display, and planned backup snapshot paths | Tier 1, Tier 3, Tier 4 | `test_tier1_f7_diff_modify_and_backup_destination_prediction`, `test_tier4_real_world_catppuccin_waybar_complete_lifecycle` |
| **F8** | TUI Asynchronous Progress Feedback | Status tracking and progress gauge representation during asynchronous operations | Tier 1, Tier 3 | `test_tier1_f4_progress_reporting_and_status` |
| **F9** | TUI Keyboard Navigation & Offline Resilience | Panic recovery, truthful offline status indicator, and functioning against local cached data without network | Tier 1 | `test_tier1_f8_f9_offline_resilience_and_no_network_dependence` |
| **F10** | CLI Capabilities Expansion | Subcommands `diff`, `search`, `info`, `recover` with proper exit codes (0, 1, 2) | Tier 1, Tier 2 | `test_tier1_f10_cli_capabilities_matrix`, `test_tier2_cli_standard_exit_codes_matrix` |
| **F11** | Transaction Recovery Mechanism | Recovery execution for interrupted transactions (`PLANNED`, `PREPARED`, `EXECUTING`, `FAILED`) | Tier 1, Tier 2, Tier 3 | `test_tier1_f11_recover_interrupted_transactions`, `test_tier2_recovery_multi_state_selectivity`, `test_tier3_interrupted_install_recovery_then_reinstall` |
| **F12** | Requirement-Driven E2E Test Suite | Opaque-box E2E test runner and 4-tier test architecture | All Tiers | Entire `tests/e2e_tests.rs` test suite |
| **F13** | Final 100% E2E Test Pass & Adversarial Hardening | 100% pass rate across workspace tests, zero clippy warnings, adversarial boundary coverage | All Tiers | All 25+ E2E integration tests |

---

## 4. Test Methodology

The E2E test suite applies four systematic testing methodologies:

### A. Category-Partition Method
Input parameters and CLI options are partitioned into equivalence classes:
- **Subcommands**: `{diff, search, info, recover, install, rollback, list, lint}`
- **Package Specifiers**: `{local directory, local manifest path, package_id, package_id@version, non-existent ID, malformed ID}`
- **Flags**: `{--summary, --no-color, --limit <n>, --force, default}`
- **Exit Status**: `{0: SUCCESS, 1: RUNTIME_FAILURE, 2: CLAP_USAGE_ERROR}`

### B. Boundary Value Analysis (BVA)
Tests probe extreme limits and edge conditions:
- **Search Queries**: Empty string `""`, single character, whitespace, 1024-character strings.
- **Search Limits**: `--limit 0`, `--limit 1`, `--limit 50` (default), `--limit 1000`.
- **FTS5 Syntax Stress**: FTS5 special characters (`*`, `"`, `'`, `OR`, `AND`, `NOT`, `( )`, `^`, `:`, `{ }`, `%`, `;`).
- **Filesystem Boundaries**: Missing source files, read-only permissions (`chmod 0444`), non-existent directory hierarchies, and path traversal strings (`../../etc/shadow`).

### C. Pairwise & Combinatorial Testing
State transitions across dependent features are exercised in multi-step sequences:
- `search` ➔ `info` ➔ `diff` ➔ `install` ➔ `diff` ➔ `rollback` ➔ `search`
- `interrupted install (PLANNED/PREPARED/EXECUTING)` ➔ `recover` ➔ `reinstall` ➔ `diff`
- `install v1` ➔ `upgrade v2` ➔ `rollback v2` ➔ `verify v1 restored`
- `install` ➔ `rollback` ➔ `second rollback attempt (rejected)`

### D. Real-World Application Workload Scenarios
Validates genuine desktop customization workflows against the real Catppuccin Waybar package:
1. Discover package via FTS5 search query.
2. Inspect package metadata, authors provenance, Wayland compatibility, and palette swatches.
3. Compute initial diff plan, asserting target `~/.config/waybar/catppuccin-mocha.css`, action `[CREATE]`, planned backup `None`.
4. Install package, verifying file placement, permissions 0644, and SQLite transaction logging.
5. Modify installed configuration, asserting subsequent diff shows action `[MODIFY]` and planned backup `backups/<sha256>`.
6. Rollback transaction, verifying clean restoration/removal and journal state synchronization.

---

## 5. Test Suite Directory Layout

```
tests/
├── e2e_tests.rs                  # Primary test suite runner
└── e2e/
    ├── mod.rs                    # Module declarations
    ├── common.rs                 # Shared TestEnvironment, runner, DB helpers
    ├── tier1_feature_coverage.rs # F1-F11 core feature coverage
    ├── tier2_boundary_corner.rs  # Boundary values, invalid inputs, edge cases
    ├── tier3_cross_feature.rs    # Combinatorial state machine transitions
    └── tier4_real_workload.rs    # Real-world Catppuccin Waybar lifecycle
```
