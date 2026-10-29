# `woderrise-transaction`

> 4-phase atomic file replacement engine, Write-Ahead Logging (WAL), pre-installation backup snapshots, LIFO rollback, and startup crash recovery for Woderrise.

[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](../../Cargo.toml)

---

## Overview

Applying untrusted community configurations must never risk corrupting a user's working desktop. An interrupted install, sudden power outage, or conflicting file must not leave `~/.config/` in an unrecoverable state.

`woderrise-transaction` guarantees atomic, transactionally safe installations backed by a persistent Write-Ahead Log (WAL), automatic pre-flight backups, POSIX directory durability commits (`fsync`), and 1-click reversible rollbacks.

### The 4-Phase Atomic Swap Pipeline

```
[ Phase 1: Staging ]      [ Phase 2: Backup ]        [ Phase 3: WAL & Swap ]      [ Phase 4: Commit ]
 Sandbox download in       Full local snapshot in     Write "Applying" to WAL;     Mark "Committed" in
 $XDG_CACHE_HOME/          $XDG_STATE_HOME/           stream copy to temp file;    journal.json;
 Security audit;           Pre-installation SHA-256   fsync -> atomic rename;      clean staging;
 hash calculation.         hashed & recorded.         parent directory fsync.      gated reload.
```

1. **Phase 1: Sandboxed Staging (`staging`)**:
   - Downloads and stages files inside `$XDG_CACHE_HOME/woderrise/staging/<tx_id>/`.
   - Computes cryptographic SHA-256 checksums and writes `staging/<tx_id>/manifest.json`.
   - Audits all files: blocks directory traversal (`..`), denies arbitrary script execution (`.sh`, `.py`, binaries), and rejects Git tree symlink objects (`mode 120000`, defending against CVE-2021-21300).
   - Preserves GNU Stow and Chezmoi setups by inspecting destination paths with `symlink_metadata()` to resolve canonical link targets.
2. **Phase 2: Pre-Installation Backup Snapshot (`backup`)**:
   - Before modifying any file on the host, copies existing files into `$XDG_STATE_HOME/woderrise/backups/<tx_id>/`.
   - Records pre-installation SHA-256 hashes for post-install verification and dirty-state tracking.
3. **Phase 3: Write-Ahead Logging & Atomic Replacement (`atomic`, `wal`)**:
   - Commits active transaction metadata with status `"Applying"` to `journal.json` using atomic sibling writes (`journal.json.tmp` -> `rename`) protected by advisory file locks (`fs2`).
   - Copies staged content into a target sibling temporary file: `<target_dir>/<filename>.woderrise_tmp` via `std::io::copy()`, eliminating cross-device `EXDEV` errors.
   - Flushes temporary file data to disk via `fsync()`.
   - Atomically replaces the live configuration file via `std::fs::rename()`.
   - Durability commit: calls `File::open(parent_dir)?.sync_all()?` ensuring directory entries survive sudden power loss.
4. **Phase 4: Transaction Commit & Cleanup (`engine`)**:
   - Atomically updates `journal.json`: sets `active_transaction` to `null` and appends a `Committed` record to history.
   - Cleans up ephemeral staging files.
   - Dispatches compositor reloads only when security-gated user confirmation is provided.

### Rollback & Crash Recovery (`rollback`, `recovery`)

- **1-Click LIFO Rollback**: Restores previous configurations in reverse chronological order.
- **Dirty-State Verification**: Before rolling back, computes `current_sha256 == post_sha256`. If the user made manual edits post-install, presents a safety warning with a visual diff.
- **Startup Crash Recovery**: When Woderrise starts up, it inspects `journal.json`. If an `active_transaction` with status `"Applying"` is found (indicating a crash or power cut during install), the recovery engine automatically rolls back partial writes from the backup snapshot before user interaction begins.

---

## Usage Example

```rust
use woderrise_core::paths::XdgPaths;
use woderrise_transaction::TransactionEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths = XdgPaths::from_env()?;
    let engine = TransactionEngine::new(paths);

    // Check journal on startup for incomplete transactions
    let recovery = engine.check_startup_recovery().await?;
    if let Some(recovered_id) = recovery {
        println!("Cleanly recovered interrupted transaction: {}", recovered_id);
    }

    Ok(())
}
```

---

## Testing

Run unit, integration, and crash recovery tests for `woderrise-transaction`:

```bash
cargo test --package woderrise-transaction
```
