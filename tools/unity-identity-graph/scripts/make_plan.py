#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Writes the §22B-1e2b measurement plans, and the synthetic documents part E reads.

Five editor modes, five plans, because three of the four questions change what
the editor *is*. A `.meta` probe that edits serialized importer metadata, an
`AddRemap` probe that puts external assets in the project and a
`ScriptedImporter` that registers an importer cannot share a project with each
other or with the graph measurement without each becoming a property of the
others.

The anchors are the same source-qualified definition identities §22B-1e2a used —
`<ImportedSourceId>/<source-local key>` — for every variant, including the
control. That is deliberate. The control's files cannot express the source half,
so its anchor for `step.product_definition#42` matches two different
definitions, and the probe reports that as an ambiguous join rather than picking
one. Giving the control a weaker anchor list would hide the thing the slice is
about.

The synthetic documents part E imports are written here rather than by the
importer, so the importer under test never chooses its own input. They carry the
same identities and the same confusions as the FBX documents: two sources
sharing one local key, two definitions sharing a designation, one definition
placed twice, several material slots, a structural node and an omitted one.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

FIRST_SOURCE = "019ffc72-2996-7000-8000-0000000000a1"
SECOND_SOURCE = "019ffc72-2996-7000-8000-0000000000b2"

ROOT = f"{FIRST_SOURCE}/step.product_definition#1"
FRAME = f"{FIRST_SOURCE}/step.product_definition#2"
INSERTED = f"{FIRST_SOURCE}/step.product_definition#10"
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

# The brief's reimport transitions, each one document change away from the base.
# `s06` removes a whole definition together with its only placement, so it is
# both "a tracked definition removed" and "a tracked occurrence removed", and
# the definition it removes shares its designation with one that stays — a
# retarget there would be silent.
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
    ("s06-remove-tracked-definition", "removed-tracked-definition.fbx", "the tracked definition and its only placement removed"),
    ("s07-change-material", "changed-material.fbx", "one material slot changes designation and colour"),
    ("s08-reuse-material", "reused-material.fbx", "a second definition gains a slot with an existing designation"),
]

WRITTEN_BY = {
    "g-flat": "fbx_channel_documents example over write_fbx_ascii_7400, copied unchanged",
    "g-flat-id": "fbx_channel_documents example over write_fbx_ascii_7400, then "
    "rewrite_graph.py --variant g-flat-id",
    "g-carrier": "fbx_channel_documents example over write_fbx_ascii_7400, then "
    "rewrite_graph.py --variant g-carrier",
    "g-carrier-detached": "fbx_channel_documents example over write_fbx_ascii_7400, then "
    "rewrite_graph.py --variant g-carrier-detached",
    "g-two-level": "fbx_channel_documents example over write_fbx_ascii_7400, then "
    "rewrite_graph.py --variant g-two-level",
}

TOPOLOGY = {
    "g-flat": "one Model per placement; the Geometry is connected to every placement",
    "g-flat-id": "the same graph as g-flat, with the invisible identity properties added",
    "g-carrier": "a machine-named definition carrier Model parented to the scene root claims "
    "the Geometry before any placement does",
    "g-carrier-detached": "the same carrier with no parent connection at all",
    "g-two-level": "each placement becomes a Null that keeps the designation and the transform, "
    "with a machine-named geometry-bearing child",
}

# name -> (definition id carried, occurrence id carried, adds objects)
VARIANTS = {
    "g-flat": (False, False, False),
    "g-flat-id": (True, True, False),
    "g-carrier": (True, True, True),
    "g-carrier-detached": (True, True, True),
    "g-two-level": (True, True, True),
}


def graph_plan(staging: Path, variants: list[str], name_of) -> dict:
    planned = []
    scenarios = []
    for variant in variants:
        definition_id, occurrence_id, adds = VARIANTS[variant]
        planned.append(
            {
                "name": variant,
                "written_by": WRITTEN_BY[variant],
                "topology": TOPOLOGY[variant],
                "carries_definition_id": definition_id,
                "carries_occurrence_id": occurrence_id,
                "adds_objects": adds,
            }
        )
        base = staging / variant / "base.fbx"
        if not base.is_file():
            raise SystemExit(f"the base document is missing: {base}")
        for name, after, change in SCENARIOS:
            path = staging / variant / after
            if not path.is_file():
                raise SystemExit(f"the document {after} was not written for {variant}")
            scenarios.append(
                {
                    "name": f"{variant}/{name}",
                    "variant": variant,
                    "change": change,
                    "before": name_of(base),
                    "after": name_of(path),
                    "mesh_definitions": MESHES,
                    "material_bindings": MATERIALS,
                    "object_bindings": OBJECTS,
                }
            )
    return {"variants": planned, "scenarios": scenarios}


