// SPDX-License-Identifier: MIT
//
// The §22B-1e2a measurement: is there a channel that keeps a Unity reference
// across a real re-export while the names a person reads stay the names a
// person recognises?
//
// §22B-1e1 answered a narrower question on a narrower document. It measured one
// imported source, so the source-local `FerriteCADDefinitionKey` was
// unambiguous inside the files it measured, and it could not say anything
// about a document containing two sources that use the same local key. It also
// left open whether identity can stop being the visible name at all. Both are
// measured here, and neither is decided here: this probe chooses no policy.
//
// Five rules, because breaking any one of them would make a candidate look
// better than it is:
//
//   1. Non-null is not survival. A resolved object must still mean the same
//      FerriteCAD definition, cross-checked against a witness the identity
//      scheme did not supply — a vertex count for a mesh, a colour for a
//      material, a placement's own translation for a `GameObject`.
//   2. An identity that cannot tell two definitions apart is reported as
//      ambiguous, never as a kept reference. That is what the production
//      property does to the two documents that share `#42`.
//   3. Geometry, Model and Material are measured separately, always. A result
//      about one of them is not a result about the others.
//   4. The visible names are part of the measurement. A candidate that keeps
//      every reference by showing a person `fcad~019ffc72-...` has not solved
//      the problem, and this probe records what the hierarchy actually reads.
//   5. Anything that needs the companion postprocessor is recorded as needing
//      it. The same bytes are imported with and without the plugin, in
//      separate editor runs, and the report says which one it was.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;
using UnityEditor;
using UnityEngine;

internal static class FerriteChannelIdentity
{
    private const string AssetFolder = "Assets/Channel";

    // Every candidate name in this measurement begins with this. It is what
    // makes "would a person see a machine token" a measurable question rather
    // than a matter of opinion.
    private const string MachinePrefix = "fcad~";

    // ------------------------------------------------------------ the plan

    [Serializable]
    private sealed class Plan
    {
        public List<PlanCandidate> candidates = new List<PlanCandidate>();
        public List<PlanScenario> scenarios = new List<PlanScenario>();
        public List<PlanName> names = new List<PlanName>();
    }

    [Serializable]
    private sealed class PlanCandidate
    {
        public string name = String.Empty;
        public string written_by = String.Empty;
        // What the candidate claims its files carry. Claimed, not trusted: the
        // probe measures what the import really delivered and the verifier
        // refuses a disagreement.
        public bool carries_definition_id;
        public bool carries_occurrence_id;
        public bool carries_display_name;
        public bool needs_companion;
    }

    [Serializable]
    private sealed class PlanScenario
    {
        public string name = String.Empty;
        public string candidate = String.Empty;
        public string change = String.Empty;
        public string before = String.Empty;
        public string after = String.Empty;
        // Source-qualified definition identities, the same list for every
        // candidate. A candidate whose files cannot express one of them is a
        // candidate whose anchor comes back ambiguous, which is the result.
        public List<string> mesh_definitions = new List<string>();
        public List<string> material_bindings = new List<string>();
        public List<string> object_bindings = new List<string>();
    }

    [Serializable]
    private sealed class PlanName
    {
        public string name = String.Empty;
        public string question = String.Empty;
        public string file = String.Empty;
    }

    // ---------------------------------------------------------- the report

    [Serializable]
    private sealed class Report
    {
        public string schema = "ferritecad.unity-channel-identity.v1";
        public string unity_version = String.Empty;
        public string colour_space = String.Empty;
        // Whether the FerriteCAD companion postprocessor renamed anything in
        // this editor run. Written at the top of the report so no reader can
        // mistake a plugin result for vanilla import behaviour.
        public bool companion_active;
        public int distinct_asset_guids;
        public int subassets_whose_identifier_is_the_guid_and_local_id;
        public int subassets_whose_identifier_is_something_else;
        public SortControlReport sort_control = new SortControlReport();
        public List<CandidateReport> candidates = new List<CandidateReport>();
        public List<ScenarioReport> scenarios = new List<ScenarioReport>();
        public List<NameReport> names = new List<NameReport>();
        public int checks;
    }

    [Serializable]
    private sealed class SortControlReport
    {
        public List<SubassetReport> with_default_sort = new List<SubassetReport>();
        public List<SubassetReport> with_sort_disabled = new List<SubassetReport>();
        public bool identifiers_are_unchanged;
        public List<string> hierarchy_with_default_sort = new List<string>();
        public List<string> hierarchy_with_sort_disabled = new List<string>();
    }

    // What one candidate's base document actually delivered to the editor.
    [Serializable]
    private sealed class CandidateReport
    {
        public string name = String.Empty;
        // Which property the join really used, measured from the import rather
        // than taken from the plan.
        public string definition_join = String.Empty;
        public string occurrence_join = String.Empty;
        public int nodes;
        public int nodes_with_definition_id;
        public int nodes_with_occurrence_id;
        public int nodes_with_display_name;
        // How many resolved definition identities name more than one geometry.
        // Not zero is the whole §22B-1e2a problem.
        public int ambiguous_definitions = -1;
        public List<string> ambiguous_definition_names = new List<string>();
        // What a person would read in the hierarchy and the asset list.
        //
        // The imported model's own root is left out of these lists and
        // recorded on its own two lines below, because Unity names it after
        // the asset file and not after anything the FBX says. That is a
        // measured limit of the name channel, not a candidate failing.
        public string root_visible_name = String.Empty;
        public bool root_name_is_the_asset_file_name;
        public int subassets_named_after_the_asset_file;
        public int subassets_named_by_machine_token;
        public int subassets_named_by_designation;
        // Whether the mesh Unity published under each node is named after the
        // node or after the FBX geometry. These documents give the two
        // different names on purpose, so this is measurable rather than
        // inferred.
        public int meshes_named_after_their_node;
        public int meshes_named_otherwise;
        public List<string> visible_node_names = new List<string>();
        public List<string> visible_mesh_names = new List<string>();
        public List<string> visible_material_names = new List<string>();
    }

    [Serializable]
    private sealed class ScenarioReport
    {
        public string name = String.Empty;
        public string candidate = String.Empty;
        public string change = String.Empty;
        public bool files_are_byte_identical;
        public long before_bytes;
        public string before_fnv1a64 = String.Empty;
        public long after_bytes;
        public string after_fnv1a64 = String.Empty;
        public List<string> warnings_before = new List<string>();
        public List<string> warnings_after = new List<string>();
        public string warning_transition = String.Empty;
        public ViewReport before = new ViewReport();
        public ViewReport after = new ViewReport();
        public List<ReferenceReport> references = new List<ReferenceReport>();
    }

