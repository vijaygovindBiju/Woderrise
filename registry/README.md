# Woderrise Registry

The current local registry is an append-only collection of versioned package
directories. Each package directory contains one `woderrise.yaml` manifest and
the local artifact bytes it references. This is the canonical format consumed
by the current importer. A future sharded JSONL index remains a planned
distribution format and is not yet accepted by the client.

Installable entries require verifiable upstream provenance and SHA-256 hashes.

`fixtures/` contains exactly 50 deterministic schema-validation manifests.
They are explicitly non-publishable test data, not claims of official or
community provenance, and are excluded from the installable package index.

`packages/` contains the small installable catalog. Its current entry is the
Catppuccin Waybar Mocha theme, pinned to an upstream commit and backed by a
locally stored artifact whose SHA-256 is verified during import.
