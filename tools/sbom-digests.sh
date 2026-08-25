#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Regenerates every Rust fragment into a temporary directory and prints what
# each one hashed to, so that three hosts can be compared without comparing
# what git happened to check out.
#
# The pinned inputs are hashed too. A Windows runner checks out with
# core.autocrlf enabled by default, and a schema file that arrived with
# different line endings would fail its own digest check for reasons that have
# nothing to do with the dependency graph. .gitattributes pins those paths;
# this is how that stays true rather than being assumed.
#
# Run from the repository root:
#   tools/sbom-digests.sh

set -euo pipefail

SBOM_TOOL='sbom-digests'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/sbom/lib.sh
. tools/sbom/lib.sh

sbom_load_pin
sbom_require_jq
sbom_require_cyclonedx
sbom_require_validator
sbom_verify_schema_files

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

printf 'tool\tcargo-cyclonedx %s\n' "$CARGO_CYCLONEDX_VERSION"
printf 'tool\tjsonschema-cli %s\n' "$JSONSCHEMA_CLI_VERSION"
printf 'tool\tfragment format %s\n' "$SBOM_FRAGMENT_FORMAT"
printf 'schema\tCycloneDX %s at %s\n' "$CYCLONEDX_SPEC_VERSION" "$CYCLONEDX_SCHEMA_COMMIT"
for f in "$SBOM_SCHEMA_BOM" "$SBOM_SCHEMA_SPDX" "$SBOM_SCHEMA_JSF" tools/sbom/pin.env; do
    printf 'policy\t%s\t%s\n' "$f" "$(notice_sha256 "$f")"
done

for target in "${NOTICE_TARGETS[@]}"; do
    tools/generate-rust-sbom.sh --target "$target" --output "$work/$target.cdx.json" >/dev/null
    printf 'fragment\t%s\t%s\n' "$target" "$(notice_sha256 "$work/$target.cdx.json")"
done

# The regenerated files are what the artifact upload should carry: they are the
# evidence this host produced, not the bytes it checked out.
if [ -n "${SBOM_KEEP_DIR:-}" ]; then
    mkdir -p "$SBOM_KEEP_DIR"
    cp "$work"/*.cdx.json "$SBOM_KEEP_DIR/"
fi
