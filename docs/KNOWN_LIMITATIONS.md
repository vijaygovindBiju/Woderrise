# Known Limitations and Explicit Non-Guarantees

> **Document ID:** SPEC-ENG-LIM-039  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Explicit Architectural Non-Guarantees

To ensure honesty and avoid creating a false sense of security, Woderrise explicitly documents what the system **cannot** promise or guarantee:

### 1.1 Non-Reversibility of Arbitrary Shell Script Side Effects
While Woderrise provides guaranteed atomic rollback for all declarative file operations and configuration injections, it **cannot** guarantee reversal of arbitrary shell scripts.
- If a community script makes outbound HTTP calls, formats an external USB drive, or registers an external cloud token, those side effects cannot be undone by a file journal rollback.
- *Mitigation:* Declarative manifests are enforced by default; community scripts are strictly quarantined within unshared Bubblewrap containers.

### 1.2 Non-Destructive Handling of Native System Binaries
When a package installs a native system dependency (e.g. `rofi-wayland` via `pacman`), rolling back or uninstalling the Woderrise package will **not automatically or silently remove the system binary** without explicit confirmation.
- *Rationale:* The user or another external application may have begun utilizing that binary independently. Woderrise flags it as an orphan candidate and prompts the user.

### 1.3 Full-Disk Disaster Recovery vs. User Dotfile Management
Woderrise is an atomic transaction manager for **user session dotfiles and assets** (`$HOME/.config`, `$HOME/.local`).
- It is **not** an operating system disaster recovery tool (such as Btrfs Snapper or Timeshift). If a user corrupts their Linux kernel, systemd boot configuration, or `/etc/fstab`, Woderrise cannot restore the operating system.

---

## 2. Hard Technical Constraints & Environmental Boundaries

### 2.1 Hyprland C++ Dynamic Plugin ABI Instability
- **Constraint:** Hyprland does not maintain a stable C++ ABI across releases or Git commits.
- **Impact:** Pre-compiled plugin `.so` files can only be dynamically loaded if they were compiled against the exact C++ headers matching the user's running `hyprctl version` commit hash.
- **Handling:** If a commit mismatch is detected, Woderrise blocks loading to prevent crashing the active Wayland compositor session. It prompts the user to either compile locally via `hyprpm` or wait for an updated release.

### 2.2 POSIX-Compliant Filesystems Only
- **Constraint:** Atomic file swapping (`renameat2`), hardlinks, and octal permission modes (`0644`, `0755`) require a POSIX-compliant Linux filesystem (such as `ext4`, `btrfs`, `xfs`, or `zfs`).
- **Impact:** Storing `$HOME` on non-POSIX filesystems (FAT32, exFAT, or NTFS via FUSE) is **unsupported** and blocked during pre-flight checks.

### 2.3 Single Desktop User Session Scope
- **Constraint:** Woderrise operates within the context of a single active user desktop session on D-Bus.
- **Impact:** Multi-seat setups or shared multi-user home directories with conflicting concurrent customization sessions are currently out of scope.

---

## 3. Systematic Handling of the 15 Hard Problems

The following matrix documents the proposed handling, relevant subsystem, and verification tests for the 15 hard problems inherent to desktop customization:

| # | The Hard Problem | Proposed Subsystem & Handling | Automated Test Verification |
| :-: | :--- | :--- | :--- |
| **1** | Overwriting a configuration file customized by the user. | **`BackupManager` + `FileOwnership`:** Creates content-addressed backup prior to modification. | `tests/integration/atomic_swap_test.rs` |
| **2** | Multiple packages attempting to modify the same file. | **`PathMutex` & Conflict Engine:** Mutex prevents multiple owners. Prompt user with Compound Swap Plan. | `tests/integration/conflict_resolution_test.rs` |
| **3** | A bundle installs some components successfully before one fails. | **`Sub-Transaction Compensator`:** Automatic rollback of all preceding sub-transactions under parent. | `tests/integration/bundle_failure_test.rs` |
| **4** | Package requires an incompatible Hyprland plugin version. | **`CompatibilityEngine`:** Pre-flight commit hash check blocks install if ABI headers mismatch. | `tests/unit/compat_checker_test.rs` |
| **5** | Machine crashes after modifying files but before journal update. | **`CrashRecoveryScanner`:** Detects `EXECUTING` state on boot; restores from verified backup. | `tests/fault_injection/crash_recovery.rs` |
| **6** | User edits files after installation and then requests rollback. | **`OptimisticConcurrencyGuard`:** Checksum mismatch halts rollback; prompts user with 3-way diff. | `tests/integration/rollback_concurrency.rs` |
| **7** | Several packages depend on the same system application. | **`DependencyResolver`:** Reference counting (`ref_count`). Package stays active until `ref_count == 0`. | `tests/unit/dependency_ref_count_test.rs` |
| **8** | Package installs a background service or alters startup. | **`ServiceTriggerEngine`:** Scoped systemd user units and `hyprland.conf` injections with unique markers. | `tests/integration/service_trigger_test.rs` |
| **9** | Installer lacks permission to perform a required operation. | **`PolkitHelper`:** Unprivileged daemon delegates strictly typed native installs via Polkit prompt. | `tests/security/privilege_separation.rs` |
| **10** | Community script performs changes store cannot reverse. | **`ScriptPolicy` + `Bubblewrap`:** Scripts confined to networkless sandbox; explicit non-reversal disclaimer. | `tests/security/bwrap_isolation.rs` |
| **11** | Package source disappears upstream after installation. | **`ContentAddressedCache`:** Cached artifacts remain local; mirror network retains immutable releases. | `tests/integration/offline_cache_test.rs` |
| **12** | Application update changes the journal database schema. | **`SQLite PRAGMA user_version`:** Automated, non-destructive migration scripts run before IPC serve. | `tests/integration/schema_migration_test.rs` |
| **13** | User triggers concurrent installation requests. | **`EngineMutexLock`:** Non-blocking file lock rejects or queues second request with active transaction alert. | `tests/integration/concurrency_lock_test.rs` |
| **14** | Full desktop setup conflicts with existing user environment. | **`MandatorySnapshotEngine`:** Full byte-for-byte pre-install snapshot of entire `~/.config/hypr/`. | `tests/integration/full_setup_snapshot_test.rs` |
| **15** | Rollback itself fails halfway through. | **`SecondaryRecoveryEngine`:** Rollback logged as first-class transaction; resumes safely on reboot. | `tests/fault_injection/rollback_crash_test.rs` |
