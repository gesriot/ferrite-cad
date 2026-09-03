// SPDX-License-Identifier: MIT
//
// The §22B-1c Unity gate: the real editor imports the complex assembly the
// shipped `export-fbx` command produced.
//
// The two probes beside this one measure one small scene in every detail — its
// transforms, its normals, its material colours, its custom properties. This
// one asks a different question, and only that question: given a real STEP
// assembly of a hundred and forty placements, thirty-four distinct parts and a
// definition that has no triangles, does Unity 6000.4.10f1 rebuild the model
// FerriteCAD described, or does it flatten, merge or drop part of it?
//
// So there are no expected transforms here and no expected colours. What is
// measured is structure: how many nodes came back, how many distinct meshes
// they share, how many triangles those hold, and how many nodes arrived
// carrying no geometry at all.
//
// # Why identity is not asked here
//
// Unity's import-time custom-property callback reports a GameObject *name*,
// and a name is not identity: this assembly gives several definitions the same
// designation, exactly as the source recorded them. Asking Unity which node is
// `#2428` would therefore be asking a question its own data cannot answer.
// That question belongs to pinned ufbx, which reads the object numbers the
// writer derived from position and never from a name, and to the Rust gate,
// which compares the file with the `ExportScene` it came from. What is left
// for the editor is what only the editor can say: whether it rebuilds this
// model or quietly reduces it.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;

internal static class FerriteFbxComplex
{
    private const string AssetPath = "Assets/Complex/fcad-complex.fbx";

    // What the Rust gate and pinned ufbx already measured about the same
    // bytes. Repeated here so a disagreement is a failure rather than a number
    // in a log nobody compares.
    private const int Nodes = 140;
    private const int Geometries = 34;
    private const int Triangles = 987203;
    // What the editor actually keeps of those, measured rather than assumed.
    // Unity's FBX importer discards degenerate polygons, and a tessellation of
    // this size contains a few: thirty-seven of nine hundred and eighty-seven
    // thousand, which is under four thousandths of one per cent. Pinned exactly
    // so that a change in either direction is a failure, and bounded below as
    // well, so a future import that quietly lost a whole part cannot pass by
    // being called "some rounding".
    private const int UnityTriangles = 987166;

    [Serializable]
    private sealed class Report
    {
        public string schema = "ferritecad.unity-complex-fbx.v1";
        public string unity_version = String.Empty;
        public List<LogReport> importer_messages = new List<LogReport>();
        public int transform_count;
        public int node_count;
        public int mesh_filter_count;
        public int unique_mesh_asset_count;
        public int triangle_count;
        public int nodes_without_mesh;
        public int checks;
    }

    [Serializable]
    private sealed class LogReport
    {
        public string severity = String.Empty;
        public string message = String.Empty;
    }

    private static int checks;

    public static void Run()
    {
        try
        {
            Execute();
        }
        catch (Exception error)
        {
            Debug.LogError("FCAD_FBX_COMPLEX_FAILURE " + error);
            EditorApplication.Exit(1);
        }
    }

    private static void Execute()
    {
        checks = 0;
        string output = ArgumentValue("-fcadOutput")
            ?? "measurement-output/unity-complex-report.json";

        Require(File.Exists(AssetPath), "the complex FBX was not placed in the project");

        Report report = new Report { unity_version = Application.unityVersion };
        report.importer_messages = ImportSynchronously(AssetPath);
        foreach (LogReport message in report.importer_messages)
        {
            Require(message.severity != "error" && message.severity != "exception",
                "the importer reported " + message.severity + ": " + message.message);
        }

        GameObject prefab = AssetDatabase.LoadAssetAtPath<GameObject>(AssetPath);
        Require(prefab != null, "Unity published no GameObject for the complex assembly");

        GameObject instance = UnityEngine.Object.Instantiate(prefab);
        instance.hideFlags = HideFlags.HideAndDontSave;
        try
        {
            Measure(report, instance);
        }
        finally
        {
            UnityEngine.Object.DestroyImmediate(instance);
        }

        Require(checks > 20, "the probe performed too few checks");
        report.checks = checks;
        string json = JsonUtility.ToJson(report, true) + "\n";

        string directory = Path.GetDirectoryName(output);
        if (!String.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }
        File.WriteAllText(output, json, new System.Text.UTF8Encoding(false));

        Debug.Log("FCAD_FBX_COMPLEX_EXECUTED checks="
            + report.checks.ToString(CultureInfo.InvariantCulture));
        EditorApplication.Exit(0);
    }

