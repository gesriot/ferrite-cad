// SPDX-License-Identifier: MIT
//
// §22B-1e2b, part E: a `ScriptedImporter` that builds its objects through
// `AssetImportContext.AddObjectToAsset(identifier, object)`, where the
// identifier comes from the full durable identity and from nothing else.
//
// This is a measurement probe for a *test* extension. It is not a FerriteCAD
// importer, it does not read FBX, it is not a package, and nothing about it is
// proposed for the product. What it exists to establish is one fact §22B-1e2a
// could not reach: whether Unity will let an importer choose the local file
// identifiers of a `GameObject`, a `Mesh` and a `Material` from an identity
// the file carries, while the names a person reads stay the designations.
//
// The document it reads is synthetic on purpose and carries, all at once, the
// confusions the real corpus has: two `ImportedSourceId`s sharing one
// source-local key, two definitions sharing a designation, several placements
// of one definition sharing one `Mesh`, several materials, a structural node
// and an omitted one. A scheme that only works because none of those were
// present has not been measured.
//
// Two rules this importer keeps, and the probe beside it checks:
//
//   * an identifier is built from the durable identity only — never from a
//     designation, never from a position, never from an ordinal;
//   * a designation is written to `name` and nowhere else, so the visible name
//     and the identity are provably separate channels.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using UnityEditor.AssetImporters;
using UnityEngine;

[ScriptedImporter(1, "fcadsyn")]
internal sealed class FerriteSyntheticImporter : ScriptedImporter
{
    // Where the probe reads what this importer decided, one file per asset
    // path. Written from the importer itself so the identifiers under test are
    // the ones it really passed to Unity, not ones the probe recomputed.
    internal static string CachePath(string assetPath)
    {
        string safe = assetPath.Replace('/', '_').Replace('\\', '_').Replace(':', '_');
        return Path.Combine(Path.GetTempPath(), "ferritecad-synthetic-" + safe + ".tsv");
    }

    [Serializable]
    private sealed class Document
    {
        public List<DefinitionRecord> definitions = new List<DefinitionRecord>();
        public List<PlacementRecord> placements = new List<PlacementRecord>();
        // Two objects given one identifier on purpose. A scheme cannot be
        // called collision-safe without measuring what Unity does when it is
        // handed a collision, so the case exists and is written here.
        public bool force_identifier_collision;
    }

    [Serializable]
    private sealed class DefinitionRecord
    {
        public string definition_id = String.Empty;
        public string designation = String.Empty;
        public int vertices;
        public List<SlotRecord> slots = new List<SlotRecord>();
    }

    [Serializable]
    private sealed class SlotRecord
    {
        public string designation = String.Empty;
        public float r;
        public float g;
        public float b;
    }

    [Serializable]
    private sealed class PlacementRecord
    {
        public string occurrence_id = String.Empty;
        public string definition_id = String.Empty;
        public string designation = String.Empty;
        // "mesh", "structural" or "omitted". An omitted definition never gets
        // a `Mesh`: a partial export that started to look complete is exactly
        // what the §22B-1c boundary exists for.
        public string kind = "mesh";
        public string parent_occurrence_id = String.Empty;
        public float x;
        public float y;
        public float z;
    }

    // The identifier a durable identity produces. Deliberately readable, and
    // deliberately built from nothing a person types: an identifier that
    // contained a designation would move when the designation moved, which is
    // the defect this whole slice is about.
    internal static string MeshIdentifier(string definitionId)
    {
        return "fcad|mesh|" + definitionId;
    }

    internal static string MaterialIdentifier(string definitionId, int slot)
    {
        return "fcad|material|" + definitionId + "|"
            + slot.ToString(CultureInfo.InvariantCulture);
    }

    internal static string ObjectIdentifier(string definitionId, string occurrenceId)
    {
        return "fcad|object|" + definitionId + "|" + occurrenceId;
    }

