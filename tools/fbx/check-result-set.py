#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Check native Body identity and components from the pinned reader's reports."""
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "triangles", Path(__file__).with_name("stl-matches-fbx.py"))
triangles = importlib.util.module_from_spec(spec)
spec.loader.exec_module(triangles)
expected = json.loads(Path(sys.argv[1]).read_text())
identity = Path(sys.argv[2]).read_text().splitlines()
rows = [line.split("\t") for line in identity if line.startswith("FCAD_IDENTITY\t")]
assert len(rows) == 1, rows
assert rows[0][3] == "fcad1:def:object:" + expected["body"], rows
assert rows[0][4] == "fcad1:occ:object:" + expected["body"], rows
summary = next(line for line in identity if line.startswith("FCAD_IDENTITY_SUMMARY"))
assert set(summary.split()[1:]) == {
    "nodes=1", "definitions=1", "occurrences=1", "meshes=1"}, summary
report = Path(sys.argv[3])
summary = next(line for line in report.read_text().splitlines()
               if line.startswith("FCAD_TRIANGLE_SUMMARY"))
assert "instances=1" in summary.split(), "one Body must have one mesh instance"
mesh = triangles.fbx(str(report))
# Union by shared vertices measures connectivity of this disconnected fixture.
parents = list(range(len(mesh)))


def find(i):
    while parents[i] != i:
        parents[i] = parents[parents[i]]
        i = parents[i]
    return i


seen = {}
for index, triangle in enumerate(mesh):
    for point in triangle:
        key = tuple(round(value, 8) for value in point)
        if key in seen:
            parents[find(index)] = find(seen[key])
        else:
            seen[key] = index
components = {}
for index, (a, b, c) in enumerate(mesh):
    volume = (a[0] * (b[1]*c[2] - b[2]*c[1]) + a[1] * (b[2]*c[0] - b[0]*c[2])
              + a[2] * (b[0]*c[1] - b[1]*c[0])) / 6
    root = find(index)
    components[root] = components.get(root, 0) + volume
assert len(components) == expected["solids"], components
assert all(v > 0 for v in components.values()), components
assert abs(sum(components.values()) * 1e9 - expected["volume_mm3"]) < 1e-3, components
noun = "component" if len(components) == 1 else "components"
print(f"Body {expected['body']}: one definition/placement, {len(components)} {noun}, volume OK")
