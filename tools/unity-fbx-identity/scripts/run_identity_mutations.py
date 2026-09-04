#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Semantic mutants for the §22B-1e1 measurement.

Each mutant is a way this measurement could look like it had proved something
it had not. They are applied to copies of the recorded canonical measurement
and fed to the real join verifier, so a mutant survives only if the verifier
genuinely cannot see it.

The verifier is the gate under test here. The Unity probe itself is mutated
separately by `mutate_identity.sh`, which compiles and runs the editor.
"""

from __future__ import annotations

import copy
import json
from pathlib import Path

from verify_identity import Refused, transitions, verify

ROOT = Path(__file__).resolve().parent.parent
UNITY = json.loads((ROOT / "expected/identity-report.json").read_text(encoding="utf-8"))
ORACLE = json.loads((ROOT / "expected/identity-oracle-report.json").read_text(encoding="utf-8"))
PLAN = json.loads((ROOT / "expected/identity-plan.json").read_text(encoding="utf-8"))
TABLE = json.loads((ROOT / "expected/identity-transitions.json").read_text(encoding="utf-8"))


def scenario(unity: dict, name: str) -> dict:
    return next(item for item in unity["scenarios"] if item["name"] == name)


def reference(unity: dict, name: str, anchor: str) -> dict:
    return next(item for item in scenario(unity, name)["references"] if item["anchor"] == anchor)


def read(oracle: dict, name: str) -> dict:
    return next(item for item in oracle["files"] if item["file"] == name)


def expect_kill(name: str, mutate) -> None:
    unity = copy.deepcopy(UNITY)
    oracle = copy.deepcopy(ORACLE)
    plan = copy.deepcopy(PLAN)
    mutate(unity, oracle, plan)
    try:
        verify(unity, oracle, plan)
    except (Refused, KeyError, StopIteration, IndexError, TypeError) as error:
        print(f"killed: {name}: {type(error).__name__}: {error}")
        return
    raise SystemExit(f"survived unexpectedly: {name}")


def only_non_null(unity: dict, *_: dict) -> None:
    """A gate that accepts any resolved object as a kept reference."""
    item = reference(unity, "s03-display-name-only", "mesh:definition=step.product_definition#100")
    item["verdict"] = "same_semantic"


def local_id_taken_for_object_number(_: dict, oracle: dict, __: dict) -> None:
    """The FBX object number replaced by Unity's local file identifier."""
    for node in read(oracle, "base.fbx")["nodes"]:
        node["geometry_object_number"] = 8172736185020031444


def display_name_as_identity(unity: dict, *_: dict) -> None:
    """Meaning replaced by the display name, which repeats on purpose."""
    for item in scenario(unity, "s03-display-name-only")["references"]:
        item["semantic_before"] = item["name_before"]
        item["semantic_after"] = item["name_after"]
        item["verdict"] = "same_semantic"


def hierarchy_path_as_identity(unity: dict, *_: dict) -> None:
    """A node's durable key replaced by where it sits."""
    for side in ("before", "after"):
        for node in scenario(unity, "s01-byte-identical-reimport")[side]["nodes"]:
            node["definition_key"] = node["sibling_path"]


def mesh_result_sold_as_material(unity: dict, *_: dict) -> None:
    """One domain's answer reported for another."""
    for item in scenario(unity, "s04a-insert-earlier-definition")["references"]:
        if item["unity_type"] == "Material":
            item["unity_type"] = "Mesh"


def a_scenario_skipped(unity: dict, *_: dict) -> None:
    unity["scenarios"] = [
        item for item in unity["scenarios"] if item["name"] != "s05-reorder-definitions"
    ]


def shared_placements_split(_: dict, oracle: dict, __: dict) -> None:
    """Two placements of one geometry turned into two geometries."""
    facts = read(oracle, "base.fbx")["facts"]
    facts["placements_sharing_one_geometry"] = 0


def equal_names_collapsed(_: dict, oracle: dict, __: dict) -> None:
    """The document stops containing two definitions with one designation."""
    facts = read(oracle, "base.fbx")["facts"]
    facts["repeated_geometry_display_names"] = 0
    facts["repeated_sibling_names"] = 0


def silent_retarget_accepted(unity: dict, *_: dict) -> None:
    """A reference that now means another definition, called kept."""
    item = reference(unity, "s12-remove-one-definition", "object:step.product_definition#300@0")
    item["verdict"] = "same_semantic"


def warning_removed_from_report(unity: dict, *_: dict) -> None:
    scenario(unity, "s03-display-name-only")["warning_transition"] = ""


