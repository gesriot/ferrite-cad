#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Semantic mutants for the §22B-1e2b measurement.

Each mutant is a way this measurement could look like it had proved something
it had not. They are applied to copies of the recorded canonical measurement
and fed to the real join verifier, so a mutant survives only if the verifier
genuinely cannot see it.

The verifier is what is under test here. The Unity probes, the document
generator and the structural transformer are mutated separately by
`mutate_graph.sh`, which compiles them and runs the real editor.
"""

from __future__ import annotations

import copy
import json
from pathlib import Path

from verify_graph import Refused, verify

ROOT = Path(__file__).resolve().parent.parent
EXPECTED = ROOT / "expected"


def read(name: str) -> dict:
    return json.loads((EXPECTED / name).read_text(encoding="utf-8"))


GRAPH = read("graph-report.json")
META = read("meta-report.json")
REMAP = read("remap-report.json")
SCRIPTED = read("scripted-report.json")
CLAIM = read("fbxclaim-report.json")
ORACLE = read("graph-oracle-report.json")
PLAN = read("graph-plan.json")
DECISION = read("graph-decision.json")

CONTROL = "g-flat"


# ------------------------------------------------------------ the accessors


def scenario(report: dict, name: str) -> dict:
    return next(item for item in report["scenarios"] if item["name"] == name)


def variant(report: dict, name: str) -> dict:
    return next(item for item in report["variants"] if item["name"] == name)


def reference(report: dict, name: str, prefix: str) -> dict:
    return next(
        item for item in scenario(report, name)["references"] if item["anchor"].startswith(prefix)
    )


def oracle_file(oracle: dict, name: str) -> dict:
    return next(item for item in oracle["files"] if item["file"] == name)


def remap_type(report: dict, kind: str) -> dict:
    return next(item for item in report["types"] if item["unity_type"] == kind)


def a_graph_that_carries_identity(report: dict) -> str:
    return next(
        item["name"]
        for item in report["variants"]
        if item["definition_join"] == "FerriteCADDefinitionId"
    )


# A mutant has to change something. Aiming a "the extra renderer was not
# counted" mutation at a graph that adds no renderer would be observationally
# equivalent to the original, and an equivalent mutant that is not killed is
# not a survivor — it is a mutant that was never applied. These pick the graph
# where the number the mutation is about is actually different.
def a_graph_with_the_most(report: dict, field: str) -> dict:
    return max(report["variants"], key=lambda item: item[field])


def expect_kill(name: str, mutate) -> None:
    graph = copy.deepcopy(GRAPH)
    meta = copy.deepcopy(META)
    remap = copy.deepcopy(REMAP)
    scripted = copy.deepcopy(SCRIPTED)
    claim = copy.deepcopy(CLAIM)
    oracle = copy.deepcopy(ORACLE)
    plan = copy.deepcopy(PLAN)
    mutate(graph, meta, remap, scripted, claim, oracle, plan)
    try:
        verify(graph, meta, remap, scripted, claim, oracle, plan)
    except (Refused, KeyError, StopIteration, IndexError, TypeError, ValueError) as error:
        print(f"killed: {name}: {type(error).__name__}: {error}")
        return
    raise SystemExit(f"survived unexpectedly: {name}")


# ------------------------------------------------------------ the mutants


def only_non_null(graph, *_):
    """A gate that accepts any resolved object as a kept reference."""
    item = reference(graph, f"{CONTROL}/s03-display-name-only", "object:")
    item["verdict"] = "same_semantic"
    item["semantic_after"] = item["semantic_before"] + ";moved"


def imported_source_id_deleted(graph, _, __, ___, ____, oracle, _____):
    """The source half of the identity taken out of a variant's files."""
    name = a_graph_that_carries_identity(graph)
    oracle_file(oracle, f"{name}/base.fbx")["facts"]["nodes_with_definition_id"] = 0


def source_local_key_taken_as_global(graph, *_):
    """The control's colliding key reported as telling every definition apart."""
    item = variant(graph, CONTROL)
    item["ambiguous_definitions"] = 0
    item["ambiguous_definition_names"] = []


def the_collision_removed_from_the_document(_, __, ___, ____, _____, oracle, ______):
    oracle_file(oracle, f"{CONTROL}/base.fbx")["facts"]["definition_key_collisions"] = 0


