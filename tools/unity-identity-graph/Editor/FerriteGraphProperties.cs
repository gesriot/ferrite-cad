// SPDX-License-Identifier: MIT
//
// Unity hands an importer's custom properties to a callback, and that callback
// is the only place a probe here is allowed to learn which FerriteCAD
// definition an imported object came from. Never a display name — §22B-1c
// measured that several definitions of one real assembly carry the same
// designation — and never a position in the hierarchy, because positions move
// on purpose in these documents.
//
// This is §22B-1e1's reader and nothing more. §22B-1e2a's companion rename
// lived in the same class there and is deliberately absent here: this slice
// measures graphs and importer APIs, and a plugin that renamed objects during
// the import would make every name below a property of the plugin.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

internal sealed class FerriteGraphProperties : AssetPostprocessor
{
    // Where a probe reads what this callback saw, one file per asset path.
    internal static string CachePath(string assetPath)
    {
        string safe = assetPath.Replace('/', '_').Replace('\\', '_').Replace(':', '_');
        return Path.Combine(Path.GetTempPath(), "ferritecad-graph-props-" + safe + ".tsv");
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
