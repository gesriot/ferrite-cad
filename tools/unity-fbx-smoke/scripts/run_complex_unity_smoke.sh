#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1c Unity gate: the real editor imports the complex AP203 assembly
# that the shipped `export-fbx` command produced.
#
# Nothing here reads a committed fixture and nothing here writes an FBX by
# hand. The Rust gate imports the committed STEP through `import-step`, deletes
# the external file, runs `export-fbx` on the document alone and leaves the
# published bytes where this script asks for them; those bytes are the asset
# Unity is given.
#
# Two hundred and thirty megabytes of ASCII FBX takes the editor a while. The
# asset and the project's import cache for it are deleted afterwards: this is
# one build's tessellation, not a fixture.
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
root="$(cd "$project/../.." && pwd)"
unity="${UNITY_EXECUTABLE:-/Applications/Unity/Hub/Editor/6000.4.10f1/Unity.app/Contents/MacOS/Unity}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --unity) unity="$2"; shift 2 ;;
    *) echo "usage: $0 [--unity PATH]" >&2; exit 2 ;;
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

asset_directory="$project/Assets/Complex"
asset="$asset_directory/fcad-complex.fbx"
cleanup() {
  rm -rf "$asset_directory" "$asset_directory.meta"
}
trap cleanup EXIT INT TERM
cleanup
mkdir -p "$asset_directory"

# The production bytes, written by the shipped command and by nothing else. A
# missing kernel makes this skip rather than fail, exactly as the Rust gate
# does.
log="$project/measurement-output/complex-export.log"
mkdir -p "$project/measurement-output"
(cd "$root" && FCAD_FBX_COMPLEX_OUT="$asset" \
  cargo test -p ferritecad-cli --test export_fbx_complex -- --nocapture) 2>&1 | tee "$log"

if ! grep -q '^FCAD_EXPORT_FBX_COMPLEX ' "$log"; then
  if grep -q 'skipped: this build has no Open CASCADE' "$log"; then
    echo "skipped: this build has no Open CASCADE"
    exit 0
  fi
  echo "the complex export gate did not run" >&2
  exit 1
fi
[ -s "$asset" ] || { echo "the gate left no FBX for Unity to import" >&2; exit 1; }
echo "asset: $(wc -c <"$asset") bytes"

report="$project/measurement-output/unity-complex-report.json"
unity_log="$project/measurement-output/unity-complex.log"

run_once() {
  local destination="$1"
  rm -f "$report" "$unity_log"
  set +e
  "$unity" \
    -batchmode -nographics -quit \
    -projectPath "$project" \
    -executeMethod FerriteFbxComplex.Run \
    -fcadOutput "$report" \
    -logFile "$unity_log"
  local status=$?
  set -e
  if ! "$project/scripts/verify_unity_run.py" \
    --log "$unity_log" \
    --report "$report" \
    --exit-status "$status" \
    --anchor FCAD_FBX_COMPLEX \
    --min-checks 20
  then
    sed -n '1,260p' "$unity_log" >&2
    exit 1
  fi
  cp "$report" "$destination"
}

# Twice, and the two canonical reports must be the same bytes: an import that
# is a function of the file says the same thing every time it is asked.
run_once "$project/measurement-output/complex-first.json"
run_once "$project/measurement-output/complex-second.json"
if ! cmp -s "$project/measurement-output/complex-first.json" \
  "$project/measurement-output/complex-second.json"; then
  echo "two Unity imports of one file produced different canonical reports" >&2
  exit 1
fi
cat "$project/measurement-output/complex-first.json"
echo "FCAD_UNITY_COMPLEX_REPEATABLE"
