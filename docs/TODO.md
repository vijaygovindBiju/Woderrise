# Actionable Implementation Backlog (TODO)

> **Document ID:** SPEC-MGT-TODO-037  
> **Status:** Actionable Engineering Task Backlog  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-25

---

## 1. Backlog Organization & Task Anatomy

Every task in this backlog is structured to be completely actionable by an engineering agent without guessing:
- **Task ID:** Unique identifier (e.g. `TASK-M1-CORE-001`).
- **Subsystem:** Architectural module.
- **Dependencies:** Preceding tasks that must be merged first.
- **Completion Criteria:** Objective verification threshold (tests, code artifacts).

---

## 2. Milestone 1: Core Engine, WAL Journal & CLI

### 2.1 Workspace Scaffolding & Manifest Types
- [x] **`TASK-M1-CORE-001`**: Initialize Cargo workspace with crates: `woderrise-core`, `woderrise-cli`, `woderrise-dbus`, `woderrise-gui`, `woderrise-privhelper`.
  - **Subsystem:** Build / Infrastructure
  - **Dependencies:** None
  - **Completion Criteria:** `cargo check --workspace` passes cleanly.
- [x] **`TASK-M1-CORE-002`**: Implement `Manifest` and `PackageSpec` AST models in `woderrise-core` using `serde` and `serde_yaml`.
  - **Subsystem:** Package Model
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Unit tests parse all 6 validated manifest examples from [PACKAGE_MANIFEST_SPEC.md](PACKAGE_MANIFEST_SPEC.md) without error.
- [x] **`TASK-M1-CORE-003`**: Implement manifest schema/path/checksum validator and typed schema-violation error reporter.
  - **Subsystem:** Validation
  - **Dependencies:** `TASK-M1-CORE-002`
  - **Completion Criteria:** Rejects path traversals (`../`) and missing SHA-256 hashes with typed `WoderriseError`.

### 2.2 Storage, WAL Journal & Backups
- [x] **`TASK-M1-CORE-004`**: Implement SQLite schema migrations for journal and ownership tables using `rusqlite`.
  - **Subsystem:** Journal Engine
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Database initializes in WAL mode; passes table existence tests.
- [x] **`TASK-M1-CORE-005`**: Implement `BackupManager` for content-addressed file snapshots in `~/.local/share/woderrise/backups/`.
  - **Subsystem:** Backup Engine
  - **Dependencies:** `TASK-M1-CORE-004`
  - **Completion Criteria:** Creates byte-for-byte verified backup snapshots; deduplicates identical files via SHA-256 hash.
- [x] **`TASK-M1-CORE-006`**: Implement `AtomicFileSwapper` using temporary sibling staging and portable atomic rename fallback.
  - **Subsystem:** Filesystem Engine
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Swaps files atomically; sets exact POSIX permissions (`0644`/`0755`).

### 2.3 Installation Pipeline & Rollback
- [x] **`TASK-M1-CORE-007`**: Implement the installation engine state machine and durable operation journal.
  - **Subsystem:** Pipeline
  - **Dependencies:** `TASK-M1-CORE-002`, `TASK-M1-CORE-005`, `TASK-M1-CORE-006`
  - **Completion Criteria:** Executes full pipeline; records all stage transitions in `transactions` and `operations` tables.
- [x] **`TASK-M1-CORE-008`**: Implement the `RollbackEngine` with optimistic concurrency checks.
  - **Subsystem:** Rollback
  - **Dependencies:** `TASK-M1-CORE-007`
  - **Completion Criteria:** Reverses operations in strict descending sequence; halts with typed conflict if user edited files post-install.
- [x] **`TASK-M1-CORE-009`**: Implement the startup crash recovery scanner.
  - **Subsystem:** Recovery
  - **Dependencies:** `TASK-M1-CORE-007`, `TASK-M1-CORE-008`
  - **Completion Criteria:** Successfully recovers from simulated `kill -9` process kills across all 15 stages.

### 2.4 Developer CLI
- [x] **`TASK-M1-CLI-010`**: Implement `woderrise-cli` with subcommands: `install`, `rollback`, `list`, `lint`.
  - **Subsystem:** CLI
  - **Dependencies:** `TASK-M1-CORE-007`, `TASK-M1-CORE-008`
  - **Completion Criteria:** Can lint manifests and install a local test package via terminal.

---

## 3. Milestone 2: D-Bus Daemon, Resolver & Privilege Helper

- [x] **`TASK-M2-DBUS-001`**: Implement a headless-safe asynchronous service abstraction for `org.woderrise.Installer`.
  - **Subsystem:** IPC
  - **Dependencies:** `TASK-M1-CORE-007`
  - **Completion Criteria:** Exposes typed methods and emits progress signals over D-Bus session bus.
