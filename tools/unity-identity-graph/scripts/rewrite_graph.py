#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The measurement-only structural transformer for §22B-1e2b.

This is not a second FBX serializer and it must never become one. It is handed
the bytes the *production* writer produced — through the `fbx_channel_documents`
example, which calls `write_fbx_ascii_7400` and nothing else — and it changes
exactly three named sections inside them:

  * ``Definitions``   — only the two ``Count:`` numbers that say how many
                        objects and how many ``Model`` objects the file holds.
  * ``Objects``       — new ``Model`` blocks are appended after the last
                        existing ``Model``; existing blocks are copied through
                        with at most two edits, both named below.
  * ``Connections``   — new ``C: "OO"`` lines are added, and for one variant
                        the geometry and material connections of an occurrence
                        are re-pointed at that occurrence's new child.

The two edits it is allowed to make inside an existing ``Model`` block are the
``"Null"``/``"Mesh"`` subclass word in the block's own header line, and custom
properties appended inside ``Properties70``. It never touches a ``Geometry``
block, a ``Material`` block, a vertex, an index, a normal, a colour, a
transform, the header or ``GlobalSettings``, and it never renumbers an existing
object. The oracle checks all of that against the control rather than trusting
this docstring: see ``read_graphs.c``.

Why a structural transformer exists at all
------------------------------------------

§22B-1e2a measured names and properties on the *flat* production graph and
found that the identity of a shared ``Mesh`` is a function of whichever
placement Unity happens to reach first. It explicitly did not measure whether a
different FBX graph moves that identity onto the definition. That is what these
variants are for, and each is a question rather than a proposal.

The variants
------------

``g-flat``
    The production bytes, copied. The control, and the graph the product ships
    today. Human designations in the object names, one ``Model`` per placement,
    the ``Geometry`` connected to every placement that uses it. It carries no
    added property at all, because a control that carried one would not be the
    production bytes.

``g-flat-id``
    The same graph, with only the invisible identity properties added. It is
    the control for the *topology* question: every variant below differs from
    it in graph shape and in nothing else, so a difference between one of them
    and this one cannot be attributed to the identity channel §22B-1e2a
    already measured.

``g-carrier``
    ``g-flat`` plus one canonical *definition carrier* ``Model`` per geometry,
    machine-named, parented to the scene root, and connected to its
    ``Geometry`` **before** any occurrence is. If Unity names a shared mesh
    after the first ``Model`` that claims it, the carrier is the first, and the
    mesh's identity stops depending on placement order.

``g-carrier-detached``
    The same carrier with **no** parent connection at all. It exists because a
    carrier parented to the root is a node a person can see, and the question
    of whether the carrier has to be visible is separable from the question of
    whether it works.

``g-two-level``
    No new definition object. Each geometry-bearing occurrence keeps its human
    name and its transform, becomes a ``"Null"``, and its ``Geometry`` and
    ``Material`` connections move to a new machine-named child ``Model`` with an
    identity transform. The occurrence node stays the thing a person reads; the
    geometry-bearing node is the thing Unity names the mesh after.

Every variant also carries, as custom properties on every ``Model``, the
source-qualified definition identity and the durable occurrence identity that
§22B-1e2a established a join needs and that the production property cannot
express. Those are invisible to a person by construction, so a variant's
*visible* names are exactly the production designations unless this file says
otherwise — which is what makes "does a person see a machine token" a property
of the graph and not of the identity channel.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
from pathlib import Path

# `::` separates an FBX object's class from its name, so a name may not contain
# it, and Unity shows the name in a hierarchy, so a control character would be a
# different experiment. `~` is the separator for that reason and no other.
SEPARATOR = "~"

VARIANTS = ("g-flat", "g-flat-id", "g-carrier", "g-carrier-detached", "g-two-level")

# The variants that add objects, and by how much per geometry-bearing thing.
# Kept here rather than counted afterwards so a transformer that added an
# object it did not mean to is a refusal rather than a larger `Count:`.
ADDS_ONE_MODEL_PER_GEOMETRY = ("g-carrier", "g-carrier-detached")
ADDS_ONE_MODEL_PER_MESH_NODE = ("g-two-level",)

