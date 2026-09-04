// SPDX-License-Identifier: MIT
//
// §22B-1e2b, part B and part F: does a *different FBX graph* move the identity
// of a shared `Mesh` off the placement that happens to reach it first?
//
// §22B-1e2a measured names and custom properties on the flat production graph
// and found that it does not, there. It said so, and it said explicitly that
// it had not measured any other graph. This probe measures four of them, and
// it measures them the same way §22B-1e1 and §22B-1e2a measured theirs, so the
// three results can be read together:
//
//   1. Non-null is not survival. A resolved object must still mean the same
//      FerriteCAD definition, cross-checked against a witness the identity
//      scheme did not supply — a vertex count for a mesh, a colour for a
//      material, a placement's own translation for a `GameObject`.
//   2. An identity that cannot tell two definitions apart is reported as
//      ambiguous, never as a kept reference.
//   3. Geometry, Model and Material are measured separately, always.
//   4. The visible names are part of the measurement, and so is every object
//      the graph added: a variant that keeps a reference by publishing one
//      more `GameObject`, one more renderer or one more visible machine token
//      has been paid for, and the price is recorded next to the result.
//   5. The transform a placement ends up with is measured, not assumed. A
//      graph that re-parents geometry and gets the world transform wrong is a
//      wrong graph however stable its identifiers are.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using UnityEditor;
using UnityEngine;

internal static class FerriteGraphProbe
{
    private const string AssetFolder = "Assets/Graph";

    // The node key given to a root Unity invented rather than read. Written
    // into the report as itself, so a reader can see which graphs cost one.
    private const string SyntheticRootKey = "<import root Unity invented>";

    // ------------------------------------------------------------ the plan

    [Serializable]
    internal sealed class Plan
    {
        public List<PlanVariant> variants = new List<PlanVariant>();
        public List<PlanScenario> scenarios = new List<PlanScenario>();
    }

    [Serializable]
    internal sealed class PlanVariant
    {
        public string name = String.Empty;
        public string written_by = String.Empty;
        public string topology = String.Empty;
        // What the variant claims its files carry. Claimed, not trusted: the
        // probe measures what the import really delivered and refuses a
        // disagreement.
        public bool carries_definition_id;
        public bool carries_occurrence_id;
        public bool adds_objects;
    }

    [Serializable]
    internal sealed class PlanScenario
    {
        public string name = String.Empty;
        public string variant = String.Empty;
        public string change = String.Empty;
        public string before = String.Empty;
        public string after = String.Empty;
        public List<string> mesh_definitions = new List<string>();
        public List<string> material_bindings = new List<string>();
        public List<string> object_bindings = new List<string>();
    }

    // ---------------------------------------------------------- the report

    [Serializable]
    internal sealed class Report
    {
        public string schema = "ferritecad.unity-graph-identity.v1";
        public string mode = "graph";
        public string unity_version = String.Empty;
        public string colour_space = String.Empty;
        public int distinct_asset_guids;
        public int subassets_whose_identifier_is_the_guid_and_local_id;
        public int subassets_whose_identifier_is_something_else;
        public List<VariantReport> variants = new List<VariantReport>();
        public List<ScenarioReport> scenarios = new List<ScenarioReport>();
        public int checks;
    }

    // What one variant's base document actually delivered to the editor.
    [Serializable]
    internal sealed class VariantReport
    {
        public string name = String.Empty;
        public string definition_join = String.Empty;
        public string occurrence_join = String.Empty;

        // ---- what the graph published
        public int game_objects;
        public int mesh_filters;
        public int mesh_renderers;
        public int meshes;
        public int materials;
        public int occurrence_nodes;
        // Every node that is not a placement: a definition carrier, a
        // geometry-bearing child, and a root Unity invented because the file
        // stopped having exactly one top-level node.
        public int carrier_nodes;
        public int structural_nodes;
        public int omitted_nodes;
        public int triangles;
        public int material_slots;
        // What the nodes that are not placements publish. A carrier with its
        // own renderer and its own slot count is a cost, and it is counted
        // apart from the placements so it cannot hide inside a total.
        public int carrier_renderers;
        public int carrier_material_slots;
        // Mesh-bearing nodes that no placement contains, and where they draw.
        // Counted rather than refused: for a carrier graph this is the price,
        // and the price is a result.
        public int geometry_drawn_outside_any_placement;
        public List<string> geometry_positions_outside_any_placement = new List<string>();

        // ---- the shared mesh
        // Definitions this document places more than once, and how many of
        // those really hand every placement one and the same `Mesh` object.
        // Reference equality, not an equal vertex count.
        public int definitions_with_several_placements;
        public int definitions_whose_placements_share_one_mesh;
        public List<string> definitions_with_a_split_mesh = new List<string>();

