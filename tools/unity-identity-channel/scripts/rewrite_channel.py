#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The measurement-only channel rewriter for §22B-1e2a.

This is not a second FBX serializer and it must never become one. It is handed
the bytes the *production* writer produced — through the `fbx_channel_documents`
example, which calls `write_fbx_ascii_7400` and nothing else — and it changes
exactly two things inside them:

  * the name in an object's own header line, and
  * the custom properties inside a `Model`'s `Properties70` block.

Everything else, including every object number, every connection, every vertex
and the whole `Definitions` section, is copied through byte for byte. It writes
no geometry, no transform, no material colour and no header: a bug here can
rename an object or add a property, and cannot invent a document.

Which object belongs to which FerriteCAD definition is read out of the file
rather than assumed. A `Model` says which node it is through the
`FerriteCADNodeKey` property the production writer already emits; a `Geometry`
and a `Material` say which nodes they belong to through the `Connections`
section. The manifest supplies only what the writer does not put in the file
at all: the `ImportedSourceId` behind each definition, and the synthetic
persistent occurrence identity this measurement invented for the placement
experiment.

Candidates
----------

``a-control``
    The production bytes, copied. No rename, no added property. This is the
    current product and the control every other candidate is compared with.

``b-ordinal``
    Source-qualified machine identity in the FBX object name, with a placement
    identified by its ordinal among the placements of its definition — which is
    the only occurrence identity FerriteCAD persists today.

``b-occurrence``
    The same, with the placement identified by the synthetic persistent
    occurrence identity instead of by an ordinal. The single difference from
    ``b-ordinal`` is that one field, so the placement experiment measures the
    durable occurrence identity and nothing else.

``c-property``
    ``b-occurrence`` plus the human designations, carried only as custom
    properties. The FBX object names are identical to ``b-occurrence``'s, so
    what this candidate adds is recoverability, not a different identity.

``d-companion``
    Not a rewrite. It is ``c-property``'s bytes imported with the FerriteCAD
    companion postprocessor active, and it exists as a name here only so a
    report cannot describe it as a property of the file. This script refuses
    to write it.

The name probes
---------------

``--names`` writes one document per naming question instead of one per
candidate: a long source-qualified token, a non-ASCII designation of the shape
the real AP203 assembly actually contains, a short stable hash token, and a
deliberate collision between two distinct durable identities. They exist to be
measured, not to be proposed: the collision case is written *because* a hash
token cannot be claimed safe without one.
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

CANDIDATES = ("a-control", "b-ordinal", "b-occurrence", "c-property")

HEADER = re.compile(
    r'^(?P<indent>\t)(?P<class>Model|Geometry|Material): '
    r'(?P<number>-?\d+), "(?P=class)::(?P<name>.*)", "(?P<subclass>[^"]*)" \{$'
)
NODE_KEY = re.compile(r'^\t+P: "FerriteCADNodeKey", "KString", "", "U", "(?P<key>[^"]*)"$')
DEFINITION_KEY = re.compile(
    r'^(?P<indent>\t+)P: "FerriteCADDefinitionKey", "KString", "", "U", "(?P<key>[^"]*)"$'
)
CONNECTION = re.compile(r'^\tC: "OO", (?P<child>-?\d+), (?P<parent>-?\d+)$')


class Refused(Exception):
    """The file is not the one this rewriter was written for."""


def escape(value: str) -> str:
    """An FBX ASCII quoted string.

    Deliberately minimal: a name that needed more than this would be a name the
    rest of the measurement could not compare, so it is refused instead.
    """
    if any(character in value for character in ('"', "\\", "\n", "\r", "\t")):
        raise Refused(f"a candidate name is not writable as an FBX ASCII string: {value!r}")
    return value


