#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1e2a mutation harness.
#
# Two halves. The controls prove the harness itself refuses the ways a mutation
# campaign can be fake: an anchor that matches nothing, an anchor that matches
# twice, a backup left over from a previous run, a probe that does not compile,
# a report with no execution behind it, a run that checked nothing, and an
# imported file left in the repository. Then real defects are put into the real
# document generator, the real channel rewriter and the real Unity probe,
# compiled, and run against the real editor.
#
# A refusal caused by a missing editor is neither a kill nor a survivor: it is a
# run that measured nothing, and it is reported as that.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
root="$(cd "$tool/../.." && pwd)"
identity="$root/tools/unity-fbx-identity"
unity="${UNITY_EXECUTABLE:-/Applications/Unity/Hub/Editor/6000.4.10f1/Unity.app/Contents/MacOS/Unity}"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-channel-mutations.XXXXXX")"

targets=(
  "$tool/Editor/FerriteChannelIdentity.cs"
  "$tool/Editor/FerriteChannelProperties.cs"
  "$here/rewrite_channel.py"
  "$here/run_channel_measurement.sh"
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

verify_run="$root/tools/unity-fbx-smoke/scripts/verify_unity_run.py"
cp "$tool/expected/vanilla-report.json" "$temporary/prewritten.json"
: > "$temporary/no-unity.log"
if "$verify_run" --log "$temporary/no-unity.log" --report "$temporary/prewritten.json" \
  --expected "$tool/expected/vanilla-report.json" --exit-status 0 \
  --anchor FCAD_CHANNEL_IDENTITY --min-checks 500 >/dev/null 2>&1; then
  echo "prewritten report without Unity execution accepted" >&2
  exit 1
fi
printf 'FCAD_CHANNEL_IDENTITY_EXECUTED checks=0\n' > "$temporary/zero.log"
python3 - "$temporary/prewritten.json" <<'PY'
import json, sys
path = sys.argv[1]
data = json.load(open(path, encoding="utf-8"))
data["checks"] = 0
open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(data) + "\n")
PY
if "$verify_run" --log "$temporary/zero.log" --report "$temporary/prewritten.json" \
  --exit-status 0 --anchor FCAD_CHANNEL_IDENTITY --min-checks 500 >/dev/null 2>&1; then
  echo "zero-check Unity run accepted" >&2
  exit 1
fi
echo "harness controls: prewritten report and zero-check run refused"

stray="$root/fcad-channel-stray.fbx"
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
"$here/run_channel_mutations.py"

if [ ! -x "$unity" ]; then
  echo "FCAD_CHANNEL_MUTATIONS_SKIPPED: no Unity 6000.4.10f1 on this machine" >&2
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
  "$here/run_channel_measurement.sh" "$@" > "$temporary/run.log" 2>&1
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
    'Refused:|FCAD_CHANNEL_IDENTITY_FAILURE|not the production writer|different canonical|did not run' \
    "$temporary/run.log" "$tool/measurement-output"/unity-*.log 2>/dev/null \
    | head -1 | cut -c1-170)"
}

# A probe that does not compile is a failure of the campaign, not a win.
probe="$tool/Editor/FerriteChannelIdentity.cs"
cp -p "$probe" "$probe.mutbak"
replace_once "$probe" 'internal static class FerriteChannelIdentity' \
  'internal static class FerriteChannelIdentity this_will_not_compile'
set +e
run_measurement --runs 1 --mode vanilla
compile_status=$?
set -e
restore
if [ "$compile_status" -eq 0 ]; then
  echo "a non-compiling probe was accepted" >&2
  exit 1
fi
if ! grep -q 'error CS' "$tool/measurement-output"/unity-vanilla-1.log; then
  echo "the non-compiling probe was refused for the wrong reason" >&2
  exit 1
fi
if grep -q 'FCAD_CHANNEL_IDENTITY_EXECUTED' "$tool/measurement-output"/unity-vanilla-1.log; then
  echo "a non-compiling probe published an execution anchor" >&2
  exit 1
fi
echo "harness control: a non-compiling probe is refused and is not counted as a kill"

# The source-qualified half of the identity, taken back out of the channel.
apply_and_expect_refusal \
  "the_imported_source_id_removed_from_the_channel" \
  "$here/rewrite_channel.py" \
  '            added = [
                ("FerriteCADSourceId", node["source"]),
                ("FerriteCADDefinitionId", node["definition_id"]),
            ]' \
  '            added = []' \
  --runs 1

# The control stops being the production writer's bytes.
apply_and_expect_refusal \
  "the_control_rewritten_instead_of_copied" \
  "$here/rewrite_channel.py" \
  '        if args.candidate == "a-control":' \
  '        if False and args.candidate == "a-control":' \
  --runs 1

# The independent reader is shown a file the editor never imports.
apply_and_expect_refusal \
  "the_oracle_given_a_different_file" \
  "$here/run_channel_measurement.sh" \
  '  "$staging"/c-property/*.fbx \' \
  '  "$staging"/b-occurrence/*.fbx \' \
  --runs 1

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
  --runs 1 --no-expected

# An identity that cannot name two definitions apart, tracked as if it could.
apply_and_expect_refusal \
  "an_ambiguous_join_tracked_as_if_it_were_not" \
  "$probe" \
  '        return view.Nodes
            .Where(other => Definition(other) == Definition(node))
            .Select(other => other.MeshLocalId)
            .Distinct()
            .Count() > 1;' \
  '        return false && view.Nodes
            .Where(other => Definition(other) == Definition(node))
            .Select(other => other.MeshLocalId)
            .Distinct()
            .Count() > 1;' \
  --runs 1 --no-expected

# The visible names, which are half the question, never recorded.
apply_and_expect_refusal \
  "the_visible_names_never_recorded" \
  "$probe" \
  '        summary.visible_node_names = view.Nodes
            .Skip(1)' \
  '        summary.visible_node_names = view.Nodes
            .Skip(view.Nodes.Count)' \
  --runs 1 --no-expected

# The companion package quietly not running, while the report still says it is
# the companion result.
apply_and_expect_refusal \
  "the_companion_rename_left_out" \
  "$tool/Editor/FerriteChannelProperties.cs" \
  '        get { return Environment.GetCommandLineArgs().Contains("-fcadCompanion"); }' \
  '        get { return false && Environment.GetCommandLineArgs().Contains("-fcadCompanion"); }' \
  --runs 1 --no-expected

# The one mutant only a second clean project can see: a report that carries the
# project's own GUIDs instead of tokens is identical to itself and different in
# every new project.
apply_and_expect_refusal \
  "the_project_guid_left_untokenised_in_the_report" \
  "$probe" \
  '            token = "<guid-" + GuidTokens.Count.ToString(CultureInfo.InvariantCulture) + ">";' \
  '            token = guid;' \
  --runs 2 --mode vanilla --no-expected

echo "mutation campaign: 8 compiled and scripted mutants killed"

# The baseline again, whole, after every restoration.
"$here/run_channel_measurement.sh" --runs 1
echo "FCAD_CHANNEL_MUTATIONS_COMPLETE"
