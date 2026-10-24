# Backup and Cache Management

> **Document ID:** SPEC-ENG-BC-016  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Architectural Separation: Cache vs. Backups vs. Journal

To prevent data loss and ensure clean system hygiene, Woderrise strictly isolates three storage domains:

```mermaid
flowchart TD
    subgraph Cache [1. Ephemeral Cache (~/.cache/woderrise/)]
        Art[Artifact Cache: artifacts/<sha256>]
        Prev[Preview Cache: previews/<sha256>]
        RegCache[Registry Shard Cache]
    end

    subgraph Backups [2. Durable Backups (~/.local/share/woderrise/backups/)]
        Snap[Content-Addressed File Snapshots]
        Meta[Backup Metadata Manifests]
    end

    subgraph Journal [3. Transaction Journal (~/.local/share/woderrise/)]
        DB[(journal.db SQLite WAL)]
        Ownership[(ownership.db SQLite)]
    end

    Cache -.->|Evictable at Any Time| Purge[Can be wiped without data loss]
    Backups -->|Pinned by Active Installs| Safety[Never wiped while package is installed]
    Journal -->|Immutable History| SourceOfTruth[Permanent Audit & Recovery Engine]
```

| Subsystem | Storage Path | Volatility | Cleanable via "Clear Cache"? |
| :--- | :--- | :--- | :--- |
| **Download Cache** | `~/.cache/woderrise/artifacts/` | **Ephemeral** | **YES.** Re-downloadable from remote sources. |
| **Preview Cache** | `~/.cache/woderrise/previews/` | **Ephemeral** | **YES.** Re-fetched on demand. |
| **Backup Storage** | `~/.local/share/woderrise/backups/` | **Durable** | **NO.** Critical for rollback and restore. |
| **Transaction Journal** | `~/.local/share/woderrise/journal.db`| **Permanent** | **NO.** System audit and recovery state. |

---

## 2. Backup Management Specification

### 2.1 Content-Addressed Backup Storage
When an existing user file is modified, replaced, or deleted, a byte-for-byte uncompressed snapshot is copied to:
`~/.local/share/woderrise/backups/<backup_id>/<sha256>`

- **Storage Optimization via Hardlinks / Deduplication:** If the user installs multiple themes that successively replace the exact same default file, the content-addressed storage deduplicates identical byte payloads using content hashing.
- **Metadata Persistence:** Each backup is indexed in the SQLite table `backup_snapshots` recording:
  - `original_path` (e.g. `/home/user/.config/hypr/hyprland.conf`)
  - `sha256` digest
  - `file_mode` (POSIX permissions `0644`)
  - `mtime` (original modification timestamp)

### 2.2 Backup Pinned Invariant
> [!IMPORTANT]
> A backup snapshot is **pinned** as long as the package that triggered its creation remains installed. Pinned backups are strictly protected from deletion or pruning. Only when a package is cleanly uninstalled or its rollback window is permanently closed by user action can associated backups be scheduled for pruning.

---

## 3. Cache Storage & Eviction Policies

### 3.1 Cache Layout
```text
~/.cache/woderrise/
├── artifacts/
│   ├── 8f/
│   │   └── 8f434346648f6b96df89dda901c5176b10a6d83961dd3c1ac88b59b2dc327aa4
│   └── 2c/
│       └── 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
└── previews/
    └── e3/
        └── e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855.webp
```

### 3.2 Quotas and LRU Eviction
1. **Configurable Cache Quota:** Default cache limit is set to **4.0 GB** (adjustable in Settings).
2. **Least Recently Used (LRU) Pruning:**
   - When the cache directory exceeds the quota:
   - Artifacts that are **not referenced by any currently installed package** are ranked by last access timestamp (`atime`).
   - Oldest unreferenced artifacts are purged until directory size falls below 80% of quota.
   - Artifacts referenced by currently installed packages are never automatically evicted (ensuring offline uninstallation and re-application).

---

## 4. Pre-Flight Disk Space Checks

Before downloading artifacts or creating backup snapshots, the installation planner checks available storage via `statvfs`:

$$\text{Required Space} = (\text{Download Archive Size}) + (\text{Extracted Payload Size}) + (\text{Pre-Install Backup Size}) \times 1.25$$

If $\text{Available Disk Space} < \text{Required Space}$:
- The engine halts before initiating Stage 3.
- Displays an alert: *"Insufficient Disk Space: Required 420 MB, Available 150 MB. Installation canceled."*

---

## 5. Upstream Disappearance & Offline Resilience

A major failure mode in the Linux dotfile world is a creator deleting their GitHub repository or changing a URL, breaking existing installations.

### Woderrise Mitigations:
1. **Installed Packages are Self-Contained:** Once installed, all required configuration files and assets exist directly in their target locations (`~/.config/`).
2. **Locally Cached Artifacts:** The original downloaded archive remains in `~/.cache/woderrise/artifacts/` tagged with its package reference.
3. **Registry Mirroring:** The official Woderrise registry CDN caches immutable releases by SHA-256 hash. Even if a creator deletes their personal GitHub account, the artifact remains retrievable from registry mirrors.
