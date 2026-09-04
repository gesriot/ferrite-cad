#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Writes the §22B-1e2a measurement plan.

Every candidate is measured on the same twelve document changes and tracks the
same seventeen references, so a difference between two rows of the result is a
difference between the two candidates and not between two experiments.

The anchors are *source-qualified* definition identities — `<ImportedSourceId>/
<source-local key>` — for every candidate, including the control. That is
deliberate. The control's files cannot express the source half, so its anchor
for `step.product_definition#42` matches two different definitions, and the
probe reports that as an ambiguous join rather than picking one. Giving the
control a weaker anchor list would hide exactly the thing this slice is about.

Two editor runs, because one of the candidates is not a property of a file. The
vanilla plan holds the candidates a stock Unity understands; the companion plan
holds `d-companion`, which is `c-property`'s bytes imported with the FerriteCAD
companion postprocessor active, plus `a-control` again as the control that the
plugin changes nothing it was not asked to change.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

FIRST_SOURCE = "019ffc72-2996-7000-8000-0000000000a1"
SECOND_SOURCE = "019ffc72-2996-7000-8000-0000000000b2"

ROOT = f"{FIRST_SOURCE}/step.product_definition#1"
FRAME = f"{FIRST_SOURCE}/step.product_definition#2"
TWIN_A = f"{FIRST_SOURCE}/step.product_definition#42"
TWIN_B = f"{SECOND_SOURCE}/step.product_definition#42"
EARLY = f"{FIRST_SOURCE}/step.product_definition#50"
ALPHA = f"{FIRST_SOURCE}/step.product_definition#100"
BETA = f"{FIRST_SOURCE}/step.product_definition#200"
GAMMA = f"{FIRST_SOURCE}/step.product_definition#300"
OMITTED = f"{FIRST_SOURCE}/step.product_definition#400"

# Geometry, Model and Material are tracked apart, always: §22B-1e1 measured
# that Unity takes their names from three different places, so a result about
# one of them is not a result about the others.
MESHES = [EARLY, ALPHA, BETA, GAMMA, TWIN_A, TWIN_B]
MATERIALS = [f"{ALPHA}@0", f"{ALPHA}@1", f"{BETA}@0", f"{TWIN_A}@0", f"{TWIN_B}@0"]
OBJECTS = [
    f"{ALPHA}@0",
    f"{ALPHA}@1",
    f"{GAMMA}@0",
    f"{TWIN_A}@0",
    f"{TWIN_B}@0",
    f"{OMITTED}@0",
    f"{FRAME}@0",
]

# The brief's reimport scenarios, each one document change away from the base.
SCENARIOS = [
    ("s01-byte-identical", "base.fbx", "the same bytes at the same asset path"),
    ("s02-reexport-unchanged", "reexport.fbx", "an unchanged document exported again"),
    ("s03-display-name-only", "renamed.fbx", "one designation; every durable identity unchanged"),
    ("s04a-insert-definition", "inserted-definition.fbx", "an unrelated definition added before the tracked ones"),
    ("s04b-remove-definition", "removed-definition.fbx", "an unrelated earlier definition removed"),
    ("s04c-reorder-definitions", "reordered-definitions.fbx", "two unrelated definitions swapped in export order"),
    ("s05a-insert-sibling", "inserted-sibling.fbx", "a sibling occurrence added between two tracked ones"),
    ("s05b-remove-sibling", "removed-sibling.fbx", "a sibling occurrence removed"),
    ("s05c-reorder-siblings", "reordered-siblings.fbx", "two sibling occurrences swapped"),
    ("s06-remove-tracked-definition", "removed-tracked-definition.fbx", "one whole definition and its only placement removed"),
    ("s07-change-material", "changed-material.fbx", "one material slot changes designation and colour"),
    ("s08-reuse-material", "reused-material.fbx", "a second definition gains a slot with an existing designation"),
]