    public override void OnImportAsset(AssetImportContext ctx)
    {
        Document document =
            JsonUtility.FromJson<Document>(File.ReadAllText(ctx.assetPath));
        if (document == null)
        {
            ctx.LogImportError("the synthetic document did not parse");
            return;
        }

        GameObject root = new GameObject(Path.GetFileNameWithoutExtension(ctx.assetPath));
        ctx.AddObjectToAsset("fcad|root", root);
        ctx.SetMainObject(root);

        List<string> record = new List<string>();
        record.Add("root\tfcad|root\t" + root.name);

        Shader shader = Shader.Find("Standard") ?? Shader.Find("Diffuse");
        if (shader == null)
        {
            ctx.LogImportError("this editor has no shader this probe can build a material with");
            return;
        }

        Dictionary<string, Mesh> meshes = new Dictionary<string, Mesh>(StringComparer.Ordinal);
        Dictionary<string, Material[]> materials =
            new Dictionary<string, Material[]>(StringComparer.Ordinal);
        Dictionary<string, DefinitionRecord> byId =
            new Dictionary<string, DefinitionRecord>(StringComparer.Ordinal);

        foreach (DefinitionRecord definition in document.definitions)
        {
            byId[definition.definition_id] = definition;
            if (definition.vertices > 0)
            {
                Mesh mesh = BuildMesh(definition.vertices);
                // The designation, and only the designation. The identity is
                // the argument to `AddObjectToAsset` below.
                mesh.name = definition.designation;
                string identifier = MeshIdentifier(definition.definition_id);
                ctx.AddObjectToAsset(identifier, mesh);
                meshes[definition.definition_id] = mesh;
                record.Add("mesh\t" + identifier + "\t" + mesh.name);
            }
            Material[] slots = new Material[definition.slots.Count];
            for (int slot = 0; slot < definition.slots.Count; ++slot)
            {
                Material material = new Material(shader);
                material.name = definition.slots[slot].designation;
                material.color = new Color(
                    definition.slots[slot].r, definition.slots[slot].g, definition.slots[slot].b);
                string identifier = document.force_identifier_collision
                    // One identifier for every slot of every definition. Not a
                    // scheme: the collision case, written so the editor's
                    // answer to it is measured rather than assumed.
                    ? "fcad|material|collision"
                    : MaterialIdentifier(definition.definition_id, slot);
                ctx.AddObjectToAsset(identifier, material);
                slots[slot] = material;
                record.Add("material\t" + identifier + "\t" + material.name);
            }
            materials[definition.definition_id] = slots;
        }

        Dictionary<string, GameObject> placements =
            new Dictionary<string, GameObject>(StringComparer.Ordinal);
        foreach (PlacementRecord placement in document.placements)
        {
            GameObject target = new GameObject(placement.designation);
            target.transform.localPosition =
                new Vector3(placement.x, placement.y, placement.z);
            string identifier =
                ObjectIdentifier(placement.definition_id, placement.occurrence_id);
            ctx.AddObjectToAsset(identifier, target);
            FerriteSyntheticTag tag = target.AddComponent<FerriteSyntheticTag>();
            tag.identifier = identifier;
            tag.definition_id = placement.definition_id;
            tag.occurrence_id = placement.occurrence_id;
            tag.kind = placement.kind;
            tag.designation = placement.designation;
            placements[placement.occurrence_id] = target;
            record.Add("object\t" + identifier + "\t" + target.name);

            if (placement.kind == "mesh"
                && meshes.TryGetValue(placement.definition_id, out Mesh shared))
            {
                // One shared `Mesh` for every placement of one definition,
                // handed out by reference. A copy here would answer the
                // shared-mesh question by dodging it.
                target.AddComponent<MeshFilter>().sharedMesh = shared;
                target.AddComponent<MeshRenderer>().sharedMaterials =
                    materials.TryGetValue(placement.definition_id, out Material[] slots)
                        ? slots
                        : Array.Empty<Material>();
            }
        }

        foreach (PlacementRecord placement in document.placements)
        {
            GameObject target = placements[placement.occurrence_id];
            if (placement.parent_occurrence_id.Length > 0
                && placements.TryGetValue(placement.parent_occurrence_id, out GameObject parent))
            {
                target.transform.SetParent(parent.transform, false);
            }
            else
            {
                target.transform.SetParent(root.transform, false);
            }
        }

        foreach (string id in byId.Keys)
        {
            if (!meshes.ContainsKey(id))
            {
                record.Add("omitted\t" + MeshIdentifier(id) + "\t<no mesh published>");
            }
        }

        File.WriteAllText(
            CachePath(ctx.assetPath),
            String.Join("\n", record) + "\n",
            new System.Text.UTF8Encoding(false));
    }

    // A mesh whose vertex count alone says which definition it came from,
    // built the same way the FBX documents beside this one build theirs.
    private static Mesh BuildMesh(int vertices)
    {
        Vector3[] positions = new Vector3[vertices];
        for (int index = 0; index < vertices; ++index)
        {
            positions[index] = new Vector3(0.1f * index, 0.2f * index, 0.3f * index);
        }
        List<int> triangles = new List<int>();
        for (int index = 0; index + 2 < vertices; index += 3)
        {
            triangles.Add(index);
            triangles.Add(index + 1);
            triangles.Add(index + 2);
        }
        Mesh mesh = new Mesh();
        mesh.vertices = positions;
        mesh.triangles = triangles.ToArray();
        mesh.RecalculateNormals();
        return mesh;
    }
}
