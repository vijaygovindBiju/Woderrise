"""Validate the deterministic, non-publishable seed fixture catalog."""
import json
from pathlib import Path

ROOT = Path(__file__).parent
index = json.loads((ROOT / "index.json").read_text())
assert len(index["entries"]) == 50
assert all(entry["fixture"] for entry in index["entries"])
for entry in index["entries"]:
    path = ROOT / Path(entry["path"]).name
    assert path.exists() and path.parent == ROOT
    text = path.read_text()
    assert "schema_version: '1.0.0'" in text
    assert f"id: {entry['id']}" in text
print("50 deterministic fixture manifests validated")
