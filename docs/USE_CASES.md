# Use Cases Specification

> **Document ID:** SPEC-PROD-UC-004  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Primary Actors

1. **Desktop User (`USR-01`):** An end-user interacting with the GUI or CLI to customize their Hyprland desktop.
2. **Package Creator (`CRE-01`):** A community author publishing themes, plugins, configs, or full setups.
3. **Core Installation Daemon (`SYS-DAEMON`):** Unprivileged background process orchestrating transactions, journals, and file systems.
4. **Privilege Broker (`SYS-POLKIT`):** Polkit-authenticated helper executing system package manager commands when approved.
5. **Compositor Session (`SYS-HYPR`):** The active Hyprland Wayland compositor process receiving IPC reload and plugin calls.

---

## 2. Comprehensive Use Cases

### UC-01: Browse and Discover Packages
- **Primary Actor:** Desktop User (`USR-01`)
- **Preconditions:** Woderrise GUI is launched.
- **Normal Flow:**
  1. User navigates through category filters (e.g. "Launchers", "Wallpapers", "Full Setups").
  2. System queries the local SQLite metadata index and displays matching cards with titles, authors, badges, and thumbnail previews.
  3. User enters a search query ("catppuccin") in the global search bar.
  4. System updates results in real-time (< 50 ms) ranking by relevance and downloads.
- **Alternate Flows:**
  - *Network Offline:* System displays a subtle banner: *"Offline Mode — Showing cached catalog"*; browsing proceeds seamlessly using local index.
- **Postconditions:** User selects a package card to open the Package Details view.

---

### UC-02: Pre-Installation Inspection and Diff Review
- **Primary Actor:** Desktop User (`USR-01`)
- **Preconditions:** Package Details view is open for a chosen package.
- **Normal Flow:**
  1. User selects the **"Inspect Files & Diffs"** tab.
  2. System displays a full file hierarchy of all files contained in the package.
  3. User clicks an individual file (e.g., `~/.config/hypr/hyprland.conf`).
  4. System compares the package's declared file against the current file on disk.
  5. System renders a side-by-side syntax-highlighted unified diff showing exact lines added, modified, or deleted.
  6. User reviews dependencies, required Polkit permissions, and license information.
- **Postconditions:** User is fully informed of exact disk impacts before initiating installation.

---

### UC-03: Install Standalone Component
- **Primary Actor:** Desktop User (`USR-01`)
- **Preconditions:** Target package has been inspected and user clicks "Install".
- **Normal Flow:**
  1. System checks compatibility (Hyprland version, architecture, display server).
  2. System downloads package artifacts into content-addressed cache (`~/.cache/woderrise/artifacts/<sha256>`).
  3. System validates cryptographic SHA-256 hashes against manifest declaration.
  4. System builds an atomic Installation Plan.
  5. If existing target files will be modified, System copies original files to a timestamped backup directory (`~/.local/share/woderrise/backups/<backup_id>`).
  6. System creates a `PREPARED` transaction record in the SQLite WAL journal.
  7. System transitions transaction to `EXECUTING` and writes files atomically using temporary staging and atomic renames.
  8. System executes declared service reloads (e.g., `hyprctl reload` or `killall -SIGUSR2 waybar`).
  9. System verifies postconditions (files exist, hashes match, service active).
  10. System commits transaction state to `COMMITTED` in journal.
  11. UI presents a success notification and records the transaction in History.
- **Error Conditions & Recovery:**
  - *E1: Checksum mismatch:* System deletes corrupt cached artifact, halts execution before touching target directories, and alerts user.
  - *E2: Disk space exhausted:* System halts at step 4, aborts transaction without modifying filesystem, and alerts user with required byte count.
- **Postconditions:** Component is active; previous state is safely backed up; journal record is durable.

---