HEADER = re.compile(
    r'^(?P<indent>\t)(?P<class>Model|Geometry|Material): '
    r'(?P<number>-?\d+), "(?P=class)::(?P<name>.*)", "(?P<subclass>[^"]*)" \{$'
)
NODE_KEY = re.compile(r'^\t+P: "FerriteCADNodeKey", "KString", "", "U", "(?P<key>[^"]*)"$')
DEFINITION_KEY = re.compile(
    r'^(?P<indent>\t+)P: "FerriteCADDefinitionKey", "KString", "", "U", "(?P<key>[^"]*)"$'
)
CONNECTION = re.compile(r'^\tC: "OO", (?P<child>-?\d+), (?P<parent>-?\d+)$')
OBJECT_TYPE = re.compile(r'^\tObjectType: "(?P<name>[^"]+)" \{$')
COUNT = re.compile(r'^(?P<indent>\t+)Count: (?P<value>\d+)$')

# New objects are numbered from a base no production number can reach: the
# writer numbers `Geometry` from 0x200000001, `Model` from 0x400000000 and
# `Material` from 0x600000000, so 0x1000000000 is four bits above the highest
# class prefix it can ever use. A collision is refused anyway rather than
# assumed impossible.
ADDED_NUMBER_BASE = 0x1000000000


class Refused(Exception):
    """The file is not the one this transformer was written for."""


def escape(value: str) -> str:
    """An FBX ASCII quoted string.

    Deliberately minimal: a name that needed more than this would be a name the
    rest of the measurement could not compare, so it is refused instead.
    """
    if any(character in value for character in ('"', "\\", "\n", "\r", "\t")):
        raise Refused(f"a variant name is not writable as an FBX ASCII string: {value!r}")
    return value


def read_document(lines: list[str]) -> dict:
    """What the production file says about itself.

    Object numbers, the node each `Model` is, the `Geometry` and `Material`
    objects connected to each `Model`, and where the three sections this
    transformer is allowed to touch begin and end.
    """
    objects: dict[int, dict] = {}
    model_of_node: dict[str, int] = {}
    order: list[int] = []
    current: int | None = None
    definitions_total: int | None = None
    model_count_line: int | None = None
    total_count_line: int | None = None
    objects_end: int | None = None
    connections_start: int | None = None
    connections_end: int | None = None
    section: str | None = None
    object_type: str | None = None
    last_model_end: int | None = None

    for index, line in enumerate(lines):
        if line == "Definitions: {":
            section = "definitions"
            continue
        if line == "Objects: {":
            section = "objects"
            continue
        if line == "Connections: {":
            section = "connections"
            connections_start = index
            continue
        if line == "}" and section == "objects":
            objects_end = index
            section = None
            continue
        if line == "}" and section == "connections":
            connections_end = index
            section = None
            continue

        if section == "definitions":
            kind = OBJECT_TYPE.match(line)
            if kind is not None:
                object_type = kind.group("name")
                continue
            count = COUNT.match(line)
            if count is not None:
                if object_type is None:
                    definitions_total = int(count.group("value"))
                    total_count_line = index
                elif object_type == "Model":
                    model_count_line = index
                continue

        header = HEADER.match(line)
        if header is not None:
            number = int(header.group("number"))
            if number in objects:
                raise Refused(f"the file numbers object {number} twice")
            objects[number] = {
                "class": header.group("class"),
                "subclass": header.group("subclass"),
                "line": index,
                "end": None,
                "name": header.group("name"),
                "node_key": None,
                "properties_end": None,
                "indent": "\t\t\t",
            }
            order.append(number)
            current = number
            continue
        if current is None:
            continue
        if line == "\t}":
            objects[current]["end"] = index
            if objects[current]["class"] == "Model":
                last_model_end = index
            current = None
            continue
        key = NODE_KEY.match(line)
        if key is not None:
            objects[current]["node_key"] = key.group("key")
            model_of_node[key.group("key")] = current
        definition = DEFINITION_KEY.match(line)
        if definition is not None:
            # The last FerriteCAD property the writer emits for a node that is
            # not omitted; the omission properties follow it. Appending after
            # this line keeps a variant's properties beside the production ones
            # rather than after an unrelated block.
            objects[current]["properties_end"] = index
            objects[current]["indent"] = definition.group("indent")

    for name, value in (
        ("Definitions.Count", definitions_total),
        ("Definitions.ObjectType(Model).Count", model_count_line),
        ("Objects", objects_end),
        ("Connections", connections_start),
        ("Connections end", connections_end),
        ("the last Model block", last_model_end),
    ):
        if value is None:
            raise Refused(f"the production file has no {name} this transformer can read")

    geometry_models: dict[int, list[int]] = {}
    material_slots: dict[int, list[tuple[int, int]]] = {}
    slots_of_model: dict[int, int] = {}
    parent_of_model: dict[int, int] = {}
    connection_lines: list[tuple[int, int, int]] = []
    for index, line in enumerate(lines):
        connection = CONNECTION.match(line)
        if connection is None:
            continue
        child = int(connection.group("child"))
        parent = int(connection.group("parent"))
        connection_lines.append((index, child, parent))
        if child not in objects:
            continue
        if objects[child]["class"] == "Model":
            parent_of_model[child] = parent
            continue
        if parent not in objects or objects[parent]["class"] != "Model":
            continue
        if objects[child]["class"] == "Geometry":
            geometry_models.setdefault(child, []).append(parent)
        elif objects[child]["class"] == "Material":
            slot = slots_of_model.get(parent, 0)
            slots_of_model[parent] = slot + 1
            material_slots.setdefault(child, []).append((parent, slot))

    return {
        "objects": objects,
        "order": order,
        "model_of_node": model_of_node,
        "geometry_models": geometry_models,
        "material_slots": material_slots,
        "parent_of_model": parent_of_model,
        "connection_lines": connection_lines,
        "definitions_total": definitions_total,
        "model_count_line": model_count_line,
        "total_count_line": total_count_line,
        "objects_end": objects_end,
        "connections_start": connections_start,
        "connections_end": connections_end,
        "last_model_end": last_model_end,
    }


