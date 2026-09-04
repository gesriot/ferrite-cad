#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Chooses the anchors for the real AP203 assembly, from the file itself.

The synthetic variants track keys this project wrote down. The real assembly's
keys come from a STEP file nobody here authored, so they are chosen from what
`ufbx` read, by a rule rather than by hand: the definitions the editor is most
likely to confuse, which are the ones whose designations repeat, plus the one
placed the most times, plus a spread over the rest.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--file", default="complex-first.fbx")
    args = parser.parse_args()

    oracle = json.loads(args.oracle.read_text(encoding="utf-8"))
    read = next((item for item in oracle["files"] if item["file"] == args.file), None)
    if read is None:
        raise SystemExit(f"the oracle never read {args.file}")

    placed: dict[str, list[dict]] = {}
    for node in read["nodes"]:
        if node["geometry_object_number"] == 0:
            continue
        placed.setdefault(node["definition_key"], []).append(node)
    if not placed:
        raise SystemExit("the real assembly has no placed geometry to anchor on")

    # Definitions a source called the same thing. This is the confusion the
    # editor already warns about, so it is measured first.
    by_name: dict[str, set[str]] = {}
    for key, nodes in placed.items():
        by_name.setdefault(nodes[0]["name"], set()).add(key)
    ambiguous = sorted(
        key
        for name, keys in by_name.items()
        if len(keys) > 1
        for key in keys
    )

    # The definition with the most placements, so shared geometry is measured.
    most = max(placed.items(), key=lambda item: (len(item[1]), item[0]))[0]

    ordered = sorted(placed)
    spread = [ordered[0], ordered[len(ordered) // 2], ordered[-1]]

    meshes: list[str] = []
    for key in ambiguous[:3] + [most] + spread:
        if key not in meshes:
            meshes.append(key)
    meshes = meshes[:6]

    materials = [f"{key}@0" for key in meshes[:3]]
    objects = [f"{most}@0", f"{most}@1"] if len(placed[most]) > 1 else [f"{most}@0"]
    for key in meshes[:2]:
        candidate = f"{key}@0"
        if candidate not in objects:
            objects.append(candidate)

    chosen = {"meshes": meshes, "materials": materials, "objects": objects}
    args.output.write_text(json.dumps(chosen, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    print(
        f"anchors: {len(meshes)} meshes, {len(materials)} materials, {len(objects)} objects; "
        f"{len(ambiguous)} definitions share a designation; the most placed one has "
        f"{len(placed[most])} placements"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
