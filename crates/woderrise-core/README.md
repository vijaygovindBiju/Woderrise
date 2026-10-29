# `woderrise-core`

> Pure domain models, FreeDesktop XDG path resolution, schema serialization, and traversal-resistant security invariants for Woderrise.

[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](../../Cargo.toml)

---

## Overview

`woderrise-core` serves as the foundational domain crate in the Woderrise workspace. It is completely headless, decoupled from Iced or any presentation framework, and maintains zero circular dependencies.

### Key Capabilities

- **Domain Models (`models`)**:
  - `CatalogRepository`, `Catalog`: Curated community configurations and metadata.
  - `ConfigComponent`: Supported Wayland component classifications (`Hyprland`, `Waybar`, `Hyprlock`, `Hypridle`, `Hyprpaper`, `Hyprcursor`, `GtkTheme`).
  - `DetectedComponent`, `TargetMapping`: Granular file-to-destination mappings and conflict status.
  - `Journal`, `TransactionRecord`, `FileOperation`: Persistent Write-Ahead Log (WAL) and durability structures.
  - `Settings`: User configuration, themes, auto-backup preferences, and GitHub authentication credentials.
- **Paths & FreeDesktop XDG Conformance (`paths`)**:
  - Full adherence to the FreeDesktop XDG Base Directory specification.
  - Computes standard locations for `$XDG_DATA_HOME/woderrise/`, `$XDG_CONFIG_HOME/`, `$XDG_STATE_HOME/woderrise/`, and `$XDG_CACHE_HOME/woderrise/`.
  - Traversal-resistant path sanitization (`sanitize_relative_path`), blocking directory traversal attacks (`..`), null bytes (`\0`), and illegal absolute path escapes.
- **Unified Error Taxonomy (`error`)**:
  - `WoderriseError` implemented with `thiserror`, providing exhaustive error categories (`DomainError`, `IoError`, `PathError`, `SecurityError`, `SerializationError`, `TransactionError`).
- **JSON Schemas (`schema`)**:
  - Versioned JSON Schema validation constants for `Catalog`, `StagingManifest`, `Journal`, and `Settings`.

---

## Usage Example

```rust
use camino::Utf8Path;
use woderrise_core::models::ConfigComponent;
use woderrise_core::paths::{sanitize_relative_path, XdgPaths};

// Resolve standard XDG paths
let paths = XdgPaths::from_env().expect("valid environment");
println!("Catalog cache: {}", paths.catalog_file());
println!("Transaction journal: {}", paths.journal_file());

// Traversal-safe path validation
let safe_path = sanitize_relative_path(Utf8Path::new("hypr/hyprland.conf"))?;
assert!(sanitize_relative_path(Utf8Path::new("../etc/passwd")).is_err());

// Inspect components
let waybar = ConfigComponent::Waybar;
assert_eq!(waybar.default_entrypoint(), "config.jsonc");
assert_eq!(waybar.default_config_subpath(), "waybar");
```

---

## Testing

Run unit tests for `woderrise-core`:

```bash
cargo test --package woderrise-core
```
