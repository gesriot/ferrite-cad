#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1e1 runner.
#
# Every FBX it shows Unity comes from the production writer through the
# `fbx_identity_variants` example, or, for the real assembly, from the shipped
# `export-fbx` route over the shared job. Nothing here writes an FBX by hand
# and nothing here reads a committed one.
#
# The editor runs in two freshly created temporary projects, so "the same
# answer twice" means two clean AssetDatabases and not one warm one. Neither
# project is inside the repository, and no imported `.fbx`, `.meta` or
# `Library` is left behind.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
root="$(cd "$tool/../.." && pwd)"
smoke="$root/tools/unity-fbx-smoke"
unity="${UNITY_EXECUTABLE:-/Applications/Unity/Hub/Editor/6000.4.10f1/Unity.app/Contents/MacOS/Unity}"
record=0
expect_stable=0
with_complex=0
runs=2

while [ "$#" -gt 0 ]; do
  case "$1" in
    --unity) unity="$2"; shift 2 ;;
    --record) record=1; shift ;;
    --expect-stable) expect_stable=1; shift ;;
    --with-complex) with_complex=1; shift ;;
    --runs) runs="$2"; shift 2 ;;
    *) echo "usage: $0 [--unity PATH] [--record] [--expect-stable] [--with-complex] [--runs N]" >&2; exit 2 ;;
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
staging="$output/variants"
mkdir -p "$staging"

workspace="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-identity.XXXXXX")"
cleanup() {
  local status=$?
  rm -rf "$workspace"
  exit "$status"
}
trap cleanup EXIT INT TERM

# ---------------------------------------------------------------- the bytes
variants="$(cd "$root" && cargo build -p ferritecad-export --example fbx_identity_variants \
  --message-format=json 2>/dev/null \
  | jq -r 'select(.reason == "compiler-artifact")
           | select(.target.name == "fbx_identity_variants")
           | .executable // empty' \
  | head -1)"
if [ -z "$variants" ] || [ ! -x "$variants" ]; then
  echo "the identity variant writer was not built" >&2
  exit 1
fi
"$variants" "$staging" | tee "$output/variants.log"

mode=synthetic
prefix=identity
complex_arguments=()
if [ "$with_complex" -eq 1 ]; then
  mode=complex
  prefix=complex
  complex_first="$staging/complex-first.fbx"
  complex_second="$staging/complex-second.fbx"
  log="$output/complex-export.log"
  (cd "$root" && FCAD_FBX_COMPLEX_OUT="$complex_first" \
    cargo test -p ferritecad-cli --test export_fbx_complex -- --nocapture) 2>&1 | tee "$log"
  if ! grep -q '^FCAD_EXPORT_FBX_COMPLEX ' "$log"; then
    if grep -q 'skipped: this build has no Open CASCADE' "$log"; then
      echo "the real assembly needs Open CASCADE and this build has none" >&2
      exit 3
    fi
    echo "the complex export gate did not run" >&2
    exit 1
  fi
  # A second, independent export of the same document: scenario 11b asks what
  # a reimport of a re-exported assembly does, not what a copy does.
  log2="$output/complex-export-second.log"
  (cd "$root" && FCAD_FBX_COMPLEX_OUT="$complex_second" \
    cargo test -p ferritecad-cli --test export_fbx_complex -- --nocapture) 2>&1 | tee "$log2"
  grep -q '^FCAD_EXPORT_FBX_COMPLEX ' "$log2" || { echo "the second complex export did not run" >&2; exit 1; }
  complex_arguments=(--complex "$complex_first" --complex-second "$complex_second")
fi

# ------------------------------------------------------- the independent read
cache="$("$smoke/scripts/fetch_ufbx.sh")"
reader="$output/read_identity"
clang -std=c11 -O2 -Wall -Wextra -Werror \
  -I "$cache" "$here/read_identity.c" "$cache/ufbx.c" -lm -o "$reader"

oracle="$output/$prefix-oracle-report.json"
# Exactly the files this mode's plan will name, and nothing else: reading a
# third of a gigabyte twice to answer a question about a ten-kilobyte document
# would only make the measurement slower.
if [ "$with_complex" -eq 1 ]; then
  "$reader" "$staging"/complex-*.fbx > "$oracle"
