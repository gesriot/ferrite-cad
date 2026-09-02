// SPDX-License-Identifier: MIT
//
// The §22B-1b2 Unity gate: the real editor imports what the production FBX
// writer produced.
//
// The §22B-1a probe beside this one measures committed fixtures a Python
// generator wrote, which settled the contract. This one never opens those: it
// is pointed at a temporary asset the Rust writer produced from the measured
// scene, and asks Unity 6000.4.10f1 whether the contract survived a real
// exporter.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;

internal static class FerriteFbxProduction
{
    private const string AssetPath = "Assets/Production/fcad-production.fbx";
    private const string ExpectedReport = "Assets/Expected/unity-production-report.json";
    private const float Epsilon = 0.00001f;

    // What FerriteCAD stored, in the linear RGB it stores colours in. Unity
    // must give these back through `Color.linear` after the writer's single
    // sRGB encoding and the importer's decoding.
    private static readonly float[] SourceRed = { 0.603827f, 0.033105f, 0.010023f };
    private static readonly float[] SourceBlue = { 0.010023f, 0.100482f, 0.787412f };
    private static readonly float[] SourceOverride = { 0.216f, 0.523f, 0.052f };

    [Serializable]
    private sealed class Report
    {
        public string schema = "ferritecad.unity-production-fbx.v1";
        public string unity_version = String.Empty;
        public string colour_space = String.Empty;
        public ImporterReport importer = new ImporterReport();
        public List<LogReport> importer_messages = new List<LogReport>();
        public List<NodeReport> tree = new List<NodeReport>();
        public List<ControlPointReport> control_points = new List<ControlPointReport>();
        public int mesh_filter_count;
        public int unique_mesh_asset_count;
        public bool repeated_parts_share_mesh;
        public List<MeshReport> meshes = new List<MeshReport>();
        public List<MaterialReport> materials = new List<MaterialReport>();
        public List<UserPropertyReport> user_properties = new List<UserPropertyReport>();
        public float placement_separation;
        public float frame_to_first_placement;
        public int checks;
    }

    [Serializable]
    private sealed class ImporterReport
    {
        public float file_scale;
        public bool use_file_scale;
        public float global_scale;
        public bool bake_axis_conversion;
    }

    [Serializable]
    private sealed class LogReport
    {
        public string severity = String.Empty;
        public string message = String.Empty;
    }

    [Serializable]
    private sealed class NodeReport
    {
        public string path = String.Empty;
        public string name = String.Empty;
        public float[] local_position = Array.Empty<float>();
        public float[] local_rotation = Array.Empty<float>();
        public float[] local_scale = Array.Empty<float>();
        public float[] world_matrix = Array.Empty<float>();
        public bool has_mesh;
    }

    [Serializable]
    private sealed class ControlPointReport
    {
        public string name = String.Empty;
        public float[] world_position = Array.Empty<float>();
        public float distance_from_origin;
    }

    [Serializable]
    private sealed class MeshReport
    {
        public string asset_name = String.Empty;
        public int vertex_count;
        public int index_count;
        public int submesh_count;
        public float[] vertices = Array.Empty<float>();
        public float[] normals = Array.Empty<float>();
        public List<SubmeshReport> submeshes = new List<SubmeshReport>();
        public List<TriangleReport> triangle_orientation = new List<TriangleReport>();
    }

    [Serializable]
    private sealed class SubmeshReport
    {
        public int slot;
        public int[] indices = Array.Empty<int>();
    }

    [Serializable]
    private sealed class TriangleReport
    {
        public int slot;
        public int triangle;
        public float[] geometric_normal = Array.Empty<float>();
        public float[] average_imported_normal = Array.Empty<float>();
        public float normal_dot;
    }

    [Serializable]
    private sealed class MaterialReport
    {
        public string node_path = String.Empty;
        public int slot;
        public string material_name = String.Empty;
        public float[] base_colour = Array.Empty<float>();
        public float[] base_colour_linear = Array.Empty<float>();
    }

