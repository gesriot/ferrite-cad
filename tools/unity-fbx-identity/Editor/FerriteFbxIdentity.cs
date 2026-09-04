// SPDX-License-Identifier: MIT
//
// The §22B-1e1 measurement: what a real Unity reference into a FerriteCAD FBX
// does when the document is exported again.
//
// This is not a smoke test of import. §22B-1a to §22B-1d already measured that
// the content arrives. The one question here is whether a reference a project
// already holds — the way a prefab, a scene or a material holds one — still
// means the same FerriteCAD definition after a reimport, and the only answers
// that count are measured on a reference that was really written to disk and
// really resolved again.
//
// Three rules this probe follows, because breaking any of them would make the
// result look better than it is:
//
//   1. Non-null is not survival. In these one-source fixtures, a resolved
//      object must still carry the same source-local `FerriteCADDefinitionKey`,
//      cross-checked against a vertex count `ufbx` read from the same file.
//   2. Geometry, Model and Material are measured separately. Unity gives them
//      different names and may give them different identity rules.
//   3. Nothing is concluded from a display name or from a position in the
//      hierarchy. Both change on purpose in these variants.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;
using UnityEditor;
using UnityEngine;

internal static class FerriteFbxIdentity
{
    private const string AssetFolder = "Assets/Identity";

    // ------------------------------------------------------------ the plan

    [Serializable]
    private sealed class Plan
    {
        public List<PlanScenario> scenarios = new List<PlanScenario>();
    }

    [Serializable]
    private sealed class PlanScenario
    {
        public string name = String.Empty;
        public string change = String.Empty;
        public string before = String.Empty;
        public string after = String.Empty;
        // Stable source-local keys the measurement tracks in this one-source
        // fixture, so the report asks the same questions of every variant.
        public List<string> mesh_definitions = new List<string>();
        public List<string> material_bindings = new List<string>();
        public List<string> object_bindings = new List<string>();
    }

    // ---------------------------------------------------------- the report

    [Serializable]
    private sealed class Report
    {
        public string schema = "ferritecad.unity-fbx-identity.v1";
        public string unity_version = String.Empty;
        public string colour_space = String.Empty;
        public int distinct_asset_guids;
        // Measured, not assumed: for every sub-asset of every import here, the
        // `GlobalObjectId` is exactly
        // `GlobalObjectId_V1-1-<asset guid>-<local file identifier as
        // unsigned>-0`. It is checked on every row and then not repeated on
        // every row, because a third copy of two numbers is not a third
        // measurement. The tracked references below keep theirs in full.
        public int subassets_whose_identifier_is_the_guid_and_local_id;
        public int subassets_whose_identifier_is_something_else;
        // Unity's model importer sorts a hierarchy by name, and it does that
        // after the custom-property callbacks have run, so the probe's join
        // between a reported key and a finished object would land on the wrong
        // object. The probe turns that display convenience off and measures
        // here whether doing so moved a single identifier. If it did, every
        // result below would be an artefact of the probe rather than of Unity.
        public SortControlReport sort_control = new SortControlReport();
        public List<ScenarioReport> scenarios = new List<ScenarioReport>();
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

    [Serializable]
    private sealed class ScenarioReport
    {
        public string name = String.Empty;
        public string change = String.Empty;
        public bool files_are_byte_identical;
        // What the editor actually opened. The independent reader hashes the
        // same way, so "ufbx read a different file" is a refusal rather than
        // an assumption.
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
        public string definition_key = String.Empty;
        public string node_key = String.Empty;
        public long mesh_local_file_id;
        public int mesh_vertex_count;
        public List<long> material_local_file_ids = new List<long>();
    }