### UC-04: Install Complete Desktop Setup ("Full Rice")
- **Primary Actor:** Desktop User (`USR-01`)
- **Preconditions:** User selects a multi-component desktop setup (e.g., "Aura Cyberpunk Full Rice").
- **Normal Flow:**
  1. System identifies package type as `desktop_setup`.
  2. System displays a high-prominence summary modal: *"Installing this full setup will configure Hyprland, Waybar, Rofi, SwayNC, and terminal themes. A complete snapshot of your current environment will be preserved for instant rollback."*
  3. User confirms. System opens a Parent Transaction.
  4. System takes a comprehensive snapshot of affected configuration directories (`~/.config/hypr/`, `~/.config/waybar/`, etc.).
  5. System resolves all child package dependencies.
  6. If native packages (`swww`, `swaync`, `grimblast`) are missing, System prompts user via Polkit and installs them.
  7. System stages and atomically deploys all child component configs in topological order.
  8. System restarts session daemons via `systemd --user` and executes `hyprctl reload`.
  9. System marks Parent Transaction as `COMMITTED`.
- **Alternate / Error Flows:**
  - *Sub-component Failure:* If any child component fails during execution, System halts, executes sub-transaction compensation, restores the pre-install snapshot, and leaves the user's prior desktop completely intact.
- **Postconditions:** Complete desktop environment is active; full restore snapshot is indexed in History.

---

### UC-05: Update Installed Package
- **Primary Actor:** Desktop User (`USR-01`)
- **Preconditions:** Package is installed; an updated version exists in the registry.
- **Normal Flow:**
  1. UI displays an "Update" button with version delta (e.g. `1.1.0 -> 1.2.0`).
  2. User clicks "Update".
  3. System generates an update plan: compares files between v1.1.0 and v1.2.0.
  4. System backs up the current v1.1.0 on-disk state.
  5. System applies the new version's files atomically.
  6. System updates package database reference and commits update transaction.
- **Postconditions:** Package is updated; user can roll back to v1.1.0 at any time.

---

### UC-06: Uninstall Package with Backup Restoration
- **Primary Actor:** Desktop User (`USR-01`)
- **Preconditions:** Package is currently marked as `INSTALLED`.
- **Normal Flow:**
  1. User navigates to "Installed" tab and clicks "Uninstall".
  2. System checks if this package replaced original user files during its initial install.
  3. System prompts user:
     - Option A: *"Restore original pre-installation files"*
     - Option B: *"Clean delete (do not restore prior files)"*
  4. User chooses Option A.
  5. System removes files owned exclusively by the package.
  6. System restores original files from the verified backup directory.
  7. System executes service reloads.
  8. System records an `UNINSTALL` transaction in the journal.
- **Postconditions:** Package is uninstalled; original pre-install environment is cleanly restored.

---

### UC-07: Rollback Completed Transaction
- **Primary Actor:** Desktop User (`USR-01`)
- **Preconditions:** At least one completed installation or update exists in History.
- **Normal Flow:**
  1. User opens "History" view and selects a transaction (e.g., `TXN-1044: Installed Waybar Gradient`).
  2. User clicks **"Rollback This Transaction"**.
  3. System checks **Optimistic Concurrency**:
     - Computes current checksums of all installed files.
     - Compares against checksums recorded at time of installation.
     - Hashes match (user made no manual file edits).
  4. System executes reverse operations:
     - Deletes files created by the transaction.
     - Restores previous file versions from backup.
     - Restores previous configuration injections.
  5. System reloads affected services.
  6. System records `ROLLBACK` transaction in SQLite journal.
- **Alternate Flow (User Modified Files):**
  - Current checksums do not match installation checksums (user edited files after install).
  - System presents a **Conflict Resolution Dialog**:
    - *"File `~/.config/waybar/config.jsonc` was modified by you after installation."*
    - Options: [Keep My Changes] / [Overwrite with Backup] / [View Diff].
  - User chooses action; system proceeds safely without data loss.
- **Postconditions:** System state is reversed to pre-transaction condition.

---

