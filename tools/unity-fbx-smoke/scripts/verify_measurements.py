#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Semantic gates over the independent-reader and Unity measurement reports."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path


class MeasurementError(Exception):
    pass


def near(left: object, right: object, tolerance: float = 1.0e-5) -> bool:
    if isinstance(right, list):
        return isinstance(left, list) and len(left) == len(right) and all(
            near(a, b, tolerance) for a, b in zip(left, right)
        )
    if isinstance(right, (int, float)):
        return isinstance(left, (int, float)) and math.isclose(float(left), float(right), abs_tol=tolerance, rel_tol=0.0)
    return left == right


def verify(unity: dict[str, object], independent: dict[str, object]) -> int:
    checks = 0

    def check(condition: bool, message: str) -> None:
        nonlocal checks
        checks += 1
        if not condition:
            raise MeasurementError(message)

    check(unity["unity_version"] == "6000.4.10f1", "measurement used another Unity version")
    check(unity["checks"] > 40, "Unity report is a zero-check/stale placeholder")
    check(unity["colour_space"] == "gamma", "measurement project colour space changed")
    fixture_by_name = {item["fixture"]: item for item in unity["fixtures"]}
    check(len(fixture_by_name) == 5, "Unity did not measure the complete five-fixture matrix")

    desired = fixture_by_name["yup_m_preconverted_ascii7400.fbx"]
    zup = fixture_by_name["zup_mm_ascii7400.fbx"]
    wrong_axis = fixture_by_name["wrong_yup_metadata_ascii7400.fbx"]
    wrong_metres = fixture_by_name["wrong_m_metadata_ascii7400.fbx"]
    wrong_double = fixture_by_name["wrong_double_unit_ascii7400.fbx"]
    for name, fixture in fixture_by_name.items():
        check(fixture["importer_messages"] == [], f"Unity importer message in {name}")
        check(fixture["importer"]["use_file_scale"] is True, f"useFileScale changed in {name}")
        check(near(fixture["importer"]["global_scale"], 1.0), f"globalScale changed in {name}")
        check(fixture["importer"]["bake_axis_conversion"] is False, f"bakeAxisConversion changed in {name}")

    check(near(desired["importer"]["file_scale"], 1.0), "metre contract does not import at fileScale one")
    check(near(zup["importer"]["file_scale"], 0.001), "millimetre metadata did not yield fileScale 0.001")
    check(near([point["distance_from_origin"] for point in desired["control_points"]], [0.0, 1.0, 2.0, 3.0]), "1000 mm did not become one Unity world unit")
    check(near([point["distance_from_origin"] for point in zup["control_points"]], [0.0, 1.0, 2.0, 3.0]), "raw Z-up/mm comparison did not scale once")
    check(near([point["distance_from_origin"] for point in wrong_metres["control_points"]], [0.0, 1000.0, 2000.0, 3000.0], 0.01), "mm-as-metres mutation was not exposed")
    check(near([point["distance_from_origin"] for point in wrong_double["control_points"]], [0.0, 0.001, 0.002, 0.003], 1.0e-6), "double conversion mutation was not exposed")
    check(desired["tree"][0]["local_rotation"] == [0.0, 0.0, 0.0, 1.0], "chosen contract has a hidden root rotation")
    check(desired["tree"][0]["local_scale"] == [1.0, 1.0, 1.0], "chosen contract has a hidden root scale")
    check(zup["tree"][0]["local_rotation"] != desired["tree"][0]["local_rotation"], "axis metadata had no root-transform effect")
    check([node["world_matrix"] for node in wrong_axis["tree"]] != [node["world_matrix"] for node in zup["tree"]], "Y-up/Z-up metadata swap had no numeric effect")

    names = [node["name"] for node in desired["tree"]]
    check(len(names) == 9, "Unity hierarchy node count changed")
    check(names.count("Assembly Frame") == 1, "Unity lost assembly parent")
    check("Repeated Part" in names and "Repeated Part 1" in names, "Unity merged duplicate display-name nodes")
    check("Omitted #2583" in names, "Unity dropped empty omitted hierarchy node")
    omitted_path = next(node["path"] for node in desired["tree"] if node["name"] == "Omitted #2583")
    check("Assembly Frame" in omitted_path, "Unity changed omitted-node parent")
    check(desired["mesh_filter_count"] == 2, "Unity placement mesh count changed")
    check(desired["unique_mesh_asset_count"] == 1, "Unity duplicated shared mesh asset")
    check(desired["repeated_parts_share_mesh"] is True, "Unity sharedMesh identity changed")

    mesh = desired["meshes"][0]
    check((mesh["vertex_count"], mesh["index_count"], mesh["submesh_count"]) == (9, 12, 2), "Unity mesh cardinality/slots changed")
    check(mesh["submeshes"] == [
        {"slot": 0, "indices": [0, 1, 2, 3, 4, 1]},
        {"slot": 1, "indices": [5, 2, 4, 6, 7, 8]},
    ], "Unity winding/index conversion changed")
    expected_normals = [
        -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0,
        0.0, 0.0, -1.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0,
    ]
    check(near(mesh["normals"], expected_normals), "Unity did not retain authored fixture normals")
    check(len(mesh["triangle_orientation"]) == 4, "Unity did not report every triangle orientation")

    materials = desired["materials"]
    check(len(materials) == 4, "Unity lost material slots on instances")
    for offset in (0, 2):
        check(materials[offset]["slot"] == 0 and near(materials[offset]["base_colour"], [0.8, 0.2, 0.1, 1.0]), "Unity red slot/colour changed")
        check(materials[offset + 1]["slot"] == 1 and near(materials[offset + 1]["base_colour"], [0.1, 0.35, 0.9, 1.0]), "Unity blue slot/colour changed")
    properties = {(item["node_name"], item["property"], item["value"]) for item in desired["user_properties"]}
    check(("Omitted #2583", "FerriteCADComplete", "False") in properties, "Unity callback did not expose partial marker")
    check(any(item[0] == "Omitted #2583" and item[1] == "FerriteCADGeometryOmission" for item in properties), "Unity callback did not expose omission marker")

    encoding = unity["encoding_probes"]
    check(len(encoding) == 1, "trusted binary encoding probe missing")
    check(encoding[0]["accepted"] is True and encoding[0]["encoding"] == "binary" and encoding[0]["fbx_version"] == 7400, "Unity rejected trusted FBX 7.4 binary")
    check(encoding[0]["importer_messages"] == [], "trusted binary probe emitted importer messages")

    check(independent["reader"] == "ufbx 0.23.0" and independent["strict"] is True, "independent reader pin/strict mode changed")
    raw_by_name = {item["file"]: item for item in independent["files"]}
    check(len(raw_by_name) == 6, "independent reader did not inspect every fixture")
    for name, raw in raw_by_name.items():
        check(raw["fbx_version"] == 7400 and raw["warnings"] == 0, f"raw fixture version/warnings changed: {name}")
    check(raw_by_name["unity_builtin_disc_binary7400.fbx"]["format"] == "binary", "trusted binary is not independently binary")
    raw = raw_by_name["yup_m_preconverted_ascii7400.fbx"]
    check(raw["format"] == "ascii", "chosen encoding is not ASCII")
    check(raw["axes"] == {"right": "+X", "up": "+Y", "front_opposite_forward": "+Z"}, "chosen raw axes changed")
    check(near(raw["unit_meters"], 1.0), "chosen raw FBX unit is not metre")
    check((raw["node_count_excluding_implicit_root"], raw["mesh_count"]) == (9, 1), "raw hierarchy/mesh count changed")
    raw_names = [node["name"] for node in raw["nodes"]]
    check(raw_names.count("Repeated Part") == 2, "raw FBX merged equal display names")
    check(next(node for node in raw["nodes"] if node["name"] == "Omitted #2583")["parent"] == "Assembly Frame", "raw FBX lost omitted parent")
    check(raw["mesh"]["instances"] == ["Repeated Part", "Repeated Part"], "raw FBX duplicated shared geometry")
    check(raw["mesh"]["vertices"] == [[0, 0, 0], [1, 0, 0], [0, 0, -2], [0, 3, 0]], "raw coordinate conversion changed")
    check(raw["mesh"]["polygons"] == [
        {"indices": [0, 2, 1], "material": 0},
        {"indices": [0, 1, 3], "material": 0},
        {"indices": [0, 3, 2], "material": 1},
        {"indices": [1, 2, 3], "material": 1},
    ], "raw winding or material assignment changed")
    expected_raw_normals = [
        [1, 0, 0], [0, 0, -1], [0, 1, 0], [0, 0, -1],
        [0, 1, 0], [1, 0, 0], [0, 1, 0], [1, 0, 0],
        [0, 0, -1], [1, 0, 0], [0, 1, 0], [0, 0, -1],
    ]
    check(raw["mesh"]["normals_by_polygon_vertex"] == expected_raw_normals, "raw authored normals changed")
    check(raw["mesh"]["materials"] == ["Ferrite Red", "Ferrite Blue"], "raw material slots changed")
    check(near([item["diffuse_colour"] for item in raw["materials"]], [[0.8, 0.2, 0.1], [0.1, 0.35, 0.9]]), "raw material colours changed")
    return checks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--unity", type=Path, required=True)
    parser.add_argument("--independent", type=Path, required=True)
    args = parser.parse_args()
    try:
        checks = verify(
            json.loads(args.unity.read_text(encoding="utf-8")),
            json.loads(args.independent.read_text(encoding="utf-8")),
        )
    except (MeasurementError, KeyError, TypeError, IndexError, json.JSONDecodeError) as error:
        print(f"FCAD_FBX_MEASUREMENT_FAILURE {error}")
        return 1
    if checks == 0:
        print("FCAD_FBX_MEASUREMENT_FAILURE zero checks")
        return 1
    print(f"FCAD_FBX_MEASUREMENT_EXECUTED checks={checks}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