    [Serializable]
    private sealed class ReferenceReport
    {
        public string anchor = String.Empty;
        public string unity_type = String.Empty;
        public string semantic_before = String.Empty;
        public string name_before = String.Empty;
        public long local_file_id_before;
        public string global_object_id_before = String.Empty;
        // The same identifier with the project's real GUID still in it, kept
        // out of the report because a GUID is new in every project and is not
        // what this slice measures. Resolving needs the real one.
        [NonSerialized]
        public string raw_global_object_id = String.Empty;
        // What the saved asset actually wrote to disk for this reference.
        public long stored_file_id;
        public string stored_guid = String.Empty;
        // Where each independent way of resolving it lands after the reimport.
        public string resolved_by_reloaded_asset = String.Empty;
        public string resolved_by_stored_identifier = String.Empty;
        public string semantic_after = String.Empty;
        public string name_after = String.Empty;
        public long local_file_id_after;
        // Where the FerriteCAD object the reference meant lives now, found by
        // durable key rather than by following the reference.
        public bool semantic_object_present_after;
        public string name_of_semantic_after = String.Empty;
        public long local_file_id_of_semantic_after;
        public string node_key_before = String.Empty;
        public string node_key_after = String.Empty;
        public bool display_name_changed;
        public bool local_file_id_changed;
        // The writer derives `FerriteCADNodeKey` from a position in the scene,
        // so it is reported beside the durable meaning and never inside it.
        public bool node_key_changed;
        public string verdict = String.Empty;
    }

    // ------------------------------------------------------- the in-memory view

    private sealed class NodeInfo
    {
        public string Path = String.Empty;
        public GameObject Target;
        public string DefinitionKey = String.Empty;
        public string NodeKey = String.Empty;
        public Mesh SharedMesh;
        public Material[] Materials = Array.Empty<Material>();
        public long LocalId;
        public long MeshLocalId = -1L;
        public long[] MaterialLocalIds = Array.Empty<long>();
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

    // Whether this object's `GlobalObjectId` is exactly its asset GUID and its
    // local file identifier, written as an unsigned 64-bit number.
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

    public static void Run()
    {
        try
        {
            Execute();
        }
        catch (Exception error)
        {
            Debug.LogError("FCAD_FBX_IDENTITY_FAILURE " + error);
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
        // The failing-first run asserts the optimistic contract: every tracked
        // reference still means the same FerriteCAD definition. It is meant to
        // fail on today's behaviour, and it is kept so the claim can be
        // reproduced rather than remembered.
        bool expectStable = HasArgument("-fcadExpectStable");

        Plan plan = JsonUtility.FromJson<Plan>(File.ReadAllText(planPath));
        Require(plan != null && plan.scenarios.Count > 0, "the plan named no scenario");

        if (!AssetDatabase.IsValidFolder(AssetFolder))
        {
            AssetDatabase.CreateFolder("Assets", "Identity");
        }

        Report report = new Report
        {
            unity_version = Application.unityVersion,
            colour_space = QualitySettings.activeColorSpace.ToString().ToLowerInvariant(),
        };

        report.sort_control = MeasureSortControl(plan.scenarios[0].before);
        foreach (PlanScenario scenario in plan.scenarios)
        {
            report.scenarios.Add(MeasureScenario(scenario, expectStable));
        }
        report.distinct_asset_guids = GuidTokens.Count;
        report.subassets_whose_identifier_is_the_guid_and_local_id = derivedIdentifiers;
        report.subassets_whose_identifier_is_something_else = otherIdentifiers;
        Require(derivedIdentifiers > 0, "no sub-asset identifier was examined at all");

        Require(checks > 200, "the probe performed too few checks");
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
            Require(File.Exists(expected), "the committed expected identity report is missing");
            string committed = File.ReadAllText(expected).Replace("\r\n", "\n");
            Require(committed == json, "the identity report differs from the committed measurement");
        }

        Debug.Log("FCAD_FBX_IDENTITY_EXECUTED checks="
            + report.checks.ToString(CultureInfo.InvariantCulture));
        EditorApplication.Exit(0);
    }

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

        // Names and local file identifiers, compared as sets. The order of the
        // rows is the hierarchy order and is expected to differ; what must not
        // differ is which object holds which identifier.
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

