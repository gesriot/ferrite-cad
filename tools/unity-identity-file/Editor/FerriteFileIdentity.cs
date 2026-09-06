// SPDX-License-Identifier: MIT
//
// The §22B-1e3b Unity measurement: what the real editor makes of the identity
// channel the shipped writer now puts in the file.
//
// # What it is asked, and what it is deliberately not asked
//
// Asked: can a vanilla `ModelImporter` read the two properties at all; is the
// join they offer unambiguous; are the designations a person reads still the
// designations they were; is the hierarchy the hierarchy it was; do the
// placements of one definition still share one `Mesh`; and does the import say
// anything it did not say before.
//
// Not asked: whether every serialised `fileID` survives. §22B-1e2a and
// §22B-1e2b measured what a `fileID` is in this editor — a function of the
// visible name, its type and a collision counter, or of the hierarchy path —
// and no property in a file changes that. This probe therefore *reports* how
// many identifiers moved between the two imports and asserts nothing about the
// number, because a measurement that demanded an impossible stability would
// have to be either wrong or silently weakened later.
//
// # Two files, one scene
//
// `fcad-measured.fbx` and `fcad-legacy.fbx` are the same scene written by the
// same writer, differing only in whether the document recorded identities.
// Comparing the two imports is what makes "nothing else moved" a measurement
// rather than a claim: the pair is the control.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using UnityEditor;
using UnityEngine;

public static class FerriteFileIdentity
{
    private const string AssetFolder = "Assets/Measured";
    private const string DefinitionProperty = "FerriteCADDefinitionId";
    private const string OccurrenceProperty = "FerriteCADOccurrenceId";
    private const string DefinitionKeyProperty = "FerriteCADDefinitionKey";
    private const string NodeKeyProperty = "FerriteCADNodeKey";

    // The two file names the pair is made of. Named rather than discovered, so
    // a run that was handed the wrong files refuses instead of measuring them.
    private const string CurrentFile = "fcad-measured.fbx";
    private const string LegacyFile = "fcad-legacy.fbx";
    private const string KeyedFile = "fcad-identity-escaping.fbx";
    // A neutral test variant: the same identities under different
    // designations. Not a STEP reimport, and not presented as one.
    private const string RenamedFile = "fcad-renamed.fbx";

    [Serializable]
    private sealed class ObjectReport
    {
        public string path = String.Empty;
        public string name = String.Empty;
        public string parent = String.Empty;
        public string definition_id = String.Empty;
        public string occurrence_id = String.Empty;
        public string definition_key = String.Empty;
        public string node_key = String.Empty;
        public string mesh = String.Empty;
        public int material_count;
        public string file_id = String.Empty;
    }

    [Serializable]
    private sealed class SubassetReport
    {
        public string kind = String.Empty;
        public string name = String.Empty;
        public string file_id = String.Empty;
    }

    [Serializable]
    private sealed class FileReport
    {
        public string file = String.Empty;
        // What this editor would have done to the hierarchy if the measurement
        // had left it alone. Recorded rather than assumed; see `Measure`.
        public bool sorted_by_name_by_default;
        // The designation the editor gives the model's own root, which in this
        // editor is the asset's file name and not anything the document said.
        public string root_designation = String.Empty;
        public int objects;
        public int meshes;
        public int materials;
        public int objects_with_definition_id;
        public int objects_with_occurrence_id;
        public int distinct_definition_ids;
        public int distinct_occurrence_ids;
        public int identifier_uniqueness_violations;
        public int import_errors;
        public List<ObjectReport> nodes = new List<ObjectReport>();
        public List<SubassetReport> subassets = new List<SubassetReport>();
    }

