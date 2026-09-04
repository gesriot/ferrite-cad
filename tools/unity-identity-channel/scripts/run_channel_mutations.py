#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Semantic mutants for the §22B-1e2a measurement.

Each mutant is a way this measurement could look like it had proved something
it had not. They are applied to copies of the recorded canonical measurement
and fed to the real join verifier, so a mutant survives only if the verifier
genuinely cannot see it.

The verifier is what is under test here. The Unity probe, the document
generator and the channel rewriter are mutated separately by
`mutate_channel.sh`, which compiles them and runs the real editor.
"""

from __future__ import annotations

import copy
import json
from pathlib import Path

from verify_channel import Refused, verify

ROOT = Path(__file__).resolve().parent.parent
EXPECTED = ROOT / "expected"
VANILLA = json.loads((EXPECTED / "vanilla-report.json").read_text(encoding="utf-8"))
COMPANION = json.loads((EXPECTED / "companion-report.json").read_text(encoding="utf-8"))
ORACLE = json.loads((EXPECTED / "channel-oracle-report.json").read_text(encoding="utf-8"))
PLANS = {
    "vanilla": json.loads((EXPECTED / "vanilla-plan.json").read_text(encoding="utf-8")),
    "companion": json.loads((EXPECTED / "companion-plan.json").read_text(encoding="utf-8")),
}
DECISION = json.loads((EXPECTED / "channel-decision.json").read_text(encoding="utf-8"))


def scenario(report: dict, name: str) -> dict:
    return next(item for item in report["scenarios"] if item["name"] == name)


def summary(report: dict, name: str) -> dict:
    return next(item for item in report["candidates"] if item["name"] == name)


def reference(report: dict, name: str, prefix: str) -> dict:
    return next(
        item for item in scenario(report, name)["references"] if item["anchor"].startswith(prefix)
    )


def read(oracle: dict, name: str) -> dict:
    return next(item for item in oracle["files"] if item["file"] == name)


def expect_kill(name: str, mutate) -> None:
    vanilla = copy.deepcopy(VANILLA)
    companion = copy.deepcopy(COMPANION)
    oracle = copy.deepcopy(ORACLE)
    plans = copy.deepcopy(PLANS)
    mutate(vanilla, companion, oracle, plans)
    try:
        verify(vanilla, companion, oracle, plans)
    except (Refused, KeyError, StopIteration, IndexError, TypeError) as error:
        print(f"killed: {name}: {type(error).__name__}: {error}")
        return
    raise SystemExit(f"survived unexpectedly: {name}")


# ------------------------------------------------------------ the mutants


def only_non_null(vanilla, *_):
    """A gate that accepts any resolved object as a kept reference."""
    item = reference(vanilla, "a-control/s03-display-name-only", "object:")
    item["verdict"] = "same_semantic"
    item["meaning_verdict"] = "same_semantic"
    item["join_was_ambiguous"] = False


def imported_source_id_deleted(_, __, oracle, ___):
    """The source half of the identity taken out of a candidate's files."""
    facts = read(oracle, "c-property/base.fbx")["facts"]
    facts["nodes_with_definition_id"] = 0


def source_local_key_taken_as_global(vanilla, *_):
    """The control's colliding key reported as telling every definition apart."""
    item = summary(vanilla, "a-control")
    item["ambiguous_definitions"] = 0
    item["ambiguous_definition_names"] = []


def companion_result_sold_as_vanilla(_, companion, __, ___):
    companion["companion_active"] = False


def companion_candidate_planned_into_vanilla(_, __, ___, plans):
    plans["vanilla"]["candidates"].append(
        dict(
            next(item for item in plans["companion"]["candidates"] if item["name"] == "d-companion")
        )
    )


def visible_names_not_recorded(vanilla, *_):
    item = summary(vanilla, "c-property")
    item["visible_node_names"] = []
    item["visible_mesh_names"] = []
    item["visible_material_names"] = []


