#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1e2b mutation harness.
#
# Two halves. The controls prove the harness itself refuses the ways a mutation
# campaign can be fake: an anchor that matches nothing, an anchor that matches
# twice, a backup left over from a previous run, a probe that does not compile,
# a report with no execution behind it, a run that checked nothing, and an
# imported file left in the repository. Then real defects are put into the real
# structural transformer, the real Unity probes and the real runner, compiled,
# and run against the real editor.
#
# A refusal caused by a missing editor is neither a kill nor a survivor: it is a
# run that measured nothing, and it is reported as that.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
root="$(cd "$tool/../.." && pwd)"
identity="$root/tools/unity-fbx-identity"
unity="${UNITY_EXECUTABLE:-/Applications/Unity/Hub/Editor/6000.4.10f1/Unity.app/Contents/MacOS/Unity}"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-graph-mutations.XXXXXX")"

targets=(
  "$tool/Editor/FerriteGraphProbe.cs"
  "$tool/Editor/FerriteGraphCommon.cs"
  "$tool/Editor/FerriteRemapProbe.cs"
  "$tool/Editor/FerriteScriptedProbe.cs"
  "$tool/Editor/FerriteSyntheticImporter.cs"
  "$here/rewrite_graph.py"
  "$here/run_graph_measurement.sh"
)

restore() {
  local file
  for file in "${targets[@]}"; do
    if [ -f "$file.mutbak" ]; then
      cp "$file.mutbak" "$file"
      rm "$file.mutbak"
      touch "$file"
    fi
  done
}

cleanup() {
  local status=$?
  restore
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
if find "$tool/Editor" "$tool/Runtime" "$here" -name '*.mutbak' -print -quit | grep -q .; then
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

verify_run="$root/tools/unity-fbx-smoke/scripts/verify_unity_run.py"
cp "$tool/expected/graph-report.json" "$temporary/prewritten.json"
: > "$temporary/no-unity.log"
if "$verify_run" --log "$temporary/no-unity.log" --report "$temporary/prewritten.json" \
  --expected "$tool/expected/graph-report.json" --exit-status 0 \
  --anchor FCAD_GRAPH_IDENTITY --min-checks 500 >/dev/null 2>&1; then
  echo "prewritten report without Unity execution accepted" >&2
  exit 1
fi
printf 'FCAD_GRAPH_IDENTITY_EXECUTED checks=0 mode=graph\n' > "$temporary/zero.log"
python3 - "$temporary/prewritten.json" <<'PY'
import json, sys
path = sys.argv[1]
data = json.load(open(path, encoding="utf-8"))
data["checks"] = 0
open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(data) + "\n")
PY
if "$verify_run" --log "$temporary/zero.log" --report "$temporary/prewritten.json" \
  --exit-status 0 --anchor FCAD_GRAPH_IDENTITY --min-checks 500 >/dev/null 2>&1; then
  echo "zero-check Unity run accepted" >&2
  exit 1
fi
echo "harness controls: prewritten report and zero-check run refused"

stray="$root/fcad-graph-stray.fbx"
touch "$stray"
if "$identity/scripts/check_repository_clean.sh" >/dev/null 2>&1; then
  rm -f "$stray"
  echo "an imported file left in the repository was accepted" >&2
  exit 1
fi
rm -f "$stray"
"$identity/scripts/check_repository_clean.sh" >/dev/null
echo "harness control: an imported file left in the repository is refused"

# The semantic half, against the recorded canonical measurement.
"$here/run_graph_mutations.py"

if [ ! -x "$unity" ]; then
  echo "FCAD_GRAPH_MUTATIONS_SKIPPED: no Unity 6000.4.10f1 on this machine" >&2
  echo "this is neither a kill nor a survivor; nothing was measured" >&2
  exit 3
fi

# ------------------------------------------------- the real source mutations
digest() { shasum -a 256 "$1" | cut -d' ' -f1; }

# The digest each mutated file must come back to. Kept in files rather than in
# an associative array: the macOS system bash is 3.2 and has none, and a
# restoration nobody checks is how a mutation campaign leaves a defect behind.
mkdir -p "$temporary/digests"
digest_slot() { printf '%s/digests/%s' "$temporary" "$(printf '%s' "$1" | tr '/.' '__')"; }
for file in "${targets[@]}"; do
  digest "$file" > "$(digest_slot "$file")"
