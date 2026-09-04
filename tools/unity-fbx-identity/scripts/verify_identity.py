#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Joins the Unity identity report to the independent ufbx oracle.

Nothing here decides what Unity should do. It refuses the ways a measurement
of this shape can be wrong without looking wrong:

  * the editor and the independent reader looking at different bytes;
  * a mandatory scenario quietly missing;
  * a base document that does not actually contain the confusions the brief
    lists, so a scenario passes because it never happened;
  * a reference judged on being non-null;
  * a Geometry result presented as a Model or Material result;
  * the Unity-side join between the custom-property callback and the finished
    hierarchy being wrong, which is checked against the geometry sharing `ufbx`
    read for the same durable keys;
  * a sub-asset identifier that is no longer the pair a project file stores,
    which would make the recorded tables an incomplete record;
  * the probe's one importer setting having moved anything.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

# Every scenario the §22B-1e1 brief requires, and the brief's number for it.
COMPLEX = {
    "s11a-complex-byte-identical": 11,
    "s11b-complex-reexported": 11,
}

MANDATORY = {
    "s01-byte-identical-reimport": 1,
    "s02-reexport-unchanged-document": 2,
    "s03-display-name-only": 3,
    "s04a-insert-earlier-definition": 4,
    "s04b-remove-earlier-definition": 4,
    "s05-reorder-definitions": 5,
    "s06a-insert-sibling": 6,
    "s06b-remove-sibling": 6,
    "s06c-reorder-siblings": 6,
    "s12-remove-one-definition": 12,
}

VERDICTS = {
    "same_semantic",
    "same_definition_other_occurrence",
    "retargeted_to_another_definition",
    "missing_though_object_still_exported",
    "missing_because_object_was_removed",
}

KINDS = {"Mesh", "Material", "GameObject"}


class Refused(Exception):
    pass


def fbx_object_number(read: dict, anchor: str) -> int:
    """The raw FBX object number the writer gave the anchored object.

    Read from the file by `ufbx`, never from Unity: the whole point of the
    oracle is that the editor's answer about which object this is cannot be
    used to check the editor.
    """
    kind, rest = anchor.split(":", 1)
    if kind == "mesh":
        key = rest.split("=", 1)[1]
        for node in read["nodes"]:
            if node["definition_key"] == key and node["geometry_object_number"] != 0:
                return node["geometry_object_number"]
        return 0
    key, index = rest.rsplit("@", 1)
    index = int(index)
    matches = [node for node in read["nodes"] if node["definition_key"] == key]
    if kind == "material":
        if not matches or index >= len(matches[0]["materials"]):
            return 0
        return matches[0]["materials"][index]["object_number"]
    if index >= len(matches):
        return 0
    return matches[index]["object_number"]


def transitions(unity: dict, oracle: dict, plan: dict) -> dict:
    """The joined table: what Unity did, beside what the file actually says."""
    files = {item["file"]: item for item in oracle["files"]}
    planned = {item["name"]: item for item in plan["scenarios"]}
    rows = []
    for scenario in unity["scenarios"]:
        before = files[Path(planned[scenario["name"]]["before"]).name]
        after = files[Path(planned[scenario["name"]]["after"]).name]
        for reference in scenario["references"]:
            first = fbx_object_number(before, reference["anchor"])
            second = fbx_object_number(after, reference["anchor"])
            rows.append(
                {
                    "scenario": scenario["name"],
                    "change": scenario["change"],
                    "unity_type": reference["unity_type"],
                    "anchor": reference["anchor"],
                    "verdict": reference["verdict"],
                    "warning_transition": scenario["warning_transition"],
                    "display_name_before": reference["name_before"],
                    "display_name_after": reference["name_after"],
                    "display_name_changed": reference["display_name_changed"],
                    "unity_local_file_id_before": reference["local_file_id_before"],
                    "unity_local_file_id_after": reference["local_file_id_after"],
                    "unity_local_file_id_changed": reference["local_file_id_changed"],
                    "fbx_object_number_before": first,
                    "fbx_object_number_after": second,
                    "fbx_object_number_changed": first != second,
                    "ferritecad_node_key_before": reference["node_key_before"],
                    "ferritecad_node_key_after": reference["node_key_after"],
                    "ferritecad_node_key_changed": reference["node_key_changed"],
                    "semantic_object_still_exported": reference["semantic_object_present_after"],
                }
            )
    return {"schema": "ferritecad.unity-fbx-identity-transitions.v1", "transitions": rows}


