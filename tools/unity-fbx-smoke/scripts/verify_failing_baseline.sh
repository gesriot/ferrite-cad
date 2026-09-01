#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
root="$(cd "$project/../.." && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-fbx-baseline.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT INT TERM

if rg -q 'ExportFbx|export-fbx' "$root/crates/ferritecad-cli/src"; then
  echo "production export-fbx route unexpectedly exists" >&2
  exit 1
fi
set +e
cargo run --quiet -p ferritecad-cli -- export-fbx > "$temporary/stdout" 2> "$temporary/stderr"
status=$?
set -e
if [ "$status" -ne 2 ] || ! grep -q "unrecognized subcommand 'export-fbx'" "$temporary/stderr"; then
  echo "export-fbx did not fail through the process CLI route as measured" >&2
  cat "$temporary/stdout" "$temporary/stderr" >&2
  exit 1
fi

python3 - "$root" <<'PY'
import json, re, sys
from pathlib import Path
root = Path(sys.argv[1])
snapshot = (root / "crates/ferritecad-viewport/src/snapshot.rs").read_text()
body = re.search(r"pub struct RenderSnapshot \{(.*?)\n\}", snapshot, re.S)
if not body:
    raise SystemExit("RenderSnapshot anchor missing")
fields = body.group(1)
for anchor in ("meshes: Vec<PackedMesh>", "items: Vec<DrawItem>", "face_owner", "edge_owner", "corner_owner"):
    if anchor not in fields:
        raise SystemExit(f"RenderSnapshot drawable anchor missing: {anchor}")
for forbidden in ("parent", "GeometryOmission", "source_unit", "schema", "name:"):
    if forbidden in fields:
        raise SystemExit(f"RenderSnapshot acquired export structure: {forbidden}")

persist = (root / "crates/ferritecad-exchange/src/persist.rs").read_text()
for anchor in ("pub source_unit: String", "pub schema: String", "pub definitions: Vec<PersistedDefinition>",
               "pub instances: Vec<PersistedInstance>", "pub parent: Option<u32>", "pub name: String",
               "pub placement: [f64; 12]", "pub colour_source: ColourSource", "pub colour: [f64; 3]"):
    if anchor not in persist:
        raise SystemExit(f"persisted Scene field missing: {anchor}")

contract = json.loads((root / "tools/unity-fbx-smoke/Assets/Fixtures/export-scene-contract.json").read_text())
measured = contract["complex_measurement"]
expected = (46, 1, 139, 35, 112)
actual = (measured["definitions"], measured["root_nodes"], measured["non_root_occurrences"],
          measured["render_snapshot_meshes"], measured["render_snapshot_draws"])
if actual != expected:
    raise SystemExit(f"complex/snapshot baseline changed: {actual}")
if measured["omitted_definition"] != "step.product_definition#2583":
    raise SystemExit("#2583 omission baseline missing")
if measured["real_mesh_definition"] != "step.product_definition#2428":
    raise SystemExit("#2428 mesh baseline missing")
print("FCAD_FBX_FAILING_BASELINE_EXECUTED checks=25")
PY
echo "FCAD_EXPORT_FBX_ABSENT exit=2"
