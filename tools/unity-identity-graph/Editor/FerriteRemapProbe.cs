// SPDX-License-Identifier: MIT
//
// §22B-1e2b, part D: `AssetImporter.SourceAssetIdentifier` and `AddRemap`,
// measured separately for `Mesh`, `Material` and `GameObject`.
//
// `AddRemap` does not give an imported sub-asset a durable identity. It
// *replaces* an imported sub-asset with an external asset of the same type, so
// whatever a project references afterwards is a file the project owns rather
// than something the FBX produced. That is a real answer to "keep my
// references across a re-export" and a different answer from the one an FBX
// graph could give, and the difference is the first thing this report records:
// every row says how many external assets the project had to grow, and no row
// is ever presented as behaviour of a vanilla FBX.
//
// The key is the second thing. A `SourceAssetIdentifier` is a *type plus a
// name*. FerriteCAD's durable identity is neither, and §22B-1c measured that
// several definitions of one real assembly share a designation, so whether the
// key can address one object at all is measured on the same document that
// carries those collisions.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;

internal static class FerriteRemapProbe
{
    private const string AssetFolder = "Assets/Remap";

    [Serializable]
    internal sealed class Plan
    {
        public string control = String.Empty;
        public string renamed = String.Empty;
        public string reexport = String.Empty;
        public string removed_tracked_definition = String.Empty;
        public string changed_material = String.Empty;
        public string mesh_to_remap = String.Empty;
        public string material_to_remap = String.Empty;
        public string game_object_to_remap = String.Empty;
    }

    [Serializable]
    internal sealed class Report
    {
        public string schema = "ferritecad.unity-remap-identity.v1";
        public string mode = "remap";
        public string unity_version = String.Empty;
        // Said once, at the top, so no reader can take a row below for a
        // property of the exported file.
        public bool is_a_property_of_the_fbx;
        public string what_it_is = String.Empty;
        public int external_assets_the_project_gained;
        // How many `(type, name)` keys in this document name more than one
        // imported object. Not zero means the key cannot address them apart.
        public int keys_that_name_more_than_one_object;
        public List<string> ambiguous_keys = new List<string>();
        public List<TypeResult> types = new List<TypeResult>();
        public int checks;
    }

    [Serializable]
    internal sealed class TypeResult
    {
        public string unity_type = String.Empty;
        public string key_shape = "SourceAssetIdentifier(type, unity visible name)";
        public string identifier_name = String.Empty;
        public bool add_remap_threw;
        public string add_remap_error = String.Empty;
        public bool appears_in_the_external_object_map;
        public bool the_import_honoured_it;
        public string what_the_scene_points_at_now = String.Empty;
        public int external_assets_required;

        // ---- the sub-asset side of it
        public int subassets_of_this_type_before;
        public int subassets_of_this_type_after;
        public List<string> visible_names_before = new List<string>();
        public List<string> visible_names_after = new List<string>();
        public bool human_names_kept;
        public int placements_sharing_one_mesh_before;
        public int placements_sharing_one_mesh_after;
        public bool one_shared_mesh_kept;

        // ---- what a stored reference did
        public string stored_reference_verdict = String.Empty;
        public bool silently_retargeted;

        // ---- the transitions
        public bool mapping_survived_a_reexport;
        public bool mapping_survived_a_designation_rename;
        public string map_key_after_the_rename = String.Empty;
        public bool mapping_survived_removing_the_definition;
        public bool remap_left_a_dangling_entry_after_removal;

        // ---- stale external content
        //
        // Two different questions, kept apart. The first is whether the
        // external asset changed at all when the FBX did; the second is
        // whether a project *looking at the model* therefore sees content the
        // file no longer has, which needs the import to have honoured the
        // remap as well.
        public int external_vertex_count_before;
        public int external_vertex_count_after_the_fbx_changed;
        public string external_colour_before = String.Empty;
        public string external_colour_after_the_fbx_changed = String.Empty;
        public bool the_fbx_changed_this_object;
        public bool external_content_unchanged_after_the_fbx_changed;
        public bool the_scene_shows_content_the_fbx_no_longer_has;

        public List<string> warnings = new List<string>();
    }