def ordinal_counted_as_durable(graph, *_):
    variant(graph, CONTROL)["occurrence_join"] = "FerriteCADOccurrenceId"


def only_the_mesh_checked(graph, *_):
    item = scenario(graph, f"{CONTROL}/s01-byte-identical")
    item["references"] = [
        row for row in item["references"] if row["unity_type"] == "Mesh"
    ]


def the_game_object_never_checked(graph, *_):
    item = scenario(graph, f"{CONTROL}/s01-byte-identical")
    item["references"] = [
        row for row in item["references"] if row["unity_type"] != "GameObject"
    ]


def the_material_never_checked(graph, *_):
    item = scenario(graph, f"{CONTROL}/s01-byte-identical")
    item["references"] = [
        row for row in item["references"] if row["unity_type"] != "Material"
    ]


def shared_mesh_replaced_by_copies(graph, *_):
    item = variant(graph, a_graph_that_carries_identity(graph))
    item["definitions_whose_placements_share_one_mesh"] += 1


def the_carrier_extra_renderer_ignored(graph, *_):
    item = a_graph_with_the_most(graph, "mesh_renderers")
    control = variant(graph, CONTROL)["mesh_renderers"]
    if item["mesh_renderers"] == control:
        raise SystemExit("no measured graph adds a renderer for this mutant to hide")
    item["mesh_renderers"] = control


def the_extra_node_not_counted(graph, *_):
    item = a_graph_with_the_most(graph, "game_objects")
    control = variant(graph, CONTROL)["game_objects"]
    if item["game_objects"] == control:
        raise SystemExit("no measured graph adds a node for this mutant to hide")
    item["game_objects"] = control


def a_wrong_transform_accepted(graph, *_):
    rows = variant(graph, a_graph_that_carries_identity(graph))["placements"]
    rows[0]["world_position"] = "[9.0000,9.0000,9.0000]"


def add_remap_outcome_called_vanilla_fbx(_, __, remap, *___):
    remap["is_a_property_of_the_fbx"] = True


def external_stale_content_accepted(_, __, remap, *___):
    item = remap_type(remap, "Material")
    item["the_import_honoured_it"] = True
    item["the_fbx_changed_this_object"] = True
    item["external_content_unchanged_after_the_fbx_changed"] = True
    item["the_scene_shows_content_the_fbx_no_longer_has"] = False


def the_remap_measured_on_an_untouched_object(_, __, remap, *___):
    """The stale-content question asked about an object nothing changes."""
    for item in remap["types"]:
        item["the_fbx_changed_this_object"] = False
        item["the_scene_shows_content_the_fbx_no_longer_has"] = False


def undocumented_meta_called_public_api(_, meta, *__):
    meta["a_public_api_writes_the_table"] = True
    meta["public_api_members_naming_the_table"] = []


def the_meta_row_counts_invented(_, meta, *__):
    meta["table_entries"] = meta["table_entries"] + 1


def scripted_identifier_built_from_a_display_name(_, __, ___, scripted, *____):
    designation = scripted["visible_mesh_names"][0]
    scripted["identifiers"][0]["identifier"] = f"fcad|mesh|{designation}"


def deterministic_identifier_replaced_by_an_ordinal(_, __, ___, scripted, *____):
    scripted["identifiers"][0]["identifier"] = "fcad|mesh|0"


def the_collision_accepted(_, __, ___, scripted, *____):
    scripted["collision_merged_two_objects_into_one"] = False
    scripted["collision_materials_published"] = scripted["collision_materials_expected"] - 1


def the_rename_result_inverted_for_one_type(_, __, ___, scripted, *____):
    """A type whose identifier Unity does not honour, reported as honoured."""
    for row in scripted["rename"]:
        if not row["the_identifier_alone_decides_the_local_file_id"]:
            row["the_identifier_alone_decides_the_local_file_id"] = True
            return
        row["the_identifier_alone_decides_the_local_file_id"] = False
        return


def the_rename_never_compared_an_identifier(_, __, ___, scripted, *____):
    for row in scripted["rename"]:
        row["identifiers_compared"] = 0