def meta_plan(staging: Path, name_of) -> dict:
    control = staging / "g-flat"
    return {
        "control": name_of(control / "base.fbx"),
        "changed": name_of(control / "renamed.fbx"),
        "reexport": name_of(control / "reexport.fbx"),
    }


def remap_plan(staging: Path, name_of) -> dict:
    control = staging / "g-flat"
    return {
        "control": name_of(control / "base.fbx"),
        "renamed": name_of(control / "renamed.fbx"),
        "reexport": name_of(control / "reexport.fbx"),
        "removed_tracked_definition": name_of(control / "removed-tracked-definition.fbx"),
        "changed_material": name_of(control / "changed-material.fbx"),
        # Named rather than picked alphabetically, because the stale-content
        # question needs the *one* object the `changed-material` document
        # actually changes. That document moves Alpha's second slot from
        # `Shell` to `Shell Blue` and recolours it, and Unity names that slot
        # `Shell #2` after the writer's own suffix. A material the document
        # leaves alone would answer a different question.
        "mesh_to_remap": "Alpha Part",
        "material_to_remap": "Shell #2",
        "game_object_to_remap": "Alpha Part",
    }


def claim_plan(staging: Path, name_of) -> dict:
    return {"control": name_of(staging / "g-flat" / "base.fbx")}


# ------------------------------------------------- the synthetic documents

# The same occurrence identities the FBX manifest carries, so a reader can put
# the two halves of this slice side by side. FerriteCAD persists nothing of the
# kind today; these exist only inside the measurement.
ROOT_OCC = "019ffc72-2996-7000-9000-000000000001"
EARLY_OCC = "019ffc72-2996-7000-9000-000000000002"
ALPHA_FIRST_OCC = "019ffc72-2996-7000-9000-000000000003"
ALPHA_SECOND_OCC = "019ffc72-2996-7000-9000-000000000004"
BETA_OCC = "019ffc72-2996-7000-9000-000000000005"
GAMMA_OCC = "019ffc72-2996-7000-9000-000000000006"
TWIN_A_OCC = "019ffc72-2996-7000-9000-000000000007"
TWIN_B_OCC = "019ffc72-2996-7000-9000-000000000008"
OMITTED_OCC = "019ffc72-2996-7000-9000-000000000009"
FRAME_OCC = "019ffc72-2996-7000-9000-00000000000a"
NESTED_BETA_OCC = "019ffc72-2996-7000-9000-00000000000b"
INSERTED_OCC = "019ffc72-2996-7000-9000-00000000000c"
INSERTED_SIBLING_OCC = "019ffc72-2996-7000-9000-00000000000d"


def slot(designation: str, colour: tuple[float, float, float]) -> dict:
    return {"designation": designation, "r": colour[0], "g": colour[1], "b": colour[2]}