        // ---- what a person reads
        public string root_visible_name = String.Empty;
        public bool root_name_is_the_asset_file_name;
        // Whether Unity had to invent a root because the file no longer has
        // exactly one top-level node. A graph that puts a second object under
        // the scene root buys its result with one more GameObject a person
        // sees, and that is a measured cost rather than an implementation
        // detail.
        public bool import_root_is_synthetic;
        public int visible_nodes_named_by_machine_token;
        public int visible_nodes_named_by_designation;
        public int subassets_named_after_the_asset_file;
        public int subassets_named_by_machine_token;
        public int subassets_named_by_designation;
        public int meshes_named_after_their_node;
        public int meshes_named_otherwise;
        public List<string> visible_node_names = new List<string>();
        public List<string> visible_mesh_names = new List<string>();
        public List<string> visible_material_names = new List<string>();

        // ---- the join
        public int ambiguous_definitions = -1;
        public List<string> ambiguous_definition_names = new List<string>();

        // ---- what the control has to agree with, per occurrence
        public List<PlacementReport> placements = new List<PlacementReport>();
        public List<string> warnings = new List<string>();
    }

    // One occurrence, as the editor built it. Compared with the control's row
    // for the same node key by the verifier: a graph that moved a part is a
    // wrong graph whatever it did to the identifiers.
    [Serializable]
    internal sealed class PlacementReport
    {
        public string node_key = String.Empty;
        public string definition = String.Empty;
        public string visible_name = String.Empty;
        public string local_position = String.Empty;
        public string local_rotation = String.Empty;
        public string local_scale = String.Empty;
        public string world_position = String.Empty;
        public string world_rotation = String.Empty;
        public string world_scale = String.Empty;
        public int triangles;
        public int material_slots;
        public string mesh_unity_name = String.Empty;
        public int mesh_vertex_count;
        public int renderers_under_this_placement;
        public int extra_nodes_under_this_placement;
        // Where the geometry under this placement actually ends up. The
        // placement's own transform is not enough: a graph that moves the
        // geometry onto a child can leave the placement exactly where the
        // control puts it and still draw the part somewhere else. That mutant
        // survived the first edition of this harness, and these three fields
        // are the check that kills it.
        public string geometry_world_position = String.Empty;
        public string geometry_world_rotation = String.Empty;
        public string geometry_world_scale = String.Empty;
    }

    [Serializable]
    internal sealed class ScenarioReport
    {
        public string name = String.Empty;
        public string variant = String.Empty;
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
    internal sealed class ViewReport
    {
        public List<SubassetReport> subassets = new List<SubassetReport>();
        public List<NodeReport> nodes = new List<NodeReport>();
    }

    [Serializable]
    internal sealed class SubassetReport
    {
        public string unity_type = String.Empty;
        public string unity_name = String.Empty;
        public string asset_guid = String.Empty;
        public long local_file_id;
    }

    [Serializable]
    internal sealed class NodeReport
    {
        public string sibling_path = String.Empty;
        public string unity_name = String.Empty;
        public long local_file_id;
        public string node_key = String.Empty;
        public string definition_key = String.Empty;
        public string source_id = String.Empty;
        public string definition_id = String.Empty;
        public string occurrence_id = String.Empty;
        public string graph_role = String.Empty;
        public string omission = String.Empty;
        public string resolved_definition = String.Empty;
        public string resolved_occurrence = String.Empty;
        public string local_position = String.Empty;
        public string world_position = String.Empty;
        public bool has_mesh_filter;
        public bool has_mesh_renderer;
        public long mesh_local_file_id;
        public string mesh_unity_name = String.Empty;
        public int mesh_vertex_count;
        public List<long> material_local_file_ids = new List<long>();
        public List<string> material_unity_names = new List<string>();
    }

    [Serializable]
    internal sealed class ReferenceReport
    {
        public string anchor = String.Empty;
        public string unity_type = String.Empty;
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
        public string meaning_verdict = String.Empty;
        public string verdict = String.Empty;
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
        public string GraphRole = String.Empty;
        public string Omission = String.Empty;
        public Mesh SharedMesh;
        public Material[] Materials = Array.Empty<Material>();
        public long LocalId;
        public long MeshLocalId = -1L;
        public long[] MaterialLocalIds = Array.Empty<long>();
        public Vector3 LocalPosition;
        public Vector3 LocalScale;
        public Quaternion LocalRotation;
        public Vector3 WorldPosition;
        public Vector3 WorldScale;
        public Quaternion WorldRotation;
        public bool HasFilter;
        public bool HasRenderer;

