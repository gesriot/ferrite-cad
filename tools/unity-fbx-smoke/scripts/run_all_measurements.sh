#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-fbx-repeat.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT INT TERM

"$project/scripts/verify_failing_baseline.sh"
"$project/scripts/verify_fixtures.py" --project "$project"
"$project/scripts/verify_export_scene.py" "$project/Assets/Fixtures/export-scene-contract.json"

"$project/scripts/run_independent_reader.sh"
cp "$project/measurement-output/independent-reader-report.json" "$temporary/independent-1.json"
"$project/scripts/run_independent_reader.sh"
cmp "$temporary/independent-1.json" "$project/measurement-output/independent-reader-report.json"

"$project/scripts/run_unity_smoke.sh"
cp "$project/measurement-output/unity-import-report.json" "$temporary/unity-1.json"
"$project/scripts/run_unity_smoke.sh"
cmp "$temporary/unity-1.json" "$project/measurement-output/unity-import-report.json"

"$project/scripts/verify_measurements.py" \
  --unity "$project/measurement-output/unity-import-report.json" \
  --independent "$project/measurement-output/independent-reader-report.json"
echo "FCAD_FBX_CANONICAL_REPORTS_BYTE_IDENTICAL repeated_runs=2"
