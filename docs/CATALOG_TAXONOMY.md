# Catalog Taxonomy & Component Classification

> **Document ID:** SPEC-ECO-CAT-007  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Overview & Classification Framework

The Hyprland desktop ecosystem is composed of modular Wayland utilities, background daemons, graphical toolkits, and configuration scripts. 

Woderrise unifies these components under **28 standardized taxonomic categories**. Every package in the registry belongs to at least one primary category and declares a concrete **Package Type**:
- `asset`: Static immutable files (images, fonts, sounds).
- `theme`: Styling assets (CSS, QSS, SVG assets, color definitions).
- `config`: Declarative dotfile configurations for existing tools.
- `plugin`: Dynamically linked C++ plugins compiled for Hyprland.
- `application`: Native binary packages or Flatpak desktop programs.
- `script`: Confined automation scripts and helper tools.
- `bundle`: A composite package referencing multiple child packages.
- `desktop_setup`: A complete desktop configuration suite ("rice").

---

## 2. The 28 Standardized Categories

### 1. Bars and Panels
- **Description:** Primary desktop status bars, panels, and taskbars displaying workspaces, window titles, system trays, and clock widgets.
- **Package Types:** `config`, `theme`, `application`.
- **Representative Projects:** Waybar, AGS (Aylur's GTK Shell), Quickshell, Ironbar, Eww (ElKowars wacky widgets).
- **Dependencies & Compatibility:** Requires Wayland layer-shell protocol support; font dependencies (Nerd Fonts); PipeWire/ALSA libraries.
- **Install/Removal Implications:** May require launching a background process via Hyprland's `exec-once`; removal must terminate daemon and restore previous bar configuration.
- **Preview Requirements:** HiDPI horizontal screenshot of full bar at 100% scale, showing both idle and active module states.

---

### 2. Bar Themes and Modules
- **Description:** Individual custom modules (weather, crypto ticker, media player) or complete stylesheet/layout themes for an existing status bar.
- **Package Types:** `theme`, `config`, `script`.
- **Representative Projects:** Custom Waybar JSONC layouts, Catppuccin Waybar CSS, AGS widget modules.
- **Dependencies & Compatibility:** Depends on a parent bar (e.g. Waybar >= 0.9.20); specific font symbols; Python/bash helper scripts.
- **Install/Removal Implications:** Injects module declarations into bar configuration; triggers `killall -SIGUSR2 waybar` on install and removal.
- **Preview Requirements:** Zoomed crop of the specific module rendered on light and dark backgrounds.

---

### 3. Wallpapers and Wallpaper Engines
- **Description:** Static, animated, or dynamic wallpapers and the background daemons that render them.
- **Package Types:** `asset`, `application`, `config`.
- **Representative Projects:** Hyprpaper, Swww, MPVpaper, Waypaper, Paperless.
- **Dependencies & Compatibility:** Multi-monitor support; GPU video acceleration (VAAPI/NVDEC) for video engines.
- **Install/Removal Implications:** Deploys assets to `~/.local/share/backgrounds/`; activates via daemon IPC (`hyprpaper` socket or `swww img`).
- **Preview Requirements:** 16:9 thumbnail preview; 8-swatch extracted hex color palette.

---

### 4. GTK and Qt Themes
- **Description:** Application styling stylesheets, Kvantum themes, color palettes, and window decoration themes.
- **Package Types:** `theme`, `asset`.
- **Representative Projects:** Adw-gtk3, Catppuccin GTK, Dracula GTK, Kvantum Qt themes, Colloid.
- **Dependencies & Compatibility:** GTK 3, GTK 4, Libadwaita, Qt 5, Qt 6; `gsettings` desktop schema.
- **Install/Removal Implications:** Placed in `~/.themes/` or `~/.config/gtk-4.0/`; updates XDG GSettings schemas (`gtk-theme`, `color-scheme`).
- **Preview Requirements:** Side-by-side screenshots showing a GTK4 app (Nautilus) and a Qt app (Kvantum Manager).

---

### 5. Icons, Cursors, and Fonts
- **Description:** System icon sets, Wayland cursor themes, and system/terminal typography.
- **Package Types:** `asset`.
- **Representative Projects:** Papirus, Tela-icon-theme, Bibata Cursor, Catppuccin Cursors, JetBrains Mono Nerd Font.
- **Dependencies & Compatibility:** `fontconfig`, XCursor inheritance rules, `gsettings`.
- **Install/Removal Implications:** Installed into `~/.local/share/icons/` and `~/.local/share/fonts/`; triggers `fc-cache -f`.
- **Preview Requirements:** Icon grid preview showing standard MIME types and folder variants; cursor animation sheet.

---

### 6. Terminal Themes and Terminal Applications
- **Description:** Terminal emulators and their associated color schemes, fonts, and layout presets.
- **Package Types:** `config`, `theme`, `application`.
- **Representative Projects:** Kitty, Foot, Alacritty, WezTerm, Ghostty.
- **Dependencies & Compatibility:** Requires Wayland terminal support; specific terminal binary.
- **Install/Removal Implications:** Manages files in `~/.config/kitty/`, `~/.config/foot/`, etc.
- **Preview Requirements:** Terminal window screenshot showing colorful neofetch/fastfetch output and code syntax.

---

### 7. Application Launchers and Menus
- **Description:** Graphical application runners, dmenu replacements, and search menus.
- **Package Types:** `config`, `theme`, `application`.
- **Representative Projects:** Rofi-Wayland, Wofi, Anyrun, Fuzzel, Walker, Toofi.
- **Dependencies & Compatibility:** Layer-shell protocol; desktop file parsers.
- **Install/Removal Implications:** Injects custom keybinds (`bind = SUPER, SPACE, exec, ...`) into Hyprland config; manages `~/.config/rofi/`.
- **Preview Requirements:** Screenshot showing launcher active over a desktop with sample search results.

---

### 8. Docks and Desktop Widgets
- **Description:** On-screen desktop widgets, clocks, system monitors, and application docks.
- **Package Types:** `config`, `theme`, `application`.
- **Representative Projects:** Eww, AGS desktop widgets, Cairo-dock, Plank-wayland.
- **Dependencies & Compatibility:** Layer-shell protocol; JavaScript (GJS) or Rust runtimes.
- **Install/Removal Implications:** Launches background widget daemons via `exec-once`.
- **Preview Requirements:** Full-screen desktop screenshot with widget highlighted.

---

### 9. Notification Daemons and Centers
- **Description:** Desktop notification servers, history viewers, and Do-Not-Disturb managers.
- **Package Types:** `config`, `theme`, `application`.
- **Representative Projects:** Dunst, Mako, SwayNC (Sway Notification Center), Fnott.
- **Dependencies & Compatibility:** D-Bus notification interface (`org.freedesktop.Notifications`).
- **Install/Removal Implications:** Replaces active notification server; reloads via D-Bus or signal (`makoctl reload`).
- **Preview Requirements:** Screenshot displaying popup notification cards (Low, Normal, Critical).

---

### 10. OSDs and Control Centers
- **Description:** On-screen displays for volume, brightness, caps-lock, and centralized quick-setting toggles.
- **Package Types:** `config`, `theme`, `application`.
- **Representative Projects:** SwayOSD, Avizo, QuickSettings (AGS).
- **Dependencies & Compatibility:** Layer-shell protocol; PipeWire, Brightnessctl, UPower.
- **Install/Removal Implications:** Binds hardware media keys in Hyprland config; starts background listener daemon.
- **Preview Requirements:** Screenshot showing volume slider and brightness bubble popup.

---

### 11. Lock Screens and Idle Management
- **Description:** Screen lockers, screen dimming, and DPMS sleep listeners.
- **Package Types:** `config`, `theme`, `application`.
- **Representative Projects:** Hyprlock, Hypridle, Swaylock-effects, Waylock.
- **Dependencies & Compatibility:** `ext-session-lock-v1` protocol; PAM authentication.
- **Install/Removal Implications:** Modifies `~/.config/hypr/hyprlock.conf` and `hypridle.conf`.
- **Preview Requirements:** HiDPI screenshot of the lockscreen UI with clock, avatar, and password field.

---

### 12. Hyprland Plugins and Extensions
- **Description:** Dynamic C++ extensions loaded into Hyprland's compositor address space.
- **Package Types:** `plugin`.
- **Representative Projects:** Hyprsplit, Hyprbars, Hyprexpo, Hyprtrails, Hyprwinwrap.
- **Dependencies & Compatibility:** **Strict Hyprland C++ ABI match** (`hyprctl version` commit hash); `cmake`, `cpio`, `hyprland-headers`.
- **Install/Removal Implications:** Loads dynamically via `hyprctl plugin load <path>`; injects plugin config block into `hyprland.conf`.
- **Preview Requirements:** Animated GIF/WebP showing the visual or layout effect in action.

---

### 13. Animations, Layouts, and Window Rules
- **Description:** Custom bezier animation curves, window decoration borders, gaps, gestures, and layout rules.
- **Package Types:** `config`.
- **Representative Projects:** Smooth bezier presets, floating window rules, hyprland-autotiling.
- **Dependencies & Compatibility:** Hyprland version compatibility.
- **Install/Removal Implications:** Injects declarative snippets into `~/.config/hypr/conf.d/`.
- **Preview Requirements:** Video or animated WebP showing window opening, closing, and workspace transitions.

---

### 14. Keybindings and Configuration Presets
- **Description:** Ergonomic keybinding presets (Vim-like, i3-compatible, macOS-like).
- **Package Types:** `config`.
- **Representative Projects:** Vim-nav keybinds, Gaming mode toggles, Scratchpad toggles.
- **Dependencies & Compatibility:** XKB keyboard layouts.
- **Install/Removal Implications:** Modular inclusion via `source = ~/.config/hypr/conf.d/keybinds.conf`.
- **Preview Requirements:** Visual keyboard cheat-sheet graphic showing key assignments.

---

### 15. Workspace Automation and Scripts
- **Description:** Automation daemons for dynamic workspace naming, application autostart, and layout scripting.
- **Package Types:** `script`, `config`.
- **Representative Projects:** Hyprworkspace, Workstyle, Hyprland-socket-listener.
- **Dependencies & Compatibility:** Python, Bash, or Rust runtimes; Hyprland UNIX domain socket IPC.
- **Install/Removal Implications:** Managed as a user systemd service or `exec-once` daemon.
- **Preview Requirements:** Diagram illustrating automated workspace assignment rules.

---

### 16. Clipboard Managers
- **Description:** Clipboard history daemons, selection pickers, and clipboard viewers.
- **Package Types:** `config`, `application`.
- **Representative Projects:** Cliphist, Wl-clipboard, Copyq.
- **Dependencies & Compatibility:** `wl-clipboard`, SQLite.
- **Install/Removal Implications:** Configures background listener (`wl-paste --watch cliphist store`).
- **Preview Requirements:** Screenshot showing clipboard selection picker menu.

---

### 17. Screenshot and Recording Tools
- **Description:** Wayland screen capture, area selection, annotation, and screencast tools.
- **Package Types:** `config`, `script`, `application`.
- **Representative Projects:** Grim, Slurp, Hyprshot, Grimblast, Wf-recorder, Satty.
- **Dependencies & Compatibility:** `xdg-desktop-portal-hyprland`, `pipewire`.
- **Install/Removal Implications:** Binds `Print` key to capture script; creates `~/Pictures/Screenshots/`.
- **Preview Requirements:** Screenshot of the interactive annotation UI in action.

---

### 18. Audio Controls and Effects
- **Description:** PipeWire equalizers, visualizers, volume mixers, and audio switcher widgets.
- **Package Types:** `config`, `application`.
- **Representative Projects:** Cava, Pavucontrol, EasyEffects, WirePlumber scripts.
- **Dependencies & Compatibility:** PipeWire audio daemon; WirePlumber.
- **Install/Removal Implications:** Deploys `~/.config/cava/config` and PipeWire presets.
- **Preview Requirements:** Screenshot of audio spectrum visualizer dancing to music.

---

### 19. Brightness and Color-Temperature Utilities
- **Description:** Blue-light reduction daemons, gamma correction, and monitor backlight controls.
- **Package Types:** `config`, `application`.
- **Representative Projects:** Gammastep, Hyprsunset, Wlsunset, Brightnessctl.
- **Dependencies & Compatibility:** `wlr-gamma-control` protocol; `sysfs` backlight access (udev rules).
- **Install/Removal Implications:** Configures systemd user service for automated twilight transitions.
- **Preview Requirements:** Comparison graphic showing day vs warm night color temperatures.

---

### 20. Display and Monitor Management
- **Description:** Multi-monitor hotplug automation, resolution switchers, and EDID profile managers.
- **Package Types:** `config`, `application`.
- **Representative Projects:** Kanshi, Wdisplays, Nwg-displays.
- **Dependencies & Compatibility:** Wayland output management protocol.
- **Install/Removal Implications:** Deploys `~/.config/kanshi/config`.
- **Preview Requirements:** Multi-monitor schematic diagram showing laptop + external display arrangements.

---

### 21. Authentication and Desktop Portals
- **Description:** Polkit authentication agents, screen sharing portals, and keyring integrations.
- **Package Types:** `config`, `application`.
- **Representative Projects:** Polkit-gnome, Hyprpolkitagent, Xdg-desktop-portal-hyprland.
- **Dependencies & Compatibility:** D-Bus system bus; Polkit framework.
- **Install/Removal Implications:** Configures autostart in `hyprland.conf` (`exec-once = systemctl --user start ...`).
- **Preview Requirements:** Screenshot of the graphical authentication password dialogue.

---

### 22. File Managers and Terminal Integrations
- **Description:** GUI and TUI file managers with custom previews and icons.
- **Package Types:** `config`, `application`.
- **Representative Projects:** Yazi, Thunar, Dolphin, Ranger.
- **Dependencies & Compatibility:** Chafa/Überzug for image previews; FZF.
- **Install/Removal Implications:** Places configs in `~/.config/yazi/`.
- **Preview Requirements:** Screenshot showing file manager with image thumbnail previews.

---

### 23. Power Menus and Power Management
- **Description:** Graphical logout screens, reboot/shutdown menus, and battery management daemons.
- **Package Types:** `config`, `theme`, `application`.
- **Representative Projects:** Wlogout, UPower, TLP, Auto-cpufreq.
- **Dependencies & Compatibility:** Systemd login manager (`loginctl`).
- **Install/Removal Implications:** Keybinding bound to `wlogout`; custom CSS styling deployed.
- **Preview Requirements:** Fullscreen blur screenshot showing circular power action buttons.

---

### 24. Input Methods and Keyboard Integrations
- **Description:** Multilingual input method frameworks (CJK), emoji pickers, and virtual keyboards.
- **Package Types:** `config`, `application`.
- **Representative Projects:** Fcitx5, Bmoji, Wvkbd.
- **Dependencies & Compatibility:** Wayland `text-input-v3` protocol.
- **Install/Removal Implications:** Sets environment variables (`GTK_IM_MODULE=fcitx`).
- **Preview Requirements:** Screenshot of the Asian input suggestion popup over a text editor.

---

### 25. Desktop Applications and Desktop Integrations
- **Description:** Standalone native Linux utilities tailored for the Hyprland desktop workflow.
- **Package Types:** `application`.
- **Representative Projects:** Btop, Fastfetch, Cmatrix, Nwg-look.
- **Dependencies & Compatibility:** Standard Linux libraries; package manager availability.
- **Install/Removal Implications:** Installs binary packages; places launcher `.desktop` entries.
- **Preview Requirements:** Application window screenshot.

---

### 26. Component Bundles
- **Description:** Curated multi-package sets that unite 2 to 6 individual components (e.g. Matching Waybar + Wallpaper + Kitty Theme).
- **Package Types:** `bundle`.
- **Representative Projects:** "Catppuccin Macchiato Suite", "Nord Core Essentials".
- **Dependencies & Compatibility:** References other store packages; atomic parent-child resolution.
- **Install/Removal Implications:** Orchestrates multi-package transaction graph; supports selective child toggling.
- **Preview Requirements:** Collage graphic showcasing the cohesive set of components.

---

### 27. Complete Desktop Configurations ("Full Rices")
- **Description:** End-to-end, turnkey desktop transformation suites orchestrating all shell components.
- **Package Types:** `desktop_setup`.
- **Representative Projects:** End-4 Illogical Impulse, ML4W Suite, JaKooLit Setup, HyDE.
- **Dependencies & Compatibility:** Extensive native packages and user daemons; comprehensive environment requirements.
- **Install/Removal Implications:** Full configuration tree deployment; mandatory complete pre-install snapshot; atomic session activation.
- **Preview Requirements:** 4-panel showcase gallery displaying clean desktop, busy workspace, launcher active, and lockscreen.

---

### 28. Creator Collections and Curated Collections
- **Description:** Curated discovery collections grouping packages by verified authors or aesthetic movements (e.g. "Cyberpunk 2077 Theme Pack", "Official Hyprland Team Picks").
- **Package Types:** `bundle`, `theme`.
- **Representative Projects:** Hyprwm Community Showcase, Unixporn Hall of Fame.
- **Dependencies & Compatibility:** Meta-packages referencing member packages.
- **Install/Removal Implications:** Can be installed collectively or browsed individually.
- **Preview Requirements:** Custom banner artwork and curator notes.