    [Serializable]
    private sealed class Comparison
    {
        public string against = String.Empty;
        public int objects_compared;
        // The root is excluded on purpose and reported instead: this editor
        // names an imported model's root after the asset file, so two files of
        // two names have two root designations however identical their contents
        // — a property of the editor, measured in §22B-1e2a, and not something
        // a property in the file changed.
        public int names_that_moved;
        public int roots_excluded_from_the_name_comparison;
        public int parents_that_moved;
        public int mesh_bindings_that_moved;
        public int material_counts_that_moved;
        public int existing_properties_that_moved;
        // Reported, never asserted. See the note at the top of this file.
        public int gameobject_file_ids_that_moved;
        public int mesh_file_ids_that_moved;
        public int material_file_ids_that_moved;
    }

    [Serializable]
    private sealed class Rename
    {
        public int objects_compared;
        public int joined_by_placement_identity;
        public int designations_that_changed;
        public int roots_excluded_from_the_name_comparison;
        // Reported, never asserted. See the note at the top of this file: this
        // is precisely the cost the channel does not remove.
        public int gameobject_file_ids_that_moved;
        // A Mesh and a Material are sub-assets and carry no custom properties
        // of their own — Unity hands properties to a callback about a
        // `GameObject` and to nothing else — so there is no identity on them to
        // join two imports by, whatever the file says. Counted rather than
        // joined, because the join does not exist.
        public int meshes_before;
        public int meshes_after;
        public int materials_before;
        public int materials_after;
    }

    [Serializable]
    private sealed class Report
    {
        public string unity_version = String.Empty;
        public int checks;
        public List<FileReport> files = new List<FileReport>();
        public Comparison against_legacy = new Comparison();
        public Rename across_a_rename = new Rename();
        public List<string> findings = new List<string>();
    }

    private static int checks;
    private static readonly List<string> Findings = new List<string>();

    public static void Run()
    {
        try
        {
            Execute();
        }
        catch (Exception error)
        {
            Debug.LogError("FCAD_FILE_IDENTITY_FAILURE " + error);
            EditorApplication.Exit(1);
        }
    }