WRITTEN_BY = {
    "a-control": "fbx_channel_documents example over write_fbx_ascii_7400, copied unchanged",
    "b-ordinal": "fbx_channel_documents example over write_fbx_ascii_7400, then "
    "rewrite_channel.py --candidate b-ordinal",
    "b-occurrence": "fbx_channel_documents example over write_fbx_ascii_7400, then "
    "rewrite_channel.py --candidate b-occurrence",
    "c-property": "fbx_channel_documents example over write_fbx_ascii_7400, then "
    "rewrite_channel.py --candidate c-property",
    "d-companion": "byte-identical to c-property; what differs is the FerriteCAD companion "
    "postprocessor in the editor, not the file",
}

# name, directory the bytes live in, what it carries, whether it needs a plugin.
CANDIDATES = {
    "a-control": ("a-control", False, False, False, False),
    "b-ordinal": ("b-ordinal", True, False, False, False),
    "b-occurrence": ("b-occurrence", True, True, False, False),
    "c-property": ("c-property", True, True, True, False),
    "d-companion": ("c-property", True, True, True, True),
}

MODES = {
    "vanilla": ["a-control", "b-ordinal", "b-occurrence", "c-property"],
    "companion": ["a-control", "d-companion"],
}

NAME_PROBES = [
    ("n01-ascii-source-qualified", "the source-qualified token the candidates use, unchanged"),
    ("n02-long-token", "the same token with 160 more ASCII characters after it"),
    ("n03-non-ascii", "the real assembly's Cyrillic designation inside the token"),
    ("n04-short-hash", "a 16-hex-digit FNV-1a token instead of the readable identity"),
    ("n05-hash-collision", "two distinct durable identities given one token on purpose"),
    ("n06-unicode-normalisation", "one designation precomposed, another decomposed"),
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--staging", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(MODES), required=True)
    parser.add_argument(
        "--basenames",
        action="store_true",
        help="write file names rather than this machine's paths, for the committed copy",
    )
    args = parser.parse_args()

    def name_of(path: Path) -> str:
        return f"{path.parent.name}/{path.name}" if args.basenames else str(path.resolve())

    candidates = []
    scenarios = []
    for candidate in MODES[args.mode]:
        directory, definition_id, occurrence_id, display_name, companion = CANDIDATES[candidate]
        candidates.append(
            {
                "name": candidate,
                "written_by": WRITTEN_BY[candidate],
                "carries_definition_id": definition_id,
                "carries_occurrence_id": occurrence_id,
                "carries_display_name": display_name,
                "needs_companion": companion,
            }
        )
        base = args.staging / directory / "base.fbx"
        if not base.is_file():
            raise SystemExit(f"the base document is missing: {base}")
        for name, after, change in SCENARIOS:
            path = args.staging / directory / after
            if not path.is_file():
                raise SystemExit(f"the document {after} was not written for {candidate}")
            scenarios.append(
                {
                    "name": f"{candidate}/{name}",
                    "candidate": candidate,
                    "change": change,
                    "before": name_of(base),
                    "after": name_of(path),
                    "mesh_definitions": MESHES,
                    "material_bindings": MATERIALS,
                    "object_bindings": OBJECTS,
                }
            )

    # The naming questions are about the name channel itself and are the same
    # whichever candidate is being imported, so they are measured once, in the
    # vanilla run, where no plugin is renaming anything.
    names = []
    if args.mode == "vanilla":
        for name, question in NAME_PROBES:
            path = args.staging / "names" / f"{name}.fbx"
            if not path.is_file():
                raise SystemExit(f"the name probe {name} was not written")
            names.append({"name": name, "question": question, "file": name_of(path)})

    args.output.write_text(
        json.dumps(
            {"candidates": candidates, "scenarios": scenarios, "names": names},
            indent=1,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"planned {len(scenarios)} scenarios and {len(names)} name probes for {args.mode}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
