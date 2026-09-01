#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Generate measurement-only FBX fixtures for the Unity importer probe.

This is deliberately not an FBX library and is not a candidate production
writer.  It emits the small, closed scene described below in enough variants
to separate axis conversion, unit conversion and encoding.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, Sequence

GENERATOR = "ferritecad-unity-fbx-measurement-generator/1"
FBX_VERSION = 7400


@dataclass(frozen=True)
class Array:
    kind: str
    values: tuple[int | float, ...]


@dataclass(frozen=True)
class Int64:
    value: int


@dataclass(frozen=True)
class ObjectName:
    value: str


@dataclass
class Node:
    name: str
    properties: list[object] = field(default_factory=list)
    children: list["Node"] = field(default_factory=list)
    block: bool = False


def node(name: str, *properties: object, children: Iterable[Node] = (), block: bool = False) -> Node:
    items = list(children)
    return Node(name, list(properties), items, block or bool(items))


def p(name: str, kind: str, label: str, flags: str, *values: object) -> Node:
    return node("P", name, kind, label, flags, *values)


def matrix_multiply(a: Sequence[Sequence[float]], b: Sequence[Sequence[float]]) -> list[list[float]]:
    return [[sum(a[r][k] * b[k][c] for k in range(3)) for c in range(3)] for r in range(3)]


def matrix_transpose(a: Sequence[Sequence[float]]) -> list[list[float]]:
    return [[a[c][r] for c in range(3)] for r in range(3)]


def euler_matrix_xyz(degrees: Sequence[float]) -> list[list[float]]:
    x, y, z = (math.radians(value) for value in degrees)
    cx, sx = math.cos(x), math.sin(x)
    cy, sy = math.cos(y), math.sin(y)
    cz, sz = math.cos(z), math.sin(z)
    rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]]
    ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]]
    rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]]
    return matrix_multiply(rz, matrix_multiply(ry, rx))


def matrix_to_euler_xyz(m: Sequence[Sequence[float]]) -> tuple[float, float, float]:
    y = math.asin(max(-1.0, min(1.0, -m[2][0])))
    if abs(math.cos(y)) > 1.0e-10:
        x = math.atan2(m[2][1], m[2][2])
        z = math.atan2(m[1][0], m[0][0])
    else:
        x = math.atan2(-m[1][2], m[1][1])
        z = 0.0
    return tuple(math.degrees(value) for value in (x, y, z))


Z_UP_TO_Y_UP = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]]


def transform_vector(values: Sequence[float], matrix: Sequence[Sequence[float]], scale: float) -> tuple[float, float, float]:
    return tuple(scale * sum(matrix[r][c] * values[c] for c in range(3)) for r in range(3))


def transform_rotation(values: Sequence[float], matrix: Sequence[Sequence[float]]) -> tuple[float, float, float]:
    source = euler_matrix_xyz(values)
    converted = matrix_multiply(matrix, matrix_multiply(source, matrix_transpose(matrix)))
    return matrix_to_euler_xyz(converted)


def properties70(translation: Sequence[float], rotation: Sequence[float], custom: Sequence[Node]) -> Node:
    return node(
        "Properties70",
        children=[
            p("Lcl Translation", "Lcl Translation", "", "A", *translation),
            p("Lcl Rotation", "Lcl Rotation", "", "A", *rotation),
            p("Lcl Scaling", "Lcl Scaling", "", "A", 1.0, 1.0, 1.0),
            *custom,
        ],
    )


def model(
    identifier: int,
    name: str,
    kind: str,
    translation: Sequence[float],
    rotation: Sequence[float],
    stable_key: str,
    omission: str | None = None,
) -> Node:
    custom = [p("FerriteCADNodeKey", "KString", "", "U", stable_key)]
    if omission is not None:
        custom.extend(
            [
                p("FerriteCADGeometryOmission", "KString", "", "U", omission),
                p("FerriteCADComplete", "bool", "", "U", 0),
            ]
        )
    children = [
        node("Version", 232),
        properties70(translation, rotation, custom),
        node("Shading", True),
        node("Culling", "CullingOff"),
    ]
    if kind == "Null":
        children.append(node("TypeFlags", "Null"))
    return node("Model", Int64(identifier), ObjectName(f"Model::{name}"), kind, children=children)


