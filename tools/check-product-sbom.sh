#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The gate on the merged product SBOM.
#
# A green run means all of the following were answered for every product
# target, not that nothing crashed:
#
#   1. a merged document exists and is checked in;
#   2. it validates against the pinned CycloneDX 1.5 schema, offline, using the
#      schema files this repository carries and their recorded digests;
#   3. it carries no timestamp, no serial number, no generated identifier, no
#      absolute path and nothing else about the machine that produced it;
#   4. merging twice reproduces the checked-in bytes exactly;
#   5. an oracle built from the Rust fragment and the native/assets inventory -
#      not from anything the merge program wrote - agrees about every Rust
#      component object, every Rust edge, the target filter, the native,
#      build-input and asset components, and every relationship between them;
#   6. the two inputs are still inputs. Neither has been rewritten to claim it
#      is a product SBOM, and both are byte for byte what the merge read.
#
# Licence questions are out of scope here and are asked nowhere in this file.
# ADR 0003 makes recorded licence risk advisory; the Rust components carry
# whatever licence fields the fragment gave them, mechanically and unexamined.
#
# Runs no network.
#
# Run from the repository root:
#   tools/check-product-sbom.sh

set -euo pipefail

PRODUCT_TOOL='check-product-sbom'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/product/lib.sh
. tools/product/lib.sh

sbom_load_pin
sbom_require_jq
sbom_require_validator
sbom_verify_schema_files

work="$(mktemp -d)"
# shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

failures=0
checks=0
check() { checks=$((checks + 1)); }
fail() {
    failures=$((failures + 1))
    echo "$PRODUCT_TOOL: $*" >&2
}

inventory="$(product_inventory)"

# ---------------------------------------------------------------------------
# What exists to be merged, counted before the merge is looked for.
# ---------------------------------------------------------------------------
#
# A run with no product SBOM has to say what is unaccounted for, not only that
# a file is missing. The two halves of the answer are both checked in and both
# say, in their own fields, that they are not the whole thing.

missing=0
for target in "${NOTICE_TARGETS[@]}"; do
    [ -f "$(product_output_for "$target")" ] || missing=$((missing + 1))
done

if [ "$missing" -eq "${#NOTICE_TARGETS[@]}" ]; then
    echo "$PRODUCT_TOOL: no CycloneDX document describes the FerriteCAD product." >&2
    echo >&2
    if [ -f "$inventory" ]; then
        for target in "${NOTICE_TARGETS[@]}"; do
            fragment="$(product_fragment_for "$target")"
            [ -f "$fragment" ] || continue
            rust="$(jq -r '.components | length' "$fragment" | native_strip_cr)"
            edges="$(jq -r '[.dependencies[].dependsOn // [] | length] | add' "$fragment" \
                | native_strip_cr)"
            native="$(jq -r --arg t "$target" \
                '[.components[] | select(.targets | index($t))] | length' \
                "$inventory" | native_strip_cr)"
            printf '  %s: %s Rust components and %s Rust edges in %s,\n' \
                "$target" "$rust" "$edges" "$fragment" >&2
            printf '    %s native, build-input and asset components in %s,\n' \
                "$native" "$inventory" >&2
            printf '    and no document that says how the two fit together.\n' >&2
        done
        echo >&2
        echo "  Both inputs say so themselves. The fragment carries" >&2
        echo "  ferritecad:sbom:complete=false and a pending-merge property; the" >&2
        echo "  inventory carries complete=false and isProductSbom=false. Every" >&2
        echo "  native component, every embedded asset and every relationship" >&2
        echo "  between them is therefore recorded in a document that disclaims" >&2
        echo "  being the product's, and the delivered product has no SBOM at all." >&2
    else
        echo "  Neither input is here: $inventory is missing." >&2
    fi
    echo >&2
    echo "  Expected one document per target under $PRODUCT_DIR;" >&2
    echo "  write them with tools/generate-product-sbom.sh --all" >&2
    exit 1
fi

[ -f "$inventory" ] || product_die "the native/assets inventory $inventory is missing"

# ---------------------------------------------------------------------------
# The schema, made resolvable without a network.
# ---------------------------------------------------------------------------
#
# The same arrangement tools/check-rust-sbom.sh uses and for the same measured
# reason: bom-1.5.schema.json declares an http `$id`, so a validator resolves
# its two companions against cyclonedx.org unless the `$id` is dropped.
mkdir -p "$work/schema"
cp "$SBOM_SCHEMA_BOM" "$SBOM_SCHEMA_SPDX" "$SBOM_SCHEMA_JSF" "$work/schema/"
jq 'del(."$id")' "$SBOM_SCHEMA_BOM" | native_strip_cr > "$work/schema/bom-1.5.schema.json"