        // A node the file carries no FerriteCAD role for is an occurrence: the
        // control has no role property at all, and treating its nodes as
        // anything else would compare a graph with a differently-read control.
        public bool IsOccurrence
        {
            get { return GraphRole.Length == 0 || GraphRole == "occurrence"; }
        }

        public bool IsInventedRoot
        {
            get { return GraphRole == "import_root"; }
        }
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
            plan != null && plan.scenarios.Count > 0, "the plan named no scenario");
        FerriteGraphCommon.Require(plan.variants.Count > 0, "the plan named no variant");

        if (!AssetDatabase.IsValidFolder(AssetFolder))
        {
            AssetDatabase.CreateFolder("Assets", "Graph");
        }

        Report report = new Report
        {
            unity_version = Application.unityVersion,
            colour_space = QualitySettings.activeColorSpace.ToString().ToLowerInvariant(),
        };

        Dictionary<string, PlanVariant> variants = new Dictionary<string, PlanVariant>();
        foreach (PlanVariant variant in plan.variants)
        {
            variants[variant.name] = variant;
            report.variants.Add(new VariantReport { name = variant.name });
        }
        Dictionary<string, VariantReport> measured = report.variants
            .ToDictionary(item => item.name, item => item, StringComparer.Ordinal);

        foreach (PlanScenario scenario in plan.scenarios)
        {
            FerriteGraphCommon.Require(
                variants.ContainsKey(scenario.variant),
                "the plan measures a scenario of an undeclared variant: " + scenario.variant);
            report.scenarios.Add(
                MeasureScenario(scenario, variants[scenario.variant], measured));
        }

        foreach (VariantReport variant in report.variants)
        {
            FerriteGraphCommon.Require(
                variant.ambiguous_definitions >= 0,
                "no scenario measured the variant " + variant.name);
        }

        report.distinct_asset_guids = FerriteGraphCommon.DistinctGuids;
        report.subassets_whose_identifier_is_the_guid_and_local_id =
            FerriteGraphCommon.DerivedIdentifiers;
        report.subassets_whose_identifier_is_something_else =
            FerriteGraphCommon.OtherIdentifiers;
        FerriteGraphCommon.Require(
            FerriteGraphCommon.DerivedIdentifiers > 0,
            "no sub-asset identifier was examined at all");
        return report;
    }

    // ---------------------------------------------------------- the scenarios

