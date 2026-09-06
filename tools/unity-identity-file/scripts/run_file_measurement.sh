#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1e3b Unity runner.
#
# The bytes are the production writer's own, produced here by the same example
# every other FBX gate uses. Nothing is rewritten on the way in: there is no
# transformer and no second serializer in this measurement, because the thing
# being measured is what a person actually gets.
#
# Three files are imported. `fcad-measured.fbx` and `fcad-legacy.fbx` are one
# scene at two document layouts and are the pair that makes "nothing else
# moved" a comparison rather than a claim; `fcad-identity-escaping.fbx` carries
# the definition keys chosen to break the wire grammar.
#
# Each run happens in a freshly created temporary project outside the
# repository, twice, and the two canonical reports must be byte-identical.
# Nothing imported is left behind: no `.fbx`, no `.meta`, no `Library`, no
# Unity project.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
root="$(cd "$tool/../.." && pwd)"
smoke="$root/tools/unity-fbx-smoke"
identity="$root/tools/unity-fbx-identity"
unity="${UNITY_EXECUTABLE:-/Applications/Unity/Hub/Editor/6000.4.10f1/Unity.app/Contents/MacOS/Unity}"
record=0
# Skips the byte-for-byte comparison with the committed measurement, so a run
# is judged by what the probe understands rather than by "these bytes are not
# the recorded bytes". The mutation campaign uses it.
no_expected=0
runs=2

while [ "$#" -gt 0 ]; do
  case "$1" in
    --unity) unity="$2"; shift 2 ;;
    --record) record=1; shift ;;
    --no-expected) no_expected=1; shift ;;
    --runs) runs="$2"; shift 2 ;;
    *)
      echo "usage: $0 [--unity PATH] [--record] [--no-expected] [--runs N]" >&2
      exit 2
      ;;
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
staging="$output/production"
mkdir -p "$staging"

workspace="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-file-identity.XXXXXX")"
cleanup() {
  local status=$?
  rm -rf "$workspace"
  exit "$status"
}
trap cleanup EXIT INT TERM

# ---------------------------------------------------------------- the bytes
artefacts="$(cd "$root" && cargo build -p ferritecad-export --example fbx_gate_artefacts \
  --message-format=json 2>/dev/null \
  | jq -r 'select(.reason == "compiler-artifact")
           | select(.target.name == "fbx_gate_artefacts")
           | .executable // empty' \
  | head -1)"
if [ -z "$artefacts" ] || [ ! -x "$artefacts" ]; then
  echo "the gate artefact writer was not built" >&2
  exit 1
fi
"$artefacts" "$staging" | tee "$output/artefacts.log"

# And they are the committed digests, so what the editor imports is provably the
# file this repository ships rather than whatever this checkout happened to
# build.
digests="$root/tools/fbx/digests.tsv"
[ -f "$digests" ] || { echo "the recorded FBX digests are missing" >&2; exit 1; }
checked=0
while IFS=$'\t' read -r expected name; do
  [ -n "$name" ] || continue
  [ -f "$staging/$name" ] || { echo "the writer produced no $name" >&2; exit 1; }
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$staging/$name" | cut -d' ' -f1)"
  else
    actual="$(shasum -a 256 "$staging/$name" | cut -d' ' -f1)"
  fi
  if [ "$actual" != "$expected" ]; then
    echo "$name is $actual here and $expected in the recorded digests" >&2
    exit 1
  fi
  checked=$((checked + 1))
done <"$digests"
if [ "$checked" -lt 3 ]; then
  echo "only $checked production files were checked against the recorded digests" >&2
  exit 1
fi
echo "FCAD_FILE_BYTES_ARE_THE_RECORDED_PRODUCTION_BYTES files=$checked"

# ------------------------------------------------------------- the editor
run_in_fresh_project() {
  local index="$1"
  local project="$workspace/run-$index"
  mkdir -p "$project/Assets/Editor"
  cp -R "$smoke/ProjectSettings" "$project/ProjectSettings"
  cp -R "$smoke/Packages" "$project/Packages"
  cp "$tool/Editor"/*.cs "$project/Assets/Editor/"

  local report="$output/file-report-$index.json"
  local log="$output/unity-file-$index.log"
  local arguments=(
    -batchmode -nographics -quit
    -projectPath "$project"
    -executeMethod FerriteFileIdentity.Run
    -fcadSource "$staging"
    -fcadOutput "$report"
    -logFile "$log"
  )
  if [ "$record" -eq 0 ] && [ "$no_expected" -eq 0 ]; then
    arguments+=(-fcadExpected "$tool/expected/file-report.json")
  fi
  set +e
  "$unity" "${arguments[@]}"
  local status=$?
  set -e
  if ! "$smoke/scripts/verify_unity_run.py" \
    --log "$log" \
    --report "$report" \
    --exit-status "$status" \
    --anchor FCAD_FILE_IDENTITY \
    --min-checks 40
  then
    sed -n '1,400p' "$log" >&2
    exit 1
  fi
  # Deleted here, so the next run cannot inherit an AssetDatabase, an import
  # cache or a GUID from it.
  rm -rf "$project"
}

index=1
while [ "$index" -le "$runs" ]; do
  run_in_fresh_project "$index"
  index=$((index + 1))
done
index=2
while [ "$index" -le "$runs" ]; do
  if ! cmp -s "$output/file-report-1.json" "$output/file-report-$index.json"; then
    echo "two clean Unity projects produced different canonical reports" >&2
    diff <(python3 -m json.tool "$output/file-report-1.json") \
         <(python3 -m json.tool "$output/file-report-$index.json") | head -60 >&2
    exit 1
  fi
  index=$((index + 1))
done
echo "FCAD_FILE_REPEATABLE_ACROSS_${runs}_CLEAN_PROJECTS"

if [ "$record" -eq 1 ]; then
  mkdir -p "$tool/expected"
  cp "$output/file-report-1.json" "$tool/expected/file-report.json"
  echo "recorded $tool/expected/file-report.json"
fi

# ---------------------------------------------------------------- the join
verify=(
  "$here/verify_file.py"
  --report "$output/file-report-1.json"
  --emit "$output/file-decision.json"
)
if [ "$record" -eq 0 ] && [ "$no_expected" -eq 0 ]; then
  verify+=(--expected "$tool/expected/file-decision.json")
fi
"${verify[@]}"
if [ "$record" -eq 1 ]; then
  cp "$output/file-decision.json" "$tool/expected/file-decision.json"
  echo "recorded $tool/expected/file-decision.json"
fi

"$identity/scripts/check_repository_clean.sh"
