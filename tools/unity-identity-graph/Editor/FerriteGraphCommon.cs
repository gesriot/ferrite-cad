// SPDX-License-Identifier: MIT
//
// The plumbing every §22B-1e2b probe shares.
//
// Four probes ask four different questions — an alternative FBX graph, the
// `.meta` identity table, `AssetImporter.AddRemap`, and a `ScriptedImporter`
// building objects through `AssetImportContext.AddObjectToAsset` — and all
// four have to count checks the same way, tokenise GUIDs the same way, and
// canonicalise a log line the same way, or their reports cannot be compared
// with each other or with the two clean projects each of them runs in.
//
// Nothing here decides anything. It reads Unity and refuses.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text.RegularExpressions;
using UnityEditor;
using UnityEngine;

internal static class FerriteGraphCommon
{
    // Every machine-derived name and identifier in this measurement begins
    // with this, which is what makes "would a person see a machine token" a
    // measurable question rather than a matter of opinion.
    internal const string MachinePrefix = "fcad~";

    internal static int Checks;
    internal static int DerivedIdentifiers;
    internal static int OtherIdentifiers;

    private static readonly Dictionary<string, string> Tokens =
        new Dictionary<string, string>(StringComparer.Ordinal);

    internal static void Reset()
    {
        Checks = 0;
        DerivedIdentifiers = 0;
        OtherIdentifiers = 0;
        Tokens.Clear();
        Instances.Clear();
    }

    internal static int DistinctGuids
    {
        get { return Tokens.Count; }
    }

    internal static void Require(bool condition, string message)
    {
        ++Checks;
        if (!condition)
        {
            throw new InvalidOperationException(message);
        }
    }

    // A GUID is new in every project, so it can never appear in a canonical
    // report. A mutant that leaves one untokenised is killed only by the
    // second clean project, which is how this harness proves that second
    // project is load-bearing rather than decorative.
    internal static string GuidToken(string guid)
    {
        if (String.IsNullOrEmpty(guid))
        {
            return "<guid-none>";
        }
        if (!Tokens.TryGetValue(guid, out string token))
        {
            token = "<guid-" + Tokens.Count.ToString(CultureInfo.InvariantCulture) + ">";
            Tokens[guid] = token;
        }
        return token;
    }

    // True reference identity, kept apart from the local file identifier on
    // purpose: "these two placements hold one and the same `Mesh` object" and
    // "these two identifiers are equal" are different questions, and the
    // shared-mesh result depends on asking the first one.
    private static readonly List<UnityEngine.Object> Instances =
        new List<UnityEngine.Object>();

    internal static int InstanceKey(UnityEngine.Object target)
    {
        for (int index = 0; index < Instances.Count; ++index)
        {
            if (ReferenceEquals(Instances[index], target))
            {
                return index;
            }
        }
        Instances.Add(target);
        return Instances.Count - 1;
    }

    internal static long LocalId(UnityEngine.Object target)
    {
        AssetDatabase.TryGetGUIDAndLocalFileIdentifier(target, out string guid, out long local);
        return local;
    }

    internal static string GuidOf(UnityEngine.Object target)
    {
        AssetDatabase.TryGetGUIDAndLocalFileIdentifier(target, out string guid, out long local);
        return GuidToken(guid);
    }

    internal static string Identify(UnityEngine.Object target)
    {
        return target.GetType().Name + ":" + target.name + ":"
            + LocalId(target).ToString(CultureInfo.InvariantCulture);
    }

    internal static string CanonicalIdentifier(GlobalObjectId identifier)
    {
        string text = identifier.ToString();
        foreach (KeyValuePair<string, string> entry in Tokens)
        {
            text = text.Replace(entry.Key, entry.Value);
        }
        return text;
    }

    // Whether an object's `GlobalObjectId` really is "this asset's GUID plus
    // this local file identifier". §22B-1e1 measured that it is, and every
    // conclusion below about a local file identifier being a reference depends
    // on it staying true, so it is counted rather than remembered.
    internal static void CountIdentifierShape(UnityEngine.Object item, string token, long local)
    {
        string expected = "GlobalObjectId_V1-1-" + token + "-"
            + unchecked((ulong)local).ToString(CultureInfo.InvariantCulture) + "-0";
        if (CanonicalIdentifier(GlobalObjectId.GetGlobalObjectIdSlow(item)) == expected)
        {
            ++DerivedIdentifiers;
        }
        else
        {
            ++OtherIdentifiers;
        }
    }

    private static readonly Regex RawGuid = new Regex("[0-9a-f]{32}", RegexOptions.Compiled);

    internal static string Canonical(string message)
    {
        string project = Directory.GetCurrentDirectory().Replace('\\', '/');
        string text = message.Replace('\\', '/').Replace(project, "<project>")
            .Replace("\r", String.Empty).Trim();
        return RawGuid.Replace(text, match => GuidToken(match.Value));
    }