def node_facts(manifest: dict, file_name: str) -> dict[str, dict]:
    for document in manifest["documents"]:
        if document["file"] == file_name:
            return {node["node_key"]: node for node in document["nodes"]}
    raise Refused(f"the manifest says nothing about {file_name}")


def definition_token(node: dict) -> str:
    return SEPARATOR.join(("fcad", node["source"], node["definition_key"]))


def added_properties(node: dict, role: str, occurrence: bool) -> list[tuple[str, str]]:
    """The identity every variant carries invisibly.

    §22B-1e2a established that the production `FerriteCADDefinitionKey` cannot
    tell two sources apart and that FerriteCAD persists no occurrence identity
    at all. Both halves are supplied here as custom properties so the graph
    question is asked with the join already possible — otherwise every variant
    would come back `ambiguous_join` and the graph would never be measured.
    """
    properties = [
        ("FerriteCADSourceId", node["source"]),
        ("FerriteCADDefinitionId", node["definition_id"]),
        ("FerriteCADGraphRole", role),
    ]
    if occurrence:
        properties.append(("FerriteCADOccurrenceId", node["occurrence"]))
    return properties


def model_block(
    number: int,
    name: str,
    subclass: str,
    node_key: str,
    node: dict,
    role: str,
    occurrence: bool,
) -> list[str]:
    """One new `Model` block, written the way the production writer writes one.

    The shape is copied from the writer's output — `Version: 232`, the three
    `Lcl` properties, `Shading`, `Culling`, and `TypeFlags` only for a `Null` —
    because a block Unity read differently from a production block would make
    every difference below unattributable. The transform is the identity, and
    the oracle checks that against the control.
    """
    lines = [
        f'\t{"Model"}: {number}, "Model::{escape(name)}", "{subclass}" {{',
        "\t\tVersion: 232",
        "\t\tProperties70: {",
        '\t\t\tP: "Lcl Translation", "Lcl Translation", "", "A", 0.0, 0.0, 0.0',
        '\t\t\tP: "Lcl Rotation", "Lcl Rotation", "", "A", 0.0, 0.0, 0.0',
        '\t\t\tP: "Lcl Scaling", "Lcl Scaling", "", "A", 1.0, 1.0, 1.0',
        f'\t\t\tP: "FerriteCADNodeKey", "KString", "", "U", "{escape(node_key)}"',
        '\t\t\tP: "FerriteCADDefinitionKey", "KString", "", "U", '
        f'"{escape(node["definition_key"])}"',
    ]
    for key, value in added_properties(node, role, occurrence):
        lines.append(f'\t\t\tP: "{key}", "KString", "", "U", "{escape(value)}"')
    lines.append("\t\t}")
    lines.append("\t\tShading: 1")
    lines.append('\t\tCulling: "CullingOff"')
    if subclass == "Null":
        lines.append('\t\tTypeFlags: "Null"')
    lines.append("\t}")
    return lines