def a_removed_object_retargeted(graph, *_):
    item = reference(
        graph,
        f"{CONTROL}/s06-remove-tracked-definition",
        "object:019ffc72-2996-7000-8000-0000000000a1/step.product_definition#300",
    )
    item["verdict"] = "same_semantic"
    item["semantic_after"] = "definition=other;occurrence=other;at=[0.0000,0.0000,0.0000]"
    item["resolved_by_reloaded_asset"] = "GameObject:Alpha Part:1"
    item["resolved_by_stored_identifier"] = "GameObject:Alpha Part:1"


def the_id_checked_only_before_the_rename(graph, *_):
    for item in graph["scenarios"]:
        if item["name"].endswith("s03-display-name-only"):
            item["after"]["nodes"] = item["before"]["nodes"]


def the_oracle_reading_a_different_file(_, __, ___, ____, _____, oracle, ______):
    base = oracle_file(oracle, f"{CONTROL}/base.fbx")
    other = oracle_file(oracle, f"{CONTROL}/renamed.fbx")
    base["fnv1a64"] = other["fnv1a64"]


def a_prewritten_report_the_reader_never_saw(graph, *_):
    scenario(graph, f"{CONTROL}/s01-byte-identical")["before_fnv1a64"] = "0" * 16


def one_mandatory_transition_skipped(graph, _, __, ___, ____, _____, plan):
    name = f"{CONTROL}/s05a-insert-sibling"
    graph["scenarios"] = [item for item in graph["scenarios"] if item["name"] != name]
    plan["scenarios"] = [item for item in plan["scenarios"] if item["name"] != name]


def a_run_that_tracked_nothing(graph, *_):
    for item in graph["scenarios"]:
        item["references"] = []


def a_zero_check_run(graph, *_):
    graph["checks"] = 0


def the_two_resolutions_allowed_to_disagree(graph, *_):
    reference(graph, f"{CONTROL}/s01-byte-identical", "mesh:")[
        "resolved_by_stored_identifier"
    ] = "<null>"


def the_stored_pair_no_longer_the_object_identity(graph, *_):
    item = reference(graph, f"{CONTROL}/s01-byte-identical", "material:")
    item["stored_file_id"] = item["stored_file_id"] + 1


def an_ambiguous_join_reported_as_a_kept_reference(graph, *_):
    item = variant(graph, CONTROL)
    definition = item["ambiguous_definition_names"][0]
    local = definition.split(":", 1)[-1]
    for row in scenario(graph, f"{CONTROL}/s01-byte-identical")["references"]:
        if local in row["anchor"] and row["verdict"] == "ambiguous_join":
            row["verdict"] = "same_semantic"
            return
    raise SystemExit("the control has no ambiguous anchor for this mutant to hide")


def the_transformer_moved_a_vertex(graph, _, __, ___, ____, oracle, _____):
    name = a_graph_that_carries_identity(graph)
    entry = oracle_file(oracle, f"{name}/base.fbx")
    entry["geometries"][0]["digest"] = "0" * 16


def the_transformer_renumbered_an_object(graph, _, __, ___, ____, oracle, _____):
    name = a_graph_that_carries_identity(graph)
    entry = oracle_file(oracle, f"{name}/base.fbx")
    entry["nodes"][0]["object_number"] += 1


def the_transformer_recoloured_a_material(graph, _, __, ___, ____, oracle, _____):
    name = a_graph_that_carries_identity(graph)
    entry = oracle_file(oracle, f"{name}/base.fbx")
    entry["materials"][0]["digest"] = "0" * 16


def the_fbx_claim_verdict_inverted(_, __, ___, ____, claim, *_____):
    claim["the_model_importer_still_owns_fbx"] = not claim["the_model_importer_still_owns_fbx"]


def the_visible_names_never_recorded(graph, *_):
    item = variant(graph, CONTROL)
    item["visible_node_names"] = []
    item["visible_mesh_names"] = []
    item["visible_material_names"] = []


def a_machine_named_variant_reported_as_human_named(graph, *_):
    for item in graph["variants"]:
        if item["visible_nodes_named_by_machine_token"] > 0:
            item["visible_nodes_named_by_machine_token"] = 0
            return
    raise SystemExit("no measured variant showed a machine-named node")