done

run_measurement() {
  "$here/run_graph_measurement.sh" "$@" > "$temporary/run.log" 2>&1
}

apply_and_expect_refusal() {
  local name="$1" file="$2" old="$3" new="$4"
  shift 4
  cp -p "$file" "$file.mutbak"
  if ! replace_once "$file" "$old" "$new"; then
    echo "mutation $name did not apply" >&2
    exit 1
  fi
  set +e
  run_measurement "$@"
  local status=$?
  set -e
  restore
  if [ "$(digest "$file")" != "$(cat "$(digest_slot "$file")")" ]; then
    echo "restoring $file after $name changed its bytes" >&2
    exit 1
  fi
  if [ "$status" -eq 0 ]; then
    echo "survived unexpectedly: $name" >&2
    tail -40 "$temporary/run.log" >&2
    exit 1
  fi
  # The refusal itself, not just the exit code: a mutant killed for the wrong
  # reason is a mutant that survived the check it was written for.
  echo "killed: $name: $(grep -ahE \
    'refused:|FCAD_GRAPH_IDENTITY_FAILURE|not the production writer|different canonical|did not run' \
    "$temporary/run.log" "$tool/measurement-output"/unity-*.log 2>/dev/null \
    | head -1 | cut -c1-170)"
}

# A probe that does not compile is a failure of the campaign, not a win.
probe="$tool/Editor/FerriteGraphProbe.cs"
cp -p "$probe" "$probe.mutbak"
replace_once "$probe" 'internal static class FerriteGraphProbe' \
  'internal static class FerriteGraphProbe this_will_not_compile'
set +e
run_measurement --runs 1 --mode graph --variants g-flat --no-expected
compile_status=$?
set -e
restore
if [ "$compile_status" -eq 0 ]; then
  echo "a non-compiling probe was accepted" >&2
  exit 1
fi
if ! grep -q 'error CS' "$tool/measurement-output"/unity-graph-1.log; then
  echo "the non-compiling probe was refused for the wrong reason" >&2
  exit 1
fi
if grep -q 'FCAD_GRAPH_IDENTITY_EXECUTED' "$tool/measurement-output"/unity-graph-1.log; then
  echo "a non-compiling probe published an execution anchor" >&2
  exit 1
fi
echo "harness control: a non-compiling probe is refused and is not counted as a kill"

# The source-qualified half of the identity, taken back out of the graph.
apply_and_expect_refusal \
  "the_imported_source_id_removed_from_the_graph" \
  "$here/rewrite_graph.py" \
  '    properties = [
        ("FerriteCADSourceId", node["source"]),
        ("FerriteCADDefinitionId", node["definition_id"]),
        ("FerriteCADGraphRole", role),
    ]' \
  '    properties = [
        ("FerriteCADGraphRole", role),
    ]' \
  --runs 1 --mode graph --variants g-flat,g-flat-id --no-expected

# The control stops being the production writer's bytes.
apply_and_expect_refusal \
  "the_control_rewritten_instead_of_copied" \
  "$here/rewrite_graph.py" \
  '        if args.variant == "g-flat":' \
  '        if False and args.variant == "g-flat":' \
  --runs 1 --mode graph --variants g-flat,g-flat-id --no-expected

# The transformer moves a placement while re-pointing its geometry: the graph
# is arithmetically plausible and the part ends up somewhere else. The anchor
# is a Python string literal full of quotes and backslashes, so it is carried
# in files rather than through two layers of shell quoting.
cat > "$temporary/transform-old.txt" <<'ANCHOR'
        '\t\t\tP: "Lcl Translation", "Lcl Translation", "", "A", 0.0, 0.0, 0.0',
ANCHOR
cat > "$temporary/transform-new.txt" <<'ANCHOR'
        '\t\t\tP: "Lcl Translation", "Lcl Translation", "", "A", 7.0, 0.0, 0.0',