def plan_variant(document: dict, facts: dict[str, dict], variant: str, file_name: str) -> dict:
    """What this variant adds and re-points, decided before a byte is written."""
    objects = document["objects"]
    node_of_model = {number: key for key, number in document["model_of_node"].items()}
    models = [number for number in document["order"] if objects[number]["class"] == "Model"]
    if len(models) != len(facts):
        raise Refused(
            f"{file_name}: the file has {len(models)} models and the manifest describes "
            f"{len(facts)} nodes"
        )
    for key in facts:
        if key not in document["model_of_node"]:
            raise Refused(f"{file_name}: no Model in the file carries {key}")

    added: list[dict] = []
    connections: list[tuple[int, int]] = []
    # Geometry and material connections that must move off an occurrence and
    # onto its new child, keyed by the line they occupy in the control.
    repointed: dict[int, int] = {}
    subclass_changes: dict[int, str] = {}
    next_number = ADDED_NUMBER_BASE

    if variant in ADDS_ONE_MODEL_PER_GEOMETRY:
        parented = variant == "g-carrier"
        for geometry in sorted(document["geometry_models"]):
            holders = document["geometry_models"][geometry]
            owners = {facts[node_of_model[model]]["definition_id"] for model in holders}
            if len(owners) != 1:
                raise Refused(
                    f"{file_name}: geometry {geometry} is placed by {len(owners)} different "
                    f"definitions, so it has no definition identity"
                )
            node = facts[node_of_model[holders[0]]]
            number = next_number
            next_number += 1
            added.append(
                {
                    "number": number,
                    "name": SEPARATOR.join((definition_token(node), "def")),
                    "subclass": "Mesh",
                    # The source-qualified identity, not the local key: two
                    # definitions in these documents share the local key on
                    # purpose, and two carriers with one node key would make
                    # the probe's own join the thing being measured.
                    "node_key": "carrier/" + node["definition_id"],
                    "node": node,
                    "role": "definition_carrier",
                    "occurrence": False,
                }
            )
            # First, so a reader that names a shared mesh after the first
            # `Model` that claims it names it after the definition.
            connections.append((geometry, number))
            if parented:
                connections.append((number, 0))

    if variant in ADDS_ONE_MODEL_PER_MESH_NODE:
        for model in models:
            key = node_of_model[model]
            node = facts[key]
            geometries = [
                geometry
                for geometry, holders in document["geometry_models"].items()
                if model in holders
            ]
            if not geometries:
                continue
            number = next_number
            next_number += 1
            added.append(
                {
                    "number": number,
                    "name": SEPARATOR.join((definition_token(node), "geo")),
                    "subclass": "Mesh",
                    "node_key": key + "/geo",
                    "node": node,
                    "role": "geometry_child",
                    "occurrence": False,
                }
            )
            connections.append((number, model))
            # The occurrence keeps its transform and its human name and stops
            # bearing geometry, which is the whole of this variant.
            subclass_changes[model] = "Null"
            for line, child, parent in document["connection_lines"]:
                if parent != model or child not in objects:
                    continue
                if objects[child]["class"] in ("Geometry", "Material"):
                    repointed[line] = number

    for item in added:
        if item["number"] in objects:
            raise Refused(f"{file_name}: an added object number collides with {item['number']}")

    return {
        "added": added,
        "connections": connections,
        "repointed": repointed,
        "subclass_changes": subclass_changes,
        "node_of_model": node_of_model,
        "models": models,
    }