def material(identifier: int, name: str, colour: Sequence[float]) -> Node:
    return node(
        "Material",
        Int64(identifier),
        ObjectName(f"Material::{name}"),
        "",
        children=[
            node("Version", 102),
            node("ShadingModel", "phong"),
            node("MultiLayer", False),
            node(
                "Properties70",
                children=[
                    p("DiffuseColor", "ColorRGB", "Color", "", *colour),
                    p("DiffuseFactor", "Number", "", "A", 1.0),
                    p("TransparencyFactor", "Number", "", "A", 0.0),
                ],
            ),
        ],
    )


def geometry(identifier: int, positions: Sequence[Sequence[float]], normals: Sequence[Sequence[float]]) -> Node:
    triangles = ((0, 2, 1), (0, 1, 3), (0, 3, 2), (1, 2, 3))
    polygon_indices: list[int] = []
    for triangle in triangles:
        polygon_indices.extend((triangle[0], triangle[1], -triangle[2] - 1))
    flat_positions = tuple(value for point in positions for value in point)
    flat_normals = tuple(value for normal in normals for value in normal)
    return node(
        "Geometry",
        Int64(identifier),
        ObjectName("Geometry::Asymmetric1000x2000x3000"),
        "Mesh",
        children=[
            node("GeometryVersion", 124),
            node("Vertices", Array("d", flat_positions)),
            node("PolygonVertexIndex", Array("i", tuple(polygon_indices))),
            node(
                "LayerElementNormal",
                0,
                children=[
                    node("Version", 101),
                    node("Name", "FCAD authored normals"),
                    node("MappingInformationType", "ByPolygonVertex"),
                    node("ReferenceInformationType", "Direct"),
                    node("Normals", Array("d", flat_normals)),
                ],
            ),
            node(
                "LayerElementMaterial",
                0,
                children=[
                    node("Version", 101),
                    node("Name", ""),
                    node("MappingInformationType", "ByPolygon"),
                    node("ReferenceInformationType", "IndexToDirect"),
                    node("Materials", Array("i", (0, 0, 1, 1))),
                ],
            ),
            node(
                "Layer",
                0,
                children=[
                    node("Version", 100),
                    node("LayerElement", children=[node("Type", "LayerElementNormal"), node("TypedIndex", 0)]),
                    node("LayerElement", children=[node("Type", "LayerElementMaterial"), node("TypedIndex", 0)]),
                ],
            ),
        ],
    )


