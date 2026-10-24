# Project Glossary and Domain Terminology

> **Document ID:** SPEC-REF-GLO-040  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Domain Terminology

### Artifact
A discrete, content-addressed file payload (e.g. an archive, image, stylesheet, or compiled dynamic library) indexed by its cryptographic SHA-256 hash.

### Backup Snapshot
An uncompressed, byte-for-byte copy of a user configuration file stored in `~/.local/share/woderrise/backups/<backup_id>/` prior to any modification, overwrite, or deletion.

### Bubblewrap (`bwrap`)
A lightweight, unprivileged Linux sandboxing utility that leverages Linux user namespaces to create isolated filesystem, PID, and network execution containers.

### Component Bundle
A composite package type that references multiple child packages without containing direct file payloads, orchestrating their installation as sub-transactions.

### Complete Desktop Configuration ("Rice")
A comprehensive, turnkey desktop customization suite that orchestrates the Wayland compositor, status bar, application launcher, notification daemon, wallpaper, and terminal themes simultaneously.

### Content-Addressed Storage (CAS)
A storage mechanism where files are named, indexed, and retrieved strictly by their cryptographic content digest (SHA-256) rather than arbitrary path strings.

### Declarative Manifest (`woderrise.yaml`)
A structured, versioned YAML specification describing a package's identity, dependencies, compatibility, file placements, and service reloads without using imperative shell scripts.

### Dotfiles
User-level configuration files and directories located within `$HOME/.config`, `$HOME/.local`, or root of `$HOME` (e.g. `~/.config/hypr/hyprland.conf`).

### Hyprland Plugin ABI
The internal C++ application binary interface exposed by Hyprland's compositor headers. Requires strict commit-level header alignment for dynamically loaded `.so` plugins.

### Installation Plan
A deterministic Directed Acyclic Graph (DAG) of atomic filesystem operations, pre-install backups, and service triggers formulated by the engine prior to executing any disk writes.

### Layer-Shell Protocol (`zwlr_layer_shell_v1`)
A Wayland protocol extension allowing desktop components (status bars, wallpapers, notification popups, launchers) to anchor to screen edges and render across specific desktop layers (Background, Bottom, Top, Overlay).

### Optimistic Concurrency Guard
An automated safety check performed prior to rollback or updates that compares the current on-disk checksum of a file against the checksum recorded at installation time, preventing silent overwriting of user customizations.

### Package
The fundamental unit of distribution in Woderrise, uniquely identified by a reverse-DNS identifier (e.g. `com.catppuccin.waybar`) and semantic version.

### Package Type
One of eight standardized classifications defining lifecycle behavior: `asset`, `theme`, `config`, `plugin`, `application`, `script`, `bundle`, or `desktop_setup`.

### PolicyKit (Polkit)
The standard Linux system authorization framework allowing unprivileged desktop applications to request strictly scoped privileged operations via graphical password dialogs.

### Registry
The decentralized, public Git repository and CDN mirror network that indexes package metadata, release lines, and cryptographic public signing keys.

### Reverse-DNS Identifier
A globally unique package naming format modeled after Java package conventions and D-Bus interfaces (e.g. `io.github.username.packagename`).

### Rollback
The automated, inverse execution of a completed transaction recorded in the journal, returning the filesystem and active services to their exact prior state.

### Single-Writer Mutex
An operating system process lock (`~/.local/state/woderrise/engine.lock`) ensuring that only a single mutating installation, update, or rollback transaction can execute at any time.

### Write-Ahead Logging (WAL)
A database logging technique where state changes are written to an append-only transaction log on disk before being applied to the main database, guaranteeing crash consistency across power outages.
