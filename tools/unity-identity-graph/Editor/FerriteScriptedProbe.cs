// SPDX-License-Identifier: MIT
//
// §22B-1e2b, part E: what a `ScriptedImporter` can do with
// `AssetImportContext.AddObjectToAsset`, measured for `GameObject`, `Mesh` and
// `Material` separately and over the same transitions as every other candidate
// in this slice.
//
// The importer beside this file is a probe for a *test* extension. It proves a
// Unity identity mechanism and nothing else: it is not a FerriteCAD importer,
// it does not read FBX, and this report says so on its own lines rather than
// leaving a reader to infer it.
//
// The rules are §22B-1e2a's, unchanged, because the results have to be
// readable together:
//
//   1. Non-null is not survival. A resolved object must still mean the same
//      FerriteCAD definition, cross-checked against a witness the identity
//      scheme did not supply.
//   2. An identity that cannot tell two definitions apart is ambiguous, never
//      a kept reference.
//   3. The three types are measured separately, always.
//   4. The visible names are part of the measurement.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;

internal static class FerriteScriptedProbe
{
    private const string AssetFolder = "Assets/Scripted";

    [Serializable]
    internal sealed class Plan
    {
        public List<PlanScenario> scenarios = new List<PlanScenario>();
    }

    [Serializable]
    internal sealed class PlanScenario
    {
        public string name = String.Empty;
        public string change = String.Empty;
        public string before = String.Empty;
        public string after = String.Empty;
        public List<string> mesh_definitions = new List<string>();
        public List<string> material_bindings = new List<string>();
        public List<string> object_bindings = new List<string>();
    }

    [Serializable]
    internal sealed class Report
    {
        public string schema = "ferritecad.unity-scripted-identity.v1";
        public string mode = "scripted";
        public string unity_version = String.Empty;

        // What this is, said before any number below it.
        public bool is_a_property_of_the_fbx;
        public bool the_importer_reads_fbx;
        public string extension_it_owns = "fcadsyn";
        public string what_it_would_take_in_the_product = String.Empty;

        public int distinct_asset_guids;
        public int subassets_whose_identifier_is_the_guid_and_local_id;
        public int subassets_whose_identifier_is_something_else;

        // ---- what the base import delivered
        public int game_objects;
        public int meshes;
        public int materials;
        public int definitions_with_several_placements;
        public int definitions_whose_placements_share_one_mesh;
        public int ambiguous_definitions;
        public List<string> visible_node_names = new List<string>();
        public List<string> visible_mesh_names = new List<string>();
        public List<string> visible_material_names = new List<string>();
        public int visible_names_carrying_a_machine_token;
        public List<IdentifierRow> identifiers = new List<IdentifierRow>();

        // ---- the deliberate collision
        //
        // Counted in `Material`s, not in sub-assets: a `ScriptedImporter`'s
        // asset also publishes every `Transform`, `MeshFilter`, `MeshRenderer`
        // and `MonoBehaviour` under it, and a total over those would move for
        // reasons that have nothing to do with the collision.
        public bool collision_document_imported;
        public int collision_materials_published;
        public int collision_materials_expected;
        public bool collision_merged_two_objects_into_one;
        public bool collision_was_refused;
        public List<string> collision_messages = new List<string>();

        // What a designation change does to a local file identifier, per
        // type, joined on the identifier the importer passed. This is the
        // sharpest form of part E's question: an identifier Unity really uses
        // cannot move when a name a person types moves.
        public List<RenameRow> rename = new List<RenameRow>();

        public List<ScenarioReport> scenarios = new List<ScenarioReport>();
        public int checks;
    }

    // One object, its durable identity, and the local file identifier Unity
    // derived from it. The whole of part E is whether the second column is a
    // function of the first and of nothing else.
    [Serializable]
    internal sealed class IdentifierRow
    {
        public string unity_type = String.Empty;
        public string identifier = String.Empty;
        public string visible_name = String.Empty;
        public long local_file_id;
        public bool visible_name_is_a_machine_token;
    }