    private static ScenarioReport MeasureScenario(PlanScenario scenario, bool expectStable)
    {
        string assetPath = AssetFolder + "/" + scenario.name + ".fbx";
        string referencePath = AssetFolder + "/" + scenario.name + "-references.asset";
        string absolute = Path.GetFullPath(assetPath);

        Require(File.Exists(scenario.before), "the scenario's first file is missing");
        Require(File.Exists(scenario.after), "the scenario's second file is missing");

        ScenarioReport result = new ScenarioReport
        {
            name = scenario.name,
            change = scenario.change,
            files_are_byte_identical = SameBytes(scenario.before, scenario.after),
            before_bytes = new FileInfo(scenario.before).Length,
            before_fnv1a64 = Fingerprint(scenario.before),
            after_bytes = new FileInfo(scenario.after).Length,
            after_fnv1a64 = Fingerprint(scenario.after),
        };

        // ---- the document as it was, and the references a project keeps.
        File.Copy(scenario.before, absolute, true);
        SettleImporter(assetPath);
        result.warnings_before = Import(assetPath);
        View before = BuildView(assetPath);
        result.before = Describe(before, assetPath);

        List<ReferenceReport> references = new List<ReferenceReport>();
        List<Mesh> meshes = new List<Mesh>();
        List<Material> materials = new List<Material>();
        List<GameObject> objects = new List<GameObject>();
        List<string> semantics = new List<string>();
        List<string> kinds = new List<string>();

        foreach (string definition in scenario.mesh_definitions)
        {
            NodeInfo node = FirstNode(before, definition);
            Require(node != null, "no imported object carries the tracked definition key " + definition);
            Require(
                node.SharedMesh != null,
                "the tracked definition " + definition + " arrived without a mesh");
            meshes.Add(node.SharedMesh);
            semantics.Add(MeshSemantic(before, node.MeshLocalId));
            kinds.Add("Mesh");
            references.Add(Anchor("mesh:definition=" + definition, "Mesh", node.SharedMesh));
        }
        foreach (string binding in scenario.material_bindings)
        {
            string[] pieces = binding.Split('@');
            Require(pieces.Length == 2, "a material binding is not 'key@slot'");
            int slot = int.Parse(pieces[1], CultureInfo.InvariantCulture);
            NodeInfo node = FirstNode(before, pieces[0]);
            Require(node != null, "no imported object carries the tracked definition key " + pieces[0]);
            Require(
                slot < node.Materials.Length,
                "the tracked material slot " + binding + " is not there; the imported node named "
                    + node.Target.name + " has " + node.Materials.Length.ToString(CultureInfo.InvariantCulture)
                    + " slots and " + Describe(before));
            materials.Add(node.Materials[slot]);
            semantics.Add(MaterialSemantic(before, node.MaterialLocalIds[slot]));
            kinds.Add("Material");
            references.Add(Anchor("material:" + binding, "Material", node.Materials[slot]));
        }
        foreach (string binding in scenario.object_bindings)
        {
            string[] pieces = binding.Split('@');
            Require(pieces.Length == 2, "an object binding is not 'key@occurrence'");
            int occurrence = int.Parse(pieces[1], CultureInfo.InvariantCulture);
            NodeInfo node = NodeAt(before, pieces[0], occurrence);
            Require(node != null, "no imported object carries the tracked occurrence " + binding);
            objects.Add(node.Target);
            semantics.Add(ObjectSemantic(before, node.LocalId));
            kinds.Add("GameObject");
            ReferenceReport tracked = Anchor("object:" + binding, "GameObject", node.Target);
            tracked.node_key_before = node.NodeKey;
            references.Add(tracked);
        }
        for (int index = 0; index < references.Count; ++index)
        {
            references[index].semantic_before = semantics[index];
        }

        // ---- a real asset, written to disk, holding those references.
        AssetDatabase.DeleteAsset(referencePath);
        FerriteIdentityReferences holder = ScriptableObject.CreateInstance<FerriteIdentityReferences>();
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
        FerriteIdentityReferences reloaded =
            AssetDatabase.LoadAssetAtPath<FerriteIdentityReferences>(referencePath);
        Require(reloaded != null, "the asset holding the references did not come back");

        View after = BuildView(assetPath);
        result.after = Describe(after, assetPath);

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
            Resolve(reference, resolved, after, scenario);
        }
        result.references = references;

        if (expectStable)
        {
            foreach (ReferenceReport reference in references)
            {
                Require(
                    reference.verdict == "same_semantic",
                    "a saved reference stopped meaning the same FerriteCAD object: "
                        + scenario.name + " " + reference.anchor + " -> " + reference.verdict);
            }
        }

        AssetDatabase.DeleteAsset(referencePath);
        AssetDatabase.DeleteAsset(assetPath);
        return result;
    }

