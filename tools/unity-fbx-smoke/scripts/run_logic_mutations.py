#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Apply semantic §22B-1a mutants to copies of canonical measurements."""

from __future__ import annotations

import copy
import json
from pathlib import Path

from verify_export_scene import verify as verify_scene
from verify_measurements import verify as verify_measurements


ROOT = Path(__file__).resolve().parent.parent
CONTRACT = json.loads((ROOT / "Assets/Fixtures/export-scene-contract.json").read_text(encoding="utf-8"))
UNITY = json.loads((ROOT / "Assets/Expected/unity-import-report.json").read_text(encoding="utf-8"))
INDEPENDENT = json.loads((ROOT / "Assets/Expected/independent-reader-report.json").read_text(encoding="utf-8"))


def fixture(report: dict[str, object], name: str) -> dict[str, object]:
    return next(item for item in report["fixtures"] if item["fixture"] == name)


def raw(report: dict[str, object], name: str) -> dict[str, object]:
    return next(item for item in report["files"] if item["file"] == name)


def node(contract: dict[str, object], key: str) -> dict[str, object]:
    return next(item for item in contract["nodes"] if item["key"] == key)


def expect_kill(name: str, mutate, gate: str) -> None:
    contract = copy.deepcopy(CONTRACT)
    unity = copy.deepcopy(UNITY)
    independent = copy.deepcopy(INDEPENDENT)
    mutate(contract, unity, independent)
    try:
        if gate == "scene":
            verify_scene(contract)
        else:
            verify_measurements(unity, independent)
    except Exception as error:  # each verifier's typed error or a missing mandatory field
        print(f"killed at runtime {gate} gate: {name}: {error}")
        return
    raise SystemExit(f"survived unexpectedly: {name}")


def main() -> int:
    checks = verify_scene(copy.deepcopy(CONTRACT)) + verify_measurements(copy.deepcopy(UNITY), copy.deepcopy(INDEPENDENT))
    if checks == 0:
        raise SystemExit("mutation baseline ran zero checks")
    print(f"mutation baseline: checks={checks}")

    mutations = [
        ("y_up_and_z_up_swapped", lambda c, u, i: raw(i, "yup_m_preconverted_ascii7400.fbx")["axes"].update(up="+Z"), "measurement"),
        ("handedness_changed_without_winding", lambda c, u, i: raw(i, "yup_m_preconverted_ascii7400.fbx")["mesh"]["polygons"][0].update(indices=[0, 1, 2]), "measurement"),
        ("millimetres_left_as_unity_metres", lambda c, u, i: [point.update(distance_from_origin=value) for point, value in zip(fixture(u, "yup_m_preconverted_ascii7400.fbx")["control_points"], [0, 1000, 2000, 3000])], "measurement"),
        ("unit_conversion_applied_twice", lambda c, u, i: [point.update(distance_from_origin=value) for point, value in zip(fixture(u, "yup_m_preconverted_ascii7400.fbx")["control_points"], [0, 0.001, 0.002, 0.003])], "measurement"),
        ("accumulated_transform_applied_twice", lambda c, u, i: c["fbx_contract"].update(transform_application="parent_local_twice"), "scene"),
        ("world_transform_substituted_for_local", lambda c, u, i: node(c, "node/instance-a").update(transform_space="world"), "scene"),
        ("shared_mesh_duplicated_per_placement", lambda c, u, i: node(c, "node/instance-b").update(definition="definition/asymmetric-copy"), "scene"),
        ("placement_parent_lost", lambda c, u, i: node(c, "node/instance-a").update(parent="node/root"), "scene"),
        ("equal_display_names_merged", lambda c, u, i: c["nodes"].remove(node(c, "node/instance-b")), "scene"),
        ("normals_recalculated", lambda c, u, i: fixture(u, "yup_m_preconverted_ascii7400.fbx")["meshes"][0].update(normals=[0.0] * 27), "measurement"),
        ("material_slot_or_colour_lost", lambda c, u, i: fixture(u, "yup_m_preconverted_ascii7400.fbx")["materials"].pop(), "measurement"),
        ("partial_assembly_declared_complete", lambda c, u, i: c.update(complete=True), "scene"),
        ("empty_omitted_node_discarded", lambda c, u, i: c["nodes"].remove(node(c, "node/omitted-2583")), "scene"),
        ("render_snapshot_used_as_structure_source", lambda c, u, i: c["source"].update(imported="RenderSnapshot"), "scene"),
    ]
    for name, mutate, gate in mutations:
        expect_kill(name, mutate, gate)
    print(f"mutation campaign: {len(mutations)} runtime mutants killed")
    print("mutation campaign: 0 unexpected survivors")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