def rewrite(lines: list[str], manifest: dict, file_name: str, variant: str) -> list[str]:
    document = read_document(lines)
    facts = node_facts(manifest, file_name)
    plan = plan_variant(document, facts, variant, file_name)
    objects = document["objects"]
    node_of_model = plan["node_of_model"]

    expected_added = len(plan["added"])
    if variant in ADDS_ONE_MODEL_PER_GEOMETRY and expected_added != len(
        document["geometry_models"]
    ):
        raise Refused(f"{file_name}: {variant} did not add exactly one carrier per geometry")

    # The closing brace of every occurrence this variant turned into a `Null`.
    # A production `Null` ends `Shading`, `Culling`, `TypeFlags`, so the line
    # goes immediately before that brace and the block keeps the writer's own
    # shape.
    null_block_ends = {
        objects[number]["end"]
        for number, subclass in plan["subclass_changes"].items()
        if subclass == "Null"
    }
    properties_end_of = {
        objects[number]["properties_end"]: number for number in plan["models"]
    }

    result: list[str] = []
    for index, line in enumerate(lines):
        # ---- Definitions: only the two counts, and only by the number added.
        if index == document["total_count_line"]:
            result.append(f'\tCount: {document["definitions_total"] + expected_added}')
            continue
        if index == document["model_count_line"]:
            count = COUNT.match(line)
            if count is None:
                raise Refused(f"{file_name}: the Model count is not where it was read from")
            result.append(f'{count.group("indent")}Count: {int(count.group("value")) + expected_added}')
            continue

        # ---- Objects: the subclass word of an occurrence that stops bearing
        # geometry, and the custom properties every variant carries.
        header = HEADER.match(line)
        if header is not None and int(header.group("number")) in plan["subclass_changes"]:
            number = int(header.group("number"))
            result.append(
                f'{header.group("indent")}{header.group("class")}: {number}, '
                f'"{header.group("class")}::{header.group("name")}", '
                f'"{plan["subclass_changes"][number]}" {{'
            )
            continue

        # ---- Connections: re-point a moved geometry or material.
        if index in plan["repointed"]:
            connection = CONNECTION.match(line)
            if connection is None:
                raise Refused(f"{file_name}: a re-pointed line is not a connection")
            result.append(
                f'\tC: "OO", {connection.group("child")}, {plan["repointed"][index]}'
            )
            continue

        if index in null_block_ends:
            result.append('\t\tTypeFlags: "Null"')
        result.append(line)

        number = properties_end_of.get(index)
        if number is not None:
            node = facts[node_of_model[number]]
            indent = objects[number]["indent"]
            for key, value in added_properties(node, "occurrence", occurrence=True):
                result.append(f'{indent}P: "{key}", "KString", "", "U", "{escape(value)}"')

        # ---- Objects: the new blocks, after the last existing `Model`.
        if index == document["last_model_end"]:
            for item in plan["added"]:
                result.extend(
                    model_block(
                        item["number"],
                        item["name"],
                        item["subclass"],
                        item["node_key"],
                        item["node"],
                        item["role"],
                        item["occurrence"],
                    )
                )

        # ---- Connections: the new lines, at the top of the section so a
        # carrier's claim on a geometry precedes every occurrence's.
        if index == document["connections_start"]:
            for child, parent in plan["connections"]:
                result.append(f'\tC: "OO", {child}, {parent}')

    return result


def read_lines(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    if not text.endswith("\n"):
        raise Refused(f"{path.name}: the production writer's last line is unterminated")
    return text[:-1].split("\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--documents", type=Path, required=True, help="the production bytes")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--variant", choices=VARIANTS, required=True)
    args = parser.parse_args()

    manifest = json.loads((args.documents / "manifest.json").read_text(encoding="utf-8"))
    if manifest["schema"] != "ferritecad.fbx-channel-manifest.v1":
        raise SystemExit("the manifest is not the one this transformer reads")
    args.output.mkdir(parents=True, exist_ok=True)

    written = 0
    for document in manifest["documents"]:
        name = document["file"]
        source = args.documents / name
        destination = args.output / name
        if args.variant == "g-flat":
            # Copied, not rewritten. The control has to be the production bytes
            # themselves or it is not a control.
            shutil.copyfile(source, destination)
            written += 1
            continue
        rewritten = rewrite(read_lines(source), manifest, name, args.variant)
        destination.write_text("\n".join(rewritten) + "\n", encoding="utf-8", newline="\n")
        written += 1
    print(f"graph {args.variant}: {written} documents")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Refused as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        raise SystemExit(1) from refusal