# Data this document must never carry: it would either differ between hosts or
# tell a reader where somebody's checkout happened to live.
readonly HOST_PATTERNS='file://|/Users/|/home/|/root/|(^|[^A-Za-z0-9])[A-Za-z]:[\\/]|/private/var/|/tmp/|runner/work|AppData|RUNNER_TEMP|\.cargo/registry'
readonly WHEN_PATTERNS='"[0-9]{4}-[0-9]{2}-[0-9]{2}T|urn:uuid|serialNumber|"timestamp"'

# ---------------------------------------------------------------------------
# The inputs are still inputs.
# ---------------------------------------------------------------------------
#
# The merge completes; the things it merges do not become complete by having
# been read. A fragment or an inventory quietly reworded to claim otherwise
# would leave two documents in this repository claiming to be the product's.

check
said="$(jq -r '[(.complete|tostring), (.isProductSbom|tostring), .pendingMerge] | join(" ")' \
    "$inventory" | native_strip_cr)"
[ "$said" = "false false rust-fragment-and-native-assets" ] \
    || fail "$inventory no longer says it is an incomplete input: $said"

for target in "${NOTICE_TARGETS[@]}"; do
    fragment="$(product_fragment_for "$target")"
    check
    if [ ! -f "$fragment" ]; then
        fail "the Rust fragment $fragment is missing"
        continue
    fi
    said="$(jq -r --arg n "$SBOM_NS" '
        [(.metadata.properties[] | select(.name == $n + ":complete") | .value),
         (.metadata.properties[] | select(.name == $n + ":pending-merge") | .value),
         (.metadata.properties[] | select(.name == $n + ":fragment-format") | .value)]
        | join(" ")' "$fragment" | native_strip_cr)"
    [ "$said" = "false native-libraries-and-assets $SBOM_FRAGMENT_FORMAT" ] \
        || fail "$fragment no longer says it is an incomplete input: $said"
done

# ---------------------------------------------------------------------------
# Per target.
# ---------------------------------------------------------------------------

for target in "${NOTICE_TARGETS[@]}"; do
    committed="$(product_output_for "$target")"
    fragment="$(product_fragment_for "$target")"

    check
    if [ ! -f "$committed" ]; then
        fail "there is no product SBOM for $target at $committed"
        echo "  generate it with: tools/generate-product-sbom.sh --all" >&2
        continue
    fi

    check
    if ! jq -e . "$committed" >/dev/null 2>&1; then
        fail "$committed is not readable JSON"
        continue
    fi

    # 2. the real schema, not a parser written here
    check
    cp "$committed" "$work/schema/instance.json"
    if ! (cd "$work/schema" && jsonschema-cli validate bom-1.5.schema.json \
            -i ./instance.json >"$work/schema.log" 2>&1); then
        fail "$committed does not validate against CycloneDX $CYCLONEDX_SPEC_VERSION:"
        sed 's/^/  /' "$work/schema.log" >&2
    fi
    rm -f "$work/schema/instance.json"

    # 3. nothing about the machine that produced it
    check
    if grep -nEi "$HOST_PATTERNS" "$committed" > "$work/host.txt"; then
        fail "$committed carries host-specific data:"
        head -n 10 "$work/host.txt" | sed 's/^/  /' >&2 || true
    fi

    check
    if grep -nEi "$WHEN_PATTERNS" "$committed" > "$work/when.txt"; then
        fail "$committed carries a timestamp or a generated identifier:"
        head -n 10 "$work/when.txt" | sed 's/^/  /' >&2 || true
    fi

    check
    if grep -q $'\r' "$committed"; then
        fail "$committed carries carriage returns; .gitattributes should keep it out of conversion"
    fi

    # 4. two merges, and the checked-in bytes
    check
    if ! tools/generate-product-sbom.sh --target "$target" \
            --output "$work/first-$target.json" >/dev/null; then
        fail "the merge failed for $target"
        continue
    fi
    if ! tools/generate-product-sbom.sh --target "$target" \
            --output "$work/second-$target.json" >/dev/null; then
        fail "the merge failed for $target on its second run"
        continue
    fi
    if ! cmp -s "$work/first-$target.json" "$work/second-$target.json"; then
        fail "two consecutive merges disagree for $target"
        # `| head` closes the pipe early, and under `pipefail` that would take
        # the gate down before it printed a summary.
        diff -u "$work/first-$target.json" "$work/second-$target.json" | head -n 20 >&2 || true
    fi

    check
    if ! cmp -s "$committed" "$work/first-$target.json"; then
        fail "$committed is stale; regenerate with tools/generate-product-sbom.sh --all"
        diff -u "$committed" "$work/first-$target.json" | head -n 40 >&2 || true
    fi

    # The document names the two files it was merged from, with their digests.
    # A merge of one thing against a different copy of the other would agree
    # with itself everywhere else.
    check
    jq -r --arg n "$SBOM_NS" '.metadata.properties[]
        | select(.name == $n + ":merged-from") | .value' "$committed" \
        | native_strip_cr | LC_ALL=C sort > "$work/claimed-inputs.txt"
    printf '%s@sha256:%s\n%s@sha256:%s\n' \
        "$fragment" "$(native_sha256 "$fragment")" \
        "$inventory" "$(native_sha256 "$inventory")" \
        | LC_ALL=C sort > "$work/actual-inputs.txt"
    if ! cmp -s "$work/claimed-inputs.txt" "$work/actual-inputs.txt"; then
        fail "$committed was merged from files that are no longer these:"
        diff -u "$work/actual-inputs.txt" "$work/claimed-inputs.txt" | sed 's/^/  /' >&2 || true
    fi

    # 5. the independent answer
    check
    if ! jq -r \
            --slurpfile fragment "$fragment" \
            --slurpfile inventory "$inventory" \
            --arg target "$target" \
            --arg ns "$SBOM_NS" \
            --arg spec "$CYCLONEDX_SPEC_VERSION" \
            --arg format "$PRODUCT_FORMAT" \
            -f tools/product/oracle.jq "$committed" 2>"$work/oracle-$target.err" \
            | native_strip_cr > "$work/oracle-$target.txt"; then
        fail "the oracle could not read $committed:"
        sed 's/^/  /' "$work/oracle-$target.err" >&2
        continue
    fi
    bounds="$(sed -n 's/^bounds\t//p' "$work/oracle-$target.txt")"
    grep -v '^bounds	' "$work/oracle-$target.txt" > "$work/findings-$target.txt" || true
    if [ -s "$work/findings-$target.txt" ]; then
        fail "the oracle disagrees with $committed:"
        sed 's/^/  /' "$work/findings-$target.txt" >&2
    fi
    [ -n "$bounds" ] || fail "the oracle did not say how much it compared for $target"

    components="$(jq -r '.components | length' "$committed" | native_strip_cr)"
    echo "$PRODUCT_TOOL: $target - $components components"
    echo "$PRODUCT_TOOL: $target - $bounds"