def synthetic(variant: str) -> dict:
    """The synthetic document, one change away from the base.

    Every field a `ScriptedImporter` identifier is derived from lives here, and
    every designation lives here too, in a different field. That separation is
    the whole of part E, so it is expressed in the document rather than left to
    the importer to arrange.
    """
    alpha_name = "Alpha Part" if variant != "renamed" else "Alpha Part Rev B"
    beta_slot = slot("Beta", (0.2, 0.4, 0.6))
    if variant == "changed-material":
        beta_slot = slot("Beta Rev B", (0.9, 0.1, 0.1))

    definitions = []
    if variant == "inserted-definition":
        definitions.append(
            {
                "definition_id": INSERTED,
                "designation": "Inserted Part",
                "vertices": 9,
                "slots": [slot("Inserted", (0.5, 0.5, 0.5))],
            }
        )
    definitions.append(
        {"definition_id": ROOT, "designation": "Assembly Root", "vertices": 0, "slots": []}
    )
    if variant != "remove-definition":
        definitions.append(
            {
                "definition_id": EARLY,
                "designation": "Early Part",
                "vertices": 6,
                "slots": [slot("Early", (0.1, 0.1, 0.1))],
            }
        )
    alpha = {
        "definition_id": ALPHA,
        "designation": alpha_name,
        "vertices": 12,
        # Two slots with one designation, because §22B-1c measured that a real
        # assembly has them and a scheme that only works without them has not
        # been measured.
        "slots": [slot("Shell", (0.3, 0.3, 0.3)), slot("Shell", (0.4, 0.4, 0.4))],
    }
    beta = {
        "definition_id": BETA,
        "designation": "Beta Part",
        "vertices": 15,
        "slots": [beta_slot]
        + ([slot("Shell", (0.3, 0.3, 0.3))] if variant == "reuse-material" else []),
    }
    gamma = {
        # The designation `Alpha Part` on purpose: removing this definition
        # must make a reference missing rather than move it onto Alpha.
        "definition_id": GAMMA,
        "designation": "Alpha Part",
        "vertices": 18,
        "slots": [slot("Gamma", (0.7, 0.2, 0.2))],
    }
    if variant == "reorder-definitions":
        definitions.extend([beta, alpha])
    else:
        definitions.append(alpha)
        definitions.append(beta)
    if variant != "remove-tracked-definition":
        definitions.append(gamma)
    definitions.extend(
        [
            {
                "definition_id": TWIN_A,
                "designation": "Twin Part",
                "vertices": 21,
                "slots": [slot("Twin", (0.8, 0.8, 0.1))],
            },
            {
                "definition_id": TWIN_B,
                "designation": "Twin Part",
                "vertices": 24,
                "slots": [slot("Twin", (0.1, 0.8, 0.8))],
            },
            # No mesh at all: a partial export that started to look complete is
            # exactly what the §22B-1c boundary exists for.
            {"definition_id": OMITTED, "designation": "Omitted Part", "vertices": 0, "slots": []},
            {"definition_id": FRAME, "designation": "Sub Frame", "vertices": 0, "slots": []},
        ]
    )

    placements = [
        {
            "occurrence_id": ROOT_OCC,
            "definition_id": ROOT,
            "designation": "Assembly Root",
            "kind": "structural",
            "parent_occurrence_id": "",
            "x": 0.0,
            "y": 0.0,
            "z": 0.0,
        }
    ]

    def place(occurrence, definition, designation, kind, x, parent=ROOT_OCC):
        placements.append(
            {
                "occurrence_id": occurrence,
                "definition_id": definition,
                "designation": designation,
                "kind": kind,
                "parent_occurrence_id": parent,
                "x": x,
                "y": 0.0,
                "z": 0.0,
            }
        )

    if variant == "inserted-definition":
        place(INSERTED_OCC, INSERTED, "Inserted Part", "mesh", 0.05)
    if variant != "remove-definition":
        place(EARLY_OCC, EARLY, "Early Part", "mesh", 0.1)
    place(ALPHA_FIRST_OCC, ALPHA, alpha_name, "mesh", 0.2)
    if variant == "insert-sibling":
        place(INSERTED_SIBLING_OCC, BETA, "Beta Part", "mesh", 0.25)
    place(ALPHA_SECOND_OCC, ALPHA, alpha_name, "mesh", 0.3)
    if variant == "reorder-siblings":
        place(GAMMA_OCC, GAMMA, "Alpha Part", "mesh", 0.6)
        place(BETA_OCC, BETA, "Beta Part", "mesh", 0.4)
    else:
        place(BETA_OCC, BETA, "Beta Part", "mesh", 0.4)
        if variant != "remove-tracked-definition":
            place(GAMMA_OCC, GAMMA, "Alpha Part", "mesh", 0.6)
    place(TWIN_A_OCC, TWIN_A, "Twin Part", "mesh", 0.7)
    place(TWIN_B_OCC, TWIN_B, "Twin Part", "mesh", 0.8)
    place(OMITTED_OCC, OMITTED, "Omitted Part", "omitted", 0.9)
    place(FRAME_OCC, FRAME, "Sub Frame", "structural", 1.0)
    if variant != "remove-sibling":
        place(NESTED_BETA_OCC, BETA, "Beta Part", "mesh", 0.05, parent=FRAME_OCC)

    return {
        "definitions": definitions,
        "placements": placements,
        "force_identifier_collision": variant == "collision",
    }


SYNTHETIC_VARIANTS = [
    ("s01-byte-identical", "base", "the same bytes at the same asset path"),
    ("s02-reexport-unchanged", "reexport", "an unchanged document exported again"),
    ("s03-display-name-only", "renamed", "one designation; every durable identity unchanged"),
    ("s04a-insert-definition", "inserted-definition", "an unrelated definition added before the tracked ones"),
    ("s04b-remove-definition", "remove-definition", "an unrelated earlier definition removed"),
    ("s04c-reorder-definitions", "reorder-definitions", "two unrelated definitions swapped in document order"),
    ("s05a-insert-sibling", "insert-sibling", "a sibling occurrence added between two tracked ones"),
    ("s05b-remove-sibling", "remove-sibling", "a sibling occurrence removed"),
    ("s05c-reorder-siblings", "reorder-siblings", "two sibling occurrences swapped"),
    ("s06-remove-tracked-definition", "remove-tracked-definition", "the tracked definition and its only placement removed"),
    ("s07-change-material", "changed-material", "one material slot changes designation and colour"),
    ("s08-reuse-material", "reuse-material", "a second definition gains a slot with an existing designation"),
]


