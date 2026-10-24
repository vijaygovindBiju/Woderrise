# Compatibility and Platform Support

> **Document ID:** SPEC-ARCH-CMP-023  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Overview & Compatibility States

Due to the rapid development of the Hyprland Wayland compositor and its underlying protocols, software components experience varying degrees of stability. A simple wallpaper works across all versions, whereas a C++ plugin compiled for Hyprland v0.41.0 will cause an immediate compositor crash if loaded into v0.41.2 without exact header alignment.

Woderrise categorizes every package relative to the host system into one of three **Explicit Compatibility States**:

| State | Badge | Definition | Installation Allowed? |
| :--- | :--- | :--- | :--- |
| **COMPATIBLE** | 🟢 **Verified Compatible** | Package has been verified on the running Hyprland version and all required protocols/ABIs are confirmed present. | **Yes.** Full one-click install. |
| **UNTESTED** | 🟡 **Untested Version** | Package has no known hard conflicts, but has not been verified on this exact Hyprland release. | **Yes, with advisory prompt.** User notified that minor visual or syntax adjustments may be needed. |
| **INCOMPATIBLE** | 🔴 **Incompatible** | Violates minimum/maximum Hyprland version, lacks required Wayland protocols, or exhibits a C++ plugin ABI mismatch. | **BLOCKED.** Install button disabled; exact reason and required upgrades displayed. |

---

## 2. Hyprland Compositor & Plugin ABI Verification

```mermaid
flowchart TD
    Req[Install Package Request] --> Query[Execute `hyprctl version` over IPC Socket]
    Query --> ParseVer[Extract Version e.g. v0.41.2 & Commit Hash e.g. 3b9a7102]
    ParseVer --> TypeCheck{Is Package Type == 'plugin'?}
    
    TypeCheck -->|No (Asset, Theme, Config)| VerBoundCheck[Check SemVer Bounds in Manifest]
    VerBoundCheck --> InBound{Within Hyprland Version Bounds?}
    InBound -->|Yes| Comp[Mark State = COMPATIBLE]
    InBound -->|No| Incomp[Mark State = INCOMPATIBLE: Version Mismatch]

    TypeCheck -->|Yes (C++ Plugin)| AbiCheck{Does pre-built binary commit match running Hyprland commit?}
    AbiCheck -->|Exact Match| Comp
    AbiCheck -->|Mismatch| HeaderCheck{Are hyprland-headers & C++ compiler present?}
    HeaderCheck -->|Yes| CompilePlan[Plan Local Compilation via hyprpm Hook]
    HeaderCheck -->|No| BlockAbi[Block Install: ABI Mismatch & Headers Missing]
```

### 2.1 Hyprland Plugin C++ ABI Invariant
- Hyprland plugins compile directly against internal C++ headers and link dynamically into the compositor process (`dlopen`).
- A commit hash mismatch between the plugin `.so` and the running Hyprland compositor is an **absolute blocker**.
- If a package provides a pre-compiled binary, the engine verifies:
  `BINARY_COMMIT_HASH == RUNNING_HYPRLAND_COMMIT_HASH`
- If they do not match, the engine prohibits dynamic loading (`hyprctl plugin load`) to prevent crashing the user's active desktop session.

---

## 3. Wayland Protocol Requirements

Many advanced widgets and lockscreens require specific Wayland protocols. Manifests declare protocol dependencies, which the engine validates against the compositor's advertised global interface registry:

| Component Type | Required Wayland Protocol | Consequence If Missing |
| :--- | :--- | :--- |
| **Bars, Widgets, Launchers** | `zwlr_layer_shell_v1` | Windows cannot anchor to screen edges or float over full-screen apps. |
| **Lock Screens** | `ext_session_lock_v1` | Locker cannot safely lock Wayland session; blocked for security. |
| **Screen Capture / Announce**| `xdg_output_unstable_v1` | Screenshot tools cannot calculate correct display coordinates. |
| **Night Light / Gamma** | `zwlr_gamma_control_v1` | Blue light reduction utilities fail to adjust display color temperature. |

---

## 4. Host Distribution Support Matrix

| Distribution | Tier | Native Package Manager | Support Status |
| :--- | :---: | :--- | :--- |
| **Arch Linux** | **Tier 1 (Primary)** | ALPM / `pacman` | **Full Support (MVP)**. Automated dependency checks and Polkit helper implementation. |
| **Fedora Linux** | **Tier 2** | `dnf` / `rpm-ostree` | Planned Milestone 5. Abstract `SystemPackageManager` trait. |
| **openSUSE Tumbleweed** | **Tier 2** | `zypper` | Planned Milestone 5. |
| **NixOS** | **Tier 3** | Nix Home Manager | Declarative export mode (generates Nix expressions). |

---

## 5. System Service Prerequisites

Before applying configurations that depend on system services, Woderrise verifies their active availability via `systemctl --user`:
- **Audio:** `pipewire.service` and `wireplumber.service`
- **Portals:** `xdg-desktop-portal-hyprland.service`
- **Polkit Agent:** Active authentication agent listening on D-Bus.