    private static void Execute()
    {
        checks = 0;
        Findings.Clear();

        string source = ArgumentValue("-fcadSource")
            ?? throw new InvalidOperationException("no -fcadSource was given");
        string output = ArgumentValue("-fcadOutput")
            ?? throw new InvalidOperationException("no -fcadOutput was given");
        string expected = ArgumentValue("-fcadExpected");

        if (!AssetDatabase.IsValidFolder(AssetFolder))
        {
            AssetDatabase.CreateFolder("Assets", "Measured");
        }

        Report report = new Report { unity_version = Application.unityVersion };

        FileReport current = Measure(source, CurrentFile);
        FileReport legacy = Measure(source, LegacyFile);
        FileReport keyed = Measure(source, KeyedFile);
        FileReport renamed = Measure(source, RenamedFile);
        report.files.Add(current);
        report.files.Add(legacy);
        report.files.Add(keyed);
        report.files.Add(renamed);

        // 1. A vanilla ModelImporter reads both properties, on every object of
        //    the current file, frames and omitted parts included.
        Require(current.objects > 0, "the current file imported no object at all");
        Require(
            current.objects_with_definition_id == current.objects,
            "an object of the current file carries no definition identity");
        Require(
            current.objects_with_occurrence_id == current.objects,
            "an object of the current file carries no placement identity");

        // 2. The join is unambiguous: one placement identity per object, and
        //    the definition identity of two placements of one part is one
        //    value rather than two.
        Require(
            current.distinct_occurrence_ids == current.objects,
            "two objects of the current file answer to one placement identity");
        Require(
            current.distinct_definition_ids < current.objects,
            "no definition of this file is placed twice, so the join measures nothing");
        Require(
            AllValues(current, OccurrenceProperty)
                .Intersect(AllValues(current, DefinitionProperty))
                .Count() == 0,
            "a definition identity and a placement identity are the same value");
        foreach (ObjectReport node in current.nodes)
        {
            Require(
                node.definition_id.StartsWith("fcad1:def:", StringComparison.Ordinal)
                    && node.occurrence_id.StartsWith("fcad1:occ:", StringComparison.Ordinal),
                "a value the editor handed back is not in the domain it was written in: "
                    + node.path);
        }
        foreach (ObjectReport node in legacy.nodes)
        {
            Require(
                node.definition_id.Length == 0 && node.occurrence_id.Length == 0,
                "a legacy object carries an identity value: " + node.path);
        }

        // 3. The placements that share a definition identity share one Mesh,
        //    and the ones that do not, do not.
        foreach (IGrouping<string, ObjectReport> group in current.nodes
            .Where(node => node.mesh.Length > 0)
            .GroupBy(node => node.definition_id))
        {
            ++checks;
            if (group.Select(node => node.mesh).Distinct().Count() != 1)
            {
                Fail("placements of one definition identity do not share one mesh: " + group.Key);
            }
        }

        // 4. A layout that recorded no identity carries no property, and says
        //    so by their absence and by nothing else.
        Require(legacy.objects == current.objects, "the pair is not the same scene");
        Require(
            legacy.objects_with_definition_id == 0,
            "the legacy file carries a definition identity its document never recorded");
        Require(
            legacy.objects_with_occurrence_id == 0,
            "the legacy file carries a placement identity its document never recorded");

        // 5. Everything a person sees is what it was. This is the whole of the
        //    "nothing else moved" claim, and it is a comparison rather than an
        //    assertion about one import.
        report.against_legacy = Compare(current, legacy);
        Require(
            report.against_legacy.objects_compared == current.objects,
            "the two imports could not be lined up object for object");
        Require(report.against_legacy.names_that_moved == 0, "a designation changed");
        Require(report.against_legacy.parents_that_moved == 0, "the hierarchy changed");
        Require(
            report.against_legacy.mesh_bindings_that_moved == 0,
            "an object's geometry changed");
        Require(
            report.against_legacy.material_counts_that_moved == 0,
            "an object's material binding changed");
        Require(
            report.against_legacy.existing_properties_that_moved == 0,
            "a property §22B-1b2 already wrote changed under the new one");
        Require(current.meshes == legacy.meshes, "the number of meshes changed");
        Require(current.materials == legacy.materials, "the number of materials changed");

        // 6. And the import says nothing it did not say before. The existing
        //    `Identifier uniqueness violation` is §22B-1e1's finding and is not
        //    fixed here; what must not happen is that the channel adds one.
        Require(
            current.identifier_uniqueness_violations == legacy.identifier_uniqueness_violations,
            "the identity channel changed how many identifier collisions the editor reports");
        Require(current.import_errors == 0, "the current file imported with errors");
        Require(legacy.import_errors == 0, "the legacy file imported with errors");

        // 7. The escaped keys survive the editor as well as the reader: a value
        //    that came back with a different number of separators would be one
        //    the importer had rewritten.
        Require(keyed.objects_with_definition_id == keyed.objects, "a keyed object lost its value");
        foreach (ObjectReport node in keyed.nodes)
        {
            ++checks;
            if (node.definition_id.Split(':').Length != 5)
            {
                Fail("an escaped identity value did not survive the editor: " + node.definition_id);
            }
        }

        // 8. And the one thing the channel is for, measured on a neutral test
        //    variant: after every designation has changed, the placement
        //    identity still names the same placement. What moves beside it is
        //    counted and not judged — that is the cost §22B-1e2a measured and
        //    this slice does not remove.
        report.across_a_rename = CompareAcrossRename(current, renamed);
        Require(
            report.across_a_rename.joined_by_placement_identity == current.objects,
            "a placement could not be found again by its identity after a rename");
        Require(
            report.across_a_rename.designations_that_changed
                + report.across_a_rename.roots_excluded_from_the_name_comparison
                == current.objects,
            "no designation actually changed, so the rename variant measures nothing");

        Require(checks > 40, "the probe performed too few checks");
        report.checks = checks;
        report.findings.AddRange(Findings);

        string json = JsonUtility.ToJson(report, true) + "\n";
        string directory = Path.GetDirectoryName(output);
        if (!String.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }
        File.WriteAllText(output, json, new UTF8Encoding(false));

        if (!String.IsNullOrEmpty(expected))
        {
            Require(File.Exists(expected), "the committed expected file report is missing");
            string committed = File.ReadAllText(expected).Replace("\r\n", "\n");
            Require(committed == json, "the file report differs from the committed measurement");
        }

        if (Findings.Count > 0)
        {
            foreach (string finding in Findings)
            {
                Debug.LogError("FCAD_FILE_IDENTITY_FAILURE " + finding);
            }
            EditorApplication.Exit(1);
            return;
        }

        Debug.Log("FCAD_FILE_IDENTITY_EXECUTED checks="
            + report.checks.ToString(CultureInfo.InvariantCulture));
        EditorApplication.Exit(0);
    }