def write_synthetic(output: Path) -> int:
    output.mkdir(parents=True, exist_ok=True)
    written = 0
    for variant in {name for _, name, _ in SYNTHETIC_VARIANTS} | {"collision"}:
        document = synthetic(variant)
        # `reexport` is the base document written differently. That is what an
        # unchanged re-export really is: the same meaning, not the same bytes.
        indent = 2 if variant == "reexport" else 1
        (output / f"{variant}.fcadsyn").write_text(
            json.dumps(document, indent=indent, sort_keys=variant != "reexport") + "\n",
            encoding="utf-8",
            newline="\n",
        )
        written += 1
    return written


def scripted_plan(documents: Path, name_of) -> dict:
    scenarios = []
    base = documents / "base.fcadsyn"
    for name, variant, change in SYNTHETIC_VARIANTS:
        path = documents / f"{variant}.fcadsyn"
        if not path.is_file():
            raise SystemExit(f"the synthetic document {variant} was not written")
        scenarios.append(
            {
                "name": name,
                "change": change,
                "before": name_of(base),
                "after": name_of(path),
                "mesh_definitions": MESHES,
                "material_bindings": MATERIALS,
                "object_bindings": [
                    f"{ALPHA}@{ALPHA_FIRST_OCC}",
                    f"{ALPHA}@{ALPHA_SECOND_OCC}",
                    f"{GAMMA}@{GAMMA_OCC}",
                    f"{TWIN_A}@{TWIN_A_OCC}",
                    f"{TWIN_B}@{TWIN_B_OCC}",
                    f"{OMITTED}@{OMITTED_OCC}",
                    f"{FRAME}@{FRAME_OCC}",
                ],
            }
        )
    # How many `Material`s the collision document would publish if every
    # identifier were distinct: one per slot. Counted from the document rather
    # than from the import, so a merge is a difference and not the baseline.
    # Materials rather than sub-assets, because a ScriptedImporter's asset also
    # publishes a Transform, a MeshFilter, a MeshRenderer and a MonoBehaviour
    # per placement, and a total over those would move for unrelated reasons.
    document = synthetic("collision")
    expected = sum(len(item["slots"]) for item in document["definitions"])
    scenarios.append(
        {
            "name": "collision",
            "change": "two durable identities given one AddObjectToAsset identifier; "
            f"distinct-identifier material count={expected}",
            "before": name_of(documents / "collision.fcadsyn"),
            "after": name_of(documents / "collision.fcadsyn"),
            "mesh_definitions": [],
            "material_bindings": [],
            "object_bindings": [],
        }
    )
    return {"scenarios": scenarios}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--staging", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--mode",
        choices=("graph", "meta", "remap", "scripted", "fbxclaim"),
        required=True,
    )
    parser.add_argument(
        "--variants",
        default=",".join(VARIANTS),
        help="the graph variants to measure, in order",
    )
    parser.add_argument(
        "--basenames",
        action="store_true",
        help="write file names rather than this machine's paths, for the committed copy",
    )
    args = parser.parse_args()

    def name_of(path: Path) -> str:
        return f"{path.parent.name}/{path.name}" if args.basenames else str(path.resolve())

    if args.mode == "graph":
        variants = [item for item in args.variants.split(",") if item]
        for variant in variants:
            if variant not in VARIANTS:
                raise SystemExit(f"unknown graph variant: {variant}")
        plan = graph_plan(args.staging, variants, name_of)
        summary = f"{len(plan['scenarios'])} scenarios over {len(variants)} graphs"
    elif args.mode == "meta":
        plan = meta_plan(args.staging, name_of)
        summary = "the .meta identity table"
    elif args.mode == "remap":
        plan = remap_plan(args.staging, name_of)
        summary = "AddRemap for Mesh, Material and GameObject"
    elif args.mode == "fbxclaim":
        plan = claim_plan(args.staging, name_of)
        summary = "whether a ScriptedImporter can own the fbx extension"
    else:
        documents = args.staging / "synthetic"
        count = write_synthetic(documents)
        plan = scripted_plan(documents, name_of)
        summary = f"{len(plan['scenarios'])} scenarios over {count} synthetic documents"

    args.output.write_text(
        json.dumps(plan, indent=1, sort_keys=True) + "\n", encoding="utf-8", newline="\n"
    )
    print(f"planned {args.mode}: {summary}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