def visible_names_claimed_human(vanilla, *_):
    """A machine-named candidate reported as showing designations."""
    item = summary(vanilla, "b-occurrence")
    item["visible_node_names"] = ["Alpha Part", "Beta Part"]
    item["visible_mesh_names"] = ["Alpha Part"]
    item["visible_material_names"] = ["Shell"]


def rename_timing_measured_on_one_side_only(_, companion, __, ___):
    """The companion's identifiers compared before the change and never after."""
    for item in companion["scenarios"]:
        item["after"]["nodes"] = item["before"]["nodes"]


def mesh_result_reported_for_material(vanilla, *_):
    for item in scenario(vanilla, "c-property/s07-change-material")["references"]:
        if item["unity_type"] == "Material":
            item["unity_type"] = "Mesh"


def ordinal_counted_as_durable(vanilla, *_):
    summary(vanilla, "b-ordinal")["occurrence_join"] = "FerriteCADOccurrenceId"


def ordinal_declared_durable_in_the_plan(_, __, oracle, plans):
    for candidate in plans["vanilla"]["candidates"]:
        if candidate["name"] == "b-ordinal":
            candidate["carries_occurrence_id"] = True


def token_collision_ignored(vanilla, *_):
    probe = next(item for item in vanilla["names"] if item["name"] == "n05-hash-collision")
    probe["unity_names_that_repeat"] = 0
    for index, row in enumerate(probe["rows"]):
        row["material_local_file_ids"] = [
            identifier + index + 1 for identifier in row["material_local_file_ids"]
        ]


def retarget_of_a_removed_object_accepted(vanilla, *_):
    item = reference(
        vanilla,
        "a-control/s06-remove-tracked-definition",
        "object:019ffc72-2996-7000-8000-0000000000a1/step.product_definition#300",
    )
    item["verdict"] = "same_semantic"
    item["meaning_verdict"] = "same_semantic"
    item["join_was_ambiguous"] = False
    item["semantic_after"] = item["semantic_before"]
    item["resolved_by_reloaded_asset"] = "GameObject:Alpha Part:1"
    item["resolved_by_stored_identifier"] = "GameObject:Alpha Part:1"


def oracle_read_a_different_file(_, __, oracle, ___):
    base = read(oracle, "a-control/base.fbx")
    other = read(oracle, "a-control/renamed.fbx")
    base["nodes"] = other["nodes"]
    base["facts"] = other["facts"]


def prewritten_report(vanilla, *_):
    scenario(vanilla, "a-control/s01-byte-identical")["before_fnv1a64"] = "0" * 16


def a_mandatory_scenario_skipped(vanilla, _, __, plans):
    name = "c-property/s05a-insert-sibling"
    vanilla["scenarios"] = [item for item in vanilla["scenarios"] if item["name"] != name]
    plans["vanilla"]["scenarios"] = [
        item for item in plans["vanilla"]["scenarios"] if item["name"] != name
    ]


def probe_setting_allowed_to_move_identifiers(vanilla, *_):
    vanilla["sort_control"]["identifiers_are_unchanged"] = False


def a_run_that_tracked_nothing(vanilla, *_):
    for item in vanilla["scenarios"]:
        item["references"] = []


def the_two_resolutions_allowed_to_disagree(vanilla, *_):
    reference(vanilla, "c-property/s01-byte-identical", "mesh:")[
        "resolved_by_stored_identifier"
    ] = "<null>"


def stored_pair_is_not_the_object(vanilla, *_):
    item = reference(vanilla, "c-property/s01-byte-identical", "material:")
    item["stored_file_id"] = item["stored_file_id"] + 1


def companion_allowed_to_change_the_control(_, companion, __, ___):
    scenario(companion, "a-control/s01-byte-identical")["before"]["nodes"][1][
        "local_file_id"
    ] += 1


def the_collision_removed_from_the_document(_, __, oracle, ___):
    read(oracle, "a-control/base.fbx")["facts"]["definition_key_collisions"] = 0


