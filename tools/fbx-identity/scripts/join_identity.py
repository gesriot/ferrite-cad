#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Joins the identity properties an independent reader found with the identities
the document recorded.

Two readings, from two programs that share nothing. One side is pinned `ufbx`
reading the shipped FBX from the outside and printing every node's identity
properties verbatim. The other is the payload the document was asked for
directly, in raw fields that carry no wire encoding at all.

The §22B-1e3b grammar is implemented here, from the specification and not from
the writer's source. That is the point of the join: agreement means two
independent implementations of one written rule produced the same string, and
disagreement names which node and which half.

Grammar, version 1:

    value       = "fcad1" ":" domain ":" kind *( ":" field )
    domain      = "def" / "occ"
    kind        = "object" / "source" / "place"
    field       = *( unreserved / pct-encoded )
    unreserved  = ALPHA / DIGIT / "-" / "." / "_" / "~"
    pct-encoded = "%" UPPER-HEX UPPER-HEX      ; one UTF-8 byte

    definition, native body   fcad1:def:object:<object id>
    definition, imported      fcad1:def:source:<source id>:<definition key>
    placement, native body    fcad1:occ:object:<object id>
    placement, imported       fcad1:occ:place:<occurrence id>

An identity a document never recorded has no property at all. There is no
spelling for "absent" inside a value, because a value that could say it would
be a value invented for a document that recorded nothing.
"""

from __future__ import annotations

import argparse
import sys

UNRESERVED = frozenset(
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~"
)


def escape(field: str) -> str:
    """Percent-escapes one field of an identity value.

    Every byte outside the unreserved alphabet becomes ``%XX`` with uppercase
    hexadecimal, over the UTF-8 encoding of the text. Injective and canonical:
    no input is truncated, no input is hashed, and each input has exactly one
    encoding, so two different identities can never produce one value.
    """
    out = []
    for byte in field.encode("utf-8"):
        character = chr(byte)
        if character in UNRESERVED:
            out.append(character)
        else:
            out.append(f"%{byte:02X}")
    return "".join(out)


def definition_value(source: str, key: str) -> str:
    return f"fcad1:def:source:{escape(source)}:{escape(key)}"


def occurrence_value(occurrence: str) -> str:
    return f"fcad1:occ:place:{escape(occurrence)}"


def read_payload(path: str) -> list[dict[str, str]]:
    entries = []
    with open(path, encoding="utf-8") as handle:
        for number, line in enumerate(handle.read().splitlines()):
            fields = line.split("\t")
            if len(fields) != 4:
                raise SystemExit(f"{path}:{number + 1}: expected four fields")
            entries.append(
                {
                    "name": fields[0],
                    "key": fields[1],
                    "source": fields[2],
                    "occurrence": fields[3],
                }
            )
    return entries


def read_reader(path: str) -> tuple[list[dict[str, str]], dict[str, int]]:
    rows = []
    summary: dict[str, int] = {}
    with open(path, encoding="utf-8") as handle:
        for line in handle.read().splitlines():
            if line.startswith("FCAD_IDENTITY\t"):
                _, key, name, definition, occurrence = line.split("\t")
                rows.append(
                    {
                        "key": key,
                        "name": name,
                        "definition": definition,
                        "occurrence": occurrence,
                    }
                )
            elif line.startswith("FCAD_IDENTITY_SUMMARY "):
                for pair in line.split(" ")[1:]:
                    field, value = pair.split("=")
                    summary[field] = int(value)
    return rows, summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reader", required=True, help="the ufbx reader's output")
    parser.add_argument("--payload", required=True, help="what the document recorded")
    parser.add_argument(
        "--expect-nodes", type=int, required=True, help="how many placements there are"
    )
    parser.add_argument(
        "--expect-definitions",
        type=int,
        required=True,
        help="how many distinct definitions there are",
    )
    arguments = parser.parse_args()

    payload = read_payload(arguments.payload)
    rows, summary = read_reader(arguments.reader)
    failures: list[str] = []

    if not summary:
        raise SystemExit("the reader printed no summary, so it did not run to the end")
    if len(rows) != arguments.expect_nodes:
        failures.append(f"the file has {len(rows)} nodes, expected {arguments.expect_nodes}")
    if len(payload) != arguments.expect_nodes:
        failures.append(
            f"the document recorded {len(payload)} placements, "
            f"expected {arguments.expect_nodes}"
        )

    # Joined on the writer's positional node key, not on the order this reader
    # happened to walk the file in: ufbx presents nodes in whatever order it
    # likes, and a join that depended on that would be measuring the reader.
    by_key = {}
    for row in rows:
        if row["key"] in by_key:
            failures.append(f"two nodes carry the node key {row['key']}")
        by_key[row["key"]] = row

    joined = 0
    for position, recorded in enumerate(payload):
        key = f"node/{position}"
        row = by_key.get(key)
        if row is None:
            failures.append(f"placement {position} has no node carrying {key}")
            continue
        where = f"{key} ({row['name']!r})"
        if row["name"] != recorded["name"]:
            failures.append(f"{where}: the document calls this placement {recorded['name']!r}")
            continue
        wanted_occurrence = occurrence_value(recorded["occurrence"])
        wanted_definition = definition_value(recorded["source"], recorded["key"])
        agreed = True
        if row["occurrence"] == "-":
            failures.append(f"{where}: carries no FerriteCADOccurrenceId")
            agreed = False
        elif row["occurrence"] != wanted_occurrence:
            failures.append(
                f"{where}: carries {row['occurrence']} and the document recorded "
                f"{wanted_occurrence}"
            )
            agreed = False
        if row["definition"] == "-":
            failures.append(f"{where}: carries no FerriteCADDefinitionId")
            agreed = False
        elif row["definition"] != wanted_definition:
            failures.append(
                f"{where}: carries {row['definition']} and the document recorded "
                f"{wanted_definition}"
            )
            agreed = False
        if agreed:
            joined += 1

    distinct_occurrences = {row["occurrence"] for row in rows if row["occurrence"] != "-"}
    distinct_definitions = {row["definition"] for row in rows if row["definition"] != "-"}
    if len(distinct_occurrences) != arguments.expect_nodes:
        failures.append(
            f"{len(distinct_occurrences)} distinct placement identities for "
            f"{arguments.expect_nodes} placements"
        )
    if len(distinct_definitions) != arguments.expect_definitions:
        failures.append(
            f"{len(distinct_definitions)} distinct definition identities for "
            f"{arguments.expect_definitions} definitions"
        )
    if distinct_occurrences & distinct_definitions:
        failures.append("a definition identity and a placement identity are the same value")

    for failure in failures:
        print(f"FAIL {failure}", file=sys.stderr)
    print(
        f"FCAD_FBX_IDENTITY_JOIN nodes={len(rows)} joined={joined} "
        f"definitions={len(distinct_definitions)} occurrences={len(distinct_occurrences)} "
        f"failures={len(failures)}"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
