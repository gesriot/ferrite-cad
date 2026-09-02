#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1b2 Unity gate. Unlike the §22B-1a smoke beside it, nothing here
# reads a committed fixture: the production FBX writer produces an asset into
# a temporary directory inside the project, the real editor imports it, and
# the probe measures whether the contract survived.
#
# The asset is deleted afterwards. It is one build's output, not a fixture.
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
root="$(cd "$project/../.." && pwd)"
unity="${UNITY_EXECUTABLE:-/Applications/Unity/Hub/Editor/6000.4.10f1/Unity.app/Contents/MacOS/Unity}"
record=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --unity) unity="$2"; shift 2 ;;
    --record) record=1; shift ;;
    *) echo "usage: $0 [--unity PATH] [--record]" >&2; exit 2 ;;
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

asset_directory="$project/Assets/Production"
asset="$asset_directory/fcad-production.fbx"
cleanup() {
  rm -rf "$asset_directory" "$asset_directory.meta"
}
trap cleanup EXIT INT TERM
cleanup
mkdir -p "$asset_directory"

# The production bytes, written here and nowhere else.
staging="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-unity-production.XXXXXX")"
artefacts="$(cd "$root" && cargo build -p ferritecad-export --example fbx_gate_artefacts \
  --message-format=json 2>/dev/null \
  | jq -r 'select(.reason == "compiler-artifact")
           | select(.target.name == "fbx_gate_artefacts")
           | .executable // empty' \
  | head -1)"
if [ -z "$artefacts" ] || [ ! -x "$artefacts" ]; then
  echo "the gate artefact writer was not built" >&2
  rm -rf "$staging"
  exit 1
fi
"$artefacts" "$staging"
cp "$staging/fcad-measured.fbx" "$asset"
rm -rf "$staging"

mkdir -p "$project/measurement-output"
report="$project/measurement-output/unity-production-report.json"
log="$project/measurement-output/unity-production.log"
expected="$project/Assets/Expected/unity-production-report.json"

run_once() {
  local destination="$1"
  local record_flag="$2"
  rm -f "$report" "$log"
  local arguments=(
    -batchmode -nographics -quit
    -projectPath "$project"
    -executeMethod FerriteFbxProduction.Run
    -fcadOutput "$report"
    -logFile "$log"
  )
  if [ "$record_flag" -eq 1 ]; then
    arguments+=(-fcadRecord)
  fi
  set +e
  "$unity" "${arguments[@]}"
  local status=$?
  set -e
  local verify=(
    "$project/scripts/verify_unity_run.py"
    --log "$log"
    --report "$report"
    --exit-status "$status"
    --anchor FCAD_FBX_PRODUCTION
    --min-checks 60
  )
  if [ "$record_flag" -eq 0 ]; then
    verify+=(--expected "$expected")
  fi
  if ! "${verify[@]}"; then
    sed -n '1,260p' "$log" >&2
    exit 1
  fi
  cp "$report" "$destination"
}

if [ "$record" -eq 1 ]; then
  run_once "$project/measurement-output/first.json" 1
  cp "$report" "$expected"
  echo "recorded $expected"
  exit 0
fi

# Twice, and the two canonical reports must be the same bytes.
run_once "$project/measurement-output/first.json" 0
run_once "$project/measurement-output/second.json" 0
if ! cmp -s "$project/measurement-output/first.json" "$project/measurement-output/second.json"; then
  echo "two Unity runs produced different canonical reports" >&2
  exit 1
fi
echo "FCAD_UNITY_PRODUCTION_REPEATABLE"
