// SPDX-License-Identifier: MIT
//
// Unity's own view of the durable keys the FBX writer put in the file.
//
// The probe beside this one must not learn which FerriteCAD definition an
// imported object came from by reading a display name: §22B-1c already
// measured that several definitions of one assembly are called the same
// thing. The importer hands custom properties to this callback, and this is
// the only place the measurement gets `FerriteCADDefinitionKey` and
// `FerriteCADNodeKey` from Unity rather than from the file.
//
// The join key between this callback and the finished asset is the chain of
// sibling indices, and nothing else: no name, because names repeat, and no
// FerriteCAD key, because writing the key into its own join key would be
// assuming the answer. The probe proves the join independently by checking
// that the mesh under each key has the vertex count `ufbx` read for that key.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

internal sealed class FerriteFbxIdentityProperties : AssetPostprocessor
{
    // Where the probe reads what this callback saw, one file per asset path.
    internal static string CachePath(string assetPath)
    {
        string safe = assetPath.Replace('/', '_').Replace('\\', '_').Replace(':', '_');
        return Path.Combine(Path.GetTempPath(), "ferritecad-identity-props-" + safe + ".tsv");
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
        StringBuilder text = new StringBuilder();
        Walk(root, "0", text);
        File.WriteAllText(CachePath(assetPath), text.ToString(), new UTF8Encoding(false));
        Seen.Clear();
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