    [Serializable]
    internal sealed class RenameRow
    {
        public string unity_type = String.Empty;
        public int identifiers_compared;
        public int local_file_ids_that_moved;
        public bool the_identifier_alone_decides_the_local_file_id;
    }

    [Serializable]
    internal sealed class ScenarioReport
    {
        public string name = String.Empty;
        public string change = String.Empty;
        public bool files_are_byte_identical;
        public string before_fnv1a64 = String.Empty;
        public string after_fnv1a64 = String.Empty;
        public List<string> warnings_before = new List<string>();
        public List<string> warnings_after = new List<string>();
        public string warning_transition = String.Empty;
        public List<IdentifierRow> before_rows = new List<IdentifierRow>();
        public List<IdentifierRow> after_rows = new List<IdentifierRow>();
        public List<ReferenceReport> references = new List<ReferenceReport>();
    }

    [Serializable]
    internal sealed class ReferenceReport
    {
        public string anchor = String.Empty;
        public string unity_type = String.Empty;
        public string identifier = String.Empty;
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
        public bool display_name_changed;
        public bool local_file_id_changed;
        public string verdict = String.Empty;
    }

    // ------------------------------------------------------- the in-memory view

    private sealed class NodeInfo
    {
        public string Path = String.Empty;
        public GameObject Target;
        public string Identifier = String.Empty;
        public string DefinitionId = String.Empty;
        public string OccurrenceId = String.Empty;
        public string Kind = String.Empty;
        public Mesh SharedMesh;
        public Material[] Materials = Array.Empty<Material>();
        public long LocalId;
        public long MeshLocalId = -1L;
        public long[] MaterialLocalIds = Array.Empty<long>();
        public Vector3 WorldPosition;
    }

    private sealed class View
    {
        public List<NodeInfo> Nodes = new List<NodeInfo>();
        public List<UnityEngine.Object> Subassets = new List<UnityEngine.Object>();
    }

    // ------------------------------------------------------------- the run

    internal static Report Execute(string planPath)
    {
        Plan plan = JsonUtility.FromJson<Plan>(File.ReadAllText(planPath));
        FerriteGraphCommon.Require(
            plan != null && plan.scenarios.Count > 0, "the scripted plan named no scenario");

        if (!AssetDatabase.IsValidFolder(AssetFolder))
        {
            AssetDatabase.CreateFolder("Assets", "Scripted");
        }

        Report report = new Report
        {
            unity_version = Application.unityVersion,
            is_a_property_of_the_fbx = false,
            the_importer_reads_fbx = false,
            what_it_would_take_in_the_product =
                "a Unity package the user installs, owning an extension the ModelImporter "
                + "does not, and reading FerriteCAD's own bytes",
        };

        bool summarised = false;
        foreach (PlanScenario scenario in plan.scenarios)
        {
            if (scenario.name == "collision")
            {
                MeasureCollision(report, scenario);
                continue;
            }
            ScenarioReport measured = MeasureScenario(scenario);
            report.scenarios.Add(measured);
            if (!summarised)
            {
                Summarise(report, scenario);
                summarised = true;
            }
        }
        FerriteGraphCommon.Require(summarised, "no scenario summarised the base import");
        MeasureRename(report);

        report.distinct_asset_guids = FerriteGraphCommon.DistinctGuids;
        report.subassets_whose_identifier_is_the_guid_and_local_id =
            FerriteGraphCommon.DerivedIdentifiers;
        report.subassets_whose_identifier_is_something_else =
            FerriteGraphCommon.OtherIdentifiers;
        FerriteGraphCommon.Require(
            FerriteGraphCommon.DerivedIdentifiers > 0,
            "no sub-asset identifier was examined at all");

        AssetDatabase.DeleteAsset(AssetFolder);
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        return report;
    }

