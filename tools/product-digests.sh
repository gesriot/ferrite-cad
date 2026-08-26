#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Merges every product SBOM into a temporary directory and prints what each one
# hashed to, so that three hosts can be compared without comparing what git
# happened to check out.
#
# The two inputs are hashed too, and so is the merge program itself. A Windows
# runner checks out with core.autocrlf enabled by default, and an input that
# arrived with different line endings would produce a different merge for a
# reason that has nothing to do with the product. .gitattributes pins those
# paths; this is how that stays true rather than being assumed.
#
# Run from the repository root:
#   tools/product-digests.sh

set -euo pipefail

PRODUCT_TOOL='product-digests'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/product/lib.sh
. tools/product/lib.sh

sbom_load_pin
sbom_require_jq
sbom_require_validator
sbom_verify_schema_files

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

inventory="$(product_inventory)"

printf 'tool\tjsonschema-cli %s\n' "$JSONSCHEMA_CLI_VERSION"
printf 'tool\tproduct format %s\n' "$PRODUCT_FORMAT"
printf 'tool\tfragment format %s\n' "$SBOM_FRAGMENT_FORMAT"
printf 'tool\tinventory format %s\n' "$NATIVE_INVENTORY_FORMAT"
printf 'schema\tCycloneDX %s at %s\n' "$CYCLONEDX_SPEC_VERSION" "$CYCLONEDX_SCHEMA_COMMIT"
for f in "$SBOM_SCHEMA_BOM" "$SBOM_SCHEMA_SPDX" "$SBOM_SCHEMA_JSF" \
         tools/sbom/pin.env tools/product/merge.jq tools/product/oracle.jq; do
    printf 'policy\t%s\t%s\n' "$f" "$(native_sha256 "$f")"
done

# The bytes the merge read. A host that merged different inputs would produce a
# different document for a reason that is not the merge's.
printf 'input\t%s\t%s\n' "$inventory" "$(native_sha256 "$inventory")"
for target in "${NOTICE_TARGETS[@]}"; do
    fragment="$(product_fragment_for "$target")"
    printf 'input\t%s\t%s\n' "$fragment" "$(native_sha256 "$fragment")"
done

for target in "${NOTICE_TARGETS[@]}"; do
    tools/generate-product-sbom.sh --target "$target" \
        --output "$work/$target.cdx.json" >/dev/null
    printf 'product\t%s\t%s\n' "$target" "$(native_sha256 "$work/$target.cdx.json")"
done

# The merged files are what the artifact upload should carry: they are the
# evidence this host produced, not the bytes it checked out.
if [ -n "${PRODUCT_KEEP_DIR:-}" ]; then
    mkdir -p "$PRODUCT_KEEP_DIR"
    cp "$work"/*.cdx.json "$PRODUCT_KEEP_DIR/"
fi