    private static void Resolve(
        ReferenceReport reference,
        UnityEngine.Object resolved,
        View after,
        PlanScenario scenario)
    {
        reference.resolved_by_reloaded_asset = resolved == null
            ? "<null>"
            : Identify(resolved);

        // The second, independent resolution: the exact pair a project file
        // stores, put back through Unity without going near the loaded asset.
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

        // Where the FerriteCAD object the reference meant lives now, found by
        // durable key and never by following the reference.
        long present = FindBySemantic(after, reference.unity_type, reference.semantic_before);
        reference.semantic_object_present_after = present != -1L;
        reference.name_of_semantic_after = present == -1L
            ? "<absent>"
            : NameOf(after, reference.unity_type, present);
        reference.local_file_id_of_semantic_after = present;

        // These two ask about the object the reference meant, not about the
        // object it happens to have landed on. "Did what I pointed at get a new
        // name and a new identifier" is the question that explains the verdict;
        // comparing the landed object with itself would always say no.
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
            reference.verdict = reference.semantic_object_present_after
                ? "missing_though_object_still_exported"
                : "missing_because_object_was_removed";
            ++checks;
            return;
        }

        long landed = LocalId(resolved);
        reference.semantic_after = Semantic(after, reference.unity_type, landed);
        if (reference.unity_type == "GameObject")
        {
            reference.node_key_after = NodeKeyOf(after, landed);
            reference.node_key_changed = reference.node_key_after != reference.node_key_before;
        }
        reference.name_after = resolved.name;
        reference.local_file_id_after = landed;
        reference.verdict = Verdict(reference);
        ++checks;
    }

    private static string Verdict(ReferenceReport reference)
    {
        if (reference.semantic_after == reference.semantic_before)
        {
            return "same_semantic";
        }
        // A reference that still names the same FerriteCAD definition but a
        // different occurrence of it is not a broken reference and not a kept
        // one; calling it either would hide the case this slice exists for.
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
        int comma = semantic.IndexOf(';', start);
        return comma < 0 ? semantic.Substring(start) : semantic.Substring(start, comma - start);
    }

    // --------------------------------------------------------- the semantics

