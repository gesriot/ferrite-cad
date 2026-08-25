#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The gate on the Rust CycloneDX fragments.
#
# It asks five separate questions of every product target, and a green run
# means all five were answered, not that nothing crashed:
#
#   1. the fragment exists and is checked in;
#   2. it validates against the pinned CycloneDX 1.5 schema, offline, using
#      the schema files this repository carries and their recorded digests;
#   3. it carries no timestamp, no serial number, no absolute path and nothing
#      else that is a property of the machine that produced it;
#   4. regenerating it twice reproduces the checked-in bytes exactly;
#   5. an oracle built from cargo metadata and Cargo.lock - not from anything
#      the generator wrote - agrees about the composition.
#
# Under ADR 0003 a recorded licence risk is not a refusal. Components carrying
# `KNOWN LICENCE RISK` are counted and printed; they never change the exit
# code. What is checked is that the marking is accurate in both directions.
#
# Runs no network.
#
# Run from the repository root:
#   tools/check-rust-sbom.sh

set -euo pipefail

SBOM_TOOL='check-rust-sbom'
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

failures=0
checks=0

check() { checks=$((checks + 1)); }

fail() {
    failures=$((failures + 1))
    echo "$SBOM_TOOL: $*" >&2
}

# ---------------------------------------------------------------------------
# The schema, made resolvable without a network.
# ---------------------------------------------------------------------------
#
# bom-1.5.schema.json declares an http `$id` and refers to its two companions
# by relative name, so a validator resolves them against cyclonedx.org.
# Measured: with the companions deleted from the directory, an instance with a
# bogus SPDX licence id was still correctly refused, which is only possible if
# the enumeration came off the network. Dropping the `$id` makes the base the
# file itself, and the same experiment then fails to even compile the schema.
# tools/sbom/schema/PROVENANCE.md records both measurements.
mkdir -p "$work/schema"
cp "$SBOM_SCHEMA_BOM" "$SBOM_SCHEMA_SPDX" "$SBOM_SCHEMA_JSF" "$work/schema/"
jq 'del(."$id")' "$SBOM_SCHEMA_BOM" | sbom_strip_cr > "$work/schema/bom-1.5.schema.json"

# ---------------------------------------------------------------------------
# The product roots, resolved to packages without asking the generator.
# ---------------------------------------------------------------------------

cargo metadata --locked --format-version 1 --no-deps 2>/dev/null > "$work/nodeps.json" \
    || sbom_die 'cargo metadata --locked failed: Cargo.lock is stale or unreadable'

: > "$work/rootspec.tsv"
for root in "${NOTICE_ROOTS[@]}"; do
    bin="${root%%|*}"
    manifest="${root#*|}"; manifest="${manifest%%|*}"
    pkg="$(jq -r --arg m "$manifest" '
        (.workspace_root | gsub("\\\\"; "/")) as $r
        | .packages[]
        | select((.manifest_path | gsub("\\\\"; "/") | ltrimstr($r + "/")) == $m)
        | .name' "$work/nodeps.json" | sbom_strip_cr)"
    [ -n "$pkg" ] || sbom_die "no workspace package is built from $manifest"
    printf '%s\t%s\n' "$bin" "$pkg" >> "$work/rootspec.tsv"
done

awk -f tools/sbom/cargo-lock.awk Cargo.lock | sbom_strip_cr > "$work/lock.tsv"

# ---------------------------------------------------------------------------
# Per target.
# ---------------------------------------------------------------------------

# Data this document must never carry: it would either differ between hosts or
# tell a reader where somebody's checkout happened to live.
readonly HOST_PATTERNS='file://|/Users/|/home/|/root/|(^|[^A-Za-z0-9])[A-Za-z]:[\\/]|/private/var/|/tmp/|runner/work|AppData|RUNNER_TEMP'

