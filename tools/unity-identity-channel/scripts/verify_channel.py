#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Joins the §22B-1e2a Unity reports to the independent ufbx oracle.

Nothing here chooses a policy. It refuses the ways a measurement of this shape
can be wrong without looking wrong:

  * the editor and the independent reader looking at different bytes;
  * a candidate's files not carrying the identity the candidate claims —
    checked against the oracle and against what the import actually delivered,
    which is what "delete the ImportedSourceId and see if anyone notices"
    would do;
  * a source-local key treated as a global identity, when the same document
    contains two sources that use it;
  * a base document that does not contain the confusions the brief lists, so a
    scenario passes because the confusion was never there;
  * a reference judged on being non-null, or on a display name;
  * a reference whose FerriteCAD object was removed reported as kept;
  * a Geometry result presented as a Model or Material result;
  * the visible names never checked, or a candidate's visible names not being
    the file's names in a vanilla import;
  * a result that needs the FerriteCAD companion postprocessor presented as
    vanilla Unity behaviour;
  * a local file identifier compared only on one side of the companion's
    rename;
  * a token collision counted as a distinct identity.

It also builds the decision table's evidence. The table is written for a person
in the measurement document; what is emitted here is the joined record it is
read from, so the two cannot drift apart.
"""

from __future__ import annotations

import argparse
import json
import unicodedata
from pathlib import Path

MACHINE_PREFIX = "fcad~"

# Every reimport scenario the brief requires, and the brief's own words for it.
MANDATORY = {
    "s01-byte-identical": "byte-identical export",
    "s02-reexport-unchanged": "an unchanged document exported again",
    "s03-display-name-only": "a display name changes",
    "s04a-insert-definition": "a definition inserted",
    "s04b-remove-definition": "a definition removed",
    "s04c-reorder-definitions": "definitions reordered",
    "s05a-insert-sibling": "a sibling placement inserted",
    "s05b-remove-sibling": "a sibling placement removed",
    "s05c-reorder-siblings": "sibling placements reordered",
    "s06-remove-tracked-definition": "a removed object must become missing, never retargeted",
    "s07-change-material": "a material changes",
    "s08-reuse-material": "a material designation is reused",
}

VERDICTS = {
    "same_semantic",
    "same_definition_other_occurrence",
    "retargeted_to_another_definition",
    "missing_though_object_still_exported",
    "missing_because_object_was_removed",
    "ambiguous_join",
}

KINDS = {"Mesh", "Material", "GameObject"}

NAME_PROBES = {
    "n01-ascii-source-qualified",
    "n02-long-token",
    "n03-non-ascii",
    "n04-short-hash",
    "n05-hash-collision",
    "n06-unicode-normalisation",
}


class Refused(Exception):
    pass


def classify(unity_name: str, file_name: str) -> str:
    """What the editor did to one name the file spelled a particular way."""
    if unity_name == file_name:
        return "identical"
    if file_name.startswith(unity_name) and unity_name:
        return "truncated"
    if unicodedata.normalize("NFC", unity_name) == unicodedata.normalize("NFC", file_name):
        return "normalised"
    head, _, tail = unity_name.rpartition(" ")
    if head == file_name and tail.isdigit():
        return "disambiguated"
    return "other"


def visible_kind(names: list[str]) -> str:
    if not names:
        return "none"
    machine = [name for name in names if name.startswith(MACHINE_PREFIX)]
    if len(machine) == len(names):
        return "machine_tokens"
    if not machine:
        return "designations"
    return "mixed"


def partition(groups: dict[str, set[int]]) -> list[list[str]]:
    """Which keys share one identifier, as a canonical partition."""
    buckets: dict[int, list[str]] = {}
    for key, identifiers in groups.items():
        for identifier in identifiers:
            buckets.setdefault(identifier, []).append(key)
    return sorted(sorted(keys) for keys in buckets.values())


def index_by(items: list[dict], key: str) -> dict:
    return {item[key]: item for item in items}


def verify(vanilla: dict, companion: dict, oracle: dict, plans: dict) -> tuple[int, dict]:
    checks = 0

    def require(condition: bool, message: str) -> None:
        nonlocal checks
        checks += 1
        if not condition:
            raise Refused(message)

    reports = {"vanilla": vanilla, "companion": companion}

    # ---- a plugin result must never be able to arrive labelled as vanilla.
    require(not vanilla["companion_active"], "the vanilla report was made with the companion on")
    require(companion["companion_active"], "the companion report was made without the companion")

    files = index_by(oracle["files"], "file")
    require(len(files) == len(oracle["files"]), "the oracle reported one file twice")

    # ---- the probe's one importer setting must have moved nothing.
    for mode, report in reports.items():
        control = report["sort_control"]
        require(len(control["with_default_sort"]) > 0, f"{mode}: the hierarchy-sort control did not run")
        require(
            control["identifiers_are_unchanged"],
            f"{mode}: turning off the importer's hierarchy sort changed a local file identifier, "
            f"so this measurement would be describing the probe rather than Unity",
        )
        require(
            control["hierarchy_with_default_sort"] != control["hierarchy_with_sort_disabled"],
            f"{mode}: the hierarchy-sort control saw no reordering at all, so it proves nothing",
        )
        require(
            report["subassets_whose_identifier_is_the_guid_and_local_id"] > 0,
            f"{mode}: no sub-asset identifier was examined",
        )
        require(
            report["subassets_whose_identifier_is_something_else"] == 0,
            f"{mode}: a sub-asset's GlobalObjectId is not its asset GUID plus its local file "
            f"identifier, so the recorded tables no longer say everything the editor knows",
        )

    # ---- every candidate, in the right run, measuring every mandatory scenario.
    declared: dict[str, dict] = {}
    for mode, plan in plans.items():
        planned = index_by(plan["scenarios"], "name")
        measured = index_by(reports[mode]["scenarios"], "name")
        require(
            set(planned) == set(measured),
            f"{mode}: the editor did not measure exactly the planned scenarios",
        )
        for candidate in plan["candidates"]:
            # `a-control` is deliberately measured in both runs: it is the
            # control that shows the companion changes nothing it was not
            # asked to. Every other candidate belongs to exactly one run.
            require(
                candidate["name"] == "a-control" or candidate["name"] not in declared,
                f"the candidate {candidate['name']} is declared in both runs",
            )
            declared.setdefault(candidate["name"], dict(candidate, mode=mode))
            require(
                candidate["needs_companion"] == (mode == "companion")
                or not candidate["needs_companion"],
                f"{candidate['name']} needs the companion and was planned into {mode}",
            )
            for scenario in MANDATORY:
                require(
                    f"{candidate['name']}/{scenario}" in measured,
                    f"{candidate['name']} did not measure the mandatory scenario {scenario}",
                )

    # `a-control` is measured in both runs on purpose, under one name, so the
    # two are the same candidate and the loop above would reject it. It is
    # compared across the runs further down instead.
    require(
        {"a-control", "b-ordinal", "b-occurrence", "c-property"}
        <= set(index_by(plans["vanilla"]["candidates"], "name")),
        "the vanilla run does not hold all four candidates a stock editor can read",
    )
    require(
        "d-companion" in index_by(plans["companion"]["candidates"], "name"),
        "the companion run does not hold the candidate that needs the companion",
    )

    # ---- the two programs must have read the same bytes.
    for mode, plan in plans.items():
        planned = index_by(plan["scenarios"], "name")
        measured = index_by(reports[mode]["scenarios"], "name")
        for name, scenario in measured.items():
            for side in ("before", "after"):
                key = "/".join(Path(planned[name][side]).parts[-2:])
                read = files.get(key)
                require(read is not None, f"{name}: the oracle never read {key}")
                require(
                    read["fnv1a64"] == scenario[f"{side}_fnv1a64"]
                    and read["bytes"] == scenario[f"{side}_bytes"],
                    f"{name}: the editor and the independent reader read different {side} bytes",
                )
                require(
                    len(scenario[side]["nodes"]) == len(read["nodes"]),
                    f"{name}/{side}: the editor and the file disagree on how many nodes there are",
                )
                require(
                    {node["node_key"] for node in scenario[side]["nodes"]}
                    == {node["node_key"] for node in read["nodes"]},
                    f"{name}/{side}: the editor and the file disagree on which nodes there are",
                )

                spelled = {node["node_key"]: node for node in read["nodes"]}
                editor_mesh: dict[str, set[int]] = {}
                file_mesh: dict[str, set[int]] = {}
                for node in scenario[side]["nodes"]:
                    other = spelled[node["node_key"]]
                    # A vanilla import shows the name the file spells, and
                    # nothing else. The imported model's own root is Unity's
                    # own naming and is excluded above and here.
                    if mode == "vanilla" and node["sibling_path"] != "0":
                        # Or the file's name with the importer's own numeric
                        # suffix, which is how Unity separates two siblings a
                        # source called the same thing. Anything else — a
                        # truncation, a normalisation, some other name — means
                        # the join below is comparing the editor with a file it
                        # did not read, and the run is refused rather than
                        # reported. What the editor does to a name that is long
                        # or not ASCII is measured by the name probes.
                        require(
                            classify(node["unity_name"], other["name"])
                            in {"identical", "disambiguated"},
                            f"{name}/{side}: the editor calls {node['node_key']} "
                            f"{node['unity_name']!r} and the file spells {other['name']!r}",
                        )
                    if node["mesh_vertex_count"] < 0:
                        require(
                            other["geometry_object_number"] == 0,
                            f"{name}/{side}: the file gives {node['node_key']} a geometry and "
                            f"the editor published none",
                        )
                        continue
                    # The portable documents weld nothing, so the counts must
                    # match exactly rather than merely be no larger.
                    require(
                        node["mesh_vertex_count"] == other["geometry_vertices"],
                        f"{name}/{side}: the mesh under {node['node_key']} has "
                        f"{node['mesh_vertex_count']} vertices and the file has "
                        f"{other['geometry_vertices']}",
                    )
                    editor_mesh.setdefault(node["node_key"], set()).add(
                        node["mesh_local_file_id"]
                    )
                    file_mesh.setdefault(node["node_key"], set()).add(
                        other["geometry_object_number"]
                    )
                # Two placements share one mesh in the editor exactly when they
                # share one geometry in the file.
                require(
                    partition(editor_mesh) == partition(file_mesh),
                    f"{name}/{side}: the editor shares geometry between different placements "
                    f"than the file does",
                )

    # ---- the base document must contain the confusions the brief names.
    base = files["a-control/base.fbx"]["facts"]
    require(
        base["definition_key_collisions"] >= 1,
        "the base document no longer contains two sources sharing one source-local key, so "
        "the whole multi-source experiment measures nothing",
    )
    require(base["placements_sharing_one_geometry"] >= 2, "no geometry has several placements")
    require(base["repeated_geometry_display_names"] >= 1, "no two definitions share a designation")
    require(base["repeated_sibling_names"] >= 1, "no two siblings share a designation")
    require(base["repeated_material_slot_names"] >= 1, "no two material slots share a designation")
    require(base["structural_nodes"] >= 2, "the document has no structural nodes")
    require(base["omitted_nodes"] >= 1, "the document has no omitted node")

    # ---- what each candidate's files really carry, read from the bytes.
    directory = {
        "a-control": "a-control",
        "b-ordinal": "b-ordinal",
        "b-occurrence": "b-occurrence",
        "c-property": "c-property",
        # Not a rewrite. Candidate D is candidate C's bytes with a plugin.
        "d-companion": "c-property",
    }
    for name, candidate in declared.items():
        read = files[f"{directory[name]}/base.fbx"]
        facts = read["facts"]
        if candidate["carries_definition_id"]:
            require(
                facts["nodes_with_definition_id"] == facts["models"],
                f"{name}: the file does not carry a source-qualified identity on every node",
            )
            require(
                facts["definition_id_collisions"] == 0,
                f"{name}: the source-qualified identity still names two definitions at once",
            )
            require(
                all(
                    item["name"].startswith(MACHINE_PREFIX)
                    for item in read["objects"]
                ),
                f"{name}: the file claims machine object names and does not have them",
            )
        else:
            require(
                facts["nodes_with_definition_id"] == 0,
                f"{name}: the file carries a source-qualified identity the candidate disclaims",
            )
            require(
                not any(item["name"].startswith(MACHINE_PREFIX) for item in read["objects"]),
                f"{name}: the control's file carries machine object names",
            )
        require(
            candidate["carries_occurrence_id"]
            == (facts["nodes_with_occurrence_id"] == facts["models"]),
            f"{name}: the occurrence identity in the file is not the one the candidate claims",
        )
        require(
            candidate["carries_display_name"]
            == (facts["nodes_with_display_name"] == facts["models"]),
            f"{name}: the designations in the file are not the ones the candidate claims",
        )
        # The source-local key collides in every one of these documents. A
        # candidate is durable because it carries something more, never because
        # the collision went away.
        require(
            facts["definition_key_collisions"] >= 1,
            f"{name}: this candidate's document lost the source-local collision",
        )

    # ---- the measured summary must agree with the file and with the claim.
    summaries: dict[str, dict] = {}
    for mode, report in reports.items():
        for summary in report["candidates"]:
            name = summary["name"]
            require(
                name + "/" + mode not in summaries,
                f"{mode}: the candidate {name} was summarised twice",
            )
            summaries[name + "/" + mode] = summary
            candidate = declared.get(name)
            require(candidate is not None, f"{mode}: an unplanned candidate was summarised")
            require(
                summary["definition_join"]
                == ("FerriteCADDefinitionId" if candidate["carries_definition_id"]
                    else "FerriteCADDefinitionKey"),
                f"{name}: the join the probe used is not the one the candidate declares",
            )
            require(
                summary["occurrence_join"]
                == ("FerriteCADOccurrenceId" if candidate["carries_occurrence_id"]
                    else "ordinal_in_scene_order"),
                f"{name}: the occurrence join is not the one the candidate declares",
            )
            require(
                summary["ambiguous_definitions"]
                == len(summary["ambiguous_definition_names"]),
                f"{name}: the ambiguity count and the ambiguity list disagree",
            )
            # The whole point of the source-qualified identity: without it the
            # document's two `#42` definitions are one, and with it they are two.
            require(
                (summary["ambiguous_definitions"] == 0)
                == candidate["carries_definition_id"],
                f"{name}: a source-local identity was reported as telling every definition "
                f"apart, or a source-qualified one as failing to",
            )

            # ---- the visible names, which are half the question.
            # Recomputed from the report's own node list rather than trusted.
            # A summary that quietly left one of the three lists empty would
            # otherwise still classify as "machine tokens" on the other two,
            # and the visible names are half of what this slice is measuring.
            base_scenario = next(
                item
                for item in reports[mode]["scenarios"]
                if item["name"] == f"{name}/s01-byte-identical"
            )
            body = [node for node in base_scenario["before"]["nodes"] if node["sibling_path"] != "0"]
            rebuilt = {
                "visible_node_names": {node["unity_name"] for node in body},
                "visible_mesh_names": {
                    node["mesh_unity_name"] for node in body if node["mesh_unity_name"] != "<none>"
                },
                "visible_material_names": {
                    material
                    for node in body
                    for material in node["material_unity_names"]
                    if material != "<none>"
                },
            }
            for field, expected_names in rebuilt.items():
                require(
                    len(expected_names) > 0,
                    f"{name}: the import published no {field.replace('_', ' ')}",
                )
                require(
                    set(summary[field]) == expected_names,
                    f"{name}: the recorded {field.replace('_', ' ')} are not the names the "
                    f"import actually published",
                )
            visible = visible_kind(
                summary["visible_node_names"]
                + summary["visible_mesh_names"]
                + summary["visible_material_names"]
            )
            require(visible != "none", f"{name}: no visible name was recorded at all")
            summary["visible_names"] = visible
            file_names = visible_kind(
                [item["name"] for item in files[f"{directory[name]}/base.fbx"]["objects"]]
            )
            if mode == "vanilla":
                require(
                    visible == file_names,
                    f"{name}: a vanilla import showed {visible} while the file spells "
                    f"{file_names}, so something other than the file named these objects",
                )
            require(
                summary["subassets_named_by_machine_token"]
                + summary["subassets_named_by_designation"]
                > 0,
                f"{name}: no sub-asset name was classified",
            )
            require(
                summary["subassets_named_after_the_asset_file"] > 0,
                f"{name}: the imported model's own root was not identified, so the visible "
                f"names below would be counting Unity's own naming of the asset",
            )

            # Which name Unity gives a Mesh, measured rather than carried over
            # from §22B-1e1: these candidates give the Model and the Geometry
            # different names on purpose, so the question has an answer here.
            read = files[f"{directory[name]}/base.fbx"]
            by_key = {node["node_key"]: node for node in read["nodes"]}
            after_model = 0
            after_geometry = 0
            neither = 0
            for node in base_scenario["before"]["nodes"]:
                if node["mesh_unity_name"] == "<none>":
                    continue
                spelled = by_key[node["node_key"]]
                if node["mesh_unity_name"] == spelled["name"]:
                    after_model += 1
                elif node["mesh_unity_name"] == spelled["geometry_name"]:
                    after_geometry += 1
                else:
                    neither += 1
                checks += 1
            summary["mesh_named_after_the_fbx_model"] = after_model
            summary["mesh_named_after_the_fbx_geometry"] = after_geometry
            summary["mesh_named_after_neither"] = neither
            require(
                after_model + after_geometry + neither > 0,
                f"{name}: no imported mesh name was compared with the file",
            )

    # ---- every tracked reference judged on meaning, per kind, per scenario.
    rows: list[dict] = []
    for mode, report in reports.items():
        planned = index_by(plans[mode]["scenarios"], "name")
        for scenario in report["scenarios"]:
            name = scenario["name"]
            expected = (
                len(planned[name]["mesh_definitions"])
                + len(planned[name]["material_bindings"])
                + len(planned[name]["object_bindings"])
            )
            require(
                len(scenario["references"]) == expected,
                f"{name}: {len(scenario['references'])} references were tracked and the plan "
                f"asked for {expected}",
            )
            kinds_here = set()
            for reference in scenario["references"]:
                kinds_here.add(reference["unity_type"])
                require(reference["unity_type"] in KINDS, f"{name}: unknown tracked type")
                require(
                    reference["verdict"] in VERDICTS,
                    f"{name}: unknown verdict {reference['verdict']}",
                )
                require(
                    reference["semantic_before"] != "",
                    f"{name}: a reference was tracked without a durable meaning",
                )
                require(
                    reference["meaning_verdict"] in VERDICTS - {"ambiguous_join"},
                    f"{name}: a reference has no meaning verdict behind its verdict",
                )
                require(
                    (reference["verdict"] == "ambiguous_join")
                    == reference["join_was_ambiguous"],
                    f"{name}: an ambiguous join and an ambiguous verdict disagree",
                )
                if reference["verdict"] == "same_semantic":
                    require(
                        reference["semantic_after"] == reference["semantic_before"],
                        f"{name}: a reference was called kept while its meaning changed",
                    )
                    require(
                        reference["resolved_by_reloaded_asset"] != "<null>",
                        f"{name}: a null reference was called kept",
                    )
                    # A removed object cannot be a kept reference, whatever the
                    # object the reference landed on is called.
                    require(
                        reference["semantic_object_present_after"],
                        f"{name}: a reference to an object the document dropped was called kept",
                    )
                if reference["meaning_verdict"].startswith("missing"):
                    require(
                        reference["resolved_by_reloaded_asset"] == "<null>",
                        f"{name}: a resolved reference was called missing",
                    )
                require(
                    reference["resolved_by_reloaded_asset"]
                    == reference["resolved_by_stored_identifier"],
                    f"{name}: the two ways of resolving one reference disagree",
                )
                require(
                    reference["stored_file_id"] == reference["local_file_id_before"],
                    f"{name}: what the project file stored is not the object's identifier",
                )
                rows.append(
                    {
                        "candidate": scenario["candidate"],
                        "mode": mode,
                        "scenario": name.split("/", 1)[1],
                        "change": scenario["change"],
                        "unity_type": reference["unity_type"],
                        "anchor": reference["anchor"],
                        "verdict": reference["verdict"],
                        "meaning_verdict": reference["meaning_verdict"],
                        "join_was_ambiguous": reference["join_was_ambiguous"],
                        "warning_transition": scenario["warning_transition"],
                        "display_name_before": reference["name_before"],
                        "display_name_after": reference["name_after"],
                        "display_name_changed": reference["display_name_changed"],
                        "unity_local_file_id_before": reference["local_file_id_before"],
                        "unity_local_file_id_after": reference["local_file_id_after"],
                        "unity_local_file_id_changed": reference["local_file_id_changed"],
                        "node_key_before": reference["node_key_before"],
                        "node_key_after": reference["node_key_after"],
                        "semantic_object_still_exported": reference[
                            "semantic_object_present_after"
                        ],
                    }
                )
            require(
                kinds_here == KINDS,
                f"{name}: only {sorted(kinds_here)} was measured, and Model, Mesh and Material "
                f"are three questions",
            )

    # ---- the control must be the same in both runs.
    #
    # It carries no designation, so the companion has nothing to rename. If its
    # identifiers moved anyway, the plugin is not inert and no comparison
    # between the two runs below would mean anything.
    control_vanilla = scenario_nodes(vanilla, "a-control")
    control_companion = scenario_nodes(companion, "a-control")
    require(
        control_vanilla == control_companion,
        "the companion postprocessor changed a document that carries no designation, so it "
        "is not inert and the two runs cannot be compared",
    )

    # ---- before or after the rename: the question candidate D exists to answer.
    control = scenario_nodes(vanilla, "a-control")
    internal = scenario_nodes(vanilla, "c-property")
    renamed = scenario_nodes(companion, "d-companion")
    require(
        set(control) == set(internal) == set(renamed),
        "the three candidates compared for the rename question do not cover the same nodes",
    )
    require(
        control != internal,
        "the control and the machine-named candidate produced the same identifiers, so this "
        "measurement cannot tell before-rename from after-rename at all",
    )
    # The imported model's own root is compared on its own, because Unity
    # names it after the asset file rather than after anything in the FBX, and
    # the three asset paths are three different names. Leaving it inside the
    # counts below would report a naming difference as a timing result.
    roots = [key for key in renamed if renamed[key]["is_root"]]
    require(len(roots) > 0, "no imported model root was identified")
    root_identifier_ignores_the_name = all(
        renamed[key]["node"] == internal[key]["node"] == control[key]["node"]
        and len({renamed[key]["node_name"], internal[key]["node_name"], control[key]["node_name"]}) == 3
        for key in roots
    )
    checks += 1

    timing = {}
    for kind in ("node", "mesh", "material"):
        counted = {"moved_by_the_rename": 0, "unmoved_by_the_rename": 0, "equal_to_the_control": 0}
        total = 0
        for key in renamed:
            if renamed[key]["is_root"]:
                continue
            # A node with no mesh, or no material, has nothing whose identifier
            # the rename could move, and counting those as "unmoved" would make
            # a structural frame look like evidence about geometry.
            if kind == "mesh" and renamed[key]["mesh"] == -1:
                continue
            if kind == "material" and not renamed[key]["material"]:
                continue
            total += 1
            if renamed[key][kind] == internal[key][kind]:
                counted["unmoved_by_the_rename"] += 1
            else:
                counted["moved_by_the_rename"] += 1
            if renamed[key][kind] == control[key][kind]:
                counted["equal_to_the_control"] += 1
            checks += 1
        if counted["unmoved_by_the_rename"] == total:
            verdict = "computed_before_the_postprocessor_rename"
        elif counted["moved_by_the_rename"] == total:
            verdict = "computed_after_the_postprocessor_rename"
        else:
            verdict = "mixed"
        timing[kind] = dict(counted, compared=total, verdict=verdict)
    require(
        "mixed" not in {item["verdict"] for item in timing.values()}
        or any(item["verdict"] != "mixed" for item in timing.values()),
        "every kind came out mixed, so the rename question was not separated at all",
    )

    postprocessor = {
        "local_file_id_timing": timing,
        "model_root_identifier_ignores_its_name": root_identifier_ignores_the_name,
        "visible_names_without_the_companion": next(
            summary["visible_names"]
            for summary in vanilla["candidates"]
            if summary["name"] == "c-property"
        ),
        "visible_names_with_the_companion": next(
            summary["visible_names"]
            for summary in companion["candidates"]
            if summary["name"] == "d-companion"
        ),
        "renamed_node_names": sorted(
            {renamed[key]["node_name"] for key in renamed}
        )[:8],
        "root_name_without_the_companion": next(
            summary["root_visible_name"]
            for summary in vanilla["candidates"]
            if summary["name"] == "c-property"
        ),
        "root_name_with_the_companion": next(
            summary["root_visible_name"]
            for summary in companion["candidates"]
            if summary["name"] == "d-companion"
        ),
    }

    # ---- the naming questions.
    require(
        {probe["name"] for probe in vanilla["names"]} == NAME_PROBES,
        "the naming questions measured are not the ones this slice asks",
    )
    names = []
    for probe in vanilla["names"]:
        read = files[f"names/{probe['name']}.fbx"]
        by_key = {node["node_key"]: node for node in read["nodes"]}
        require(
            set(by_key) == {row["node_key"] for row in probe["rows"]},
            f"{probe['name']}: the editor and the file disagree on which nodes exist",
        )
        outcomes: dict[str, int] = {}
        longest = 0
        model_names = {node["name"] for node in read["nodes"]}
        stem = probe["name"]

        def count(outcome: str) -> None:
            outcomes[outcome] = outcomes.get(outcome, 0) + 1

        for row in probe["rows"]:
            node = by_key[row["node_key"]]
            longest = max(longest, len(node["name"].encode("utf-8")))
            # The imported model's own root carries the asset's name, whatever
            # the file called it. Named rather than left in the catch-all,
            # because an unexplained outcome is what this table is looking for.
            if row["unity_node_name"] == stem:
                count("named_after_the_asset_file")
            else:
                count(classify(row["unity_node_name"], node["name"]))
            if row["unity_mesh_name"] != "<none>":
                if row["unity_mesh_name"] == node["name"]:
                    count("mesh_named_after_its_own_node")
                elif row["unity_mesh_name"] == node["geometry_name"]:
                    count("mesh_named_after_the_fbx_geometry")
                elif row["unity_mesh_name"] in model_names:
                    count("mesh_named_after_another_placement")
                else:
                    count("mesh_" + classify(row["unity_mesh_name"], node["name"]))
            for slot, observed in enumerate(row["unity_material_names"]):
                if slot < len(node["materials"]):
                    count("material_" + classify(observed, node["materials"][slot]["name"]))
            checks += 1
        # A distinct FBX object that arrived sharing another one's identifier
        # is the collision result, and it has to be counted rather than
        # described.
        identifiers = [row["node_local_file_id"] for row in probe["rows"]]
        identifiers += [
            row["mesh_local_file_id"] for row in probe["rows"] if row["mesh_local_file_id"] != -1
        ]
        for row in probe["rows"]:
            identifiers += row["material_local_file_ids"]

        # Two objects the file numbers separately that arrived sharing one
        # Unity identifier. This is what a token collision actually costs, and
        # it is counted rather than described.
        merged: dict[int, set[int]] = {}
        for row in probe["rows"]:
            node = by_key[row["node_key"]]
            merged.setdefault(row["node_local_file_id"], set()).add(node["object_number"])
            if row["mesh_local_file_id"] != -1:
                merged.setdefault(row["mesh_local_file_id"], set()).add(
                    node["geometry_object_number"]
                )
            for slot, identifier in enumerate(row["material_local_file_ids"]):
                if slot < len(node["materials"]):
                    merged.setdefault(identifier, set()).add(
                        node["materials"][slot]["object_number"]
                    )
        collapsed = sorted(
            sorted(numbers) for numbers in merged.values() if len(numbers) > 1
        )
        names.append(
            {
                "name": probe["name"],
                "question": probe["question"],
                "longest_file_name_bytes": longest,
                "file_non_ascii_names": read["facts"]["non_ascii_object_names"],
                "unity_outcomes": dict(sorted(outcomes.items())),
                "unity_names_that_repeat": probe["unity_names_that_repeat"],
                "distinct_file_objects_sharing_one_unity_identifier": collapsed,
                "subassets": probe["subassets"],
                "distinct_local_file_ids": probe["distinct_local_file_ids"],
                "distinct_tracked_identifiers": len(set(identifiers)),
                "tracked_identifiers": len(identifiers),
                "warnings": probe["warnings"],
            }
        )
    collision = next(item for item in names if item["name"] == "n05-hash-collision")
    require(
        collision["unity_names_that_repeat"] > 0
        or collision["distinct_file_objects_sharing_one_unity_identifier"],
        "the deliberate token collision produced neither a repeated name nor a merged "
        "identifier, so the collision was not actually in the file",
    )
    clean = next(item for item in names if item["name"] == "n04-short-hash")
    require(
        not clean["distinct_file_objects_sharing_one_unity_identifier"],
        "the token scheme merged two distinct objects even without a deliberate collision, "
        "so the collision case above proves nothing about collisions",
    )

    # ---- the decision table's evidence, per candidate.
    table = []
    for name, candidate in sorted(declared.items()):
        mode = candidate["mode"]
        summary = summaries[name + "/" + mode]
        mine = [row for row in rows if row["candidate"] == name]
        require(len(mine) > 0, f"{name}: no reference row survived the join")
        counts: dict[str, dict[str, int]] = {}
        for row in mine:
            counts.setdefault(row["unity_type"], {})
            counts[row["unity_type"]][row["verdict"]] = (
                counts[row["unity_type"]].get(row["verdict"], 0) + 1
            )
        # A candidate whose identity cannot tell two definitions apart has to
        # say so on the anchors as well as in its summary. A probe that
        # counted the ambiguity once and then quietly tracked one of the two
        # definitions as if it were the other would agree with itself here.
        ambiguous_rows = [row for row in mine if row["join_was_ambiguous"]]
        require(
            (summary["ambiguous_definitions"] > 0) == bool(ambiguous_rows),
            f"{name}: the candidate's ambiguity count and its tracked anchors disagree",
        )
        removed = [
            row
            for row in mine
            if row["scenario"] == "s06-remove-tracked-definition"
            and row["anchor"].endswith("step.product_definition#300")
        ]
        require(len(removed) > 0, f"{name}: the removed-object scenario tracked nothing")
        table.append(
            {
                "candidate": name,
                "mode": mode,
                "needs_companion": candidate["needs_companion"],
                "written_by": candidate["written_by"],
                "definition_join": summary["definition_join"],
                "occurrence_join": summary["occurrence_join"],
                "ambiguous_definitions": summary["ambiguous_definitions"],
                "ambiguous_definition_names": summary["ambiguous_definition_names"],
                "ambiguous_anchor_rows": len(ambiguous_rows),
                "visible_names": summary["visible_names"],
                "root_visible_name_is_the_asset_file_name": summary[
                    "root_name_is_the_asset_file_name"
                ],
                "mesh_named_after_the_fbx_model": summary["mesh_named_after_the_fbx_model"],
                "mesh_named_after_the_fbx_geometry": summary["mesh_named_after_the_fbx_geometry"],
                "mesh_named_after_neither": summary["mesh_named_after_neither"],
                "verdicts": {kind: dict(sorted(counts.get(kind, {}).items())) for kind in sorted(KINDS)},
                "removed_object_verdicts": sorted({row["verdict"] for row in removed}),
            }
        )

    decision = {
        "schema": "ferritecad.unity-channel-decision.v1",
        "multi_source_collision": {
            "source_local_key_collisions_in_the_file": base["definition_key_collisions"],
            "candidates_whose_identity_removes_it": sorted(
                item["candidate"]
                for item in table
                if item["ambiguous_definitions"] == 0
            ),
            "candidates_that_still_collide": sorted(
                item["candidate"]
                for item in table
                if item["ambiguous_definitions"] > 0
            ),
        },
        "candidates": table,
        "postprocessor": postprocessor,
        "names": names,
        "transitions": rows,
    }
    return checks, decision


def scenario_nodes(report: dict, candidate: str) -> dict:
    """Every node of every scenario of one candidate, by scenario and node key.

    The identifiers are what the rename question is decided on, so they are
    taken from the recorded `before` view of every scenario rather than from
    one import — a candidate whose identifiers only agree on the first document
    has not answered the question.
    """
    result: dict[str, dict] = {}
    for scenario in report["scenarios"]:
        if scenario["candidate"] != candidate:
            continue
        suffix = scenario["name"].split("/", 1)[1]
        for side in ("before", "after"):
            for node in scenario[side]["nodes"]:
                result[f"{suffix}/{side}/{node['node_key']}"] = {
                    "is_root": node["sibling_path"] == "0",
                    "node": node["local_file_id"],
                    "node_name": node["unity_name"],
                    "mesh": node["mesh_local_file_id"],
                    "material": tuple(node["material_local_file_ids"]),
                }
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vanilla", type=Path, required=True)
    parser.add_argument("--companion", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--vanilla-plan", type=Path, required=True)
    parser.add_argument("--companion-plan", type=Path, required=True)
    parser.add_argument("--emit", type=Path)
    parser.add_argument("--expected", type=Path)
    args = parser.parse_args()

    def load(path: Path) -> dict:
        return json.loads(path.read_text(encoding="utf-8"))

    checks, decision = verify(
        load(args.vanilla),
        load(args.companion),
        load(args.oracle),
        {"vanilla": load(args.vanilla_plan), "companion": load(args.companion_plan)},
    )
    if checks == 0:
        raise SystemExit("the join verifier ran zero checks")
    if args.emit is not None:
        if not decision["transitions"]:
            raise SystemExit("the joined transition table is empty")
        args.emit.write_text(
            json.dumps(decision, indent=1, sort_keys=True) + "\n", encoding="utf-8"
        )
    if args.expected is not None:
        if args.emit is None:
            raise SystemExit("nothing was emitted to compare with the committed table")
        if args.emit.read_bytes() != args.expected.read_bytes():
            raise SystemExit("the joined decision record differs from the committed measurement")
    print(f"FCAD_CHANNEL_JOIN_VERIFIED checks={checks}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
