#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Writes the §22B-1e1 measurement plan.

Every scenario is one document change away from one base document, and every
scenario tracks the same references, so the report answers the same question
about Geometry, Model and Material each time. The tracked anchors are durable
FerriteCAD keys, never display names and never positions.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

EARLY = "step.product_definition#50"
ALPHA = "step.product_definition#100"
BETA = "step.product_definition#200"
GAMMA = "step.product_definition#300"

# What every synthetic scenario tracks. Geometry, Model and Material are
# measured apart on purpose: an importer may identify one of them by a rule it
# does not use for the others.
MESHES = [EARLY, ALPHA, BETA, GAMMA]
MATERIALS = [f"{ALPHA}@0", f"{ALPHA}@1", f"{BETA}@0"]
OBJECTS = [f"{ALPHA}@0", f"{ALPHA}@1", f"{GAMMA}@0"]

# The mandatory variants. The number in front of each name is the scenario it
# answers in the §22B-1e1 brief, so a missing one is visible in the report.
SCENARIOS = [
    ("s01-byte-identical-reimport", "base.fbx", "the same bytes at the same asset path"),
    ("s02-reexport-unchanged-document", "reexport.fbx", "an unchanged document exported again"),
    ("s03-display-name-only", "renamed.fbx", "one display name; every durable key unchanged"),
    ("s04a-insert-earlier-definition", "inserted-definition.fbx", "an unrelated definition added before the tracked ones"),
    ("s04b-remove-earlier-definition", "removed-definition-earlier.fbx", "an unrelated earlier definition removed"),
    ("s05-reorder-definitions", "reordered-definitions.fbx", "two unrelated definitions swapped in export order"),
    ("s06a-insert-sibling", "inserted-sibling.fbx", "a sibling occurrence added between two tracked ones"),
    ("s06b-remove-sibling", "removed-sibling.fbx", "a sibling occurrence removed"),
    ("s06c-reorder-siblings", "reordered-siblings.fbx", "two sibling occurrences swapped"),
    ("s12-remove-one-definition", "removed-tracked-definition.fbx", "one whole definition and its only placement removed"),
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--variants", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--complex", type=Path, help="the real AP203 assembly, exported twice")
    parser.add_argument("--complex-second", type=Path)
    parser.add_argument(
        "--basenames",
        action="store_true",
        help="write file names rather than this machine's paths, for the committed copy",
    )
    args = parser.parse_args()

    def name_of(path: Path) -> str:
        return path.name if args.basenames else str(path.resolve())

    scenarios = []
    # The real assembly needs Open CASCADE and a third of a gigabyte of ASCII,
    # so it is a separate plan with a separate recorded measurement. Making the
    # portable one conditional on a kernel would mean two different canonical
    # reports under one name.
    if args.complex is None:
        base = args.variants / "base.fbx"
        if not base.is_file():
            raise SystemExit(f"the base document is missing: {base}")
        for name, after, change in SCENARIOS:
            path = args.variants / after
            if not path.is_file():
                raise SystemExit(f"the variant {after} was not written")
            scenarios.append(
                {
                    "name": name,
                    "change": change,
                    "before": name_of(base),
                    "after": name_of(path),
                    "mesh_definitions": MESHES,
                    "material_bindings": MATERIALS,
                    "object_bindings": OBJECTS,
                }
            )

    # Scenario 11: the real assembly whose repeated designations already make
    # the editor warn. Nothing about it is edited here, because editing a STEP
    # source is not this slice; it is imported twice, and exported twice.
    if args.complex is not None:
        first = args.complex.resolve()
        second = (args.complex_second or args.complex).resolve()
        for name, after, change in [
            ("s11a-complex-byte-identical", first, "the real AP203 assembly, the same bytes again"),
            ("s11b-complex-reexported", second, "the real AP203 assembly, exported again"),
        ]:  # noqa: B007 - `after` is a path here, not a variant file name
            scenarios.append(
                {
                    "name": name,
                    "change": change,
                    "before": name_of(first),
                    "after": name_of(Path(after)),
                    # The complex assembly's tracked keys are chosen by the
                    # runner from the oracle, because they come from a STEP
                    # file rather than from this script.
                    "mesh_definitions": complex_keys(args.variants, "meshes"),
                    "material_bindings": complex_keys(args.variants, "materials"),
                    "object_bindings": complex_keys(args.variants, "objects"),
                }
            )

    args.output.write_text(
        json.dumps({"scenarios": scenarios}, indent=1, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"planned {len(scenarios)} scenarios")
    return 0


def complex_keys(variants: Path, kind: str) -> list[str]:
    """The AP203 anchors the runner chose, written beside the variants."""
    path = variants / "complex-anchors.json"
    if not path.is_file():
        raise SystemExit("the complex anchors were not chosen before planning")
    return json.loads(path.read_text(encoding="utf-8"))[kind]


if __name__ == "__main__":
    raise SystemExit(main())
