#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1e2a runner.
#
# The control's bytes come from the production writer through the
# `fbx_channel_documents` example, which calls `write_fbx_ascii_7400` and
# nothing else. Every candidate's bytes are those bytes put through the
# measurement-only rewriter, which renames objects and adds custom properties
# and does nothing else. No second serializer exists, and the report says for
# every file which of the two produced it.
#
# The independent `ufbx` reader is given exactly the files the editor is about
# to import — the same paths, not copies — and both programs hash what they
# opened, so "the oracle read a different file" is a refusal.
#
# Two editor modes, because one candidate is not a property of a file at all.
# `vanilla` is stock Unity. `companion` is the same editor with the FerriteCAD
# companion postprocessor renaming objects from the designations the file
# carries, and it re-measures the control so the plugin can be shown to leave a
# document without designations exactly as it found it.
#
# Each mode runs in freshly created temporary projects outside the repository,
# twice, and the two canonical reports must be byte-identical. Nothing imported
# is left behind: no `.fbx`, no `.meta`, no `Library`, no Unity project.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
root="$(cd "$tool/../.." && pwd)"
smoke="$root/tools/unity-fbx-smoke"
identity="$root/tools/unity-fbx-identity"
unity="${UNITY_EXECUTABLE:-/Applications/Unity/Hub/Editor/6000.4.10f1/Unity.app/Contents/MacOS/Unity}"
record=0
expect_durable=0
# Skips the two byte-for-byte comparisons with the committed measurement, so a
# run is judged by what the gates understand rather than by "these bytes are
# not the recorded bytes". The mutation campaign uses it: a mutant killed only
# because the report changed has not been caught by any check that knows what
# is wrong with it.
no_expected=0
runs=2
modes=(vanilla companion)

while [ "$#" -gt 0 ]; do
  case "$1" in
    --unity) unity="$2"; shift 2 ;;
    --record) record=1; shift ;;
    --expect-durable-join) expect_durable=1; shift ;;
    --no-expected) no_expected=1; shift ;;
    --runs) runs="$2"; shift 2 ;;
    --mode) modes=("$2"); shift 2 ;;
    *) echo "usage: $0 [--unity PATH] [--record] [--expect-durable-join] [--no-expected] [--runs N] [--mode M]" >&2; exit 2 ;;
  esac
done

if [ ! -x "$unity" ]; then
  echo "Unity executable is not executable: $unity" >&2
  exit 1
fi
version="$($unity -version 2>&1 | tr -d '\r' | tail -1)"
if [ "$version" != "6000.4.10f1" ]; then
  echo "expected Unity 6000.4.10f1, measured: $version" >&2
  exit 1
fi

output="$tool/measurement-output"
rm -rf "$output"
mkdir -p "$output"
staging="$output/documents"
mkdir -p "$staging/production"

workspace="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-channel.XXXXXX")"
cleanup() {
  local status=$?
  rm -rf "$workspace"
  exit "$status"
}
trap cleanup EXIT INT TERM

# ---------------------------------------------------------------- the bytes
documents="$(cd "$root" && cargo build -p ferritecad-export --example fbx_channel_documents \
  --message-format=json 2>/dev/null \
  | jq -r 'select(.reason == "compiler-artifact")
           | select(.target.name == "fbx_channel_documents")
           | .executable // empty' \
  | head -1)"
if [ -z "$documents" ] || [ ! -x "$documents" ]; then
  echo "the channel document writer was not built" >&2
  exit 1
fi
"$documents" "$staging/production" | tee "$output/documents.log"

for candidate in a-control b-ordinal b-occurrence c-property; do
  "$here/rewrite_channel.py" --documents "$staging/production" \
    --output "$staging/$candidate" --candidate "$candidate"
done
"$here/rewrite_channel.py" --documents "$staging/production" \
  --output "$staging/names" --names