else
  measured=()
  for candidate in "$staging"/*.fbx; do
    case "$(basename "$candidate")" in
      complex-*) ;;
      *) measured+=("$candidate") ;;
    esac
  done
  "$reader" "${measured[@]}" > "$oracle"
fi
echo "FCAD_IDENTITY_ORACLE_EXECUTED"

if [ "$with_complex" -eq 1 ]; then
  "$here/choose_complex_anchors.py" --oracle "$oracle" --output "$staging/complex-anchors.json"
fi

plan="$output/$prefix-plan.json"
"$here/make_plan.py" --variants "$staging" --output "$plan" ${complex_arguments[@]+"${complex_arguments[@]}"}
# The committed copy names files rather than this machine, so the recorded
# measurement is the same on every machine that reproduces it.
"$here/make_plan.py" --variants "$staging" --output "$output/$prefix-plan-committed.json" \
  --basenames ${complex_arguments[@]+"${complex_arguments[@]}"}

# ------------------------------------------------------------- the editor
run_in_fresh_project() {
  local index="$1"
  local destination="$2"
  local project="$workspace/project-$index"
  mkdir -p "$project/Assets/Editor"
  cp -R "$smoke/ProjectSettings" "$project/ProjectSettings"
  cp -R "$smoke/Packages" "$project/Packages"
  cp "$tool/Editor"/*.cs "$project/Assets/Editor/"

  local report="$output/$prefix-report-$index.json"
  local log="$output/unity-$prefix-$index.log"
  local arguments=(
    -batchmode -nographics -quit
    -projectPath "$project"
    -executeMethod FerriteFbxIdentity.Run
    -fcadPlan "$plan"
    -fcadOutput "$report"
    -logFile "$log"
  )
  if [ "$expect_stable" -eq 1 ]; then
    arguments+=(-fcadExpectStable)
  fi
  if [ "$record" -eq 0 ] && [ "$expect_stable" -eq 0 ]; then
    arguments+=(-fcadExpected "$tool/expected/$prefix-report.json")
  fi
  set +e
  "$unity" "${arguments[@]}"
  local status=$?
  set -e
  if ! "$smoke/scripts/verify_unity_run.py" \
    --log "$log" \
    --report "$report" \
    --exit-status "$status" \
    --anchor FCAD_FBX_IDENTITY \
    --min-checks 200
  then
    sed -n '1,400p' "$log" >&2
    exit 1
  fi
  # The project this run used is deleted here, so the next run cannot inherit
  # an AssetDatabase, an import cache or a GUID from it.
  rm -rf "$project"
  cp "$report" "$destination"
}

index=1
while [ "$index" -le "$runs" ]; do
  run_in_fresh_project "$index" "$output/$prefix-run-$index.json"
  index=$((index + 1))
done

index=2
while [ "$index" -le "$runs" ]; do
  if ! cmp -s "$output/$prefix-run-1.json" "$output/$prefix-run-$index.json"; then
    echo "two clean Unity projects produced different canonical identity reports" >&2
    diff <(python3 -m json.tool "$output/$prefix-run-1.json") \
         <(python3 -m json.tool "$output/$prefix-run-$index.json") | head -60 >&2
    exit 1
  fi
  index=$((index + 1))
done
echo "FCAD_IDENTITY_REPEATABLE_ACROSS_${runs}_CLEAN_PROJECTS"

if [ "$record" -eq 1 ]; then
  mkdir -p "$tool/expected"
  cp "$output/$prefix-run-1.json" "$tool/expected/$prefix-report.json"
  cp "$oracle" "$tool/expected/$prefix-oracle-report.json"
  cp "$output/$prefix-plan-committed.json" "$tool/expected/$prefix-plan.json"
  echo "recorded $tool/expected/$prefix-report.json"
fi

join="$output/$prefix-transitions.json"
verify=(
  "$here/verify_identity.py"
  --unity "$output/$prefix-run-1.json"
  --oracle "$oracle"
  --plan "$plan"
  --mode "$mode"
  --emit "$join"
)
if [ "$record" -eq 0 ] && [ "$expect_stable" -eq 0 ]; then
  verify+=(--expected "$tool/expected/$prefix-transitions.json")
fi
"${verify[@]}"
if [ "$record" -eq 1 ]; then
  cp "$join" "$tool/expected/$prefix-transitions.json"
  echo "recorded $tool/expected/$prefix-transitions.json"
fi

"$here/check_repository_clean.sh"
