# Woderrise Registry

The Woderrise registry provides verified package manifests, seed catalogs, and deterministic testing fixtures for the Woderrise Wayland configuration store.

Installable package entries require verifiable upstream provenance, pinned commit SHAs, and SHA-256 integrity hashes for all associated artifact bytes.

---

## Directory Structure

```
registry/
├── catalog.v1.json      # Pre-seeded offline catalog of top verified community Wayland configurations
├── fixtures/            # 50 deterministic schema-validation manifests for CI and indexing tests
├── packages/            # Installable package trees with versioned manifests and verified artifacts
└── README.md            # Registry documentation
```

### 1. `catalog.v1.json`
The pre-seeded catalog embedded into `woderrise-gui` (Tier 3 fallback). It guarantees 100% offline day-1 functionality, providing rich metadata, star counts, author profiles, and component tags for top real-world community configurations (e.g., Catppuccin, Hyprdots, ML4W, End-4) without requiring initial network queries or rate limit consumption.

### 2. `packages/`
Contains installable package trees. Each package directory holds a versioned `woderrise.yaml` manifest and the local artifact bytes referenced by that manifest. Woderrise validates the manifest, verifies every artifact SHA-256 hash, and stores canonical metadata before the application presents it as installable.

### 3. `fixtures/`
Contains exactly 50 schema-valid local fixtures generated from numeric IDs. These are strictly non-publishable test data used to exercise catalog indexing, schema validation, and CI test pipelines. They do not claim official or community provenance and are excluded from the installable package index.

---

## Catalog Resolution Pipeline

Woderrise resolves package catalog data through a deterministic 4-tier pipeline:
1. **Tier 1 (Remote Fastly CDN Feed)**: Fetches the latest published `catalog.v1.json` from the Fastly CDN edge endpoint (consuming 0 GitHub REST quota).
2. **Tier 2 (Local XDG Cache)**: If offline or CDN is unreachable, reads persisted cache from `$XDG_DATA_HOME/woderrise/catalog.json`.
3. **Tier 3 (Bundled Static Catalog)**: If cache is absent, falls back to the embedded `registry/catalog.v1.json` bundled at compile time.
4. **Tier 4 (Live Remote Queries)**: Explicit user-submitted searches query live GitHub endpoints when local results do not match.