def subasset_identifier_that_is_not_the_stored_pair(vanilla, *_):
    vanilla["subassets_whose_identifier_is_something_else"] = 1


def an_ambiguous_join_called_a_verdict(vanilla, *_):
    item = reference(
        vanilla,
        "a-control/s01-byte-identical",
        "mesh:019ffc72-2996-7000-8000-0000000000b2",
    )
    item["verdict"] = "same_semantic"


def the_shared_mesh_break_hidden(vanilla, *_):
    """The one scenario where a durable occurrence identity loses a mesh."""
    item = reference(
        vanilla,
        "b-occurrence/s05a-insert-sibling",
        "mesh:019ffc72-2996-7000-8000-0000000000a1/step.product_definition#200",
    )
    item["verdict"] = "same_semantic"
    item["meaning_verdict"] = "same_semantic"


MUTATIONS = [
    ("a_gate_that_only_checks_non_null", only_non_null),
    ("the_imported_source_id_deleted", imported_source_id_deleted),
    ("the_source_local_key_taken_as_global", source_local_key_taken_as_global),
    ("a_companion_result_presented_as_vanilla", companion_result_sold_as_vanilla),
    ("a_companion_candidate_planned_into_the_vanilla_run", companion_candidate_planned_into_vanilla),
    ("the_visible_names_never_recorded", visible_names_not_recorded),
    ("a_machine_named_candidate_reported_as_human_named", visible_names_claimed_human),
    ("the_rename_timing_measured_on_one_side_only", rename_timing_measured_on_one_side_only),
    ("a_mesh_result_reported_as_a_material_result", mesh_result_reported_for_material),
    ("the_ordinal_reported_as_a_durable_occurrence_identity", ordinal_counted_as_durable),
    ("the_ordinal_declared_durable_in_the_plan", ordinal_declared_durable_in_the_plan),
    ("the_token_collision_ignored", token_collision_ignored),
    ("a_retarget_of_a_removed_object_accepted", retarget_of_a_removed_object_accepted),
    ("the_oracle_reading_a_different_file", oracle_read_a_different_file),
    ("a_prewritten_report_the_reader_never_saw", prewritten_report),
    ("one_mandatory_scenario_skipped", a_mandatory_scenario_skipped),
    ("the_probe_setting_allowed_to_move_identifiers", probe_setting_allowed_to_move_identifiers),
    ("a_run_that_tracked_nothing", a_run_that_tracked_nothing),
    ("the_two_resolutions_allowed_to_disagree", the_two_resolutions_allowed_to_disagree),
    ("the_stored_pair_no_longer_the_object_identity", stored_pair_is_not_the_object),
    ("the_companion_allowed_to_change_the_control", companion_allowed_to_change_the_control),
    ("the_multi_source_collision_removed_from_the_document", the_collision_removed_from_the_document),
    ("a_subasset_identifier_that_is_not_the_stored_pair", subasset_identifier_that_is_not_the_stored_pair),
    ("an_ambiguous_join_reported_as_a_kept_reference", an_ambiguous_join_called_a_verdict),
    ("the_shared_mesh_break_reported_as_a_kept_reference", the_shared_mesh_break_hidden),
]


def main() -> int:
    checks, decision = verify(
        copy.deepcopy(VANILLA),
        copy.deepcopy(COMPANION),
        copy.deepcopy(ORACLE),
        copy.deepcopy(PLANS),
    )
    if checks == 0:
        raise SystemExit("the mutation baseline ran zero checks")
    if decision != DECISION:
        raise SystemExit("the committed decision record is not the one this measurement makes")
    print(
        f"mutation baseline: checks={checks}, "
        f"transitions={len(DECISION['transitions'])}, "
        f"candidates={len(DECISION['candidates'])}"
    )

    for name, mutate in MUTATIONS:
        expect_kill(name, mutate)
    print(f"mutation campaign: {len(MUTATIONS)} runtime mutants killed")
    print("mutation campaign: 0 unexpected survivors")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
