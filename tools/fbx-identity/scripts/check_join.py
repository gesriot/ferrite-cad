#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The gate on the independent join, and on the grammar it implements.

`join_identity.py` is the second reading of the §22B-1e3b wire contract: it
implements the written grammar from the specification, not from the writer, so
that agreement between the file and the document means two implementations of
one rule produced one string. That only means something while the joiner can
still tell a wrong file from a right one, and every way it could stop being
able to compiles and runs.

So this hands it one transcript that must be accepted and one per defect that
must be refused — and for each refusal it requires the message that names *that*
defect, rather than merely a non-zero exit. Requiring the message is the whole
point: a joiner with two overlapping comparisons of one fact would refuse a bad
transcript whichever of the two was removed, and a gate that only looked at the
exit status would call that healthy.

Needs nothing but Python: no editor, no kernel, no compiler. It runs on every
push.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
JOINER = HERE / "join_identity.py"

SOURCE = "019ffc72-1e3b-7000-8000-000000000001"
OTHER_SOURCE = "019ffc72-1e3b-7000-8000-000000000002"
PLACES = [
    "019ffc72-1e3b-7000-8000-0000000000a0",
    "019ffc72-1e3b-7000-8000-0000000000a1",
    "019ffc72-1e3b-7000-8000-0000000000a2",
]
# One key that needs escaping, one that does not, and one that is a second
# source's — so the definition half has something to be wrong about.
KEYS = [
    ("step.product_definition#1", "step.product_definition%231", SOURCE),
    ("a:b", "a%3Ab", SOURCE),
    ("step.product_definition#1", "step.product_definition%231", OTHER_SOURCE),
]
NAMES = ["Root", "Part", "Other"]


def payload() -> str:
    lines = []
    for index, (key, _, source) in enumerate(KEYS):
        lines.append(f"{NAMES[index]}\t{key}\t{source}\t{PLACES[index]}")
    return "".join(line + "\n" for line in lines)


def row(index: int, *, name=None, definition=None, occurrence=None, key=None) -> str:
    _, escaped, source = KEYS[index]
    if definition is None:
        definition = f"fcad1:def:source:{source}:{escaped}"
    if occurrence is None:
        occurrence = f"fcad1:occ:place:{PLACES[index]}"
    if name is None:
        name = NAMES[index]
    if key is None:
        key = f"node/{index}"
    return f"FCAD_IDENTITY\t{key}\t{name}\t{definition}\t{occurrence}\n"


def transcript(rows: list[str], *, summary: bool = True) -> str:
    text = "reader ufbx 0.23.0 strict\n" + "".join(rows)
    if summary:
        text += (
            f"FCAD_IDENTITY_SUMMARY nodes={len(rows)} definitions={len(rows)} "
            f"occurrences={len(rows)} meshes=0\n"
        )
    return text


def good() -> list[str]:
    return [row(index) for index in range(len(KEYS))]


# Every case: what the transcript says, and what the joiner must say about it.
# `None` means the transcript is correct and must be accepted.
CASES: list[tuple[str, str, str | None]] = [
    ("a file that carries what the document recorded", transcript(good()), None),
    (
        "a reader that found no node at all",
        transcript([], summary=True),
        "has no node carrying node/0",
    ),
    (
        "a reader that did not run to the end",
        transcript(good(), summary=False),
        "printed no summary",
    ),
    (
        "two nodes carrying one node key",
        transcript([row(0), row(1, key="node/0"), row(2)]),
        "two nodes carry the node key node/0",
    ),
    (
        "a placement the document calls something else",
        transcript([row(0, name="Renamed"), row(1), row(2)]),
        "the document calls this placement",
    ),
    (
        "two placements carrying each other's identities",
        transcript(
            [
                row(0, occurrence=f"fcad1:occ:place:{PLACES[1]}"),
                row(1, occurrence=f"fcad1:occ:place:{PLACES[0]}"),
                row(2),
            ]
        ),
        "and the document recorded fcad1:occ:place:",
    ),
    (
        "a placement carrying another definition's identity",
        transcript(
            [
                row(0, definition=f"fcad1:def:source:{OTHER_SOURCE}:x"),
                row(1),
                row(2),
            ]
        ),
        "and the document recorded fcad1:def:source:",
    ),
    (
        "a placement carrying no placement identity",
        transcript([row(0, occurrence="-"), row(1), row(2)]),
        "carries no FerriteCADOccurrenceId",
    ),
    (
        "a placement carrying no definition identity",
        transcript([row(0, definition="-"), row(1), row(2)]),
        "carries no FerriteCADDefinitionId",
    ),
    (
        "two placements answering to one identity",
        transcript(
            [
                row(0),
                row(1, occurrence=f"fcad1:occ:place:{PLACES[0]}"),
                row(2),
            ]
        ),
        "distinct placement identities",
    ),
    (
        "two definitions answering to one identity",
        transcript([row(0), row(1), row(2, definition=f"fcad1:def:source:{SOURCE}:a%3Ab")]),
        "distinct definition identities",
    ),
    (
        "one value in both domains at once",
        transcript(
            [
                row(0, definition=f"fcad1:occ:place:{PLACES[1]}"),
                row(1),
                row(2),
            ]
        ),
        "are the same value",
    ),
]


def main() -> int:
    work = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("/tmp")
    work.mkdir(parents=True, exist_ok=True)
    payload_path = work / "join-payload"
    payload_path.write_text(payload(), encoding="utf-8")

    checked = 0
    failures = 0
    for what, text, expected in CASES:
        reader = work / "join-reader"
        reader.write_text(text, encoding="utf-8")
        run = subprocess.run(
            [
                sys.executable,
                str(JOINER),
                "--reader",
                str(reader),
                "--payload",
                str(payload_path),
                "--expect-nodes",
                str(len(KEYS)),
                "--expect-definitions",
                str(len(KEYS)),
            ],
            capture_output=True,
            text=True,
        )
        output = run.stdout + run.stderr
        checked += 1
        if expected is None:
            if run.returncode != 0:
                print(f"FAIL the join refused {what}:\n{output}", file=sys.stderr)
                failures += 1
            continue
        if run.returncode == 0:
            print(f"FAIL the join accepted {what}", file=sys.stderr)
            failures += 1
        elif expected not in output:
            # Refused, but not for the reason this case exists to provoke. That
            # is a joiner whose checks overlap, and a gate that accepted it
            # would let one of them be deleted unnoticed.
            print(
                f"FAIL the join refused {what} without naming it; expected "
                f"{expected!r} in:\n{output}",
                file=sys.stderr,
            )
            failures += 1

    print(f"FCAD_IDENTITY_JOIN_CHECKED {checked} failures={failures}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
