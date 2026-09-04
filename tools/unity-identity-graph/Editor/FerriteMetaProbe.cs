// SPDX-License-Identifier: MIT
//
// §22B-1e2b, part C: the Unity `.meta` identity/remapping table.
//
// §22B-1e2a proved that the *name* is the only identity channel a stock
// importer reads out of an FBX. It did not measure the other end: the sidecar
// Unity writes beside the file, which is where `internalIDToNameTable` lives
// and which several forum answers describe as the place to pin an imported
// object's local file identifier to a name.
//
// This probe asks what that table really is in 6000.4.10f1, and it asks the
// question that decides whether it can be recommended at all: is there a
// public API that writes it, or is the only working path editing undocumented
// serialized metadata by hand. Those two answers are not interchangeable and
// this probe never reports one as the other.
//
// Nothing here is proposed. The probe edits a `.meta`, records what happened,
// and deletes everything it made.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using UnityEditor;
using UnityEngine;

internal static class FerriteMetaProbe
{
    private const string AssetFolder = "Assets/Meta";

    // Unity's class identifiers for the three types this slice is about. Hard
    // numbers, because the table is keyed on them and a name would not tell a
    // reader which rows of a real `.meta` are being counted.
    private const int GameObjectClassId = 1;
    private const int MeshClassId = 43;
    private const int MaterialClassId = 21;

    [Serializable]
    internal sealed class Plan
    {
        public string control = String.Empty;
        public string changed = String.Empty;
        public string reexport = String.Empty;
    }

    [Serializable]
    internal sealed class Report
    {
        public string schema = "ferritecad.unity-meta-identity.v1";
        public string mode = "meta";
        public string unity_version = String.Empty;

        // ---- what Unity itself writes
        public bool importer_is_a_model_importer;
        public bool meta_file_exists;
        public bool internal_id_to_name_table_present;
        public bool external_objects_table_present;
        public int table_entries;
        public int table_entries_for_game_objects;
        public int table_entries_for_meshes;
        public int table_entries_for_materials;
        public List<int> table_class_ids = new List<int>();
        public List<MetaRow> table = new List<MetaRow>();
        public List<string> meta_top_level_keys = new List<string>();
        // The table exactly as Unity spelled it, so a reader can see whether
        // "no entries" means an empty sequence, an empty mapping, or a key
        // Unity wrote and never filled.
        public List<string> internal_id_table_lines = new List<string>();

        // ---- is there a public API for it
        public List<string> public_api_members_naming_the_table = new List<string>();
        public List<string> public_api_members_naming_external_objects = new List<string>();
        public bool a_public_api_writes_the_table;
        public string only_working_path = String.Empty;

        // ---- what direct editing of the undocumented metadata does
        public bool direct_edit_changed_the_meta_on_disk;
        public bool direct_edit_survived_reimport;
        public string renamed_entry_target_name_before = String.Empty;
        public string renamed_entry_target_name_after = String.Empty;
        public bool renamed_entry_changed_a_visible_name;
        public bool renamed_entry_changed_a_local_file_id;
        public bool added_entry_created_an_object;
        public long added_entry_file_id;

        // ---- does it bind a stable machine identity to a human name
        public bool table_maps_identity_to_visible_name;
        public string what_the_table_maps = String.Empty;

        // ---- the survivals
        public bool table_survived_a_reexport;
        public int table_entries_after_reexport;
        public bool file_ids_unchanged_after_reexport;
        public bool table_survived_a_real_change;
        public int table_entries_after_a_real_change;
        public bool file_ids_unchanged_after_a_real_change;
        public bool table_rebuilt_after_deleting_the_meta;
        public bool file_ids_unchanged_after_deleting_the_meta;
        public string asset_guid_after_deleting_the_meta = String.Empty;

        // ---- must the sidecar exist first
        public bool sidecar_written_before_the_first_import_was_honoured;
        public string sidecar_guid_requested = String.Empty;
        public string sidecar_guid_observed = String.Empty;
        public int sidecar_table_entries_observed;

        public List<string> warnings = new List<string>();
        public int checks;
    }

    [Serializable]
    internal sealed class MetaRow
    {
        public int class_id;
        public long file_id;
        public string name = String.Empty;
        public string unity_type_at_this_file_id = String.Empty;
        public string unity_name_at_this_file_id = String.Empty;
    }