    private static ScenarioReport MeasureScenario(
        PlanScenario scenario,
        PlanVariant variant,
        Dictionary<string, VariantReport> measured)
    {
        string assetPath = AssetFolder + "/" + scenario.name.Replace('/', '_') + ".fbx";
        string referencePath = AssetFolder + "/" + scenario.name.Replace('/', '_')
            + "-references.asset";
        string absolute = Path.GetFullPath(assetPath);

        FerriteGraphCommon.Require(
            File.Exists(scenario.before), "the scenario's first file is missing");
        FerriteGraphCommon.Require(
            File.Exists(scenario.after), "the scenario's second file is missing");

        ScenarioReport result = new ScenarioReport
        {
            name = scenario.name,
            variant = scenario.variant,
            change = scenario.change,
            files_are_byte_identical =
                FerriteGraphCommon.SameBytes(scenario.before, scenario.after),
            before_bytes = new FileInfo(scenario.before).Length,
            before_fnv1a64 = FerriteGraphCommon.Fingerprint(scenario.before),
            after_bytes = new FileInfo(scenario.after).Length,
            after_fnv1a64 = FerriteGraphCommon.Fingerprint(scenario.after),
        };

        File.Copy(scenario.before, absolute, true);
        FerriteGraphCommon.SettleImporter(assetPath, requireDefaultSort: false);
        result.warnings_before = FerriteGraphCommon.Import(assetPath);
        View before = BuildView(assetPath);
        result.before = Describe(before, variant);

        VariantReport summary = measured[scenario.variant];
        if (summary.ambiguous_definitions < 0)
        {
            Summarise(summary, variant, before, assetPath, result.warnings_before);
        }

        List<ReferenceReport> references = new List<ReferenceReport>();
        List<Mesh> meshes = new List<Mesh>();
        List<Material> materials = new List<Material>();
        List<GameObject> objects = new List<GameObject>();
        List<string> semantics = new List<string>();
        List<string> kinds = new List<string>();

        foreach (string definition in scenario.mesh_definitions)
        {
            List<NodeInfo> bearers = Bearing(before, definition);
            FerriteGraphCommon.Require(
                bearers.Count > 0,
                "no imported object carries a mesh for the tracked definition " + definition);
            // A graph that published more than one `Mesh` for one definition
            // is a negative result, not a harness failure, so it is recorded
            // in the variant summary and reported as an ambiguous anchor here
            // rather than raised: the point of measuring four graphs is to
            // find out which of them do this.
            NodeInfo node = bearers[0];
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
            FerriteGraphCommon.Require(
                pieces.Length == 2, "a material binding is not 'definition@slot'");
            int slot = int.Parse(pieces[1], CultureInfo.InvariantCulture);
            List<NodeInfo> bearers = Bearing(before, pieces[0]);
            FerriteGraphCommon.Require(bearers.Count > 0, "no imported object carries " + pieces[0]);
            NodeInfo node = bearers[0];
            FerriteGraphCommon.Require(
                slot < node.Materials.Length,
                "the tracked material slot " + binding + " is not there; the imported node named "
                    + node.Target.name + " has "
                    + node.Materials.Length.ToString(CultureInfo.InvariantCulture) + " slots");
            materials.Add(node.Materials[slot]);
            semantics.Add(MaterialSemantic(before, node.MaterialLocalIds[slot]));
            kinds.Add("Material");
            ReferenceReport tracked =
                Anchor("material:" + binding, "Material", node.Materials[slot]);
            tracked.join_was_ambiguous = Ambiguous(before, node);
            references.Add(tracked);
        }
        foreach (string binding in scenario.object_bindings)
        {
            string[] pieces = binding.Split('@');
            FerriteGraphCommon.Require(
                pieces.Length == 2, "an object binding is not 'definition@occurrence'");
            int occurrence = int.Parse(pieces[1], CultureInfo.InvariantCulture);
            // Occurrences only. A carrier or a geometry-bearing child is a
            // machine object; tracking one as if it were a placement would let
            // a variant answer the placement question with an object no
            // project would ever reference.
            List<NodeInfo> matches = Matching(before, pieces[0])
                .Where(node => node.IsOccurrence)
                .ToList();
            FerriteGraphCommon.Require(
                occurrence < matches.Count,
                "no imported occurrence carries the tracked binding " + binding);
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

        // ---- the document after the change, imported over the same path.
        File.Copy(scenario.after, absolute, true);
        result.warnings_after = FerriteGraphCommon.Import(assetPath);
        result.warning_transition =
            FerriteGraphCommon.Transition(result.warnings_before, result.warnings_after);

        AssetDatabase.ImportAsset(
            referencePath,
            ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        FerriteGraphReferences reloaded =
            AssetDatabase.LoadAssetAtPath<FerriteGraphReferences>(referencePath);
        FerriteGraphCommon.Require(reloaded != null, "the asset holding the references did not come back");

        View after = BuildView(assetPath);
        result.after = Describe(after, variant);

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

    // --------------------------------------------------------- the summaries

    private static void Summarise(
        VariantReport summary,
        PlanVariant variant,
        View view,
        string assetPath,
        List<string> warnings)
    {
        // A root Unity invented corresponds to nothing in the file and so
        // carries no FerriteCAD property. It is left out of the identity
        // counts and recorded on its own line instead, because counting it as
        // a node without an identity would make every graph that costs one
        // look like a graph that lost its identity channel.
        List<NodeInfo> carried = view.Nodes.Where(node => !node.IsInventedRoot).ToList();
        int withDefinitionId = carried.Count(node => node.DefinitionId.Length > 0);
        int withOccurrenceId =
            carried.Count(node => node.IsOccurrence && node.OccurrenceId.Length > 0);
        int occurrences = carried.Count(node => node.IsOccurrence);
        summary.definition_join = withDefinitionId == carried.Count
            ? "FerriteCADDefinitionId"
            : "FerriteCADDefinitionKey";
        summary.occurrence_join = withOccurrenceId == occurrences
            ? "FerriteCADOccurrenceId"
            : "ordinal_in_scene_order";
        FerriteGraphCommon.Require(
            variant.carries_definition_id == (withDefinitionId == carried.Count),
            "the variant " + variant.name + " does not carry the definition identity it claims");
        FerriteGraphCommon.Require(
            variant.carries_occurrence_id == (withOccurrenceId == occurrences),
            "the variant " + variant.name + " does not carry the occurrence identity it claims");

        summary.warnings = warnings;
        summary.game_objects = view.Nodes.Count;
        summary.mesh_filters = view.Nodes.Count(node => node.HasFilter);
        summary.mesh_renderers = view.Nodes.Count(node => node.HasRenderer);
        summary.occurrence_nodes = occurrences;
        summary.carrier_nodes = view.Nodes.Count(node => !node.IsOccurrence);
        summary.structural_nodes = view.Nodes
            .Count(node => node.IsOccurrence && node.SharedMesh == null && node.Omission.Length == 0);
        summary.omitted_nodes = view.Nodes.Count(node => node.Omission.Length > 0);
        summary.material_slots = view.Nodes.Sum(node => node.Materials.Length);
        summary.carrier_renderers = view.Nodes.Count(node => !node.IsOccurrence && node.HasRenderer);
        summary.carrier_material_slots = view.Nodes
            .Where(node => !node.IsOccurrence)
            .Sum(node => node.Materials.Length);
        List<NodeInfo> outside = view.Nodes
            .Where(node => node.SharedMesh != null && !InsideAPlacement(view, node))
            .ToList();
        summary.geometry_drawn_outside_any_placement = outside.Count;
        summary.geometry_positions_outside_any_placement = outside
            .Select(node => Definition(node) + "@"
                + FerriteGraphCommon.Position(node.WorldPosition))
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();

        List<UnityEngine.Object> subassets = view.Subassets;
        summary.meshes = subassets.Count(item => item is Mesh);
        summary.materials = subassets.Count(item => item is Material);
        summary.triangles = subassets.OfType<Mesh>().Sum(mesh => (int)(mesh.triangles.Length / 3));
        ++FerriteGraphCommon.Checks;

        // ---- the shared mesh, by reference equality.
        List<string> split = new List<string>();
        int several = 0;
        int shared = 0;
        foreach (string definition in view.Nodes
            .Select(Definition)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal))
        {
            List<NodeInfo> bearers = view.Nodes
                .Where(node => Definition(node) == definition && node.SharedMesh != null)
                .ToList();
            List<NodeInfo> placements = view.Nodes
                .Where(node => Definition(node) == definition && node.IsOccurrence)
                .ToList();
            if (placements.Count < 2 || bearers.Count == 0)
            {
                continue;
            }
            ++several;
            if (bearers.Select(node => ReferenceEqualityKey(node.SharedMesh)).Distinct().Count() == 1)
            {
                ++shared;
            }
            else
            {
                split.Add(definition);
            }
            ++FerriteGraphCommon.Checks;
        }
        summary.definitions_with_several_placements = several;
        summary.definitions_whose_placements_share_one_mesh = shared;
        summary.definitions_with_a_split_mesh = split;

        // ---- the join.
        List<string> ambiguous = new List<string>();
        foreach (string definition in view.Nodes
            .Select(Definition)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal))
        {
            List<NodeInfo> nodes = view.Nodes.Where(node => Definition(node) == definition).ToList();
            if (nodes.Where(node => node.MeshLocalId != -1L)
                .Select(node => node.MeshLocalId)
                .Distinct()
                .Count() > 1)
            {
                ambiguous.Add(definition);
            }
            ++FerriteGraphCommon.Checks;
        }
        summary.ambiguous_definitions = ambiguous.Count;
        summary.ambiguous_definition_names = ambiguous;

        // ---- what a person reads.
        string stem = Path.GetFileNameWithoutExtension(assetPath);
        summary.root_visible_name = view.Nodes[0].Target.name;
        summary.root_name_is_the_asset_file_name = summary.root_visible_name == stem;
        summary.import_root_is_synthetic = view.Nodes[0].NodeKey == SyntheticRootKey;
        summary.visible_nodes_named_by_machine_token = view.Nodes
            .Skip(1)
            .Count(node => node.Target.name.StartsWith(
                FerriteGraphCommon.MachinePrefix, StringComparison.Ordinal));
        summary.visible_nodes_named_by_designation =
            view.Nodes.Count - 1 - summary.visible_nodes_named_by_machine_token;
        summary.meshes_named_after_their_node = view.Nodes
            .Count(node => node.SharedMesh != null && node.SharedMesh.name == node.Target.name);
        summary.meshes_named_otherwise = view.Nodes
            .Count(node => node.SharedMesh != null && node.SharedMesh.name != node.Target.name);
        summary.visible_node_names = view.Nodes
            .Skip(1)
            .Select(node => node.Target.name)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();
        summary.visible_mesh_names = subassets.OfType<Mesh>()
            .Select(mesh => mesh.name)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();
        summary.visible_material_names = subassets.OfType<Material>()
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
            else if (item.unity_name.StartsWith(
                FerriteGraphCommon.MachinePrefix, StringComparison.Ordinal))
            {
                ++summary.subassets_named_by_machine_token;
            }
            else
            {
                ++summary.subassets_named_by_designation;
            }
        }
        ++FerriteGraphCommon.Checks;

        // ---- one row per occurrence, for the transform comparison.
        foreach (NodeInfo node in view.Nodes.Where(
            item => item.IsOccurrence && !item.IsInventedRoot))
        {
            List<NodeInfo> subtree = Subtree(view, node);
            List<NodeInfo> bearers = subtree.Where(item => item.SharedMesh != null).ToList();
            summary.placements.Add(new PlacementReport
            {
                node_key = node.NodeKey,
                definition = Definition(node),
                visible_name = node.Target.name,
                local_position = FerriteGraphCommon.Position(node.LocalPosition),
                local_rotation = Rotation(node.LocalRotation),
                local_scale = FerriteGraphCommon.Position(node.LocalScale),
                world_position = FerriteGraphCommon.Position(node.WorldPosition),
                world_rotation = Rotation(node.WorldRotation),
                world_scale = FerriteGraphCommon.Position(node.WorldScale),
                triangles = bearers.Sum(item => item.SharedMesh.triangles.Length / 3),
                material_slots = subtree.Sum(item => item.Materials.Length),
                mesh_unity_name = bearers.Count == 0 ? "<none>" : bearers[0].SharedMesh.name,
                mesh_vertex_count = bearers.Count == 0 ? -1 : bearers[0].SharedMesh.vertexCount,
                geometry_world_position = Joined(
                    bearers, item => FerriteGraphCommon.Position(item.WorldPosition)),
                geometry_world_rotation = Joined(bearers, item => Rotation(item.WorldRotation)),
                geometry_world_scale = Joined(
                    bearers, item => FerriteGraphCommon.Position(item.WorldScale)),
                renderers_under_this_placement = subtree.Count(item => item.HasRenderer),
                // Everything this graph put under a placement that the control
                // does not have. The verifier compares it with the control's
                // row, so "the carrier costs one more object" is a number.
                extra_nodes_under_this_placement = subtree.Count - 1,
            });
            ++FerriteGraphCommon.Checks;
        }
    }