    // ----------------------------------------------------------- one import

    private static FileReport Measure(string source, string name)
    {
        string from = Path.Combine(source, name);
        Require(File.Exists(from), "the production writer left no " + name);
        string assetPath = AssetFolder + "/" + name;
        File.Copy(from, Path.GetFullPath(assetPath), true);

        // Imported once first, because an asset has no importer until it has
        // been imported, and then again with the hierarchy sort turned off.
        //
        // This editor sorts an imported hierarchy by name by default, which
        // reorders it relative to the file. That is a measured property of the
        // editor and it is recorded below rather than hidden — but it is also
        // why the sort has to be off here: the callback that hands over the
        // custom properties sees the tree *before* the sort and the finished
        // asset is the tree *after* it, and a measurement whose two halves are
        // two different orderings of one import is measuring its own bookkeeping.
        AssetDatabase.ImportAsset(
            assetPath,
            ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        ModelImporter importer = AssetImporter.GetAtPath(assetPath) as ModelImporter;
        Require(importer != null, name + " got no ModelImporter");
        bool sortedByDefault = importer != null && importer.sortHierarchyByName;
        if (importer != null && importer.sortHierarchyByName)
        {
            importer.sortHierarchyByName = false;
            importer.SaveAndReimport();
        }

        int violations = 0;
        int errors = 0;
        Application.LogCallback capture = (condition, stack, type) =>
        {
            if (condition.Contains("Identifier uniqueness violation"))
            {
                ++violations;
            }
            if (type == LogType.Error || type == LogType.Exception)
            {
                // The probe's own refusals are not the importer's.
                if (!condition.StartsWith("FCAD_", StringComparison.Ordinal))
                {
                    ++errors;
                }
            }
        };
        Application.logMessageReceived += capture;
        try
        {
            AssetDatabase.ImportAsset(
                assetPath,
                ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        }
        finally
        {
            Application.logMessageReceived -= capture;
        }

        FileReport report = new FileReport
        {
            file = name,
            sorted_by_name_by_default = sortedByDefault,
            identifier_uniqueness_violations = violations,
            import_errors = errors,
        };

        Dictionary<string, Dictionary<string, string>> properties =
            ReadProperties(FerriteFileProperties.CachePath(assetPath));

        GameObject root = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
        Require(root != null, name + " did not import as a model");
        Walk(root, "0", String.Empty, properties, report);
        // The imported asset carries a root of its own, named after the file
        // and belonging to no placement. A FerriteCAD placement is exactly an
        // object the writer gave a node key, so that is what the rest of this
        // measurement is about — rather than an assumption about how many roots
        // this editor invents.
        report.nodes.RemoveAll(node => node.node_key.Length == 0);

        foreach (UnityEngine.Object item in AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .OrderBy(item => item.GetType().Name, StringComparer.Ordinal)
            .ThenBy(item => item.name, StringComparer.Ordinal))
        {
            if (item is Mesh)
            {
                ++report.meshes;
            }
            else if (item is Material)
            {
                ++report.materials;
            }
            else if (!(item is GameObject))
            {
                continue;
            }
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(item, out string _, out long local);
            report.subassets.Add(new SubassetReport
            {
                kind = item is Mesh ? "Mesh" : (item is Material ? "Material" : "GameObject"),
                name = item.name,
                file_id = unchecked((ulong)local).ToString(CultureInfo.InvariantCulture),
            });
        }

        report.objects = report.nodes.Count;
        report.root_designation = report.nodes.Count > 0 ? report.nodes[0].name : String.Empty;
        report.objects_with_definition_id =
            report.nodes.Count(node => node.definition_id.Length > 0);
        report.objects_with_occurrence_id =
            report.nodes.Count(node => node.occurrence_id.Length > 0);
        report.distinct_definition_ids = report.nodes
            .Select(node => node.definition_id)
            .Where(value => value.Length > 0)
            .Distinct(StringComparer.Ordinal)
            .Count();
        report.distinct_occurrence_ids = report.nodes
            .Select(node => node.occurrence_id)
            .Where(value => value.Length > 0)
            .Distinct(StringComparer.Ordinal)
            .Count();
        return report;
    }

    private static void Walk(
        GameObject target,
        string path,
        string parent,
        Dictionary<string, Dictionary<string, string>> properties,
        FileReport report)
    {
        report.nodes.Add(Describe(target, path, parent, properties));
        int index = 0;
        foreach (Transform child in Children(target))
        {
            Walk(
                child.gameObject,
                path + "/" + index.ToString(CultureInfo.InvariantCulture),
                path,
                properties,
                report);
            ++index;
        }
    }

    private static IEnumerable<Transform> Children(GameObject target)
    {
        Transform transform = target.transform;
        for (int index = 0; index < transform.childCount; ++index)
        {
            yield return transform.GetChild(index);
        }
    }

    private static ObjectReport Describe(
        GameObject target,
        string path,
        string parent,
        Dictionary<string, Dictionary<string, string>> properties)
    {
        Dictionary<string, string> values;
        if (!properties.TryGetValue(path, out values))
        {
            values = new Dictionary<string, string>();
        }
        MeshFilter filter = target.GetComponent<MeshFilter>();
        MeshRenderer renderer = target.GetComponent<MeshRenderer>();
        AssetDatabase.TryGetGUIDAndLocalFileIdentifier(target, out string _, out long local);
        return new ObjectReport
        {
            path = path,
            name = target.name,
            parent = parent,
            definition_id = Value(values, DefinitionProperty),
            occurrence_id = Value(values, OccurrenceProperty),
            definition_key = Value(values, DefinitionKeyProperty),
            node_key = Value(values, NodeKeyProperty),
            // The mesh by name plus its identifier, so two objects share a mesh
            // here exactly when they share it in the editor.
            mesh = filter != null && filter.sharedMesh != null
                ? filter.sharedMesh.name + "#"
                    + filter.sharedMesh.GetInstanceID().ToString(CultureInfo.InvariantCulture)
                : String.Empty,
            material_count = renderer != null ? renderer.sharedMaterials.Length : 0,
            file_id = unchecked((ulong)local).ToString(CultureInfo.InvariantCulture),
        };
    }

    // -------------------------------------------------------- the comparison

    private static Comparison Compare(FileReport current, FileReport legacy)
    {
        Comparison comparison = new Comparison { against = legacy.file };
        Dictionary<string, ObjectReport> by_path = legacy.nodes
            .ToDictionary(node => node.path, StringComparer.Ordinal);
        foreach (ObjectReport node in current.nodes)
        {
            ObjectReport other;
            if (!by_path.TryGetValue(node.path, out other))
            {
                continue;
            }
            ++comparison.objects_compared;
            ++checks;
            if (node.parent.Length == 0)
            {
                ++comparison.roots_excluded_from_the_name_comparison;
            }
            else if (node.name != other.name)
            {
                ++comparison.names_that_moved;
            }
            if (node.parent != other.parent)
            {
                ++comparison.parents_that_moved;
            }
            // Compared by name rather than by instance identifier: two imports
            // are two sets of objects, and their instance identifiers are
            // different by construction. Sharing *within* one import is what
            // the instance identifier above answers.
            if (MeshName(node.mesh) != MeshName(other.mesh))
            {
                ++comparison.mesh_bindings_that_moved;
            }
            if (node.material_count != other.material_count)
            {
                ++comparison.material_counts_that_moved;
            }
            if (node.definition_key != other.definition_key || node.node_key != other.node_key)
            {
                ++comparison.existing_properties_that_moved;
            }
            if (node.file_id != other.file_id)
            {
                ++comparison.gameobject_file_ids_that_moved;
            }
        }

        comparison.mesh_file_ids_that_moved = MovedIdentifiers(current, legacy, "Mesh");
        comparison.material_file_ids_that_moved = MovedIdentifiers(current, legacy, "Material");
        return comparison;
    }

    // The join a project would make after a rename: by identity, never by name.
    private static Rename CompareAcrossRename(FileReport current, FileReport renamed)
    {
        Rename across = new Rename();
        Dictionary<string, ObjectReport> by_identity = new Dictionary<string, ObjectReport>(
            StringComparer.Ordinal);
        foreach (ObjectReport node in renamed.nodes)
        {
            if (node.occurrence_id.Length > 0)
            {
                by_identity[node.occurrence_id] = node;
            }
        }
        foreach (ObjectReport node in current.nodes)
        {
            ++across.objects_compared;
            ++checks;
            ObjectReport other;
            if (!by_identity.TryGetValue(node.occurrence_id, out other))
            {
                continue;
            }
            ++across.joined_by_placement_identity;
            if (node.parent.Length == 0)
            {
                ++across.roots_excluded_from_the_name_comparison;
            }
            else if (node.name != other.name)
            {
                ++across.designations_that_changed;
            }
            if (node.file_id != other.file_id)
            {
                ++across.gameobject_file_ids_that_moved;
            }
        }
        across.meshes_before = current.meshes;
        across.meshes_after = renamed.meshes;
        across.materials_before = current.materials;
        across.materials_after = renamed.materials;
        return across;
    }

    private static int MovedIdentifiers(FileReport current, FileReport legacy, string kind)
    {
        Dictionary<string, string> before = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (SubassetReport item in legacy.subassets.Where(item => item.kind == kind))
        {
            before[item.name] = item.file_id;
        }
        int moved = 0;
        foreach (SubassetReport item in current.subassets.Where(item => item.kind == kind))
        {
            string was;
            if (before.TryGetValue(item.name, out was) && was != item.file_id)
            {
                ++moved;
            }
        }
        return moved;
    }

    private static string MeshName(string mesh)
    {
        int at = mesh.LastIndexOf('#');
        return at < 0 ? mesh : mesh.Substring(0, at);
    }

    private static IEnumerable<string> AllValues(FileReport report, string property)
    {
        return report.nodes
            .Select(node => property == DefinitionProperty ? node.definition_id : node.occurrence_id)
            .Where(value => value.Length > 0);
    }

    // ------------------------------------------------------------- plumbing

    private static Dictionary<string, Dictionary<string, string>> ReadProperties(string cache)
    {
        Dictionary<string, Dictionary<string, string>> properties =
            new Dictionary<string, Dictionary<string, string>>(StringComparer.Ordinal);
        if (!File.Exists(cache))
        {
            return properties;
        }
        foreach (string line in File.ReadAllLines(cache))
        {
            string[] fields = line.Split('\t');
            if (fields.Length < 3)
            {
                continue;
            }
            Dictionary<string, string> values;
            if (!properties.TryGetValue(fields[0], out values))
            {
                values = new Dictionary<string, string>(StringComparer.Ordinal);
                properties[fields[0]] = values;
            }
            values[fields[1]] = String.Join("\t", fields.Skip(2).ToArray());
        }
        return properties;
    }

    private static string Value(Dictionary<string, string> values, string name)
    {
        string found;
        return values.TryGetValue(name, out found) ? found : String.Empty;
    }

    private static void Require(bool condition, string what)
    {
        ++checks;
        if (!condition)
        {
            Fail(what);
        }
    }

    private static void Fail(string what)
    {
        Findings.Add(what);
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
}
