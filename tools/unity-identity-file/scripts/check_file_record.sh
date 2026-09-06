#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The half of the §22B-1e3b Unity measurement that needs neither the editor nor
# a kernel, so it can run on every platform on every push.
#
# Unity itself runs locally, on the one measured version, as §22B-1a decided.
# What runs here is the part that keeps the recorded result honest between
# those runs: the decision record is rebuilt from the recorded report and
# compared with the committed one, and the semantic mutation campaign is run
# against the real verifier.
#
# A recorded measurement no gate ever reads is a file, not a result.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
identity="$(cd "$tool/../unity-fbx-identity" && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-file-record.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT INT TERM

"$here/verify_file.py" \
  --report "$tool/expected/file-report.json" \
  --emit "$temporary/file-decision.json" \
  --expected "$tool/expected/file-decision.json"

"$here/run_file_mutations.py"
"$identity/scripts/check_repository_clean.sh"
echo "file record: the recorded run still answers every question, and the campaign \
still kills every mutant"
