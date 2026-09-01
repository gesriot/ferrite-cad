#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

mkdir -p "$project/measurement-output"
report="$project/measurement-output/unity-import-report.json"
log="$project/measurement-output/unity.log"
rm -f "$report" "$log"
arguments=(
  -batchmode -nographics -quit
  -projectPath "$project"
  -executeMethod FerriteFbxSmoke.Run
  -fcadOutput "$report"
  -logFile "$log"
)
if [ "$record" -eq 1 ]; then
  arguments+=(-fcadRecord)
fi
set +e
"$unity" "${arguments[@]}"
status=$?
set -e
verify=(
  "$project/scripts/verify_unity_run.py"
  --log "$log"
  --report "$report"
  --exit-status "$status"
)
if [ "$record" -eq 0 ]; then
  verify+=(--expected "$project/Assets/Expected/unity-import-report.json")
fi
if ! "${verify[@]}"; then
  sed -n '1,260p' "$log" >&2
  exit 1
fi
