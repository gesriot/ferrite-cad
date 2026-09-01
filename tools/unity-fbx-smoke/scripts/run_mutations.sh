#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Mutation controls plus semantic and real Unity compile mutations.
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
probe="$project/Assets/Editor/FerriteFbxSmoke.cs"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-fbx-mutations.XXXXXX")"
backup="$probe.mutbak"

cleanup() {
  local status=$?
  if [ -f "$backup" ]; then
    cp "$backup" "$probe"
    rm "$backup"
    touch "$probe"
  fi
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

if find "$project/Assets" "$project/scripts" -name '*.mutbak' -print -quit | grep -q .; then
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

cp "$project/Assets/Expected/unity-import-report.json" "$temporary/prewritten.json"
: > "$temporary/no-unity.log"
if "$project/scripts/verify_unity_run.py" --log "$temporary/no-unity.log" --report "$temporary/prewritten.json" --expected "$project/Assets/Expected/unity-import-report.json" --exit-status 0 >/dev/null 2>&1; then
  echo "prewritten report without Unity execution accepted" >&2
  exit 1
fi
printf 'FCAD_FBX_SMOKE_EXECUTED checks=0\n' > "$temporary/zero.log"
python3 - "$temporary/prewritten.json" <<'PY'
import json, sys
path = sys.argv[1]
data = json.load(open(path, encoding="utf-8"))
data["checks"] = 0
open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(data) + "\n")
PY
if "$project/scripts/verify_unity_run.py" --log "$temporary/zero.log" --report "$temporary/prewritten.json" --exit-status 0 >/dev/null 2>&1; then
  echo "zero-check Unity run accepted" >&2
  exit 1
fi
echo "harness controls: skipped Unity/prewritten report and zero-check run refused"

"$project/scripts/run_logic_mutations.py"

original="$(shasum -a 256 "$probe" | cut -d' ' -f1)"
cp -p "$probe" "$backup"
replace_once "$probe" 'internal static class FerriteFbxSmoke' 'internal static class FerriteFbxSmoke this_will_not_compile'
set +e
"$project/scripts/run_unity_smoke.sh" > "$temporary/noncompiling.log" 2>&1
compile_status=$?
set -e
if [ "$compile_status" -eq 0 ] || ! grep -q 'error CS' "$project/measurement-output/unity.log"; then
  echo "non-compiling Unity probe was not refused as a compile failure" >&2
  cat "$temporary/noncompiling.log" >&2
  exit 1
fi
if grep -q 'FCAD_FBX_SMOKE_EXECUTED' "$project/measurement-output/unity.log"; then
  echo "non-compiling Unity probe published an execution anchor" >&2
  exit 1
fi
cp "$backup" "$probe"
rm "$backup"
touch "$probe"
if [ "$(shasum -a 256 "$probe" | cut -d' ' -f1)" != "$original" ]; then
  echo "Unity probe restoration changed bytes" >&2
  exit 1
fi
echo "harness control: non-compiling Unity probe refused before runtime"

"$project/scripts/run_unity_smoke.sh"
echo "mutation campaign: final real Unity baseline passed"
