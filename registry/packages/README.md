# Installable registry packages

Each package directory contains a versioned `woderrise.yaml` and the local
artifact bytes referenced by its manifest. The importer validates the
manifest, verifies every artifact hash, and stores the exact manifest path in
SQLite before the TUI can display it as installable.

The Catppuccin Waybar entry is pinned to upstream commit
`ee8ed32b4f63e9c417249c109818dcc05a2e25da`. Its artifact was fetched from the
corresponding GitHub raw URL and its SHA-256 is recorded in the manifest. The
palette values are a small, manifest-declared subset copied from that pinned
CSS artifact; the TUI presents them as package metadata, not as an independently
sampled or cryptographically signed color analysis.
