# Actionable Implementation Backlog (TODO)

> **Document ID:** SPEC-MGT-TODO-037  
> **Status:** Actionable Engineering Task Backlog  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

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
- [ ] **`TASK-M1-CORE-001`**: Initialize Cargo workspace with crates: `woderrise-core`, `woderrise-cli`, `woderrise-dbus`, `woderrise-gui`, `woderrise-privhelper`.
  - **Subsystem:** Build / Infrastructure
  - **Dependencies:** None
  - **Completion Criteria:** `cargo check --workspace` passes cleanly.
- [ ] **`TASK-M1-CORE-002`**: Implement `Manifest` and `PackageSpec` AST models in `woderrise-core` using `serde` and `serde_yaml`.
  - **Subsystem:** Package Model
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Unit tests parse all 6 validated manifest examples from [PACKAGE_MANIFEST_SPEC.md](PACKAGE_MANIFEST_SPEC.md) without error.
- [ ] **`TASK-M1-CORE-003`**: Implement JSON Schema Draft 2020-12 validator and schema-violation error reporter.
  - **Subsystem:** Validation
  - **Dependencies:** `TASK-M1-CORE-002`
  - **Completion Criteria:** Rejects path traversals (`../`) and missing SHA-256 hashes with typed `WoderriseError`.

### 2.2 Storage, WAL Journal & Backups
- [ ] **`TASK-M1-CORE-004`**: Implement SQLite schema migrations for `journal.db` and `ownership.db` using `rusqlite`.
  - **Subsystem:** Journal Engine
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Database initializes in WAL mode; passes table existence tests.
- [ ] **`TASK-M1-CORE-005`**: Implement `BackupManager` for content-addressed file snapshots in `~/.local/share/woderrise/backups/`.
  - **Subsystem:** Backup Engine
  - **Dependencies:** `TASK-M1-CORE-004`
  - **Completion Criteria:** Creates byte-for-byte verified backup snapshots; deduplicates identical files via SHA-256 hash.
- [ ] **`TASK-M1-CORE-006`**: Implement `AtomicFileSwapper` using Linux `renameat2(RENAME_EXCHANGE)` and temporary sibling staging.
  - **Subsystem:** Filesystem Engine
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Swaps files atomically; sets exact POSIX permissions (`0644`/`0755`).

### 2.3 Installation Pipeline & Rollback
- [ ] **`TASK-M1-CORE-007`**: Implement the 15-stage `InstallationEngine` state machine.
  - **Subsystem:** Pipeline
  - **Dependencies:** `TASK-M1-CORE-002`, `TASK-M1-CORE-005`, `TASK-M1-CORE-006`
  - **Completion Criteria:** Executes full pipeline; records all stage transitions in `transactions` and `operations` tables.
- [ ] **`TASK-M1-CORE-008`**: Implement the `RollbackEngine` with Optimistic Concurrency Checks.
  - **Subsystem:** Rollback
  - **Dependencies:** `TASK-M1-CORE-007`
  - **Completion Criteria:** Reverses operations in strict descending sequence; halts with typed conflict if user edited files post-install.
- [ ] **`TASK-M1-CORE-009`**: Implement the Startup Crash Recovery Scanner.
  - **Subsystem:** Recovery
  - **Dependencies:** `TASK-M1-CORE-007`, `TASK-M1-CORE-008`
  - **Completion Criteria:** Successfully recovers from simulated `kill -9` process kills across all 15 stages.

### 2.4 Developer CLI
- [ ] **`TASK-M1-CLI-010`**: Implement `woderrise-cli` with subcommands: `install`, `rollback`, `list`, `lint`.
  - **Subsystem:** CLI
  - **Dependencies:** `TASK-M1-CORE-007`, `TASK-M1-CORE-008`
  - **Completion Criteria:** Can lint manifests and install a local test package via terminal.

---

## 3. Milestone 2: D-Bus Daemon, Resolver & Privilege Helper

- [ ] **`TASK-M2-DBUS-001`**: Implement `zbus` asynchronous interface for `org.woderrise.Installer`.
  - **Subsystem:** IPC
  - **Dependencies:** `TASK-M1-CORE-007`
  - **Completion Criteria:** Exposes typed methods and emits progress signals over D-Bus session bus.
- [ ] **`TASK-M2-POLKIT-002`**: Implement `woderrise-privhelper` binary with Polkit action `org.woderrise.install-system-packages`.
  - **Subsystem:** Security
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Restricts commands strictly to native package installations; rejects arbitrary arguments.
- [ ] **`TASK-M2-SOLVER-003`**: Integrate `pubgrub` SAT dependency solver for multi-package graphs.
  - **Subsystem:** Dependency Solver
  - **Dependencies:** `TASK-M1-CORE-002`
  - **Completion Criteria:** Solves complex dependency trees; detects and rejects circular dependencies.
