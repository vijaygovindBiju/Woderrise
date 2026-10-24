# System Architecture & Technical Design

> **Document ID:** SPEC-ARCH-SYS-019  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. High-Level Subsystem Topology

Woderrise is architected as a **modular, multi-process system** adhering to strict UNIX separation of concerns, principle of least privilege, and asynchronous event-driven design.

```mermaid
flowchart TD
    subgraph UI_Layer [User Interface Layer (Unprivileged)]
        GUI[Graphical Frontend: woderrise-gui (Libadwaita / GTK4)]
        CLI[Developer & Power CLI: woderrise]
    end

    subgraph IPC_Layer [Local Session IPC Broker]
        DBus[D-Bus Session Bus: org.woderrise.Installer]
    end

    subgraph Core_Daemon [Core Installation Engine: woderised (Unprivileged)]
        Router[IPC Request Router & Dispatcher]
        Planner[Installation Planner & Conflict Engine]
        Resolver[PubGrub Dependency Solver]
        Compat[Compositor & ABI Compatibility Engine]
        Executor[Atomic Transaction Executor]
        Journal[SQLite WAL Journal & Recovery Engine]
        BackupMgr[Backup Snapshot Manager]
        CacheMgr[Content-Addressed Cache Manager]
        OwnerDB[(File Ownership DB)]
        RegistryClient[Registry Git/HTTP Sync Client]
    end

    subgraph Host_System [Linux Host Environment]
        FS[User Filesystem: ~/.config, ~/.local]
        HyprIPC[Hyprland IPC Socket: hyprctl]
        SystemdUser[systemd --user session manager]
        PolkitHelper[Privilege Helper: woderrise-privhelper (Polkit Auth)]
        ALPM[Arch Linux ALPM / Pacman Backend]
    end

    GUI -->|D-Bus Methods & Signals| DBus
    CLI -->|Direct or D-Bus IPC| DBus
    DBus --> Router

    Router --> Planner
    Planner --> Resolver
    Planner --> Compat
    Planner --> CacheMgr
    Planner --> BackupMgr
    Planner --> OwnerDB

    Planner --> Executor
    Executor --> Journal
    Executor --> FS
    Executor --> HyprIPC
    Executor --> SystemdUser

    Executor -.->|Request System Package Install| PolkitHelper
    PolkitHelper -->|Root via Polkit Auth| ALPM
```

---

## 2. Core Subsystem Responsibilities & Interfaces

### 2.1 Graphical Frontend (`woderrise-gui`)
- **Technology:** Rust with GTK4 and Libadwaita (`libadwaita-rs`).
- **Role:** Presents the responsive, Wayland-native desktop store. Renders catalog cards, HiDPI preview galleries, syntax-highlighted code viewers, unified file diffs, and real-time transaction progress streams.
- **Security Boundary:** Runs entirely unprivileged as the desktop user. Interacts with the daemon strictly over asynchronous D-Bus.

### 2.2 Developer & System CLI (`woderrise`)
- **Technology:** Rust with `clap`.
- **Role:** Provides an identical interface for terminal enthusiasts, headless script testing, manifest linting (`woderrise lint`), package signing, and CI automated verification.

### 2.3 Installation Daemon (`woderised`)
- **Technology:** Rust async service running on the `tokio` runtime.
- **Role:** Orchestrates transaction journals, content-addressed caches, file ownership tracking, and compositor signals. Ensures single-writer mutual exclusion via OS file locks.

### 2.4 Registry Client & Local Indexer
- **Role:** Synchronizes sharded JSON index metadata from Git repositories or compressed snapshot mirrors. Maintains the local SQLite FTS5 database for instantaneous sub-10ms full-text searches.

### 2.5 PubGrub Dependency Solver
- **Role:** Formulates deterministic installation graphs across store packages, native system binaries (`ALPM`), and Hyprland plugin ABIs. Resolves version bounds and detects circular dependencies.

### 2.6 Compatibility Engine
- **Role:** Queries running Hyprland version, Git commit hash, Wayland protocol versions, and CPU architecture. Blocks installation if C++ ABI headers mismatch.

### 2.7 Backup & Recovery Manager
- **Role:** Generates pre-modification content-addressed snapshots in `~/.local/share/woderrise/backups/`. Reconstructs prior file states during rollback or crash recovery.

### 2.8 Atomic Transaction Executor
- **Role:** Executes disk operations using sibling staging files and atomic POSIX `renameat2(RENAME_EXCHANGE)`. Updates the SQLite Write-Ahead Log (WAL) at each stage.

### 2.9 Polkit Privilege Broker (`woderrise-privhelper`)
- **Technology:** Minimalist Rust helper invoked via D-Bus system activation or Polkit agent.
- **Role:** The **only** component with potential elevated access. Performs strictly scoped native system package operations (e.g. `pacman -S waybar`) after explicit user authorization. Dropped immediately after execution.

---

## 3. Trust Boundaries & Security Enclaves

```text
[ UNTRUSTED ZONE ]
├── Community Package Manifests (Untrusted until linted & hash-verified)
└── Remote Git Repositories / Download URLs
        │
        ▼ (Cryptographic Verification: SHA-256 + Ed25519 Signatures)
[ USER-SESSION ZONE (Unprivileged, UID=1000) ]
├── woderrise-gui
├── woderrise-cli
├── woderised (Core Engine)
├── ~/.config/, ~/.local/share/
        │
        ▼ (Polkit Authentication Boundary: org.woderrise.install-system-packages)
[ ELEVATED SYSTEM ZONE (Root, UID=0) ]
└── woderrise-privhelper (Restricted ALPM / pacman binary installs only)
```

1. **User Dotfiles Do Not Need Root:**
   $HOME configuration files are owned by the desktop user. The daemon never runs as root and never invokes `sudo` for dotfiles.
2. **Path Containment:**
   All filesystem writes are sandboxed to `$HOME/.config`, `$HOME/.local/share`, `$HOME/.local/state`, and `$HOME/.themes`. Any write targeting `/etc`, `/usr`, or system paths is blocked by the engine.
3. **Restricted Polkit Scope:**
   The privileged helper cannot execute arbitrary shell commands; it only exposes a single typed method: `InstallNativePackages(Vec<String>)`.
