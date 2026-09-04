// SPDX-License-Identifier: MIT
//
// §22B-1e2b, part E, the separable half: can a `ScriptedImporter` own the
// `.fbx` extension, or does it need one of its own?
//
// This file exists only in the `fbxclaim` editor run, in its own freshly
// created project. It claims `fbx` and does the least an importer can do. If
// the native `ModelImporter` keeps the extension, this importer never runs and
// the probe records that; if this importer takes it, every `.fbx` in the
// project stops being a model and the probe records that instead.
//
// It is compiled into no other mode on purpose. An importer that claimed `fbx`
// while the graph, `.meta` and `AddRemap` probes were importing `.fbx` files
// would make all three of them measurements of this file.
using System.IO;
using UnityEditor.AssetImporters;
using UnityEngine;

[ScriptedImporter(1, "fbx")]
internal sealed class FerriteFbxClaimImporter : ScriptedImporter
{
    public override void OnImportAsset(AssetImportContext ctx)
    {
        // The marker the probe looks for to decide whether this importer
        // ran at all: "no object was published" and "the importer never
        // ran" are different answers.
        File.AppendAllText(FerriteFbxClaimProbe.MarkerPath, ctx.assetPath + "\n");
        GameObject root = new GameObject(Path.GetFileNameWithoutExtension(ctx.assetPath));
        ctx.AddObjectToAsset("fcad|claim|root", root);
        ctx.SetMainObject(root);
    }
}
