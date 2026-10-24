# Architecture Decision Records (ADR)

> **Document ID:** SPEC-ENG-ADR-038  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Overview & ADR Index

This document formalizes the key architectural, technical, and security decisions governing **Woderrise**. Each record documents the context, trade-offs, evaluated alternatives, and conditions that would justify revisiting the decision.

| ADR ID | Decision Title | Status | Date |
| :--- | :--- | :---: | :--- |
| **ADR-001** | Rust as Primary Implementation Language | Accepted | 2026-10-24 |
| **ADR-002** | Libadwaita / GTK4 as Graphical Frontend Framework | Accepted | 2026-10-24 |
| **ADR-003** | SQLite WAL as Transaction Journal & Persistence Engine | Accepted | 2026-10-24 |
| **ADR-004** | D-Bus via Pure-Rust `zbus` for Session IPC | Accepted | 2026-10-24 |
| **ADR-005** | Declarative Operations Over Arbitrary Shell Scripts | Accepted | 2026-10-24 |
| **ADR-006** | Single-Writer Mutex Lock for Mutating Operations | Accepted | 2026-10-24 |
| **ADR-007** | Content-Addressed Backup & Artifact Storage | Accepted | 2026-10-24 |
| **ADR-008** | Optimistic Concurrency Checks on Transaction Rollback | Accepted | 2026-10-24 |
| **ADR-009** | Strict Separation of Session Daemon from Polkit Helper | Accepted | 2026-10-24 |
| **ADR-010** | Git-Backed Sharded Repository for Public Package Registry | Accepted | 2026-10-24 |
| **ADR-011** | Mandatory Pre-Install Snapshot for Full Desktop Setups | Accepted | 2026-10-24 |
| **ADR-012** | PubGrub SAT Algorithm for Dependency Resolution | Accepted | 2026-10-24 |

---

## 2. Architectural Decisions

### ADR-001: Rust as Primary Implementation Language
- **Context:** The core daemon must manage filesystem atomic operations, parse untrusted YAML/archives, and execute crash-consistent rollbacks without crashing or introducing memory vulnerabilities.
- **Decision:** Implement `woderrise-core`, `woderrise-cli`, and `woderrise-dbus` in Rust (edition 2021).
- **Consequences:**
  - *Positive:* Memory safety without GC pauses; strong type system (`Result<T, E>`); first-class Linux system call access (`renameat2`, `statvfs`).
  - *Negative:* Longer initial compilation times; steeper contributor onboarding curve compared to Python.
- **Alternatives Considered:** Go (poor C API bindings for Wayland/Polkit), Python (untyped, slow startup, runtime exceptions), C++ (memory safety risks).
- **Revisit Trigger:** If a specific subcomponent requires web tech, or if language ecosystem shifts.

---

### ADR-002: Libadwaita / GTK4 as Graphical Frontend Framework
- **Context:** Modern Hyprland users demand a fast, aesthetic, Wayland-native UI with dark mode, fractional scaling, and sub-pixel typography.
- **Decision:** Build `woderrise-gui` in Rust using GTK4 and Libadwaita (`libadwaita-rs`).
- **Consequences:**
  - *Positive:* Native Wayland subsurfaces; responsive mobile-to-desktop navigation; instant startup (< 500ms); low memory (< 90MB RSS).
  - *Negative:* Libadwaita styling requires careful attention to complement non-GNOME Hyprland aesthetic rices.
- **Alternatives Considered:** Tauri (WebKitGTK memory bloat > 250MB), Qt6/QML (theming friction with GTK-centric dotfiles), Slint (lacking mature syntax diff viewers).
- **Revisit Trigger:** If Slint or another lightweight Rust toolkit achieves feature parity for complex code diffs and webp carousels.

---

### ADR-003: SQLite WAL as Transaction Journal & Persistence Engine
- **Context:** The system requires an ACID-compliant, crash-consistent store to track transactions, backups, and file ownership across power outages and unexpected reboots.
- **Decision:** Utilize SQLite 3 operating in Write-Ahead Logging (WAL) mode via `rusqlite`.
- **Consequences:**
  - *Positive:* Atomic multi-table updates; concurrent read-during-write without stalling GUI queries; built-in SQLite FTS5 for sub-10ms search.
  - *Negative:* Binary database format requires diagnostic CLI tools to inspect without SQLite clients.
- **Alternatives Considered:** Plain JSON/TOML (vulnerable to corruption mid-write; no concurrent transactions), Sled (embedded key-value lacks relational integrity and FTS5).
- **Revisit Trigger:** Never; SQLite is the industry standard for local ACID application state.

---

### ADR-004: D-Bus via Pure-Rust `zbus` for Session IPC
- **Context:** The GUI and CLI need to communicate asynchronously with the background service `woderised`, emit progress streams, and integrate with system authorization.
- **Decision:** Export `org.woderrise.Installer` on the D-Bus session bus using `zbus`.
- **Consequences:**
  - *Positive:* Standard Linux desktop protocol; systemd user activation; native Polkit integration; pure Rust without `libdbus` C dependencies.
  - *Negative:* Slightly higher payload serialization overhead than raw UNIX sockets.
- **Alternatives Considered:** Raw UNIX domain socket with custom JSON-RPC (cannot integrate natively with Polkit or systemd D-Bus activation).
- **Revisit Trigger:** If Woderrise is ported to non-D-Bus platforms (e.g. musl/Alpine containers without D-Bus).

---

