// SPDX-License-Identifier: MIT
//
// The §22B-1e2b entry point.
//
// Four questions §22B-1e2a left open, each measured in its own editor run in
// its own freshly created project, because three of them change what the
// editor *is*: a `.meta` probe that edits serialized importer metadata, an
// `AddRemap` probe that puts external assets in the project, and a
// `ScriptedImporter` probe that registers an importer. Running them in one
// project would make each one's result a property of the others.
//
// Every mode writes one canonical report, and the runner insists two clean
// projects produce byte-identical ones.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using UnityEditor;
using UnityEngine;

internal static class FerriteGraphIdentity
{
    public static void Run()
    {
        try
        {
            Execute();
        }
        catch (Exception error)
        {
            Debug.LogError("FCAD_GRAPH_IDENTITY_FAILURE " + error);
            EditorApplication.Exit(1);
        }
    }

    // The check count a mode's report carries, asserted to be a real one.
    // Sealed before the report is written, so nothing a later comparison adds
    // can drift away from the number in the file.
    private static int Seal(string mode, int minimum)
    {
        FerriteGraphCommon.Require(
            FerriteGraphCommon.Checks > minimum,
            "the " + mode + " probe performed too few checks: "
                + FerriteGraphCommon.Checks.ToString(CultureInfo.InvariantCulture));
        return FerriteGraphCommon.Checks;
    }

    private static void Execute()
    {
        FerriteGraphCommon.Reset();

        string mode = FerriteGraphCommon.ArgumentValue("-fcadMode")
            ?? throw new InvalidOperationException("no -fcadMode was given");
        string planPath = FerriteGraphCommon.ArgumentValue("-fcadPlan")
            ?? throw new InvalidOperationException("no -fcadPlan was given");
        string output = FerriteGraphCommon.ArgumentValue("-fcadOutput")
            ?? throw new InvalidOperationException("no -fcadOutput was given");
        string expected = FerriteGraphCommon.ArgumentValue("-fcadExpected");

        // The failing-first run asserts the contract §22B-1e2b is about: that
        // the graph delivers a source-qualified definition identity, one
        // shared `Mesh` per definition, human-readable visible names, and a
        // durable placement identity, all at once. It is meant to fail on the
        // production graph, and it is kept so the claim can be reproduced
        // rather than remembered.
        bool expectContract = FerriteGraphCommon.HasArgument("-fcadExpectFullContract");

        string json;
        int recorded;
        switch (mode)
        {
            case "graph":
            {
                FerriteGraphProbe.Report report = FerriteGraphProbe.Execute(planPath);
                if (expectContract)
                {
                    FerriteGraphContract.Assert(report);
                }
                recorded = Seal("graph", 500);
                report.checks = recorded;
                json = JsonUtility.ToJson(report, true) + "\n";
                break;
            }
            case "meta":
            {
                FerriteMetaProbe.Report report = FerriteMetaProbe.Execute(planPath);
                recorded = Seal("meta", 40);
                report.checks = recorded;
                json = JsonUtility.ToJson(report, true) + "\n";
                break;
            }
            case "remap":
            {
                FerriteRemapProbe.Report report = FerriteRemapProbe.Execute(planPath);
                recorded = Seal("remap", 60);
                report.checks = recorded;
                json = JsonUtility.ToJson(report, true) + "\n";
                break;
            }
            case "scripted":
            {
                FerriteScriptedProbe.Report report = FerriteScriptedProbe.Execute(planPath);
                recorded = Seal("scripted", 60);
                report.checks = recorded;
                json = JsonUtility.ToJson(report, true) + "\n";
                break;
            }
            case "fbxclaim":
            {
                FerriteFbxClaimProbe.Report report = FerriteFbxClaimProbe.Execute(planPath);
                recorded = Seal("fbxclaim", 3);
                report.checks = recorded;
                json = JsonUtility.ToJson(report, true) + "\n";
                break;
            }
            default:
                throw new InvalidOperationException("unknown -fcadMode: " + mode);
        }

        string directory = Path.GetDirectoryName(output);
        if (!String.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }
        File.WriteAllText(output, json, new UTF8Encoding(false));

        if (!String.IsNullOrEmpty(expected))
        {
            FerriteGraphCommon.Require(
                File.Exists(expected), "the committed expected " + mode + " report is missing");
            string committed = File.ReadAllText(expected).Replace("\r\n", "\n");
            FerriteGraphCommon.Require(
                committed == json,
                "the " + mode + " report differs from the committed measurement");
        }

        // The anchor the run verifier greps for; the shape of this line is
        // its contract, so the check count comes first and the mode after it.
        // It is the number *inside* the report, not the counter as it stands
        // now: the comparison with the committed measurement above is itself a
        // check, and a log line that counted it would never match the file.
        Debug.Log("FCAD_GRAPH_IDENTITY_EXECUTED checks="
            + recorded.ToString(CultureInfo.InvariantCulture)
            + " mode=" + mode);
        EditorApplication.Exit(0);
    }
}

