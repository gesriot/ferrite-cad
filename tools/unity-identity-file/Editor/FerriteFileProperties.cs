// SPDX-License-Identifier: MIT
//
// The one place this measurement is allowed to learn what an imported object
// came from.
//
// Unity hands an FBX's custom properties to this callback and nowhere else: a
// finished `GameObject` in the AssetDatabase carries its name, its components
// and its transform, and no trace of the properties the file wrote. So the
// values are recorded here, keyed by where the object sits in the imported
// hierarchy, and the probe reads them back out of that cache.
//
// This is a measurement, not a product. It renames nothing, remaps nothing and
// publishes nothing: §22B-1e3b ships no companion package, and a probe that
// changed what the editor produced would be measuring itself.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

internal sealed class FerriteFileProperties : AssetPostprocessor
{
    // Where the probe reads what this callback saw, one file per asset path.
    internal static string CachePath(string assetPath)
    {
        string safe = assetPath.Replace('/', '_').Replace('\\', '_').Replace(':', '_');
        return Path.Combine(Path.GetTempPath(), "ferritecad-file-props-" + safe + ".tsv");
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
