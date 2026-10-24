# Uninstallation and Update Lifecycle

> **Document ID:** SPEC-ENG-UPD-018  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Uninstallation vs. Rollback: Key Architectural Differences

While both operations remove package artifacts from disk, their intent and filesystem impacts differ substantially:

| Dimension | Package Uninstallation | Transaction Rollback |
| :--- | :--- | :--- |
| **Trigger** | User navigates to "Installed" tab and clicks "Uninstall". | User opens "History" tab and clicks "Rollback" on a specific transaction. |
| **Scope** | Targets a specific installed package regardless of when it was installed. | Targets a specific chronological event (e.g. reversing a transaction from 3 weeks ago). |
| **Backup Restoration** | **Optional.** User can choose between "Clean Delete" (remove files) or "Restore Original State" (restore pre-install backup). | **Mandatory.** Restores the exact prior byte-for-byte configuration recorded in the transaction snapshot. |
| **Dependency Impact** | Checks reference counts; offers to purge orphan shared dependencies. | Reverses the exact dependency state changes recorded during the original transaction. |

---

## 2. The Uninstallation Pipeline

```mermaid
flowchart TD
    Start[User Clicks Uninstall on Package X] --> CheckDeps{Is Package X a dependency of another package?}
    CheckDeps -->|Yes (ref_count > 0)| Block[Block Uninstallation: Display Dependent Packages]
    CheckDeps -->|No| CheckBackup{Did Package X replace pre-existing user files?}
    
    CheckBackup -->|Yes| PromptUser[Prompt: Restore Pre-Install Backup or Clean Delete?]
    PromptUser --> UserChoice{User Choice}
    UserChoice -->|Restore Backup| RestorePlan[Formulate Restore Plan]
    UserChoice -->|Clean Delete| DeletePlan[Formulate Pure Deletion Plan]
    CheckBackup -->|No| DeletePlan
    
    RestorePlan --> Exec[Execute Transaction Atomically]
    DeletePlan --> Exec
    
    Exec --> PruneFiles[Remove Exclusively Owned Files]
    Exec --> RestoreOld[Swap In Original Backups If Selected]
    Exec --> PruneDirs[Prune Empty Parent Directories]
    Exec --> ServiceReload[Execute Service Reloads]
    Exec --> UpdateDB[Remove from file_ownership & Add UNINSTALL to Journal]
    UpdateDB --> CheckOrphans[Detect Orphan Store Dependencies & Offer Cleanup]
```

### 2.1 Orphan Dependency Cleanup
When Package A is uninstalled:
1. The engine checks all store dependencies declared by Package A.
2. For each dependency, decrements `ref_count` by 1 in the local database.
3. If any dependency reaches `ref_count == 0` and was marked as `installed_implicitly`:
   - It is flagged as an **Orphan Candidate**.
   - The user is presented with a non-intrusive prompt:
     *"Package B (GTK Theme) is no longer required by any installed package. Would you like to remove it?"*
   - If approved, it is cleanly removed; if declined, it transitions to `installed_explicitly`.

---

## 3. Package Updates and Safe Upgrades

Updating a package from version `1.0.0` to `1.1.0` is treated as an **Atomic Migration Transaction**:

### 3.1 Upgrade Execution Pipeline
1. **Pre-Upgrade Inspection:** The user reviews changelog notes and a side-by-side unified diff of modified configuration files.
2. **Current State Backup:** The engine snapshots the active `1.0.0` state on disk to `~/.local/share/woderrise/backups/`.
3. **Optimistic Concurrency Check:** The engine verifies whether the user customized files while using `1.0.0`.
   - If customized: Prompts user to merge changes or keep custom settings.
4. **Deploy Updated Files:** Deploys `1.1.0` files atomically via staging files and POSIX renames.
5. **Update Registry & Ownership:** Transitions `installed_version` to `1.1.0` and records updated file hashes in `file_ownership`.
6. **Commit Update Transaction:** Persists `UPDATE` transaction in the journal.

### 3.2 Rollback of Upgrades
Because a full snapshot of the `1.0.0` state was committed to the journal during Step 2:
- The user can open "History" and click **"Rollback Update"** at any time.
- The engine instantly swaps the system back to the exact byte-for-byte `1.0.0` configuration.

---

## 4. Failed Upgrade Recovery

If an upgrade fails mid-process (e.g. artifact hash mismatch, network interruption, or service reload failure):
1. The engine immediately aborts the upgrade.
2. It restores the `1.0.0` snapshot from the pre-upgrade backup.
3. Reloads services to the `1.0.0` configuration.
4. Marks the transaction as `ABORTED_RECOVERED` in the journal.
5. The user's desktop remains completely stable on `1.0.0`, with a diagnostic dialog explaining why the update failed.