// The whole contract, in one place, asserted against a real import.
//
// Kept apart from the probe so it cannot quietly become a summary of whatever
// the probe measured. It is the list §22B-1e2b was asked for, and the run that
// asserts it against the production graph is the failing-first evidence.
internal static class FerriteGraphContract
{
    // Every clause, against every graph, collected rather than thrown on the
    // first failure. A failing-first run that stopped at the control would
    // leave "and every other graph fails it too" as a claim instead of a
    // record.
    internal static void Assert(FerriteGraphProbe.Report report)
    {
        List<string> failures = new List<string>();
        foreach (FerriteGraphProbe.VariantReport variant in report.variants)
        {
            string prefix = "the graph " + variant.name + " ";
            Fail(
                failures,
                variant.ambiguous_definitions == 0,
                prefix + "cannot tell two definitions with one source-local key apart: "
                    + String.Join(", ", variant.ambiguous_definition_names));
            Fail(
                failures,
                variant.definitions_whose_placements_share_one_mesh
                    == variant.definitions_with_several_placements,
                prefix + "does not give every placement of a definition one shared Mesh: "
                    + String.Join(", ", variant.definitions_with_a_split_mesh));
            Fail(
                failures,
                variant.visible_nodes_named_by_machine_token == 0,
                prefix + "shows a person "
                    + variant.visible_nodes_named_by_machine_token.ToString(
                        CultureInfo.InvariantCulture)
                    + " machine-named nodes");
            Fail(
                failures,
                variant.subassets_named_by_machine_token == 0,
                prefix + "shows a person "
                    + variant.subassets_named_by_machine_token.ToString(
                        CultureInfo.InvariantCulture)
                    + " machine-named sub-assets");
            Fail(
                failures,
                variant.occurrence_join == "FerriteCADOccurrenceId",
                prefix + "identifies a placement by " + variant.occurrence_join
                    + ", which moves when a sibling does");
            Fail(
                failures,
                !variant.import_root_is_synthetic,
                prefix + "made Unity invent a root, which is one more object a person sees");
            Fail(
                failures,
                variant.warnings.Count == 0,
                prefix + "imports with warnings: " + String.Join(" | ", variant.warnings));
        }

        foreach (FerriteGraphProbe.ScenarioReport scenario in report.scenarios)
        {
            bool removal = scenario.change.Contains("the tracked definition and its only");
            string wanted = removal ? "missing_because_object_was_removed" : "same_semantic";
            foreach (FerriteGraphProbe.ReferenceReport reference in scenario.references)
            {
                Fail(
                    failures,
                    reference.verdict == wanted,
                    scenario.name + ": the reference " + reference.anchor + " came back "
                        + reference.verdict + " where the contract needs " + wanted);
            }
        }

        if (failures.Count > 0)
        {
            throw new InvalidOperationException(
                "the whole contract fails on "
                    + failures.Count.ToString(CultureInfo.InvariantCulture)
                    + " clauses; the first twenty are:\n  "
                    + String.Join("\n  ", failures.Take(20)));
        }
    }

    private static void Fail(List<string> failures, bool condition, string message)
    {
        ++FerriteGraphCommon.Checks;
        if (!condition)
        {
            failures.Add(message);
        }
    }
}
