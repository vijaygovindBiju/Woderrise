# Package Manifest Specification (`woderrise.yaml`)

> **Document ID:** SPEC-ECO-SPEC-009  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Specification Overview & Versioning

All packages distributed within the Woderrise ecosystem are defined by a single, declarative YAML manifest located at the root of the package archive or repository: `woderrise.yaml`.

The manifest schema is strictly versioned:
- Current Specification: `schema_version: "1.0.0"`
- Schema Evolution: Backwards-incompatible schema changes will increment the MAJOR version. The engine guarantees compatibility with older minor versions.
- Schema Validator: Manifests are validated against the formal JSON Schema Draft 2020-12 definition before parsing.

---

## 2. Complete Manifest Schema (v1.0.0)

```yaml
# =============================================================================
# WODERRISE PACKAGE MANIFEST SPECIFICATION v1.0.0
# =============================================================================
schema_version: "1.0.0"

package:
  id: "com.example.packagename"
  name: "Human Readable Package Name"
  version: "1.0.0"
  type: "config" # asset | theme | config | plugin | application | script | bundle | desktop_setup
  category: "bars_panels" # One of the 28 taxonomy categories
  license: "MIT" # Valid SPDX identifier
  authors:
    - name: "Author Name"
      email: "author@example.com"
      url: "https://github.com/author"
  description: "A concise 1-2 sentence description of the package."
  tags: ["hyprland", "waybar", "catppuccin", "minimal"]
  repository: "https://github.com/author/repo"

previews:
  thumbnail: "previews/thumb.webp"
  screenshots:
    - path: "previews/dark.webp"
      caption: "Dark Mode Layout"
    - path: "previews/light.webp"
      caption: "Light Mode Layout"
  palette:
    - name: "Background"
      hex: "#1e1e2e"
    - name: "Accent"
      hex: "#cba6f7"

compatibility:
  compositors:
    - name: "hyprland"
      version: ">=0.40.0"
  display_server: "wayland_only" # wayland_only | any
  architectures: ["x86_64", "aarch64"]

dependencies:
  system:
    - name: "waybar"
      version: ">=0.9.20"
  packages:
    - id: "com.catppuccin.gtk"
      version: "^1.0.0"
  optional:
    - id: "com.catppuccin.cava"
      description: "Audio visualizer styling"

conflicts:
  - id: "com.other.bar"
    reason: "Both packages manage ~/.config/waybar/config.jsonc"

artifacts:
  - path: "artifacts/config.jsonc"
    sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
  - path: "artifacts/style.css"
    sha256: "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a"

install:
  files:
    - source: "artifacts/config.jsonc"
      target: "~/.config/waybar/config.jsonc"
      mode: "0644"
      strategy: "replace" # replace | preserve_if_exists
      backup_before_write: true

    - source: "artifacts/style.css"
      target: "~/.config/waybar/style.css"
      mode: "0644"
      strategy: "replace"
      backup_before_write: true

  config_injections:
    - target: "~/.config/hypr/conf.d/woderrise_exec.conf"
      source_block: "exec-once = waybar"
      unique_marker: "woderrise:exec:waybar"

  service_triggers:
    - action: "send_signal"
      process_name: "waybar"
      signal: "SIGUSR2"

removal:
  revert_files: true
  remove_injections: true
  service_triggers:
    - action: "send_signal"
      process_name: "waybar"
      signal: "SIGUSR2"
```

---

## 3. Concrete Validated Manifest Examples

