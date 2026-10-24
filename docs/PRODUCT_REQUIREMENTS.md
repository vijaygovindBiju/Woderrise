# Product Requirements Document (PRD)

> **Document ID:** SPEC-PROD-REQ-002  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Overview & Requirement Classification

This document formally specifies all functional and non-functional requirements for the **Woderrise** desktop store and package management engine.

Requirements are categorized using standard requirement identifiers:
- `REQ-F-*`: Functional Requirements
- `REQ-NF-*`: Non-Functional Requirements

Priority levels adhere to the MoSCoW framework:
- **P1 (Must Have):** Core MVP requirement essential for safety and basic operation.
- **P2 (Should Have):** Important capability targeted for initial production releases.
- **P3 (Could Have):** Desirable enhancement for future iterations.
- **P4 (Won't Have / Out of Scope):** Explicitly excluded from the current scope.

---

## 2. Functional Requirements (`REQ-F`)

### 2.1 Search and Discovery (`REQ-F-DISC`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-DISC-001` | Full-Text Search | P1 | The store must provide instant, local full-text search across package IDs, titles, descriptions, authors, and tags. |
| `REQ-F-DISC-002` | Faceted Filtering | P1 | Users must be able to filter search results by category, author, license, Hyprland version compatibility, and package type (asset, theme, config, plugin, bundle, full setup). |
| `REQ-F-DISC-003` | Curated & Featured | P2 | The home view must feature curated collections, trending packages, and recently updated packages fetched from the registry. |
| `REQ-F-DISC-004` | Offline Discovery | P1 | If the network is disconnected, search and browsing must gracefully fall back to locally cached registry indexes and installed packages without throwing unhandled exceptions. |

### 2.2 Category Browsing (`REQ-F-CAT`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-CAT-001` | 28 Taxonomic Categories | P1 | The store must organize packages into the 28 defined categories (bars, wallpapers, themes, plugins, bundles, full rices, etc.) as specified in [CATALOG_TAXONOMY.md](CATALOG_TAXONOMY.md). |
| `REQ-F-CAT-002` | Subcategory Navigation | P2 | Categories with large catalogs must support secondary filtering (e.g., Filtering "Bars" by Waybar, AGS, or Quickshell). |
| `REQ-F-CAT-003` | Category Metadata Counts | P2 | Each category item in the navigation sidebar must display the count of available compatible packages. |

### 2.3 Screenshots and Previews (`REQ-F-PREV`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-PREV-001` | HiDPI Image Carousel | P1 | The package detail view must display a responsive image gallery supporting light and dark screenshots with smooth keyboard/mouse navigation. |
| `REQ-F-PREV-002` | Color Palette Extraction | P2 | Theme and wallpaper packages must display an extracted swatch palette showing primary background, foreground, and accent hex codes. |
| `REQ-F-PREV-003` | Animated Previews | P2 | Packages with dynamic animations or video wallpapers must support short WebP/GIF/MP4 preview playback with pause/play controls. |
| `REQ-F-PREV-004` | Lazy Loading & Caching | P1 | Preview media must load asynchronously and be cached locally in `~/.cache/woderrise/previews/` with an LRU eviction policy. |

### 2.4 Configuration & Code Inspection (`REQ-F-INSP`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-INSP-001` | File Tree Inspection | P1 | Users must be able to browse the complete directory hierarchy of files contained in the package prior to installation. |
| `REQ-F-INSP-002` | Syntax-Highlighted Code Viewer | P1 | Users must be able to inspect the exact contents of configuration files, CSS, scripts, and manifests with syntax highlighting. |
| `REQ-F-INSP-003` | Pre-Install Unified Diff | P1 | If an installation will alter or overwrite an existing file on the user's system, a side-by-side or unified diff must be rendered showing additions, deletions, and modifications. |
| `REQ-F-INSP-004` | Raw Manifest Inspection | P1 | Users must be able to view the raw `woderrise.yaml` manifest file directly within the inspector tab. |

### 2.5 Package Details (`REQ-F-DET`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-DET-001` | Metadata Display | P1 | Must display: package ID, semantic version, author names/contacts, license, release date, repository URL, and cryptographic SHA-256 hash. |
| `REQ-F-DET-002` | Compatibility Badging | P1 | Must display a clear badge: "Compatible with your Hyprland vX.Y.Z", "Untested", or "Incompatible (Requires Hyprland >= vA.B.C)". |
| `REQ-F-DET-003` | Dependency Tree Display | P1 | Must list all required system packages, store package dependencies, optional enhancements, and declared conflicts. |
| `REQ-F-DET-004` | File Ownership Impact | P1 | Must list all destination filesystem paths that this package will manage upon installation. |

### 2.6 One-Click Installation (`REQ-F-INST`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-INST-001` | Single-Button Trigger | P1 | Supported packages must initiate the full 15-stage installation pipeline upon clicking the "Install" button. |
| `REQ-F-INST-002` | Confirmation & Safety Summary | P1 | Before executing disk writes, an installation confirmation modal must present: affected files, dependencies to install, required backups, and required permissions. |
| `REQ-F-INST-003` | Progress & Step Indication | P1 | The UI must display granular, real-time progress indicators: Downloading, Verifying Hashes, Backing Up, Writing Files, Reloading Services, and Verification. |
| `REQ-F-INST-004` | Non-Blocking Cancellation | P1 | Users must be able to cancel an installation during the download or planning stages with immediate cleanup and zero disk contamination. |
| `REQ-F-INST-005` | Post-Install Verification | P1 | Upon completion, the engine must verify file integrity and service responsiveness before reporting success. |

### 2.7 Dependency Resolution (`REQ-F-DEP`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-DEP-001` | SAT-Based Graph Solver | P1 | The engine must resolve multi-package dependency graphs, enforcing semantic version constraints and detecting cycles. |
| `REQ-F-DEP-002` | Native Package Checking | P1 | Must query local system package databases (e.g. `alpm` on Arch Linux) to detect whether native system dependencies (`waybar`, `rofi-wayland`) are already satisfied. |
| `REQ-F-DEP-003` | Shared Dependency Tracking | P1 | Must record shared store dependencies with reference counts so that uninstalling package A does not remove dependencies still required by package B. |
| `REQ-F-DEP-004` | Conflict Prevention | P1 | Must reject installation plans where two packages attempt to claim exclusive ownership over the same destination file path. |

### 2.8 Updates and Version Pinning (`REQ-F-UPD`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-UPD-001` | Update Detection | P1 | The store must automatically query the registry for new semantic versions of installed packages and display an "Update Available" notification badge. |
| `REQ-F-UPD-002` | Version Pinning | P2 | Users must be able to pin a package to a specific version or branch, preventing automatic update alerts. |
| `REQ-F-UPD-003` | Safe Upgrade Migration | P1 | Upgrades must execute as atomic transactions: create new backups, apply file migrations, verify postconditions, and preserve rollback ability to the prior version. |

### 2.9 Uninstallation (`REQ-F-UNINST`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-UNINST-001` | Clean File Purge | P1 | Uninstallation must remove all files owned exclusively by the package without leaving orphan artifacts in `~/.config`. |
| `REQ-F-UNINST-002` | Backup Restoration Option | P1 | If an installed package replaced an existing user configuration during install, uninstallation must offer an option to restore the original backed-up configuration. |
| `REQ-F-UNINST-003` | Orphan Dependency Sweep | P2 | Must detect when dependencies installed by the package are no longer referenced by any other package and offer to remove them. |

### 2.10 Installation History & Audit (`REQ-F-HIST`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-HIST-001` | Chronological Timeline | P1 | A dedicated "History" view must present all historical transactions (installs, updates, rollbacks, uninstalls) with timestamps and status badges. |
| `REQ-F-HIST-002` | Detailed Transaction Audit | P1 | Clicking any transaction must display: Transaction ID, user, package versions, list of created/modified/deleted files, backup pointers, and log traces. |
| `REQ-F-HIST-003` | Exportable Audit Logs | P2 | Users must be able to export transaction records and diagnostic reports to JSON/Markdown for troubleshooting. |

### 2.11 Transactional Rollback (`REQ-F-ROLL`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-ROLL-001` | One-Click Rollback | P1 | Each completed installation or upgrade transaction in history must feature a "Rollback" action. |
| `REQ-F-ROLL-002` | Inverse Operation Order | P1 | Rollback must reverse operations in strict reverse dependency order: restore modified files, delete created files, and re-apply restored configurations. |
| `REQ-F-ROLL-003` | Optimistic Concurrency Guard | P1 | Prior to restoring a backup file, rollback must verify that the current on-disk checksum matches the installed checksum. If user edits occurred after install, rollback must prompt with a diff viewer instead of clobbering. |
| `REQ-F-ROLL-004` | Atomic Rollback Recovery | P1 | If a rollback operation itself fails mid-execution, the rollback engine must halt safely, retain all backup artifacts, and offer manual recovery steps. |

### 2.12 Cache & Storage Management (`REQ-F-CACHE`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-CACHE-001` | Content-Addressed Storage | P1 | All downloaded archives and artifacts must be indexed by cryptographic SHA-256 hash in `~/.cache/woderrise/artifacts/`. |
| `REQ-F-CACHE-002` | Disk Space Quota & Eviction | P2 | Users can configure cache size limits; LRU eviction must automatically clean unreferenced artifacts while retaining active backups. |
| `REQ-F-CACHE-003` | Manual Cache Purge | P1 | Settings must include a "Clear Download Cache" action that preserves journal records and active backups. |

### 2.13 User Preferences (`REQ-F-PREF`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-PREF-001` | Theme Customization | P1 | Must support System, Dark, and Light appearance modes conforming to XDG Desktop Dark Mode preference. |
| `REQ-F-PREF-002` | Custom Registry Endpoints | P2 | Users must be able to add, toggle, or prioritize third-party Git/HTTP package repositories alongside the official registry. |
| `REQ-F-PREF-003` | Hyprland Reload Trigger | P1 | Users can toggle whether `hyprctl reload` is called automatically upon configuration changes. |

### 2.14 Community Publishing (`REQ-F-PUB`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-PUB-001` | Manifest Linting CLI | P1 | A standalone CLI (`woderrise lint <manifest>`) must validate manifests against the v1 JSON schema, checking syntax, required fields, and asset hashes. |
| `REQ-F-PUB-002` | Package Creation Assistant | P2 | The CLI must provide an interactive generator (`woderrise init`) to scaffold a compliant `woderrise.yaml` from an existing directory. |
| `REQ-F-PUB-003` | Dry-Run Sandbox Test | P1 | Authors must be able to run `woderrise test-install <manifest>` inside an isolated Bubblewrap sandbox to verify clean installation without touching `$HOME`. |

### 2.15 Accessibility & Usability (`REQ-F-A11Y`)

| ID | Title | Priority | Description |
| :--- | :--- | :--- | :--- |
| `REQ-F-A11Y-001` | Full Keyboard Navigation | P1 | Every interaction, list, modal, and button must be 100% accessible via standard keyboard controls (`Tab`, `Shift+Tab`, `Enter`, `Escape`, arrow keys). |
| `REQ-F-A11Y-002` | Screen Reader Semantics | P2 | All widgets must expose standard ATK / AccessKit accessibility roles, labels, and descriptions for screen reader compatibility. |
| `REQ-F-A11Y-003` | High Contrast & Text Scaling | P1 | UI elements must remain fully legible and responsive under 200% font scaling and high contrast modes without clipping. |

---

## 3. Non-Functional Requirements (`REQ-NF`)

### 3.1 Performance (`REQ-NF-PERF`)

| ID | Title | Target | Verification Method |
| :--- | :--- | :--- | :--- |
| `REQ-NF-PERF-001` | Cold App Startup | < 500 ms to interactive window on modern SSD | Automated benchmark |
| `REQ-NF-PERF-002` | Search Latency | < 50 ms for full-text search across 5,000 packages | Profiling harness |
| `REQ-NF-PERF-003` | Transaction Planning | < 250 ms to evaluate compatibility and build install plan | Integration test |
| `REQ-NF-PERF-004` | Memory Footprint | < 90 MB RSS during idle browsing | Resource monitor |

### 3.2 Reliability & Crash Consistency (`REQ-NF-REL`)

| ID | Title | Target | Verification Method |
| :--- | :--- | :--- | :--- |
| `REQ-NF-REL-001` | WAL Journal Integrity | 100% atomic durability; zero corruption across power cuts | SIGKILL fault-injection tests |
| `REQ-NF-REL-002` | Crash Recovery | Automatic detection and safe rollback of incomplete transactions on reboot | Recovery test suite |
| `REQ-NF-REL-003` | Backup Durability | Backups must be verified by SHA-256 before any target file is overwritten | Automated integration test |

### 3.3 Security & Least Privilege (`REQ-NF-SEC`)

| ID | Title | Target | Verification Method |
| :--- | :--- | :--- | :--- |
| `REQ-NF-SEC-001` | Unprivileged Execution | 100% of daemon and GUI operations run as unprivileged user | Automated UID check |
| `REQ-NF-SEC-002` | Polkit Privilege Separation | System package installs use dedicated, isolated Polkit helper | Polkit auth audit |
| `REQ-NF-SEC-003` | Path Traversal Protection | Zero writes permitted outside `$HOME/.config`, `$HOME/.local`, or explicit allowlists | Path traversal fuzzer |
| `REQ-NF-SEC-004` | Sandboxed Script Execution | Script adapters execute strictly within Bubblewrap containers | Sandbox boundary test |

### 3.4 Compatibility & Portability (`REQ-NF-COMPAT`)

| ID | Title | Target | Verification Method |
| :--- | :--- | :--- | :--- |
| `REQ-NF-COMPAT-001` | Compositor Scope | Hyprland >= v0.38.0 on Wayland | Headless Hyprland test |
| `REQ-NF-COMPAT-002` | Distribution Tier 1 | Arch Linux rolling (ALPM / Pacman integration) | CI Linux container |
| `REQ-NF-COMPAT-003` | Distribution Tier 2 | Portable filesystem operations agnostic to Linux distro | Cross-distro test matrix |