- [ ] **`TASK-M2-HYPR-004`**: Implement native Hyprland IPC client (`hyprctl version`, `hyprctl reload`, `hyprctl plugin load`).
  - **Subsystem:** Compositor Integration
  - **Dependencies:** `TASK-M1-CORE-001`
  - **Completion Criteria:** Queries active Hyprland socket; validates C++ plugin ABI commit hash.

---

## 4. Milestone 3: Native Libadwaita Graphical Frontend

- [ ] **`TASK-M3-GUI-001`**: Scaffold Libadwaita window with `AdwNavigationSplitView` and responsive sidebar.
  - **Subsystem:** UI Layout
  - **Dependencies:** `TASK-M2-DBUS-001`
  - **Completion Criteria:** Window launches in Wayland; adapts between mobile and desktop widths.
- [ ] **`TASK-M3-GUI-002`**: Implement Local Full-Text Search and Category Catalog Views.
  - **Subsystem:** UI Discovery
  - **Dependencies:** `TASK-M3-GUI-001`
  - **Completion Criteria:** Renders package cards; queries SQLite FTS5 index in < 50ms.
- [ ] **`TASK-M3-GUI-003`**: Implement HiDPI Image Carousel and Color Palette Swatch Viewer.
  - **Subsystem:** UI Previews
  - **Dependencies:** `TASK-M3-GUI-001`
  - **Completion Criteria:** Smoothly scrolls screenshots; provides 1-click hex copying for theme palettes.
- [ ] **`TASK-M3-GUI-004`**: Implement Side-by-Side Unified Diff Inspector widget.
  - **Subsystem:** UI Diff Viewer
  - **Dependencies:** `TASK-M3-GUI-001`
  - **Completion Criteria:** Renders synchronized dual-pane diffs for configuration files.
- [ ] **`TASK-M3-GUI-005`**: Implement Installation Preview Modal and Live Transaction Progress Stream.
  - **Subsystem:** UI Transactions
  - **Dependencies:** `TASK-M3-GUI-004`, `TASK-M2-DBUS-001`
  - **Completion Criteria:** Displays pre-install safety summary; renders real-time pipeline step progress.
- [ ] **`TASK-M3-GUI-006`**: Implement Chronological History and 1-Click Rollback Interface.
  - **Subsystem:** UI History
  - **Dependencies:** `TASK-M3-GUI-001`, `TASK-M2-DBUS-001`
  - **Completion Criteria:** Lists transaction records; triggers rollback with optimistic concurrency modal if needed.

---

## 5. Milestone 4: Community Registry & Ecosystem Seeding

- [ ] **`TASK-M4-REG-001`**: Scaffold public `github.com/woderrise/registry` with 2-character sharding structure.
  - **Subsystem:** Registry
  - **Dependencies:** None
  - **Completion Criteria:** Repository initialized with schema files, documentation, and root keys.
- [ ] **`TASK-M4-CI-002`**: Implement GitHub Actions automated validation pipeline for incoming Pull Requests.
  - **Subsystem:** Registry CI
  - **Dependencies:** `TASK-M4-REG-001`, `TASK-M1-CLI-010`
  - **Completion Criteria:** Runs `woderrise lint`, checksum verification, and AST security scanner on all PRs.
- [ ] **`TASK-M4-SEED-003`**: Author and publish seed catalog of 50 official and community packages.
  - **Subsystem:** Catalog
  - **Dependencies:** `TASK-M4-CI-002`
  - **Completion Criteria:** 50 validated manifests merged into registry index.

---

## 6. Milestone 5: Sandboxing & Multi-Distribution Support

- [ ] **`TASK-M5-SAND-001`**: Implement Bubblewrap (`bwrap`) script execution harness for legacy packages.
  - **Subsystem:** Sandboxing
  - **Dependencies:** `TASK-M1-CORE-007`
  - **Completion Criteria:** Enforces isolated `$HOME`, unshared network, read-only root, and 30s timeout.
- [ ] **`TASK-M5-DIST-002`**: Implement abstract `SystemPackageManager` trait with Fedora (`dnf`) backend.
  - **Subsystem:** Portability
  - **Dependencies:** `TASK-M2-POLKIT-002`
  - **Completion Criteria:** Passes packaging tests on Fedora 40 container.
