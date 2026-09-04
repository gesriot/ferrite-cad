#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Joins the five §22B-1e2b editor runs to the independent `ufbx` reading, and
rebuilds the decision record the measurement document is read from.

Five things happen here, and each one is a refusal rather than a note.

**The transformer is held to its claim.** It says it changes three named FBX
sections and nothing else. So every variant's geometry arrays, material colours
and existing object numbers are compared with the control's, as pinned `ufbx`
read them, and every node the control also has must come out of the import with
the same world transform. A transformer that moved a vertex, recoloured a
material, renumbered an object or displaced a part is caught here, by a program
that never saw it.

**Every report is held to itself.** §22B-1e2a's one surviving mutant emptied a
summary list while leaving the table it was summarised from intact. Every
summary in these reports is therefore rebuilt from the report's own rows and
compared, so a probe that reported a conclusion its own rows do not support is a
refusal.

**Every claim about a mechanism is held to the mechanism.** `AddRemap` is never
allowed to be described as behaviour of an FBX; a `.meta` edit is never allowed
to be described as a public API; a `ScriptedImporter` identifier is never
allowed to be built out of a designation.

**Nothing is scored on non-null.** A reference is `same_semantic` or it is not,
and an anchor a variant could not name at all is `ambiguous_join`, which is
neither kept nor broken.

**Nothing is decided.** The decision table below has a row per mechanism with
what was measured, what was not, and what it would cost. It chooses nothing.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

CONTROL = "g-flat"

# The one transition whose honest outcome is a missing reference: it removes the
# tracked definition together with its only placement, and the definition it
# removes shares its designation with one that stays.
REMOVAL_SCENARIOS = ("s06-remove-tracked-definition",)

# The one transition that changes an object's *content* on purpose. The witness
# a material is judged on is its colour, and this document moves one slot's
# colour and designation, so the honest outcome for that one anchor is "the
# same binding, different content" — kept, but recorded in its own column so it
# is never counted as a reference that survived unchanged.
CONTENT_CHANGE_SCENARIOS = ("s07-change-material",)

VERDICTS = (
    "same_semantic",
    "same_definition_other_occurrence",
    "retargeted_to_another_definition",
    "missing_though_object_still_exported",
    "missing_because_object_was_removed",
    "ambiguous_join",
)

# What a `ScriptedImporter` identifier is allowed to be built out of: the word
# `fcad`, the kind, and the durable identity. Never a designation, never a
# position, never an ordinal.
IDENTIFIER_SHAPE = re.compile(
    r"^fcad\|(root|mesh|material|object)"
    r"(\|[0-9a-f-]{36}/[a-z_.]+#\d+(\|(\d+|[0-9a-f-]{36}))?)?$"
)

TYPES = ("GameObject", "Mesh", "Material")


class Refused(Exception):
    """The recorded measurement does not join."""


class Counter:
    """How many comparisons this verification really made.

    A zero-check run is a mutant in its own right, so the number is carried out
    of here and asserted rather than assumed.
    """

    def __init__(self) -> None:
        self.value = 0

    def check(self, condition: bool, message: str) -> None:
        self.value += 1
        if not condition:
            raise Refused(message)


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


# ---------------------------------------------------------------- the oracle


def oracle_index(oracle: dict, count: Counter) -> dict[str, dict]:
    count.check(
        oracle.get("schema") == "ferritecad.fbx-graph-oracle.v1",
        "the oracle report is not the one this verifier reads",
    )
    files: dict[str, dict] = {}
    for entry in oracle["files"]:
        count.check(entry["file"] not in files, f"the oracle read {entry['file']} twice")
        files[entry["file"]] = entry
    return files


def check_transformer(files: dict[str, dict], variants: list[str], count: Counter) -> dict:
    """Every variant against the control, on the things the transformer swore
    it did not touch."""
    documents = sorted({name.split("/", 1)[1] for name in files if name.startswith(CONTROL + "/")})
    count.check(bool(documents), "the oracle read no control document")
    per_variant = []
    for variant in variants:
        moved_geometry: list[str] = []
        moved_material: list[str] = []
        lost_numbers: list[str] = []
        moved_transform: list[str] = []
        added_objects = 0
        for document in documents:
            control = files.get(f"{CONTROL}/{document}")
            candidate = files.get(f"{variant}/{document}")
            count.check(
                control is not None and candidate is not None,
                f"the oracle did not read {variant}/{document}",
            )

            control_geometry = {item["object_number"]: item for item in control["geometries"]}
            candidate_geometry = {item["object_number"]: item for item in candidate["geometries"]}
            count.check(
                set(control_geometry) == set(candidate_geometry),
                f"{variant}/{document}: the set of Geometry object numbers is not the control's",
            )
            for number, item in control_geometry.items():
                other = candidate_geometry[number]
                if item["digest"] != other["digest"]:
                    moved_geometry.append(f"{document}#{number}")
                if (item["vertices"], item["indices"], item["triangles"]) != (
                    other["vertices"],
                    other["indices"],
                    other["triangles"],
                ):
                    moved_geometry.append(f"{document}#{number}/counts")
                count.value += 1

            control_material = {item["object_number"]: item for item in control["materials"]}
            candidate_material = {item["object_number"]: item for item in candidate["materials"]}
            count.check(
                set(control_material) == set(candidate_material),
                f"{variant}/{document}: the set of Material object numbers is not the control's",
            )
            for number, item in control_material.items():
                if item["digest"] != candidate_material[number]["digest"]:
                    moved_material.append(f"{document}#{number}")
                if item["name"] != candidate_material[number]["name"]:
                    moved_material.append(f"{document}#{number}/name")
                count.value += 1

            control_nodes = {item["node_key"]: item for item in control["nodes"]}
            candidate_nodes = {item["node_key"]: item for item in candidate["nodes"]}
            for key, item in control_nodes.items():
                other = candidate_nodes.get(key)
                if other is None:
                    lost_numbers.append(f"{document}:{key}")
                    continue
                if item["object_number"] != other["object_number"]:
                    lost_numbers.append(f"{document}:{key}/number")
                if item["world_transform_digest"] != other["world_transform_digest"]:
                    moved_transform.append(f"{document}:{key}")
                count.value += 1
            added_objects += len(candidate_nodes) - len(control_nodes)

        count.check(
            not (moved_geometry or moved_material or lost_numbers or moved_transform),
            f"the structural transformer changed something it says it does not touch in "
            f"{variant}: geometry={sorted(set(moved_geometry))[:3]} "
            f"materials={sorted(set(moved_material))[:3]} "
            f"numbers={sorted(set(lost_numbers))[:3]} "
            f"transforms={sorted(set(moved_transform))[:3]}",
        )
        per_variant.append(
            {
                "variant": variant,
                "geometry_arrays_equal_the_control": True,
                "material_colours_equal_the_control": True,
                "control_object_numbers_all_present": True,
                "control_world_transforms_unchanged": True,
                "objects_the_transformer_added": added_objects,
            }
        )
    return {"variants": per_variant, "documents": len(documents)}


def check_bytes(report: dict, files: dict[str, dict], plan: dict, count: Counter) -> int:
    """The editor and the oracle opened the same bytes."""
    by_name = {}
    for scenario in plan["scenarios"]:
        for side in ("before", "after"):
            by_name[scenario["name"], side] = "/".join(Path(scenario[side]).parts[-2:])
    joined = 0
    for scenario in report["scenarios"]:
        for side in ("before", "after"):
            key = by_name.get((scenario["name"], side))
            count.check(key is not None, f"the plan does not name the {side} file of {scenario['name']}")
            entry = files.get(key)
            count.check(entry is not None, f"the oracle never read {key}")
            count.check(
                entry["fnv1a64"] == scenario[f"{side}_fnv1a64"],
                f"the editor and the oracle read different bytes for {key}: "
                f"{scenario[f'{side}_fnv1a64']} vs {entry['fnv1a64']}",
            )
            count.check(
                entry["bytes"] == scenario[f"{side}_bytes"],
                f"the editor and the oracle disagree on the size of {key}",
            )
            joined += 1
    count.check(joined > 0, "no scenario was joined to the oracle at all")
    return joined


