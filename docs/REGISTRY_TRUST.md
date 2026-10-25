# Registry Trust Design

Woderrise currently verifies package metadata and local artifact bytes with
SHA-256. It does **not** verify registry signatures. The following design is
the required trust model before signed remote registry content can be enabled.

## Signed data

The canonical signed payload is the UTF-8 bytes of the normalized manifest
plus the ordered artifact records (`path`, lowercase `sha256`, and URL). YAML
is parsed into the existing manifest model and re-serialized using a
deterministic canonical JSON representation before signing. The signature
record contains:

```json
{
  "key_id": "root-2026/maintainer-01",
  "algorithm": "ed25519",
  "payload_sha256": "…",
  "signature": "base64…"
}
```

Signatures cover the package ID and version, so a valid signature cannot be
replayed for another release.

## Trust roots and rotation

The client ships a small set of maintainer root Ed25519 public keys. A signed
keyring update can add or retire package-signing keys. Root-key rotation
requires a threshold signature from two currently trusted roots, and each
key has an `introduced_at`, `expires_at`, and optional `revoked_at` timestamp.
The client rejects expired or revoked keys for new installations.

## Failure and offline behavior

- Unknown keys are rejected; they are never trusted merely because a package
  contains the key.
- Invalid signatures, payload-hash mismatches, revoked keys, and malformed
  keyring updates make that package unavailable.
- An offline client may install an already cached artifact only when its
  previously verified signature, manifest hash, and artifact hash remain
  available locally.
- A failed keyring or registry refresh leaves the last known-good catalog and
  trust store unchanged.

Implementation is intentionally deferred until the registry format includes
canonical signature records and committed root-key infrastructure. The current
local package importer must not be described as signature-verified.