    [Serializable]
    private sealed class UserPropertyReport
    {
        public string node_name = String.Empty;
        public string property = String.Empty;
        public string value = String.Empty;
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
            Debug.LogError("FCAD_FBX_PRODUCTION_FAILURE " + error);
            EditorApplication.Exit(1);
        }
    }

    private static void Execute()
    {
        checks = 0;
        string output = ArgumentValue("-fcadOutput")
            ?? "measurement-output/unity-production-report.json";
        bool record = HasArgument("-fcadRecord");

        Require(File.Exists(AssetPath), "the production FBX was not placed in the project");

        Report report = new Report
        {
            unity_version = Application.unityVersion,
            colour_space = QualitySettings.activeColorSpace.ToString().ToLowerInvariant(),
        };

        report.importer_messages = ImportSynchronously(AssetPath);
        ModelImporter importer = AssetImporter.GetAtPath(AssetPath) as ModelImporter;
        Require(importer != null, "Unity gave the production asset no ModelImporter");
        report.importer = new ImporterReport
        {
            file_scale = Q(importer.fileScale),
            use_file_scale = importer.useFileScale,
            global_scale = Q(importer.globalScale),
            bake_axis_conversion = importer.bakeAxisConversion,
        };
        GameObject prefab = AssetDatabase.LoadAssetAtPath<GameObject>(AssetPath);
        Require(prefab != null, "Unity published no GameObject for the production asset");

        GameObject instance = UnityEngine.Object.Instantiate(prefab);
        instance.hideFlags = HideFlags.HideAndDontSave;
        try
        {
            Measure(report, instance);
            report.user_properties = ReadUserProperties(AssetPath);
            Validate(report, instance);
        }
        finally
        {
            UnityEngine.Object.DestroyImmediate(instance);
        }

        Require(checks > 60, "the probe performed too few checks");
        report.checks = checks;
        string json = JsonUtility.ToJson(report, true) + "\n";

        string directory = Path.GetDirectoryName(output);
        if (!String.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }
        File.WriteAllText(output, json, new System.Text.UTF8Encoding(false));

        if (!record)
        {
            Require(File.Exists(ExpectedReport), "the committed expected production report is missing");
            string expected = File.ReadAllText(ExpectedReport).Replace("\r\n", "\n");
            Require(expected == json, "the production import report differs from the committed contract");
        }

        Debug.Log("FCAD_FBX_PRODUCTION_EXECUTED checks=" + report.checks.ToString(CultureInfo.InvariantCulture));
        EditorApplication.Exit(0);
    }

    private static void Measure(Report report, GameObject instance)
    {
        Transform[] transforms = instance.GetComponentsInChildren<Transform>(true);
        foreach (Transform transform in transforms)
        {
            report.tree.Add(new NodeReport
            {
                path = StablePath(transform, instance.transform),
                name = transform.name,
                local_position = V(transform.localPosition),
                local_rotation = Q4(transform.localRotation),
                local_scale = V(transform.localScale),
                world_matrix = M(transform.localToWorldMatrix),
                has_mesh = transform.GetComponent<MeshFilter>() != null,
            });
        }

        MeshFilter[] filters = instance.GetComponentsInChildren<MeshFilter>(true);
        report.mesh_filter_count = filters.Length;
        Mesh[] unique = filters.Select(filter => filter.sharedMesh).Distinct().ToArray();
        report.unique_mesh_asset_count = unique.Length;
        Transform[] repeated = transforms
            .Where(transform => transform.name.StartsWith("Repeated Part", StringComparison.Ordinal))
            .ToArray();
        report.repeated_parts_share_mesh = repeated.Length == 2
            && repeated[0].GetComponent<MeshFilter>() != null
            && repeated[1].GetComponent<MeshFilter>() != null
            && ReferenceEquals(
                repeated[0].GetComponent<MeshFilter>().sharedMesh,
                repeated[1].GetComponent<MeshFilter>().sharedMesh);
        foreach (Mesh mesh in unique.OrderBy(mesh => mesh.name, StringComparer.Ordinal))
        {
            report.meshes.Add(MeasureMesh(mesh));
        }

        foreach (MeshRenderer renderer in instance.GetComponentsInChildren<MeshRenderer>(true))
        {
            Material[] materials = renderer.sharedMaterials;
            for (int slot = 0; slot < materials.Length; ++slot)
            {
                Color colour = BaseColour(materials[slot]);
                report.materials.Add(new MaterialReport
                {
                    node_path = StablePath(renderer.transform, instance.transform),
                    slot = slot,
                    material_name = materials[slot] == null ? "<null>" : materials[slot].name,
                    base_colour = C(colour),
                    base_colour_linear = C(colour.linear),
                });
            }
        }
        report.materials = report.materials
            .OrderBy(item => item.node_path, StringComparer.Ordinal)
            .ThenBy(item => item.slot)
            .ToList();

        Transform origin = transforms.Single(transform => transform.name == "CP Origin");
        foreach (string name in new[] { "CP Origin", "CP X1000", "CP Y2000", "CP Z3000" })
        {
            Transform point = transforms.Single(transform => transform.name == name);
            report.control_points.Add(new ControlPointReport
            {
                name = name,
                world_position = V(point.position),
                distance_from_origin = Q(Vector3.Distance(origin.position, point.position)),
            });
        }

        // Two independent world-distance measurements. A rigid transform keeps
        // distances, so the metres between two placements and between a frame
        // and a placement are computable from the FerriteCAD millimetres alone.
        report.placement_separation = Q(Vector3.Distance(repeated[0].position, repeated[1].position));
        Transform frame = transforms.Single(transform => transform.name == "Assembly Frame");
        report.frame_to_first_placement = Q(Vector3.Distance(frame.position, repeated[0].position));
    }

    private static MeshReport MeasureMesh(Mesh mesh)
    {
        Vector3[] vertices = mesh.vertices;
        Vector3[] normals = mesh.normals;
        MeshReport report = new MeshReport
        {
            asset_name = mesh.name,
            vertex_count = mesh.vertexCount,
            index_count = Enumerable.Range(0, mesh.subMeshCount).Sum(slot => (int)mesh.GetIndexCount(slot)),
            submesh_count = mesh.subMeshCount,
            vertices = Flatten(vertices),
            normals = Flatten(normals),
        };
        for (int slot = 0; slot < mesh.subMeshCount; ++slot)
        {
            int[] indices = mesh.GetTriangles(slot);
            report.submeshes.Add(new SubmeshReport { slot = slot, indices = indices });
            for (int triangle = 0; triangle < indices.Length / 3; ++triangle)
            {
                int first = triangle * 3;
                Vector3 a = vertices[indices[first]];
                Vector3 b = vertices[indices[first + 1]];
                Vector3 c = vertices[indices[first + 2]];
                Vector3 geometric = Vector3.Cross(b - a, c - a).normalized;
                Vector3 average = (normals[indices[first]] + normals[indices[first + 1]] + normals[indices[first + 2]]).normalized;
                report.triangle_orientation.Add(new TriangleReport
                {
                    slot = slot,
                    triangle = triangle,
                    geometric_normal = V(geometric),
                    average_imported_normal = V(average),
                    normal_dot = Q(Vector3.Dot(geometric, average)),
                });
            }
        }
        return report;
    }

    private static void Validate(Report report, GameObject instance)
    {
        // Units: no hidden scale anywhere between the file and the world.
        Require(Near(report.importer.file_scale, 1.0f), "fileScale is not one");
        Require(report.importer.use_file_scale, "useFileScale was disabled");
        Require(Near(report.importer.global_scale, 1.0f), "globalScale is not one");
        Require(!report.importer.bake_axis_conversion, "the importer baked an axis conversion");
        Require(report.importer_messages.Count == 0, "the production asset emitted importer warnings/errors");

        Transform[] transforms = instance.GetComponentsInChildren<Transform>(true);
        Require(transforms.Length == 9, "the production asset changed the node count");
        Require(Near(instance.transform.localScale, Vector3.one), "the imported root is scaled");
        Require(
            Quaternion.Angle(instance.transform.localRotation, Quaternion.identity) <= 0.0001f,
            "the imported root is rotated");
        Require(
            transforms.All(transform => Near(transform.localScale, Vector3.one)),
            "the conversion hid itself in a hierarchy scale");

        // 1000, 2000 and 3000 FerriteCAD millimetres are 1, 2 and 3 Unity units.
        float[] expectedDistances = { 0.0f, 1.0f, 2.0f, 3.0f };
        for (int index = 0; index < expectedDistances.Length; ++index)
        {
            Require(
                Math.Abs(report.control_points[index].distance_from_origin - expectedDistances[index]) <= Epsilon,
                "control point " + report.control_points[index].name + " measured "
                    + report.control_points[index].distance_from_origin.ToString("R", CultureInfo.InvariantCulture));
        }

        // World matrices, measured as distances a rigid transform must keep.
        Require(
            Math.Abs(report.placement_separation - 2.35583532f) <= 0.0001f,
            "the two placements are " + report.placement_separation.ToString("R", CultureInfo.InvariantCulture)
                + " apart rather than the 2.35584 m their FerriteCAD placements are");
        Require(
            Math.Abs(report.frame_to_first_placement - 1.49666296f) <= 0.0001f,
            "the frame and the first placement are " + report.frame_to_first_placement.ToString("R", CultureInfo.InvariantCulture)
                + " apart rather than 1.49666 m");
        // And the world matrix of every node is its parent's times its own.
        foreach (Transform transform in transforms)
        {
            if (transform.parent == null)
            {
                continue;
            }
            Matrix4x4 composed = transform.parent.localToWorldMatrix
                * Matrix4x4.TRS(transform.localPosition, transform.localRotation, transform.localScale);
            for (int element = 0; element < 16; ++element)
            {
                Require(
                    Math.Abs(composed[element] - transform.localToWorldMatrix[element]) <= 0.0001f,
                    "the world matrix of " + transform.name + " is not its parent's times its own");
            }
        }

        // One definition owns geometry once, however often it is placed.
        Transform[] repeated = transforms
            .Where(transform => transform.name.StartsWith("Repeated Part", StringComparison.Ordinal))
            .ToArray();
        Require(repeated.Length == 2, "the writer merged two siblings with one display name");
        Require(!ReferenceEquals(repeated[0], repeated[1]), "two placements became one transform");
        Require(report.mesh_filter_count == 2, "the placement count changed");
        Require(report.unique_mesh_asset_count == 1, "the definition's mesh was duplicated per placement");
        Require(report.repeated_parts_share_mesh, "the two placements do not share one sharedMesh");
        Require(report.meshes.Count == 1, "the unique mesh count changed");
        Require(report.meshes[0].index_count == 12, "the index count changed");
        Require(report.meshes[0].submesh_count == 2, "a material slot was lost");
        Require(report.meshes[0].vertex_count >= 4, "the mesh imported no vertices");
        Require(
            report.meshes[0].normals.Length == report.meshes[0].vertex_count * 3,
            "one normal per vertex did not survive");
        Require(
            report.meshes[0].triangle_orientation.Any(item => Math.Abs(item.normal_dot - 1.0f) > 0.01f),
            "every imported normal agrees with its polygon, so they were recalculated");

        // Structure and the omission both survive as empty hierarchy nodes.
        Transform frame = transforms.Single(transform => transform.name == "Assembly Frame");
        Require(frame.parent == instance.transform, "the assembly frame changed parent");
        Require(repeated.All(transform => transform.parent == frame), "a placement lost its parent");
        Transform omitted = transforms.Single(transform => transform.name == "Omitted #2583");
        Require(omitted.GetComponent<MeshFilter>() == null, "the omitted definition was given geometry");
        Require(omitted.parent == frame, "the omitted definition lost its hierarchy node");
        Require(
            transforms.Count(transform => transform.name.StartsWith("CP ", StringComparison.Ordinal)) == 4,
            "a structural control point was dropped");

        // Materials: two slots on each placement, the definition's colours on
        // one and the placement's override on the other, and Unity's linear
        // reading gives back what FerriteCAD stored.
        Require(report.materials.Count == 4, "the two slots on two placements changed");
        MaterialReport[] plain = report.materials
            .Where(item => item.node_path == report.materials[0].node_path).ToArray();
        MaterialReport[] recoloured = report.materials
            .Where(item => item.node_path != report.materials[0].node_path).ToArray();
        Require(plain.Length == 2 && recoloured.Length == 2, "a placement lost a slot");
        RequireLinear(plain[0], SourceRed, "the first slot");
        RequireLinear(plain[1], SourceBlue, "the second slot");
        RequireLinear(recoloured[0], SourceOverride, "the overridden first slot");
        RequireLinear(recoloured[1], SourceOverride, "the overridden second slot");
        Require(
            plain[0].material_name != recoloured[0].material_name,
            "the override changed the definition's own material instead of binding its own");

        // The omission is visible to the import callback, and structure is not
        // marked as missing.
        Require(
            report.user_properties.Any(item =>
                item.property == "FerriteCADGeometryOmission" && item.value.Contains("#2583")),
            "the omission property did not reach the AssetPostprocessor");
        Require(
            report.user_properties.Any(item =>
                item.property == "FerriteCADOmissionRefusal" && item.value == "IncompleteFace"),
            "the typed refusal name did not reach the AssetPostprocessor");
        Require(
            report.user_properties.Any(item =>
                item.property == "FerriteCADComplete" && item.value == "False"),
            "the partial marker did not reach the AssetPostprocessor");
        Require(
            report.user_properties.Count(item => item.property == "FerriteCADGeometryOmission") == 1,
            "a structural node was marked as a missing part");
        Require(
            report.user_properties.Count(item => item.property == "FerriteCADNodeKey") == 9,
            "not every node carried its deterministic key");
    }

    private static void RequireLinear(MaterialReport material, float[] expected, string what)
    {
        for (int component = 0; component < 3; ++component)
        {
            Require(
                Math.Abs(material.base_colour_linear[component] - expected[component]) <= 0.002f,
                what + " component " + component.ToString(CultureInfo.InvariantCulture) + " read back as "
                    + material.base_colour_linear[component].ToString("R", CultureInfo.InvariantCulture)
                    + " rather than the stored " + expected[component].ToString("R", CultureInfo.InvariantCulture));
        }
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

    private static List<UserPropertyReport> ReadUserProperties(string assetPath)
    {
        string path = FerriteFbxUserPropertyProbe.CachePath(assetPath);
        Require(File.Exists(path), "the custom-property callback did not run");
        List<UserPropertyReport> result = new List<UserPropertyReport>();
        foreach (string line in File.ReadAllLines(path))
        {
            if (String.IsNullOrEmpty(line))
            {
                continue;
            }
            string[] fields = line.Split('\t');
            Require(fields.Length == 3, "malformed custom-property probe line");
            result.Add(new UserPropertyReport
            {
                node_name = fields[0],
                property = fields[1],
                value = fields[2],
            });
        }
        return result
            .OrderBy(item => item.node_name, StringComparer.Ordinal)
            .ThenBy(item => item.property, StringComparer.Ordinal)
            .ThenBy(item => item.value, StringComparer.Ordinal)
            .ToList();
    }

    private static Color BaseColour(Material material)
    {
        Require(material != null, "Unity imported a null material slot");
        if (material.HasProperty("_BaseColor"))
        {
            return material.GetColor("_BaseColor");
        }
        if (material.HasProperty("_Color"))
        {
            return material.GetColor("_Color");
        }
        throw new InvalidOperationException("material has no base colour property: " + material.name);
    }

    private static string StablePath(Transform transform, Transform root)
    {
        List<string> pieces = new List<string>();
        Transform current = transform;
        while (current != null)
        {
            pieces.Add(current.GetSiblingIndex().ToString(CultureInfo.InvariantCulture) + ":" + current.name);
            if (current == root)
            {
                break;
            }
            current = current.parent;
        }
        pieces.Reverse();
        return String.Join("/", pieces);
    }

    private static string CanonicalMessage(string message)
    {
        string project = Directory.GetCurrentDirectory().Replace('\\', '/');
        return message.Replace('\\', '/').Replace(project, "<project>").Replace("\r", String.Empty).Trim();
    }

    private static float[] V(Vector3 value)
    {
        return new[] { Q(value.x), Q(value.y), Q(value.z) };
    }

    private static float[] Q4(Quaternion value)
    {
        if (value.w < 0.0f)
        {
            value = new Quaternion(-value.x, -value.y, -value.z, -value.w);
        }
        return new[] { Q(value.x), Q(value.y), Q(value.z), Q(value.w) };
    }

    private static float[] C(Color value)
    {
        return new[] { Q(value.r), Q(value.g), Q(value.b), Q(value.a) };
    }

    private static float[] M(Matrix4x4 value)
    {
        float[] result = new float[16];
        for (int row = 0; row < 4; ++row)
        {
            for (int column = 0; column < 4; ++column)
            {
                result[row * 4 + column] = Q(value[row, column]);
            }
        }
        return result;
    }

    private static float[] Flatten(Vector3[] values)
    {
        float[] result = new float[values.Length * 3];
        for (int index = 0; index < values.Length; ++index)
        {
            result[index * 3] = Q(values[index].x);
            result[index * 3 + 1] = Q(values[index].y);
            result[index * 3 + 2] = Q(values[index].z);
        }
        return result;
    }

    private static float Q(float value)
    {
        float rounded = (float)Math.Round(value, 6, MidpointRounding.AwayFromZero);
        return rounded == 0.0f ? 0.0f : rounded;
    }

    private static bool Near(float left, float right)
    {
        return Math.Abs(left - right) <= Epsilon;
    }

    private static bool Near(Vector3 left, Vector3 right)
    {
        return (left - right).sqrMagnitude <= Epsilon * Epsilon;
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