for target in "${NOTICE_TARGETS[@]}"; do
    committed="$(sbom_output_for "$target")"

    check
    if [ ! -f "$committed" ]; then
        fail "there is no Rust CycloneDX fragment for $target at $committed"
        echo "  generate it with: tools/generate-rust-sbom.sh --all" >&2
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
    if grep -q $'\r' "$committed"; then
        fail "$committed carries carriage returns; .gitattributes should keep it out of conversion"
    fi

    # 4. two runs, and the checked-in bytes
    check
    tools/generate-rust-sbom.sh --target "$target" --output "$work/first-$target.json" \
        >/dev/null || { fail "the generator failed for $target"; continue; }
    tools/generate-rust-sbom.sh --target "$target" --output "$work/second-$target.json" \
        >/dev/null || { fail "the generator failed for $target on its second run"; continue; }
    if ! cmp -s "$work/first-$target.json" "$work/second-$target.json"; then
        fail "two consecutive runs of the generator disagree for $target"
        # `| head` closes the pipe early, and under `pipefail` that would take
        # the whole gate down before it printed a summary. Measured, not
        # guessed: a mutation that added a timestamp made this script exit
        # here, and the harness could not tell a failure from a crash.
        diff -u "$work/first-$target.json" "$work/second-$target.json" \
            | head -n 20 >&2 || true
    fi

    check
    if ! cmp -s "$committed" "$work/first-$target.json"; then
        fail "$committed is stale; regenerate with tools/generate-rust-sbom.sh --all"
        diff -u "$committed" "$work/first-$target.json" | head -n 40 >&2 || true
    fi

    # 5. the independent answer
    check
    cargo metadata --locked --format-version 1 --filter-platform "$target" \
        2>/dev/null > "$work/md-$target.json" \
        || { fail "cargo metadata failed for $target"; continue; }

    if ! jq -r \
            --slurpfile md "$work/md-$target.json" \
            --rawfile lock "$work/lock.tsv" \
            --rawfile risk "$SBOM_RISK_TSV" \
            --rawfile rootspec "$work/rootspec.tsv" \
            --arg target "$target" \
            --arg ns "$SBOM_NS" \
            --arg spec "$CYCLONEDX_SPEC_VERSION" \
            --arg format "$SBOM_FRAGMENT_FORMAT" \
            -f tools/sbom/oracle.jq "$committed" 2>"$work/oracle-$target.err" \
            | sbom_strip_cr > "$work/oracle-$target.txt"; then
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
    [ -n "$bounds" ] || fail "the oracle did not say how much it pinned for $target"

    # ADR 0003: counted and printed, never a refusal.
    risks="$(jq -r --arg n "$SBOM_NS:licence-risk" \
        '[.components[] | select((.properties // []) | any(.name == $n))] | length' \
        "$committed" | sbom_strip_cr)"
    components="$(jq -r '.components | length' "$committed" | sbom_strip_cr)"
    echo "$SBOM_TOOL: $target - $components components, $risks marked KNOWN LICENCE RISK"
    echo "$SBOM_TOOL: $target - $bounds"
done

# ---------------------------------------------------------------------------
# Across targets.
# ---------------------------------------------------------------------------
#
# Three fragments that were identical would mean the target argument does
# nothing, and every per-target check above would still pass.
check
present=0
for target in "${NOTICE_TARGETS[@]}"; do
    [ -f "$(sbom_output_for "$target")" ] && present=$((present + 1))
done
if [ "$present" -eq "${#NOTICE_TARGETS[@]}" ]; then
    distinct="$(for target in "${NOTICE_TARGETS[@]}"; do
        jq -S -c '[.components[]."bom-ref"]' "$(sbom_output_for "$target")" | sbom_strip_cr
    done | sort -u | wc -l | tr -d ' ')"
    if [ "$distinct" -ne "${#NOTICE_TARGETS[@]}" ]; then
        fail "the ${#NOTICE_TARGETS[@]} fragments describe only $distinct distinct component sets, so the target argument is not deciding anything"
    fi
else
    fail "only $present of ${#NOTICE_TARGETS[@]} product targets have a fragment"
fi

if [ "$failures" -gt 0 ]; then
    echo >&2
    echo "$SBOM_TOOL: $failures of $checks checks failed" >&2
    exit 1
fi

echo "$SBOM_TOOL: $checks checks passed"
