#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1e2b runner.
#
# The control's bytes come from the production writer through the
# `fbx_channel_documents` example, which calls `write_fbx_ascii_7400` and
# nothing else. Every graph variant's bytes are those bytes put through the
# measurement-only structural transformer, which adds `Model` blocks, adds and
# re-points `OO` connections, adjusts the two `Definitions` counts and adds
# custom properties, and does nothing else. There is no second serializer, and
# the independent `ufbx` oracle compares every variant's geometry arrays,
# material colours, node transforms and existing object numbers with the
# control's rather than taking that on trust.
#
# The independent reader is given exactly the files the editor is about to
# import — the same paths, not copies — and both programs hash what they
# opened, so "the oracle read a different file" is a refusal.
#
# Five editor modes, because three of the four questions change what the editor
# is. `graph` is stock Unity importing FBX. `meta` edits serialized importer
# metadata. `remap` puts external assets in the project. `scripted` registers a
# `ScriptedImporter` for a test extension. `fbxclaim` is the one project where
# a `ScriptedImporter` claims `fbx`, kept apart so it cannot make the other
# four measurements of itself.
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
expect_contract=0
# Skips the byte-for-byte comparisons with the committed measurement, so a run
# is judged by what the gates understand rather than by "these bytes are not
# the recorded bytes". The mutation campaign uses it: a mutant killed only
# because the report changed has not been caught by any check that knows what
# is wrong with it.
no_expected=0
runs=2
modes=(graph meta remap scripted fbxclaim)
variants="g-flat,g-flat-id,g-carrier,g-carrier-detached,g-two-level"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --unity) unity="$2"; shift 2 ;;
    --record) record=1; shift ;;
    --expect-full-contract) expect_contract=1; shift ;;
    --no-expected) no_expected=1; shift ;;
    --runs) runs="$2"; shift 2 ;;
    --mode) modes=("$2"); shift 2 ;;
    --variants) variants="$2"; shift 2 ;;
    *)
      echo "usage: $0 [--unity PATH] [--record] [--expect-full-contract]" \
           "[--no-expected] [--runs N] [--mode M] [--variants LIST]" >&2
      exit 2 ;;
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

workspace="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-graph.XXXXXX")"
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
  echo "the document writer was not built" >&2
  exit 1
fi
"$documents" "$staging/production" | tee "$output/documents.log"

IFS=',' read -r -a variant_list <<< "$variants"
for variant in "${variant_list[@]}"; do
  "$here/rewrite_graph.py" --documents "$staging/production" \
    --output "$staging/$variant" --variant "$variant"
done