def check_joins_against_the_file(
    report: dict, files: dict[str, dict], plan: dict, count: Counter
) -> None:
    """What the editor says a variant carries, against what the file carries.

    The probe already refuses a variant whose plan and import disagree. This is
    the third opinion: pinned `ufbx`, which read the same bytes and never saw
    the importer.
    """
    base = {
        scenario["variant"]: "/".join(Path(scenario["before"]).parts[-2:])
        for scenario in plan["scenarios"]
    }
    for variant in report["variants"]:
        entry = files[base[variant["name"]]]
        facts = entry["facts"]
        nodes_in_the_file = facts["models"]
        if variant["definition_join"] == "FerriteCADDefinitionId":
            count.check(
                facts["nodes_with_definition_id"] == nodes_in_the_file,
                f"{variant['name']}: the report joins on FerriteCADDefinitionId, and the file "
                f"carries it on {facts['nodes_with_definition_id']} of {nodes_in_the_file} nodes",
            )
            count.check(
                facts["definition_id_collisions"] == 0,
                f"{variant['name']}: the file's definition identity names two geometries",
            )
        else:
            count.check(
                facts["nodes_with_definition_id"] == 0,
                f"{variant['name']}: the report joins on the source-local key while the file "
                f"carries a source-qualified identity",
            )
        if variant["occurrence_join"] == "FerriteCADOccurrenceId":
            count.check(
                facts["nodes_with_occurrence_id"] > 0,
                f"{variant['name']}: the report calls the occurrence identity durable and the "
                f"file carries none",
            )
        # The confusion the whole slice is about has to be in the document, or
        # a scenario could pass because it was never there.
        count.check(
            facts["definition_key_collisions"] > 0,
            f"{variant['name']}: the base document no longer contains two sources sharing one "
            f"source-local key",
        )
        count.check(
            facts["placements_sharing_one_geometry"] > 0,
            f"{variant['name']}: the base document no longer shares a geometry between placements",
        )
        count.check(
            facts["structural_nodes"] > 0 and facts["omitted_nodes"] > 0,
            f"{variant['name']}: the base document lost its structural or omitted node",
        )
        count.check(
            facts["repeated_model_names"] > 0,
            f"{variant['name']}: the base document lost its repeated designations",
        )
        # What the file added, against what the import published. A variant
        # whose files carry carriers the editor then dropped is a result, and
        # the two numbers are recorded side by side rather than assumed equal.
        count.check(
            (facts["machine_named_carrier_nodes"] > 0)
            == (variant["name"] != CONTROL and variant["name"] != "g-flat-id"),
            f"{variant['name']}: the file's carrier count does not match the graph it names",
        )


# ------------------------------------------------------- the report's own rows


def rebuild_variant_summaries(report: dict, count: Counter) -> int:
    """Every summary rebuilt from the report's own node and sub-asset tables."""
    first: dict[str, dict] = {}
    for scenario in report["scenarios"]:
        first.setdefault(scenario["variant"], scenario)
    rebuilt = 0
    for variant in report["variants"]:
        scenario = first.get(variant["name"])
        count.check(scenario is not None, f"no scenario measured {variant['name']}")
        nodes = scenario["before"]["nodes"]
        subassets = scenario["before"]["subassets"]
        name = variant["name"]

        count.check(
            variant["game_objects"] == len(nodes),
            f"{name}: the GameObject count is not the node count",
        )
        count.check(
            variant["mesh_filters"] == sum(1 for node in nodes if node["has_mesh_filter"]),
            f"{name}: the MeshFilter count is not what the rows say",
        )
        count.check(
            variant["mesh_renderers"] == sum(1 for node in nodes if node["has_mesh_renderer"]),
            f"{name}: the MeshRenderer count is not what the rows say",
        )
        count.check(
            variant["meshes"] == sum(1 for item in subassets if item["unity_type"] == "Mesh"),
            f"{name}: the Mesh count is not what the sub-assets say",
        )
        count.check(
            variant["materials"] == sum(1 for item in subassets if item["unity_type"] == "Material"),
            f"{name}: the Material count is not what the sub-assets say",
        )

        occurrences = [node for node in nodes if node["graph_role"] in ("", "occurrence")]
        count.check(
            variant["occurrence_nodes"] == len(occurrences),
            f"{name}: the occurrence count is not what the rows say",
        )
        count.check(
            variant["carrier_nodes"] == len(nodes) - len(occurrences),
            f"{name}: the carrier count is not what the rows say",
        )
        carriers = [node for node in nodes if node["graph_role"] not in ("", "occurrence")]
        count.check(
            variant["carrier_renderers"]
            == sum(1 for node in carriers if node["has_mesh_renderer"]),
            f"{name}: the carrier renderer count is not what the rows say",
        )
        count.check(
            variant["carrier_material_slots"]
            == sum(len(node["material_local_file_ids"]) for node in carriers),
            f"{name}: the carrier material-slot count is not what the rows say",
        )
        count.check(
            variant["geometry_drawn_outside_any_placement"]
            == len(variant["geometry_positions_outside_any_placement"]),
            f"{name}: the count of geometry drawn outside a placement is not the list beside it",
        )
        count.check(
            len(variant["placements"])
            == sum(1 for node in occurrences if node["graph_role"] != "import_root"),
            f"{name}: there is not one placement row per occurrence",
        )

        count.check(
            variant["visible_node_names"] == sorted({node["unity_name"] for node in nodes[1:]}),
            f"{name}: the visible node names are not the ones in the rows",
        )
        count.check(
            variant["visible_mesh_names"]
            == sorted({item["unity_name"] for item in subassets if item["unity_type"] == "Mesh"}),
            f"{name}: the visible mesh names are not the ones in the rows",
        )
        count.check(
            variant["visible_material_names"]
            == sorted(
                {item["unity_name"] for item in subassets if item["unity_type"] == "Material"}
            ),
            f"{name}: the visible material names are not the ones in the rows",
        )
        count.check(
            variant["visible_nodes_named_by_machine_token"]
            == sum(1 for node in nodes[1:] if node["unity_name"].startswith("fcad~")),
            f"{name}: the machine-named node count is not what the rows say",
        )
        count.check(
            variant["subassets_named_by_machine_token"]
            == sum(1 for item in subassets if item["unity_name"].startswith("fcad~")),
            f"{name}: the machine-named sub-asset count is not what the sub-assets say",
        )

        several = 0
        shared = 0
        split = []
        for definition in sorted({node["resolved_definition"] for node in nodes}):
            placements = [
                node
                for node in nodes
                if node["resolved_definition"] == definition
                and node["graph_role"] in ("", "occurrence")
            ]
            bearers = [
                node
                for node in nodes
                if node["resolved_definition"] == definition and node["mesh_local_file_id"] != -1
            ]
            if len(placements) < 2 or not bearers:
                continue
            several += 1
            if len({node["mesh_local_file_id"] for node in bearers}) == 1:
                shared += 1
            else:
                split.append(definition)
        count.check(
            variant["definitions_with_several_placements"] == several,
            f"{name}: the count of definitions with several placements is not what the rows say",
        )
        count.check(
            variant["definitions_whose_placements_share_one_mesh"] == shared,
            f"{name}: the shared-mesh count is not what the rows say",
        )
        count.check(
            sorted(variant["definitions_with_a_split_mesh"]) == sorted(split),
            f"{name}: the split-mesh list is not what the rows say",
        )

        # An identity that names two geometries is ambiguous, and the count is
        # rebuilt here rather than believed: it is the number that decides
        # whether a variant's anchors are references at all.
        ambiguous = []
        for definition in sorted({node["resolved_definition"] for node in nodes}):
            meshes = {
                node["mesh_local_file_id"]
                for node in nodes
                if node["resolved_definition"] == definition and node["mesh_local_file_id"] != -1
            }
            if len(meshes) > 1:
                ambiguous.append(definition)
        count.check(
            variant["ambiguous_definitions"] == len(ambiguous),
            f"{name}: the ambiguous-definition count is not what the rows say",
        )
        count.check(
            sorted(variant["ambiguous_definition_names"]) == sorted(ambiguous),
            f"{name}: the ambiguous-definition list is not what the rows say",
        )
        rebuilt += 1
    return rebuilt


