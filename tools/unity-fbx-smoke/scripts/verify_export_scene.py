#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Validate the measurement-only ExportScene contract fixture."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


class ContractError(Exception):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def verify(data: dict[str, object]) -> int:
    checks = 0

    def check(condition: bool, message: str) -> None:
        nonlocal checks
        checks += 1
        require(condition, message)

    source = data["source"]
    check(source["writer_input"] == "kernel_neutral_export_scene", "writer input is not ExportScene")
    check("RenderSnapshot" in source["forbidden"], "RenderSnapshot was admitted as an export source")
    check(source["imported"].startswith("persisted_scene_hierarchy"), "imported hierarchy is not persisted Scene")
    check(source["native"] == "one_cold_rebuild_in_document_order", "native build is not one cold rebuild")
    for flag in ("second_solve", "second_step_read", "external_step_required"):
        check(source[flag] is False, f"{flag} must be false")

    fbx = data["fbx_contract"]
    expected = {
        "encoding": "ascii",
        "version": 7400,
        "right_axis": "+X",
        "up_axis": "+Y",
        "front_opposite_forward_axis": "+Z",
        "unit_meters": 1.0,
        "fcad_mm_to_fbx_m": "(x,y,z)->(x,z,-y)*0.001",
        "fcad_mm_to_unity_world": "(x,y,z)->(-x,z,-y)*0.001",
        "writer_reverses_winding": False,
        "normal_conversion": "(nx,ny,nz)->(nx,nz,-ny)",
        "transform_application": "parent_local_once",
        "mesh_ownership": "one_geometry_per_definition",
    }
    for key, value in expected.items():
        check(fbx[key] == value, f"wrong FBX contract field: {key}")

    definitions = data["definitions"]
    definition_by_key = {item["key"]: item for item in definitions}
    check(len(definition_by_key) == len(definitions), "duplicate definition key")
    check(definition_by_key["definition/asymmetric"]["material_slots"] == ["material/red", "material/blue"], "material slot order changed")
    check(definition_by_key["definition/asymmetric"]["normal_count_by_polygon_vertex"] == 12, "authored normals were not retained")
    omitted = definition_by_key["step.product_definition#2583"]
    check(omitted["geometry_key"] is None, "omitted definition acquired invented geometry")
    check(omitted["omission"]["kind"] == "GeometryOmission", "typed omission was lost")
    check(definition_by_key["step.product_definition#2428"]["measured_as_real_mesh"] is True, "#2428 was not retained as real mesh")

    nodes = data["nodes"]
    keys = [node["key"] for node in nodes]
    check(len(keys) == len(set(keys)), "nodes were merged by display name")
    stable_names = [node["stable_name"] for node in nodes]
    check(len(stable_names) == len(set(stable_names)), "stable export names are not unique")
    seen: set[str] = set()
    for node in nodes:
        check(node["parent"] is None or node["parent"] in seen, "parent missing or ordered after child")
        check(node["transform_space"] == "parent_local", "world transform substituted for local")
        if node["definition"] is not None:
            check(node["definition"] in definition_by_key, "node references unknown definition")
        seen.add(node["key"])
    repeated = [node for node in nodes if node["display_name"] == "Repeated Part"]
    check(len(repeated) == 2, "duplicate display-name placements did not remain distinct")
    check(repeated[0]["definition"] == repeated[1]["definition"] == "definition/asymmetric", "placements do not share one definition mesh")
    check(all(node["parent"] == "node/assembly" for node in repeated), "placement parent was lost")
    omitted_nodes = [node for node in nodes if node["definition"] == "step.product_definition#2583"]
    check(len(omitted_nodes) == 1, "empty omitted hierarchy node was dropped")
    check(omitted_nodes[0]["parent"] == "node/assembly", "omitted-node parent was lost")
    check(omitted_nodes[0]["omission_marker"] == "FerriteCADGeometryOmission", "omission marker was dropped")

    check(data["complete"] is False, "partial assembly was declared complete")
    check(data["partial_exit_code"] == 6, "partial export has no deterministic non-clean exit")
    complex_data = data["complex_measurement"]
    expected_complex = {
        "definitions": 46,
        "root_nodes": 1,
        "non_root_occurrences": 139,
        "tessellated_leaf_definitions": 34,
        "render_snapshot_meshes": 35,
        "render_snapshot_draws": 112,
        "omitted_definition": "step.product_definition#2583",
        "real_mesh_definition": "step.product_definition#2428",
    }
    for key, value in expected_complex.items():
        check(complex_data[key] == value, f"wrong complex baseline: {key}")
    check(complex_data["definitions"] != complex_data["render_snapshot_meshes"], "RenderSnapshot was treated as the assembly structure")
    return checks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("contract", type=Path)
    args = parser.parse_args()
    try:
        checks = verify(json.loads(args.contract.read_text(encoding="utf-8")))
    except (ContractError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"FCAD_EXPORT_SCENE_CONTRACT_FAILURE {error}")
        return 1
    if checks == 0:
        print("FCAD_EXPORT_SCENE_CONTRACT_FAILURE zero checks")
        return 1
    print(f"FCAD_EXPORT_SCENE_CONTRACT_EXECUTED checks={checks}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
