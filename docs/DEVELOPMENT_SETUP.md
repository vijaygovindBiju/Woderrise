# Local Development Setup Guide

> **Document ID:** SPEC-ENG-DEV-034  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Prerequisites (Arch Linux)

To develop, build, and test Woderrise locally on an Arch Linux workstation, install the required development libraries and toolchains:

```bash
# 1. Base Development Libraries & Compilers
sudo pacman -S --needed \
    base-devel \
    rust \
    cargo \
    git \
    pkgconf \
    cmake \
    clang

# 2. GTK4, Libadwaita & Wayland Development Headers
sudo pacman -S --needed \
    gtk4 \
    libadwaita \
    wayland \
    wayland-protocols

# 3. Security, Sandboxing & Storage Dependencies
sudo pacman -S --needed \
    bubblewrap \
    sqlite \
    polkit
```

---

## 2. Repository Layout & Architecture Modules

The project is structured as a **Cargo Workspace**:

```text
woderrise/
├── Cargo.toml                      # Workspace root manifest
├── crates/
│   ├── woderrise-core/             # Core engine, WAL journal, solver, backups
│   ├── woderrise-dbus/             # D-Bus IPC client & service definitions
│   ├── woderrise-cli/              # Command-line interface and manifest linter
│   ├── woderrise-gui/              # Ratatui / Crossterm terminal frontend
│   └── woderrise-privhelper/       # Minimalist Polkit ALPM helper
├── tests/                          # Integration and fault-injection suites
└── docs/                           # Architectural specifications
```

---

## 3. Safe Local Development Environment

> [!IMPORTANT]
> Always run development builds with an **isolated temporary `$HOME`** to avoid altering your real desktop configuration files:

```bash
# Helper script to launch Woderrise in a sandboxed test environment
mkdir -p /tmp/woderrise-sandbox/config /tmp/woderrise-sandbox/local
env HOME=/tmp/woderrise-sandbox \
    RUST_LOG=debug \
    cargo run --bin woderrise-gui
```

The checked-in `registry/packages` directory is used by default. To refresh
from a GitHub-hosted Woderrise registry asynchronously, configure its
repository explicitly:

```bash
env HOME=/tmp/woderrise-sandbox \
    WODERRISE_REGISTRY_URL=https://github.com/<owner>/<registry-repo> \
    cargo run --bin woderrise-gui
```

Only HTTPS GitHub repositories are accepted for this refresh path. Failed
fetches or invalid remote manifests leave the last validated local catalog
available.

---

## 4. Building and Testing

### 4.1 Compiling All Binaries
```bash
# Debug build
cargo build --workspace

# Release build (optimized)
cargo build --workspace --release
```

### 4.2 Running the Test Suite
```bash
# Run all unit tests
cargo test --workspace --lib

# Run integration tests in disposable tempdirs
cargo test --test '*' -- --nocapture

# Run manifest linter test suite
cargo test -p woderrise-cli --test lint_test
```

### 4.3 Running in a Headless Wayland Session
If developing on a headless machine or CI runner without an active monitor:
```bash
# Launch a headless wlroots session
WLR_BACKENDS=headless WLR_RENDERER=pixman hyprland &
HYPR_PID=$!

export WAYLAND_DISPLAY=wayland-1
cargo test --workspace

kill $HYPR_PID
```
