#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Mutants in the §22B-1e3b measurement itself: the Unity probe and the runner
# around it, compiled and run against the real editor.
#
# The semantic mutants against the recorded measurement live in
# `run_file_mutations.py`; those need no editor. These need one, because what
# they attack is the part of the measurement that only exists while Unity is
# running: whether the probe reads the channel at all, whether it compares the
# pair or compares a file with itself, and whether the runner shows the editor
# the bytes it claims to.
#
# Every run uses `--no-expected`, so a mutant dies from a check that understands
# the defect rather than from "these bytes are not the recorded bytes". Every
# edit is restored byte-for-byte, on success, on failure and on interruption. A
# mutant that fails to compile is refused rather than credited as a kill.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
probe="$tool/Editor/FerriteFileIdentity.cs"
properties="$tool/Editor/FerriteFileProperties.cs"
runner="$here/run_file_measurement.sh"

killed=0
backup=()
logs="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-file-mutants.XXXXXX")"

restore() {
  local file
  for file in "${backup[@]-}"; do
    [ -n "$file" ] || continue
    if [ -e "$file.mutbak" ]; then
      cp "$file.mutbak" "$file"
      rm "$file.mutbak"
    fi
  done
  backup=()
}
trap 'status=$?; restore; rm -rf "$logs"; exit "$status"' EXIT INT TERM

begin() {
  if [ "${#backup[@]}" -ne 0 ]; then
    echo "a mutation backup is already active" >&2
    exit 1
  fi
  local file
  for file in "$@"; do
    if [ -e "$file.mutbak" ]; then
      echo "stale mutation backup: $file.mutbak" >&2
      exit 1
    fi
    backup+=("$file")
    cp -p "$file" "$file.mutbak"
  done
}

replace_once() {
  local file="$1"
  local old="$2"
  local new="$3"
  local count
  count="$(FCAD_OLD="$old" perl -0ne '
    $old = $ENV{FCAD_OLD}; $at = 0; $count = 0;
    while (($found = index($_, $old, $at)) >= 0) {
      ++$count; $at = $found + length($old);
    }
    print $count;
  ' "$file")"
  if [ "$count" -ne 1 ]; then
    echo "anchor in $file matched $count times, expected once" >&2
    exit 1
  fi
  FCAD_OLD="$old" FCAD_NEW="$new" perl -0pi -e '
    $old = $ENV{FCAD_OLD}; $new = $ENV{FCAD_NEW};
    $at = index($_, $old); substr($_, $at, length($old), $new);
  ' "$file"
}

run_measurement() {
  "$runner" --no-expected --runs 1
}

expect_refusal() {
  local name="$1"
  local reason="${2:-}"
  set +e
  run_measurement >"$logs/mutant.log" 2>&1
  local status=$?
  set -e
  restore
  if [ "$status" -eq 0 ]; then
    echo "survived unexpectedly: $name" >&2
    exit 1
  fi
  if grep -q 'error CS' "$tool/measurement-output"/unity-file-1.log 2>/dev/null; then
    echo "compile refusal (not a runtime kill): $name" >&2
    exit 1
  fi
  if [ -n "$reason" ] && ! grep -Fq "$reason" "$tool/measurement-output"/unity-file-1.log; then
    echo "refused for the wrong reason: $name (expected: $reason)" >&2
    exit 1
  fi
  echo "killed against the real editor: $name"
  killed=$((killed + 1))
}

# --------------------------------------------------------- harness controls

# A probe that does not compile is refused and is not counted.
begin "$probe"
replace_once "$probe" 'private static int checks;' 'private static int checks = this is not C#;'
set +e
run_measurement >"$logs/control.log" 2>&1
control=$?
set -e
restore
if [ "$control" -eq 0 ]; then
  echo "a non-compiling probe was accepted" >&2
  exit 1
fi
if ! grep -q 'error CS' "$tool/measurement-output"/unity-file-1.log 2>/dev/null; then
  echo "the non-compiling probe was refused for the wrong reason" >&2
  exit 1
fi
if grep -q 'FCAD_FILE_IDENTITY_EXECUTED' "$tool/measurement-output"/unity-file-1.log 2>/dev/null; then
  echo "a non-compiling probe published an execution anchor" >&2
  exit 1
fi
echo "harness control: a non-compiling probe is refused and is not counted as a kill"

# ---------------------------------------------------------------- mutants
#
# Every one of these is a defect the measurement must notice, rather than a
# question removed from it: deleting an assertion about a file that satisfies it
# changes nothing and would survive for the right reason, which is not a
# mutation campaign. So each mutant makes the measurement look at something
# that really is wrong.

# The file the channel is asserted on is the one that has no channel.
begin "$probe"
replace_once "$probe" \
  '        FileReport current = Measure(source, CurrentFile);' \
  '        FileReport current = Measure(source, LegacyFile);'
expect_refusal the_current_file_is_actually_the_one_without_a_channel

# The control of the pair is a file whose designations really did move, so
# "nothing else moved" is being asserted against a moving target.
begin "$probe"
replace_once "$probe" \
  '        FileReport legacy = Measure(source, LegacyFile);' \
  '        FileReport legacy = Measure(source, RenamedFile);'
expect_refusal the_pair_control_is_a_file_that_really_did_change

# The properties callback records nothing, so every value the probe reports is
# an absence it never noticed.
begin "$properties"
replace_once "$properties" \
  '        Seen[target] = properties;' \
  '        Seen.Remove(target);'
expect_refusal the_property_callback_records_nothing

# A successful previous project must not supply the capture a new import
# failed to publish. This survived before the review fix: the global temp
# cache supplied every value and all 78 original checks still passed.
run_measurement >"$logs/capture-baseline.log" 2>&1
begin "$properties"
replace_once "$properties" \
  '        File.WriteAllText(cache, text.ToString(), new UTF8Encoding(false));' \
  '        // Mutant: no property capture is published by this import.'
expect_refusal a_previous_project_supplies_the_missing_capture \
  'the measured import published no fresh property capture'

# The rename variant is the measured file, so the one question the channel
# exists to answer is asked of a rename that never happened.
begin "$probe"
replace_once "$probe" \
  '        FileReport renamed = Measure(source, RenamedFile);' \
  '        FileReport renamed = Measure(source, CurrentFile);'
expect_refusal the_rename_variant_is_a_rename_that_never_happened

# The join across the rename is made by name rather than by identity, which is
# the one thing the channel exists so that nobody has to do.
begin "$probe"
replace_once "$probe" \
  '                by_identity[node.occurrence_id] = node;' \
  '                by_identity[node.name] = node;'
replace_once "$probe" \
  '            if (!by_identity.TryGetValue(node.occurrence_id, out other))' \
  '            if (!by_identity.TryGetValue(node.name, out other))'
expect_refusal the_rename_join_is_by_name_instead_of_by_identity

# The editor is shown bytes the production writer did not produce. What must
# notice is the runner's comparison with the recorded digests, before Unity is
# started at all.
begin "$runner"
# The replacement is shell source for the runner, expanded only in that run.
# shellcheck disable=SC2016
replace_once "$runner" \
  '"$artefacts" "$staging" | tee "$output/artefacts.log"' \
  $'"$artefacts" "$staging" | tee "$output/artefacts.log"\nprintf \'\\n\' >>"$staging/fcad-measured.fbx"'
expect_refusal the_editor_shown_bytes_the_production_writer_did_not_produce

echo "file probe campaign: $killed mutants killed against the real editor"