def verify(unity: dict, oracle: dict, plan: dict, mode: str = "synthetic") -> int:
    checks = 0

    def require(condition: bool, message: str) -> None:
        nonlocal checks
        checks += 1
        if not condition:
            raise Refused(message)

    # The probe turns off Unity's hierarchy-by-name sort so a durable key can
    # be joined to the object it belongs to. That is only allowed if it moves
    # nothing: if this control ever fails, every verdict below is an artefact
    # of the probe and the measurement is void.
    control = unity["sort_control"]
    require(len(control["with_default_sort"]) > 0, "the hierarchy-sort control did not run")
    require(
        control["identifiers_are_unchanged"],
        "turning off the importer's hierarchy sort changed a local file identifier, so this "
        "measurement would be describing the probe rather than Unity",
    )
    require(
        control["hierarchy_with_default_sort"] != control["hierarchy_with_sort_disabled"],
        "the hierarchy-sort control saw no reordering at all, so it proves nothing",
    )

    # Every sub-asset identifier must be the pair a project file stores. If one
    # of them ever is not, the reduced table below stops being the whole truth
    # and the run has to be refused rather than reported.
    require(
        unity["subassets_whose_identifier_is_the_guid_and_local_id"] > 0,
        "no sub-asset identifier was examined",
    )
    require(
        unity["subassets_whose_identifier_is_something_else"] == 0,
        "a sub-asset's GlobalObjectId is not its asset GUID plus its local file identifier, "
        "so the recorded table no longer says everything the editor knows",
    )

    files = {item["file"]: item for item in oracle["files"]}
    require(len(files) == len(oracle["files"]), "the oracle reported one file twice")

    scenarios = {item["name"]: item for item in unity["scenarios"]}
    require(len(scenarios) == len(unity["scenarios"]), "the editor reported one scenario twice")
    required = COMPLEX if mode == "complex" else MANDATORY
    for name in required:
        require(name in scenarios, f"the mandatory scenario {name} is not in the report")

    planned = {item["name"]: item for item in plan["scenarios"]}
    require(set(planned) == set(scenarios), "the editor did not measure exactly the planned scenarios")

    # ---- the two programs must have read the same bytes.
    for name, scenario in scenarios.items():
        for side in ("before", "after"):
            path = Path(planned[name][side])
            read = files.get(path.name)
            require(read is not None, f"{name}: the oracle never read {path.name}")
            require(
                read["fnv1a64"] == scenario[f"{side}_fnv1a64"]
                and read["bytes"] == scenario[f"{side}_bytes"],
                f"{name}: the editor and the independent reader read different {side} bytes",
            )

    # ---- the measured document must contain the confusions the brief names.
    # A scenario that passes because the confusion was never in the file would
    # be a green result about nothing.
    if mode == "complex":
        base = files[Path(planned["s11a-complex-byte-identical"]["before"]).name]["facts"]
        require(
            base["repeated_model_names"] >= 1,
            "the real assembly no longer repeats a designation, so scenario 11 measures nothing",
        )
        require(
            base["placements_sharing_one_geometry"] >= 2,
            "the real assembly no longer shares a geometry between placements",
        )
    else:
        base = files["base.fbx"]["facts"]
        require(base["repeated_geometry_display_names"] >= 1, "scenario 7 is not in the document: no two definitions share a display name")
        require(base["repeated_sibling_names"] >= 1, "scenario 8 is not in the document: no two siblings share a display name")
        require(base["placements_sharing_one_geometry"] >= 2, "scenario 9 is not in the document: no geometry has several placements")
        require(base["repeated_material_slot_names"] >= 1, "scenario 10 is not in the document: no two slots share a display name")

    # ---- the Unity-side join, checked against the independent reader.
    #
    # The witness is which definitions share one geometry, not how many
    # vertices each has: Unity welds equal corners, so on a real assembly its
    # count is legitimately lower than the file's. The sharing partition is
    # exact in both, and a join that put a durable key on the wrong object
    # would break it immediately.
    for name, scenario in scenarios.items():
        for side in ("before", "after"):
            read = files[Path(planned[name][side]).name]

            file_geometry: dict[str, set[int]] = {}
            file_vertices: dict[str, set[int]] = {}
            for node in read["nodes"]:
                if node["geometry_object_number"] == 0:
                    continue
                file_geometry.setdefault(node["definition_key"], set()).add(
                    node["geometry_object_number"]
                )
                file_vertices.setdefault(node["definition_key"], set()).add(
                    node["geometry_vertices"]
                )
            for key, numbers in file_geometry.items():
                require(
                    len(numbers) == 1,
                    f"{name}/{side}: {key} owns more than one geometry in the file",
                )

            editor_mesh: dict[str, set[int]] = {}
            for node in scenario[side]["nodes"]:
                require(node["definition_key"] != "", f"{name}/{side}: an imported node has no durable key")
                if node["mesh_vertex_count"] < 0:
                    continue
                editor_mesh.setdefault(node["definition_key"], set()).add(
                    node["mesh_local_file_id"]
                )
            for key, identifiers in editor_mesh.items():
                require(
                    len(identifiers) == 1,
                    f"{name}/{side}: the editor gave {key} more than one mesh, so its "
                    f"placements stopped sharing one geometry",
                )

            require(
                set(editor_mesh) == set(file_geometry),
                f"{name}/{side}: the editor and the file disagree about which definitions "
                f"carry geometry",
            )
            # Two definitions share one geometry in the editor exactly when they
            # share one in the file.
            file_partition = sorted(
                sorted(key for key, numbers in file_geometry.items() if next(iter(numbers)) == number)
                for number in {next(iter(numbers)) for numbers in file_geometry.values()}
            )
            editor_partition = sorted(
                sorted(key for key, ids in editor_mesh.items() if next(iter(ids)) == identifier)
                for identifier in {next(iter(ids)) for ids in editor_mesh.values()}
            )
            require(
                file_partition == editor_partition,
                f"{name}/{side}: the editor shares geometry between different definitions "
                f"than the file does",
            )

            for node in scenario[side]["nodes"]:
                if node["mesh_vertex_count"] < 0:
                    continue
                expected = next(iter(file_vertices[node["definition_key"]]))
                require(
                    0 < node["mesh_vertex_count"] <= expected,
                    f"{name}/{side}: the editor's mesh under {node['definition_key']} has "
                    f"{node['mesh_vertex_count']} vertices and the file has {expected}",
                )
                if mode != "complex":
                    # The portable document's corner normals never disagree, so
                    # the editor has nothing to weld and the counts must match
                    # exactly. On the real assembly they legitimately do not.
                    require(
                        node["mesh_vertex_count"] == expected,
                        f"{name}/{side}: the editor's mesh under {node['definition_key']} is "
                        f"not the geometry the file gives that key",
                    )
            require(
                len(scenario[side]["nodes"]) == len(read["nodes"]),
                f"{name}/{side}: the editor and the file disagree on how many nodes there are",
            )

    # ---- every tracked reference must have been judged on meaning.
    kinds_seen: set[str] = set()
    anchors_seen: set[str] = set()
    for name, scenario in scenarios.items():
        require(len(scenario["references"]) > 0, f"{name}: nothing was tracked")
        expected = (
            len(planned[name]["mesh_definitions"])
            + len(planned[name]["material_bindings"])
            + len(planned[name]["object_bindings"])
        )
        require(
            len(scenario["references"]) == expected,
            f"{name}: the report has {len(scenario['references'])} tracked references, the plan asked for {expected}",
        )
        for reference in scenario["references"]:
            kinds_seen.add(reference["unity_type"])
            anchors_seen.add(reference["unity_type"] + "/" + name)
            require(reference["unity_type"] in KINDS, f"{name}: unknown tracked type")
            require(reference["verdict"] in VERDICTS, f"{name}: unknown verdict {reference['verdict']}")
            require(
                reference["semantic_before"] != "",
                f"{name}: a reference was tracked without a durable meaning",
            )
            # A verdict of survival must rest on meaning, not on a pointer
            # being non-null, and not on the two names being equal.
            if reference["verdict"] == "same_semantic":
                require(
                    reference["semantic_after"] == reference["semantic_before"],
                    f"{name}: a reference was called kept while its meaning changed",
                )
                require(
                    reference["resolved_by_reloaded_asset"] != "<null>",
                    f"{name}: a null reference was called kept",
                )
            if reference["verdict"].startswith("missing"):
                require(
                    reference["resolved_by_reloaded_asset"] == "<null>",
                    f"{name}: a resolved reference was called missing",
                )
            require(
                reference["resolved_by_reloaded_asset"] == reference["resolved_by_stored_identifier"],
                f"{name}: the two ways of resolving one reference disagree",
            )
            require(
                reference["stored_file_id"] == reference["local_file_id_before"],
                f"{name}: what the project file stored is not the object's local file identifier",
            )
    require(kinds_seen == KINDS, f"the measurement covered only {sorted(kinds_seen)}")

    # ---- Geometry, Model and Material are reported apart.
    for name, scenario in scenarios.items():
        for kind in KINDS:
            require(
                any(item["unity_type"] == kind for item in scenario["references"]),
                f"{name}: {kind} was not measured in this scenario",
            )

    # ---- a warning is a measurement, so it has to be in the report.
    for name, scenario in scenarios.items():
        require(
            scenario["warning_transition"]
            in {"never_warned", "warning_appeared", "warning_disappeared", "warning_unchanged", "warning_changed"},
            f"{name}: no warning transition was recorded",
        )
    return checks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--unity", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--emit", type=Path)
    parser.add_argument("--expected", type=Path)
    parser.add_argument("--mode", choices=("synthetic", "complex"), default="synthetic")
    args = parser.parse_args()
    checks = verify(
        json.loads(args.unity.read_text(encoding="utf-8")),
        json.loads(args.oracle.read_text(encoding="utf-8")),
        json.loads(args.plan.read_text(encoding="utf-8")),
        args.mode,
    )
    if checks == 0:
        raise SystemExit("the join verifier ran zero checks")
    if args.emit is not None:
        table = transitions(
            json.loads(args.unity.read_text(encoding="utf-8")),
            json.loads(args.oracle.read_text(encoding="utf-8")),
            json.loads(args.plan.read_text(encoding="utf-8")),
        )
        if not table["transitions"]:
            raise SystemExit("the joined transition table is empty")
        args.emit.write_text(
            json.dumps(table, indent=1, sort_keys=True) + "\n", encoding="utf-8"
        )
    if args.expected is not None:
        if args.emit is None:
            raise SystemExit("nothing was emitted to compare with the committed table")
        if args.emit.read_bytes() != args.expected.read_bytes():
            raise SystemExit("the joined transition table differs from the committed measurement")
    print(f"FCAD_IDENTITY_JOIN_VERIFIED checks={checks}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