    private static void Summarise(Report report, PlanScenario scenario)
    {
        string assetPath = AssetFolder + "/summary.fcadsyn";
        File.Copy(scenario.before, Path.GetFullPath(assetPath), true);
        FerriteGraphCommon.Import(assetPath);
        View view = BuildView(assetPath);

        report.game_objects = view.Nodes.Count;
        report.meshes = view.Subassets.Count(item => item is Mesh);
        report.materials = view.Subassets.Count(item => item is Material);
        report.visible_node_names = view.Nodes
            .Select(node => node.Target.name)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();
        report.visible_mesh_names = view.Subassets.OfType<Mesh>()
            .Select(mesh => mesh.name)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();
        report.visible_material_names = view.Subassets.OfType<Material>()
            .Select(material => material.name)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();
        report.visible_names_carrying_a_machine_token =
            report.visible_node_names.Count(IsMachine)
            + report.visible_mesh_names.Count(IsMachine)
            + report.visible_material_names.Count(IsMachine);

        int several = 0;
        int shared = 0;
        int ambiguous = 0;
        foreach (string definition in view.Nodes
            .Select(node => node.DefinitionId)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal))
        {
            List<NodeInfo> placements = view.Nodes
                .Where(node => node.DefinitionId == definition)
                .ToList();
            List<NodeInfo> bearers = placements.Where(node => node.SharedMesh != null).ToList();
            if (bearers.Select(node => FerriteGraphCommon.InstanceKey(node.SharedMesh)).Distinct().Count() > 1)
            {
                ++ambiguous;
            }
            if (placements.Count < 2 || bearers.Count == 0)
            {
                continue;
            }
            ++several;
            if (bearers.Select(node => FerriteGraphCommon.InstanceKey(node.SharedMesh)).Distinct().Count() == 1)
            {
                ++shared;
            }
            ++FerriteGraphCommon.Checks;
        }
        report.definitions_with_several_placements = several;
        report.definitions_whose_placements_share_one_mesh = shared;
        report.ambiguous_definitions = ambiguous;
        report.identifiers = Rows(view);