    [Serializable]
    private sealed class ViewReport
    {
        public List<SubassetReport> subassets = new List<SubassetReport>();
        public List<NodeReport> nodes = new List<NodeReport>();
    }

    [Serializable]
    private sealed class SubassetReport
    {
        public string unity_type = String.Empty;
        public string unity_name = String.Empty;
        public string asset_guid = String.Empty;
        public long local_file_id;
    }

    [Serializable]
    private sealed class NodeReport
    {
        public string sibling_path = String.Empty;
        public string unity_name = String.Empty;
        public long local_file_id;
        public string node_key = String.Empty;
        public string definition_key = String.Empty;
        public string source_id = String.Empty;
        public string definition_id = String.Empty;
        public string occurrence_id = String.Empty;
        public string display_name = String.Empty;
        public string omission = String.Empty;
        public string resolved_definition = String.Empty;
        public string resolved_occurrence = String.Empty;
        public long mesh_local_file_id;
        public string mesh_unity_name = String.Empty;
        public int mesh_vertex_count;
        public List<long> material_local_file_ids = new List<long>();
        public List<string> material_unity_names = new List<string>();
    }

    [Serializable]
    private sealed class ReferenceReport
    {
        public string anchor = String.Empty;
        public string unity_type = String.Empty;
        // Whether the candidate's identity could name this object at all. An
        // anchor that matches two different definitions is not a reference
        // that survived and not one that broke: it is one that was never
        // expressible, and it is reported as that.
        public bool join_was_ambiguous;
        public string semantic_before = String.Empty;
        public string name_before = String.Empty;
        public long local_file_id_before;
        public string global_object_id_before = String.Empty;
        [NonSerialized]
        public string raw_global_object_id = String.Empty;
        public long stored_file_id;
        public string stored_guid = String.Empty;
        public string resolved_by_reloaded_asset = String.Empty;
        public string resolved_by_stored_identifier = String.Empty;
        public string semantic_after = String.Empty;
        public string name_after = String.Empty;
        public long local_file_id_after;
        public bool semantic_object_present_after;
        public string name_of_semantic_after = String.Empty;
        public long local_file_id_of_semantic_after;
        public string node_key_before = String.Empty;
        public string node_key_after = String.Empty;
        public bool display_name_changed;
        public bool local_file_id_changed;
        public bool node_key_changed;
        // What the meaning comparison says, kept even when the join was
        // ambiguous, so an ambiguous row still records what actually happened.
        public string meaning_verdict = String.Empty;
        public string verdict = String.Empty;
    }

    // One naming question, measured on one import.
    [Serializable]
    private sealed class NameReport
    {
        public string name = String.Empty;
        public string question = String.Empty;
        public long bytes;
        public string fnv1a64 = String.Empty;
        public List<NameRow> rows = new List<NameRow>();
        public int subassets;
        public int distinct_local_file_ids;
        // Two objects the file named differently that arrived as one Unity
        // name, and objects Unity had to disambiguate with a suffix.
        public int unity_names_that_repeat;
        public List<string> warnings = new List<string>();
    }

    [Serializable]
    private sealed class NameRow
    {
        public string node_key = String.Empty;
        public string unity_node_name = String.Empty;
        public int unity_node_name_bytes;
        public long node_local_file_id;
        public string unity_mesh_name = String.Empty;
        public int unity_mesh_name_bytes;
        public long mesh_local_file_id;
        public List<string> unity_material_names = new List<string>();
        public List<long> material_local_file_ids = new List<long>();
    }

    // ------------------------------------------------------- the in-memory view

    private sealed class NodeInfo
    {
        public string Path = String.Empty;
        public GameObject Target;
        public string NodeKey = String.Empty;
        public string DefinitionKey = String.Empty;
        public string SourceId = String.Empty;
        public string DefinitionId = String.Empty;
        public string OccurrenceId = String.Empty;
        public string DisplayName = String.Empty;
        public string Omission = String.Empty;
        public Mesh SharedMesh;
        public Material[] Materials = Array.Empty<Material>();
        public long LocalId;
        public long MeshLocalId = -1L;
        public long[] MaterialLocalIds = Array.Empty<long>();
        public Vector3 LocalPosition;
    }

    private sealed class View
    {
        public List<NodeInfo> Nodes = new List<NodeInfo>();
        public List<UnityEngine.Object> Subassets = new List<UnityEngine.Object>();
    }

    private static int checks;
    private static int derivedIdentifiers;
    private static int otherIdentifiers;
    private static readonly Dictionary<string, string> GuidTokens = new Dictionary<string, string>();

    public static void Run()
    {
        try
        {
            Execute();
        }
        catch (Exception error)
        {
            Debug.LogError("FCAD_CHANNEL_IDENTITY_FAILURE " + error);
            EditorApplication.Exit(1);
        }
    }

