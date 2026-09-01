// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;

internal sealed class FerriteFbxUserPropertyProbe : AssetPostprocessor
{
    private const string CacheDirectory = "Library/FerriteFbxSmokeUserProperties";

    private void OnPreprocessModel()
    {
        Directory.CreateDirectory(CacheDirectory);
        File.WriteAllText(CachePath(assetPath), String.Empty);
    }

    private void OnPostprocessGameObjectWithUserProperties(
        GameObject gameObject,
        string[] propertyNames,
        object[] values)
    {
        Directory.CreateDirectory(CacheDirectory);
        using (StreamWriter writer = File.AppendText(CachePath(assetPath)))
        {
            for (int index = 0; index < propertyNames.Length; ++index)
            {
                writer.Write(Escape(gameObject.name));
                writer.Write('\t');
                writer.Write(Escape(propertyNames[index]));
                writer.Write('\t');
                writer.WriteLine(Escape(Convert.ToString(values[index], CultureInfo.InvariantCulture) ?? String.Empty));
            }
        }
    }

    internal static string CachePath(string path)
    {
        return Path.Combine(CacheDirectory, Path.GetFileName(path) + ".tsv");
    }

    private static string Escape(string value)
    {
        return value.Replace("\\", "\\\\").Replace("\t", "\\t").Replace("\r", "\\r").Replace("\n", "\\n");
    }
}

internal static class FerriteFbxSmoke
{
    private const string FixtureDirectory = "Assets/Fixtures";
    private const string ExpectedReport = "Assets/Expected/unity-import-report.json";
    private const float Epsilon = 0.00001f;

    [Serializable]
    private sealed class Report
    {
        public string schema = "ferritecad.unity-fbx-smoke.v1";
        public string unity_version = String.Empty;
        public string colour_space = String.Empty;
        public List<FixtureReport> fixtures = new List<FixtureReport>();
        public List<EncodingReport> encoding_probes = new List<EncodingReport>();
        public int checks;
    }

    [Serializable]
    private sealed class EncodingReport
    {
        public string fixture = String.Empty;
        public string encoding = String.Empty;
        public int fbx_version;
        public bool accepted;
        public ImporterReport importer = new ImporterReport();
        public string root_name = String.Empty;
        public int mesh_filter_count;
        public List<LogReport> importer_messages = new List<LogReport>();
    }