- [x] **`TASK-M2-POLKIT-002`**: Implement `woderrise-privhelper` binary with Polkit action `org.woderrise.install-system-packages`.
  - **Subsystem:** Security
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Restricts commands strictly to native package installations; rejects arbitrary arguments.
- [x] **`TASK-M2-SOLVER-003`**: Implement deterministic dependency resolution with cycle detection and version constraints.
  - **Subsystem:** Dependency Solver
  - **Dependencies:** `TASK-M1-CORE-002`
  - **Completion Criteria:** Solves complex dependency trees; detects and rejects circular dependencies.
- [x] **`TASK-M2-HYPR-004`**: Implement native Hyprland IPC client (`hyprctl version`, `hyprctl reload`, `hyprctl plugin load`).
  - **Subsystem:** Compositor Integration
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Queries active Hyprland socket; validates C++ plugin ABI commit hash.

---

## 4. Milestone 3: Native Terminal User Interface

> The former graphical frontend direction has been replaced by a Ratatui +
> Crossterm terminal interface. The `woderrise-gui` package now provides the
> interactive TUI and remains independent of GTK/Wayland system libraries.

- [x] **`TASK-M3-GUI-001`**: Scaffold Ratatui/Crossterm TUI with responsive page navigation and keyboard controls.
  - **Subsystem:** UI Layout
  - **Dependencies:** `TASK-M2-DBUS-001`
  - **Completion Criteria:** TUI launches in a terminal with keyboard navigation.
- [x] **`TASK-M3-GUI-002`**: Implement local SQLite FTS search and category catalog views.
  - **Subsystem:** UI Discovery
  - **Dependencies:** `TASK-M3-GUI-001`
  - **Completion Criteria:** Renders package cards; queries SQLite FTS5 index in < 50ms.
- [x] **`TASK-M3-GUI-003`**: Implement manifest preview metadata and palette viewer in the TUI.
  - **Subsystem:** UI Previews
  - **Dependencies:** `TASK-M3-GUI-001`
  - **Completion Criteria:** Smoothly scrolls screenshots; provides 1-click hex copying for theme palettes.
- [x] **`TASK-M3-GUI-004`**: Implement source-versus-target diff inspector in the TUI.
  - **Subsystem:** UI Diff Viewer
  - **Dependencies:** `TASK-M3-GUI-001`
  - **Completion Criteria:** Renders synchronized dual-pane diffs for configuration files.
- [x] **`TASK-M3-GUI-005`**: Implement installation preview and asynchronous live transaction progress.
  - **Subsystem:** UI Transactions
  - **Dependencies:** `TASK-M3-GUI-004`, `TASK-M2-DBUS-001`
  - **Completion Criteria:** Displays pre-install safety summary; renders real-time pipeline step progress.
- [x] **`TASK-M3-GUI-006`**: Implement chronological history and confirmed rollback interface.
  - **Subsystem:** UI History
  - **Dependencies:** `TASK-M3-GUI-001`, `TASK-M2-DBUS-001`
  - **Completion Criteria:** Lists transaction records; triggers rollback with optimistic concurrency modal if needed.
- [x] **`TASK-M3-GUI-007`**: Load the TUI catalog from validated manifests and an optional asynchronous GitHub registry cache.
  - **Subsystem:** UI Discovery
  - **Dependencies:** `TASK-M3-GUI-002`, `TASK-M4-REG-001`
  - **Completion Criteria:** Refreshes configured HTTPS GitHub registries without blocking the TUI, preserves cached entries on failure, and previews verified artifact content.

---

## 5. Milestone 4: Community Registry & Ecosystem Seeding

- [x] **`TASK-M4-REG-001`**: Scaffold registry schema, index, sharding documentation, and provenance policy.
  - **Subsystem:** Registry
  - **Dependencies:** None
  - **Completion Criteria:** Repository initialized with schema files, documentation, and root keys.
- [x] **`TASK-M4-CI-002`**: Implement GitHub Actions automated validation pipeline for incoming Pull Requests.
  - **Subsystem:** Registry CI
  - **Dependencies:** `TASK-M4-REG-001`, `TASK-M1-CLI-010`
  - **Completion Criteria:** Runs `woderrise lint`, checksum verification, and AST security scanner on all PRs.
- [ ] **`TASK-M4-SEED-003`**: Author and publish seed catalog of 50 official and community packages.
  - **Subsystem:** Catalog
  - **Dependencies:** `TASK-M4-CI-002`
  - **Completion Criteria:** 50 validated manifests merged into registry index.
  - **Limitation:** `registry/fixtures/` contains 50 deterministic, explicitly
    non-publishable validation fixtures. They make no official/community
    provenance claims and are intentionally excluded from the installable index.
  A single pinned Catppuccin Waybar package is available under
  `registry/packages/`; remote signature verification is not implemented.