    // Every mesh-bearing node under one placement, in one string. Sorted, so
    // the value is a property of what is drawn and not of the order the walk
    // happened to reach it in.
    private static string Joined(List<NodeInfo> nodes, Func<NodeInfo, string> render)
    {
        if (nodes.Count == 0)
        {
            return "<none>";
        }
        return String.Join(
            "+", nodes.Select(render).OrderBy(item => item, StringComparer.Ordinal));
    }

    private static List<NodeInfo> Subtree(View view, NodeInfo root)
    {
        List<NodeInfo> result = new List<NodeInfo>();
        foreach (NodeInfo node in view.Nodes)
        {
            if (node == root || node.Path.StartsWith(root.Path + "/", StringComparison.Ordinal))
            {
                result.Add(node);
            }
        }
        return result;
    }

    private static bool InsideAPlacement(View view, NodeInfo node)
    {
        foreach (NodeInfo other in view.Nodes)
        {
            if (!other.IsOccurrence || other.IsInventedRoot)
            {
                continue;
            }
            if (node == other
                || node.Path.StartsWith(other.Path + "/", StringComparison.Ordinal))
            {
                return true;
            }
        }
        return false;
    }

    private static string ReferenceEqualityKey(Mesh mesh)
    {
        return FerriteGraphCommon.InstanceKey(mesh).ToString(CultureInfo.InvariantCulture);
    }