    private static void Measure(Report report, GameObject instance)
    {
        Transform[] transforms = instance.GetComponentsInChildren<Transform>(true);
        report.transform_count = transforms.Length;
        // One transform per Model in the file, and no more. A file with a
        // single root gets no extra wrapper: Unity renames that root after the
        // asset and hangs the rest below it. Measured on the §22B-1b2 scene,
        // whose nine models come back as exactly nine transforms.
        report.node_count = transforms.Length;
        Require(report.node_count == Nodes,
            "Unity rebuilt " + report.node_count.ToString(CultureInfo.InvariantCulture)
                + " nodes rather than " + Nodes.ToString(CultureInfo.InvariantCulture));

        MeshFilter[] filters = instance.GetComponentsInChildren<MeshFilter>(true);
        report.mesh_filter_count = filters.Length;
        Mesh[] unique = filters
            .Select(filter => filter.sharedMesh)
            .Where(mesh => mesh != null)
            .Distinct()
            .ToArray();
        report.unique_mesh_asset_count = unique.Length;
        Require(unique.Length == Geometries,
            "Unity holds " + unique.Length.ToString(CultureInfo.InvariantCulture)
                + " distinct meshes rather than " + Geometries.ToString(CultureInfo.InvariantCulture));
        Require(filters.Length > unique.Length,
            "no part is placed more than once, so geometry sharing did not survive the import");

        int triangles = 0;
        foreach (Mesh mesh in unique)
        {
            int indices = 0;
            for (int slot = 0; slot < mesh.subMeshCount; ++slot)
            {
                indices += (int)mesh.GetIndexCount(slot);
            }
            Require(indices % 3 == 0, "an imported mesh does not hold whole triangles");
            triangles += indices / 3;
        }
        report.triangle_count = triangles;
        Require(triangles <= Triangles,
            "Unity holds " + triangles.ToString(CultureInfo.InvariantCulture)
                + " triangles, more than the "
                + Triangles.ToString(CultureInfo.InvariantCulture)
                + " the file contains, so the import invented geometry");
        Require(triangles * 1000L >= Triangles * 999L,
            "Unity kept only " + triangles.ToString(CultureInfo.InvariantCulture)
                + " of " + Triangles.ToString(CultureInfo.InvariantCulture)
                + " triangles, which is a lost part rather than dropped degenerates");
        Require(triangles == UnityTriangles,
            "Unity rebuilt " + triangles.ToString(CultureInfo.InvariantCulture)
                + " triangles rather than the measured "
                + UnityTriangles.ToString(CultureInfo.InvariantCulture));

        // Every node that came back with no geometry: the assembly frames that
        // never had any, and the definition this build could not tessellate.
        // Which of them is which is a question about identity and is answered
        // by ufbx; what matters here is that they arrived as nodes at all
        // rather than being dropped, and that they carry no triangles nobody
        // computed.
        foreach (Transform transform in transforms)
        {
            MeshFilter filter = transform.GetComponent<MeshFilter>();
            if (filter == null || filter.sharedMesh == null)
            {
                ++report.nodes_without_mesh;
            }
        }
        Require(report.nodes_without_mesh > 0,
            "every node came back with geometry, so the frames and the omitted part were lost");
        Require(report.nodes_without_mesh + report.mesh_filter_count == Nodes,
            "the nodes with and without geometry do not account for every node");
    }

    private static List<LogReport> ImportSynchronously(string assetPath)
    {
        List<LogReport> messages = new List<LogReport>();
        Application.LogCallback capture = (message, stack, kind) =>
        {
            if (kind == LogType.Warning || kind == LogType.Error || kind == LogType.Exception)
            {
                messages.Add(new LogReport
                {
                    severity = kind.ToString().ToLowerInvariant(),
                    message = CanonicalMessage(message),
                });
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
            .OrderBy(item => item.severity, StringComparer.Ordinal)
            .ThenBy(item => item.message, StringComparer.Ordinal)
            .ToList();
    }

    private static string CanonicalMessage(string message)
    {
        string project = Directory.GetCurrentDirectory().Replace('\\', '/');
        return message.Replace('\\', '/').Replace(project, "<project>").Replace("\r", String.Empty).Trim();
    }

    private static string ArgumentValue(string name)
    {
        string[] args = Environment.GetCommandLineArgs();
        for (int index = 0; index + 1 < args.Length; ++index)
        {
            if (args[index] == name)
            {
                return args[index + 1];
            }
        }
        return null;
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
