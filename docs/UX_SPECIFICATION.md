# User Experience (UX) Specification

> **Document ID:** SPEC-UI-UX-030  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. UX Design Philosophy: Accessible Depth

The core user experience of Woderrise is anchored on the principle of **Progressive Disclosure**:
- **For Beginners:** An inviting, visually rich, zero-friction application store. Clear screenshots, aesthetic preview galleries, and a single prominent **"Install"** button.
- **For Power Users:** Instant access to full transparency. One click expands syntax-highlighted code diffs, file trees, cryptographic signatures, Polkit permissions, and system dependencies.

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│ ☰  Woderrise            🔍 Search packages, rices, themes...         _ □ ✕ │
├───────────────┬─────────────────────────────────────────────────────────────┤
│ 🏠 Home       │  FEATURED RICE OF THE WEEK                                  │
│ 📁 Categories │  ┌────────────────────────────────────────────────────────┐ │
│ 📦 Installed  │  │  End-4 Illogical Impulse (Material You Dynamic Rice)   │ │
│ ⏱️ History    │  │  [ Install Setup ]  [ Inspect Files ]                  │ │
│ ⚙️ Settings   │  └────────────────────────────────────────────────────────┘ │
│               │                                                             │
│ POPULAR       │  TOP WAYBAR THEMES              RECENT WALLPAPERS           │
│ • Bars        │  ┌──────┐ ┌──────┐ ┌──────┐     ┌──────┐ ┌──────┐ ┌──────┐  │
│ • Wallpapers  │  │ Catp │ │ Nord │ │ Neon │     │ City │ │ Mist │ │ Mtns │  │
│ • Plugins     │  └──────┘ └──────┘ └──────┘     └──────┘ └──────┘ └──────┘  │
└───────────────┴─────────────────────────────────────────────────────────────┘
```

---

## 2. Screen Specifications

### 2.1 Screen 1: Home & Featured View
- **Header:** Global search input with instant autocomplete suggestions; theme switcher (Dark/Light).
- **Hero Carousel:** Curated full setups and community-voted rices of the month.
- **Content Rows:** Horizontal scroll cards for "Trending Waybar Themes", "New Wallpapers", and "Essential Hyprland Plugins".

### 2.2 Screen 2: Category Browsing
- **Left Sidebar:** The 28 standardized categories with active package count badges.
- **Main Grid:** Responsive card layout showing package thumbnails, title, author avatar, version, and compatibility pills (🟢 Compatible).
- **Secondary Filters:** Subcategory chips (e.g. Waybar vs AGS), license filters, and sort options (Downloads, Recent, Alphabetical).

### 2.3 Screen 3: Package Detail & Preview Gallery
- **Hero Section:** Package title, author, version, license badge, and primary action button (**"Install"**, **"Installed"**, or **"Update Available"**).
- **Media Carousel:** HiDPI screenshots with full-screen zoom lightbox and extracted color palette swatches with one-click hex copying.
- **Tabbed Interface:**
  - **Overview Tab:** Markdown-rendered documentation, features, and keybinding lists.
  - **Dependencies Tab:** Tree view of required system packages and store packages.
  - **File Inspector Tab:** Interactive directory tree and side-by-side syntax-highlighted unified diffs.

### 2.4 Screen 4: Configuration File Inspector & Diff Viewer
- **Left Pane:** File tree showing all destination paths (e.g. `~/.config/waybar/config.jsonc`).
  - Badges indicate file status: `[NEW]`, `[MODIFIED]`, `[INJECTED]`.
- **Right Pane:** Side-by-side unified diff viewer comparing the user's active configuration against the package's declared version. Green highlights additions; red highlights removals.

### 2.5 Screen 5: Installation Confirmation & Safety Modal
Before any write operation occurs, an intuitive confirmation dialogue summarizes:
- **Disk Impact:** "Creates 3 files, modifies 1 file, stores 1 backup."
- **System Dependencies:** "Requires native package `rofi-wayland` (Polkit prompt will follow)."
- **Reversibility Guarantee:** "Guaranteed 100% reversible via History."
- **Buttons:** `[Cancel]` and `[Confirm & Install]`.

### 2.6 Screen 6: Live Progress & Transaction Feedback
- Renders real-time progress steps:
  1. `[✓] Verifying Checksums`
  2. `[✓] Creating Pre-Install Backup`
  3. `[●] Applying Atomic File Swaps (75%)`
  4. `[ ] Reloading Hyprland Compositor`
- Non-blocking: user can minimize window or cancel if in pre-execution stage.

### 2.7 Screen 7: Installed Packages Manager
- Lists all active store packages on the user's system.
- Per-package controls:
  - **[Inspect Active Configs]**
  - **[Check for Updates]**
  - **[Uninstall]** (Triggers modal offering clean delete or backup restoration).
  - **[Pin Version]** (Prevents automatic update prompts).

### 2.8 Screen 8: Chronological History & Rollback Hub
- Displays an interactive timeline of every transaction recorded in the SQLite WAL journal.
- Filter by: Installs, Updates, Rollbacks, Recoveries.
- Each entry displays:
  - Timestamp, Package ID, Version, Status badge (`COMMITTED`).
  - Expandable operation list showing pre/post SHA-256 hashes.
  - A prominent **"Rollback"** button that invokes the inverse transaction engine.

### 2.9 Screen 9: Conflict & Concurrency Resolution
- Displayed when a user has edited a file after install and later requests rollback or update.
- Shows a 3-way visual diff:
  - Left: *Pre-Install Backup*
  - Center: *Your Current Modifications*
  - Right: *Package Version*
- Clear choices: `[Keep My Edits]`, `[Overwrite with Backup]`, `[Cancel]`.

### 2.10 Screen 10: Cache, Backup & Storage Settings
- Visual storage meter: Download Cache (e.g. 1.2 GB), Backup Storage (e.g. 450 MB).
- Actions:
  - **[Clear Download Cache]** (Safe, unreferenced files only).
  - **[Export Diagnostic Report]** (Sanitized bug report bundle).
  - **[Custom Registry URL]** (Add private or community Git repositories).
