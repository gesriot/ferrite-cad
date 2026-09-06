#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Rebuilds the §22B-1e3b decision record from what the editor actually did.

The probe inside Unity refuses on the spot when the channel is missing, the
join is ambiguous, a designation moved or the hierarchy moved. This reads the
same report from outside and answers the questions a decision record has to
answer whether they pass or fail — including the one that must *not* be turned
into an assertion: how many serialised local file identifiers moved between the
two imports.

That number is reported and not judged. §22B-1e2a and §22B-1e2b measured what a
`fileID` is in this editor — a function of the visible name plus the type plus a
collision counter, or of the hierarchy path — and no property in a file changes
that. A record that demanded stability here would be a record that had to be
either wrong or quietly weakened later, so it says what happened instead.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

DEFINITION_PREFIX = "fcad1:def:"
OCCURRENCE_PREFIX = "fcad1:occ:"


def one(report: dict, name: str) -> dict:
    for entry in report["files"]:
        if entry["file"] == name:
            return entry
    raise SystemExit(f"the report has no file called {name}")


def decide(report: dict) -> dict:
    """The whole decision record, questions and failures together.

    Importable so the mutation campaign beside this file can feed the real
    verifier a perturbed measurement rather than a re-implementation of it.
    """
    current = one(report, "fcad-measured.fbx")
    legacy = one(report, "fcad-legacy.fbx")
    keyed = one(report, "fcad-identity-escaping.fbx")
    renamed = one(report, "fcad-renamed.fbx")
    against = report["against_legacy"]
    across = report["across_a_rename"]

    failures: list[str] = []

    def ask(question: str, answer: bool) -> bool:
        if not answer:
            failures.append(question)
        return answer

    nodes = current["nodes"]
    if not nodes:
        raise SystemExit("the editor imported no object at all, so nothing was measured")

    # Every value the editor handed back is in the domain it was written in, and
    # the two domains stayed apart.
    definitions = {node["definition_id"] for node in nodes}
    occurrences = {node["occurrence_id"] for node in nodes}
    domains_kept = all(value.startswith(DEFINITION_PREFIX) for value in definitions) and all(
        value.startswith(OCCURRENCE_PREFIX) for value in occurrences
    )

    # The join a project would actually make: one placement per value, and one
    # value per placement.
    unambiguous = len(occurrences) == len(nodes) and not (definitions & occurrences)

    # A definition placed more than once is what makes the definition half worth
    # having; without one the join is trivially unambiguous.
    shared = len(nodes) - len(definitions)

    # One shared Mesh per definition identity, in the editor's own object graph.
    meshes: dict[str, set[str]] = {}
    for node in nodes:
        if node["mesh"]:
            meshes.setdefault(node["definition_id"], set()).add(node["mesh"])
    geometry_shared = all(len(seen) == 1 for seen in meshes.values())

    decision = {
        "unity_version": report["unity_version"],
        "checks": report["checks"],
        "objects": len(nodes),
        "answered": {
            "a vanilla ModelImporter reads both properties on every object": ask(
                "the channel is readable",
                current["objects_with_definition_id"] == len(nodes)
                and current["objects_with_occurrence_id"] == len(nodes),
            ),
            "every value stayed in the domain it was written in": ask(
                "the domains stayed apart", domains_kept
            ),
            "the join is unambiguous": ask("the join is unambiguous", unambiguous),
            "placements of one definition share one Mesh": ask(
                "geometry sharing survived", geometry_shared
            ),
            "the designations a person reads did not move": ask(
                "designations survived",
                against["names_that_moved"] == 0
                and against["roots_excluded_from_the_name_comparison"] == 1,
            ),
            "the hierarchy did not move": ask(
                "the hierarchy survived", against["parents_that_moved"] == 0
            ),
            "the geometry and material bindings did not move": ask(
                "bindings survived",
                against["mesh_bindings_that_moved"] == 0
                and against["material_counts_that_moved"] == 0,
            ),
            "the properties §22B-1b2 already wrote did not move": ask(
                "the existing properties survived",
                against["existing_properties_that_moved"] == 0,
            ),
            "the import says nothing it did not say before": ask(
                "the diagnostics did not change",
                current["identifier_uniqueness_violations"]
                == legacy["identifier_uniqueness_violations"]
                and current["import_errors"] == 0
                and legacy["import_errors"] == 0,
            ),
            "a layout that recorded nothing carries no property": ask(
                "the legacy layout is honest",
                legacy["objects_with_definition_id"] == 0
                and legacy["objects_with_occurrence_id"] == 0,
            ),
            "a placement is found again by its identity after every designation "
            "changed": ask(
                "the identity outlived the rename",
                across["joined_by_placement_identity"] == len(nodes)
                and across["designations_that_changed"]
                + across["roots_excluded_from_the_name_comparison"]
                == len(nodes)
                and len(renamed["nodes"]) == len(nodes),
            ),
            "an escaped key survives the editor unchanged": ask(
                "the escaping survived",
                keyed["objects_with_definition_id"] == len(keyed["nodes"])
                and all(
                    node["definition_id"].count(":") == 4 for node in keyed["nodes"]
                ),
            ),
        },
        "measured_and_not_judged": {
            "placements of one definition, in this scene": shared,
            "meshes": current["meshes"],
            "materials": current["materials"],
            "identifier uniqueness violations, current": current[
                "identifier_uniqueness_violations"
            ],
            "identifier uniqueness violations, legacy": legacy[
                "identifier_uniqueness_violations"
            ],
            "the editor sorts an imported hierarchy by name by default": current[
                "sorted_by_name_by_default"
            ],
            "the designation this editor gives the model root, current": current[
                "root_designation"
            ],
            "the designation this editor gives the model root, legacy": legacy[
                "root_designation"
            ],
            "GameObject local file identifiers that moved": against[
                "gameobject_file_ids_that_moved"
            ],
            "Mesh local file identifiers that moved": against["mesh_file_ids_that_moved"],
            "Material local file identifiers that moved": against[
                "material_file_ids_that_moved"
            ],
            "GameObject local file identifiers that moved across the rename": across[
                "gameobject_file_ids_that_moved"
            ],
            "meshes before and after the rename": [
                across["meshes_before"],
                across["meshes_after"],
            ],
            "materials before and after the rename": [
                across["materials_before"],
                across["materials_after"],
            ],
        },
        "limits": [
            "A local file identifier in this editor is a function of the visible name, "
            "the type and a collision counter, or of the hierarchy path. No property in "
            "an FBX changes that, so the three counts above are reported and none of "
            "them is asserted. The presence of the channel is not a claim that a "
            "serialised reference survives an edit.",
            "The existing Identifier uniqueness violation is the finding §22B-1e1 "
            "recorded and is not fixed here. What is measured is that the channel does "
            "not add one.",
            "No companion package, ScriptedImporter, .meta edit or AddRemap takes part. "
            "The probe reads and reports; it renames nothing and publishes nothing.",
            "A Mesh and a Material are sub-assets and carry no custom properties of "
            "their own: this editor hands properties to a callback about a GameObject "
            "and to nothing else. There is therefore no identity on them for anything "
            "to join two imports by, whatever the file says, and the rename comparison "
            "counts them rather than joining them.",
            "fcad-renamed.fbx is a neutral test variant built by naming different "
            "strings. It is not a STEP reimport: this build has no reimport semantics, "
            "and nothing here presents it as one.",
            "This editor names an imported model's root after the asset file rather "
            "than after anything the document said, so the root's designation differs "
            "between two files of two names however identical their contents. It is "
            "excluded from the designation comparison and reported instead. Its "
            "identity properties are read like any other object's.",
            "This editor sorts an imported hierarchy by name by default, which "
            "reorders it relative to the file. The probe turns that off, because the "
            "callback that hands over the custom properties sees the tree before the "
            "sort and the finished asset is the tree after it. The default is recorded "
            "above rather than hidden.",
        ],
        "failures": failures,
    }

    return decision


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--emit", type=Path, required=True)
    parser.add_argument("--expected", type=Path)
    arguments = parser.parse_args()

    report = json.loads(arguments.report.read_text(encoding="utf-8"))
    decision = decide(report)
    failures = decision["failures"]
    nodes = decision["objects"]
    shared = decision["measured_and_not_judged"]["placements of one definition, in this scene"]

    text = json.dumps(decision, indent=2, ensure_ascii=False, sort_keys=True) + "\n"
    arguments.emit.write_text(text, encoding="utf-8")

    if arguments.expected is not None:
        committed = arguments.expected.read_text(encoding="utf-8")
        if committed != text:
            raise SystemExit(
                f"the decision record differs from the committed one at {arguments.expected}"
            )

    for failure in failures:
        print(f"FAIL {failure}")
    print(
        f"FCAD_FILE_IDENTITY_DECISION objects={nodes} shared={shared} "
        f"failures={len(failures)}"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