def read_document(lines: list[str]) -> dict:
    """What the production file says about itself.

    Object numbers, the node each `Model` is, and the `Geometry` and `Material`
    objects connected to each `Model`, in file order.
    """
    objects: dict[int, dict] = {}
    model_of_node: dict[str, int] = {}
    current: int | None = None
    for index, line in enumerate(lines):
        header = HEADER.match(line)
        if header is not None:
            number = int(header.group("number"))
            if number in objects:
                raise Refused(f"the file numbers object {number} twice")
            objects[number] = {
                "class": header.group("class"),
                "line": index,
                "name": header.group("name"),
                "node_key": None,
                "properties_end": None,
            }
            current = number
            continue
        if current is None:
            continue
        key = NODE_KEY.match(line)
        if key is not None:
            objects[current]["node_key"] = key.group("key")
            model_of_node[key.group("key")] = current
        definition = DEFINITION_KEY.match(line)
        if definition is not None:
            # The last FerriteCAD property the writer emits for a node that is
            # not omitted; the omission properties follow it. Appending after
            # this line keeps a candidate's properties beside the production
            # ones rather than after an unrelated block.
            objects[current]["properties_end"] = index
            objects[current]["indent"] = definition.group("indent")

    geometry_models: dict[int, list[int]] = {}
    material_slots: dict[int, list[tuple[int, int]]] = {}
    slots_of_model: dict[int, int] = {}
    for line in lines:
        connection = CONNECTION.match(line)
        if connection is None:
            continue
        child = int(connection.group("child"))
        parent = int(connection.group("parent"))
        if child not in objects or parent not in objects:
            continue
        if objects[parent]["class"] != "Model":
            continue
        if objects[child]["class"] == "Geometry":
            geometry_models.setdefault(child, []).append(parent)
        elif objects[child]["class"] == "Material":
            slot = slots_of_model.get(parent, 0)
            slots_of_model[parent] = slot + 1
            material_slots.setdefault(child, []).append((parent, slot))

    return {
        "objects": objects,
        "model_of_node": model_of_node,
        "geometry_models": geometry_models,
        "material_slots": material_slots,
    }


def node_facts(manifest: dict, file_name: str) -> dict[str, dict]:
    for document in manifest["documents"]:
        if document["file"] == file_name:
            return {node["node_key"]: node for node in document["nodes"]}
    raise Refused(f"the manifest says nothing about {file_name}")


def ordinals(facts: dict[str, dict]) -> dict[str, int]:
    """Each placement's position among the placements of its definition.

    This is the occurrence identity FerriteCAD has today. It is computed here,
    from the file's own node order, so `b-ordinal` really is the current
    identity rather than a weakened copy of the durable one.
    """
    seen: dict[str, int] = {}
    result: dict[str, int] = {}
    for key in sorted(facts, key=lambda item: int(item.split("/")[1])):
        definition = facts[key]["definition_id"]
        result[key] = seen.get(definition, 0)
        seen[definition] = result[key] + 1
    return result


def definition_token(node: dict) -> str:
    return SEPARATOR.join(("fcad", node["source"], node["definition_key"]))