def check_every_variant_measured_every_scenario(report: dict, plan: dict, count: Counter) -> None:
    """A transition dropped from one variant would make its row look better
    than the others' for free."""
    planned: dict[str, set[str]] = {}
    for scenario in plan["scenarios"]:
        planned.setdefault(scenario["variant"], set()).add(scenario["name"])
    measured: dict[str, set[str]] = {}
    for scenario in report["scenarios"]:
        measured.setdefault(scenario["variant"], set()).add(scenario["name"])
    count.check(
        set(planned) == set(measured),
        f"the plan and the report measure different variants: "
        f"{sorted(set(planned) ^ set(measured))}",
    )
    sizes = {len(names) for names in measured.values()}
    count.check(
        len(sizes) == 1,
        f"the variants were measured on different numbers of transitions: {sorted(sizes)}",
    )
    for variant, names in planned.items():
        count.check(
            names == measured[variant],
            f"{variant} did not measure every planned transition: "
            f"{sorted(names ^ measured[variant])}",
        )


def check_ambiguous_anchors_are_reported_as_ambiguous(report: dict, count: Counter) -> None:
    """An anchor a variant's identity cannot name is never a kept reference."""
    ambiguous = {
        variant["name"]: set(variant["ambiguous_definition_names"])
        for variant in report["variants"]
    }
    for scenario in report["scenarios"]:
        names = ambiguous[scenario["variant"]]
        if not names:
            continue
        for reference in scenario["references"]:
            definition = reference["anchor"].split(":", 1)[1].split("@")[0]
            if definition in names or f"key:{definition.split('/')[-1]}" in names:
                count.check(
                    reference["verdict"] == "ambiguous_join",
                    f"{scenario['name']}: {reference['anchor']} names an ambiguous definition "
                    f"and is reported as {reference['verdict']}",
                )


def check_every_type_is_tracked(report: dict, count: Counter) -> None:
    """A result about a Mesh is not a result about a GameObject."""
    for scenario in report["scenarios"]:
        present = {reference["unity_type"] for reference in scenario["references"]}
        count.check(
            set(TYPES) <= present,
            f"{scenario['name']} tracked only {sorted(present)}; a slice that measures one type "
            f"has not measured the others",
        )


def check_a_rename_really_renamed(report: dict, count: Counter) -> None:
    """The rename transition has to have renamed something.

    An identifier compared only before the change would pass every rename
    scenario for free, so the two sides of the one scenario that is *about* a
    rename must actually differ in a visible name.
    """
    for scenario in report["scenarios"]:
        if not scenario["name"].endswith("s03-display-name-only"):
            continue
        before = sorted(node["unity_name"] for node in scenario["before"]["nodes"])
        after = sorted(node["unity_name"] for node in scenario["after"]["nodes"])
        count.check(
            before != after,
            f"{scenario['name']}: the two sides of the rename scenario read the same names, so "
            f"nothing was renamed and every identifier below was compared with itself",
        )


def rebuild_transitions(report: dict, count: Counter) -> list[dict]:
    rows = []
    for scenario in report["scenarios"]:
        counts: dict[str, int] = {}
        per_type: dict[str, dict[str, int]] = {}
        for reference in scenario["references"]:
            verdict = reference["verdict"]
            count.check(
                verdict in VERDICTS, f"unknown verdict {verdict} in {scenario['name']}"
            )
            counts[verdict] = counts.get(verdict, 0) + 1
            bucket = per_type.setdefault(reference["unity_type"], {})
            bucket[verdict] = bucket.get(verdict, 0) + 1
            # Every anchor is judged on meaning, so the verdict and the two
            # semantics have to agree. A row that resolved to some object and
            # called that survival is caught here.
            if verdict == "same_semantic":
                count.check(
                    reference["semantic_before"] == reference["semantic_after"],
                    f"{scenario['name']}: {reference['anchor']} is called same_semantic while "
                    f"its two semantics differ",
                )
                count.check(
                    reference["resolved_by_reloaded_asset"] != "<null>",
                    f"{scenario['name']}: {reference['anchor']} is called same_semantic while "
                    f"nothing resolved",
                )
            if verdict.startswith("missing"):
                count.check(
                    reference["resolved_by_reloaded_asset"] == "<null>",
                    f"{scenario['name']}: {reference['anchor']} is called missing while an "
                    f"object resolved",
                )
            count.check(
                reference["resolved_by_reloaded_asset"]
                == reference["resolved_by_stored_identifier"],
                f"{scenario['name']}: {reference['anchor']} resolves differently through the "
                f"reloaded asset and through GlobalObjectId",
            )
            count.check(
                reference["stored_file_id"] == reference["local_file_id_before"],
                f"{scenario['name']}: {reference['anchor']} stored a file identifier that is "
                f"not the object's own",
            )
        rows.append(
            {
                "scenario": scenario["name"],
                "variant": scenario["variant"],
                "change": scenario["change"],
                "files_are_byte_identical": scenario["files_are_byte_identical"],
                "warning_transition": scenario["warning_transition"],
                "verdicts": dict(sorted(counts.items())),
                "verdicts_by_type": {
                    kind: dict(sorted(value.items())) for kind, value in sorted(per_type.items())
                },
            }
        )
    return rows