    private static string Rotation(Quaternion value)
    {
        Vector3 euler = value.eulerAngles;
        return FerriteGraphCommon.Position(euler);
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
        reference.resolved_by_stored_identifier = byIdentifier == null
            ? "<null>"
            : FerriteGraphCommon.Identify(byIdentifier);
        FerriteGraphCommon.Require(
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
            long landed = FerriteGraphCommon.LocalId(resolved);
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

        reference.verdict = reference.join_was_ambiguous
            ? "ambiguous_join"
            : reference.meaning_verdict;
        ++FerriteGraphCommon.Checks;
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
        if (!node.IsOccurrence)
        {
            return "carrier:" + node.GraphRole;
        }
        int ordinal = view.Nodes
            .Where(other => other.IsOccurrence && Definition(other) == Definition(node))
            .ToList()
            .IndexOf(node);
        return "ordinal:" + ordinal.ToString(CultureInfo.InvariantCulture);
    }

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

    // The nodes of one definition a *project* would reference for its geometry
    // and its materials. In the flat graph that is the placement itself; under
    // the two-level graph it is the placement's own child; under a carrier
    // both the carrier and the placements bear the geometry, and the placement
    // is the one a person points at.
    //
    // A carrier that no placement contains is deliberately last. It is a
    // machine object, and answering the material question from it would let a
    // graph pass by publishing a node whose slots nobody asked for — which is
    // exactly what the carrier turned out to do.
    private static List<NodeInfo> Bearing(View view, string anchor)
    {
        List<NodeInfo> matches = Matching(view, anchor);
        List<NodeInfo> placements = matches
            .Where(node => node.IsOccurrence && node.SharedMesh != null)
            .ToList();
        if (placements.Count > 0)
        {
            return placements;
        }
        List<NodeInfo> children = matches
            .Where(node => node.SharedMesh != null && Under(view, node, anchor))
            .ToList();
        if (children.Count > 0)
        {
            return children;
        }
        return matches.Where(node => node.SharedMesh != null).ToList();
    }

    // Whether this geometry-bearing node sits inside a placement of the same
    // definition, which is what makes it that placement's geometry rather than
    // a free-standing machine object.
    private static bool Under(View view, NodeInfo node, string anchor)
    {
        foreach (NodeInfo other in Matching(view, anchor))
        {
            if (other.IsOccurrence
                && node.Path.StartsWith(other.Path + "/", StringComparison.Ordinal))
            {
                return true;
            }
        }
        return false;
    }

    private static bool Ambiguous(View view, NodeInfo node)
    {
        return view.Nodes
            .Where(other => Definition(other) == Definition(node) && other.MeshLocalId != -1L)
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
                    bindings.Add(
                        Definition(node) + "@" + slot.ToString(CultureInfo.InvariantCulture));
                    found = node.Materials[slot];
                }
            }
        }
        if (found == null)
        {
            return "<not in this import>";
        }
        bindings = bindings.Distinct().OrderBy(item => item, StringComparer.Ordinal).ToList();
        return "definition=[" + String.Join(",", bindings) + "];colour="
            + FerriteGraphCommon.Colour(found);
    }

    // A placement's durable meaning is which definition it places and which
    // occurrence of it this is, plus the placement's own *world* position —
    // which no identity scheme in this measurement supplies, so it is the
    // independent witness that a reference really landed where it says it did.
    // World, not local, because a graph that inserts a node between a
    // placement and the root would otherwise get a free pass.
    private static string ObjectSemantic(View view, long id)
    {
        NodeInfo node = view.Nodes.FirstOrDefault(item => item.LocalId == id);
        if (node == null)
        {
            return "<not in this import>";
        }
        return "definition=" + Definition(node) + ";occurrence=" + Occurrence(view, node)
            + ";at=" + FerriteGraphCommon.Position(node.WorldPosition);
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
        FerriteGraphCommon.Require(root != null, "Unity published no GameObject for the imported asset");

        Dictionary<string, Dictionary<string, string>> properties = ReadProperties(assetPath);

        View view = new View();
        Walk(root, "0", view, properties);
        view.Subassets = AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .ToList();

        // A file with exactly one top-level node arrives with that node as the
        // import's root; a file with more than one makes Unity invent a root
        // of its own, and an invented root carries no FerriteCAD property
        // because it corresponds to nothing in the file. That is allowed for
        // the root and for nothing else: any *other* node without the key the
        // file carries is a refusal.
        if (view.Nodes.Count > 0 && view.Nodes[0].NodeKey.Length == 0)
        {
            view.Nodes[0].NodeKey = SyntheticRootKey;
            view.Nodes[0].DefinitionKey = SyntheticRootKey;
            view.Nodes[0].GraphRole = "import_root";
        }
        FerriteGraphCommon.Require(
            view.Nodes.All(node => node.NodeKey.Length > 0),
            "an imported node arrived without the FerriteCAD node key the file carries");
        FerriteGraphCommon.Require(
            view.Nodes.All(node => node.DefinitionKey.Length > 0),
            "an imported node arrived without the FerriteCAD definition key the file carries");

        List<long> identifiers = new List<long>();
        foreach (UnityEngine.Object item in view.Subassets)
        {
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(item, out string guid, out long local);
            identifiers.Add(local);
        }
        FerriteGraphCommon.Require(
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
            LocalId = FerriteGraphCommon.LocalId(target),
            LocalPosition = target.transform.localPosition,
            LocalRotation = target.transform.localRotation,
            LocalScale = target.transform.localScale,
            WorldPosition = target.transform.position,
            WorldRotation = target.transform.rotation,
            WorldScale = target.transform.lossyScale,
        };
        if (properties.TryGetValue(path, out Dictionary<string, string> values))
        {
            node.NodeKey = Value(values, "FerriteCADNodeKey");
            node.DefinitionKey = Value(values, "FerriteCADDefinitionKey");
            node.SourceId = Value(values, "FerriteCADSourceId");
            node.DefinitionId = Value(values, "FerriteCADDefinitionId");
            node.OccurrenceId = Value(values, "FerriteCADOccurrenceId");
            node.GraphRole = Value(values, "FerriteCADGraphRole");
            node.Omission = Value(values, "FerriteCADGeometryOmission");
        }
        MeshFilter filter = target.GetComponent<MeshFilter>();
        node.HasFilter = filter != null;
        node.SharedMesh = filter == null ? null : filter.sharedMesh;
        node.MeshLocalId = node.SharedMesh == null ? -1L : FerriteGraphCommon.LocalId(node.SharedMesh);
        MeshRenderer renderer = target.GetComponent<MeshRenderer>();
        node.HasRenderer = renderer != null;
        node.Materials = renderer == null ? Array.Empty<Material>() : renderer.sharedMaterials;
        node.MaterialLocalIds = node.Materials
            .Select(material => material == null ? -1L : FerriteGraphCommon.LocalId(material))
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

    private static ViewReport Describe(View view, PlanVariant variant)
    {
        ViewReport report = new ViewReport();
        foreach (UnityEngine.Object item in view.Subassets
            .OrderBy(item => item.GetType().Name, StringComparer.Ordinal)
            .ThenBy(item => item.name, StringComparer.Ordinal)
            .ThenBy(FerriteGraphCommon.LocalId))
        {
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(item, out string guid, out long local);
            report.subassets.Add(new SubassetReport
            {
                unity_type = item.GetType().Name,
                unity_name = item.name,
                asset_guid = FerriteGraphCommon.GuidToken(guid),
                local_file_id = local,
            });
            FerriteGraphCommon.CountIdentifierShape(item, FerriteGraphCommon.GuidToken(guid), local);
            ++FerriteGraphCommon.Checks;
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
                graph_role = node.GraphRole,
                omission = node.Omission,
                resolved_definition = Definition(node),
                resolved_occurrence = Occurrence(view, node),
                local_position = FerriteGraphCommon.Position(node.LocalPosition),
                world_position = FerriteGraphCommon.Position(node.WorldPosition),
                has_mesh_filter = node.HasFilter,
                has_mesh_renderer = node.HasRenderer,
                mesh_local_file_id = node.MeshLocalId,
                mesh_unity_name = node.SharedMesh == null ? "<none>" : node.SharedMesh.name,
                mesh_vertex_count = node.SharedMesh == null ? -1 : node.SharedMesh.vertexCount,
                material_local_file_ids = node.MaterialLocalIds.ToList(),
                material_unity_names = node.Materials
                    .Select(material => material == null ? "<none>" : material.name)
                    .ToList(),
            });
            ++FerriteGraphCommon.Checks;
        }
        // Whether the objects a variant's files added arrived in the import
        // at all is a *result* — an unparented Model may simply be dropped —
        // so it is recorded in the rows above and joined to what pinned ufbx
        // read from the same bytes by the verifier, not asserted here.
        return report;
    }

    // ------------------------------------------------------------- plumbing

    private static List<SubassetReport> Subassets(string assetPath)
    {
        List<SubassetReport> result = new List<SubassetReport>();
        foreach (UnityEngine.Object item in AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .OrderBy(item => item.GetType().Name, StringComparer.Ordinal)
            .ThenBy(item => item.name, StringComparer.Ordinal)
            .ThenBy(FerriteGraphCommon.LocalId))
        {
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(item, out string guid, out long local);
            result.Add(new SubassetReport
            {
                unity_type = item.GetType().Name,
                unity_name = item.name,
                asset_guid = FerriteGraphCommon.GuidToken(guid),
                local_file_id = local,
            });
            ++FerriteGraphCommon.Checks;
        }
        return result;
    }

    private static ReferenceReport Anchor(string anchor, string kind, UnityEngine.Object target)
    {
        AssetDatabase.TryGetGUIDAndLocalFileIdentifier(target, out string guid, out long local);
        FerriteGraphCommon.Require(guid.Length > 0, "a tracked object has no asset GUID");
        FerriteGraphCommon.GuidToken(guid);
        return new ReferenceReport
        {
            anchor = anchor,
            unity_type = kind,
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
        string path = FerriteGraphProperties.CachePath(assetPath);
        FerriteGraphCommon.Require(
            File.Exists(path), "the custom-property callback did not run for this import");
        Dictionary<string, Dictionary<string, string>> result =
            new Dictionary<string, Dictionary<string, string>>();
        foreach (string line in File.ReadAllLines(path))
        {
            if (String.IsNullOrEmpty(line))
            {
                continue;
            }
            string[] fields = line.Split('\t');
            FerriteGraphCommon.Require(fields.Length == 3, "malformed custom-property line");
            if (!result.TryGetValue(fields[0], out Dictionary<string, string> values))
            {
                values = new Dictionary<string, string>();
                result[fields[0]] = values;
            }
            values[fields[1]] = fields[2];
        }
        return result;
    }
}