    internal static Report Execute(string planPath)
    {
        Plan plan = JsonUtility.FromJson<Plan>(File.ReadAllText(planPath));
        FerriteGraphCommon.Require(
            plan != null && File.Exists(plan.control), "the meta plan names no control document");
        FerriteGraphCommon.Require(File.Exists(plan.changed), "the meta plan names no changed document");
        FerriteGraphCommon.Require(File.Exists(plan.reexport), "the meta plan names no re-export");

        if (!AssetDatabase.IsValidFolder(AssetFolder))
        {
            AssetDatabase.CreateFolder("Assets", "Meta");
        }

        Report report = new Report { unity_version = Application.unityVersion };
        string assetPath = AssetFolder + "/meta-probe.fbx";
        string absolute = Path.GetFullPath(assetPath);
        string metaPath = absolute + ".meta";

        // ------------------------------------------------ what Unity writes
        File.Copy(plan.control, absolute, true);
        report.warnings = FerriteGraphCommon.Import(assetPath);
        FerriteGraphCommon.SettleImporter(assetPath, requireDefaultSort: false);
        AssetDatabase.SaveAssets();
        report.importer_is_a_model_importer =
            AssetImporter.GetAtPath(assetPath) is ModelImporter;
        report.meta_file_exists = File.Exists(metaPath);
        FerriteGraphCommon.Require(report.meta_file_exists, "Unity wrote no .meta for the import");

        string meta = File.ReadAllText(metaPath).Replace("\r\n", "\n");
        report.meta_top_level_keys = TopLevelKeys(meta);
        report.internal_id_to_name_table_present = meta.Contains("internalIDToNameTable:");
        report.internal_id_table_lines = TableLines(meta);
        report.external_objects_table_present = meta.Contains("externalObjects:");
        List<MetaRow> rows = ParseTable(meta);
        report.table_entries = rows.Count;
        report.table_class_ids = rows.Select(row => row.class_id).Distinct().OrderBy(id => id).ToList();
        report.table_entries_for_game_objects = rows.Count(row => row.class_id == GameObjectClassId);
        report.table_entries_for_meshes = rows.Count(row => row.class_id == MeshClassId);
        report.table_entries_for_materials = rows.Count(row => row.class_id == MaterialClassId);
        Annotate(assetPath, rows);
        report.table = rows;
        ++FerriteGraphCommon.Checks;

        // What the table actually relates. `internalIDToNameTable` maps a
        // local file identifier to a *name*, so what it can express is "the
        // object that used to be called X is now this identifier" — it is
        // Unity's own record of the names it already assigned, not a place to
        // put an identity a person never sees.
        report.table_maps_identity_to_visible_name = rows.Count > 0
            && rows.All(row => row.name.Length > 0);
        report.what_the_table_maps = rows.Count == 0
            ? "nothing: the table is absent or empty for this import"
            : "local_file_id <- unity_visible_name, per class id";

        // ------------------------------------------------ is there a public API
        report.public_api_members_naming_the_table = PublicMembersNaming("internalid", "nametable");
        report.public_api_members_naming_external_objects =
            PublicMembersNaming("externalobject", "remap");
        report.a_public_api_writes_the_table =
            report.public_api_members_naming_the_table.Count > 0;
        ++FerriteGraphCommon.Checks;

        // --------------------------------- what direct editing of it does
        //
        // Written against a real imported object rather than against a row
        // Unity produced, because Unity may have produced none: the question
        // is whether a *person* can create the mapping by hand, and that
        // question is the same either way.
        Dictionary<long, string> before = NamesByFileId(assetPath);
        Mesh target = AssetDatabase.LoadAllAssetsAtPath(assetPath).OfType<Mesh>()
            .OrderBy(item => item.name, StringComparer.Ordinal)
            .ThenBy(FerriteGraphCommon.LocalId)
            .FirstOrDefault();
        FerriteGraphCommon.Require(target != null, "the control published no Mesh to map");
        long targetId = FerriteGraphCommon.LocalId(target);
        report.renamed_entry_target_name_before = target.name;
        long invented = unchecked(targetId + 1);
        report.added_entry_file_id = invented;

        // Two rows: one that renames an object the import really published,
        // and one for an identifier nothing produced. "Unity honoured an entry
        // a person wrote" and "Unity rewrote an entry it had written itself"
        // are different answers, and this separates them.
        string edited = InsertRow(meta, MeshClassId, targetId, "fcad~directly~edited");
        edited = InsertRow(edited, MeshClassId, invented, "fcad~invented~row");
        File.WriteAllText(metaPath, edited, new UTF8Encoding(false));
        report.direct_edit_changed_the_meta_on_disk =
            File.ReadAllText(metaPath).Contains("fcad~directly~edited");
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        FerriteGraphCommon.Import(assetPath);

        string reread = File.ReadAllText(metaPath).Replace("\r\n", "\n");
        report.direct_edit_survived_reimport = reread.Contains("fcad~directly~edited");
        Dictionary<long, string> now = NamesByFileId(assetPath);
        report.renamed_entry_target_name_after =
            now.TryGetValue(targetId, out string landed)
                ? landed.Substring(landed.IndexOf(':') + 1)
                : "<absent>";
        report.renamed_entry_changed_a_visible_name =
            report.renamed_entry_target_name_after != report.renamed_entry_target_name_before;
        report.renamed_entry_changed_a_local_file_id = !SameIds(before, now);
        report.added_entry_created_an_object = now.ContainsKey(invented);
        report.only_working_path = report.a_public_api_writes_the_table
            ? "a public API writes the table"
            : report.renamed_entry_changed_a_visible_name
                ? "editing undocumented serialized metadata by hand"
                : "no measured path writes the table at all";
        ++FerriteGraphCommon.Checks;

        // -------------------------------------------------- the survivals
        // Restore Unity's own `.meta` before asking the survival questions, so
        // each of them is about the import and not about the edit above.
        AssetDatabase.DeleteAsset(assetPath);
        File.Copy(plan.control, absolute, true);
        FerriteGraphCommon.Import(assetPath);
        FerriteGraphCommon.SettleImporter(assetPath, requireDefaultSort: false);
        AssetDatabase.SaveAssets();
        Dictionary<long, string> baseline = NamesByFileId(assetPath);
        int baselineEntries = ParseTable(File.ReadAllText(metaPath).Replace("\r\n", "\n")).Count;

        File.Copy(plan.reexport, absolute, true);
        FerriteGraphCommon.Import(assetPath);
        AssetDatabase.SaveAssets();
        List<MetaRow> afterReexport = ParseTable(File.ReadAllText(metaPath).Replace("\r\n", "\n"));
        report.table_entries_after_reexport = afterReexport.Count;
        report.table_survived_a_reexport = afterReexport.Count == baselineEntries;
        report.file_ids_unchanged_after_reexport = SameIds(baseline, NamesByFileId(assetPath));
        ++FerriteGraphCommon.Checks;

        File.Copy(plan.changed, absolute, true);
        FerriteGraphCommon.Import(assetPath);
        AssetDatabase.SaveAssets();
        List<MetaRow> afterChange = ParseTable(File.ReadAllText(metaPath).Replace("\r\n", "\n"));
        report.table_entries_after_a_real_change = afterChange.Count;
        report.table_survived_a_real_change = afterChange.Count > 0;
        report.file_ids_unchanged_after_a_real_change = SameIds(baseline, NamesByFileId(assetPath));
        ++FerriteGraphCommon.Checks;

        // ---- deleting the `.meta` outright, which is what a fresh checkout
        // without the sidecar looks like.
        File.Copy(plan.control, absolute, true);
        FerriteGraphCommon.Import(assetPath);
        AssetDatabase.SaveAssets();
        Dictionary<long, string> restored = NamesByFileId(assetPath);
        File.Delete(metaPath);
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        FerriteGraphCommon.Import(assetPath);
        AssetDatabase.SaveAssets();
        report.table_rebuilt_after_deleting_the_meta =
            File.Exists(metaPath)
            && ParseTable(File.ReadAllText(metaPath).Replace("\r\n", "\n")).Count > 0;
        report.file_ids_unchanged_after_deleting_the_meta =
            SameIds(restored, NamesByFileId(assetPath));
        report.asset_guid_after_deleting_the_meta =
            FerriteGraphCommon.GuidToken(AssetDatabase.AssetPathToGUID(assetPath));
        ++FerriteGraphCommon.Checks;

        // -------------------------------- must the sidecar exist beforehand
        AssetDatabase.DeleteAsset(assetPath);
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        string sidecarPath = AssetFolder + "/meta-sidecar.fbx";
        string sidecarAbsolute = Path.GetFullPath(sidecarPath);
        string requestedGuid = "fcad00000000000000000000000e2b01";
        report.sidecar_guid_requested = FerriteGraphCommon.GuidToken(requestedGuid);
        File.WriteAllText(
            sidecarAbsolute + ".meta",
            Sidecar(requestedGuid),
            new UTF8Encoding(false));
        File.Copy(plan.control, sidecarAbsolute, true);
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        FerriteGraphCommon.Import(sidecarPath);
        AssetDatabase.SaveAssets();
        string observed = AssetDatabase.AssetPathToGUID(sidecarPath);
        report.sidecar_guid_observed = FerriteGraphCommon.GuidToken(observed);
        report.sidecar_written_before_the_first_import_was_honoured = observed == requestedGuid;
        List<MetaRow> sidecarRows =
            ParseTable(File.ReadAllText(sidecarAbsolute + ".meta").Replace("\r\n", "\n"));
        report.sidecar_table_entries_observed = sidecarRows.Count;
        ++FerriteGraphCommon.Checks;

        AssetDatabase.DeleteAsset(sidecarPath);
        AssetDatabase.DeleteAsset(assetPath);
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        return report;
    }