### Example 1: Standalone 8K Wallpaper (`asset`)
```yaml
schema_version: "1.0.0"

package:
  id: "org.woderrise.wallpapers.neon-rain"
  name: "Neon Rain (8K)"
  version: "1.0.0"
  type: "asset"
  category: "wallpapers_engines"
  license: "CC-BY-4.0"
  authors:
    - name: "Artist Studio"
      url: "https://artstation.com/artist"
  description: "Cinematic cyberpunk rainy street wallpaper in 8K resolution."
  tags: ["wallpaper", "cyberpunk", "8k", "dark"]

previews:
  thumbnail: "previews/thumb.webp"
  screenshots:
    - path: "previews/neon-rain-full.webp"
      caption: "Full resolution preview"
  palette:
    - name: "Neon Cyan"
      hex: "#00f0ff"
    - name: "Neon Magenta"
      hex: "#ff003c"
    - name: "Deep Navy"
      hex: "#05051a"

artifacts:
  - path: "artifacts/neon-rain.png"
    sha256: "8f434346648f6b96df89dda901c5176b10a6d83961dd3c1ac88b59b2dc327aa4"

install:
  files:
    - source: "artifacts/neon-rain.png"
      target: "~/.local/share/backgrounds/woderrise/neon-rain.png"
      mode: "0644"
  service_triggers:
    - action: "apply_wallpaper"
      file: "~/.local/share/backgrounds/woderrise/neon-rain.png"
```

---

### Example 2: Waybar Minimal Glow Theme (`theme` / `config`)
```yaml
schema_version: "1.0.0"

package:
  id: "com.community.waybar-minimal-glow"
  name: "Waybar Minimal Glow"
  version: "1.2.0"
  type: "config"
  category: "bar_themes_modules"
  license: "MIT"
  authors:
    - name: "AestheticRicers"
      email: "ricers@example.org"
  description: "Floating, rounded Waybar layout with Catppuccin Mocha neon glow."
  tags: ["waybar", "catppuccin", "floating"]

dependencies:
  system:
    - name: "waybar"
      version: ">=0.9.22"
    - name: "otf-font-awesome"

artifacts:
  - path: "artifacts/config.jsonc"
    sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
  - path: "artifacts/style.css"
    sha256: "8277e0910d750195b448797616e091ad79a620985a4f923c3a25ed52fddec511"

install:
  files:
    - source: "artifacts/config.jsonc"
      target: "~/.config/waybar/config.jsonc"
      strategy: "replace"
      backup_before_write: true
    - source: "artifacts/style.css"
      target: "~/.config/waybar/style.css"
      strategy: "replace"
      backup_before_write: true
  service_triggers:
    - action: "send_signal"
      process_name: "waybar"
      signal: "SIGUSR2"
```

---

### Example 3: Rofi Glass Application Launcher (`config`)
```yaml
schema_version: "1.0.0"

package:
  id: "io.github.themer.rofi-glass"
  name: "Rofi Glass Blur Launcher"
  version: "1.0.1"
  type: "config"
  category: "application_launchers_menus"
  license: "GPL-3.0-only"
  authors:
    - name: "GlassThemer"

dependencies:
  system:
    - name: "rofi-wayland"

artifacts:
  - path: "artifacts/config.rasi"
    sha256: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
  - path: "artifacts/glass-theme.rasi"
    sha256: "17ac6ca2c332462b4aac5d0309fb6be518747f02d2edd4abec82d6134ee7893d"

install:
  files:
    - source: "artifacts/config.rasi"
      target: "~/.config/rofi/config.rasi"
      strategy: "replace"
      backup_before_write: true
    - source: "artifacts/glass-theme.rasi"
      target: "~/.config/rofi/glass-theme.rasi"
      strategy: "replace"
  config_injections:
    - target: "~/.config/hypr/conf.d/woderrise_keybinds.conf"
      source_block: "bind = SUPER, SPACE, exec, rofi -show drun -theme ~/.config/rofi/glass-theme.rasi"
      unique_marker: "woderrise:keybind:rofi"
  service_triggers:
    - action: "reload_hyprland"
```

---

