# Acceptance Criteria (MVP)

> **Document ID:** SPEC-PROD-AC-005  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Overview & Testing Methodology

This document establishes the testable, unambiguous acceptance criteria for the Minimum Viable Product (MVP) of **Woderrise**. Every requirement is framed using the **Given-When-Then** specification format, paired with explicit pass/fail verification thresholds.

Vague statements (e.g. *"system should be reliable"*, *"rollback should work"*) are strictly prohibited.

---

## 2. Core MVP Acceptance Criteria

### AC-01: Catalog Browsing & Full-Text Search
- **Requirement Reference:** `REQ-F-DISC-001`, `REQ-F-DISC-002`, `REQ-NF-PERF-002`
- **Criteria:**
  - **Scenario 1: Sub-50ms Search Response**
    - **Given** a local registry database containing 2,000 package manifests.
    - **When** the user types the search query `"catppuccin"` into the search input.
    - **Then** the filtered results list must render within **50 milliseconds** (measured via tracing span).
    - **And** all returned results must contain `"catppuccin"` in their title, description, tags, or package ID.
  - **Scenario 2: Offline Browsing Continuity**
    - **Given** the machine has no network interface connected (`ip link set lo up` only).
    - **When** the user launches Woderrise and selects the "Waybar" category.
    - **Then** the UI must render previously cached catalog entries without displaying network error popups or crashing.

---

### AC-02: Pre-Installation File Diff Inspection
- **Requirement Reference:** `REQ-F-INSP-001`, `REQ-F-INSP-003`, `REQ-F-PREV-001`
- **Criteria:**
  - **Scenario 1: Accurate Side-by-Side Diff Display**
    - **Given** an existing user file `~/.config/waybar/config.jsonc` containing 50 lines.
    - **And** a target package that provides an updated `config.jsonc` modifying 10 lines and adding 5 lines.
    - **When** the user clicks "Inspect Files & Diffs" in the package details view.
    - **Then** the UI must display a unified diff with green highlighting for the 5 added lines and red highlighting for modified/removed lines.
    - **And** no disk write operations must occur during inspection.

---

### AC-03: Atomic Installation & File Ownership
- **Requirement Reference:** `REQ-F-INST-001`, `REQ-F-INST-005`, `REQ-NF-REL-003`
- **Criteria:**
  - **Scenario 1: Clean File Deployment with Checksum Verification**
    - **Given** an unmanaged clean directory `~/.config/rofi/`.
    - **When** the user installs package `com.creator.rofi-nord` declaring 3 files with verified SHA-256 hashes.
    - **Then** all 3 files must exist at the target paths with exact declared permissions (`0644`).
    - **And** the SHA-256 checksum of each deployed file on disk must match the manifest declaration.
    - **And** the local ownership database must record `com.creator.rofi-nord` as the exclusive owner of all 3 paths.

---

### AC-04: Mandatory Backups Before File Overwrite
- **Requirement Reference:** `REQ-F-INST-002`, `REQ-F-ROLL-001`
- **Criteria:**
  - **Scenario 1: Pre-Modification Snapshot Creation**
    - **Given** an existing user configuration file `~/.config/hypr/hyprland.conf` with SHA-256 hash `HASH_ORIGINAL`.
    - **When** the user installs a package that overwrites `hyprland.conf`.
    - **Then** a backup copy must be written to `~/.local/share/woderrise/backups/<backup_id>/` **before** the target file is touched.
    - **And** the backup file's SHA-256 hash must be verified equal to `HASH_ORIGINAL`.
    - **And** if backup creation fails (e.g. disk read error), the installation must immediately abort with zero writes to `~/.config/hypr/hyprland.conf`.

---

### AC-05: Crash Recovery from Interrupted Transactions
- **Requirement Reference:** `REQ-NF-REL-001`, `REQ-NF-REL-002`
- **Criteria:**
  - **Scenario 1: Power Loss / SIGKILL during File Deployment**
    - **Given** an active installation transaction modifying 5 files.
    - **When** the daemon process is abruptly terminated (`kill -9`) after modifying 2 files but before committing the transaction.
    - **When** the daemon is restarted.
    - **Then** the daemon startup scanner must detect the incomplete transaction in state `EXECUTING`.
    - **And** the daemon must automatically restore the 2 modified files from their pre-installation backup copies.
    - **And** the daemon must delete any orphan temporary staging files (`*.woderrise.tmp`).
    - **And** the transaction state in the SQLite WAL journal must transition to `ABORTED_RECOVERED`.
    - **And** the user's filesystem must be byte-for-byte identical to its state prior to the transaction.

---