def variant_outcome(report: dict, transitions: list[dict], count: Counter) -> list[dict]:
    control = next(
        (item for item in report["variants"] if item["name"] == CONTROL), None
    )
    count.check(control is not None, "the graph report has no control variant")
    outcome = []
    for variant in report["variants"]:
        rows = [row for row in transitions if row["variant"] == variant["name"]]
        count.check(bool(rows), f"{variant['name']} has no transition rows")
        kept = 0
        lost = 0
        ambiguous = 0
        retargeted = 0
        wrongly_missing = 0
        content_changed = 0
        by_type: dict[str, dict[str, int]] = {}
        for row in rows:
            removal = any(row["scenario"].endswith(name) for name in REMOVAL_SCENARIOS)
            content = any(row["scenario"].endswith(name) for name in CONTENT_CHANGE_SCENARIOS)
            for kind, verdicts in row["verdicts_by_type"].items():
                bucket = by_type.setdefault(
                    kind, {"kept": 0, "lost": 0, "ambiguous": 0, "content_changed": 0}
                )
                for verdict, number in verdicts.items():
                    if verdict == "ambiguous_join":
                        bucket["ambiguous"] += number
                    elif verdict == "same_semantic" or (
                        removal and verdict == "missing_because_object_was_removed"
                    ):
                        bucket["kept"] += number
                    elif (
                        content
                        and kind == "Material"
                        and verdict == "same_definition_other_occurrence"
                    ):
                        bucket["content_changed"] += number
                    else:
                        bucket["lost"] += number
            for verdict, number in row["verdicts"].items():
                if verdict == "ambiguous_join":
                    ambiguous += number
                elif verdict == "same_semantic":
                    kept += number
                elif verdict == "missing_because_object_was_removed" and removal:
                    kept += number
                elif content and verdict == "same_definition_other_occurrence":
                    content_changed += number
                elif verdict == "retargeted_to_another_definition":
                    retargeted += number
                    lost += number
                elif verdict == "missing_though_object_still_exported":
                    wrongly_missing += number
                    lost += number
                else:
                    lost += number
        outcome.append(
            {
                "variant": variant["name"],
                "anchors": kept + lost + ambiguous + content_changed,
                "references_kept": kept,
                "references_kept_with_changed_content": content_changed,
                "references_lost": lost,
                "references_ambiguous": ambiguous,
                "references_retargeted": retargeted,
                "references_missing_though_still_exported": wrongly_missing,
                "by_type": {kind: dict(sorted(value.items())) for kind, value in sorted(by_type.items())},
                "definitions_with_several_placements": variant[
                    "definitions_with_several_placements"
                ],
                "definitions_whose_placements_share_one_mesh": variant[
                    "definitions_whose_placements_share_one_mesh"
                ],
                "definitions_with_a_split_mesh": variant["definitions_with_a_split_mesh"],
                "shared_mesh_kept_for_every_definition": variant[
                    "definitions_whose_placements_share_one_mesh"
                ]
                == variant["definitions_with_several_placements"],
                "visible_nodes_named_by_machine_token": variant[
                    "visible_nodes_named_by_machine_token"
                ],
                "subassets_named_by_machine_token": variant["subassets_named_by_machine_token"],
                "human_names_only": variant["visible_nodes_named_by_machine_token"] == 0
                and variant["subassets_named_by_machine_token"] == 0,
                "ambiguous_definitions": variant["ambiguous_definitions"],
                "import_root_is_synthetic": variant["import_root_is_synthetic"],
                "extra_game_objects_vs_control": variant["game_objects"] - control["game_objects"],
                "extra_mesh_renderers_vs_control": variant["mesh_renderers"]
                - control["mesh_renderers"],
                "extra_meshes_vs_control": variant["meshes"] - control["meshes"],
                "carrier_renderers": variant["carrier_renderers"],
                "geometry_drawn_outside_any_placement": variant[
                    "geometry_drawn_outside_any_placement"
                ],
                "carrier_material_slots": variant["carrier_material_slots"],
                "triangles": variant["triangles"],
                "triangles_equal_the_control": variant["triangles"] == control["triangles"],
                "material_slots": variant["material_slots"],
                "definition_join": variant["definition_join"],
                "occurrence_join": variant["occurrence_join"],
                "warnings": variant["warnings"],
            }
        )
    return outcome


def placement_fidelity(report: dict, count: Counter) -> dict:
    """Every variant's placements against the control's, inside Unity.

    The oracle already compared the files. This compares what the *editor*
    built, because a graph can be arithmetically identical in the file and
    still arrive with a part in a different place, with a different number of
    triangles under it, or with a slot missing.
    """
    control = next((item for item in report["variants"] if item["name"] == CONTROL), None)
    count.check(control is not None, "the graph report has no control variant")
    reference = {row["node_key"]: row for row in control["placements"]}
    count.check(bool(reference), "the control published no placement rows")
    moved: list[str] = []
    absent: list[str] = []
    grew: list[str] = []
    extra_nodes: dict[str, int] = {}
    extra_renderers: dict[str, int] = {}
    for variant in report["variants"]:
        for row in variant["placements"]:
            base = reference.get(row["node_key"])
            if base is None:
                absent.append(f"{variant['name']}:{row['node_key']}")
                continue
            if (row["world_position"], row["world_rotation"], row["world_scale"]) != (
                base["world_position"],
                base["world_rotation"],
                base["world_scale"],
            ):
                moved.append(f"{variant['name']}:{row['node_key']}")
            if (row["triangles"], row["material_slots"]) != (
                base["triangles"],
                base["material_slots"],
            ):
                grew.append(f"{variant['name']}:{row['node_key']}")
            # Where the geometry under this placement is actually drawn. The
            # placement's own transform is not enough: a graph that moves the
            # geometry onto a child leaves the placement exactly where the
            # control puts it and still draws the part somewhere else. That
            # mutant survived the first edition of this harness.
            if (
                row["geometry_world_position"],
                row["geometry_world_rotation"],
                row["geometry_world_scale"],
            ) != (
                base["geometry_world_position"],
                base["geometry_world_rotation"],
                base["geometry_world_scale"],
            ):
                moved.append(f"{variant['name']}:{row['node_key']}/geometry")
            count.value += 1
        extra_nodes[variant["name"]] = sum(
            row["extra_nodes_under_this_placement"] for row in variant["placements"]
        )
        extra_renderers[variant["name"]] = sum(
            row["renderers_under_this_placement"] for row in variant["placements"]
        )
    # A graph that moved a part, lost a placement the control has, or changed
    # what is drawn under one is a wrong graph however stable its identifiers
    # are, and that is a refusal rather than a column.
    count.check(
        not moved,
        f"a graph variant placed a part somewhere the control does not: {sorted(moved)[:5]}",
    )
    count.check(
        not absent,
        f"a graph variant has a placement the control does not: {sorted(absent)[:5]}",
    )
    count.check(
        not grew,
        f"a graph variant changed what is drawn under a placement: {sorted(grew)[:5]}",
    )
    return {
        "every_placement_lands_where_the_control_puts_it": True,
        "every_placement_draws_its_geometry_where_the_control_draws_it": True,
        "extra_nodes_under_placements": dict(sorted(extra_nodes.items())),
        "renderers_under_placements": dict(sorted(extra_renderers.items())),
        "geometry_drawn_outside_any_placement": {
            variant["name"]: variant["geometry_drawn_outside_any_placement"]
            for variant in report["variants"]
        },
    }


# ------------------------------------------------------------ the other probes


def meta_summary(meta: dict, count: Counter) -> dict:
    # A `.meta` edit is never a public API. If the report says a public API
    # writes the table, it has to name one.
    count.check(
        meta["a_public_api_writes_the_table"]
        == bool(meta["public_api_members_naming_the_table"]),
        "the meta report claims a public API writes the identity table and names none",
    )
    count.check(
        meta["meta_file_exists"] and meta["importer_is_a_model_importer"],
        "the meta probe did not measure a model import at all",
    )
    count.check(
        meta["table_entries"] == len(meta["table"]),
        "the meta report's entry count is not the number of rows it recorded",
    )
    count.check(
        meta["table_entries_for_meshes"]
        == sum(1 for row in meta["table"] if row["class_id"] == 43),
        "the meta report's Mesh row count is not what its rows say",
    )
    count.check(
        meta["table_entries_for_game_objects"]
        == sum(1 for row in meta["table"] if row["class_id"] == 1),
        "the meta report's GameObject row count is not what its rows say",
    )
    count.check(
        meta["table_entries_for_materials"]
        == sum(1 for row in meta["table"] if row["class_id"] == 21),
        "the meta report's Material row count is not what its rows say",
    )
    undocumented = (
        not meta["a_public_api_writes_the_table"]
        and meta["renamed_entry_changed_a_visible_name"]
    )
    count.check(
        meta["only_working_path"]
        != "a public API writes the table"
        or meta["a_public_api_writes_the_table"],
        "the meta report calls an undocumented edit a public API",
    )
    return {
        "internal_id_to_name_table_present": meta["internal_id_to_name_table_present"],
        "table_entries": meta["table_entries"],
        "covers_game_objects": meta["table_entries_for_game_objects"] > 0,
        "covers_meshes": meta["table_entries_for_meshes"] > 0,
        "covers_materials": meta["table_entries_for_materials"] > 0,
        "class_ids": meta["table_class_ids"],
        "a_public_api_writes_the_table": meta["a_public_api_writes_the_table"],
        "public_api_members_found": meta["public_api_members_naming_the_table"],
        "external_object_api_members_found": meta["public_api_members_naming_external_objects"],
        "only_working_path": meta["only_working_path"],
        "direct_edit_changed_a_visible_name": meta["renamed_entry_changed_a_visible_name"],
        "direct_edit_changed_a_local_file_id": meta["renamed_entry_changed_a_local_file_id"],
        "an_invented_entry_created_an_object": meta["added_entry_created_an_object"],
        "what_the_table_maps": meta["what_the_table_maps"],
        "survived_a_reexport": meta["table_survived_a_reexport"],
        "file_ids_unchanged_after_reexport": meta["file_ids_unchanged_after_reexport"],
        "survived_a_real_change": meta["table_survived_a_real_change"],
        "file_ids_unchanged_after_a_real_change": meta["file_ids_unchanged_after_a_real_change"],
        "rebuilt_after_deleting_the_meta": meta["table_rebuilt_after_deleting_the_meta"],
        "file_ids_unchanged_after_deleting_the_meta": meta[
            "file_ids_unchanged_after_deleting_the_meta"
        ],
        "a_sidecar_written_first_was_honoured": meta[
            "sidecar_written_before_the_first_import_was_honoured"
        ],
        # The stop condition §22B-1e2b names by hand: if the only path that
        # works is editing undocumented serialized metadata, that is reported
        # as a stop condition and never as a supported Unity API.
        "undocumented_editing_is_the_only_working_path": undocumented,
    }