MUTATIONS = [
    ("a_gate_that_only_checks_non_null", only_non_null),
    ("the_imported_source_id_removed_from_the_file", imported_source_id_deleted),
    ("the_source_local_key_taken_as_global", source_local_key_taken_as_global),
    ("the_multi_source_collision_removed_from_the_document", the_collision_removed_from_the_document),
    ("the_ordinal_reported_as_a_durable_occurrence_identity", ordinal_counted_as_durable),
    ("only_the_mesh_checked", only_the_mesh_checked),
    ("the_game_object_never_checked", the_game_object_never_checked),
    ("the_material_never_checked", the_material_never_checked),
    ("the_shared_mesh_replaced_by_copies", shared_mesh_replaced_by_copies),
    ("a_carrier_that_adds_a_renderer_not_counted", the_carrier_extra_renderer_ignored),
    ("an_extra_node_not_counted", the_extra_node_not_counted),
    ("a_wrong_transform_accepted", a_wrong_transform_accepted),
    ("an_add_remap_outcome_called_vanilla_fbx", add_remap_outcome_called_vanilla_fbx),
    ("external_stale_content_accepted", external_stale_content_accepted),
    ("the_remap_measured_on_an_object_nothing_changes", the_remap_measured_on_an_untouched_object),
    ("undocumented_meta_editing_called_a_public_api", undocumented_meta_called_public_api),
    ("the_meta_row_count_invented", the_meta_row_counts_invented),
    ("a_scripted_identifier_built_from_a_display_name", scripted_identifier_built_from_a_display_name),
    ("a_deterministic_identifier_replaced_by_an_ordinal", deterministic_identifier_replaced_by_an_ordinal),
    ("an_identifier_collision_accepted", the_collision_accepted),
    ("the_scripted_rename_result_inverted_for_one_type", the_rename_result_inverted_for_one_type),
    ("the_scripted_rename_compared_no_identifier", the_rename_never_compared_an_identifier),
    ("a_removed_object_retargeted", a_removed_object_retargeted),
    ("the_identifiers_compared_only_before_the_rename", the_id_checked_only_before_the_rename),
    ("the_oracle_reading_a_different_file", the_oracle_reading_a_different_file),
    ("a_prewritten_report_the_reader_never_saw", a_prewritten_report_the_reader_never_saw),
    ("one_mandatory_transition_skipped", one_mandatory_transition_skipped),
    ("a_run_that_tracked_nothing", a_run_that_tracked_nothing),
    ("a_zero_check_run", a_zero_check_run),
    ("the_two_resolutions_allowed_to_disagree", the_two_resolutions_allowed_to_disagree),
    ("the_stored_pair_no_longer_the_object_identity", the_stored_pair_no_longer_the_object_identity),
    ("an_ambiguous_join_reported_as_a_kept_reference", an_ambiguous_join_reported_as_a_kept_reference),
    ("the_transformer_moved_a_vertex", the_transformer_moved_a_vertex),
    ("the_transformer_renumbered_an_object", the_transformer_renumbered_an_object),
    ("the_transformer_recoloured_a_material", the_transformer_recoloured_a_material),
    ("the_fbx_extension_claim_verdict_inverted", the_fbx_claim_verdict_inverted),
    ("the_visible_names_never_recorded", the_visible_names_never_recorded),
    ("a_machine_named_variant_reported_as_human_named", a_machine_named_variant_reported_as_human_named),
]


def main() -> int:
    checks, decision = verify(
        copy.deepcopy(GRAPH),
        copy.deepcopy(META),
        copy.deepcopy(REMAP),
        copy.deepcopy(SCRIPTED),
        copy.deepcopy(CLAIM),
        copy.deepcopy(ORACLE),
        copy.deepcopy(PLAN),
    )
    if checks == 0:
        raise SystemExit("the mutation baseline ran zero checks")
    if decision != DECISION:
        raise SystemExit("the committed decision record is not the one this measurement makes")
    print(
        f"mutation baseline: checks={checks}, "
        f"transitions={len(DECISION['graph_transitions'])}, "
        f"variants={len(DECISION['graph']['variants'])}, "
        f"decision rows={len(DECISION['decision_table'])}"
    )

    for name, mutate in MUTATIONS:
        expect_kill(name, mutate)
    print(f"mutation campaign: {len(MUTATIONS)} runtime mutants killed")
    print("mutation campaign: 0 unexpected survivors")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