def rewrite(lines: list[str], manifest: dict, file_name: str, candidate: str) -> list[str]:
    document = read_document(lines)
    facts = node_facts(manifest, file_name)
    order = ordinals(facts)
    objects = document["objects"]

    models = {number for number, item in objects.items() if item["class"] == "Model"}
    if len(models) != len(facts):
        raise Refused(
            f"{file_name}: the file has {len(models)} models and the manifest describes "
            f"{len(facts)} nodes"
        )
    for key in facts:
        if key not in document["model_of_node"]:
            raise Refused(f"{file_name}: no Model in the file carries {key}")

    node_of_model = {number: key for key, number in document["model_of_node"].items()}

    names: dict[int, str] = {}
    for number in models:
        node = facts[node_of_model[number]]
        occurrence = (
            str(order[node_of_model[number]])
            if candidate == "b-ordinal"
            else node["occurrence"]
        )
        names[number] = SEPARATOR.join((definition_token(node), "occ", occurrence))

    for number, holders in document["geometry_models"].items():
        owners = {facts[node_of_model[model]]["definition_id"] for model in holders}
        if len(owners) != 1:
            raise Refused(
                f"{file_name}: geometry {number} is placed by {len(owners)} different "
                f"definitions, so it has no definition identity"
            )
        names[number] = SEPARATOR.join((definition_token(facts[node_of_model[holders[0]]]), "geom"))

    for number, bindings in document["material_slots"].items():
        owners = {
            (facts[node_of_model[model]]["definition_id"], slot) for model, slot in bindings
        }
        if len(owners) != 1:
            raise Refused(
                f"{file_name}: material {number} is bound to {len(owners)} different "
                f"definition slots, so it has no definition identity"
            )
        model, slot = bindings[0]
        names[number] = SEPARATOR.join(
            (definition_token(facts[node_of_model[model]]), "mat", str(slot))
        )

    result: list[str] = []
    for index, line in enumerate(lines):
        header = HEADER.match(line)
        if header is not None and int(header.group("number")) in names:
            number = int(header.group("number"))
            result.append(
                f'{header.group("indent")}{header.group("class")}: {number}, '
                f'"{header.group("class")}::{escape(names[number])}", '
                f'"{header.group("subclass")}" {{'
            )
            continue
        result.append(line)
        for number in models:
            if objects[number]["properties_end"] != index:
                continue
            node = facts[node_of_model[number]]
            indent = objects[number]["indent"]
            added = [
                ("FerriteCADSourceId", node["source"]),
                ("FerriteCADDefinitionId", node["definition_id"]),
            ]
            # `b-ordinal` deliberately carries no occurrence identity: it is
            # the candidate that keeps today's ordinal, and giving it one would
            # make the placement experiment compare two durable schemes.
            if candidate != "b-ordinal":
                added.append(("FerriteCADOccurrenceId", node["occurrence"]))
            if candidate == "c-property":
                added.append(("FerriteCADDisplayName", node["node_display_name"]))
                added.append(
                    ("FerriteCADGeometryDisplayName", node["definition_display_name"])
                )
                for slot, designation in enumerate(node["slots"]):
                    added.append((f"FerriteCADMaterialDisplayName{slot}", designation))
            for name, value in added:
                result.append(f'{indent}P: "{name}", "KString", "", "U", "{escape(value)}"')
    return result


# ------------------------------------------------------------- the name probes

# The designation the real AP203 assembly measured in §22B-1e1 actually
# carries. Used verbatim rather than invented, so the non-ASCII case is a real
# one.
CYRILLIC_DESIGNATION = "МГИФ.773754.239"

# `\u041a\u0440\u043e\u043d\u0448\u0442\u0435\u0439\u043d` with its `\u0439` as one code point, and the same word with
# `\u0438` followed by a combining breve. Two different byte strings that a
# normalising reader folds into one. Written as escapes so no editor and no
# filesystem can quietly normalise this file and turn the experiment into a
# comparison of a string with itself.
PRECOMPOSED = "\u041a\u0440\u043e\u043d\u0448\u0442\u0435\u0439\u043d"
DECOMPOSED = "\u041a\u0440\u043e\u043d\u0448\u0442\u0435\u0438\u0306\u043d"

NAME_PROBES = {
    "n01-ascii-source-qualified": "the source-qualified token the candidates use, unchanged",
    "n02-long-token": "the same token with 160 more ASCII characters after it",
    "n03-non-ascii": "the real assembly's Cyrillic designation inside the token",
    "n04-short-hash": "a 16-hex-digit FNV-1a token instead of the readable identity",
    "n05-hash-collision": "two distinct durable identities given one token on purpose",
    "n06-unicode-normalisation": "one designation precomposed, another decomposed",
}