    // Meaning is looked up by the pair a project file stores — the asset GUID
    // and the local file identifier — and never by a managed object reference.
    // Reimporting hands back a different C# instance for the same persisted
    // object, so reference equality would call every kept reference broken.
    private static string MeshSemantic(View view, long id)
    {
        List<NodeInfo> holders = view.Nodes.Where(node => node.MeshLocalId == id).ToList();
        if (holders.Count == 0)
        {
            return "<not in this import>";
        }
        List<string> definitions = holders
            .Select(node => node.DefinitionKey)
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
                    bindings.Add(node.DefinitionKey + "@" + slot.ToString(CultureInfo.InvariantCulture));
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
    // occurrence of it this is. `FerriteCADNodeKey` is deliberately not part of
    // it: the writer derives that key from a position in the scene, so putting
    // it here would make every unrelated insertion look like a broken
    // reference. It is reported beside the verdict instead.
    private static string ObjectSemantic(View view, long id)
    {
        NodeInfo node = view.Nodes.FirstOrDefault(item => item.LocalId == id);
        if (node == null)
        {
            return "<not in this import>";
        }
        int occurrence = view.Nodes
            .Where(other => other.DefinitionKey == node.DefinitionKey)
            .ToList()
            .IndexOf(node);
        return "definition=" + node.DefinitionKey + ";occurrence="
            + occurrence.ToString(CultureInfo.InvariantCulture);
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

    // Where the FerriteCAD object a reference meant lives now, found by asking
    // every candidate what it means, never by following the reference.
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

    private static string Identify(UnityEngine.Object target)
    {
        return target.GetType().Name + ":" + target.name + ":"
            + LocalId(target).ToString(CultureInfo.InvariantCulture);
    }

    // The same 64-bit FNV-1a the independent reader computes, over the same
    // whole file. Not a security digest and not used as one.
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

    // ------------------------------------------------------------ the views

    private static View BuildView(string assetPath)
    {
        GameObject root = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
        Require(root != null, "Unity published no GameObject for the imported asset");

        Dictionary<string, Dictionary<string, string>> properties =
            ReadProperties(assetPath);

        View view = new View();
        Walk(root, "0", view, properties);
        view.Subassets = AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .ToList();

        // The join between Unity's custom-property callback and the finished
        // hierarchy is a chain of sibling indices, so it is checked rather than
        // trusted: every node the file gave a key must have got one here.
        Require(
            view.Nodes.All(node => node.DefinitionKey.Length > 0),
            "an imported node arrived without the FerriteCAD definition key the file carries");
        Require(
            view.Nodes.All(node => node.NodeKey.Length > 0),
            "an imported node arrived without the FerriteCAD node key the file carries");

        // Every measurement below treats a local file identifier as an
        // identity, so an import that gave two sub-objects the same one would
        // make the whole table meaningless rather than merely surprising.
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
        };
        if (properties.TryGetValue(path, out Dictionary<string, string> values))
        {
            values.TryGetValue("FerriteCADDefinitionKey", out string definition);
            values.TryGetValue("FerriteCADNodeKey", out string key);
            node.DefinitionKey = definition ?? String.Empty;
            node.NodeKey = key ?? String.Empty;
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

    private static ViewReport Describe(View view, string assetPath)
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
                definition_key = node.DefinitionKey,
                node_key = node.NodeKey,
                mesh_local_file_id = node.MeshLocalId,
                mesh_vertex_count = node.SharedMesh == null ? -1 : node.SharedMesh.vertexCount,
                material_local_file_ids = node.MaterialLocalIds.ToList(),
            });
            ++checks;
        }
        return report;
    }

    // ------------------------------------------------------------- plumbing

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

    // What the reference asset really wrote. Read as text on purpose: a
    // reference Unity resolved for us in memory would not prove that a project
    // file on disk carries a local file identifier and an asset GUID.
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
        string path = FerriteFbxIdentityProperties.CachePath(assetPath);
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

    // Imports once, with the importer settings already settled, and returns the
    // warnings that import produced.
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

    // Only used to make a refusal readable, never to decide anything.
    private static string Describe(View view)
    {
        return String.Join(
            " | ",
            view.Nodes.Select(node =>
                node.Path + " " + node.Target.name + " key=" + node.DefinitionKey
                + " mesh=" + (node.SharedMesh == null ? "none" : node.SharedMesh.name)
                + " slots=" + node.Materials.Length.ToString(CultureInfo.InvariantCulture)));
    }

    // One import whose only job is to create the importer, plus the setting
    // change, so every measured import below is a single forced reimport with
    // the same settings on both sides of a document change.
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

    private static NodeInfo FirstNode(View view, string definition)
    {
        return view.Nodes.FirstOrDefault(node => node.DefinitionKey == definition);
    }

    private static NodeInfo NodeAt(View view, string definition, int occurrence)
    {
        List<NodeInfo> nodes = view.Nodes
            .Where(node => node.DefinitionKey == definition)
            .ToList();
        return occurrence < nodes.Count ? nodes[occurrence] : null;
    }

    private static long LocalId(UnityEngine.Object target)
    {
        AssetDatabase.TryGetGUIDAndLocalFileIdentifier(target, out string guid, out long local);
        return local;
    }

    // Part of a material's measured meaning, so a material this probe cannot
    // read a colour from refuses the run. Substituting a placeholder here would
    // make two different materials compare equal, which is the exact failure
    // this slice exists to catch.
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

    private static string Round(float value)
    {
        float rounded = (float)Math.Round(value, 4, MidpointRounding.AwayFromZero);
        if (rounded == 0.0f)
        {
            rounded = 0.0f;
        }
        return rounded.ToString("0.0000", CultureInfo.InvariantCulture);
    }

    // A project's GUIDs are new in every project, and this measurement is not
    // about them. They become tokens in first-seen order so two clean projects
    // can be compared byte-for-byte; the local file identifiers below them are
    // left exactly as Unity produced, because those are the measurement.
    private static string GuidToken(string guid)
    {
        if (String.IsNullOrEmpty(guid))
        {
            return "<guid-none>";
        }
        if (!GuidTokens.TryGetValue(guid, out string token))
        {
            // Bracketed rather than prefixed with `guid:`, because Unity's own
            // messages already say `guid:` in front of one and the report
            // should not read `guid:guid:6`.
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

    private static bool SameBytes(string left, string right)
    {
        byte[] first = File.ReadAllBytes(left);
        byte[] second = File.ReadAllBytes(right);
        return first.Length == second.Length && first.SequenceEqual(second);
    }

    private static readonly Regex RawGuid = new Regex("[0-9a-f]{32}", RegexOptions.Compiled);

    private static string Canonical(string message)
    {
        string project = Directory.GetCurrentDirectory().Replace('\\', '/');
        string text = message.Replace('\\', '/').Replace(project, "<project>")
            .Replace("\r", String.Empty).Trim();
        // Unity names the asset by its GUID in some of its own messages, and a
        // GUID is new in every project. Tokenised the same way as everywhere
        // else, so two clean projects can be compared byte-for-byte.
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
