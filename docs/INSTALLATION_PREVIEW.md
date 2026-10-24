# Pre-Installation Inspection & Preview Specification

> **Document ID:** SPEC-UI-PREV-032  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Architectural Mandate: The Zero-Surprise Contract

In traditional Linux dotfile installations, users have no idea what an installer will do until after it has executed. Files are moved, configs are clobbered, and packages are installed invisibly.

Woderrise establishes the **Zero-Surprise Contract**:
Every user has the absolute right to inspect every proposed change to their system prior to confirming installation. The **Installation Preview Modal** is the gatekeeper that renders these proposed changes transparently.

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│ 📦 Installation Preview: Catppuccin Waybar Glow (v1.2.0)                    │
├─────────────────────────────────────────────────────────────────────────────┤
│ ⚠️  AFFECTED FILES & BACKUPS                                                │
│  [MODIFIED] ~/.config/waybar/config.jsonc (Will backup original)   [View Diff]│
│  [NEW FILE] ~/.config/waybar/catppuccin.css                         [View Code]│
│                                                                             │
│ 📦 DEPENDENCIES TO INSTALL                                                  │
│  • System Binary: waybar (>= 0.9.20) — Already Installed (Satisfied)        │
│  • System Binary: rofi-wayland — MISSING (Requires Polkit Authorization)    │
│                                                                             │
│ 🔄 SERVICES TO RELOAD                                                       │
│  • Send SIGUSR2 to active Waybar daemon                                     │
│                                                                             │
│ 🛡️ RECOVERY GUARANTEE                                                        │
│  • 100% Guaranteed Rollback: All files backed up to local journal.          │
│                                                                             │
│ [ Cancel ]                                               [ Confirm & Install ]
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Information Display Requirements

The Installation Preview is organized into five structured sections:

### 2.1 Affected Files & Merges
- **Target File Path:** Full path (e.g. `~/.config/waybar/config.jsonc`).
- **Operation Classification:**
  - `[CREATE]`: Path is currently empty. File will be created cleanly.
  - `[OVERWRITE]`: File exists. Original will be copied to content-addressed backup before overwrite.
  - `[INJECT]`: Configuration block will be appended/injected into existing file with unique delimiter comments.
- **Interactive Actions:** Each file row provides a `[View Diff]` button that opens the side-by-side unified diff inspector.

### 2.2 System & Store Dependencies
- **Satisfied Dependencies:** Native packages already present on the machine (marked with a green checkmark `[✓]`).
- **Unsatisfied Store Dependencies:** Sibling store packages that will be installed concurrently as sub-transactions.
- **Unsatisfied Native System Packages:** Packages that require host package manager installation via Polkit (clearly labeled with required download size).

### 2.3 Required Privileges
- If native packages need to be installed, the preview explicitly notes:
  *"Requires administrator consent (Polkit action: org.woderrise.install-system-packages)."*
- If no system packages are required, displays:
  *"Zero elevated privileges required (100% unprivileged user execution)."*

### 2.4 Service & Daemon Side Effects
- Declares any processes that will be signaled or restarted:
  - *"Will send signal SIGUSR2 to active Waybar process."*
  - *"Will reload Hyprland keybindings via hyprctl reload."*

### 2.5 Reversibility & Rollback Disclosure
- Distinguishes **guaranteed reversible operations** (all declarative file creations, overwrites, and config injections) from **best-effort operations** (system package uninstalls or external daemon state).

---

## 3. Previewed Changes vs. Runtime Discovered Changes

> [!IMPORTANT]
> **Architectural Distinction:**
> The preview displays the **Planned Execution Model** calculated during Stage 6. However, runtime conditions may uncover changes during Stage 10 (e.g. a directory was created by another process in the intervening seconds). 
> 
> If runtime conditions diverge from the previewed plan (e.g. an unexpected file exists that was not present during planning), the engine **halts immediately**, does not proceed blindly, and returns the user to the preview dialogue with updated conflict metadata.
