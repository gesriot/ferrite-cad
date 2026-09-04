#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1e1 mutation harness.
#
# Two halves. The controls prove the harness itself refuses the ways a
# mutation campaign can be fake: an anchor that matches nothing, an anchor that
# matches twice, a backup left over from a previous run, a probe that does not
# compile, a Unity report with no execution behind it, and a run that checked
# nothing. Then real defects are put into the real Unity probe, compiled and
# run against the real editor.
#
# A refusal caused by a missing editor is neither a kill nor a survivor: it is
# a run that measured nothing, and it is reported as that.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
probe="$tool/Editor/FerriteFbxIdentity.cs"
backup="$probe.mutbak"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-identity-mutations.XXXXXX")"
unity="${UNITY_EXECUTABLE:-/Applications/Unity/Hub/Editor/6000.4.10f1/Unity.app/Contents/MacOS/Unity}"

cleanup() {
  local status=$?
  if [ -f "$backup" ]; then
    cp "$backup" "$probe"
    rm "$backup"
    touch "$probe"
  fi
  rm -rf "$temporary"
  exit "$status"
}
trap cleanup EXIT INT TERM

replace_once() {
  local file="$1" old="$2" new="$3"
  local count
  count="$(FCAD_OLD="$old" perl -0ne '$o=$ENV{FCAD_OLD};$n=0;$p=0;while(($p=index($_,$o,$p))>=0){$n++;$p+=length($o)}print $n' "$file")"
  if [ "$count" -ne 1 ]; then
    echo "anchor matched $count times, expected one" >&2
    return 1
  fi
  FCAD_OLD="$old" FCAD_NEW="$new" perl -0pi -e '$o=$ENV{FCAD_OLD};$n=$ENV{FCAD_NEW};$p=index($_,$o);substr($_,$p,length($o),$n)' "$file"
}

# ------------------------------------------------------------- the controls
if find "$tool/Editor" "$here" -name '*.mutbak' -print -quit | grep -q .; then
  echo "stale mutation backup" >&2
  exit 1
fi
printf 'one\n' > "$temporary/one"
if replace_once "$temporary/one" missing x 2>/dev/null; then
  echo "anchor miss accepted" >&2
  exit 1
fi
printf 'twice twice\n' > "$temporary/twice"
if replace_once "$temporary/twice" twice x 2>/dev/null; then
  echo "multiple anchor accepted" >&2
  exit 1
fi
touch "$temporary/stale.mutbak"
if ! find "$temporary" -name '*.mutbak' -print -quit | grep -q .; then
  echo "stale backup control missed" >&2
  exit 1
fi
rm "$temporary/stale.mutbak"
echo "harness controls: anchor miss, multiple matches and stale backup refused"

verify_run="$tool/../unity-fbx-smoke/scripts/verify_unity_run.py"
cp "$tool/expected/identity-report.json" "$temporary/prewritten.json"
: > "$temporary/no-unity.log"
if "$verify_run" --log "$temporary/no-unity.log" --report "$temporary/prewritten.json" \
  --expected "$tool/expected/identity-report.json" --exit-status 0 \
  --anchor FCAD_FBX_IDENTITY --min-checks 200 >/dev/null 2>&1; then
  echo "prewritten report without Unity execution accepted" >&2
  exit 1
fi
printf 'FCAD_FBX_IDENTITY_EXECUTED checks=0\n' > "$temporary/zero.log"
python3 - "$temporary/prewritten.json" <<'PY'
import json, sys
path = sys.argv[1]
data = json.load(open(path, encoding="utf-8"))
data["checks"] = 0
open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(data) + "\n")
PY
if "$verify_run" --log "$temporary/zero.log" --report "$temporary/prewritten.json" \
  --exit-status 0 --anchor FCAD_FBX_IDENTITY --min-checks 200 >/dev/null 2>&1; then
  echo "zero-check Unity run accepted" >&2
  exit 1
fi
echo "harness controls: prewritten report and zero-check run refused"

# The repository-cleanliness gate has to refuse something, or it is decoration.
stray="$tool/../../fcad-identity-stray.fbx"
touch "$stray"
if "$here/check_repository_clean.sh" >/dev/null 2>&1; then
  rm -f "$stray"
  echo "an imported file left in the repository was accepted" >&2
  exit 1
fi
rm -f "$stray"
"$here/check_repository_clean.sh" >/dev/null
echo "harness control: an imported file left in the repository is refused"

# The semantic half, against the recorded canonical measurement.
"$here/run_identity_mutations.py"