def remap_summary(remap: dict, count: Counter) -> dict:
    # Never a property of the FBX. This is the mutation the brief names by
    # hand, and it is refused rather than footnoted.
    count.check(
        remap["is_a_property_of_the_fbx"] is False,
        "the remap report describes AddRemap as behaviour of the exported FBX",
    )
    count.check(
        {item["unity_type"] for item in remap["types"]} == set(TYPES),
        "the remap probe did not measure all three types",
    )
    count.check(
        remap["external_assets_the_project_gained"]
        == sum(item["external_assets_required"] for item in remap["types"]),
        "the remap report's external-asset count is not what its rows say",
    )
    count.check(
        remap["keys_that_name_more_than_one_object"] == len(remap["ambiguous_keys"]),
        "the remap report's ambiguous-key count is not what its rows say",
    )
    # The stale-content question is only asked if the probe remapped an object
    # the transitions really change. A probe that picked an object the
    # documents leave alone would report "nothing went stale" about nothing.
    count.check(
        any(item["the_fbx_changed_this_object"] for item in remap["types"]),
        "the remap probe never remapped an object the measured transitions change, so its "
        "stale-content result is about nothing",
    )
    rows = []
    for item in remap["types"]:
        count.check(
            item["human_names_kept"]
            == all(
                not name.startswith("fcad~") for name in item["visible_names_after"]
            ),
            f"the remap report's human-name verdict for {item['unity_type']} is not what its "
            f"names say",
        )
        count.check(
            item["one_shared_mesh_kept"]
            == (
                item["placements_sharing_one_mesh_after"]
                == item["placements_sharing_one_mesh_before"]
            ),
            f"the remap report's shared-mesh verdict for {item['unity_type']} is not what its "
            f"counts say",
        )
        # An external copy the import really uses, holding content the file no
        # longer has, is stale content shown to a person. Calling it anything
        # else is the mutation the brief names by hand.
        count.check(
            item["the_scene_shows_content_the_fbx_no_longer_has"]
            == (
                item["the_import_honoured_it"]
                and item["the_fbx_changed_this_object"]
                and item["external_content_unchanged_after_the_fbx_changed"]
            ),
            f"the remap report's stale-content verdict for {item['unity_type']} is not what its "
            f"own three measurements say",
        )
        rows.append(
            {
                "unity_type": item["unity_type"],
                "key_shape": item["key_shape"],
                "add_remap_threw": item["add_remap_threw"],
                "add_remap_error": item["add_remap_error"],
                "supported": item["the_import_honoured_it"],
                "accepted_but_ignored": item["appears_in_the_external_object_map"]
                and not item["the_import_honoured_it"],
                "what_the_scene_points_at_now": item["what_the_scene_points_at_now"],
                "external_assets_required": item["external_assets_required"],
                "human_names_kept": item["human_names_kept"],
                "one_shared_mesh_kept": item["one_shared_mesh_kept"],
                "stored_reference_verdict": item["stored_reference_verdict"],
                "silently_retargeted": item["silently_retargeted"],
                "survived_a_reexport": item["mapping_survived_a_reexport"],
                "survived_a_designation_rename": item["mapping_survived_a_designation_rename"],
                "map_key_after_the_rename": item["map_key_after_the_rename"],
                "survived_removing_the_definition": item["mapping_survived_removing_the_definition"],
                "left_a_dangling_entry_after_removal": item[
                    "remap_left_a_dangling_entry_after_removal"
                ],
                "the_fbx_changed_this_object": item["the_fbx_changed_this_object"],
                "external_content_unchanged_after_the_fbx_changed": item[
                    "external_content_unchanged_after_the_fbx_changed"
                ],
                "the_scene_shows_content_the_fbx_no_longer_has": item[
                    "the_scene_shows_content_the_fbx_no_longer_has"
                ],
                "warnings": item["warnings"],
            }
        )
    return {
        "is_a_property_of_the_fbx": remap["is_a_property_of_the_fbx"],
        "what_it_is": remap["what_it_is"],
        "external_assets_the_project_gained": remap["external_assets_the_project_gained"],
        "keys_that_name_more_than_one_object": remap["keys_that_name_more_than_one_object"],
        "ambiguous_keys": remap["ambiguous_keys"],
        "types": rows,
    }


