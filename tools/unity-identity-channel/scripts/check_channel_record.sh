#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The half of the §22B-1e2a measurement that needs neither the editor nor a
# kernel, so it can run on every platform on every push.
#
# Unity itself runs locally, on the one measured version, as §22B-1a decided.
# What runs here is the part that keeps the recorded result honest between
# those runs: both canonical reports are re-joined to the independent `ufbx`
# reading of the same bytes, the decision record is rebuilt and compared with
# the committed one, and the semantic mutation campaign is run against the real
# verifier.
#
# A recorded measurement no gate ever reads is a file, not a result.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
identity="$(cd "$tool/../unity-fbx-identity" && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-channel-record.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT INT TERM

"$here/verify_channel.py" \
  --vanilla "$tool/expected/vanilla-report.json" \
  --companion "$tool/expected/companion-report.json" \
  --oracle "$tool/expected/channel-oracle-report.json" \
  --vanilla-plan "$tool/expected/vanilla-plan.json" \
  --companion-plan "$tool/expected/companion-plan.json" \
  --emit "$temporary/channel-decision.json" \
  --expected "$tool/expected/channel-decision.json"

"$here/run_channel_mutations.py"
"$identity/scripts/check_repository_clean.sh"
echo "channel record: both runs re-join, and the campaign still kills every mutant"