    internal static Report Execute(string planPath)
    {
        Plan plan = JsonUtility.FromJson<Plan>(File.ReadAllText(planPath));
        FerriteGraphCommon.Require(plan != null, "the remap plan did not parse");
        foreach (string path in new[]
        {
            plan.control, plan.renamed, plan.reexport,
            plan.removed_tracked_definition, plan.changed_material,
        })
        {
            FerriteGraphCommon.Require(File.Exists(path), "the remap plan names a missing file: " + path);
        }

        if (!AssetDatabase.IsValidFolder(AssetFolder))
        {
            AssetDatabase.CreateFolder("Assets", "Remap");
        }

        Report report = new Report
        {
            unity_version = Application.unityVersion,
            is_a_property_of_the_fbx = false,
            what_it_is = "a project-side importer setting that replaces an imported sub-asset "
                + "with an external asset of the same type",
        };

        string assetPath = AssetFolder + "/remap-probe.fbx";
        string absolute = Path.GetFullPath(assetPath);
        File.Copy(plan.control, absolute, true);
        FerriteGraphCommon.Import(assetPath);
        FerriteGraphCommon.SettleImporter(assetPath, requireDefaultSort: false);

        report.ambiguous_keys = AmbiguousKeys(assetPath);
        report.keys_that_name_more_than_one_object = report.ambiguous_keys.Count;
        ++FerriteGraphCommon.Checks;

        report.types.Add(Measure(plan, assetPath, "Mesh"));
        report.types.Add(Measure(plan, assetPath, "Material"));
        report.types.Add(Measure(plan, assetPath, "GameObject"));
        report.external_assets_the_project_gained =
            report.types.Sum(item => item.external_assets_required);

        AssetDatabase.DeleteAsset(assetPath);
        // Nothing this probe made may outlive it, external assets included.
        foreach (string guid in AssetDatabase.FindAssets(String.Empty, new[] { AssetFolder }))
        {
            AssetDatabase.DeleteAsset(AssetDatabase.GUIDToAssetPath(guid));
        }
        AssetDatabase.DeleteAsset(AssetFolder);
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        return report;
    }

    // Whether the `changed-material` document really changes the object this
    // probe is about to remap, measured **before** any remap exists.
    //
    // Inferring it afterwards from "the name is no longer among the imported
    // objects" would be wrong for exactly the type the importer honours,
    // because an honoured remap removes that sub-asset too. A mutant that
    // pointed the probe at an object the documents leave alone survived on
    // that confusion once; this is the measurement that does not have it.
    private static bool DocumentChanges(Plan plan, string assetPath, string kind, string wanted)
    {
        string absolute = Path.GetFullPath(assetPath);
        File.Copy(plan.control, absolute, true);
        FerriteGraphCommon.Import(assetPath);
        string before = Content(assetPath, kind, wanted);
        File.Copy(plan.changed_material, absolute, true);
        FerriteGraphCommon.Import(assetPath);
        string after = Content(assetPath, kind, wanted);
        ++FerriteGraphCommon.Checks;
        return before != after;
    }

