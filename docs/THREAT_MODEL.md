# Threat Model & Vulnerability Analysis

> **Document ID:** SPEC-SEC-THR-026  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Scope, Protected Assets, and Threat Actors

This document establishes the formal **STRIDE Threat Model** for the Woderrise desktop customization platform.

### Protected Assets
1. **User Personal Data:** Private files, SSH keys (`~/.ssh`), GPG keys (`~/.gnupg`), browser cookies, and session tokens located within `$HOME`.
2. **System Integrity:** Core OS files (`/usr`, `/etc`), bootloader, kernel modules, and systemd system services.
3. **Session Stability:** The active Hyprland Wayland compositor process, D-Bus session bus, and running desktop services.
4. **Historical Backups & Journal:** The local SQLite WAL journal and content-addressed backup repository.

### Threat Actors
- **Rogue Community Creators:** Authors publishing superficially attractive rices containing malicious scripts or telemetry exfiltration.
- **Compromised Maintainers:** Legitimate package accounts hijacked via credential stuffing or compromised GitHub tokens.
- **Man-in-the-Middle (MitM) Attackers:** Network adversaries intercepting package artifact downloads on public Wi-Fi.
- **Local Malicious Software:** Unprivileged background applications attempting to abuse Woderrise's Polkit helper or manipulate configuration files.

---

## 2. STRIDE Threat Analysis Matrix

| Threat Category | Specific Attack Vector | Potential Impact | Architectural Mitigation | Residual Risk |
| :--- | :--- | :--- | :--- | :--- |
| **Spoofing** | Compromised registry repository or hijacked GitHub release. | User downloads trojanized dotfile payload. | Ed25519 cryptographic creator signatures verified against public key registry. | Zero-day compromise of creator private signing key. |
| **Tampering** | In-transit artifact manipulation or corrupted cache storage. | Malicious bytes injected into executable script. | Immutable content addressing: strict SHA-256 validation before every file write. | Extremely low (SHA-256 collision resistance). |
| **Repudiation** | Malicious author claims they did not author a destructive script. | Inability to attribute malicious packages. | Git commit cryptographic signatures + append-only public registry index. | None. |
| **Information Disclosure** | Script attempts to read and exfiltrate `~/.ssh/id_rsa` or browser passwords. | Compromise of personal developer credentials. | Confined Bubblewrap sandboxing (`--unshare-net`, `--ro-bind` with unshared `$HOME`). | User explicitly granting script unrestricted network permissions. |
| **Denial of Service** | C++ plugin compiled with invalid ABI loaded into Hyprland compositor. | Total crash of active graphical session, killing all open apps. | Strict commit hash verification (`hyprctl version`) before dynamic loading. | Bugs inside Hyprland core compositor itself. |
| **Elevation of Privilege**| Package attempts to escalate to root via Polkit or `sudo` abuse. | Full system compromise; overwrite of `/etc/shadow` or root filesystem. | Polkit privilege helper accepts only typed ALPM package names; zero arbitrary command execution. | Vulnerabilities in system `pacman` or Polkit daemon itself. |

---

## 3. Detailed Attack Vectors & Defensive Mitigations

### 3.1 Path Traversal & Symlink Attacks (`CVE-STYLE-01`)
- **Attack Vector:** A malicious manifest specifies target path: `~/.config/../../etc/cron.d/malicious_cron` or creates a symlink pointing to `/etc/passwd`.
- **Mitigation:**
  - Strict path normalization: all paths are canonicalized via `std::fs::canonicalize`.
  - Prefix validation: target paths MUST resolve inside `$HOME/.config`, `$HOME/.local`, or `$HOME/.themes`.
  - Symlink policy: existing symlinks are replaced rather than dereferenced and followed during atomic write operations.

### 3.2 Dependency Confusion
- **Attack Vector:** A package claims a native system dependency with a generic name designed to shadow legitimate packages or pull from an unvetted AUR repository.
- **Mitigation:**
  - Woderrise strictly validates native dependencies against the official Arch Linux package repositories (`core`, `extra`, `multilib`).
  - AUR packages are flagged with high-visibility warnings and require explicit per-package confirmation.

### 3.3 Rollback & Journal Poisoning
- **Attack Vector:** A malicious local process attempts to edit `journal.db` to trick Woderrise into deleting system files during a rollback.
- **Mitigation:**
  - SQLite foreign key constraints, WAL checksums, and strict file permission enforcement (`0600` on database files).
  - Optimistic concurrency checks: rollback verifies backup file hashes and target file identity before unlinking.
