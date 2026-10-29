# Installable Registry Packages

Each package directory contains a versioned `woderrise.yaml` manifest and the local artifact bytes referenced by its manifest.

The Woderrise engine validates the manifest schema, verifies every artifact SHA-256 hash, and checks component path safety before the application presents the package as installable in the desktop interface.

---

## Verified Packages

### Catppuccin Waybar Mocha (`com.catppuccin.waybar`)
- **Pinned Upstream Commit:** `ee8ed32b4f63e9c417249c109818dcc05a2e25da`
- **Source:** Pinned GitHub raw URL with cryptographic SHA-256 validation.
- **Component Classification:** Waybar configuration (`config.jsonc`) and GTK CSS stylesheet (`style.css`).
- **Metadata:** Includes verified palette tokens and theme variants declared directly in the manifest.