### ADR-005: Declarative Operations Over Arbitrary Shell Scripts
- **Context:** Monolithic bash installers (`curl | bash`) are the primary source of broken setups, security breaches, and irreversible filesystem corruption.
- **Decision:** Mandate declarative operations (`files`, `config_injections`, `service_triggers`) for all official package manifests. Arbitrary shell scripts are restricted to isolated Bubblewrap containers.
- **Consequences:**
  - *Positive:* 100% deterministic file tracking; guaranteed atomic rollback; static security linting.
  - *Negative:* Some complex legacy rices with dynamic bash calculations require refactoring to fit the declarative schema.
- **Alternatives Considered:** Free-form bash scripts with manual undo hooks (proven to fail in community practice).
- **Revisit Trigger:** Never; declarative architecture is a non-negotiable core principle.

---

### ADR-006: Single-Writer Mutex Lock for Mutating Operations
- **Context:** Concurrent installations or uninstalls could result in race conditions, corrupted backups, or conflicting file ownership claims.
- **Decision:** Enforce a process-level mutual exclusion lock (`~/.local/state/woderrise/engine.lock`) before executing any mutating transaction.
- **Consequences:**
  - *Positive:* Eliminates concurrency race conditions and corrupt states.
  - *Negative:* Concurrent install requests must be queued or rejected.
- **Alternatives Considered:** Multi-version concurrency control (MVCC) on filesystem (overly complex; POSIX filesystems do not support multi-tenant user dotfile versioning natively).
- **Revisit Trigger:** None; desktop customization is an inherently single-user interactive activity.

---

### ADR-007: Content-Addressed Backup & Artifact Storage
- **Context:** Storing backups by simple filename causes collisions if multiple packages overwrite the same file; downloading packages repeatedly wastes bandwidth.
- **Decision:** Index all backup snapshots and downloaded archives by cryptographic SHA-256 hash in `~/.local/share/woderrise/backups/` and `~/.cache/woderrise/artifacts/`.
- **Consequences:**
  - *Positive:* Automatic deduplication; guaranteed byte integrity; immutable cache entries.
  - *Negative:* Requires mapping metadata tables to resolve human-readable paths to content hashes.
- **Alternatives Considered:** Timestamped directory trees (creates duplicate copies of large identical files; hard to deduplicate).
- **Revisit Trigger:** None; content-addressing is the gold standard for package caches.

---

### ADR-008: Optimistic Concurrency Checks on Transaction Rollback
- **Context:** If a user edits a file after installing a package, blindly rolling back the transaction would destroy the user's manual work.
- **Decision:** Before reverting or restoring any file, verify that `sha256(current_file) == post_sha256(journal_record)`. If modified, halt and prompt user with diff modal.
- **Consequences:**
  - *Positive:* Zero silent loss of user modifications.
  - *Negative:* Rollback requires interactive user confirmation when files have been modified.
- **Alternatives Considered:** Force overwrite (causes user data loss), Refuse rollback entirely (poor user experience).
- **Revisit Trigger:** None; protecting user work is paramount.

---

### ADR-009: Strict Separation of Session Daemon from Polkit Helper
- **Context:** Desktop dotfiles must be owned by the user. Giving the core daemon root privileges violates least privilege and invites system compromise.
- **Decision:** The core daemon runs completely unprivileged as the desktop user. Only native package installations escalate via a narrowly scoped Polkit helper (`woderrise-privhelper`).
- **Consequences:**
  - *Positive:* Complete isolation of elevated privileges; zero risk of corrupting `/etc` or `/usr` during dotfile deployment.
  - *Negative:* Requires Polkit configuration and distinct IPC communication between daemon and helper.
- **Alternatives Considered:** Running entire daemon as root or using `sudo` (catastrophic security anti-pattern).
- **Revisit Trigger:** None.

---

### ADR-010: Git-Backed Sharded Repository for Public Package Registry
- **Context:** The package registry must scale to thousands of packages without costly centralized server infrastructure or vendor lock-in.
- **Decision:** Maintain the registry index in a public Git repository with 2-character sharding (identical to Cargo and Homebrew).
- **Consequences:**
  - *Positive:* Zero hosting cost; full public audit trail; decentralized pull-request contribution model; offline git syncing.
  - *Negative:* Initial index clone requires ~20MB download (mitigated by compressed snapshot tarballs).
- **Alternatives Considered:** Centralized custom REST API (requires maintaining servers, databases, and operational overhead).
- **Revisit Trigger:** If catalog exceeds 500,000 packages and Git transfer protocols become a bottleneck.

---

### ADR-011: Mandatory Pre-Install Snapshot for Full Desktop Setups
- **Context:** Installing a complete desktop setup ("full rice") alters dozens of interconnected configuration files across Hyprland, Waybar, Rofi, and Daemons.
- **Decision:** Require a comprehensive, byte-for-byte snapshot of all target configuration directories before deploying any `desktop_setup` package.
- **Consequences:**
  - *Positive:* Guaranteed 1-click restoration to previous desktop environment even if multiple complex components are swapped.
  - *Negative:* Slightly higher pre-install backup disk footprint (~20-50MB).
- **Alternatives Considered:** Selective file backups only (frequently leaves orphan config snippets that cause session conflicts).
- **Revisit Trigger:** None; user peace of mind is essential.

---

### ADR-012: PubGrub SAT Algorithm for Dependency Resolution
- **Context:** Resolving dependencies across store packages, native system packages, and Hyprland plugin ABIs requires solving complex Boolean satisfiability constraints.
- **Decision:** Integrate the `pubgrub` crate to resolve dependency graphs.
- **Consequences:**
  - *Positive:* Fast, deterministic resolution; human-readable failure explanations; detection of circular dependencies.
  - *Negative:* Requires strict SemVer constraint modeling across all packages.
- **Alternatives Considered:** Custom ad-hoc greedy solver (prone to infinite loops and poor error explanations).
- **Revisit Trigger:** If distribution package managers (e.g. `libsolv`) can be cleanly linked without heavy C dependencies.
