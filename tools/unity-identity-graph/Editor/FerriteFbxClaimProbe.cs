// SPDX-License-Identifier: MIT
//
// What happened when a `ScriptedImporter` claimed `fbx`.
//
// Measured, not argued: the probe imports a real production FBX into a project
// where `FerriteFbxClaimImporter` is compiled, and records which importer the
// asset actually got, whether the scripted one ran, whether the model's
// sub-assets are still there, and every message Unity produced while deciding.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;

internal static class FerriteFbxClaimProbe
{
    private const string AssetFolder = "Assets/Claim";

    // The marker the claiming importer appends to when it runs. It lives here
    // rather than on the importer because this probe is compiled into every
    // mode and the importer into exactly one: a probe that could not compile
    // without the importer could not report that the importer is absent.
    internal static string MarkerPath
    {
        get { return Path.Combine(Path.GetTempPath(), "ferritecad-fbx-claim-ran.txt"); }
    }

    [Serializable]
    internal sealed class Plan
    {
        public string control = String.Empty;
    }

    [Serializable]
    internal sealed class Report
    {
        public string schema = "ferritecad.unity-fbx-claim.v1";
        public string mode = "fbxclaim";
        public string unity_version = String.Empty;
        public bool a_scripted_importer_claiming_fbx_compiles;
        public string importer_the_fbx_actually_got = String.Empty;
        public bool the_model_importer_still_owns_fbx;
        public bool the_scripted_importer_ran;
        public bool the_import_still_published_meshes;
        public int subassets;
        public int meshes;
        public int materials;
        public int game_objects;
        public List<string> messages = new List<string>();
        public string conclusion = String.Empty;
        public int checks;
    }

    internal static Report Execute(string planPath)
    {
        Plan plan = JsonUtility.FromJson<Plan>(File.ReadAllText(planPath));
        FerriteGraphCommon.Require(
            plan != null && File.Exists(plan.control), "the claim plan names no control document");

        if (!AssetDatabase.IsValidFolder(AssetFolder))
        {
            AssetDatabase.CreateFolder("Assets", "Claim");
        }

        // The importer's own type is asked for by name, so "it compiles" is a
        // fact about this editor rather than about this file being present.
        Type claim = AppDomain.CurrentDomain.GetAssemblies()
            .SelectMany(assembly =>
            {
                try
                {
                    return assembly.GetTypes();
                }
                catch (Exception)
                {
                    return Array.Empty<Type>();
                }
            })
            .FirstOrDefault(type => type.Name == "FerriteFbxClaimImporter");

        Report report = new Report
        {
            unity_version = Application.unityVersion,
            a_scripted_importer_claiming_fbx_compiles = claim != null,
        };

        if (File.Exists(MarkerPath))
        {
            File.Delete(MarkerPath);
        }

        string assetPath = AssetFolder + "/claim-probe.fbx";
        File.Copy(plan.control, Path.GetFullPath(assetPath), true);
        report.messages = FerriteGraphCommon.Import(assetPath);

        AssetImporter importer = AssetImporter.GetAtPath(assetPath);
        report.importer_the_fbx_actually_got =
            importer == null ? "<none>" : importer.GetType().Name;
        report.the_model_importer_still_owns_fbx = importer is ModelImporter;
        report.the_scripted_importer_ran = File.Exists(MarkerPath);

        FerriteGraphCommon.RecordSubassets(assetPath);
        UnityEngine.Object[] published = AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null)
            .ToArray();
        report.subassets = published.Length;
        report.meshes = published.Count(item => item is Mesh);
        report.materials = published.Count(item => item is Material);
        report.game_objects = published.Count(item => item is GameObject);
        report.the_import_still_published_meshes = report.meshes > 0;
        ++FerriteGraphCommon.Checks;

        report.conclusion = report.the_model_importer_still_owns_fbx
            ? "the native ModelImporter kept the extension; a ScriptedImporter needs its own"
            : report.the_scripted_importer_ran
                ? "the ScriptedImporter took the extension and the model import stopped happening"
                : "neither importer claimed the asset";

        AssetDatabase.DeleteAsset(assetPath);
        AssetDatabase.DeleteAsset(AssetFolder);
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        return report;
    }
}
