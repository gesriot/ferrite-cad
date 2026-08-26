#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Generates the deterministic CycloneDX 1.5 product SBOM for one product target,
# or for all three.
#
# This is the merge §21A-2b2b0b2b2 exists for, and it is the first document in
# this repository that claims to be complete. Its two inputs stay exactly as
# they are and go on saying they are not:
#
#   * sbom/rust/rust-fragment-<triple>.cdx.json, which describes only the Rust
#     components, and
#   * sbom/native/native-assets-inventory.json, which describes only the native
#     components, the build inputs and the embedded assets.
#
# Neither becomes a product SBOM retroactively. Nothing here rewrites either of
# them, and tools/check-product-sbom.sh refuses a run in which either changed.
#
# Nothing here recomputes a Rust identity. Every Rust component object is
# carried across as it stands and every Rust edge survives; the resolver that
# produced them is tools/generate-rust-sbom.sh and there is not a second one.
#
# Runs no network.
#
# Run from the repository root:
#   tools/generate-product-sbom.sh --all
#   tools/generate-product-sbom.sh --target <triple> [--output <file>]

set -euo pipefail

PRODUCT_TOOL='generate-product-sbom'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/product/lib.sh
. tools/product/lib.sh

target=''
output=''
all=''
while [ $# -gt 0 ]; do
    case "$1" in
        --all)    all=1; shift ;;
        --target) target="${2:?--target needs a triple}"; shift 2 ;;
        --output) output="${2:?--output needs a path}"; shift 2 ;;
        *) product_die "unknown argument: $1" ;;
    esac
done

[ -n "$all" ] || [ -n "$target" ] || product_die 'one of --all or --target is required'
[ -z "$all" ] || [ -z "$output" ] || product_die '--output describes one file, so it cannot go with --all'

sbom_load_pin
sbom_require_jq

inventory="$(product_inventory)"
[ -f "$inventory" ] || product_die "the native/assets inventory $inventory is missing"

generate() { # target output
    local t="$1" out="$2" fragment work
    fragment="$(product_fragment_for "$t")"
    [ -f "$fragment" ] || product_die "the Rust fragment $fragment is missing"

    work="$(mktemp -d)"
    # Handed back explicitly, for the reason tools/generate-native-inventory.sh
    # records: on bash 3.2 the last command of a trap can decide the status.
    # shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
    trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

    jq -n -S \
        --slurpfile fragment "$fragment" \
        --slurpfile inventory "$inventory" \
        --arg target "$t" \
        --arg ns "$SBOM_NS" \
        --arg spec "$CYCLONEDX_SPEC_VERSION" \
        --arg format "$PRODUCT_FORMAT" \
        --arg fragment_path "$fragment" \
        --arg fragment_sha256 "$(native_sha256 "$fragment")" \
        --arg inventory_path "$inventory" \
        --arg inventory_sha256 "$(native_sha256 "$inventory")" \
        -f tools/product/merge.jq \
        | native_strip_cr > "$work/out.json"

    [ -s "$work/out.json" ] || product_die "the document for $t was not written"
    mkdir -p "$(dirname "$out")"
    mv "$work/out.json" "$out"
    rm -rf "$work"
    trap - EXIT
    echo "$PRODUCT_TOOL: wrote $out"
}

if [ -n "$all" ]; then
    for t in "${NOTICE_TARGETS[@]}"; do
        generate "$t" "$(product_output_for "$t")"
    done
else
    case " ${NOTICE_TARGETS[*]} " in
        *" $target "*) ;;
        *) product_die "$target is not a product target" ;;
    esac
    generate "$target" "${output:-$(product_output_for "$target")}"
fi