    // ------------------------------------------------------------- plumbing

    // The `internalIDToNameTable` block verbatim, up to the next key at the
    // same indentation. Recorded because "the table is empty" and "the table
    // is a key Unity wrote and never filled" read the same in a count.
    private static List<string> TableLines(string meta)
    {
        List<string> lines = new List<string>();
        string[] all = meta.Split('\n');
        int start = Array.FindIndex(
            all,
            line => line.TrimEnd().EndsWith("internalIDToNameTable:", StringComparison.Ordinal));
        if (start < 0)
        {
            return lines;
        }
        int indent = all[start].Length - all[start].TrimStart().Length;
        lines.Add(all[start].TrimEnd());
        for (int index = start + 1; index < all.Length; ++index)
        {
            string line = all[index];
            if (line.Trim().Length == 0)
            {
                continue;
            }
            int here = line.Length - line.TrimStart().Length;
            if (here <= indent && !line.TrimStart().StartsWith("-", StringComparison.Ordinal))
            {
                break;
            }
            lines.Add(line.TrimEnd());
        }
        return lines;
    }

    private static List<string> TopLevelKeys(string meta)
    {
        List<string> keys = new List<string>();
        foreach (string line in meta.Split('\n'))
        {
            if (line.Length == 0 || line[0] == ' ' || line[0] == '-' || line[0] == '%')
            {
                continue;
            }
            int colon = line.IndexOf(':');
            if (colon > 0)
            {
                keys.Add(line.Substring(0, colon));
            }
        }
        return keys.Distinct().OrderBy(key => key, StringComparer.Ordinal).ToList();
    }

