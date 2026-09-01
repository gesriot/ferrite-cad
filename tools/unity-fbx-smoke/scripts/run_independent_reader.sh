#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixtures="$project/Assets/Fixtures"
expected="$project/Assets/Expected/independent-reader-report.json"
output="$project/measurement-output/independent-reader-report.json"
record=0
if [ "${1:-}" = "--record" ]; then
  record=1
elif [ "$#" -ne 0 ]; then
  echo "usage: $0 [--record]" >&2
  exit 2
fi

cache="$($project/scripts/fetch_ufbx.sh)"
mkdir -p "$project/measurement-output"
clang -std=c11 -O2 -Wall -Wextra -Werror \
  -I "$cache" "$project/scripts/read_fixture.c" "$cache/ufbx.c" -lm \
  -o "$project/measurement-output/read_fixture"

"$project/measurement-output/read_fixture" \
  "$fixtures/wrong_double_unit_ascii7400.fbx" \
  "$fixtures/wrong_m_metadata_ascii7400.fbx" \
  "$fixtures/wrong_yup_metadata_ascii7400.fbx" \
  "$fixtures/yup_m_preconverted_ascii7400.fbx" \
  "$fixtures/zup_mm_ascii7400.fbx" \
  "$fixtures/unity_builtin_disc_binary7400.fbx" > "$output"

if [ "$record" -eq 1 ]; then
  cp "$output" "$expected"
else
  cmp "$expected" "$output"
fi
echo "FCAD_INDEPENDENT_FBX_READER_EXECUTED"
