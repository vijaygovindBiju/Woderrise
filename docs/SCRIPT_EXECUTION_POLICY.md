# Script Execution Policy and Sandboxing

> **Document ID:** SPEC-SEC-SCP-028  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Declarative Operations vs. Shell Scripts

In traditional Linux dotfile setups, shell scripts (`install.sh`, `apply.sh`, `post_install.sh`) are the primary vehicle for customization. However, shell scripts present severe architectural and security hazards:
- They are opaque black boxes to automated dependency solvers.
- They can silently execute network exfiltration commands (`curl`, `nc`).
- They can leave external processes running or create unjournaled files.
- **They cannot be deterministically reversed by a file transaction rollback engine.**

> [!CAUTION]
> **Explicit Non-Guarantee:**
> Woderrise will NEVER claim that rollback can reverse arbitrary shell script behavior. If a script sends an HTTP request, formats a partition, or modifies an external database, that operation is fundamentally irreversible by a file journal.

Therefore, Woderrise enforces a strict **Declarative-First Policy**:
1. All standard operations (file placement, font cache updates, Hyprland reloads, Waybar signals) **must** be expressed declaratively using native manifest stanzas (`files`, `config_injections`, `service_triggers`).
2. Manifests attempting to use shell scripts where declarative primitives exist are rejected during registry linting.

---

## 2. Legacy Script Adapters & Bubblewrap Isolation

For legacy or complex packages that genuinely require helper scripts (e.g. dynamic color palette generation via Python or SASS compilation):

### 2.1 Bubblewrap (`bwrap`) Sandbox Specification
All executable scripts are executed strictly inside a temporary, confined **Bubblewrap** container:

```bash
bwrap \
  --ro-bind /usr /usr \
  --ro-bind /lib /lib \
  --ro-bind /lib64 /lib64 \
  --ro-bind /bin /bin \
  --ro-bind /etc /etc \
  --dir /tmp \
  --dir /var \
  --proc /proc \
  --dev /dev \
  --unshare-all \
  --unshare-net \
  --bind ~/.local/state/woderrise/sandbox_home /home/user \
  --chdir /home/user \
  --setenv HOME /home/user \
  --setenv PATH /usr/bin:/bin \
  -- /bin/bash -c "script.sh"
```

### 2.2 Sandbox Security Constraints
1. **Network Disconnected (`--unshare-net`):** The sandbox has zero network access. No DNS, no outbound HTTP, no socket connections.
2. **Read-Only Root Filesystem:** `/usr`, `/etc`, and system binaries are mounted strictly read-only (`--ro-bind`).
3. **Isolated Home Directory:** Scripts cannot see the user's real `$HOME`. An isolated directory is mounted at `/home/user`. Only explicitly declared target directories (`~/.config/app`) are exposed.
4. **PID Isolation (`--unshare-pid`):** Scripts cannot view or send signals to processes running outside the sandbox.
5. **Strict Execution Timeout:** Scripts are terminated with `SIGKILL` if execution exceeds **30.0 seconds**.

---

## 3. User Consent & Mandatory Code Inspection

If a package requires a sandboxed script execution hook:
1. The package is flagged with a high-visibility **"Executes Script"** warning badge.
2. The user **cannot click One-Click Install** without viewing the script.
3. The UI presents the complete, syntax-highlighted script source code.
4. The user must check a box: *"I have reviewed this script and understand that external side effects cannot be rolled back."*
5. The execution outcome, exit code, and stdout/stderr output are captured in the SQLite transaction journal for auditability.