ANCHOR
apply_and_expect_refusal \
  "the_added_node_given_a_transform_of_its_own" \
  "$here/rewrite_graph.py" \
  "$(cat "$temporary/transform-old.txt")" \
  "$(cat "$temporary/transform-new.txt")" \
  --runs 1 --mode graph --variants g-flat,g-two-level --no-expected

# The independent reader is shown files the editor never imports.
apply_and_expect_refusal \
  "the_oracle_given_a_different_variant" \
  "$here/run_graph_measurement.sh" \
  '  oracle_inputs+=("$staging/$variant"/*.fbx)' \
  '  oracle_inputs+=("$staging/g-flat"/*.fbx)' \
  --runs 1 --mode graph --variants g-flat,g-two-level --no-expected

# A verdict that only asks whether something came back.
apply_and_expect_refusal \
  "a_verdict_that_only_asks_whether_something_resolved" \
  "$probe" \
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
        #pragma warning restore 0162' \
  --runs 1 --mode graph --variants g-flat --no-expected

# An identity that cannot name two definitions apart, tracked as if it could.
apply_and_expect_refusal \
  "an_ambiguous_join_tracked_as_if_it_were_not" \
  "$probe" \
  '        return view.Nodes
            .Where(other => Definition(other) == Definition(node) && other.MeshLocalId != -1L)
            .Select(other => other.MeshLocalId)
            .Distinct()
            .Count() > 1;' \
  '        return false && view.Nodes
            .Where(other => Definition(other) == Definition(node) && other.MeshLocalId != -1L)
            .Select(other => other.MeshLocalId)
            .Distinct()
            .Count() > 1;' \
  --runs 1 --mode graph --variants g-flat --no-expected

# The visible names, which are half the question, never recorded.
apply_and_expect_refusal \
  "the_visible_names_never_recorded" \
  "$probe" \
  '        summary.visible_node_names = view.Nodes
            .Skip(1)' \
  '        summary.visible_node_names = view.Nodes
            .Skip(view.Nodes.Count)' \
  --runs 1 --mode graph --variants g-flat --no-expected

# A shared mesh answered by identifier equality instead of by object identity.
apply_and_expect_refusal \
  "the_shared_mesh_answered_by_name_instead_of_by_object" \
  "$probe" \
  '            if (bearers.Select(node => ReferenceEqualityKey(node.SharedMesh)).Distinct().Count() == 1)' \
  '            if (bearers.Select(node => node.SharedMesh.name).Distinct().Count() == 1)' \
  --runs 1 --mode graph --variants g-flat,g-two-level --no-expected

# The `ScriptedImporter` identifier built from something other than the durable
# identity. Run with every mode, because the check that understands this defect
# lives in the join and the join needs all five reports.
apply_and_expect_refusal \
  "a_scripted_identifier_that_is_not_the_durable_identity" \
  "$tool/Editor/FerriteSyntheticImporter.cs" \
  '        return "fcad|mesh|" + definitionId;' \
  '        return "fcad|mesh|" + definitionId + "|display";' \
  --runs 1 --no-expected

# `AddRemap` measured on an object the transitions never touch, which is how a
# stale-content result turns into "nothing happened". Also a whole-measurement
# run, for the same reason.
apply_and_expect_refusal \
  "the_remap_measured_on_an_object_the_document_never_changes" \
  "$tool/Editor/FerriteRemapProbe.cs" \
  '            case "Material": return plan.material_to_remap;' \
  '            case "Material": return "Beta #3";' \
  --runs 1 --no-expected

# The one mutant only a second clean project can see: a report that carries the
# project's own GUIDs instead of tokens is identical to itself and different in
# every new project.
apply_and_expect_refusal \
  "the_project_guid_left_untokenised_in_the_report" \
  "$tool/Editor/FerriteGraphCommon.cs" \
  '            token = "<guid-" + Tokens.Count.ToString(CultureInfo.InvariantCulture) + ">";' \
  '            token = guid;' \
  --runs 2 --mode graph --variants g-flat --no-expected

echo "mutation campaign: 11 compiled and scripted mutants killed"

# The baseline again, whole, after every restoration.
"$here/run_graph_measurement.sh" --runs 1
echo "FCAD_GRAPH_MUTATIONS_COMPLETE"