def prewritten_report(unity: dict, oracle: dict, _: dict) -> None:
    """The editor's report is not the one made from the bytes the reader saw."""
    scenario(unity, "s01-byte-identical-reimport")["before_fnv1a64"] = "0" * 16


def oracle_read_a_different_file(_: dict, oracle: dict, __: dict) -> None:
    base = read(oracle, "base.fbx")
    other = read(oracle, "renamed.fbx")
    base["nodes"] = other["nodes"]
    base["facts"] = other["facts"]


def sort_control_ignored(unity: dict, *_: dict) -> None:
    """The probe's importer setting is allowed to have moved identifiers."""
    unity["sort_control"]["identifiers_are_unchanged"] = False


def an_identifier_that_is_not_the_stored_pair(unity: dict, *_: dict) -> None:
    """One sub-asset whose GlobalObjectId is not the pair a project stores."""
    unity["subassets_whose_identifier_is_something_else"] = 1


def zero_checks(unity: dict, *_: dict) -> None:
    for item in unity["scenarios"]:
        item["references"] = []


def reference_judged_without_reloading(unity: dict, *_: dict) -> None:
    """The two independent resolutions are allowed to disagree."""
    item = reference(unity, "s01-byte-identical-reimport", "material:step.product_definition#100@0")
    item["resolved_by_stored_identifier"] = "<null>"


def stored_pair_is_not_the_object(unity: dict, *_: dict) -> None:
    """What a project file writes is no longer the object's identifier."""
    item = reference(unity, "s01-byte-identical-reimport", "mesh:definition=step.product_definition#50")
    item["stored_file_id"] = item["stored_file_id"] + 1


def missing_reference_called_resolved(unity: dict, *_: dict) -> None:
    item = reference(unity, "s03-display-name-only", "mesh:definition=step.product_definition#300")
    item["resolved_by_reloaded_asset"] = "Mesh:Alpha Part:1"
    item["resolved_by_stored_identifier"] = "Mesh:Alpha Part:1"


def node_count_disagreement_hidden(unity: dict, *_: dict) -> None:
    scenario(unity, "s01-byte-identical-reimport")["before"]["nodes"].pop()


MUTATIONS = [
    ("a_gate_that_only_checks_non_null", only_non_null),
    ("local_file_id_taken_for_the_fbx_object_number", local_id_taken_for_object_number),
    ("display_name_used_as_identity", display_name_as_identity),
    ("hierarchy_path_used_as_identity", hierarchy_path_as_identity),
    ("a_mesh_result_reported_as_a_material_result", mesh_result_sold_as_material),
    ("one_mandatory_scenario_skipped", a_scenario_skipped),
    ("shared_placements_turned_into_separate_meshes", shared_placements_split),
    ("two_equal_display_names_collapsed", equal_names_collapsed),
    ("a_silent_retarget_counted_as_a_kept_reference", silent_retarget_accepted),
    ("the_warning_dropped_from_the_report", warning_removed_from_report),
    ("a_prewritten_report_the_reader_never_saw", prewritten_report),
    ("the_oracle_reading_a_different_file", oracle_read_a_different_file),
    ("the_probe_setting_allowed_to_move_identifiers", sort_control_ignored),
    ("a_run_that_tracked_nothing", zero_checks),
    ("a_subasset_identifier_that_is_not_the_stored_pair", an_identifier_that_is_not_the_stored_pair),
    ("the_two_resolutions_allowed_to_disagree", reference_judged_without_reloading),
    ("the_stored_pair_no_longer_the_object_identity", stored_pair_is_not_the_object),
    ("a_missing_reference_called_resolved", missing_reference_called_resolved),
    ("the_editor_and_the_file_disagreeing_on_node_count", node_count_disagreement_hidden),
]


def main() -> int:
    checks = verify(copy.deepcopy(UNITY), copy.deepcopy(ORACLE), copy.deepcopy(PLAN))
    if checks == 0:
        raise SystemExit("the mutation baseline ran zero checks")
    table = transitions(copy.deepcopy(UNITY), copy.deepcopy(ORACLE), copy.deepcopy(PLAN))
    if table != TABLE:
        raise SystemExit("the committed transition table is not the one this measurement makes")
    print(f"mutation baseline: checks={checks}, transitions={len(TABLE['transitions'])}")

    for name, mutate in MUTATIONS:
        expect_kill(name, mutate)
    print(f"mutation campaign: {len(MUTATIONS)} runtime mutants killed")
    print("mutation campaign: 0 unexpected survivors")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
