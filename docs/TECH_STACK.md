# Technology Stack Evaluation & Recommendations

> **Document ID:** SPEC-ARCH-TECH-020  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Executive Summary & Recommended Stack

To ensure extreme reliability, crash consistency, memory safety, and native Wayland integration on Arch Linux and Hyprland, the project stack is selected based on **rigorous architectural benchmarks** rather than transient trends:

| Component | Selected Technology | Primary Rationale | Evaluated Alternatives |
| :--- | :--- | :--- | :--- |
| **Core Engine & Daemon** | **Rust (`tokio`, `rusqlite`, `pubgrub`)** | Memory safety, zero-cost abstractions, robust error handling (`Result<T, E>`), atomic filesystem primitives. | Go, C++, Python |
| **Graphical Frontend** | **Rust + Libadwaita / GTK4 (`libadwaita-rs`)** | Wayland native, sub-pixel text rendering, native fractional scaling, responsive modern widgets matching modern Linux desktop aesthetics. | Qt6/QML, Slint, Tauri, Flutter |
| **Local Session IPC** | **D-Bus via `zbus` (Pure Rust)** | Linux desktop standard; asynchronous; native integration with Polkit and systemd user services. | UNIX Socket (JSON-RPC), gRPC |
| **Persistent Storage** | **SQLite 3 (WAL Mode via `rusqlite`)** | ACID durability, atomic commits, zero-process embedded database, built-in Full-Text Search (FTS5). | Sled, RocksDB, Plain JSON/TOML |
| **Package Manifests** | **YAML (Strict JSON Schema Draft 2020-12)** | Ergonomic multi-line strings for config injections and comments; human-readable and community standard. | TOML, JSON, KDL |
| **Registry Backend** | **Git Repository + Global CDN Mirrors** | Zero-cost serverless hosting, transparent commit history, cryptographic Git SHAs, offline sparse cloning. | Custom REST API (Go/Node) |
| **Script Sandboxing** | **Bubblewrap (`bwrap`)** | Unprivileged Linux user namespaces, unshared network/PID, strict read-only host binds. | Docker, Podman, Firejail |

---

## 2. In-Depth Comparative Evaluations

### 2.1 Graphical Frontend Evaluation

```mermaid
flowchart TD
    Choice{GUI Toolkit Decision} --> GTK4[Libadwaita / GTK4 (Selected)]
    Choice --> Qt6[Qt6 / QML]
    Choice --> Tauri[Tauri (Webview)]
    Choice --> Slint[Slint]

    GTK4 -->|Pros| P1[Native Wayland, Adwaita Styling, Smooth Animations, Wayland Fractional Scaling]
    Qt6 -->|Cons| C1[Heavy C++ runtime, styling conflicts with GTK-centric Hyprland rices]
    Tauri -->|Cons| C2[WebKitGTK memory bloat ~250MB RSS, high startup latency]
    Slint -->|Cons| C3[Immature ecosystem, lacking complex syntax-highlighted diff viewers]
```

#### Detailed Comparison:
1. **Libadwaita / GTK4 (Selected):**
   - *Pros:* Libadwaita provides polished, responsive mobile-to-desktop navigation splits (`AdwNavigationSplitView`), search bars, carousels, and dark mode toggles out of the box. In modern Hyprland environments (e.g. ML4W, End-4), GTK4 apps look native and integrate cleanly with system fonts and themes.
   - *Cons:* Slightly steeper Rust binding learning curve than Web-based tools.
2. **Qt6 / QML:**
   - *Evaluation:* Excellent performance, but QML requires managing separate QML files or C++ bindings. Qt theming frequently clashes with GTK setups on Hyprland unless Kvantum is meticulously configured.
3. **Tauri (Webview / Svelte / React):**
   - *Evaluation:* While web UI development is fast, WebKitGTK on Linux has significant cold startup latency (> 1.2s), heavy memory overhead (200-300MB RAM), and sub-optimal Wayland sub-surface support.
4. **Slint:**
   - *Evaluation:* Very promising Rust-native UI toolkit, but lacks mature components for side-by-side syntax-highlighted code diff viewers and complex accessibility hooks.

---

### 2.2 Core Engine Language Evaluation: Rust vs. Go vs. Python

1. **Rust (Selected):**
   - Provides guaranteed memory safety without garbage collection pauses.
   - Rust's `Result<T, E>` pattern and the `thiserror`/`anyhow` crates enforce strict, comprehensive error handling across all 15 stages of the installation pipeline.
   - Native access to advanced Linux system calls: `renameat2`, `statvfs`, `flock`, and POSIX user namespace sandboxing via `bwrap`.
2. **Go:**
   - *Evaluation:* Simple concurrency with goroutines, but lacks sum types (enums with data), making complex state machines and transactional AST manipulation verbose. Calling Linux C APIs requires CGo.
3. **Python:**
   - *Evaluation:* Extremely fast to prototype, but interpreted, vulnerable to runtime `TypeError`/`NoneType` exceptions, slow startup times (300-500ms interpreter overhead), and lacks reliable single-binary deployment.

---

### 2.3 IPC Broker: D-Bus (`zbus`) vs. UNIX Domain Socket (JSON-RPC)

1. **D-Bus via `zbus` (Selected):**
   - D-Bus is the canonical inter-process communication bus on Linux desktop systems.
   - `zbus` is a 100% pure-Rust, asynchronous D-Bus implementation that requires no `libdbus` C library dependencies.
   - Seamlessly integrates with **systemd user services** and **Polkit** for authentication.
   - Emits native desktop D-Bus signals for real-time progress bars and notifications.
2. **UNIX Domain Socket (JSON-RPC):**
   - *Evaluation:* Simpler protocol, but requires building a custom permission broker, socket file lifecycle manager, and cannot integrate directly with Polkit authorization policies.

---

### 2.4 Persistent Storage: SQLite WAL vs. Plain Files

1. **SQLite 3 in WAL Mode (Selected):**
   - Standard, battle-tested ACID relational database.
   - **Write-Ahead Logging (WAL):** Writers do not block readers, enabling the UI to query search indexes and transaction history concurrently without stalling the installation engine.
   - Built-in `FTS5` extension delivers sub-10ms full-text search across thousands of packages without external search services like Elasticsearch.
2. **Plain JSON / TOML Files:**
   - *Evaluation:* Prone to file corruption if power cuts occur mid-write. Cannot provide atomic multi-table rollbacks or concurrent read/write isolation.

---

## 3. Toolchain & Dependency Matrix

| Category | Crate / Library | Version | Purpose |
| :--- | :--- | :--- | :--- |
| **Async Runtime** | `tokio` | `1.40+` | Multi-threaded async I/O, timers, subprocesses |
| **GUI Framework** | `libadwaita` | `0.7+` | GTK4 / Libadwaita Rust bindings |
| **Database** | `rusqlite` | `0.32+` | SQLite bundled with FTS5 support |
| **IPC** | `zbus` | `4.4+` | Async D-Bus client and service implementation |
| **Serialization** | `serde`, `serde_json`, `serde_yaml` | `1.0+` | Manifest and JSONL parsing |
| **Graph Solver** | `pubgrub` | `0.2+` | SAT-based dependency resolution algorithm |
| **Crypto & Hashing**| `sha2`, `ed25519-dalek` | `0.10+ / 2.1+` | Checksum verification and creator signing |
| **CLI Parser** | `clap` | `4.5+` | Command-line interface argument parsing |
| **Error Handling** | `thiserror`, `anyhow` | `1.0+` | Structured error enumerations |
| **Diff Engine** | `similar` | `2.6+` | Unified text diffing for pre-install inspection |