    private static void Execute()
    {
        checks = 0;
        derivedIdentifiers = 0;
        otherIdentifiers = 0;
        GuidTokens.Clear();

        string planPath = ArgumentValue("-fcadPlan")
            ?? throw new InvalidOperationException("no -fcadPlan was given");
        string output = ArgumentValue("-fcadOutput")
            ?? throw new InvalidOperationException("no -fcadOutput was given");
        string expected = ArgumentValue("-fcadExpected");
        // The failing-first run asserts the optimistic contract: that today's
        // exported identity tells every definition of the document apart. It
        // is meant to fail on the control, and it is kept so the claim can be
        // reproduced rather than remembered.
        bool expectDurableJoin = HasArgument("-fcadExpectDurableJoin");

        Plan plan = JsonUtility.FromJson<Plan>(File.ReadAllText(planPath));
        Require(plan != null && plan.scenarios.Count > 0, "the plan named no scenario");
        Require(plan.candidates.Count > 0, "the plan named no candidate");

        if (!AssetDatabase.IsValidFolder(AssetFolder))
        {
            AssetDatabase.CreateFolder("Assets", "Channel");
        }

        Report report = new Report
        {
            unity_version = Application.unityVersion,
            colour_space = QualitySettings.activeColorSpace.ToString().ToLowerInvariant(),
            companion_active = FerriteChannelProperties.CompanionActive,
        };

        Dictionary<string, PlanCandidate> candidates = new Dictionary<string, PlanCandidate>();
        foreach (PlanCandidate candidate in plan.candidates)
        {
            candidates[candidate.name] = candidate;
            Require(
                candidate.needs_companion == report.companion_active
                    || !candidate.needs_companion,
                "a candidate that needs the companion postprocessor was planned into a "
                    + "vanilla editor run: " + candidate.name);
            report.candidates.Add(new CandidateReport { name = candidate.name });
        }

        report.sort_control = MeasureSortControl(plan.scenarios[0].before);

        Dictionary<string, CandidateReport> measured = report.candidates
            .ToDictionary(item => item.name, item => item, StringComparer.Ordinal);
        foreach (PlanScenario scenario in plan.scenarios)
        {
            Require(
                candidates.ContainsKey(scenario.candidate),
                "the plan measures a scenario of an undeclared candidate: " + scenario.candidate);
            report.scenarios.Add(
                MeasureScenario(scenario, candidates[scenario.candidate], measured));
        }
        foreach (PlanName probe in plan.names)
        {
            report.names.Add(MeasureName(probe));
        }

        foreach (CandidateReport candidate in report.candidates)
        {
            Require(
                candidate.ambiguous_definitions >= 0,
                "no scenario measured the candidate " + candidate.name);
            if (expectDurableJoin)
            {
                Require(
                    candidate.ambiguous_definitions == 0,
                    "the exported identity of " + candidate.name + " cannot tell two "
                        + "definitions apart: "
                        + String.Join(", ", candidate.ambiguous_definition_names));
            }
        }

        report.distinct_asset_guids = GuidTokens.Count;
        report.subassets_whose_identifier_is_the_guid_and_local_id = derivedIdentifiers;
        report.subassets_whose_identifier_is_something_else = otherIdentifiers;
        Require(derivedIdentifiers > 0, "no sub-asset identifier was examined at all");
        Require(checks > 500, "the probe performed too few checks");

        report.checks = checks;
        string json = JsonUtility.ToJson(report, true) + "\n";
        string directory = Path.GetDirectoryName(output);
        if (!String.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }
        File.WriteAllText(output, json, new UTF8Encoding(false));

        if (!String.IsNullOrEmpty(expected))
        {
            Require(File.Exists(expected), "the committed expected channel report is missing");
            string committed = File.ReadAllText(expected).Replace("\r\n", "\n");
            Require(committed == json, "the channel report differs from the committed measurement");
        }

        Debug.Log("FCAD_CHANNEL_IDENTITY_EXECUTED checks="
            + report.checks.ToString(CultureInfo.InvariantCulture));
        EditorApplication.Exit(0);
    }

    // ------------------------------------------------------ the sort control