done

# ---------------------------------------------------------------------------
# Across targets.
# ---------------------------------------------------------------------------
#
# Three documents that were identical would mean the target argument decides
# nothing, and every per-target question above would still be answered.

check
present=0
for target in "${NOTICE_TARGETS[@]}"; do
    [ -f "$(product_output_for "$target")" ] && present=$((present + 1))
done
if [ "$present" -ne "${#NOTICE_TARGETS[@]}" ]; then
    fail "only $present of ${#NOTICE_TARGETS[@]} product targets have a merged document"
else
    distinct="$(for target in "${NOTICE_TARGETS[@]}"; do
        jq -S -c '[.components[]."bom-ref"]' "$(product_output_for "$target")" | native_strip_cr
    done | LC_ALL=C sort -u | wc -l | tr -d ' ')"
    if [ "$distinct" -ne "${#NOTICE_TARGETS[@]}" ]; then
        fail "the ${#NOTICE_TARGETS[@]} product SBOMs describe only $distinct distinct component sets, so the target argument decides nothing"
    fi

    # And the difference is not only in the Rust half: the native side has to
    # differ too, or the merge is filtering nothing.
    check
    distinct="$(for target in "${NOTICE_TARGETS[@]}"; do
        jq -S -c '[.components[]."bom-ref" | select(startswith("native+"))]' \
            "$(product_output_for "$target")" | native_strip_cr
    done | LC_ALL=C sort -u | wc -l | tr -d ' ')"
    [ "$distinct" -gt 1 ] \
        || fail 'every target carries the same native components, so the native target filter decides nothing'

    # The Windows import library is linker metadata for one platform. On any
    # other target it is a file from another build.
    for target in "${NOTICE_TARGETS[@]}"; do
        check
        held="$(jq -r '[.components[]."bom-ref" | select(test("import-library"))] | length' \
            "$(product_output_for "$target")" | native_strip_cr)"
        case "$target" in
            *windows*) [ "$held" -eq 1 ] \
                || fail "$target carries $held import libraries and it is the one target that has one" ;;
            *) [ "$held" -eq 0 ] \
                || fail "$target carries a Windows import library" ;;
        esac
    done
fi

# Exactly one document in this repository may call itself complete, and it is
# each target's merged SBOM. An input that started saying so would make the
# claim meaningless.
check
if grep -rln --include='*.json' '"ferritecad:sbom:complete"' sbom \
        | grep -v "^$PRODUCT_DIR/" > "$work/claimants.txt"; then
    while IFS= read -r f; do
        value="$(jq -r --arg n "$SBOM_NS" '[.metadata.properties[]?
            | select(.name == $n + ":complete") | .value] | join(",")' "$f" | native_strip_cr)"
        [ "$value" = "false" ] \
            || fail "$f is not a product SBOM and says complete=$value"
    done < "$work/claimants.txt"
fi

if [ "$failures" -gt 0 ]; then
    echo >&2
    echo "$PRODUCT_TOOL: $failures of $checks checks failed" >&2
    exit 1
fi

echo "$PRODUCT_TOOL: $checks checks passed"