    // What an object of this type and name holds, or that it is absent.
    private static string Content(string assetPath, string kind, string wanted)
    {
        UnityEngine.Object found = AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null && item.GetType().Name == kind && item.name == wanted)
            .OrderBy(FerriteGraphCommon.LocalId)
            .FirstOrDefault();
        if (found == null)
        {
            return "<absent>";
        }
        if (found is Material material)
        {
            return "colour=" + FerriteGraphCommon.Colour(material);
        }
        if (found is Mesh mesh)
        {
            return "vertices=" + mesh.vertexCount.ToString(CultureInfo.InvariantCulture);
        }
        return "children="
            + ((GameObject)found).transform.childCount.ToString(CultureInfo.InvariantCulture);
    }

    private static TypeResult Measure(Plan plan, string assetPath, string kind)
    {
        bool documentChanges = DocumentChanges(plan, assetPath, kind, Wanted(plan, kind));

        string absolute = Path.GetFullPath(assetPath);
        File.Copy(plan.control, absolute, true);
        FerriteGraphCommon.Import(assetPath);

        TypeResult result = new TypeResult
        {
            unity_type = kind,
            the_fbx_changed_this_object = documentChanges,
        };
        result.subassets_of_this_type_before = Count(assetPath, kind);
        result.visible_names_before = Names(assetPath, kind);
        result.placements_sharing_one_mesh_before = SharedMeshPlacements(assetPath);

        // The object to replace, and the name that is the only key `AddRemap`
        // has for it.
        UnityEngine.Object target = Pick(assetPath, kind, Wanted(plan, kind));
        FerriteGraphCommon.Require(target != null, "the control published no " + kind + " to remap");
        result.identifier_name = target.name;

        // The external asset. One per remapped object, always: this is where
        // the cost of the mechanism is, and it is counted rather than
        // described.
        string externalPath;
        UnityEngine.Object external;
        switch (kind)
        {
            case "Mesh":
            {
                Mesh source = (Mesh)target;
                Mesh copy = UnityEngine.Object.Instantiate(source);
                copy.name = source.name;
                externalPath = AssetFolder + "/external-mesh.asset";
                AssetDatabase.CreateAsset(copy, externalPath);
                external = copy;
                result.external_vertex_count_before = copy.vertexCount;
                break;
            }
            case "Material":
            {
                Material source = (Material)target;
                Material copy = new Material(source);
                copy.name = source.name;
                externalPath = AssetFolder + "/external-material.mat";
                AssetDatabase.CreateAsset(copy, externalPath);
                external = copy;
                result.external_colour_before = FerriteGraphCommon.Colour(copy);
                break;
            }
            default:
            {
                GameObject source = (GameObject)target;
                externalPath = AssetFolder + "/external-object.prefab";
                GameObject instance = UnityEngine.Object.Instantiate(source);
                instance.name = source.name;
                GameObject prefab = PrefabUtility.SaveAsPrefabAsset(instance, externalPath);
                UnityEngine.Object.DestroyImmediate(instance);
                external = prefab;
                break;
            }
        }
        AssetDatabase.SaveAssets();
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        result.external_assets_required = 1;
        FerriteGraphCommon.Require(external != null, "the external " + kind + " asset was not created");

        // ---- a reference stored before the remap, so "silently retargeted"
        // is measurable rather than assumed.
        string referencePath = AssetFolder + "/remap-references.asset";
        AssetDatabase.DeleteAsset(referencePath);
        FerriteGraphReferences holder = ScriptableObject.CreateInstance<FerriteGraphReferences>();
        switch (kind)
        {
            case "Mesh": holder.meshes.Add((Mesh)target); break;
            case "Material": holder.materials.Add((Material)target); break;
            default: holder.objects.Add((GameObject)target); break;
        }
        AssetDatabase.CreateAsset(holder, referencePath);
        AssetDatabase.SaveAssets();
        string before = FerriteGraphCommon.Identify(target);

        // ---- the remap itself.
        AssetImporter importer = AssetImporter.GetAtPath(assetPath);
        FerriteGraphCommon.Require(importer != null, "the asset lost its importer");
        AssetImporter.SourceAssetIdentifier identifier =
            new AssetImporter.SourceAssetIdentifier(TypeOf(kind), result.identifier_name);
        List<string> messages = new List<string>();
        Application.LogCallback capture = (message, stack, level) =>
        {
            if (level == LogType.Warning || level == LogType.Error || level == LogType.Exception)
            {
                messages.Add(level.ToString().ToLowerInvariant() + ": "
                    + FerriteGraphCommon.Canonical(message));
            }
        };
        Application.logMessageReceived += capture;
        try
        {
            importer.AddRemap(identifier, external);
            importer.SaveAndReimport();
            AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        }
        catch (Exception error)
        {
            result.add_remap_threw = true;
            result.add_remap_error = FerriteGraphCommon.Canonical(error.Message);
        }
        finally
        {
            Application.logMessageReceived -= capture;
        }
        result.warnings = FerriteGraphCommon.Group(messages);

        importer = AssetImporter.GetAtPath(assetPath);
        result.appears_in_the_external_object_map = InMap(importer, identifier);
        result.subassets_of_this_type_after = Count(assetPath, kind);
        result.visible_names_after = Names(assetPath, kind);
        result.placements_sharing_one_mesh_after = SharedMeshPlacements(assetPath);
        result.human_names_kept = result.visible_names_after
            .All(name => !name.StartsWith(FerriteGraphCommon.MachinePrefix, StringComparison.Ordinal));
        result.one_shared_mesh_kept =
            result.placements_sharing_one_mesh_after == result.placements_sharing_one_mesh_before;
        result.what_the_scene_points_at_now = WhatTheSceneUses(assetPath, kind, external);
        // Honoured means the import really uses the external object, not that
        // the setting was accepted. Unity accepts a remap of a type its model
        // importer never consults, and a report that stopped at "accepted"
        // would call that a working mechanism.
        result.the_import_honoured_it =
            result.what_the_scene_points_at_now.StartsWith("external:", StringComparison.Ordinal);
        ++FerriteGraphCommon.Checks;

        // ---- what the stored reference did.
        AssetDatabase.ImportAsset(
            referencePath, ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        FerriteGraphReferences reloaded =
            AssetDatabase.LoadAssetAtPath<FerriteGraphReferences>(referencePath);
        UnityEngine.Object resolved = kind == "Mesh"
            ? (UnityEngine.Object)(reloaded.meshes.Count > 0 ? reloaded.meshes[0] : null)
            : kind == "Material"
                ? (UnityEngine.Object)(reloaded.materials.Count > 0 ? reloaded.materials[0] : null)
                : (UnityEngine.Object)(reloaded.objects.Count > 0 ? reloaded.objects[0] : null);
        if (resolved == null)
        {
            result.stored_reference_verdict = "missing";
        }
        else if (FerriteGraphCommon.Identify(resolved) == before)
        {
            result.stored_reference_verdict = "same_object";
        }
        else if (resolved == external)
        {
            result.stored_reference_verdict = "resolved_to_the_external_asset";
            result.silently_retargeted = true;
        }
        else
        {
            result.stored_reference_verdict = "resolved_to_another_object";
            result.silently_retargeted = true;
        }
        ++FerriteGraphCommon.Checks;

        // ---- the transitions, each over the same asset path.
        result.mapping_survived_a_reexport = Survives(plan.reexport, assetPath, identifier);
        result.mapping_survived_a_designation_rename = Survives(plan.renamed, assetPath, identifier);
        result.map_key_after_the_rename = KeyAfter(assetPath, identifier);
        result.mapping_survived_removing_the_definition =
            Survives(plan.removed_tracked_definition, assetPath, identifier);
        result.remap_left_a_dangling_entry_after_removal =
            result.mapping_survived_removing_the_definition
            && !Names(assetPath, kind).Contains(result.identifier_name);
        ++FerriteGraphCommon.Checks;

        // ---- stale external content.
        File.Copy(plan.changed_material, Path.GetFullPath(assetPath), true);
        FerriteGraphCommon.Import(assetPath);
        if (kind == "Material")
        {
            Material reread =
                AssetDatabase.LoadAssetAtPath<Material>(AssetFolder + "/external-material.mat");
            result.external_colour_after_the_fbx_changed =
                reread == null ? "<absent>" : FerriteGraphCommon.Colour(reread);
            result.external_content_unchanged_after_the_fbx_changed =
                reread != null
                && result.external_colour_after_the_fbx_changed == result.external_colour_before;
        }
        else if (kind == "Mesh")
        {
            Mesh reread = AssetDatabase.LoadAssetAtPath<Mesh>(AssetFolder + "/external-mesh.asset");
            result.external_vertex_count_after_the_fbx_changed =
                reread == null ? -1 : reread.vertexCount;
            result.external_content_unchanged_after_the_fbx_changed =
                reread != null
                && result.external_vertex_count_after_the_fbx_changed
                    == result.external_vertex_count_before;
        }
        else
        {
            GameObject reread =
                AssetDatabase.LoadAssetAtPath<GameObject>(AssetFolder + "/external-object.prefab");
            result.external_content_unchanged_after_the_fbx_changed = reread != null;
        }
        // A project only *sees* stale content if the import used the external
        // asset. An accepted-but-ignored remap leaves an orphan copy, which is
        // a different and lesser problem, and the two are never merged.
        result.the_scene_shows_content_the_fbx_no_longer_has =
            result.the_import_honoured_it
            && result.the_fbx_changed_this_object
            && result.external_content_unchanged_after_the_fbx_changed;
        ++FerriteGraphCommon.Checks;

        AssetDatabase.DeleteAsset(referencePath);
        AssetDatabase.DeleteAsset(externalPath);
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        return result;
    }

    // ------------------------------------------------------------- plumbing

    private static bool Survives(
        string document, string assetPath, AssetImporter.SourceAssetIdentifier identifier)
    {
        File.Copy(document, Path.GetFullPath(assetPath), true);
        FerriteGraphCommon.Import(assetPath);
        FerriteGraphCommon.RecordSubassets(assetPath);
        ++FerriteGraphCommon.Checks;
        return InMap(AssetImporter.GetAtPath(assetPath), identifier);
    }

    private static string KeyAfter(string assetPath, AssetImporter.SourceAssetIdentifier identifier)
    {
        AssetImporter importer = AssetImporter.GetAtPath(assetPath);
        if (importer == null)
        {
            return "<no importer>";
        }
        foreach (KeyValuePair<AssetImporter.SourceAssetIdentifier, UnityEngine.Object> entry
            in importer.GetExternalObjectMap())
        {
            if (entry.Key.type == identifier.type && entry.Key.name == identifier.name)
            {
                return entry.Key.type.Name + ":" + entry.Key.name
                    + " -> " + (entry.Value == null ? "<null>" : entry.Value.name);
            }
        }
        return "<the key is no longer in the map>";
    }

    private static bool InMap(
        AssetImporter importer, AssetImporter.SourceAssetIdentifier identifier)
    {
        if (importer == null)
        {
            return false;
        }
        return importer.GetExternalObjectMap()
            .Any(entry => entry.Key.type == identifier.type && entry.Key.name == identifier.name);
    }

    private static Type TypeOf(string kind)
    {
        switch (kind)
        {
            case "Mesh": return typeof(Mesh);
            case "Material": return typeof(Material);
            default: return typeof(GameObject);
        }
    }

    private static string Wanted(Plan plan, string kind)
    {
        switch (kind)
        {
            case "Mesh": return plan.mesh_to_remap;
            case "Material": return plan.material_to_remap;
            default: return plan.game_object_to_remap;
        }
    }

    // The object the plan names, which is chosen so the transitions below
    // really change it. The imported root is a `GameObject` too, and it is
    // named after the asset file rather than after anything the FBX says, so
    // remapping it would measure the file name; it is excluded.
    private static UnityEngine.Object Pick(string assetPath, string kind, string wanted)
    {
        List<UnityEngine.Object> candidates = AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null && item.GetType().Name == kind)
            .Where(item => !(item is GameObject go) || go.transform.parent != null)
            .OrderBy(item => item.name, StringComparer.Ordinal)
            .ThenBy(FerriteGraphCommon.LocalId)
            .ToList();
        FerriteGraphCommon.Require(
            candidates.Any(item => item.name == wanted),
            "the control published no " + kind + " named " + wanted);
        return candidates.First(item => item.name == wanted);
    }

    private static int Count(string assetPath, string kind)
    {
        FerriteGraphCommon.RecordSubassets(assetPath);
        return AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Count(item => item != null && item.GetType().Name == kind);
    }

    private static List<string> Names(string assetPath, string kind)
    {
        return AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null && item.GetType().Name == kind)
            .Select(item => item.name)
            .Distinct()
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();
    }

    // How many placements the import gives a `MeshFilter` whose `sharedMesh`
    // some other placement also has. Reference equality, so a remap that
    // handed every placement its own copy is visible as this number falling.
    private static int SharedMeshPlacements(string assetPath)
    {
        GameObject root = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
        if (root == null)
        {
            return -1;
        }
        List<int> identifiers = root.GetComponentsInChildren<MeshFilter>(true)
            .Where(filter => filter.sharedMesh != null)
            .Select(filter => FerriteGraphCommon.InstanceKey(filter.sharedMesh))
            .ToList();
        return identifiers.GroupBy(id => id).Where(group => group.Count() > 1).Sum(group => group.Count());
    }

    private static string WhatTheSceneUses(
        string assetPath, string kind, UnityEngine.Object external)
    {
        GameObject root = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
        if (root == null)
        {
            return "<the asset published no GameObject>";
        }
        switch (kind)
        {
            case "Mesh":
                foreach (MeshFilter filter in root.GetComponentsInChildren<MeshFilter>(true))
                {
                    if (filter.sharedMesh == external)
                    {
                        return "external:" + external.name;
                    }
                }
                return "imported sub-assets only";
            case "Material":
                foreach (MeshRenderer renderer in root.GetComponentsInChildren<MeshRenderer>(true))
                {
                    if (renderer.sharedMaterials.Any(material => material == external))
                    {
                        return "external:" + external.name;
                    }
                }
                return "imported sub-assets only";
            default:
                // A remapped `GameObject` would have to appear in the imported
                // hierarchy as the external prefab, which is a different thing
                // from a `GameObject` merely named the same.
                foreach (Transform transform in root.GetComponentsInChildren<Transform>(true))
                {
                    if (PrefabUtility.GetCorrespondingObjectFromSource(transform.gameObject)
                        == external)
                    {
                        return "external:" + external.name;
                    }
                }
                return "imported sub-assets only";
        }
    }

    // Every `(type, visible name)` pair the control publishes more than once.
    // That pair is exactly what a `SourceAssetIdentifier` is, so this is the
    // measured answer to "can the key address one object".
    private static List<string> AmbiguousKeys(string assetPath)
    {
        return AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .GroupBy(item => item.GetType().Name + ":" + item.name, StringComparer.Ordinal)
            .Where(group => group.Count() > 1)
            .Select(group => group.Key + " x "
                + group.Count().ToString(CultureInfo.InvariantCulture))
            .OrderBy(item => item, StringComparer.Ordinal)
            .ToList();
    }
}