        AssetDatabase.DeleteAsset(assetPath);
        ++FerriteGraphCommon.Checks;
    }

    // Joined on the identifier, not on the name and not on the position: the
    // whole claim under test is that the identifier decides the local file
    // identifier, so the comparison has to be keyed on the identifier itself.
    private static void MeasureRename(Report report)
    {
        ScenarioReport rename = report.scenarios
            .FirstOrDefault(item => item.name == "s03-display-name-only");
        FerriteGraphCommon.Require(
            rename != null, "no scenario measured a designation change");
        foreach (string kind in new[] { "GameObject", "Mesh", "Material" })
        {
            Dictionary<string, long> before = rename.before_rows
                .Where(row => row.unity_type == kind)
                .ToDictionary(row => row.identifier, row => row.local_file_id, StringComparer.Ordinal);
            int compared = 0;
            int moved = 0;
            foreach (IdentifierRow row in rename.after_rows.Where(row => row.unity_type == kind))
            {
                if (!before.TryGetValue(row.identifier, out long was))
                {
                    continue;
                }
                ++compared;
                if (was != row.local_file_id)
                {
                    ++moved;
                }
                ++FerriteGraphCommon.Checks;
            }
            report.rename.Add(new RenameRow
            {
                unity_type = kind,
                identifiers_compared = compared,
                local_file_ids_that_moved = moved,
                the_identifier_alone_decides_the_local_file_id = compared > 0 && moved == 0,
            });
        }
    }

    private static void MeasureCollision(Report report, PlanScenario scenario)
    {
        string assetPath = AssetFolder + "/collision.fcadsyn";
        File.Copy(scenario.before, Path.GetFullPath(assetPath), true);
        report.collision_messages = FerriteGraphCommon.Import(assetPath);
        UnityEngine.Object[] published = AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .ToArray();
        report.collision_document_imported = published.Length > 0;
        report.collision_materials_published = published.Count(item => item is Material);
        // What the same document would publish if every identifier were
        // distinct, taken from the plan rather than from the import so a merge
        // is a difference rather than the baseline.
        report.collision_materials_expected =
            int.Parse(scenario.change.Split('=').Last(), CultureInfo.InvariantCulture);
        report.collision_merged_two_objects_into_one =
            report.collision_materials_published < report.collision_materials_expected;
        report.collision_was_refused = report.collision_messages
            .Any(message => message.Contains("error:") || message.Contains("exception:"));
        AssetDatabase.DeleteAsset(assetPath);
        ++FerriteGraphCommon.Checks;
    }

    private static ScenarioReport MeasureScenario(PlanScenario scenario)
    {
        string assetPath = AssetFolder + "/" + scenario.name + ".fcadsyn";
        string referencePath = AssetFolder + "/" + scenario.name + "-references.asset";
        string absolute = Path.GetFullPath(assetPath);

        FerriteGraphCommon.Require(
            File.Exists(scenario.before), "the scenario's first document is missing");
        FerriteGraphCommon.Require(
            File.Exists(scenario.after), "the scenario's second document is missing");

        ScenarioReport result = new ScenarioReport
        {
            name = scenario.name,
            change = scenario.change,
            files_are_byte_identical =
                FerriteGraphCommon.SameBytes(scenario.before, scenario.after),
            before_fnv1a64 = FerriteGraphCommon.Fingerprint(scenario.before),
            after_fnv1a64 = FerriteGraphCommon.Fingerprint(scenario.after),
        };

        File.Copy(scenario.before, absolute, true);
        result.warnings_before = FerriteGraphCommon.Import(assetPath);
        View before = BuildView(assetPath);
        result.before_rows = Rows(before);

        List<ReferenceReport> references = new List<ReferenceReport>();
        List<Mesh> meshes = new List<Mesh>();
        List<Material> materials = new List<Material>();
        List<GameObject> objects = new List<GameObject>();
        List<string> kinds = new List<string>();
        List<string> semantics = new List<string>();

        foreach (string definition in scenario.mesh_definitions)
        {
            List<NodeInfo> bearers = before.Nodes
                .Where(node => node.DefinitionId == definition && node.SharedMesh != null)
                .ToList();
            FerriteGraphCommon.Require(
                bearers.Count > 0, "no imported object carries a mesh for " + definition);
            FerriteGraphCommon.Require(
                bearers.Select(node => FerriteGraphCommon.InstanceKey(node.SharedMesh)).Distinct().Count() == 1,
                "the synthetic importer published more than one Mesh for " + definition);
            meshes.Add(bearers[0].SharedMesh);
            semantics.Add(MeshSemantic(before, bearers[0].MeshLocalId));
            kinds.Add("Mesh");
            references.Add(Anchor(
                "mesh:" + definition,
                "Mesh",
                bearers[0].SharedMesh,
                FerriteSyntheticImporter.MeshIdentifier(definition)));
        }
        foreach (string binding in scenario.material_bindings)
        {
            string[] pieces = binding.Split('@');
            FerriteGraphCommon.Require(pieces.Length == 2, "a material binding is not 'definition@slot'");
            int slot = int.Parse(pieces[1], CultureInfo.InvariantCulture);
            NodeInfo node = before.Nodes
                .FirstOrDefault(item => item.DefinitionId == pieces[0] && item.Materials.Length > slot);
            FerriteGraphCommon.Require(node != null, "no imported object carries " + binding);
            materials.Add(node.Materials[slot]);
            semantics.Add(MaterialSemantic(before, node.MaterialLocalIds[slot]));
            kinds.Add("Material");
            references.Add(Anchor(
                "material:" + binding,
                "Material",
                node.Materials[slot],
                FerriteSyntheticImporter.MaterialIdentifier(pieces[0], slot)));
        }
        foreach (string binding in scenario.object_bindings)
        {
            string[] pieces = binding.Split('@');
            FerriteGraphCommon.Require(
                pieces.Length == 2, "an object binding is not 'definition@occurrence'");
            NodeInfo node = before.Nodes.FirstOrDefault(
                item => item.DefinitionId == pieces[0] && item.OccurrenceId == pieces[1]);
            FerriteGraphCommon.Require(node != null, "no imported occurrence carries " + binding);
            objects.Add(node.Target);
            semantics.Add(ObjectSemantic(before, node.LocalId));
            kinds.Add("GameObject");
            references.Add(Anchor(
                "object:" + binding,
                "GameObject",
                node.Target,
                FerriteSyntheticImporter.ObjectIdentifier(pieces[0], pieces[1])));
        }
        for (int index = 0; index < references.Count; ++index)
        {
            references[index].semantic_before = semantics[index];
        }

        AssetDatabase.DeleteAsset(referencePath);
        FerriteGraphReferences holder = ScriptableObject.CreateInstance<FerriteGraphReferences>();
        holder.meshes = meshes;
        holder.materials = materials;
        holder.objects = objects;
        AssetDatabase.CreateAsset(holder, referencePath);
        AssetDatabase.SaveAssets();
        List<StoredReference> stored = ReadStoredReferences(referencePath);
        FerriteGraphCommon.Require(
            stored.Count == references.Count,
            "the saved asset did not write one persistent reference per tracked object");
        for (int index = 0; index < references.Count; ++index)
        {
            references[index].stored_file_id = stored[index].FileId;
            references[index].stored_guid = FerriteGraphCommon.GuidToken(stored[index].Guid);
            FerriteGraphCommon.Require(
                stored[index].FileId == references[index].local_file_id_before,
                "the file identifier a reference stores is not the object's local file identifier");
        }

        File.Copy(scenario.after, absolute, true);
        result.warnings_after = FerriteGraphCommon.Import(assetPath);
        result.warning_transition =
            FerriteGraphCommon.Transition(result.warnings_before, result.warnings_after);

        AssetDatabase.ImportAsset(
            referencePath, ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        FerriteGraphReferences reloaded =
            AssetDatabase.LoadAssetAtPath<FerriteGraphReferences>(referencePath);
        FerriteGraphCommon.Require(reloaded != null, "the asset holding the references did not come back");

        View after = BuildView(assetPath);
        result.after_rows = Rows(after);

        int meshIndex = 0;
        int materialIndex = 0;
        int objectIndex = 0;
        for (int index = 0; index < references.Count; ++index)
        {
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
            Resolve(references[index], resolved, after);
        }
        result.references = references;

        AssetDatabase.DeleteAsset(referencePath);
        AssetDatabase.DeleteAsset(assetPath);
        return result;
    }

    // ---------------------------------------------------------- the verdicts

    private static void Resolve(ReferenceReport reference, UnityEngine.Object resolved, View after)
    {
        reference.resolved_by_reloaded_asset =
            resolved == null ? "<null>" : FerriteGraphCommon.Identify(resolved);
        UnityEngine.Object byIdentifier = null;
        if (GlobalObjectId.TryParse(reference.raw_global_object_id, out GlobalObjectId parsed))
        {
            byIdentifier = GlobalObjectId.GlobalObjectIdentifierToObjectSlow(parsed);
        }
        reference.resolved_by_stored_identifier =
            byIdentifier == null ? "<null>" : FerriteGraphCommon.Identify(byIdentifier);
        FerriteGraphCommon.Require(
            reference.resolved_by_reloaded_asset == reference.resolved_by_stored_identifier,
            "the two independent ways of resolving one saved reference disagree");

        long present = FindBySemantic(after, reference.unity_type, reference.semantic_before);
        reference.semantic_object_present_after = present != -1L;
        reference.local_file_id_changed =
            present != -1L && present != reference.local_file_id_before;

        if (resolved == null)
        {
            reference.semantic_after = "<null>";
            reference.name_after = "<null>";
            reference.local_file_id_after = -1L;
            reference.verdict = reference.semantic_object_present_after
                ? "missing_though_object_still_exported"
                : "missing_because_object_was_removed";
            ++FerriteGraphCommon.Checks;
            return;
        }

        long landed = FerriteGraphCommon.LocalId(resolved);
        reference.semantic_after = Semantic(after, reference.unity_type, landed);
        reference.name_after = resolved.name;
        reference.local_file_id_after = landed;
        reference.display_name_changed = reference.name_after != reference.name_before;
        if (reference.semantic_after == reference.semantic_before)
        {
            reference.verdict = "same_semantic";
        }
        else
        {
            string before = DefinitionOf(reference.semantic_before);
            string now = DefinitionOf(reference.semantic_after);
            reference.verdict = before == now && before.Length > 0
                ? "same_definition_other_occurrence"
                : "retargeted_to_another_definition";
        }
        ++FerriteGraphCommon.Checks;
    }

    private static string DefinitionOf(string semantic)
    {
        int start = semantic.IndexOf("definition", StringComparison.Ordinal);
        if (start < 0)
        {
            return String.Empty;
        }
        int end = semantic.IndexOf(';', start);
        return end < 0 ? semantic.Substring(start) : semantic.Substring(start, end - start);
    }

    // --------------------------------------------------------- the semantics

    private static string MeshSemantic(View view, long id)
    {
        List<NodeInfo> holders = view.Nodes.Where(node => node.MeshLocalId == id).ToList();
        if (holders.Count == 0)
        {
            return "<not in this import>";
        }
        return "definitions=[" + String.Join(",", holders
            .Select(node => node.DefinitionId)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)) + "];vertices="
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
                    bindings.Add(node.DefinitionId + "@" + slot.ToString(CultureInfo.InvariantCulture));
                    found = node.Materials[slot];
                }
            }
        }
        if (found == null)
        {
            return "<not in this import>";
        }
        return "definition=[" + String.Join(",", bindings.Distinct().OrderBy(
            item => item, StringComparer.Ordinal)) + "];colour=" + FerriteGraphCommon.Colour(found);
    }

    private static string ObjectSemantic(View view, long id)
    {
        NodeInfo node = view.Nodes.FirstOrDefault(item => item.LocalId == id);
        if (node == null)
        {
            return "<not in this import>";
        }
        return "definition=" + node.DefinitionId + ";occurrence=" + node.OccurrenceId
            + ";at=" + FerriteGraphCommon.Position(node.WorldPosition);
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
            case "Mesh": candidates = view.Nodes.Select(node => node.MeshLocalId); break;
            case "Material": candidates = view.Nodes.SelectMany(node => node.MaterialLocalIds); break;
            default: candidates = view.Nodes.Select(node => node.LocalId); break;
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

    // ------------------------------------------------------------ the views

    private static View BuildView(string assetPath)
    {
        GameObject root = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
        FerriteGraphCommon.Require(root != null, "the synthetic importer published no GameObject");

        View view = new View();
        foreach (FerriteSyntheticTag tag in root.GetComponentsInChildren<FerriteSyntheticTag>(true))
        {
            GameObject target = tag.gameObject;
            MeshFilter filter = target.GetComponent<MeshFilter>();
            MeshRenderer renderer = target.GetComponent<MeshRenderer>();
            NodeInfo node = new NodeInfo
            {
                Path = HierarchyPath(target.transform),
                Target = target,
                Identifier = tag.identifier,
                DefinitionId = tag.definition_id,
                OccurrenceId = tag.occurrence_id,
                Kind = tag.kind,
                LocalId = FerriteGraphCommon.LocalId(target),
                WorldPosition = target.transform.position,
                SharedMesh = filter == null ? null : filter.sharedMesh,
                Materials = renderer == null ? Array.Empty<Material>() : renderer.sharedMaterials,
            };
            node.MeshLocalId =
                node.SharedMesh == null ? -1L : FerriteGraphCommon.LocalId(node.SharedMesh);
            node.MaterialLocalIds = node.Materials
                .Select(material => material == null ? -1L : FerriteGraphCommon.LocalId(material))
                .ToArray();
            view.Nodes.Add(node);
        }
        view.Nodes = view.Nodes.OrderBy(node => node.Identifier, StringComparer.Ordinal).ToList();
        view.Subassets = AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .ToList();

        FerriteGraphCommon.Require(
            view.Nodes.All(node => node.Identifier.Length > 0),
            "an imported object arrived without the identifier the importer gave it");
        List<long> identifiers = new List<long>();
        foreach (UnityEngine.Object item in view.Subassets)
        {
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(item, out string guid, out long local);
            identifiers.Add(local);
            FerriteGraphCommon.CountIdentifierShape(item, FerriteGraphCommon.GuidToken(guid), local);
        }
        FerriteGraphCommon.Require(
            identifiers.Distinct().Count() == identifiers.Count,
            "one import gave two sub-objects the same local file identifier");
        return view;
    }

    private static string HierarchyPath(Transform transform)
    {
        return transform.parent == null
            ? transform.name
            : HierarchyPath(transform.parent) + "/" + transform.name;
    }

    private static List<IdentifierRow> Rows(View view)
    {
        List<IdentifierRow> rows = new List<IdentifierRow>();
        foreach (NodeInfo node in view.Nodes)
        {
            rows.Add(Row("GameObject", node.Identifier, node.Target));
            if (node.SharedMesh != null)
            {
                rows.Add(Row(
                    "Mesh",
                    FerriteSyntheticImporter.MeshIdentifier(node.DefinitionId),
                    node.SharedMesh));
            }
            for (int slot = 0; slot < node.Materials.Length; ++slot)
            {
                if (node.Materials[slot] != null)
                {
                    rows.Add(Row(
                        "Material",
                        FerriteSyntheticImporter.MaterialIdentifier(node.DefinitionId, slot),
                        node.Materials[slot]));
                }
            }
        }
        return rows
            .GroupBy(row => row.unity_type + "|" + row.identifier, StringComparer.Ordinal)
            .Select(group => group.First())
            .OrderBy(row => row.unity_type, StringComparer.Ordinal)
            .ThenBy(row => row.identifier, StringComparer.Ordinal)
            .ToList();
    }

    private static IdentifierRow Row(string kind, string identifier, UnityEngine.Object target)
    {
        ++FerriteGraphCommon.Checks;
        return new IdentifierRow
        {
            unity_type = kind,
            identifier = identifier,
            visible_name = target.name,
            local_file_id = FerriteGraphCommon.LocalId(target),
            visible_name_is_a_machine_token = IsMachine(target.name),
        };
    }

    private static bool IsMachine(string name)
    {
        return name.StartsWith(FerriteGraphCommon.MachinePrefix, StringComparison.Ordinal)
            || name.StartsWith("fcad|", StringComparison.Ordinal);
    }

    private static ReferenceReport Anchor(
        string anchor, string kind, UnityEngine.Object target, string identifier)
    {
        AssetDatabase.TryGetGUIDAndLocalFileIdentifier(target, out string guid, out long local);
        FerriteGraphCommon.Require(guid.Length > 0, "a tracked object has no asset GUID");
        FerriteGraphCommon.GuidToken(guid);
        return new ReferenceReport
        {
            anchor = anchor,
            unity_type = kind,
            identifier = identifier,
            name_before = target.name,
            local_file_id_before = local,
            global_object_id_before =
                FerriteGraphCommon.CanonicalIdentifier(GlobalObjectId.GetGlobalObjectIdSlow(target)),
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
}