if [ ! -x "$unity" ]; then
  echo "FCAD_IDENTITY_MUTATIONS_SKIPPED: no Unity 6000.4.10f1 on this machine" >&2
  echo "this is neither a kill nor a survivor; nothing was measured" >&2
  exit 3
fi

# ------------------------------------------------- the real probe mutations
original="$(shasum -a 256 "$probe" | cut -d' ' -f1)"

run_measurement() {
  "$here/run_identity_measurement.sh" --runs 1 > "$temporary/run.log" 2>&1
}

apply_and_expect_refusal() {
  local name="$1" old="$2" new="$3"
  cp -p "$probe" "$backup"
  if ! replace_once "$probe" "$old" "$new"; then
    echo "mutation $name did not apply" >&2
    exit 1
  fi
  set +e
  run_measurement
  local status=$?
  set -e
  cp "$backup" "$probe"
  rm "$backup"
  touch "$probe"
  if [ "$(shasum -a 256 "$probe" | cut -d' ' -f1)" != "$original" ]; then
    echo "restoring the probe after $name changed its bytes" >&2
    exit 1
  fi
  if [ "$status" -eq 0 ]; then
    echo "survived unexpectedly: $name" >&2
    tail -30 "$temporary/run.log" >&2
    exit 1
  fi
  echo "killed: $name"
}

# A probe that does not compile is a failure of the campaign, not a win.
cp -p "$probe" "$backup"
replace_once "$probe" 'internal static class FerriteFbxIdentity' \
  'internal static class FerriteFbxIdentity this_will_not_compile'
set +e
run_measurement
compile_status=$?
set -e
cp "$backup" "$probe"
rm "$backup"
touch "$probe"
if [ "$compile_status" -eq 0 ]; then
  echo "a non-compiling probe was accepted" >&2
  exit 1
fi
if ! grep -q 'error CS' "$tool/measurement-output"/unity-identity-1.log; then
  echo "the non-compiling probe was refused for the wrong reason" >&2
  exit 1
fi
if grep -q 'FCAD_FBX_IDENTITY_EXECUTED' "$tool/measurement-output"/unity-identity-1.log; then
  echo "a non-compiling probe published an execution anchor" >&2
  exit 1
fi
echo "harness control: a non-compiling probe is refused and is not counted as a kill"

apply_and_expect_refusal \
  "a_verdict_that_only_asks_whether_something_resolved" \
  '        if (reference.semantic_after == reference.semantic_before)
        {
            return "same_semantic";
        }' \
  '        return "same_semantic";
        #pragma warning disable 0162
        if (reference.semantic_after == reference.semantic_before)
        {
            return "same_semantic";
        }
        #pragma warning restore 0162'

apply_and_expect_refusal \
  "a_placement_identified_by_its_display_name" \
  '        return "definition=" + node.DefinitionKey + ";occurrence="
            + occurrence.ToString(CultureInfo.InvariantCulture);' \
  '        return "name=" + node.Target.name
            + (occurrence < 0 ? "" : "");'

apply_and_expect_refusal \
  "the_importer_left_free_to_reorder_the_hierarchy_under_the_keys" \
  '        if (importer.sortHierarchyByName)
        {
            importer.sortHierarchyByName = false;' \
  '        if (false && importer.sortHierarchyByName)
        {
            importer.sortHierarchyByName = false;'

apply_and_expect_refusal \
  "the_saved_reference_never_reloaded_from_disk" \
  '        View after = BuildView(assetPath);
        result.after = Describe(after, assetPath);' \
  '        View after = before;
        result.after = Describe(after, assetPath);'

apply_and_expect_refusal \
  "the_local_file_identifier_taken_for_the_fbx_object_number" \
  '        return "definitions=[" + String.Join(",", definitions) + "];vertices="
            + holders[0].SharedMesh.vertexCount.ToString(CultureInfo.InvariantCulture);' \
  '        return "definitions=[" + String.Join(",", definitions) + "];vertices="
            + id.ToString(CultureInfo.InvariantCulture);'

apply_and_expect_refusal \
  "the_asset_own_script_line_counted_as_a_tracked_reference" \
  '            if (!line.TrimStart().StartsWith("- {fileID:", StringComparison.Ordinal))' \
  '            if (!line.TrimStart().StartsWith("m", StringComparison.Ordinal)
                && !line.TrimStart().StartsWith("- {fileID:", StringComparison.Ordinal))'

echo "mutation campaign: 6 compiled Unity probe mutants killed"

# The baseline again, whole, after every restoration.
"$here/run_identity_measurement.sh" --runs 1
echo "FCAD_IDENTITY_MUTATIONS_COMPLETE"