    private static SortControlReport MeasureSortControl(string source)
    {
        string assetPath = AssetFolder + "/sort-control.fbx";
        string absolute = Path.GetFullPath(assetPath);
        File.Copy(source, absolute, true);

        AssetDatabase.ImportAsset(
            assetPath,
            ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        ModelImporter importer = AssetImporter.GetAtPath(assetPath) as ModelImporter;
        Require(importer != null, "the sort control got no ModelImporter");
        Require(
            importer.sortHierarchyByName,
            "this editor no longer sorts an imported hierarchy by name by default, so the "
                + "control below is measuring something else than it was written for");

        SortControlReport control = new SortControlReport();
        control.with_default_sort = Subassets(assetPath);
        control.hierarchy_with_default_sort = Hierarchy(assetPath);

        importer.sortHierarchyByName = false;
        importer.SaveAndReimport();
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        control.with_sort_disabled = Subassets(assetPath);
        control.hierarchy_with_sort_disabled = Hierarchy(assetPath);

        control.identifiers_are_unchanged =
            control.with_default_sort.Count == control.with_sort_disabled.Count
            && control.with_default_sort
                .Select(item => item.unity_type + "|" + item.unity_name + "|" + item.local_file_id)
                .OrderBy(item => item, StringComparer.Ordinal)
                .SequenceEqual(control.with_sort_disabled
                    .Select(item => item.unity_type + "|" + item.unity_name + "|" + item.local_file_id)
                    .OrderBy(item => item, StringComparer.Ordinal));
        ++checks;

        AssetDatabase.DeleteAsset(assetPath);
        return control;
    }

    private static List<SubassetReport> Subassets(string assetPath)
    {
        List<SubassetReport> result = new List<SubassetReport>();
        foreach (UnityEngine.Object item in AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .OrderBy(item => item.GetType().Name, StringComparer.Ordinal)
            .ThenBy(item => item.name, StringComparer.Ordinal)
            .ThenBy(LocalId))
        {
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(item, out string guid, out long local);
            result.Add(new SubassetReport
            {
                unity_type = item.GetType().Name,
                unity_name = item.name,
                asset_guid = GuidToken(guid),
                local_file_id = local,
            });
            CountIdentifierShape(item, GuidToken(guid), local);
            ++checks;
        }
        return result;
    }

    private static List<string> Hierarchy(string assetPath)
    {
        GameObject root = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
        Require(root != null, "the control asset published no GameObject");
        List<string> names = new List<string>();
        foreach (Transform transform in root.GetComponentsInChildren<Transform>(true))
        {
            names.Add(transform.name);
        }
        return names;
    }

    // ------------------------------------------------------- the name probes

    private static NameReport MeasureName(PlanName probe)
    {
        string assetPath = AssetFolder + "/" + probe.name + ".fbx";
        string absolute = Path.GetFullPath(assetPath);
        Require(File.Exists(probe.file), "a name probe's file is missing: " + probe.file);
        File.Copy(probe.file, absolute, true);

        NameReport report = new NameReport
        {
            name = probe.name,
            question = probe.question,
            bytes = new FileInfo(probe.file).Length,
            fnv1a64 = Fingerprint(probe.file),
        };
        SettleImporter(assetPath);
        report.warnings = Import(assetPath);
        View view = BuildView(assetPath);

        foreach (NodeInfo node in view.Nodes)
        {
            NameRow row = new NameRow
            {
                node_key = node.NodeKey,
                unity_node_name = node.Target.name,
                unity_node_name_bytes = Encoding.UTF8.GetByteCount(node.Target.name),
                node_local_file_id = node.LocalId,
                unity_mesh_name = node.SharedMesh == null ? "<none>" : node.SharedMesh.name,
                unity_mesh_name_bytes = node.SharedMesh == null
                    ? 0
                    : Encoding.UTF8.GetByteCount(node.SharedMesh.name),
                mesh_local_file_id = node.MeshLocalId,
                unity_material_names = node.Materials
                    .Select(material => material == null ? "<none>" : material.name)
                    .ToList(),
                material_local_file_ids = node.MaterialLocalIds.ToList(),
            };
            report.rows.Add(row);
            ++checks;
        }

        List<SubassetReport> subassets = Subassets(assetPath);
        report.subassets = subassets.Count;
        report.distinct_local_file_ids = subassets
            .Select(item => item.local_file_id)
            .Distinct()
            .Count();
        report.unity_names_that_repeat = subassets
            .GroupBy(item => item.unity_type + "|" + item.unity_name, StringComparer.Ordinal)
            .Count(group => group.Count() > 1);
        ++checks;

        AssetDatabase.DeleteAsset(assetPath);
        return report;
    }

    // ---------------------------------------------------------- the scenarios

    private static ScenarioReport MeasureScenario(
        PlanScenario scenario,
        PlanCandidate candidate,
        Dictionary<string, CandidateReport> measured)
    {
        string assetPath = AssetFolder + "/" + scenario.name.Replace('/', '_') + ".fbx";
        string referencePath = AssetFolder + "/" + scenario.name.Replace('/', '_')
            + "-references.asset";
        string absolute = Path.GetFullPath(assetPath);

        Require(File.Exists(scenario.before), "the scenario's first file is missing");
        Require(File.Exists(scenario.after), "the scenario's second file is missing");

        ScenarioReport result = new ScenarioReport
        {
            name = scenario.name,
            candidate = scenario.candidate,
            change = scenario.change,
            files_are_byte_identical = SameBytes(scenario.before, scenario.after),
            before_bytes = new FileInfo(scenario.before).Length,
            before_fnv1a64 = Fingerprint(scenario.before),
            after_bytes = new FileInfo(scenario.after).Length,
            after_fnv1a64 = Fingerprint(scenario.after),
        };

        File.Copy(scenario.before, absolute, true);
        SettleImporter(assetPath);
        result.warnings_before = Import(assetPath);
        View before = BuildView(assetPath);
        result.before = Describe(before, candidate);

        // What this candidate's identity really is, recorded once from the
        // first scenario that measures it. Taken from the import, not from the
        // plan: a candidate that claims to carry a source-qualified identity
        // and does not would be caught here rather than believed.
        CandidateReport summary = measured[scenario.candidate];
        if (summary.ambiguous_definitions < 0)
        {
            Summarise(summary, candidate, before, assetPath);
        }

        List<ReferenceReport> references = new List<ReferenceReport>();
        List<Mesh> meshes = new List<Mesh>();
        List<Material> materials = new List<Material>();
        List<GameObject> objects = new List<GameObject>();
        List<string> semantics = new List<string>();
        List<string> kinds = new List<string>();

        foreach (string definition in scenario.mesh_definitions)
        {
            List<NodeInfo> matches = Matching(before, definition);
            Require(
                matches.Count > 0,
                "no imported object carries the tracked definition " + definition);
            NodeInfo node = matches[0];
            Require(
                node.SharedMesh != null,
                "the tracked definition " + definition + " arrived without a mesh");
            meshes.Add(node.SharedMesh);
            semantics.Add(MeshSemantic(before, node.MeshLocalId));
            kinds.Add("Mesh");
            ReferenceReport tracked = Anchor("mesh:" + definition, "Mesh", node.SharedMesh);
            tracked.join_was_ambiguous = Ambiguous(before, node);
            references.Add(tracked);
        }
        foreach (string binding in scenario.material_bindings)
        {
            string[] pieces = binding.Split('@');
            Require(pieces.Length == 2, "a material binding is not 'definition@slot'");
            int slot = int.Parse(pieces[1], CultureInfo.InvariantCulture);
            List<NodeInfo> matches = Matching(before, pieces[0]);
            Require(matches.Count > 0, "no imported object carries " + pieces[0]);
            NodeInfo node = matches[0];
            Require(
                slot < node.Materials.Length,
                "the tracked material slot " + binding + " is not there; the imported node named "
                    + node.Target.name + " has "
                    + node.Materials.Length.ToString(CultureInfo.InvariantCulture) + " slots");
            materials.Add(node.Materials[slot]);
            semantics.Add(MaterialSemantic(before, node.MaterialLocalIds[slot]));
            kinds.Add("Material");
            ReferenceReport tracked = Anchor("material:" + binding, "Material", node.Materials[slot]);
            tracked.join_was_ambiguous = Ambiguous(before, node);
            references.Add(tracked);
        }
        foreach (string binding in scenario.object_bindings)
        {
            string[] pieces = binding.Split('@');
            Require(pieces.Length == 2, "an object binding is not 'definition@occurrence'");
            int occurrence = int.Parse(pieces[1], CultureInfo.InvariantCulture);
            List<NodeInfo> matches = Matching(before, pieces[0]);
            Require(
                occurrence < matches.Count,
                "no imported object carries the tracked occurrence " + binding);
            NodeInfo node = matches[occurrence];
            objects.Add(node.Target);
            semantics.Add(ObjectSemantic(before, node.LocalId));
            kinds.Add("GameObject");
            ReferenceReport tracked = Anchor("object:" + binding, "GameObject", node.Target);
            tracked.node_key_before = node.NodeKey;
            tracked.join_was_ambiguous = Ambiguous(before, node);
            references.Add(tracked);
        }
        for (int index = 0; index < references.Count; ++index)
        {
            references[index].semantic_before = semantics[index];
        }

        // ---- a real asset, written to disk, holding those references.
        AssetDatabase.DeleteAsset(referencePath);
        FerriteChannelReferences holder = ScriptableObject.CreateInstance<FerriteChannelReferences>();
        holder.meshes = meshes;
        holder.materials = materials;
        holder.objects = objects;
        AssetDatabase.CreateAsset(holder, referencePath);
        AssetDatabase.SaveAssets();
        List<StoredReference> stored = ReadStoredReferences(referencePath);
        Require(
            stored.Count == references.Count,
            "the saved asset did not write one persistent reference per tracked object");
        for (int index = 0; index < references.Count; ++index)
        {
            references[index].stored_file_id = stored[index].FileId;
            references[index].stored_guid = GuidToken(stored[index].Guid);
            Require(
                stored[index].FileId == references[index].local_file_id_before,
                "the file identifier a reference stores is not the object's local file identifier");
        }

        // ---- the document after the change, imported over the same path.
        File.Copy(scenario.after, absolute, true);
        result.warnings_after = Import(assetPath);
        result.warning_transition = Transition(result.warnings_before, result.warnings_after);

        AssetDatabase.ImportAsset(
            referencePath,
            ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        FerriteChannelReferences reloaded =
            AssetDatabase.LoadAssetAtPath<FerriteChannelReferences>(referencePath);
        Require(reloaded != null, "the asset holding the references did not come back");

        View after = BuildView(assetPath);
        result.after = Describe(after, candidate);

        int meshIndex = 0;
        int materialIndex = 0;
        int objectIndex = 0;
        for (int index = 0; index < references.Count; ++index)
        {
            ReferenceReport reference = references[index];
            UnityEngine.Object resolved;
            switch (kinds[index])
            {
                case "Mesh":
                    resolved = meshIndex < reloaded.meshes.Count ? reloaded.meshes[meshIndex] : null;
                    ++meshIndex;
                    break;
                case "Material":
                    resolved = materialIndex < reloaded.materials.Count
                        ? reloaded.materials[materialIndex]
                        : null;
                    ++materialIndex;
                    break;
                default:
                    resolved = objectIndex < reloaded.objects.Count
                        ? reloaded.objects[objectIndex]
                        : null;
                    ++objectIndex;
                    break;
            }
            Resolve(reference, resolved, after);
        }
        result.references = references;

        AssetDatabase.DeleteAsset(referencePath);
        AssetDatabase.DeleteAsset(assetPath);
        return result;
    }

    private static void Summarise(
        CandidateReport summary,
        PlanCandidate candidate,
        View view,
        string assetPath)
    {
        summary.nodes = view.Nodes.Count;
        summary.nodes_with_definition_id = view.Nodes.Count(node => node.DefinitionId.Length > 0);
        summary.nodes_with_occurrence_id = view.Nodes.Count(node => node.OccurrenceId.Length > 0);
        summary.nodes_with_display_name = view.Nodes.Count(node => node.DisplayName.Length > 0);
        summary.definition_join = summary.nodes_with_definition_id == summary.nodes
            ? "FerriteCADDefinitionId"
            : "FerriteCADDefinitionKey";
        summary.occurrence_join = summary.nodes_with_occurrence_id == summary.nodes
            ? "FerriteCADOccurrenceId"
            : "ordinal_in_scene_order";
        Require(
            candidate.carries_definition_id == (summary.nodes_with_definition_id == summary.nodes),
            "the candidate " + candidate.name + " does not carry the definition identity it claims");
        Require(
            candidate.carries_occurrence_id == (summary.nodes_with_occurrence_id == summary.nodes),
            "the candidate " + candidate.name + " does not carry the occurrence identity it claims");
        Require(
            candidate.carries_display_name == (summary.nodes_with_display_name == summary.nodes),
            "the candidate " + candidate.name + " does not carry the designations it claims");

        List<string> ambiguous = new List<string>();
        foreach (string definition in view.Nodes
            .Select(Definition)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal))
        {
            List<NodeInfo> nodes = view.Nodes.Where(node => Definition(node) == definition).ToList();
            if (nodes.Select(node => node.MeshLocalId).Distinct().Count() > 1)
            {
                ambiguous.Add(definition);
            }
            ++checks;
        }
        summary.ambiguous_definitions = ambiguous.Count;
        summary.ambiguous_definition_names = ambiguous;

        string stem = Path.GetFileNameWithoutExtension(assetPath);
        summary.root_visible_name = view.Nodes[0].Target.name;
        summary.root_name_is_the_asset_file_name = summary.root_visible_name == stem;
        ++checks;

        summary.meshes_named_after_their_node = view.Nodes
            .Count(node => node.SharedMesh != null && node.SharedMesh.name == node.Target.name);
        summary.meshes_named_otherwise = view.Nodes
            .Count(node => node.SharedMesh != null && node.SharedMesh.name != node.Target.name);
        ++checks;

        summary.visible_node_names = view.Nodes
            .Skip(1)
            .Select(node => node.Target.name)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();
        summary.visible_mesh_names = view.Nodes
            .Where(node => node.SharedMesh != null)
            .Select(node => node.SharedMesh.name)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();
        summary.visible_material_names = view.Nodes
            .SelectMany(node => node.Materials)
            .Where(material => material != null)
            .Select(material => material.name)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();

        foreach (SubassetReport item in Subassets(assetPath))
        {
            if (item.unity_name == stem)
            {
                ++summary.subassets_named_after_the_asset_file;
            }
            else if (item.unity_name.StartsWith(MachinePrefix, StringComparison.Ordinal))
            {
                ++summary.subassets_named_by_machine_token;
            }
            else
            {
                ++summary.subassets_named_by_designation;
            }
        }
        ++checks;
    }

    private static void Resolve(ReferenceReport reference, UnityEngine.Object resolved, View after)
    {
        reference.resolved_by_reloaded_asset = resolved == null ? "<null>" : Identify(resolved);

        UnityEngine.Object byIdentifier = null;
        if (GlobalObjectId.TryParse(reference.raw_global_object_id, out GlobalObjectId parsed))
        {
            byIdentifier = GlobalObjectId.GlobalObjectIdentifierToObjectSlow(parsed);
        }
        reference.resolved_by_stored_identifier = byIdentifier == null
            ? "<null>"
            : Identify(byIdentifier);
        Require(
            reference.resolved_by_reloaded_asset == reference.resolved_by_stored_identifier,
            "the two independent ways of resolving one saved reference disagree");

        long present = FindBySemantic(after, reference.unity_type, reference.semantic_before);
        reference.semantic_object_present_after = present != -1L;
        reference.name_of_semantic_after = present == -1L
            ? "<absent>"
            : NameOf(after, reference.unity_type, present);
        reference.local_file_id_of_semantic_after = present;
        reference.display_name_changed = present != -1L
            && reference.name_of_semantic_after != reference.name_before;
        reference.local_file_id_changed = present != -1L
            && reference.local_file_id_of_semantic_after != reference.local_file_id_before;

        if (resolved == null)
        {
            reference.semantic_after = "<null>";
            reference.name_after = "<null>";
            reference.local_file_id_after = -1L;
            reference.node_key_after = "<absent>";
            reference.node_key_changed = reference.unity_type == "GameObject";
            reference.meaning_verdict = reference.semantic_object_present_after
                ? "missing_though_object_still_exported"
                : "missing_because_object_was_removed";
        }
        else
        {
            long landed = LocalId(resolved);
            reference.semantic_after = Semantic(after, reference.unity_type, landed);
            if (reference.unity_type == "GameObject")
            {
                reference.node_key_after = NodeKeyOf(after, landed);
                reference.node_key_changed = reference.node_key_after != reference.node_key_before;
            }
            reference.name_after = resolved.name;
            reference.local_file_id_after = landed;
            reference.meaning_verdict = Verdict(reference);
        }

        // An anchor the candidate's identity could not name is not a kept
        // reference and not a broken one. The meaning comparison is kept
        // beside it, so an ambiguous row still records what happened.
        reference.verdict = reference.join_was_ambiguous
            ? "ambiguous_join"
            : reference.meaning_verdict;
        ++checks;
    }

    private static string Verdict(ReferenceReport reference)
    {
        if (reference.semantic_after == reference.semantic_before)
        {
            return "same_semantic";
        }
        string before = DefinitionOf(reference.semantic_before);
        string after = DefinitionOf(reference.semantic_after);
        if (before == after && before.Length > 0)
        {
            return "same_definition_other_occurrence";
        }
        return "retargeted_to_another_definition";
    }

    private static string DefinitionOf(string semantic)
    {
        int start = semantic.IndexOf("definitions=[", StringComparison.Ordinal);
        if (start < 0)
        {
            start = semantic.IndexOf("definition=", StringComparison.Ordinal);
        }
        if (start < 0)
        {
            return String.Empty;
        }
        int end = semantic.IndexOf(';', start);
        return end < 0 ? semantic.Substring(start) : semantic.Substring(start, end - start);
    }

    // ---------------------------------------------------- the resolved identity

    // The strongest durable identity this import actually delivered.
    //
    // A candidate that carries a source-qualified identity is joined on it; one
    // that carries only the production property is joined on that, which is
    // exactly what makes two sources sharing `#42` indistinguishable. The probe
    // never falls back to a display name or to a position.
    private static string Definition(NodeInfo node)
    {
        return node.DefinitionId.Length > 0 ? node.DefinitionId : "key:" + node.DefinitionKey;
    }

    private static string Occurrence(View view, NodeInfo node)
    {
        if (node.OccurrenceId.Length > 0)
        {
            return node.OccurrenceId;
        }
        int ordinal = view.Nodes
            .Where(other => Definition(other) == Definition(node))
            .ToList()
            .IndexOf(node);
        return "ordinal:" + ordinal.ToString(CultureInfo.InvariantCulture);
    }

    // The plan names a source-qualified definition. A candidate whose files
    // carry no source can only match its local half, and then two definitions
    // may match one anchor — which is the measurement, not a failure of it.
    private static List<NodeInfo> Matching(View view, string anchor)
    {
        int slash = anchor.IndexOf('/');
        string local = slash < 0 ? anchor : anchor.Substring(slash + 1);
        List<NodeInfo> exact = view.Nodes
            .Where(node => node.DefinitionId.Length > 0 && node.DefinitionId == anchor)
            .ToList();
        if (exact.Count > 0)
        {
            return exact;
        }
        return view.Nodes.Where(node => node.DefinitionKey == local).ToList();
    }

    private static bool Ambiguous(View view, NodeInfo node)
    {
        return view.Nodes
            .Where(other => Definition(other) == Definition(node))
            .Select(other => other.MeshLocalId)
            .Distinct()
            .Count() > 1;
    }

    // --------------------------------------------------------- the semantics

    private static string MeshSemantic(View view, long id)
    {
        List<NodeInfo> holders = view.Nodes.Where(node => node.MeshLocalId == id).ToList();
        if (holders.Count == 0)
        {
            return "<not in this import>";
        }
        List<string> definitions = holders
            .Select(Definition)
            .Distinct()
            .OrderBy(key => key, StringComparer.Ordinal)
            .ToList();
        return "definitions=[" + String.Join(",", definitions) + "];vertices="
            + holders[0].SharedMesh.vertexCount.ToString(CultureInfo.InvariantCulture);
    }

    private static string MaterialSemantic(View view, long id)
    {
        List<string> bindings = new List<string>();
        Material found = null;
        foreach (NodeInfo node in view.Nodes)
        {
            for (int slot = 0; slot < node.MaterialLocalIds.Length; ++slot)
            {
                if (node.MaterialLocalIds[slot] == id)
                {
                    bindings.Add(Definition(node) + "@" + slot.ToString(CultureInfo.InvariantCulture));
                    found = node.Materials[slot];
                }
            }
        }
        if (found == null)
        {
            return "<not in this import>";
        }
        bindings = bindings.Distinct().OrderBy(item => item, StringComparer.Ordinal).ToList();
        return "definition=[" + String.Join(",", bindings) + "];colour=" + Colour(found);
    }

    // A placement's durable meaning is which definition it places and which
    // occurrence of it this is, plus the placement's own position — which no
    // identity scheme in this measurement supplies, so it is the independent
    // witness that a reference really landed where it says it did.
    private static string ObjectSemantic(View view, long id)
    {
        NodeInfo node = view.Nodes.FirstOrDefault(item => item.LocalId == id);
        if (node == null)
        {
            return "<not in this import>";
        }
        return "definition=" + Definition(node) + ";occurrence=" + Occurrence(view, node)
            + ";at=" + Position(node.LocalPosition);
    }

    private static string NodeKeyOf(View view, long id)
    {
        NodeInfo node = view.Nodes.FirstOrDefault(item => item.LocalId == id);
        return node == null ? "<absent>" : node.NodeKey;
    }

    private static string Semantic(View view, string kind, long id)
    {
        switch (kind)
        {
            case "Mesh": return MeshSemantic(view, id);
            case "Material": return MaterialSemantic(view, id);
            default: return ObjectSemantic(view, id);
        }
    }

    private static long FindBySemantic(View view, string kind, string semantic)
    {
        IEnumerable<long> candidates;
        switch (kind)
        {
            case "Mesh":
                candidates = view.Nodes.Select(node => node.MeshLocalId);
                break;
            case "Material":
                candidates = view.Nodes.SelectMany(node => node.MaterialLocalIds);
                break;
            default:
                candidates = view.Nodes.Select(node => node.LocalId);
                break;
        }
        foreach (long candidate in candidates.Where(id => id != -1L).Distinct())
        {
            if (Semantic(view, kind, candidate) == semantic)
            {
                return candidate;
            }
        }
        return -1L;
    }

    private static string NameOf(View view, string kind, long id)
    {
        switch (kind)
        {
            case "Mesh":
                NodeInfo mesh = view.Nodes.FirstOrDefault(node => node.MeshLocalId == id);
                return mesh == null ? "<absent>" : mesh.SharedMesh.name;
            case "Material":
                foreach (NodeInfo node in view.Nodes)
                {
                    for (int slot = 0; slot < node.MaterialLocalIds.Length; ++slot)
                    {
                        if (node.MaterialLocalIds[slot] == id)
                        {
                            return node.Materials[slot].name;
                        }
                    }
                }
                return "<absent>";
            default:
                NodeInfo target = view.Nodes.FirstOrDefault(node => node.LocalId == id);
                return target == null ? "<absent>" : target.Target.name;
        }
    }

    // ------------------------------------------------------------ the views

    private static View BuildView(string assetPath)
    {
        GameObject root = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
        Require(root != null, "Unity published no GameObject for the imported asset");

        Dictionary<string, Dictionary<string, string>> properties = ReadProperties(assetPath);

        View view = new View();
        Walk(root, "0", view, properties);
        view.Subassets = AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .ToList();

        Require(
            view.Nodes.All(node => node.NodeKey.Length > 0),
            "an imported node arrived without the FerriteCAD node key the file carries");
        Require(
            view.Nodes.All(node => node.DefinitionKey.Length > 0),
            "an imported node arrived without the FerriteCAD definition key the file carries");

        List<long> identifiers = new List<long>();
        foreach (UnityEngine.Object item in view.Subassets)
        {
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(item, out string guid, out long local);
            identifiers.Add(local);
        }
        Require(
            identifiers.Distinct().Count() == identifiers.Count,
            "one import gave two sub-objects the same local file identifier");
        return view;
    }

    private static void Walk(
        GameObject target,
        string path,
        View view,
        Dictionary<string, Dictionary<string, string>> properties)
    {
        NodeInfo node = new NodeInfo
        {
            Path = path,
            Target = target,
            LocalId = LocalId(target),
            LocalPosition = target.transform.localPosition,
        };
        if (properties.TryGetValue(path, out Dictionary<string, string> values))
        {
            node.NodeKey = Value(values, "FerriteCADNodeKey");
            node.DefinitionKey = Value(values, "FerriteCADDefinitionKey");
            node.SourceId = Value(values, "FerriteCADSourceId");
            node.DefinitionId = Value(values, "FerriteCADDefinitionId");
            node.OccurrenceId = Value(values, "FerriteCADOccurrenceId");
            node.DisplayName = Value(values, "FerriteCADDisplayName");
            node.Omission = Value(values, "FerriteCADGeometryOmission");
        }
        MeshFilter filter = target.GetComponent<MeshFilter>();
        node.SharedMesh = filter == null ? null : filter.sharedMesh;
        node.MeshLocalId = node.SharedMesh == null ? -1L : LocalId(node.SharedMesh);
        MeshRenderer renderer = target.GetComponent<MeshRenderer>();
        node.Materials = renderer == null ? Array.Empty<Material>() : renderer.sharedMaterials;
        node.MaterialLocalIds = node.Materials
            .Select(material => material == null ? -1L : LocalId(material))
            .ToArray();
        view.Nodes.Add(node);

        Transform transform = target.transform;
        for (int index = 0; index < transform.childCount; ++index)
        {
            Walk(
                transform.GetChild(index).gameObject,
                path + "/" + index.ToString(CultureInfo.InvariantCulture),
                view,
                properties);
        }
    }

    private static string Value(Dictionary<string, string> values, string name)
    {
        return values.TryGetValue(name, out string found) ? found : String.Empty;
    }

    private static ViewReport Describe(View view, PlanCandidate candidate)
    {
        ViewReport report = new ViewReport();
        foreach (UnityEngine.Object item in view.Subassets
            .OrderBy(item => item.GetType().Name, StringComparer.Ordinal)
            .ThenBy(item => item.name, StringComparer.Ordinal)
            .ThenBy(LocalId))
        {
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(item, out string guid, out long local);
            report.subassets.Add(new SubassetReport
            {
                unity_type = item.GetType().Name,
                unity_name = item.name,
                asset_guid = GuidToken(guid),
                local_file_id = local,
            });
            CountIdentifierShape(item, GuidToken(guid), local);
            ++checks;
        }
        foreach (NodeInfo node in view.Nodes)
        {
            report.nodes.Add(new NodeReport
            {
                sibling_path = node.Path,
                unity_name = node.Target.name,
                local_file_id = node.LocalId,
                node_key = node.NodeKey,
                definition_key = node.DefinitionKey,
                source_id = node.SourceId,
                definition_id = node.DefinitionId,
                occurrence_id = node.OccurrenceId,
                display_name = node.DisplayName,
                omission = node.Omission,
                resolved_definition = Definition(node),
                resolved_occurrence = Occurrence(view, node),
                mesh_local_file_id = node.MeshLocalId,
                mesh_unity_name = node.SharedMesh == null ? "<none>" : node.SharedMesh.name,
                mesh_vertex_count = node.SharedMesh == null ? -1 : node.SharedMesh.vertexCount,
                material_local_file_ids = node.MaterialLocalIds.ToList(),
                material_unity_names = node.Materials
                    .Select(material => material == null ? "<none>" : material.name)
                    .ToList(),
            });
            ++checks;
        }
        // A candidate that says its files carry designations has to have
        // delivered them to this import, or the rest of its row is about a
        // different file.
        Require(
            !candidate.carries_display_name
                || view.Nodes.All(node => node.DisplayName.Length > 0),
            "a candidate that carries designations delivered an import without them");
        return report;
    }

    // ------------------------------------------------------------- plumbing

    private static void CountIdentifierShape(UnityEngine.Object item, string token, long local)
    {
        string expected = "GlobalObjectId_V1-1-" + token + "-"
            + unchecked((ulong)local).ToString(CultureInfo.InvariantCulture) + "-0";
        if (CanonicalIdentifier(GlobalObjectId.GetGlobalObjectIdSlow(item)) == expected)
        {
            ++derivedIdentifiers;
        }
        else
        {
            ++otherIdentifiers;
        }
    }

    private static ReferenceReport Anchor(string anchor, string kind, UnityEngine.Object target)
    {
        AssetDatabase.TryGetGUIDAndLocalFileIdentifier(target, out string guid, out long local);
        Require(guid.Length > 0, "a tracked object has no asset GUID");
        GuidToken(guid);
        return new ReferenceReport
        {
            anchor = anchor,
            unity_type = kind,
            name_before = target.name,
            local_file_id_before = local,
            global_object_id_before =
                CanonicalIdentifier(GlobalObjectId.GetGlobalObjectIdSlow(target)),
            raw_global_object_id = GlobalObjectId.GetGlobalObjectIdSlow(target).ToString(),
        };
    }

    private sealed class StoredReference
    {
        public long FileId;
        public string Guid = String.Empty;
    }

    private static List<StoredReference> ReadStoredReferences(string assetPath)
    {
        List<StoredReference> result = new List<StoredReference>();
        foreach (string line in File.ReadAllLines(Path.GetFullPath(assetPath)))
        {
            // Only the list entries. The asset's own `m_Script` line carries a
            // file identifier and a GUID too, and counting it as a tracked
            // reference would silently shift every row of the table by one.
            if (!line.TrimStart().StartsWith("- {fileID:", StringComparison.Ordinal))
            {
                continue;
            }
            int fileId = line.IndexOf("fileID:", StringComparison.Ordinal);
            int guid = line.IndexOf("guid:", StringComparison.Ordinal);
            if (fileId < 0 || guid < 0)
            {
                continue;
            }
            string identifier = line.Substring(fileId + 7, guid - fileId - 8).Trim(' ', ',');
            string value = line.Substring(guid + 5).Trim();
            int comma = value.IndexOf(',');
            if (comma >= 0)
            {
                value = value.Substring(0, comma);
            }
            result.Add(new StoredReference
            {
                FileId = long.Parse(identifier.Trim(), CultureInfo.InvariantCulture),
                Guid = value.Trim(),
            });
        }
        return result;
    }

    private static Dictionary<string, Dictionary<string, string>> ReadProperties(string assetPath)
    {
        string path = FerriteChannelProperties.CachePath(assetPath);
        Require(File.Exists(path), "the custom-property callback did not run for this import");
        Dictionary<string, Dictionary<string, string>> result =
            new Dictionary<string, Dictionary<string, string>>();
        foreach (string line in File.ReadAllLines(path))
        {
            if (String.IsNullOrEmpty(line))
            {
                continue;
            }
            string[] fields = line.Split('\t');
            Require(fields.Length == 3, "malformed custom-property line");
            if (!result.TryGetValue(fields[0], out Dictionary<string, string> values))
            {
                values = new Dictionary<string, string>();
                result[fields[0]] = values;
            }
            values[fields[1]] = fields[2];
        }
        return result;
    }

    private static List<string> Import(string assetPath)
    {
        List<string> messages = new List<string>();
        Application.LogCallback capture = (message, stack, kind) =>
        {
            if (kind == LogType.Warning || kind == LogType.Error || kind == LogType.Exception)
            {
                messages.Add(kind.ToString().ToLowerInvariant() + ": " + Canonical(message));
            }
        };
        Application.logMessageReceived += capture;
        try
        {
            AssetDatabase.ImportAsset(
                assetPath,
                ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
            AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        }
        finally
        {
            Application.logMessageReceived -= capture;
        }
        return messages
            .GroupBy(message => message, StringComparer.Ordinal)
            .OrderBy(group => group.Key, StringComparer.Ordinal)
            .Select(group => group.Count().ToString(CultureInfo.InvariantCulture) + " x " + group.Key)
            .ToList();
    }

    private static string Transition(List<string> before, List<string> after)
    {
        bool had = before.Count > 0;
        bool has = after.Count > 0;
        if (!had && !has)
        {
            return "never_warned";
        }
        if (had && !has)
        {
            return "warning_disappeared";
        }
        if (!had)
        {
            return "warning_appeared";
        }
        return before.SequenceEqual(after) ? "warning_unchanged" : "warning_changed";
    }

    private static void SettleImporter(string assetPath)
    {
        AssetDatabase.ImportAsset(
            assetPath,
            ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        ModelImporter importer = AssetImporter.GetAtPath(assetPath) as ModelImporter;
        Require(importer != null, "Unity gave the imported asset no ModelImporter");
        if (importer.sortHierarchyByName)
        {
            importer.sortHierarchyByName = false;
            importer.SaveAndReimport();
            AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        }
    }

    private static long LocalId(UnityEngine.Object target)
    {
        AssetDatabase.TryGetGUIDAndLocalFileIdentifier(target, out string guid, out long local);
        return local;
    }

    private static string Identify(UnityEngine.Object target)
    {
        return target.GetType().Name + ":" + target.name + ":"
            + LocalId(target).ToString(CultureInfo.InvariantCulture);
    }

    private static string Colour(Material material)
    {
        Color colour;
        if (material.HasProperty("_BaseColor"))
        {
            colour = material.GetColor("_BaseColor");
        }
        else if (material.HasProperty("_Color"))
        {
            colour = material.GetColor("_Color");
        }
        else
        {
            throw new InvalidOperationException(
                "an imported material has no base colour property: " + material.name);
        }
        Color linear = colour.linear;
        return "[" + Round(linear.r) + "," + Round(linear.g) + "," + Round(linear.b) + "]";
    }

    private static string Position(Vector3 value)
    {
        return "[" + Round(value.x) + "," + Round(value.y) + "," + Round(value.z) + "]";
    }

    private static string Round(float value)
    {
        float rounded = (float)Math.Round(value, 4, MidpointRounding.AwayFromZero);
        if (rounded == 0.0f)
        {
            rounded = 0.0f;
        }
        return rounded.ToString("0.0000", CultureInfo.InvariantCulture);
    }

    private static string Fingerprint(string path)
    {
        ulong hash = 14695981039346656037UL;
        using (FileStream stream = File.OpenRead(path))
        {
            byte[] buffer = new byte[65536];
            int read;
            while ((read = stream.Read(buffer, 0, buffer.Length)) > 0)
            {
                for (int index = 0; index < read; ++index)
                {
                    hash ^= buffer[index];
                    hash *= 1099511628211UL;
                }
            }
        }
        return hash.ToString("x16", CultureInfo.InvariantCulture);
    }

    private static bool SameBytes(string left, string right)
    {
        byte[] first = File.ReadAllBytes(left);
        byte[] second = File.ReadAllBytes(right);
        return first.Length == second.Length && first.SequenceEqual(second);
    }

    private static string GuidToken(string guid)
    {
        if (String.IsNullOrEmpty(guid))
        {
            return "<guid-none>";
        }
        if (!GuidTokens.TryGetValue(guid, out string token))
        {
            token = "<guid-" + GuidTokens.Count.ToString(CultureInfo.InvariantCulture) + ">";
            GuidTokens[guid] = token;
        }
        return token;
    }

    private static string CanonicalIdentifier(GlobalObjectId identifier)
    {
        string text = identifier.ToString();
        foreach (KeyValuePair<string, string> entry in GuidTokens)
        {
            text = text.Replace(entry.Key, entry.Value);
        }
        return text;
    }

    private static readonly Regex RawGuid = new Regex("[0-9a-f]{32}", RegexOptions.Compiled);

    private static string Canonical(string message)
    {
        string project = Directory.GetCurrentDirectory().Replace('\\', '/');
        string text = message.Replace('\\', '/').Replace(project, "<project>")
            .Replace("\r", String.Empty).Trim();
        return RawGuid.Replace(text, match => GuidToken(match.Value));
    }

    private static string ArgumentValue(string name)
    {
        string[] arguments = Environment.GetCommandLineArgs();
        for (int index = 0; index + 1 < arguments.Length; ++index)
        {
            if (arguments[index] == name)
            {
                return arguments[index + 1];
            }
        }
        return null;
    }

    private static bool HasArgument(string name)
    {
        return Environment.GetCommandLineArgs().Contains(name);
    }

    private static void Require(bool condition, string message)
    {
        ++checks;
        if (!condition)
        {
            throw new InvalidOperationException(message);
        }
    }
}