def fnv1a64(value: str) -> str:
    """The same 64-bit FNV-1a the editor and the oracle compute, over UTF-8.

    A content token, not a security digest, and never presented as one.
    """
    digest = 0xCBF29CE484222325
    for byte in value.encode("utf-8"):
        digest ^= byte
        digest = (digest * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{digest:016x}"


def probe_name(probe: str, node: dict, base: str, role: str) -> str:
    """The candidate name for one object under one naming question."""
    if probe == "n01-ascii-source-qualified":
        return base
    if probe == "n02-long-token":
        return base + SEPARATOR + ("x" * 160)
    if probe == "n03-non-ascii":
        # Added to the token rather than substituted for the key: replacing the
        # key would make every definition of one source share a name, which is
        # the collision question and not the encoding one.
        return base.replace(
            "fcad" + SEPARATOR, "fcad" + SEPARATOR + CYRILLIC_DESIGNATION + SEPARATOR, 1
        )
    if probe == "n04-short-hash":
        return SEPARATOR.join(("fcad", fnv1a64(base)))
    if probe == "n05-hash-collision":
        # Both twins are given one token. That is the whole point: a token
        # scheme cannot be called collision-safe without measuring what the
        # editor does when two durable identities land on one name.
        if node["definition_key"] == "step.product_definition#42":
            return SEPARATOR.join(("fcad", "c011", role))
        return SEPARATOR.join(("fcad", fnv1a64(base)))
    if probe == "n06-unicode-normalisation":
        if node["definition_key"] == "step.product_definition#42":
            form = PRECOMPOSED if node["source"].endswith("a1") else DECOMPOSED
            return SEPARATOR.join(("fcad", form, role))
        return base
    raise Refused(f"unknown name probe {probe}")


def rewrite_names(lines: list[str], manifest: dict, file_name: str, probe: str) -> list[str]:
    """`b-occurrence`'s naming, put through one naming question.

    Only the names move. No property is added and none is removed, so a
    difference between two of these files is a difference in the name and
    nothing else.
    """
    document = read_document(lines)
    facts = node_facts(manifest, file_name)
    objects = document["objects"]
    node_of_model = {number: key for key, number in document["model_of_node"].items()}

    names: dict[int, str] = {}
    for number, item in objects.items():
        if item["class"] != "Model":
            continue
        node = facts[node_of_model[number]]
        base = SEPARATOR.join((definition_token(node), "occ", node["occurrence"]))
        names[number] = probe_name(probe, node, base, "occ")
    for number, holders in document["geometry_models"].items():
        node = facts[node_of_model[holders[0]]]
        base = SEPARATOR.join((definition_token(node), "geom"))
        names[number] = probe_name(probe, node, base, "geom")
    for number, bindings in document["material_slots"].items():
        model, slot = bindings[0]
        node = facts[node_of_model[model]]
        base = SEPARATOR.join((definition_token(node), "mat", str(slot)))
        names[number] = probe_name(probe, node, base, f"mat{slot}")

    result: list[str] = []
    for line in lines:
        header = HEADER.match(line)
        if header is not None and int(header.group("number")) in names:
            number = int(header.group("number"))
            result.append(
                f'{header.group("indent")}{header.group("class")}: {number}, '
                f'"{header.group("class")}::{escape(names[number])}", '
                f'"{header.group("subclass")}" {{'
            )
            continue
        result.append(line)
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
    parser.add_argument("--candidate", choices=CANDIDATES)
    parser.add_argument(
        "--names",
        action="store_true",
        help="write one document per naming question instead of one per document",
    )
    args = parser.parse_args()
    if (args.candidate is None) == (not args.names):
        raise SystemExit("give exactly one of --candidate and --names")

    manifest = json.loads((args.documents / "manifest.json").read_text(encoding="utf-8"))
    if manifest["schema"] != "ferritecad.fbx-channel-manifest.v1":
        raise SystemExit("the manifest is not the one this rewriter reads")
    args.output.mkdir(parents=True, exist_ok=True)

    if args.names:
        for probe in sorted(NAME_PROBES):
            lines = read_lines(args.documents / "base.fbx")
            rewritten = rewrite_names(lines, manifest, "base.fbx", probe)
            (args.output / f"{probe}.fbx").write_text(
                "\n".join(rewritten) + "\n", encoding="utf-8", newline="\n"
            )
        print(f"name probes: {len(NAME_PROBES)} documents")
        return 0

    written = 0
    for document in manifest["documents"]:
        name = document["file"]
        source = args.documents / name
        destination = args.output / name
        if args.candidate == "a-control":
            # Copied, not rewritten. The control has to be the production bytes
            # themselves or it is not a control.
            shutil.copyfile(source, destination)
            written += 1
            continue
        rewritten = rewrite(read_lines(source), manifest, name, args.candidate)
        destination.write_text("\n".join(rewritten) + "\n", encoding="utf-8", newline="\n")
        written += 1
    print(f"channel {args.candidate}: {written} documents")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Refused as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        raise SystemExit(1) from refusal