def scene_nodes(axis_y_up: bool, coordinate_y_up: bool, values_in_metres: bool, unit_metres: bool) -> list[Node]:
    coordinate = Z_UP_TO_Y_UP if coordinate_y_up else [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    scale = 0.001 if values_in_metres else 1.0

    raw_positions = ((0.0, 0.0, 0.0), (1000.0, 0.0, 0.0), (0.0, 2000.0, 0.0), (0.0, 0.0, 3000.0))
    positions = [transform_vector(value, coordinate, scale) for value in raw_positions]
    raw_normals = (
        (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0),
        (0.0, 1.0, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0),
        (0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0),
        (1.0, 0.0, 0.0), (0.0, 0.0, 1.0), (0.0, 1.0, 0.0),
    )
    normals = [transform_vector(value, coordinate, 1.0) for value in raw_normals]

    transforms = {
        "root": ((0.0, 0.0, 0.0), (0.0, 0.0, 0.0)),
        "assembly": ((100.0, 200.0, 300.0), (11.0, 23.0, -17.0)),
        "instance_a": ((1200.0, -400.0, 800.0), (31.0, -19.0, 47.0)),
        "instance_b": ((-700.0, 900.0, 1300.0), (-13.0, 29.0, -37.0)),
        "omitted": ((400.0, 500.0, 600.0), (7.0, 13.0, 29.0)),
    }

    def converted(key: str) -> tuple[tuple[float, float, float], tuple[float, float, float]]:
        translation, rotation = transforms[key]
        return (
            transform_vector(translation, coordinate, scale),
            transform_rotation(rotation, coordinate),
        )

    objects: list[Node] = [geometry(1000, positions, normals)]
    for identifier, name, kind, key, stable, omission in (
        (2000, "FCAD_ROOT", "Null", "root", "node/root", None),
        (2001, "Assembly Frame", "Null", "assembly", "node/assembly", None),
        (2002, "Repeated Part", "Mesh", "instance_a", "node/instance-a", None),
        (2003, "Repeated Part", "Mesh", "instance_b", "node/instance-b", None),
        (
            2004,
            "Omitted #2583",
            "Null",
            "omitted",
            "node/omitted-2583",
            "definition=step.product_definition#2583;reason=tessellation status 6",
        ),
    ):
        translation, rotation = converted(key)
        objects.append(model(identifier, name, kind, translation, rotation, stable, omission))

    for offset, (name, point, stable) in enumerate(
        (
            ("CP Origin", raw_positions[0], "control/origin"),
            ("CP X1000", raw_positions[1], "control/x1000"),
            ("CP Y2000", raw_positions[2], "control/y2000"),
            ("CP Z3000", raw_positions[3], "control/z3000"),
        )
    ):
        translation = transform_vector(point, coordinate, scale)
        objects.append(model(2100 + offset, name, "Null", translation, (0.0, 0.0, 0.0), stable))

    objects.extend(
        [
            material(3000, "Ferrite Red", (0.8, 0.2, 0.1)),
            material(3001, "Ferrite Blue", (0.1, 0.35, 0.9)),
        ]
    )

    up_axis = 1 if axis_y_up else 2
    front_axis = 2 if axis_y_up else 1
    front_axis_sign = 1 if axis_y_up else -1
    unit_scale = 100.0 if unit_metres else 0.1
    properties = [
        p("UpAxis", "int", "Integer", "", up_axis),
        p("UpAxisSign", "int", "Integer", "", 1),
        p("FrontAxis", "int", "Integer", "", front_axis),
        p("FrontAxisSign", "int", "Integer", "", front_axis_sign),
        p("CoordAxis", "int", "Integer", "", 0),
        p("CoordAxisSign", "int", "Integer", "", 1),
        p("OriginalUpAxis", "int", "Integer", "", up_axis),
        p("OriginalUpAxisSign", "int", "Integer", "", 1),
        p("UnitScaleFactor", "double", "Number", "", unit_scale),
        p("OriginalUnitScaleFactor", "double", "Number", "", unit_scale),
    ]

    connections = [
        node("C", "OO", Int64(2000), Int64(0)),
        node("C", "OO", Int64(2001), Int64(2000)),
        node("C", "OO", Int64(2002), Int64(2001)),
        node("C", "OO", Int64(2003), Int64(2001)),
        node("C", "OO", Int64(2004), Int64(2001)),
        node("C", "OO", Int64(2100), Int64(2002)),
        node("C", "OO", Int64(2101), Int64(2002)),
        node("C", "OO", Int64(2102), Int64(2002)),
        node("C", "OO", Int64(2103), Int64(2002)),
        node("C", "OO", Int64(1000), Int64(2002)),
        node("C", "OO", Int64(1000), Int64(2003)),
        node("C", "OO", Int64(3000), Int64(2002)),
        node("C", "OO", Int64(3001), Int64(2002)),
        node("C", "OO", Int64(3000), Int64(2003)),
        node("C", "OO", Int64(3001), Int64(2003)),
    ]

    return [
        node(
            "FBXHeaderExtension",
            children=[
                node("FBXHeaderVersion", 1003),
                node("FBXVersion", FBX_VERSION),
                node("EncryptionType", 0),
                node(
                    "CreationTimeStamp",
                    children=[
                        node("Version", 1000), node("Year", 2000), node("Month", 1),
                        node("Day", 1), node("Hour", 0), node("Minute", 0),
                        node("Second", 0), node("Millisecond", 0),
                    ],
                ),
                node("Creator", GENERATOR),
            ],
        ),
        node("FileId", bytes.fromhex("464341442d3232422d31412d4649585455")),
        node("CreationTime", "2000-01-01 00:00:00:000"),
        node("Creator", GENERATOR),
        node("GlobalSettings", children=[node("Version", 1000), node("Properties70", children=properties)]),
        node(
            "Documents",
            children=[
                node("Count", 1),
                node(
                    "Document",
                    Int64(4000),
                    "Scene",
                    "Scene",
                    children=[node("Properties70", block=True), node("RootNode", Int64(0))],
                )
            ],
        ),
        node("References", block=True),
        node(
            "Definitions",
            children=[
                node("Version", 100),
                node("Count", len(objects) + 1),
                node("ObjectType", "GlobalSettings", children=[node("Count", 1)]),
                node("ObjectType", "Geometry", children=[node("Count", 1)]),
                node("ObjectType", "Model", children=[node("Count", 9)]),
                node("ObjectType", "Material", children=[node("Count", 2)]),
            ],
        ),
        node("Objects", children=objects),
        node("Connections", children=connections),
        node("Takes", children=[node("Current", "")]),
    ]


def format_number(value: int | float) -> str:
    if isinstance(value, bool):
        return "1" if value else "0"
    if isinstance(value, int):
        return str(value)
    if value == 0.0:
        return "0.0"
    return format(value, ".17g")


def quote(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def ascii_property(value: object) -> str:
    if isinstance(value, Int64):
        return str(value.value)
    if isinstance(value, ObjectName):
        return quote(value.value)
    if isinstance(value, str):
        return quote(value)
    if isinstance(value, bytes):
        return '"' + value.hex().upper() + '"'
    if isinstance(value, (bool, int, float)):
        return format_number(value)
    raise TypeError(value)


def render_ascii(nodes: Sequence[Node]) -> bytes:
    lines = ["; FBX 7.4.0 project file", f"; {GENERATOR}", ""]

    def render(current: Node, depth: int) -> None:
        indent = "\t" * depth
        arrays = [value for value in current.properties if isinstance(value, Array)]
        if arrays:
            if len(current.properties) != 1:
                raise ValueError("an array node must have exactly one property")
            array = arrays[0]
            values = ",".join(format_number(value) for value in array.values)
            lines.append(f"{indent}{current.name}: *{len(array.values)} {{")
            lines.append(f"{indent}\ta: {values}")
            lines.append(f"{indent}}}")
            return
        suffix = ":"
        if current.properties:
            suffix += " " + ", ".join(ascii_property(value) for value in current.properties)
        if current.block:
            lines.append(f"{indent}{current.name}{suffix} {{")
            for child in current.children:
                render(child, depth + 1)
            lines.append(f"{indent}}}")
        else:
            lines.append(f"{indent}{current.name}{suffix}")

    for item in nodes:
        render(item, 0)
    lines.append("")
    return "\n".join(lines).encode("utf-8")


def write_fixture(
    path: Path,
    *,
    axis_y_up: bool,
    coordinate_y_up: bool,
    values_in_metres: bool,
    unit_metres: bool,
) -> dict[str, object]:
    nodes = scene_nodes(axis_y_up, coordinate_y_up, values_in_metres, unit_metres)
    data = render_ascii(nodes)
    path.write_bytes(data)
    return {
        "file": path.name,
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
        "fbx_version": FBX_VERSION,
        "encoding": "ascii",
        "axis": "y_up_right_handed" if axis_y_up else "z_up_right_handed",
        "unit": "metre" if unit_metres else "millimetre",
        "unit_scale_factor_centimetres": 100.0 if unit_metres else 0.1,
        "coordinate_axis": "y_up" if coordinate_y_up else "fcad_z_up",
        "coordinate_unit": "metre" if values_in_metres else "millimetre",
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)

    fixtures = [
        write_fixture(args.output / "zup_mm_ascii7400.fbx", axis_y_up=False, coordinate_y_up=False, values_in_metres=False, unit_metres=False),
        write_fixture(args.output / "wrong_yup_metadata_ascii7400.fbx", axis_y_up=True, coordinate_y_up=False, values_in_metres=False, unit_metres=False),
        write_fixture(args.output / "wrong_m_metadata_ascii7400.fbx", axis_y_up=False, coordinate_y_up=False, values_in_metres=False, unit_metres=True),
        write_fixture(args.output / "wrong_double_unit_ascii7400.fbx", axis_y_up=False, coordinate_y_up=False, values_in_metres=True, unit_metres=False),
        write_fixture(args.output / "yup_m_preconverted_ascii7400.fbx", axis_y_up=True, coordinate_y_up=True, values_in_metres=True, unit_metres=True),
    ]
    manifest = {
        "generator": GENERATOR,
        "fbx_version": FBX_VERSION,
        "fixtures": fixtures,
    }
    (args.output / "fixture-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )


if __name__ == "__main__":
    main()
