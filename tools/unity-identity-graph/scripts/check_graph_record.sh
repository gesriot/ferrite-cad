#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The half of the §22B-1e2b measurement that needs neither the editor nor a
# kernel, so it can run on every platform on every push.
#
# Unity itself runs locally, on the one measured version, as §22B-1a decided.
# What runs here is the part that keeps the recorded result honest between
# those runs: all five canonical reports are re-joined to the independent
# `ufbx` reading of the same bytes, the structural transformer is held to its
# claim against the control, the decision record is rebuilt and compared with
# the committed one, and the semantic mutation campaign is run against the real
# verifier.
#
# A recorded measurement no gate ever reads is a file, not a result.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
identity="$(cd "$tool/../unity-fbx-identity" && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-graph-record.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT INT TERM

"$here/verify_graph.py" \
  --graph "$tool/expected/graph-report.json" \
  --meta "$tool/expected/meta-report.json" \
  --remap "$tool/expected/remap-report.json" \
  --scripted "$tool/expected/scripted-report.json" \
  --claim "$tool/expected/fbxclaim-report.json" \
  --oracle "$tool/expected/graph-oracle-report.json" \
  --graph-plan "$tool/expected/graph-plan.json" \
  --emit "$temporary/graph-decision.json" \
  --expected "$tool/expected/graph-decision.json"

"$here/run_graph_mutations.py"
"$identity/scripts/check_repository_clean.sh"
echo "graph record: all five runs re-join, and the campaign still kills every mutant"