### UC-08: Recover from Interrupted Installation / System Crash
- **Primary Actor:** Core Installation Daemon (`SYS-DAEMON`)
- **Preconditions:** System lost power, crashed, or process was terminated mid-transaction.
- **Normal Flow:**
  1. On next startup, Daemon opens SQLite WAL journal.
  2. Scans for transactions with state `EXECUTING` or `PREPARED`.
  3. Finds incomplete transaction `TXN-1052`.
  4. Daemon initiates **Crash Recovery Routine**:
     - Reads backup IDs associated with `TXN-1052`.
     - Deletes any partially written temporary or target files.
     - Restores original files from backup snapshots.
     - Updates journal state to `ABORTED_RECOVERED`.
  5. GUI displays a desktop notification: *"Woderrise detected an interrupted installation and safely restored your configuration."*
- **Postconditions:** No dirty state or broken configuration left on disk.

---

### UC-09: Resolve File Ownership Conflict
- **Primary Actor:** Desktop User (`USR-01`)
- **Preconditions:** User attempts to install Package B, which claims a file path already owned by Package A.
- **Normal Flow:**
  1. System detects collision: `~/.config/waybar/config.jsonc` is owned by `Package A (Installed)`.
  2. System blocks direct overwrite and displays a conflict modal:
     - *"Conflict: Package B modifies the Waybar configuration currently managed by Package A."*
     - Available options:
       - **Replace Package A:** Uninstall Package A (with automatic backup) and install Package B.
       - **Cancel Installation:** Abort without making changes.
  3. User selects "Replace Package A".
  4. System executes compound transaction: uninstalls A, installs B.
- **Postconditions:** Exactly one package owns the file; ownership table remains strictly consistent.

---

### UC-10: Install Package with Privileged System Dependencies
- **Primary Actor:** Desktop User (`USR-01`) + Privilege Broker (`SYS-POLKIT`)
- **Preconditions:** Package requires an uninstalled native system package (e.g. `hyprpaper`).
- **Normal Flow:**
  1. System identifies missing dependency: `hyprpaper`.
  2. UI displays an informational prompt: *"This package requires system package 'hyprpaper' (1.8 MB). Administrator authorization is required."*
  3. User clicks "Authorize".
  4. Polkit dialog opens: `org.woderrise.install-system-packages`.
  5. User inputs password / fingerprint.
  6. Privileged helper invokes ALPM / pacman to install `hyprpaper`.
  7. Helper drops privileges and returns success code to Daemon.
  8. Unprivileged Daemon proceeds with user dotfile placement.
- **Postconditions:** Native dependency is installed; core daemon ran completely unprivileged.

---

### UC-11: Offline Package Inspection and Uninstallation
- **Primary Actor:** Desktop User (`USR-01`)
- **Preconditions:** Machine has no internet connectivity.
- **Normal Flow:**
  1. User opens Woderrise in offline mode.
  2. Navigates to "Installed" tab.
  3. Inspects installed package files and local documentation.
  4. User clicks "Uninstall".
  5. Engine utilizes local journal and local backup artifacts to cleanly remove files and restore previous configs.
- **Postconditions:** Full uninstallation and rollback capabilities function 100% offline.

---

### UC-12: Creator Manifest Linting and Dry-Run Testing
- **Primary Actor:** Package Creator (`CRE-01`)
- **Preconditions:** Creator has authored a `woderrise.yaml` file in their dotfile repository.
- **Normal Flow:**
  1. Creator runs CLI command: `woderrise lint woderrise.yaml`.
  2. Tool validates syntax, JSON schema compliance, required fields, and semantic versioning.
  3. Creator runs: `woderrise test-install woderrise.yaml`.
  4. System spawns an isolated Bubblewrap sandbox with a dummy `$HOME`.
  5. Executes full installation pipeline inside the sandbox.
  6. Verifies file placements, permissions, and exit codes.
  7. Prints a green validation report: *"Manifest valid. 0 errors, 0 warnings. Safe for registry submission."*
- **Postconditions:** Package is verified ready for community pull request.