def scripted_summary(scripted: dict, claim: dict, count: Counter) -> dict:
    count.check(
        scripted["is_a_property_of_the_fbx"] is False,
        "the scripted report describes a ScriptedImporter as behaviour of the exported FBX",
    )
    count.check(
        claim["the_model_importer_still_owns_fbx"]
        == (claim["importer_the_fbx_actually_got"] == "ModelImporter"),
        "the fbx-claim report's ownership verdict is not the importer it recorded",
    )
    count.check(
        claim["a_scripted_importer_claiming_fbx_compiles"],
        "the fbx-claim run compiled no importer claiming fbx, so it measured nothing",
    )
    count.check(
        scripted["the_importer_reads_fbx"] is False,
        "the scripted report claims the probe importer reads FBX, which it does not",
    )
    designations = set(scripted["visible_node_names"]) | set(scripted["visible_mesh_names"]) | set(
        scripted["visible_material_names"]
    )
    count.check(bool(designations), "the scripted probe recorded no visible names at all")
    for row in scripted["identifiers"]:
        # An identifier built out of a designation would move when the
        # designation moved, which is the defect this whole slice is about.
        count.check(
            IDENTIFIER_SHAPE.match(row["identifier"]) is not None,
            f"a ScriptedImporter identifier is not built from the durable identity: "
            f"{row['identifier']}",
        )
        count.check(
            not any(
                designation and designation in row["identifier"] for designation in designations
            ),
            f"a ScriptedImporter identifier contains a designation: {row['identifier']}",
        )
        count.check(
            not row["visible_name_is_a_machine_token"],
            f"a ScriptedImporter object is named after its identifier: {row['visible_name']}",
        )
    count.check(
        set(TYPES) <= {row["unity_type"] for row in scripted["identifiers"]},
        "the scripted probe did not record identifiers for all three types",
    )
    # One identifier per published Mesh, and one per published Material. A
    # shared-mesh claim rests on there being one `Mesh` per definition, and
    # this is where a claim of more or fewer is caught.
    count.check(
        sum(1 for row in scripted["identifiers"] if row["unity_type"] == "Mesh")
        == scripted["meshes"],
        "the scripted report's Mesh identifiers do not account for the Meshes it published",
    )
    count.check(
        sum(1 for row in scripted["identifiers"] if row["unity_type"] == "Material")
        == scripted["materials"],
        "the scripted report's Material identifiers do not account for the Materials it published",
    )
    count.check(
        len({row["local_file_id"] for row in scripted["identifiers"]})
        == len(scripted["identifiers"]),
        "two objects the scripted importer published share one local file identifier",
    )
    # A collision that published fewer objects than distinct identifiers would
    # have is a merge, and accepting it silently is the mutation the brief names.
    count.check(
        scripted["collision_merged_two_objects_into_one"]
        == (
            scripted["collision_materials_published"] < scripted["collision_materials_expected"]
        ),
        "the scripted report's collision verdict is not what its counts say",
    )
    count.check(
        {row["unity_type"] for row in scripted["rename"]} == set(TYPES),
        "the scripted report did not measure the designation change for all three types",
    )
    for row in scripted["rename"]:
        count.check(
            row["identifiers_compared"] > 0,
            f"the scripted rename measurement compared no {row['unity_type']} identifiers",
        )
        count.check(
            row["the_identifier_alone_decides_the_local_file_id"]
            == (row["local_file_ids_that_moved"] == 0),
            f"the scripted rename verdict for {row['unity_type']} is not what its counts say",
        )

    kept = 0
    lost = 0
    content_changed = 0
    per_type: dict[str, dict[str, int]] = {}
    transitions = []
    for scenario in scripted["scenarios"]:
        counts: dict[str, int] = {}
        removal = scenario["name"] in REMOVAL_SCENARIOS
        content = scenario["name"] in CONTENT_CHANGE_SCENARIOS
        present = {reference["unity_type"] for reference in scenario["references"]}
        count.check(
            set(TYPES) <= present,
            f"the scripted scenario {scenario['name']} tracked only {sorted(present)}",
        )
        for reference in scenario["references"]:
            verdict = reference["verdict"]
            count.check(verdict in VERDICTS, f"unknown scripted verdict {verdict}")
            if verdict == "same_semantic":
                count.check(
                    reference["semantic_before"] == reference["semantic_after"],
                    f"the scripted scenario {scenario['name']} calls {reference['anchor']} "
                    f"same_semantic while its two semantics differ",
                )
            count.check(
                reference["resolved_by_reloaded_asset"]
                == reference["resolved_by_stored_identifier"],
                f"the scripted scenario {scenario['name']} resolves {reference['anchor']} "
                f"differently through the two paths",
            )
            counts[verdict] = counts.get(verdict, 0) + 1
            bucket = per_type.setdefault(reference["unity_type"], {})
            bucket[verdict] = bucket.get(verdict, 0) + 1
            if verdict == "same_semantic" or (
                removal and verdict == "missing_because_object_was_removed"
            ):
                kept += 1
            elif (
                content
                and reference["unity_type"] == "Material"
                and verdict == "same_definition_other_occurrence"
            ):
                content_changed += 1
            else:
                lost += 1
        transitions.append(
            {
                "scenario": scenario["name"],
                "change": scenario["change"],
                "warning_transition": scenario["warning_transition"],
                "verdicts": dict(sorted(counts.items())),
            }
        )
    return {
        "is_a_property_of_the_fbx": scripted["is_a_property_of_the_fbx"],
        "the_importer_reads_fbx": scripted["the_importer_reads_fbx"],
        "extension_it_owns": scripted["extension_it_owns"],
        "what_it_would_take_in_the_product": scripted["what_it_would_take_in_the_product"],
        "a_scripted_importer_can_own_fbx": not claim["the_model_importer_still_owns_fbx"],
        "the_model_importer_kept_fbx": claim["the_model_importer_still_owns_fbx"],
        "the_claiming_importer_compiled": claim["a_scripted_importer_claiming_fbx_compiles"],
        "the_claiming_importer_ran": claim["the_scripted_importer_ran"],
        "importer_the_fbx_actually_got": claim["importer_the_fbx_actually_got"],
        "claim_conclusion": claim["conclusion"],
        "game_objects": scripted["game_objects"],
        "meshes": scripted["meshes"],
        "materials": scripted["materials"],
        "identifier_rows": len(scripted["identifiers"]),
        "identifier_rows_by_type": {
            kind: sum(1 for row in scripted["identifiers"] if row["unity_type"] == kind)
            for kind in sorted({row["unity_type"] for row in scripted["identifiers"]})
        },
        "visible_names_carrying_a_machine_token": scripted[
            "visible_names_carrying_a_machine_token"
        ],
        "definitions_with_several_placements": scripted["definitions_with_several_placements"],
        "definitions_whose_placements_share_one_mesh": scripted[
            "definitions_whose_placements_share_one_mesh"
        ],
        "shared_mesh_kept_for_every_definition": scripted[
            "definitions_whose_placements_share_one_mesh"
        ]
        == scripted["definitions_with_several_placements"],
        "ambiguous_definitions": scripted["ambiguous_definitions"],
        "references_kept": kept,
        "references_kept_with_changed_content": content_changed,
        "references_lost": lost,
        "rename": scripted["rename"],
        "types_whose_local_file_id_is_the_identifier": [
            row["unity_type"]
            for row in scripted["rename"]
            if row["the_identifier_alone_decides_the_local_file_id"]
        ],
        "types_whose_local_file_id_moved_with_the_designation": [
            row["unity_type"]
            for row in scripted["rename"]
            if not row["the_identifier_alone_decides_the_local_file_id"]
        ],
        "verdicts_by_type": {
            kind: dict(sorted(value.items())) for kind, value in sorted(per_type.items())
        },
        "collision_materials_expected": scripted["collision_materials_expected"],
        "collision_materials_published": scripted["collision_materials_published"],
        "collision_merged_two_objects_into_one": scripted["collision_merged_two_objects_into_one"],
        "collision_was_refused": scripted["collision_was_refused"],
        "collision_messages": scripted["collision_messages"],
        "transitions": transitions,
    }


# ------------------------------------------------------------ the decision table