    // Imports one asset and reports every warning and error Unity raised while
    // doing it, grouped and counted. The `Identifier uniqueness violation`
    // §22B-1b2 first saw is one of these, and a variant that trades it for a
    // different warning has not removed it.
    internal static List<string> Import(string assetPath)
    {
        List<string> messages = new List<string>();
        Application.LogCallback capture = (message, stack, kind) =>
        {
            if (kind == LogType.Warning || kind == LogType.Error || kind == LogType.Exception)
            {
                messages.Add(kind.ToString().ToLowerInvariant() + ": " + Canonical(message));
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
        return Group(messages);
    }

    internal static List<string> Group(IEnumerable<string> messages)
    {
        return messages
            .GroupBy(message => message, StringComparer.Ordinal)
            .OrderBy(group => group.Key, StringComparer.Ordinal)
            .Select(group => group.Count().ToString(CultureInfo.InvariantCulture) + " x " + group.Key)
            .ToList();
    }

    internal static string Transition(List<string> before, List<string> after)
    {
        bool had = before.Count > 0;
        bool has = after.Count > 0;
        if (!had && !has)
        {
            return "never_warned";
        }
        if (had && !has)
        {
            return "warning_disappeared";
        }
        if (!had)
        {
            return "warning_appeared";
        }
        return before.SequenceEqual(after) ? "warning_unchanged" : "warning_changed";
    }

    // Unity sorts an imported hierarchy by name by default, which would make
    // every ordering experiment below measure the sort instead of the file.
    // The default is asserted first, so a future editor that stopped sorting
    // is a refusal rather than a silently different measurement.
    internal static void SettleImporter(string assetPath, bool requireDefaultSort)
    {
        AssetDatabase.ImportAsset(
            assetPath,
            ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        ModelImporter importer = AssetImporter.GetAtPath(assetPath) as ModelImporter;
        Require(importer != null, "Unity gave the imported asset no ModelImporter");
        if (requireDefaultSort)
        {
            Require(
                importer.sortHierarchyByName,
                "this editor no longer sorts an imported hierarchy by name by default, so the "
                    + "controls here are measuring something else than they were written for");
        }
        if (importer.sortHierarchyByName)
        {
            importer.sortHierarchyByName = false;
            importer.SaveAndReimport();
            AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        }
    }

    // Every object an import published, with the identifier shape each one
    // has. Called at every phase of every probe: a conclusion about what an
    // import contains is only as good as the enumeration behind it, and this
    // is where that enumeration is counted.
    internal static int RecordSubassets(string assetPath)
    {
        int seen = 0;
        foreach (UnityEngine.Object item in AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null))
        {
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(item, out string guid, out long local);
            CountIdentifierShape(item, GuidToken(guid), local);
            ++Checks;
            ++seen;
        }
        return seen;
    }

    internal static string Colour(Material material)
    {
        Color colour;
        if (material.HasProperty("_BaseColor"))
        {
            colour = material.GetColor("_BaseColor");
        }
        else if (material.HasProperty("_Color"))
        {
            colour = material.GetColor("_Color");
        }
        else
        {
            throw new InvalidOperationException(
                "an imported material has no base colour property: " + material.name);
        }
        Color linear = colour.linear;
        return "[" + Round(linear.r) + "," + Round(linear.g) + "," + Round(linear.b) + "]";
    }

    internal static string Position(Vector3 value)
    {
        return "[" + Round(value.x) + "," + Round(value.y) + "," + Round(value.z) + "]";
    }

    internal static string Round(float value)
    {
        float rounded = (float)Math.Round(value, 4, MidpointRounding.AwayFromZero);
        if (rounded == 0.0f)
        {
            rounded = 0.0f;
        }
        return rounded.ToString("0.0000", CultureInfo.InvariantCulture);
    }

    internal static string Fingerprint(string path)
    {
        ulong hash = 14695981039346656037UL;
        using (FileStream stream = File.OpenRead(path))
        {
            byte[] buffer = new byte[65536];
            int read;
            while ((read = stream.Read(buffer, 0, buffer.Length)) > 0)
            {
                for (int index = 0; index < read; ++index)
                {
                    hash ^= buffer[index];
                    hash *= 1099511628211UL;
                }
            }
        }
        return hash.ToString("x16", CultureInfo.InvariantCulture);
    }

    internal static string FingerprintText(string text)
    {
        ulong hash = 14695981039346656037UL;
        foreach (byte value in System.Text.Encoding.UTF8.GetBytes(text))
        {
            hash ^= value;
            hash *= 1099511628211UL;
        }
        return hash.ToString("x16", CultureInfo.InvariantCulture);
    }

    internal static bool SameBytes(string left, string right)
    {
        byte[] first = File.ReadAllBytes(left);
        byte[] second = File.ReadAllBytes(right);
        return first.Length == second.Length && first.SequenceEqual(second);
    }

    internal static string ArgumentValue(string name)
    {
        string[] arguments = Environment.GetCommandLineArgs();
        for (int index = 0; index + 1 < arguments.Length; ++index)
        {
            if (arguments[index] == name)
            {
                return arguments[index + 1];
            }
        }
        return null;
    }

    internal static bool HasArgument(string name)
    {
        return Environment.GetCommandLineArgs().Contains(name);
    }
}
