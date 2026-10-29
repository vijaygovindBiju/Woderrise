# `woderrise-detect`

> Independent component classification, syntax validation, hardware display preservation, and procedural execution auditing for modern Wayland desktop configurations.

[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](../../Cargo.toml)

---

## Overview

Community dotfile repositories are complex and heterogeneous. A single repository might contain Hyprland window manager rules, a Waybar bar theme, a custom lock screen, and display-specific monitor resolutions.

`woderrise-detect` scans repository trees in memory to automatically classify individual components, validate their syntax, protect local display configurations, and audit untrusted procedural commands before any file is copied to the host.

### Supported Component Classifiers

| Component | Target Path | Parser & Detection Mechanics |
| :--- | :--- | :--- |
| **Hyprland** | `~/.config/hypr/` | Parses legacy block/directive syntax (`hyprland.conf`) and modern Lua syntax (`hyprland.lua`). Identifies source directives, legacy config conflicts, and prepares deactivation markers. |
| **Waybar** | `~/.config/waybar/` | Parses JSONC configurations with the `json5` lexer, safely handling line (`//`) and block (`/* */`) comments and trailing commas without regex corruption of embedded URLs. Validates GTK CSS (`style.css`). |
| **Hardware Protection** | `~/.config/hypr/monitors.conf` | Detects monitor and display-bound files (`monitors.conf`, `workspaces.conf`, `hardware.conf`). **Automatically preserves local hardware configs by default** with `[Preserve Local Display Config]` badges. |
| **Hyprlock** | `~/.config/hypr/hyprlock.conf` | Validates Hyprlock directives (`background`, `input-field`, `label`). |
| **Hypridle** | `~/.config/hypr/hypridle.conf` | Validates listener directives, timeout triggers, and system sleep/resume hooks. |
| **Wallpapers** | `~/.config/hypr/`, `~/.local/share/wallpapers/` | Detects raster and vector image assets (`.png`, `.jpg`, `.jpeg`, `.webp`), remapping `hyprpaper.conf` wallpaper preload directives to canonical host destinations. |
| **Hyprcursor & GTK** | `~/.icons/`, `~/.config/gtk-3.0/`, `~/.config/gtk-4.0/` | Validates FreeDesktop `index.theme`, `manifest.hl`, and GTK `settings.ini`. |

### Security & Procedural Auditing

- **Hardware Display Isolation**: Monolithic community setups often hardcode the original author's dual-monitor EDID IDs, display port names, and custom refresh rates. Overwriting local monitor settings results in black screens or broken display outputs upon login. `woderrise-detect` detects and isolates these files, ensuring they remain unselected during installation.
- **Procedural Command Auditing (`ProceduralDirective`)**: Untrusted configurations frequently bundle background autostarts (`exec-once = ...`, `exec = ...`, `on-click = ...`). `woderrise-detect` catalogs all procedural directives, neutralizes them by default in staging, and flags them for explicit user review.

---

## Usage Example

```rust
use woderrise_detect::{DetectionEngine, TreeEntry};

let engine = DetectionEngine::new();

// Simulate remote tree entries
let entries = vec![
    TreeEntry { path: "hypr/hyprland.conf".into(), mode: "100644".into(), size: Some(2048) },
    TreeEntry { path: "hypr/monitors.conf".into(), mode: "100644".into(), size: Some(512) },
    TreeEntry { path: "waybar/config.jsonc".into(), mode: "100644".into(), size: Some(4096) },
];

let report = engine.scan_components(&entries);
for component in report.components {
    println!("Detected component: {:?} ({} files)", component.component, component.files.len());
    if component.is_hardware_bound {
        println!("  ⚠️ Marked as hardware-bound: preserved by default!");
    }
}
```

---

## Testing

Run unit and validation tests for `woderrise-detect`:

```bash
cargo test --package woderrise-detect
```
