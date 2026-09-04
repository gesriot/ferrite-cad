// SPDX-License-Identifier: MIT
//
// Two jobs that have to live in one `AssetPostprocessor`, and one of them is
// the thing being measured.
//
// The first is the same as §22B-1e1's: Unity hands an importer's custom
// properties to a callback, and that callback is the only place the probe is
// allowed to learn which FerriteCAD definition an imported object came from.
// Never a display name — §22B-1c measured that several definitions of one real
// assembly carry the same designation — and never a position in the hierarchy,
// because positions move on purpose in these documents.
//
// The second is candidate D itself: a FerriteCAD companion postprocessor that
// renames the finished `GameObject`, `Mesh` and `Material` from the human
// designations the file carries as properties. That is a *plugin*, not a
// property of the FBX. It runs only when `-fcadCompanion` is on the editor's
// command line, so the same bytes can be imported with and without it and the
// difference attributed to the plugin rather than to the file.
//
// The two jobs are in one class because Unity does not order two independent
// postprocessors of the same asset, and a rename that happened before the
// properties were recorded would be a measurement of the harness.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using UnityEditor;
using UnityEngine;

internal sealed class FerriteChannelProperties : AssetPostprocessor
{
    // Where the probe reads what this callback saw, one file per asset path.
    internal static string CachePath(string assetPath)
    {
        string safe = assetPath.Replace('/', '_').Replace('\\', '_').Replace(':', '_');
        return Path.Combine(Path.GetTempPath(), "ferritecad-channel-props-" + safe + ".tsv");
    }

    // Whether the FerriteCAD companion package's rename is active for this
    // editor run. A candidate that needs this is a candidate that needs a
    // plugin, and the report says so rather than calling it FBX behaviour.
    internal static bool CompanionActive
    {
        get { return Environment.GetCommandLineArgs().Contains("-fcadCompanion"); }
    }

    private static readonly Dictionary<GameObject, List<KeyValuePair<string, string>>> Seen =
        new Dictionary<GameObject, List<KeyValuePair<string, string>>>();

    private void OnPostprocessGameObjectWithUserProperties(
        GameObject target,
        string[] names,
        object[] values)
    {
        List<KeyValuePair<string, string>> properties = new List<KeyValuePair<string, string>>();
        for (int index = 0; index < names.Length && index < values.Length; ++index)
        {
            properties.Add(new KeyValuePair<string, string>(names[index], Render(values[index])));
        }
        Seen[target] = properties;
    }

    private void OnPostprocessModel(GameObject root)
    {
        // Recorded first, always, and from the object references rather than
        // from any name: whatever the companion does below must not be able to
        // change what this measurement says the file contained.
        StringBuilder text = new StringBuilder();
        Walk(root, "0", text);
        File.WriteAllText(CachePath(assetPath), text.ToString(), new UTF8Encoding(false));

        if (CompanionActive)
        {
            Rename(root);
        }
        Seen.Clear();
    }

    // Candidate D: the companion package's rename.
    //
    // Nothing here invents a designation. An object is renamed only when the
    // file says what a person calls it, so a document that carries no
    // designation keeps whatever name the importer gave it — which is also why
    // running this over candidate A's bytes must change nothing at all.
    private static void Rename(GameObject target)
    {
        if (Seen.TryGetValue(target, out List<KeyValuePair<string, string>> properties))
        {
            Dictionary<string, string> values = new Dictionary<string, string>();
            foreach (KeyValuePair<string, string> property in properties)
            {
                values[property.Key] = property.Value;
            }

            if (values.TryGetValue("FerriteCADDisplayName", out string designation)
                && !String.IsNullOrEmpty(designation))
            {
                target.name = designation;
            }

            MeshFilter filter = target.GetComponent<MeshFilter>();
            if (filter != null
                && filter.sharedMesh != null
                && values.TryGetValue("FerriteCADGeometryDisplayName", out string geometry)
                && !String.IsNullOrEmpty(geometry))
            {
                // A shared mesh is reached once per placement, so this is
                // written more than once with the same value on purpose: the
                // alternative is deciding which placement owns the name, which
                // is exactly the ordering dependence §22B-1e1 measured.
                filter.sharedMesh.name = geometry;
            }

            MeshRenderer renderer = target.GetComponent<MeshRenderer>();
            if (renderer != null)
            {
                Material[] materials = renderer.sharedMaterials;
                for (int slot = 0; slot < materials.Length; ++slot)
                {
                    string key = "FerriteCADMaterialDisplayName"
                        + slot.ToString(CultureInfo.InvariantCulture);
                    if (materials[slot] != null
                        && values.TryGetValue(key, out string material)
                        && !String.IsNullOrEmpty(material))
                    {
                        materials[slot].name = material;
                    }
                }
            }
        }

        Transform transform = target.transform;
        for (int index = 0; index < transform.childCount; ++index)
        {
            Rename(transform.GetChild(index).gameObject);
        }
    }

    private static void Walk(GameObject target, string path, StringBuilder text)
    {
        if (Seen.TryGetValue(target, out List<KeyValuePair<string, string>> properties))
        {
            foreach (KeyValuePair<string, string> property in properties)
            {
                text.Append(path).Append('\t')
                    .Append(property.Key).Append('\t')
                    .Append(property.Value).Append('\n');
            }
        }
        Transform transform = target.transform;
        for (int index = 0; index < transform.childCount; ++index)
        {
            Walk(
                transform.GetChild(index).gameObject,
                path + "/" + index.ToString(CultureInfo.InvariantCulture),
                text);
        }
    }

    private static string Render(object value)
    {
        if (value == null)
        {
            return "<null>";
        }
        if (value is float number)
        {
            return number.ToString("R", CultureInfo.InvariantCulture);
        }
        if (value is double wide)
        {
            return wide.ToString("R", CultureInfo.InvariantCulture);
        }
        if (value is bool flag)
        {
            return flag ? "true" : "false";
        }
        return Convert.ToString(value, CultureInfo.InvariantCulture) ?? "<null>";
    }
}