# The control has to be the production bytes themselves, so that is checked
# rather than trusted to the rewriter's copy path.
for file in "$staging/production"/*.fbx; do
  if ! cmp -s "$file" "$staging/a-control/$(basename "$file")"; then
    echo "the control is not the production writer's bytes: $(basename "$file")" >&2
    exit 1
  fi
done
echo "FCAD_CHANNEL_CONTROL_IS_PRODUCTION_BYTES"

# ------------------------------------------------------- the independent read
cache="$("$smoke/scripts/fetch_ufbx.sh")"
reader="$output/read_channels"
clang -std=c11 -O2 -Wall -Wextra -Werror \
  -I "$cache" "$here/read_channels.c" "$cache/ufbx.c" -lm -o "$reader"

oracle="$output/channel-oracle-report.json"
"$reader" \
  "$staging"/a-control/*.fbx \
  "$staging"/b-ordinal/*.fbx \
  "$staging"/b-occurrence/*.fbx \
  "$staging"/c-property/*.fbx \
  "$staging"/names/*.fbx > "$oracle"
echo "FCAD_CHANNEL_ORACLE_EXECUTED"

for mode in "${modes[@]}"; do
  "$here/make_plan.py" --staging "$staging" --mode "$mode" \
    --output "$output/$mode-plan.json"
  "$here/make_plan.py" --staging "$staging" --mode "$mode" --basenames \
    --output "$output/$mode-plan-committed.json"
done

# ------------------------------------------------------------- the editor
run_in_fresh_project() {
  local mode="$1"
  local index="$2"
  local project="$workspace/$mode-$index"
  mkdir -p "$project/Assets/Editor"
  cp -R "$smoke/ProjectSettings" "$project/ProjectSettings"
  cp -R "$smoke/Packages" "$project/Packages"
  cp "$tool/Editor"/*.cs "$project/Assets/Editor/"

  local report="$output/$mode-report-$index.json"
  local log="$output/unity-$mode-$index.log"
  local arguments=(
    -batchmode -nographics -quit
    -projectPath "$project"
    -executeMethod FerriteChannelIdentity.Run
    -fcadPlan "$output/$mode-plan.json"
    -fcadOutput "$report"
    -logFile "$log"
  )
  if [ "$mode" = "companion" ]; then
    arguments+=(-fcadCompanion)
  fi
  if [ "$expect_durable" -eq 1 ]; then
    arguments+=(-fcadExpectDurableJoin)
  fi
  if [ "$record" -eq 0 ] && [ "$expect_durable" -eq 0 ] && [ "$no_expected" -eq 0 ]; then
    arguments+=(-fcadExpected "$tool/expected/$mode-report.json")
  fi
  set +e
  "$unity" "${arguments[@]}"
  local status=$?
  set -e
  if ! "$smoke/scripts/verify_unity_run.py" \
    --log "$log" \
    --report "$report" \
    --exit-status "$status" \
    --anchor FCAD_CHANNEL_IDENTITY \
    --min-checks 500
  then
    sed -n '1,400p' "$log" >&2
    exit 1
  fi
  # Deleted here, so the next run cannot inherit an AssetDatabase, an import
  # cache or a GUID from it.
  rm -rf "$project"
}

for mode in "${modes[@]}"; do
  index=1
  while [ "$index" -le "$runs" ]; do
    run_in_fresh_project "$mode" "$index"
    index=$((index + 1))
  done
  index=2
  while [ "$index" -le "$runs" ]; do
    if ! cmp -s "$output/$mode-report-1.json" "$output/$mode-report-$index.json"; then
      echo "two clean Unity projects produced different canonical $mode reports" >&2
      diff <(python3 -m json.tool "$output/$mode-report-1.json") \
           <(python3 -m json.tool "$output/$mode-report-$index.json") | head -60 >&2
      exit 1
    fi
    index=$((index + 1))
  done
  echo "FCAD_CHANNEL_REPEATABLE_ACROSS_${runs}_CLEAN_PROJECTS mode=$mode"
done

if [ "$record" -eq 1 ]; then
  mkdir -p "$tool/expected"
  for mode in "${modes[@]}"; do
    cp "$output/$mode-report-1.json" "$tool/expected/$mode-report.json"
    cp "$output/$mode-plan-committed.json" "$tool/expected/$mode-plan.json"
  done
  cp "$oracle" "$tool/expected/channel-oracle-report.json"
  echo "recorded $tool/expected"
fi

# ---------------------------------------------------------------- the join
if [ "${#modes[@]}" -eq 2 ]; then
  join="$output/channel-decision.json"
  verify=(
    "$here/verify_channel.py"
    --vanilla "$output/vanilla-report-1.json"
    --companion "$output/companion-report-1.json"
    --oracle "$oracle"
    --vanilla-plan "$output/vanilla-plan.json"
    --companion-plan "$output/companion-plan.json"
    --emit "$join"
  )
  if [ "$record" -eq 0 ] && [ "$expect_durable" -eq 0 ] && [ "$no_expected" -eq 0 ]; then
    verify+=(--expected "$tool/expected/channel-decision.json")
  fi
  "${verify[@]}"
  if [ "$record" -eq 1 ]; then
    cp "$join" "$tool/expected/channel-decision.json"
    echo "recorded $tool/expected/channel-decision.json"
  fi
fi

"$identity/scripts/check_repository_clean.sh"
