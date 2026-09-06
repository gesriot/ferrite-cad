#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Semantic mutants for the §22B-1e3b Unity measurement.

Each mutant is a way this measurement could look like it had proved something
it had not. They are applied to copies of the recorded canonical measurement
and fed to the real decision verifier, so a mutant survives only if the
verifier genuinely cannot see it.

The verifier is what is under test here. The Unity probe itself is mutated
separately by `mutate_file.sh`, which compiles it and runs the real editor.

There are two kinds of control, and both are run. A perturbation that must be
noticed is a mutant; a perturbation that must *not* be noticed is a metamorphic
control, and a verifier that refused one would be a verifier that depends on
something it has no business depending on — which is exactly how a record ends
up asserting that a local file identifier is stable.
"""

from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from verify_file import decide  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
RECORDED = json.loads(
    (ROOT / "expected" / "file-report.json").read_text(encoding="utf-8")
)


def one(report: dict, name: str) -> dict:
    return next(entry for entry in report["files"] if entry["file"] == name)


def current(report: dict) -> dict:
    return one(report, "fcad-measured.fbx")


def legacy(report: dict) -> dict:
    return one(report, "fcad-legacy.fbx")


def expect_kill(name: str, mutate) -> None:
    report = copy.deepcopy(RECORDED)
    mutate(report)
    try:
        decision = decide(report)
    except (SystemExit, KeyError, StopIteration, IndexError, TypeError) as error:
        print(f"killed: {name}: {type(error).__name__}: {error}")
        return
    if decision["failures"]:
        print(f"killed: {name}: {decision['failures']}")
        return
    raise SystemExit(f"survived unexpectedly: {name}")


def expect_survivor(name: str, mutate) -> None:
    report = copy.deepcopy(RECORDED)
    mutate(report)
    decision = decide(report)
    if decision["failures"]:
        raise SystemExit(
            f"a metamorphic change was refused, so the record depends on it: "
            f"{name}: {decision['failures']}"
        )
    print(f"survived as required (metamorphic): {name}")


# ------------------------------------------------------------ the mutants


def the_channel_was_never_read(report: dict) -> None:
    """The editor handed back no identity at all and the record said nothing."""
    entry = current(report)
    entry["objects_with_definition_id"] = 0
    entry["objects_with_occurrence_id"] = 0
    for node in entry["nodes"]:
        node["definition_id"] = ""
        node["occurrence_id"] = ""


def only_the_definition_half_was_read(report: dict) -> None:
    entry = current(report)
    entry["objects_with_occurrence_id"] = 0
    for node in entry["nodes"]:
        node["occurrence_id"] = ""


def two_placements_answer_to_one_identity(report: dict) -> None:
    """Stable, distinct-looking and ambiguous: the join a project cannot make."""
    entry = current(report)
    entry["nodes"][1]["occurrence_id"] = entry["nodes"][0]["occurrence_id"]
    entry["distinct_occurrence_ids"] -= 1


def the_two_domains_produced_one_value(report: dict) -> None:
    entry = current(report)
    entry["nodes"][0]["occurrence_id"] = entry["nodes"][0]["definition_id"]


def a_value_left_its_domain(report: dict) -> None:
    entry = current(report)
    entry["nodes"][0]["definition_id"] = "step.product_definition#1"


def one_definition_stopped_sharing_its_mesh(report: dict) -> None:
    entry = current(report)
    meshed = [node for node in entry["nodes"] if node["mesh"]]
    if len(meshed) < 2:
        raise SystemExit("the recorded measurement has no shared mesh to break")
    meshed[1]["mesh"] = meshed[1]["mesh"] + "-copy#999"


def a_designation_moved(report: dict) -> None:
    report["against_legacy"]["names_that_moved"] = 1


def the_hierarchy_moved(report: dict) -> None:
    report["against_legacy"]["parents_that_moved"] = 1


def a_geometry_binding_moved(report: dict) -> None:
    report["against_legacy"]["mesh_bindings_that_moved"] = 1


def a_material_binding_moved(report: dict) -> None:
    report["against_legacy"]["material_counts_that_moved"] = 1


def an_existing_property_was_rewritten(report: dict) -> None:
    report["against_legacy"]["existing_properties_that_moved"] = 1


def the_channel_added_an_identifier_collision(report: dict) -> None:
    current(report)["identifier_uniqueness_violations"] += 1


def the_import_reported_errors(report: dict) -> None:
    current(report)["import_errors"] = 1


def a_legacy_layout_was_given_an_identity(report: dict) -> None:
    entry = legacy(report)
    entry["objects_with_occurrence_id"] = entry["objects"]


def an_escaped_value_came_back_rewritten(report: dict) -> None:
    entry = one(report, "fcad-identity-escaping.fbx")
    entry["nodes"][1]["definition_id"] = "fcad1:def:source:x:a:b:c"


def the_keyed_file_lost_its_values(report: dict) -> None:
    entry = one(report, "fcad-identity-escaping.fbx")
    entry["objects_with_definition_id"] = 0


def a_placement_was_lost_across_the_rename(report: dict) -> None:
    report["across_a_rename"]["joined_by_placement_identity"] -= 1


def nothing_was_actually_renamed(report: dict) -> None:
    """A rename variant that renamed nothing measures nothing."""
    report["across_a_rename"]["designations_that_changed"] = 0
    report["across_a_rename"]["roots_excluded_from_the_name_comparison"] = 0


def the_root_stopped_being_excluded_from_the_name_comparison(report: dict) -> None:
    """The exclusion is a named allowance, not a hole anything may fall into."""
    report["against_legacy"]["roots_excluded_from_the_name_comparison"] = 2


# --------------------------------------------------------- the metamorphic
#
# What the record must NOT depend on. Each of these changes the measurement in
# a way §22B-1e2a and §22B-1e2b already showed is a property of the editor
# rather than of the file, and a record that refused one would be asserting a
# stability nothing in an FBX can deliver.


def every_gameobject_identifier_moved(report: dict) -> None:
    report["against_legacy"]["gameobject_file_ids_that_moved"] = report["against_legacy"][
        "objects_compared"
    ]


def every_mesh_and_material_identifier_moved(report: dict) -> None:
    report["against_legacy"]["mesh_file_ids_that_moved"] = 99
    report["against_legacy"]["material_file_ids_that_moved"] = 99


def the_existing_identifier_collision_is_still_there(report: dict) -> None:
    """§22B-1e1's finding, unchanged in both imports. Not this slice's to fix."""
    current(report)["identifier_uniqueness_violations"] = 7
    legacy(report)["identifier_uniqueness_violations"] = 7


def every_identifier_moved_across_the_rename(report: dict) -> None:
    """Exactly the cost §22B-1e2a measured. Reported, never asserted."""
    report["across_a_rename"]["gameobject_file_ids_that_moved"] = report[
        "across_a_rename"
    ]["objects_compared"]


def main() -> int:
    for name, mutate in [
        ("the_channel_was_never_read", the_channel_was_never_read),
        ("only_the_definition_half_was_read", only_the_definition_half_was_read),
        ("two_placements_answer_to_one_identity", two_placements_answer_to_one_identity),
        ("the_two_domains_produced_one_value", the_two_domains_produced_one_value),
        ("a_value_left_its_domain", a_value_left_its_domain),
        ("one_definition_stopped_sharing_its_mesh", one_definition_stopped_sharing_its_mesh),
        ("a_designation_moved", a_designation_moved),
        ("the_hierarchy_moved", the_hierarchy_moved),
        ("a_geometry_binding_moved", a_geometry_binding_moved),
        ("a_material_binding_moved", a_material_binding_moved),
        ("an_existing_property_was_rewritten", an_existing_property_was_rewritten),
        (
            "the_channel_added_an_identifier_collision",
            the_channel_added_an_identifier_collision,
        ),
        ("the_import_reported_errors", the_import_reported_errors),
        ("a_legacy_layout_was_given_an_identity", a_legacy_layout_was_given_an_identity),
        ("an_escaped_value_came_back_rewritten", an_escaped_value_came_back_rewritten),
        ("the_keyed_file_lost_its_values", the_keyed_file_lost_its_values),
        ("a_placement_was_lost_across_the_rename", a_placement_was_lost_across_the_rename),
        ("nothing_was_actually_renamed", nothing_was_actually_renamed),
        (
            "the_root_stopped_being_excluded_from_the_name_comparison",
            the_root_stopped_being_excluded_from_the_name_comparison,
        ),
    ]:
        expect_kill(name, mutate)

    for name, mutate in [
        ("every_gameobject_identifier_moved", every_gameobject_identifier_moved),
        (
            "every_mesh_and_material_identifier_moved",
            every_mesh_and_material_identifier_moved,
        ),
        (
            "the_existing_identifier_collision_is_still_there",
            the_existing_identifier_collision_is_still_there,
        ),
        (
            "every_identifier_moved_across_the_rename",
            every_identifier_moved_across_the_rename,
        ),
    ]:
        expect_survivor(name, mutate)

    pristine = decide(copy.deepcopy(RECORDED))
    if pristine["failures"]:
        raise SystemExit(f"the recorded measurement itself fails: {pristine['failures']}")
    print("file record campaign: 19 mutants killed, 4 metamorphic controls survived")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