def decision_table(graph: dict, meta: dict, remap: dict, scripted: dict) -> list[dict]:
    """Six rows, one per mechanism, each saying what was measured and what was
    not. It chooses nothing: §22B-1e2b is a decision handoff."""
    control = next(row for row in graph["variants"] if row["variant"] == CONTROL)
    others = [row for row in graph["variants"] if row["variant"] != CONTROL]
    best = max(
        others,
        key=lambda row: (
            row["shared_mesh_kept_for_every_definition"],
            row["human_names_only"],
            row["references_kept"],
            -row["extra_game_objects_vs_control"],
        ),
        default=None,
    )
    remap_supported = [row["unity_type"] for row in remap["types"] if row["supported"]]
    return [
        {
            "mechanism": "1. pure FBX graph",
            "measured": "five graphs on the production writer's bytes, twelve transitions each, "
            "with the source-qualified identity carried as invisible custom properties",
            "not_measured": "graphs this slice did not write, binary FBX, and any importer "
            "setting other than the hierarchy sort",
            "stable_game_object_references": bool(
                best and best["by_type"].get("GameObject", {}).get("lost", 1) == 0
            ),
            "stable_mesh_references": bool(
                best and best["by_type"].get("Mesh", {}).get("lost", 1) == 0
            ),
            "stable_material_references": bool(
                best and best["by_type"].get("Material", {}).get("lost", 1) == 0
            ),
            "human_names": bool(best and best["human_names_only"]),
            "shared_mesh": bool(best and best["shared_mesh_kept_for_every_definition"]),
            "multi_source_identity": bool(best and best["ambiguous_definitions"] == 0),
            "needs_a_persistent_occurrence_id": True,
            "needs_a_companion_package": False,
            "needs_external_assets": False,
            "public_api": "the FBX format itself",
            "user_workflow": "none: the file is imported the way it is imported today",
            "production_cost": "a writer change plus a persisted occurrence identity",
            "best_measured_variant": best["variant"] if best else None,
            "extra_game_objects_it_costs": best["extra_game_objects_vs_control"] if best else None,
            "extra_renderers_it_costs": best["extra_mesh_renderers_vs_control"] if best else None,
        },
        {
            "mechanism": "2. .meta identity table",
            "measured": "what 6000.4.10f1 writes into a model's .fbx.meta, what public API "
            "names it, what a direct edit does, and whether it survives a re-export, a real "
            "change, deleting the .meta and a sidecar written before the first import",
            "not_measured": "editor versions other than 6000.4.10f1, and any table Unity writes "
            "for asset types this slice does not import",
            "stable_game_object_references": meta["covers_game_objects"],
            "stable_mesh_references": meta["covers_meshes"],
            "stable_material_references": meta["covers_materials"],
            "human_names": True,
            "shared_mesh": None,
            "multi_source_identity": False,
            "needs_a_persistent_occurrence_id": True,
            "needs_a_companion_package": False,
            "needs_external_assets": False,
            "public_api": ", ".join(meta["public_api_members_found"])
            if meta["a_public_api_writes_the_table"]
            else "none found",
            "user_workflow": "the sidecar has to travel with the file and stay in version control",
            "production_cost": "writing undocumented serialized metadata by hand"
            if meta["undocumented_editing_is_the_only_working_path"]
            else "no measured path writes it",
        },
        {
            "mechanism": "3. AddRemap plus external assets",
            "measured": "SourceAssetIdentifier and AddRemap for Mesh, Material and GameObject "
            "separately, with a stored reference across each transition",
            "not_measured": "types other than those three, and remapping to assets a project "
            "already owns rather than to copies made here",
            "stable_game_object_references": "GameObject" in remap_supported,
            "stable_mesh_references": "Mesh" in remap_supported,
            "stable_material_references": "Material" in remap_supported,
            "human_names": all(row["human_names_kept"] for row in remap["types"]),
            "shared_mesh": all(row["one_shared_mesh_kept"] for row in remap["types"]),
            "multi_source_identity": remap["keys_that_name_more_than_one_object"] == 0,
            "needs_a_persistent_occurrence_id": True,
            "needs_a_companion_package": False,
            "needs_external_assets": True,
            "public_api": "AssetImporter.AddRemap, public and documented",
            "user_workflow": "one external asset per remapped object, kept in the project and "
            "re-pointed by hand whenever a designation changes",
            "production_cost": "not a writer change at all: a project-side convention plus a "
            "tool that maintains the map",
            "types_the_import_honoured": remap_supported,
        },
        {
            "mechanism": "4. ScriptedImporter / custom extension",
            "measured": "AddObjectToAsset identifiers derived from the full durable identity, "
            "for GameObject, Mesh and Material, over the same transitions, plus a deliberate "
            "identifier collision and whether a ScriptedImporter can own .fbx",
            "not_measured": "reading real FBX bytes from a ScriptedImporter, packaging, and "
            "anything about how such a package would be distributed or versioned",
            # Per type, from the transition that isolates the question: does a
            # designation change move the local file identifier of an object
            # whose `AddObjectToAsset` identifier did not change.
            "stable_game_object_references": "GameObject"
            in scripted["types_whose_local_file_id_is_the_identifier"],
            "stable_mesh_references": "Mesh"
            in scripted["types_whose_local_file_id_is_the_identifier"],
            "stable_material_references": "Material"
            in scripted["types_whose_local_file_id_is_the_identifier"],
            "human_names": scripted["visible_names_carrying_a_machine_token"] == 0,
            "shared_mesh": scripted["shared_mesh_kept_for_every_definition"],
            "multi_source_identity": scripted["ambiguous_definitions"] == 0,
            "needs_a_persistent_occurrence_id": True,
            "needs_a_companion_package": True,
            "needs_external_assets": False,
            "public_api": "UnityEditor.AssetImporters.ScriptedImporter and "
            "AssetImportContext.AddObjectToAsset, public and documented",
            "user_workflow": "install a FerriteCAD Unity package, and export the extension it "
            "owns rather than .fbx"
            if not scripted["a_scripted_importer_can_own_fbx"]
            else "install a FerriteCAD Unity package",
            "production_cost": "a Unity package, its own importer, and its own file format read "
            "on the Unity side",
        },
        {
            "mechanism": "5. machine-visible names as a fallback",
            "measured": "by §22B-1e2a, on the flat graph: references kept except one shared "
            "Mesh under an earlier-sibling insert, and every name a person reads becomes a token",
            "not_measured": "nothing new here; this row exists so the fallback sits in the same "
            "table as the mechanisms it competes with",
            "stable_game_object_references": True,
            "stable_mesh_references": False,
            "stable_material_references": True,
            "human_names": False,
            "shared_mesh": True,
            "multi_source_identity": True,
            "needs_a_persistent_occurrence_id": True,
            "needs_a_companion_package": False,
            "needs_external_assets": False,
            "public_api": "the FBX format itself",
            "user_workflow": "the hierarchy, the mesh list and the material list read as tokens",
            "production_cost": "a writer change plus a persisted occurrence identity",
        },
        {
            "mechanism": "6. human names without a durable-reference guarantee",
            "measured": "the control, here and in §22B-1e1: the references that break are named "
            "and counted rather than described as a risk",
            "not_measured": "nothing new here; it is the product as it ships",
            "stable_game_object_references": control["by_type"]
            .get("GameObject", {})
            .get("lost", 1)
            == 0,
            "stable_mesh_references": control["by_type"].get("Mesh", {}).get("lost", 1) == 0,
            "stable_material_references": control["by_type"].get("Material", {}).get("lost", 1)
            == 0,
            "human_names": control["human_names_only"],
            "shared_mesh": control["shared_mesh_kept_for_every_definition"],
            "multi_source_identity": control["ambiguous_definitions"] == 0,
            "needs_a_persistent_occurrence_id": False,
            "needs_a_companion_package": False,
            "needs_external_assets": False,
            "public_api": "the FBX format itself",
            "user_workflow": "unchanged",
            "production_cost": "none",
        },
    ]