### Example 4: Hyprsplit Dynamic Plugin (`plugin`)
```yaml
schema_version: "1.0.0"

package:
  id: "org.hyprland.plugins.hyprsplit"
  name: "Hyprsplit Workspace Manager"
  version: "0.41.2"
  type: "plugin"
  category: "hyprland_plugins_extensions"
  license: "BSD-3-Clause"
  authors:
    - name: "Hyprwm Team"

compatibility:
  compositors:
    - name: "hyprland"
      version: "==0.41.2"
      # Cryptographic hash of Hyprland C++ headers for this release
      abi_hash: "3b9a71028dfbc611c01e92d84784a92c0199e81b9578ec09d841b9"

artifacts:
  - path: "artifacts/hyprsplit.so"
    sha256: "d5a835a045ff801455bb5bb26c368d184768ab30541243fa4a86df7fe381284a"

install:
  files:
    - source: "artifacts/hyprsplit.so"
      target: "~/.local/share/woderrise/plugins/hyprsplit.so"
      mode: "0755"
  service_triggers:
    - action: "hyprland_plugin_load"
      plugin_path: "~/.local/share/woderrise/plugins/hyprsplit.so"
```

---

### Example 5: Nord Minimalist Suite (`bundle`)
```yaml
schema_version: "1.0.0"

package:
  id: "com.nordic.nord-minimal-suite"
  name: "Nord Minimalist Suite"
  version: "2.0.0"
  type: "bundle"
  category: "component_bundles"
  license: "MIT"
  authors:
    - name: "Nordic Design Collective"
  description: "A cohesive desktop bundle featuring Nord Waybar, GTK Theme, Wallpaper, and Kitty Config."

dependencies:
  packages:
    - id: "org.woderrise.wallpapers.nord-landscape"
      version: "^1.0.0"
    - id: "com.nordic.waybar-nord-pill"
      version: "^2.0.0"
    - id: "com.nordic.kitty-nord-theme"
      version: "^1.1.0"
    - id: "org.nordtheme.gtk"
      version: "^0.4.0"
```

---

### Example 6: End-4 Illogical Impulse Setup (`desktop_setup`)
```yaml
schema_version: "1.0.0"

package:
  id: "io.end4.illogical-impulse"
  name: "End-4 Illogical Impulse Desktop"
  version: "3.2.0"
  type: "desktop_setup"
  category: "complete_desktop_configurations"
  license: "GPL-3.0-or-later"
  authors:
    - name: "End-4"
      url: "https://github.com/end-4"
  description: "Stunning Material-You dynamic theming suite with custom widgets, AGS control center, and fluid animations."

compatibility:
  compositors:
    - name: "hyprland"
      version: ">=0.40.0"

dependencies:
  system:
    - name: "aylurs-gtk-shell"
    - name: "swww"
    - name: "dart-sass"
    - name: "swaync"
    - name: "brightnessctl"
    - name: "wl-clipboard"
    - name: "ttf-material-symbols-variable-git"

install:
  mandate_full_snapshot: true
  files:
    - source: "artifacts/hyprland.conf"
      target: "~/.config/hypr/hyprland.conf"
      strategy: "replace"
      backup_before_write: true
    - source: "artifacts/ags_config.tar.gz"
      target: "~/.config/ags/"
      strategy: "extract_archive"
      backup_before_write: true
  service_triggers:
    - action: "user_systemd_restart"
      unit: "ags.service"
    - action: "reload_hyprland"
```

---

## 4. Rejection Policy for Unsupported & Unsafe Operations

To protect user workstations, the manifest parser strictly rejects manifests containing:
1. **Unrestricted Arbitrary Commands:** Any manifest attempting to declare raw `exec: "curl ... | bash"` without a sandboxed, isolated adapter is rejected during schema validation.
2. **Absolute Path Traversal Outside Allowed XDG Roots:** Any destination path containing `../` or targeting paths outside `$HOME` (e.g. `/etc`, `/usr/bin`) is immediately rejected.
3. **Missing or Corrupt Checksums:** If an artifact file's hash does not strictly match the declared `sha256` value, validation halts immediately.