    // `internalIDToNameTable` is a sequence of `{class id: file id}` to name.
    // Parsed by hand rather than with a YAML library, because the point of
    // this probe is what the bytes on disk say and a library that normalised
    // them would hide exactly the thing being measured.
    private static List<MetaRow> ParseTable(string meta)
    {
        List<MetaRow> rows = new List<MetaRow>();
        string[] lines = meta.Split('\n');
        int start = Array.FindIndex(
            lines, line => line.TrimEnd().EndsWith("internalIDToNameTable:", StringComparison.Ordinal));
        if (start < 0)
        {
            return rows;
        }
        int indent = lines[start].Length - lines[start].TrimStart().Length;
        MetaRow current = null;
        for (int index = start + 1; index < lines.Length; ++index)
        {
            string line = lines[index];
            if (line.Trim().Length == 0)
            {
                continue;
            }
            int here = line.Length - line.TrimStart().Length;
            string trimmed = line.Trim();
            if (here <= indent && !trimmed.StartsWith("-", StringComparison.Ordinal))
            {
                break;
            }
            if (trimmed.StartsWith("- first:", StringComparison.Ordinal))
            {
                current = new MetaRow();
                rows.Add(current);
                continue;
            }
            if (current == null)
            {
                continue;
            }
            if (trimmed.StartsWith("second:", StringComparison.Ordinal))
            {
                current.name = trimmed.Substring("second:".Length).Trim();
                continue;
            }
            int colon = trimmed.IndexOf(':');
            if (colon > 0
                && int.TryParse(
                    trimmed.Substring(0, colon),
                    NumberStyles.Integer,
                    CultureInfo.InvariantCulture,
                    out int classId)
                && long.TryParse(
                    trimmed.Substring(colon + 1).Trim(),
                    NumberStyles.Integer,
                    CultureInfo.InvariantCulture,
                    out long fileId))
            {
                current.class_id = classId;
                current.file_id = fileId;
            }
        }
        return rows;
    }