def stop_conditions(graph: dict, meta: dict, remap: dict, scripted: dict) -> dict:
    """The stop-and-report conditions, each stated as what was measured.

    Written so that "false" never reads as "this mechanism is fine". A
    mechanism appears in a list only when the measurement put it there, and the
    contract is checked clause by clause rather than as one word.
    """
    # The whole contract, per FBX graph: every reference kept, nothing
    # ambiguous, one shared Mesh, only designations visible, and no object the
    # control does not have.
    keeps_every_reference = [
        row["variant"]
        for row in graph["variants"]
        if row["references_lost"] == 0 and row["references_ambiguous"] == 0
    ]
    keeps_shared_mesh_and_human_names = [
        row["variant"]
        for row in graph["variants"]
        if row["shared_mesh_kept_for_every_definition"] and row["human_names_only"]
    ]
    costs_nothing_visible = [
        row["variant"]
        for row in graph["variants"]
        if row["extra_game_objects_vs_control"] == 0
        and row["extra_mesh_renderers_vs_control"] == 0
        and not row["import_root_is_synthetic"]
    ]
    solved = [
        row["variant"]
        for row in graph["variants"]
        if row["variant"] != CONTROL
        and row["variant"] in keeps_every_reference
        and row["variant"] in keeps_shared_mesh_and_human_names
        and row["variant"] in costs_nothing_visible
        and row["ambiguous_definitions"] == 0
    ]

    # The ScriptedImporter meets the contract only if all three types keep
    # their identity when a designation moves. Two out of three is not two
    # thirds of a solution.
    scripted_types = set(scripted["types_whose_local_file_id_is_the_identifier"])
    scripted_meets_the_contract = (
        scripted_types == set(TYPES)
        and scripted["references_lost"] == 0
        and scripted["shared_mesh_kept_for_every_definition"]
        and scripted["visible_names_carrying_a_machine_token"] == 0
        and scripted["ambiguous_definitions"] == 0
    )
    remap_honoured = [row["unity_type"] for row in remap["types"] if row["supported"]]

    return {
        # ---- the graphs
        "a_pure_fbx_graph_solves_it": bool(solved),
        "graphs_that_solve_it": solved,
        "graphs_that_keep_every_reference": keeps_every_reference,
        "graphs_that_keep_shared_mesh_and_human_names": keeps_shared_mesh_and_human_names,
        "graphs_that_add_no_visible_object": costs_nothing_visible,
        # ---- the other three mechanisms
        "a_scripted_importer_meets_the_whole_contract": scripted_meets_the_contract,
        "types_whose_scripted_identifier_decides_the_local_file_id": sorted(scripted_types),
        "types_whose_scripted_local_file_id_moved_with_the_designation": scripted[
            "types_whose_local_file_id_moved_with_the_designation"
        ],
        "a_companion_package_would_be_required_for_the_scripted_mechanism": True,
        "the_scripted_mechanism_needs_its_own_extension": not scripted[
            "a_scripted_importer_can_own_fbx"
        ],
        "types_add_remap_actually_replaced": remap_honoured,
        "add_remap_requires_external_assets": bool(remap_honoured),
        "undocumented_meta_editing_is_the_only_working_path_for_that_mechanism": meta[
            "undocumented_editing_is_the_only_working_path"
        ],
        "a_public_api_writes_the_meta_identity_table": meta["a_public_api_writes_the_table"],
        # ---- the two the brief asks about by name
        "a_new_persisted_occurrence_id_is_still_required": True,
        "no_measured_mechanism_keeps_shared_mesh_human_names_and_every_reference": not [
            row["variant"]
            for row in graph["variants"]
            if row["variant"] in keeps_every_reference
            and row["variant"] in keeps_shared_mesh_and_human_names
        ]
        and not scripted_meets_the_contract,
        "no_measured_mechanism_meets_the_whole_contract": not solved
        and not scripted_meets_the_contract,
        # Two clean projects producing different canonical reports is a runner
        # refusal, so reaching this line at all means they agreed.
        "two_clean_projects_diverged": False,
    }


# ------------------------------------------------------------------ the join


def verify_graph_only(graph_report: dict, oracle: dict, plan: dict) -> tuple[int, dict]:
    """The half of the join that needs only the graph run.

    Split out so a graph-only measurement is still held to the structural
    claims: the transformer's, the report's own rows', and the placements'. A
    mutation campaign that could only run the whole five-mode measurement would
    be too slow to run at all, and a structural defect that no gate looks at
    between full runs is a structural defect nobody notices.
    """
    count = Counter()
    count.check(
        graph_report.get("schema") == "ferritecad.unity-graph-identity.v1",
        f"the graph report is not the one this verifier reads: {graph_report.get('schema')}",
    )
    count.check(graph_report.get("checks", 0) > 0, "the graph report records no checks at all")
    files = oracle_index(oracle, count)
    variants = [item["name"] for item in graph_report["variants"]]
    count.check(CONTROL in variants, "the graph report does not measure the control")
    transformer = check_transformer(files, variants, count)
    joined = check_bytes(graph_report, files, plan, count)
    check_joins_against_the_file(graph_report, files, plan, count)
    rebuilt = rebuild_variant_summaries(graph_report, count)
    check_every_variant_measured_every_scenario(graph_report, plan, count)
    check_ambiguous_anchors_are_reported_as_ambiguous(graph_report, count)
    check_every_type_is_tracked(graph_report, count)
    check_a_rename_really_renamed(graph_report, count)
    transitions = rebuild_transitions(graph_report, count)
    outcome = variant_outcome(graph_report, transitions, count)
    fidelity = placement_fidelity(graph_report, count)
    return count.value, {
        "joined_scenario_files": joined,
        "rebuilt_variant_summaries": rebuilt,
        "transformer_fidelity": transformer,
        "placement_fidelity": fidelity,
        "graph": {"variants": outcome},
        "graph_transitions": transitions,
    }


def verify(
    graph_report: dict,
    meta_report: dict,
    remap_report: dict,
    scripted_report: dict,
    claim_report: dict,
    oracle: dict,
    plan: dict,
) -> tuple[int, dict]:
    count = Counter()
    for report, schema in (
        (graph_report, "ferritecad.unity-graph-identity.v1"),
        (meta_report, "ferritecad.unity-meta-identity.v1"),
        (remap_report, "ferritecad.unity-remap-identity.v1"),
        (scripted_report, "ferritecad.unity-scripted-identity.v1"),
        (claim_report, "ferritecad.unity-fbx-claim.v1"),
    ):
        count.check(
            report.get("schema") == schema,
            f"a report is not the one this verifier reads: {report.get('schema')}",
        )
        count.check(
            report.get("checks", 0) > 0,
            f"the {report.get('mode')} report records no checks at all",
        )

    structural, half = verify_graph_only(graph_report, oracle, plan)
    count.value += structural
    graph_summary = half["graph"]
    transitions = half["graph_transitions"]
    transformer = half["transformer_fidelity"]
    fidelity = half["placement_fidelity"]
    joined = half["joined_scenario_files"]
    rebuilt = half["rebuilt_variant_summaries"]
    meta = meta_summary(meta_report, count)
    remap = remap_summary(remap_report, count)
    scripted = scripted_summary(scripted_report, claim_report, count)

    record = {
        "schema": "ferritecad.unity-graph-decision.v1",
        "unity_version": graph_report["unity_version"],
        "joined_scenario_files": joined,
        "rebuilt_variant_summaries": rebuilt,
        "transformer_fidelity": transformer,
        "placement_fidelity": fidelity,
        "graph": graph_summary,
        "graph_transitions": transitions,
        "meta": meta,
        "remap": remap,
        "scripted": scripted,
        "decision_table": decision_table(graph_summary, meta, remap, scripted),
        "stop_conditions": stop_conditions(graph_summary, meta, remap, scripted),
    }
    return count.value, record


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--graph", type=Path, required=True)
    parser.add_argument("--meta", type=Path)
    parser.add_argument("--remap", type=Path)
    parser.add_argument("--scripted", type=Path)
    parser.add_argument("--claim", type=Path)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--graph-plan", type=Path, required=True)
    parser.add_argument("--emit", type=Path)
    parser.add_argument("--expected", type=Path)
    parser.add_argument(
        "--structural-only",
        action="store_true",
        help="hold the graph run to the structural claims without the other four modes",
    )
    args = parser.parse_args()

    if args.structural_only:
        checks, half = verify_graph_only(
            load(args.graph), load(args.oracle), load(args.graph_plan)
        )
        if args.emit is not None:
            args.emit.write_text(
                json.dumps(half, indent=1, sort_keys=True) + "\n", encoding="utf-8", newline="\n"
            )
        print(
            f"graph structure: {checks} checks, "
            f"{half['joined_scenario_files']} scenario files re-joined, "
            f"{half['rebuilt_variant_summaries']} variant summaries rebuilt"
        )
        return 0

    for name in ("meta", "remap", "scripted", "claim", "emit"):
        if getattr(args, name) is None:
            raise SystemExit(f"--{name} is required without --structural-only")

    checks, record = verify(
        load(args.graph),
        load(args.meta),
        load(args.remap),
        load(args.scripted),
        load(args.claim),
        load(args.oracle),
        load(args.graph_plan),
    )
    text = json.dumps(record, indent=1, sort_keys=True) + "\n"
    args.emit.write_text(text, encoding="utf-8", newline="\n")
    if args.expected is not None and args.expected.read_text(encoding="utf-8") != text:
        raise Refused("the decision record differs from the committed measurement")
    print(
        f"graph record: {checks} checks, "
        f"{record['joined_scenario_files']} scenario files re-joined, "
        f"{record['rebuilt_variant_summaries']} variant summaries rebuilt"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Refused as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        raise SystemExit(1) from refusal