### AC-06: Optimistic Concurrency Guard on Rollback
- **Requirement Reference:** `REQ-F-ROLL-003`, `REQ-F-ROLL-004`
- **Criteria:**
  - **Scenario 1: Clean Rollback when Unmodified**
    - **Given** package `com.user.theme` was installed in transaction `TXN-100`.
    - **And** no files owned by `com.user.theme` have been modified since install.
    - **When** the user clicks "Rollback" on `TXN-100`.
    - **Then** all files installed by `com.user.theme` must be deleted or restored to their pre-install backup state.
    - **And** the transaction history must record a `ROLLBACK` event.
  - **Scenario 2: Conflict Interception on Post-Install Edits**
    - **Given** package `com.user.theme` installed file `~/.config/waybar/style.css`.
    - **And** the user subsequently manually edited `style.css` via a text editor (changing its hash).
    - **When** the user clicks "Rollback" on `com.user.theme`.
    - **Then** the engine must **block silent rollback**.
    - **And** the UI must display a "File Modified by User" dialog showing a diff between the user's current version and the original backup.
    - **And** the user must be prompted to choose: [Keep My Edits] or [Overwrite with Backup].

---

### AC-07: Uninstallation Without Orphan File Leaks
- **Requirement Reference:** `REQ-F-UNINST-001`, `REQ-F-UNINST-002`
- **Criteria:**
  - **Scenario 1: Complete Directory Cleanup**
    - **Given** package `com.creator.fastfetch` installed 4 assets into `~/.config/fastfetch/`.
    - **When** the user clicks "Uninstall" and chooses "Clean Delete".
    - **Then** all 4 files must be deleted from disk.
    - **And** if `~/.config/fastfetch/` is now empty, the empty directory must be pruned.
    - **And** unrelated directories in `~/.config/` must remain completely untouched.

---

### AC-08: Privilege Separation & Minimal Root
- **Requirement Reference:** `REQ-NF-SEC-001`, `REQ-NF-SEC-002`
- **Criteria:**
  - **Scenario 1: Zero Root for User Configurations**
    - **Given** an installation containing only user dotfiles and wallpapers.
    - **When** the installation executes from start to finish.
    - **Then** the operating UID of all executing processes must remain equal to the unprivileged user (`UID != 0`).
    - **And** zero Polkit authentication prompts must be requested.
  - **Scenario 2: Granular Polkit Escalation for ALPM Packages**
    - **Given** a package requiring native Arch package `swww`.
    - **When** the user approves installation.
    - **Then** the Polkit authorization dialogue must explicitly identify the action `org.woderrise.install-system-packages` and list the exact package (`swww`).
    - **And** the privileged helper must terminate immediately after package installation.

---

### AC-09: Sandboxed Script Execution Boundaries
- **Requirement Reference:** `REQ-NF-SEC-004`, `REQ-F-PUB-003`
- **Criteria:**
  - **Scenario 1: Isolated Environment for Optional Script Hooks**
    - **Given** a legacy package providing a post-install configuration script.
    - **When** the script executes.
    - **Then** execution must occur inside a `bwrap` (Bubblewrap) container with:
      - Read-only root filesystem (`/`, `/usr`, `/etc` mounted `--ro-bind`).
      - Confined `$HOME` with write access limited strictly to declared target paths.
      - Network access disabled (`--unshare-net`) unless explicitly declared.
      - Process isolation (`--unshare-pid`).
    - **And** if the script attempts to write outside the allowlisted directory, the OS must deny the write with `EPERM` / `EACCES`.

---

## 3. MVP Pass/Fail Verification Matrix

| Area | Test Suite | Pass Threshold | Blocking for Release? |
| :--- | :--- | :--- | :--- |
| **Catalog & Search** | `tests/unit/catalog_search_test.rs` | 100% pass, search < 50ms | Yes |
| **Package Manifests** | `tests/unit/manifest_parser_test.rs` | 100% schema validation | Yes |
| **Atomic File Engine** | `tests/integration/atomic_swap_test.rs` | 100% pass, zero dirty writes | Yes |
| **WAL Transaction Journal**| `tests/integration/wal_journal_test.rs` | 100% durability | Yes |
| **Crash & Fault Injection**| `tests/fault_injection/crash_recovery.rs` | 100% recovery across 15 pipeline stages | Yes |
| **Optimistic Concurrency** | `tests/integration/rollback_concurrency.rs` | 100% collision interception | Yes |
| **Polkit Security** | `tests/security/privilege_separation.rs`| Zero unauthorized escalations | Yes |
| **Bubblewrap Sandbox** | `tests/security/bwrap_isolation.rs` | Zero path escapes | Yes |