---

## 6. Milestone 5: Sandboxing & Multi-Distribution Support

- [x] **`TASK-M5-SAND-001`**: Implement Bubblewrap (`bwrap`) script execution harness for legacy packages.
  - **Subsystem:** Sandboxing
  - **Dependencies:** `TASK-M1-CORE-007`
  - **Completion Criteria:** Enforces isolated `$HOME`, unshared network, read-only root, and 30s timeout.
- [x] **`TASK-M5-DIST-002`**: Implement abstract `SystemPackageManager` trait with Fedora (`dnf`) backend.
  - **Subsystem:** Portability
  - **Dependencies:** `TASK-M2-POLKIT-002`
  - **Completion Criteria:** Passes packaging tests on Fedora 40 container.

---

## 7. Next Iteration Backlog (High-Impact Discovery & UX Enhancements)

- [ ] **`TASK-NEXT-DISC-001`**: Background Tree Pre-Inspection (Eager Detection)
  - **Subsystem:** Discovery Engine (`woderrise-detect`, `woderrise-github`, `woderrise-gui`)
  - **Dependencies:** None
  - **What:** When GitHub returns search results, have a background worker asynchronously fetch the top 5–10 repository trees via Fastly CDN / GitHub Git Trees API.
  - **Benefit & Completion Criteria:** Component badges (Hyprland, Waybar, Hyprlock, Hypridle, Wallpapers, GTK) and file counts populate on discovery cards automatically without requiring manual "Inspect" clicks.

- [ ] **`TASK-NEXT-DISC-002`**: Multi-Topic Aggregation & Curation Feed
  - **Subsystem:** Discovery Engine (`woderrise-github`, `woderrise-gui`)
  - **Dependencies:** None
  - **What:** Expand the catalog refresh logic to query multiple search terms in parallel (`topic:hyprland`, `topic:waybar`, `quickshell`, `caelestia-dots`), then deduplicate and sort by stars.
  - **Benefit & Completion Criteria:** Expands the home screen catalog from 30 to 100+ curated setups while preserving rate limit quotas.

- [ ] **`TASK-NEXT-AUTH-003`**: Personal Access Token (PAT) Integration in Settings
  - **Subsystem:** Authentication & GUI (`woderrise-github`, `woderrise-gui`)
  - **Dependencies:** None
  - **What:** Provide an input dialog in the GUI/CLI to store an optional GitHub Personal Access Token in the local keyring / config file.
  - **Benefit & Completion Criteria:** Increases GitHub search rate limits from 10/min to 30/min, and core REST API limits from 60/hr to 5,000/hr, with immediate gauge indicator updates.

- [ ] **`TASK-NEXT-SEARCH-004`**: Fuzzy & Typo-Tolerant Search
  - **Subsystem:** Search Engine (`woderrise-github`)
  - **Dependencies:** None
  - **What:** Replace simple substring matching in `crates/woderrise-github/src/search.rs` with a lightweight fuzzy matching crate (`sublime_fuzzy` or `nucleo-matcher`).
  - **Benefit & Completion Criteria:** Search queries tolerate typos and abbreviations (e.g. `ml4` -> `mylinuxforwork`, `hyprlnd` -> `hyprland`), ranking closest matches first.

- [ ] **`TASK-NEXT-FEED-005`**: Remote Curated Catalog Feed (CDN Hosted)
  - **Subsystem:** Registry & Infrastructure (`woderrise-github`, CI/CD)
  - **Dependencies:** `TASK-NEXT-DISC-001`
  - **What:** Publish a daily automated GitHub Action in the Woderrise repository that scans GitHub, verifies all components, and uploads a validated `catalog.v1.json` to a static CDN or GitHub Pages.
  - **Benefit & Completion Criteria:** Client applications download a rich, pre-audited index of hundreds of configurations on startup with 0 GitHub API rate limit consumption.

- [x] **`TASK-NEXT-BUG-006`**: Fix Cosmic-Text Line Height Panic on Inspect/History/Install Screens
  - **Subsystem:** GUI Layout Stability (`woderrise-gui`)
  - **Dependencies:** None
  - **Issue:** Using dummy `text("").size(0)` or `container(text("").size(0))` for empty/conditional placeholders causes `cosmic-text` to panic with `assertion left != right failed: line height cannot be 0` (specifically in `screens/inspect.rs` lines 188/224, `screens/history.rs`, and `screens/install.rs`).
  - **Fix & Completion Criteria:** Eliminate all `size(0)` text widgets across the GUI; conditionally push elements into layout columns or use `iced::widget::Space::new(0, 0)` so clicking "Inspect" renders cleanly without crashing.