    [Serializable]
    private sealed class FixtureReport
    {
        public string fixture = String.Empty;
        public ImporterReport importer = new ImporterReport();
        public List<LogReport> importer_messages = new List<LogReport>();
        public List<NodeReport> tree = new List<NodeReport>();
        public List<ControlPointReport> control_points = new List<ControlPointReport>();
        public float[] world_bounds_min = Array.Empty<float>();
        public float[] world_bounds_max = Array.Empty<float>();
        public int mesh_filter_count;
        public int unique_mesh_asset_count;
        public bool repeated_parts_share_mesh;
        public List<MeshReport> meshes = new List<MeshReport>();
        public List<MaterialReport> materials = new List<MaterialReport>();
        public List<UserPropertyReport> user_properties = new List<UserPropertyReport>();
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
        public float[] local_bounds_center = Array.Empty<float>();
        public float[] local_bounds_size = Array.Empty<float>();
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
            Debug.LogError("FCAD_FBX_SMOKE_FAILURE " + error);
            EditorApplication.Exit(1);
        }
    }

    private static void Execute()
    {
        checks = 0;
        string output = ArgumentValue("-fcadOutput") ?? "measurement-output/unity-import-report.json";
        bool record = HasArgument("-fcadRecord");

        Report report = new Report
        {
            unity_version = Application.unityVersion,
            colour_space = QualitySettings.activeColorSpace.ToString().ToLowerInvariant(),
        };

        string[] fixturePaths = new[]
        {
            "wrong_double_unit_ascii7400.fbx",
            "wrong_m_metadata_ascii7400.fbx",
            "wrong_yup_metadata_ascii7400.fbx",
            "yup_m_preconverted_ascii7400.fbx",
            "zup_mm_ascii7400.fbx",
        }.Select(name => FixtureDirectory + "/" + name).ToArray();
        Require(fixturePaths.All(File.Exists), "the five-scene ASCII fixture matrix is incomplete");

        foreach (string path in fixturePaths)
        {
            report.fixtures.Add(Measure(path));
        }
        report.encoding_probes.Add(MeasureEncodingProbe(FixtureDirectory + "/unity_builtin_disc_binary7400.fbx"));

        CrossCheck(report.fixtures);
        Require(checks > 40, "the probe performed too few checks");
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
            Require(File.Exists(ExpectedReport), "the committed expected Unity report is missing");
            string expected = File.ReadAllText(ExpectedReport).Replace("\r\n", "\n");
            Require(expected == json, "Unity importer report differs from the committed contract");
        }

        Debug.Log("FCAD_FBX_SMOKE_EXECUTED checks=" + report.checks.ToString(CultureInfo.InvariantCulture));
        EditorApplication.Exit(0);
    }

    private static EncodingReport MeasureEncodingProbe(string assetPath)
    {
        List<LogReport> messages = ImportSynchronously(assetPath);
        ModelImporter importer = AssetImporter.GetAtPath(assetPath) as ModelImporter;
        GameObject prefab = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
        Require(importer != null, "the binary encoding probe has no ModelImporter");
        Require(prefab != null, "Unity rejected the pinned FBX 7.4 binary encoding probe");
        EncodingReport result = new EncodingReport
        {
            fixture = Path.GetFileName(assetPath),
            encoding = "binary",
            fbx_version = 7400,
            accepted = true,
            importer = Importer(importer),
            root_name = prefab.name,
            mesh_filter_count = prefab.GetComponentsInChildren<MeshFilter>(true).Length,
            importer_messages = messages,
        };
        Require(result.mesh_filter_count > 0, "the accepted binary probe published no mesh");
        Require(result.importer_messages.Count == 0, "the accepted binary probe emitted warnings/errors");
        return result;
    }

    private static FixtureReport Measure(string assetPath)
    {
        List<LogReport> messages = ImportSynchronously(assetPath);
        ModelImporter importer = AssetImporter.GetAtPath(assetPath) as ModelImporter;
        Require(importer != null, "no ModelImporter for " + assetPath);
        GameObject prefab = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
        Require(prefab != null, "Unity did not publish a GameObject for " + assetPath);

        FixtureReport result = new FixtureReport
        {
            fixture = Path.GetFileName(assetPath),
            importer = Importer(importer),
            importer_messages = messages,
        };

        GameObject instance = UnityEngine.Object.Instantiate(prefab);
        instance.hideFlags = HideFlags.HideAndDontSave;
        try
        {
            Transform[] transforms = instance.GetComponentsInChildren<Transform>(true);
            foreach (Transform transform in transforms)
            {
                result.tree.Add(new NodeReport
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
            result.mesh_filter_count = filters.Length;
            Mesh[] uniqueMeshes = filters.Select(filter => filter.sharedMesh).Distinct().ToArray();
            result.unique_mesh_asset_count = uniqueMeshes.Length;
            Transform[] repeated = transforms.Where(transform => transform.name.StartsWith("Repeated Part", StringComparison.Ordinal)).ToArray();
            result.repeated_parts_share_mesh = repeated.Length == 2
                && repeated[0].GetComponent<MeshFilter>() != null
                && repeated[1].GetComponent<MeshFilter>() != null
                && ReferenceEquals(
                    repeated[0].GetComponent<MeshFilter>().sharedMesh,
                    repeated[1].GetComponent<MeshFilter>().sharedMesh);

            foreach (Mesh mesh in uniqueMeshes.OrderBy(mesh => mesh.name, StringComparer.Ordinal))
            {
                result.meshes.Add(MeasureMesh(mesh));
            }

            foreach (MeshRenderer renderer in instance.GetComponentsInChildren<MeshRenderer>(true))
            {
                Material[] materials = renderer.sharedMaterials;
                for (int slot = 0; slot < materials.Length; ++slot)
                {
                    Color colour = BaseColour(materials[slot]);
                    result.materials.Add(new MaterialReport
                    {
                        node_path = StablePath(renderer.transform, instance.transform),
                        slot = slot,
                        material_name = materials[slot] == null ? "<null>" : materials[slot].name,
                        base_colour = C(colour),
                        base_colour_linear = C(colour.linear),
                    });
                }
            }
            result.materials = result.materials
                .OrderBy(item => item.node_path, StringComparer.Ordinal)
                .ThenBy(item => item.slot)
                .ToList();

            Bounds? bounds = null;
            foreach (Renderer renderer in instance.GetComponentsInChildren<Renderer>(true))
            {
                if (bounds.HasValue)
                {
                    Bounds combined = bounds.Value;
                    combined.Encapsulate(renderer.bounds);
                    bounds = combined;
                }
                else
                {
                    bounds = renderer.bounds;
                }
            }
            Require(bounds.HasValue, "fixture has no world bounds");
            result.world_bounds_min = V(bounds.Value.min);
            result.world_bounds_max = V(bounds.Value.max);

            Transform origin = transforms.Single(transform => transform.name == "CP Origin");
            foreach (string name in new[] { "CP Origin", "CP X1000", "CP Y2000", "CP Z3000" })
            {
                Transform point = transforms.Single(transform => transform.name == name);
                result.control_points.Add(new ControlPointReport
                {
                    name = name,
                    world_position = V(point.position),
                    distance_from_origin = Q(Vector3.Distance(origin.position, point.position)),
                });
            }

            result.user_properties = ReadUserProperties(assetPath);
            ValidateFixture(result, transforms, filters);
        }
        finally
        {
            UnityEngine.Object.DestroyImmediate(instance);
        }

        return result;
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

    private static ImporterReport Importer(ModelImporter importer)
    {
        return new ImporterReport
        {
            file_scale = Q(importer.fileScale),
            use_file_scale = importer.useFileScale,
            global_scale = Q(importer.globalScale),
            bake_axis_conversion = importer.bakeAxisConversion,
        };
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
            local_bounds_center = V(mesh.bounds.center),
            local_bounds_size = V(mesh.bounds.size),
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

    private static void ValidateFixture(FixtureReport report, Transform[] transforms, MeshFilter[] filters)
    {
        Require(report.importer.use_file_scale, report.fixture + " disabled useFileScale");
        Require(Near(report.importer.global_scale, 1.0f), report.fixture + " globalScale is not one");
        Require(!report.importer.bake_axis_conversion, report.fixture + " unexpectedly baked axis conversion");
        Require(report.importer_messages.Count == 0, report.fixture + " emitted importer warnings/errors");
        Require(transforms.Length == 9, report.fixture + " changed the complete node count");
        Transform[] repeated = transforms.Where(transform => transform.name.StartsWith("Repeated Part", StringComparison.Ordinal)).ToArray();
        Require(repeated.Length == 2, report.fixture + " merged duplicate display names");
        Require(!ReferenceEquals(repeated[0], repeated[1]), report.fixture + " collapsed two placements into one transform");
        Require(transforms.Count(transform => transform.name == "Assembly Frame") == 1, report.fixture + " lost the assembly parent");
        Require(transforms.Single(transform => transform.name == "Assembly Frame").parent == transforms[0], report.fixture + " changed the assembly parent");
        Require(repeated.All(transform => transform.parent.name == "Assembly Frame"), report.fixture + " lost a placement parent");
        Transform omitted = transforms.Single(transform => transform.name == "Omitted #2583");
        Require(omitted.GetComponent<MeshFilter>() == null, report.fixture + " invented geometry for #2583");
        Require(omitted.parent.name == "Assembly Frame", report.fixture + " lost #2583's hierarchy node");
        Require(filters.Length == 2, report.fixture + " changed MeshFilter count");
        Require(report.unique_mesh_asset_count == 1, report.fixture + " duplicated the definition mesh");
        Require(report.repeated_parts_share_mesh, report.fixture + " placements do not share sharedMesh");
        Require(report.meshes.Count == 1, report.fixture + " changed unique mesh count");
        Require(report.meshes[0].vertex_count >= 4, report.fixture + " imported no mesh vertices");
        Require(report.meshes[0].index_count == 12, report.fixture + " changed index count");
        Require(report.meshes[0].submesh_count == 2, report.fixture + " lost a material slot");
        Require(report.meshes[0].normals.Length == report.meshes[0].vertex_count * 3, report.fixture + " did not import one normal per vertex");
        Require(report.materials.Count == 4, report.fixture + " changed the two slots on two placements");
        Require(report.user_properties.Any(item => item.property == "FerriteCADGeometryOmission" && item.value.Contains("#2583")), report.fixture + " did not surface the omission custom property");
        Require(report.user_properties.Any(item => item.property == "FerriteCADComplete" && item.value == "False"), report.fixture + " did not surface the partial marker");
        Require(transforms.All(transform => Near(transform.localScale, Vector3.one)), report.fixture + " hid conversion in a hierarchy scale");
    }

    private static void CrossCheck(List<FixtureReport> fixtures)
    {
        FixtureReport zupAscii = Find(fixtures, "zup_mm_ascii7400.fbx");
        FixtureReport yupAscii = Find(fixtures, "yup_m_preconverted_ascii7400.fbx");
        FixtureReport wrongAxis = Find(fixtures, "wrong_yup_metadata_ascii7400.fbx");
        FixtureReport wrongMetres = Find(fixtures, "wrong_m_metadata_ascii7400.fbx");
        FixtureReport wrongDouble = Find(fixtures, "wrong_double_unit_ascii7400.fbx");

        Require(DistancesNear(zupAscii, new[] { 0.0f, 1.0f, 2.0f, 3.0f }, 0.00001f), "raw FCAD millimetres did not become Unity metres once");
        Require(DistancesNear(yupAscii, new[] { 0.0f, 1.0f, 2.0f, 3.0f }, 0.00001f), "preconverted metre contract did not preserve metres");
        Require(
            DistancesNear(wrongMetres, new[] { 0.0f, 1000.0f, 2000.0f, 3000.0f }, 0.01f),
            "metre metadata result was " + String.Join(",", Distances(wrongMetres).Select(value => value.ToString("R", CultureInfo.InvariantCulture))));
        Require(
            DistancesNear(wrongDouble, new[] { 0.0f, 0.001f, 0.002f, 0.003f }, 0.000001f),
            "double unit conversion result was " + String.Join(",", Distances(wrongDouble).Select(value => value.ToString("R", CultureInfo.InvariantCulture))));
        Require(!MatricesEqual(zupAscii, wrongAxis), "changing Z-up to Y-up metadata had no measured effect");
    }

    private static bool MatricesEqual(FixtureReport left, FixtureReport right)
    {
        return left.tree.SelectMany(node => node.world_matrix)
            .SequenceEqual(right.tree.SelectMany(node => node.world_matrix));
    }

    private static float[] Distances(FixtureReport fixture)
    {
        return fixture.control_points.Select(point => point.distance_from_origin).ToArray();
    }

    private static bool DistancesNear(FixtureReport fixture, float[] expected, float tolerance)
    {
        float[] actual = Distances(fixture);
        return actual.Length == expected.Length
            && actual.Zip(expected, (left, right) => Math.Abs(left - right) <= tolerance).All(equal => equal);
    }

    private static FixtureReport Find(List<FixtureReport> fixtures, string name)
    {
        return fixtures.Single(fixture => fixture.fixture == name);
    }

    private static List<UserPropertyReport> ReadUserProperties(string assetPath)
    {
        string path = FerriteFbxUserPropertyProbe.CachePath(assetPath);
        Require(File.Exists(path), "custom-property callback did not run for " + assetPath);
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
