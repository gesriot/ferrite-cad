#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The half of the §22B-1e1 measurement that needs neither the editor nor a
# kernel, so it can run on every platform on every push.
#
# Unity itself is run locally, on the one measured version, as §22B-1a decided
# and as the CI comment beside the FBX writer gate says. What runs here is the
# part that keeps the recorded result honest between those runs: the canonical
# measurement is re-joined to the independent `ufbx` reading of the same bytes,
# the joined transition table is rebuilt and compared with the committed one,
# and the semantic mutation campaign is run against the real verifier.
#
# A recorded measurement no gate ever reads is a file, not a result.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-identity-record.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT INT TERM

for mode in identity complex; do
  case "$mode" in
    identity) kind=synthetic ;;
    *) kind=complex ;;
  esac
  "$here/verify_identity.py" \
    --unity "$tool/expected/$mode-report.json" \
    --oracle "$tool/expected/$mode-oracle-report.json" \
    --plan "$tool/expected/$mode-plan.json" \
    --mode "$kind" \
    --emit "$temporary/$mode-transitions.json" \
    --expected "$tool/expected/$mode-transitions.json"
done

"$here/run_identity_mutations.py"
"$here/check_repository_clean.sh"
echo "identity record: both measurements re-join, and the campaign still kills every mutant"