# The control has to be the production bytes themselves, so that is checked
# rather than trusted to the transformer's copy path.
for file in "$staging/production"/*.fbx; do
  if ! cmp -s "$file" "$staging/g-flat/$(basename "$file")"; then
    echo "the control is not the production writer's bytes: $(basename "$file")" >&2
    exit 1
  fi
done
echo "FCAD_GRAPH_CONTROL_IS_PRODUCTION_BYTES"

# ------------------------------------------------------- the independent read
cache="$("$smoke/scripts/fetch_ufbx.sh")"
reader="$output/read_graphs"
clang -std=c11 -O2 -Wall -Wextra -Werror \
  -I "$cache" "$here/read_graphs.c" "$cache/ufbx.c" -lm -o "$reader"

oracle="$output/graph-oracle-report.json"
oracle_inputs=()
for variant in "${variant_list[@]}"; do
  oracle_inputs+=("$staging/$variant"/*.fbx)
done
"$reader" "${oracle_inputs[@]}" > "$oracle"
echo "FCAD_GRAPH_ORACLE_EXECUTED"

for mode in "${modes[@]}"; do
  "$here/make_plan.py" --staging "$staging" --mode "$mode" --variants "$variants" \
    --output "$output/$mode-plan.json"
  "$here/make_plan.py" --staging "$staging" --mode "$mode" --variants "$variants" --basenames \
    --output "$output/$mode-plan-committed.json"
done

# ------------------------------------------------------------- the editor
run_in_fresh_project() {
  local mode="$1"
  local index="$2"
  local project="$workspace/$mode-$index"
  mkdir -p "$project/Assets/Editor" "$project/Assets/Runtime"
  cp -R "$smoke/ProjectSettings" "$project/ProjectSettings"
  cp -R "$smoke/Packages" "$project/Packages"
  cp "$tool/Editor"/*.cs "$project/Assets/Editor/"
  # Outside `Assets/Editor`, because Unity refuses to attach an editor script
  # to a `GameObject` and part E's join depends on attaching one.
  cp "$tool/Runtime"/*.cs "$project/Assets/Runtime/"
  # Only this one mode compiles an importer that claims `fbx`. Every other
  # mode imports `.fbx` files, and one that had this script in it would be
  # measuring this script.
  if [ "$mode" = "fbxclaim" ]; then
    cp "$tool/EditorFbxClaim"/*.cs "$project/Assets/Editor/"
  fi

  local report="$output/$mode-report-$index.json"
  local log="$output/unity-$mode-$index.log"
  local arguments=(
    -batchmode -nographics -quit
    -projectPath "$project"
    -executeMethod FerriteGraphIdentity.Run
    -fcadMode "$mode"
    -fcadPlan "$output/$mode-plan.json"
    -fcadOutput "$report"
    -logFile "$log"
  )
  if [ "$expect_contract" -eq 1 ] && [ "$mode" = "graph" ]; then
    arguments+=(-fcadExpectFullContract)
  fi
  if [ "$record" -eq 0 ] && [ "$expect_contract" -eq 0 ] && [ "$no_expected" -eq 0 ]; then
    arguments+=(-fcadExpected "$tool/expected/$mode-report.json")
  fi
  local minimum=40
  case "$mode" in
    graph) minimum=500 ;;
    fbxclaim) minimum=3 ;;
  esac
  set +e
  "$unity" "${arguments[@]}"
  local status=$?
  set -e
  if ! "$smoke/scripts/verify_unity_run.py" \
    --log "$log" \
    --report "$report" \
    --exit-status "$status" \
    --anchor FCAD_GRAPH_IDENTITY \
    --min-checks "$minimum"
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
  echo "FCAD_GRAPH_REPEATABLE_ACROSS_${runs}_CLEAN_PROJECTS mode=$mode"
done

if [ "$record" -eq 1 ]; then
  mkdir -p "$tool/expected"
  for mode in "${modes[@]}"; do
    cp "$output/$mode-report-1.json" "$tool/expected/$mode-report.json"
    cp "$output/$mode-plan-committed.json" "$tool/expected/$mode-plan.json"
  done
  cp "$oracle" "$tool/expected/graph-oracle-report.json"
  echo "recorded $tool/expected"
fi

# ---------------------------------------------------------------- the join
#
# The structural half runs whenever the graph mode ran, even on its own. The
# transformer's claim, the report's own rows and every placement are checked
# there, so a graph-only run is still a measurement someone can be wrong in
# front of rather than a report nobody reads.
for mode in "${modes[@]}"; do
  if [ "$mode" = "graph" ]; then
    "$here/verify_graph.py" --structural-only \
      --graph "$output/graph-report-1.json" \
      --oracle "$oracle" \
      --graph-plan "$output/graph-plan.json" \
      --emit "$output/graph-structure.json"
  fi
done

if [ "${#modes[@]}" -eq 5 ]; then
  join="$output/graph-decision.json"
  verify=(
    "$here/verify_graph.py"
    --graph "$output/graph-report-1.json"
    --meta "$output/meta-report-1.json"
    --remap "$output/remap-report-1.json"
    --scripted "$output/scripted-report-1.json"
    --claim "$output/fbxclaim-report-1.json"
    --oracle "$oracle"
    --graph-plan "$output/graph-plan.json"
    --emit "$join"
  )
  if [ "$record" -eq 0 ] && [ "$expect_contract" -eq 0 ] && [ "$no_expected" -eq 0 ]; then
    verify+=(--expected "$tool/expected/graph-decision.json")
  fi
  "${verify[@]}"
  if [ "$record" -eq 1 ]; then
    cp "$join" "$tool/expected/graph-decision.json"
    echo "recorded $tool/expected/graph-decision.json"
  fi
fi

"$identity/scripts/check_repository_clean.sh"