    // Unity may write the key with an empty sequence after it, or with `[]`
    // on the same line. Both are handled, because which one it wrote is the
    // thing being measured and a writer that only understood one would report
    // "there is no table" for the other.
    private static string InsertRow(string meta, int classId, long fileId, string name)
    {
        int key = meta.IndexOf("internalIDToNameTable:", StringComparison.Ordinal);
        if (key < 0)
        {
            return meta;
        }
        int lineEnd = meta.IndexOf('\n', key);
        if (lineEnd < 0)
        {
            return meta;
        }
        string keyLine = meta.Substring(key, lineEnd - key);
        if (keyLine.TrimEnd().EndsWith("[]", StringComparison.Ordinal)
            || keyLine.TrimEnd().EndsWith("{}", StringComparison.Ordinal))
        {
            meta = meta.Substring(0, key) + "internalIDToNameTable:" + meta.Substring(lineEnd);
            lineEnd = meta.IndexOf('\n', key);
        }
        int insert = lineEnd + 1;
        string row = "  - first:\n      "
            + classId.ToString(CultureInfo.InvariantCulture) + ": "
            + fileId.ToString(CultureInfo.InvariantCulture) + "\n    second: " + name + "\n";
        return meta.Substring(0, insert) + row + meta.Substring(insert);
    }

    private static void Annotate(string assetPath, List<MetaRow> rows)
    {
        Dictionary<long, UnityEngine.Object> byId = new Dictionary<long, UnityEngine.Object>();
        foreach (UnityEngine.Object item in AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null))
        {
            byId[FerriteGraphCommon.LocalId(item)] = item;
        }
        foreach (MetaRow row in rows)
        {
            if (byId.TryGetValue(row.file_id, out UnityEngine.Object found))
            {
                row.unity_type_at_this_file_id = found.GetType().Name;
                row.unity_name_at_this_file_id = found.name;
            }
            else
            {
                row.unity_type_at_this_file_id = "<no object at this identifier>";
                row.unity_name_at_this_file_id = "<absent>";
            }
            ++FerriteGraphCommon.Checks;
        }
    }

    // Every phase enumerates the whole import, so a `.meta` conclusion rests
    // on a list of objects that was really walked rather than on two numbers.
    private static Dictionary<long, string> NamesByFileId(string assetPath)
    {
        FerriteGraphCommon.RecordSubassets(assetPath);
        Dictionary<long, string> result = new Dictionary<long, string>();
        foreach (UnityEngine.Object item in AssetDatabase.LoadAllAssetsAtPath(assetPath)
            .Where(item => item != null))
        {
            result[FerriteGraphCommon.LocalId(item)] = item.GetType().Name + ":" + item.name;
        }
        return result;
    }

    private static bool SameIds(Dictionary<long, string> left, Dictionary<long, string> right)
    {
        return left.Count == right.Count
            && left.All(entry => right.TryGetValue(entry.Key, out string value) && value == entry.Value);
    }

    // Every public member of the importer types whose name mentions the thing
    // being looked for. A list, not a yes/no, so a reader can see what was
    // searched and a future editor that adds one is visible as a difference.
    private static List<string> PublicMembersNaming(params string[] fragments)
    {
        List<string> found = new List<string>();
        foreach (Type type in new[] { typeof(AssetImporter), typeof(ModelImporter) })
        {
            foreach (MemberInfo member in type.GetMembers(
                BindingFlags.Public | BindingFlags.Instance | BindingFlags.Static
                    | BindingFlags.DeclaredOnly))
            {
                string lowered = member.Name.ToLowerInvariant();
                if (fragments.Any(fragment => lowered.Contains(fragment)))
                {
                    found.Add(type.Name + "." + member.Name);
                }
            }
        }
        ++FerriteGraphCommon.Checks;
        return found.Distinct().OrderBy(item => item, StringComparer.Ordinal).ToList();
    }

    private static string Sidecar(string guid)
    {
        // The smallest `.meta` Unity accepts for a model, written by hand so
        // the question "must the sidecar exist before the first import" is
        // asked with a sidecar that existed before the first import.
        return "fileFormatVersion: 2\nguid: " + guid + "\nModelImporter:\n"
            + "  serializedVersion: 22200\n  internalIDToNameTable:\n"
            + "  - first:\n      43: 4300000\n    second: fcad~sidecar~mesh\n"
            + "  externalObjects: {}\n  materials:\n    materialImportMode: 2\n"
            + "  animations:\n    legacyGenerateAnimations: 4\n"
            + "  userData: \n  assetBundleName: \n  assetBundleVariant: \n";
    }
}
