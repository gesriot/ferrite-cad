#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Regenerates every notice into a temporary directory and prints what each one
# hashed to, so that three hosts can be compared without comparing what git
# happened to check out.
#
# The policy inputs are hashed too. A Windows runner checks out with
# core.autocrlf enabled by default, and a policy file that arrived with
# different line endings would generate a different notice for reasons that
# have nothing to do with the dependency graph. .gitattributes pins those
# paths; this is how that stays true rather than being assumed.
#
# Run from the repository root:
#   tools/notice-digests.sh

set -euo pipefail

# Read by notice_die in tools/notices/lib.sh.
# shellcheck disable=SC2034
NOTICE_TOOL='notice-digests'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/notices/lib.sh
. tools/notices/lib.sh

notice_load_pin
notice_require_about

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

printf 'tool\tcargo-about %s threshold %s\n' "$CARGO_ABOUT_VERSION" "$CARGO_ABOUT_THRESHOLD"
for f in "$NOTICE_ABOUT_CONFIG" "$NOTICE_UPSTREAM_TSV" "$NOTICE_DECLARED_TSV" tools/notices/pin.env; do
    printf 'policy\t%s\t%s\n' "$f" "$(notice_sha256 "$f")"
done
for f in "$NOTICE_TEXT_DIR"/*.txt; do
    [ -e "$f" ] || continue
    printf 'payload\t%s\t%s\n' "$(basename "$f")" "$(notice_sha256 "$f")"
done

for target in "${NOTICE_TARGETS[@]}"; do
    tools/generate-rust-notices.sh --target "$target" --output "$work/$target.md" >/dev/null
    printf 'notice\t%s\t%s\n' "$target" "$(notice_sha256 "$work/$target.md")"
done
