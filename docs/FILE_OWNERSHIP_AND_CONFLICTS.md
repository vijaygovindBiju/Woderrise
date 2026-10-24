# File Ownership and Conflict Resolution

> **Document ID:** SPEC-ENG-OWN-017  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Architectural Philosophy & The Ownership Invariant

In standard Linux package management (e.g. `pacman`), files in `/usr/` are strictly owned by exactly one package; collisions cause immediate transaction errors. In user-session dotfiles (`$HOME/.config`), however, multiple components frequently need to interact with the same applications (e.g. one package provides a Waybar style, another provides a custom Waybar battery module script, and the user has their own existing custom configs).

To maintain order without sacrificing flexibility, Woderrise establishes the **File Ownership Invariant**:

> [!IMPORTANT]
> **The File Ownership Invariant:**
> 1. Every filesystem path under `$HOME` is classified as either **User-Owned (Unmanaged)** or **Store-Managed (Owned)**.
> 2. For any file managed via `strategy: replace`, exactly **one active package** holds exclusive ownership.
> 3. No package may modify a User-Owned file without creating a verified pre-modification backup first.
> 4. Ownership records are persisted in a dedicated SQLite database: `~/.local/share/woderrise/ownership.db`.

---

## 2. File Ownership Database Schema

```sql
CREATE TABLE IF NOT EXISTS file_ownership (
    path TEXT PRIMARY KEY,                         -- Normalized absolute path (e.g. '/home/user/.config/waybar/config.jsonc')
    package_id TEXT NOT NULL,                      -- Owning package ID
    installed_version TEXT NOT NULL,               -- Version when ownership was claimed
    installed_sha256 TEXT NOT NULL,                -- Hash of file as deployed by the package
    file_mode INTEGER NOT NULL DEFAULT 420,        -- Octal 0644
    is_symlink INTEGER NOT NULL DEFAULT 0,         -- 1 if path is a symlink
    backup_id TEXT,                                -- Backup ID of original file that existed prior to ownership
    claimed_at INTEGER NOT NULL                    -- Millisecond timestamp
);

CREATE INDEX IF NOT EXISTS idx_owner_package ON file_ownership(package_id);
```

---

## 3. Conflict Scenarios & Resolution Mechanics

```mermaid
flowchart TD
    Req[Package B requests write to: ~/.config/waybar/config.jsonc] --> Query[Query file_ownership Table]
    Query --> Status{Is path in ownership table?}
    
    Status -->|No (Path is Unmanaged)| CheckDisk{Does file exist on disk?}
    CheckDisk -->|No| CleanCreate[Clean Create: Set Owner = Package B]
    CheckDisk -->|Yes (Existing User Dotfile)| PromptUserBackup[Snapshot to Backup & Transfer Owner to Package B]

    Status -->|Yes (Already Managed)| SameOwner{Is current owner Package B?}
    SameOwner -->|Yes (Package Upgrade)| UpgradeFile[Atomic Upgrade: Update Hash & Version]
    SameOwner -->|No (Owned by Package A)| Collision[Path Collision Detected]

    Collision --> Modal[Present Conflict Resolution Dialog]
    Modal --> Choice{User Resolution Selection}
    Choice -->|Replace Package A| CompoundSwap[Trigger Compound Swap: Uninstall A -> Install B]
    Choice -->|Cancel| AbortInstall[Abort Installation with Zero Changes]
```

### Scenario 1: Target File Does Not Exist
- Target directory is created if needed.
- File is deployed atomically.
- Ownership is assigned to the installing package.

### Scenario 2: Target File Exists but is Unmanaged (User's Pre-Existing Custom Config)
- The user had already created `~/.config/hypr/hyprland.conf` before installing Woderrise.
- The engine creates a full content-addressed backup snapshot (`backup_snapshots`).
- Deploys the package file.
- Records ownership under the package, linking the `backup_id` so that uninstallation can cleanly restore the user's original configuration.

### Scenario 3: Target File is Owned by Another Package (Collision)
- Package A is currently installed and owns `~/.config/waybar/config.jsonc`.
- User attempts to install Package B, which also provides `~/.config/waybar/config.jsonc`.
- The engine blocks direct overwrite and prompts the user with the **Package Replacement Dialog**:
  - *"Package B conflicts with installed Package A. Would you like to replace Package A?"*
  - If approved, the engine creates a compound transaction: uninstalls Package A (backing up its files) and installs Package B.

---

## 4. Atomic File Replacement Semantics

To prevent partial writes, corrupted configuration files, or race conditions where background daemons read half-written configs:

1. **Sibling Staging File:**
   The file is written to a hidden sibling file in the same directory:
   `<target_path>.woderrise.tmp.<uuid>`
2. **Synchronize to Disk (`fsync`):**
   The temporary file is flushed to physical storage using `fsync()` to ensure data integrity.
3. **Atomic POSIX Swap:**
   The engine invokes the Linux `renameat2()` system call with the `RENAME_EXCHANGE` flag (or standard `rename()` if replacing):
   - Atomically replaces the target path with the temporary file in a single filesystem inode operation.
   - If an error occurs, the original file remains 100% intact.
4. **Prune Staging File:**
   Any leftover staging or swapped temporary files are unlinked.

---

## 5. Symlinks and Directory Traversal Safety

To prevent malicious packages or broken symlinks from escaping the user directory:
- **Canonical Path Resolution:** Target paths are resolved using `canonicalize()`. If any symlink resolves outside of `$HOME` (e.g. attempting to redirect to `/etc` or `/var`), the transaction is aborted immediately.
- **No In-Place Symlink Overwrites:** If a target path is an existing symlink pointing to an arbitrary location, Woderrise deletes the symlink itself rather than following it and overwriting the target destination file.
- **Strict POSIX Modes:** Configuration files are explicitly enforced as `0644` (`rw-r--r--`). Scripts and executable binaries are enforced as `0755` (`rwxr-xr-x`). Dangerous setuid/setgid bits (`4755`, `2755`) are strictly forbidden and stripped during validation.
