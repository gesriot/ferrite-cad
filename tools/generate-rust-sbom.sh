#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Generates the deterministic CycloneDX 1.5 fragment describing the Rust part
# of the FerriteCAD product for one target triple.
#
# This is a fragment and says so in its own metadata. It describes the union of
# the runtime dependency graphs of the two shipped binaries and nothing else:
# no Open CASCADE, no planegcs, no Eigen, no Boost, no fonts and no staged
# files. Merging those is §21A-2b2b0b2b.
#
# Why there is a normalizer at all, measured on cargo-cyclonedx 0.5.9 rather
# than assumed:
#
#   * It cannot express a feature-resolved graph. `--features`, `-F` and
#     `--no-default-features` change nothing about which packages it reports:
#     generating the viewer's BOM with and without `-F planegcs` produced
#     identical component sets. What it reports is the resolve graph filtered
#     by target cfg only, which keeps every optional dependency whether or not
#     a feature turns it on. Measured on aarch64-apple-darwin: 165 dependency
#     components against the 144 cargo's own resolver reaches, including the
#     Vulkan, GLES and RenderDoc backends of a wgpu built with none of them.
#   * It writes a random `serialNumber` and a wall-clock `timestamp` unless
#     SOURCE_DATE_EPOCH is set, and absolute host paths into the `bom-ref` and
#     `purl` of every workspace package regardless.
#   * `--describe binaries` emits one file per binary, so the union of the two
#     shipped binaries has to be taken by something.
#
# So cargo-cyclonedx stays the source of what each component *is* – its
# licence expression, its purl, its crate digest, its description and its
# external references – and the graph is taken from cargo's own feature-aware
# resolver. Nothing here invents a component.
#
# Runs no network. `cargo metadata --locked` is asked first so that a stale
# Cargo.lock is a refusal rather than a silent update.
#
# Run from the repository root:
#   tools/generate-rust-sbom.sh --target <triple> --output <file>
#   tools/generate-rust-sbom.sh --all      # rewrites the committed fragments

set -euo pipefail

SBOM_TOOL='generate-rust-sbom'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/sbom/lib.sh
. tools/sbom/lib.sh

target=''
output=''
all=''
while [ $# -gt 0 ]; do
    case "$1" in
        --target) target="${2:?--target needs a triple}"; shift 2 ;;
        --output) output="${2:?--output needs a path}"; shift 2 ;;
        --all) all=1; shift ;;
        *) sbom_die "unknown argument: $1" ;;
    esac
done

if [ -n "$all" ]; then
    [ -z "$target$output" ] || sbom_die '--all takes no other argument'
else
    [ -n "$target" ] || sbom_die 'expected --target <triple> --output <file>, or --all'
    [ -n "$output" ] || sbom_die '--target needs --output'
fi

sbom_load_pin
sbom_require_jq
sbom_require_cyclonedx

work="$(mktemp -d)"
# cargo-cyclonedx writes beside each manifest and has no way to be told
# otherwise, so the working tree is left as it was found even on failure.
cleanup() {
    local root bin manifest
    for root in "${NOTICE_ROOTS[@]}"; do
        bin="${root%%|*}"
        manifest="${root#*|}"; manifest="${manifest%%|*}"
        rm -f "$(dirname "$manifest")/${bin}_bin.cdx.json"
    done
    rm -rf "$work"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# The graph, from cargo's own feature-aware resolver.
# ---------------------------------------------------------------------------

# `--prefix depth` prints the depth with no separator before the package name,
# which would be ambiguous for a crate whose name starts with a digit. Cargo
# allows such names, so this refuses rather than mis-parsing one.
guard_package_names() {
    local bad
    bad="$(awk -F'"' '/^name = /{print $2}' Cargo.lock | grep '^[0-9]' || true)"
    [ -z "$bad" ] || sbom_die \
        "Cargo.lock contains a package whose name starts with a digit, which this parser cannot read from cargo tree --prefix depth: $bad"
}

# One root's graph: every edge cargo walks for that binary on that target,
# normal dependencies only, plus the key of the root itself.
#
# Build dependencies are deliberately out. They are not linked into either
# shipped binary, and `cargo tree -e build` answers partly about the machine
# doing the building rather than about the target, which would make the same
# target's fragment differ between hosts. The notices keep them under a
# different rule and for a different purpose; the exclusion is declared in the
# fragment's own metadata so a reader is never left to guess.
root_graph() { # binary manifest features target nodes edges roots
    local bin="$1" manifest="$2" features="$3" target="$4"
    local nodes="$5" edges="$6" roots="$7"
    local -a tree=(cargo tree --locked --target "$target" -e normal
                   --prefix depth --format '{p}' --manifest-path "$manifest")
    [ -z "$features" ] || tree+=(--features "$features")

    "${tree[@]}" 2>/dev/null | tr -d '\r' \
        | awk -v bin="$bin" -v nodes="$nodes" -v edges="$edges" -v roots="$roots" '
            {
                if (match($0, /^[0-9]+/) == 0) next
                depth = substr($0, 1, RLENGTH) + 0
                rest = substr($0, RLENGTH + 1)
                # "<name> v<version>[ (proc-macro)][ (*)][ (<path>)]"
                if (match(rest, /^[^ ]+ v[^ ]+/) == 0) next
                head = substr(rest, 1, RLENGTH)
                i = index(head, " v")
                key = substr(head, 1, i - 1) "@" substr(head, i + 2)
                stack[depth] = key
                print key >> nodes
                if (depth > 0) print stack[depth - 1] "\t" key >> edges
                else print bin "\t" key >> roots
            }
            END { if (!(0 in stack)) exit 1 }' \
        || sbom_die "cargo tree produced no graph for $bin on $target"
}

# ---------------------------------------------------------------------------
# The component records, from cargo-cyclonedx.
# ---------------------------------------------------------------------------

run_cyclonedx() { # target
    local target="$1" root bin manifest produced stray
    # `--describe binaries` emits one document per binary in the workspace.
    # The workspace manifest is the entry point on purpose: a binary this
    # project does not declare then shows up as an extra file rather than
    # being quietly absent from a fragment that claims to describe the
    # product.
    SOURCE_DATE_EPOCH=0 cargo cyclonedx \
        --manifest-path Cargo.toml \
        --format json \
        --spec-version "$CYCLONEDX_SPEC_VERSION" \
        --describe binaries \
        --target "$target" \
        --no-build-deps \
        --all \
        --quiet >/dev/null 2>&1 \
        || sbom_die "cargo-cyclonedx failed for $target"

    for root in "${NOTICE_ROOTS[@]}"; do
        bin="${root%%|*}"
        manifest="${root#*|}"; manifest="${manifest%%|*}"
        produced="$(dirname "$manifest")/${bin}_bin.cdx.json"
        [ -f "$produced" ] || sbom_die \
            "cargo-cyclonedx produced no document for the shipped binary '$bin'"
        mv "$produced" "$work/bom-$bin.json"
    done

    # Found by generating from a copy of the tree that was not a git checkout:
    # the first spelling of this asked git, and git answered "not a repository"
    # into a discarded stream, so the check passed by doing nothing.
    # `-prune` rather than `-not -path`: the latter still descends into the
    # build directory, and on a repository with a warm target/ that turned a
    # four second gate into a several minute one.
    stray="$(find . \( -path ./target -o -path ./.git \) -prune -o \
                  -name '*_bin.cdx.json' -print | sort | tr '\n' ' ')"
    [ -z "$stray" ] || sbom_die \
        "cargo-cyclonedx described binaries that tools/notices/lib.sh does not name: $stray"
}

# ---------------------------------------------------------------------------
# The fragment.
# ---------------------------------------------------------------------------

generate() { # target output
    local target="$1" out="$2" root bin manifest features

    : > "$work/nodes.txt"
    : > "$work/edges.tsv"
    : > "$work/roots.tsv"
    for root in "${NOTICE_ROOTS[@]}"; do
        bin="${root%%|*}"
        manifest="${root#*|}"; manifest="${manifest%%|*}"
        features="${root##*|}"
        root_graph "$bin" "$manifest" "$features" "$target" \
            "$work/nodes.txt" "$work/edges.tsv" "$work/roots.tsv"
    done
    sort -u "$work/nodes.txt" -o "$work/nodes.txt"
    sort -u "$work/edges.tsv" -o "$work/edges.tsv"
    sort -u "$work/roots.tsv" -o "$work/roots.tsv"

    run_cyclonedx "$target"

    # Cargo.lock owns every package's exact source and digest.
    awk -f tools/sbom/cargo-lock.awk Cargo.lock | sbom_strip_cr > "$work/lock.tsv"

    # Where each workspace package lives inside this repository. Taken against
    # cargo's own workspace root so the answer is the same on a host that
    # spells absolute paths with drive letters and backslashes.
    cargo metadata --locked --format-version 1 --no-deps 2>/dev/null \
        | jq -r '(.workspace_root | gsub("\\\\"; "/")) as $root
                 | .packages[]
                 | (.manifest_path | gsub("\\\\"; "/")) as $m
                 | [.name + "@" + .version,
                    ($m | ltrimstr($root + "/") | rtrimstr("/Cargo.toml"))]
                 | @tsv' \
        | sbom_strip_cr | sort -u > "$work/paths.tsv"

    # A real file, not a process substitution. jq on Windows is a native
    # binary and the shell running this is MSYS, so `<(...)` hands it a
    # /proc/<pid>/fd path it cannot open. Measured on the first three-host run:
    # `Bad JSON in --slurpfile boms /proc/799/fd/63`.
    cat "$work"/bom-*.json > "$work/boms.json"

    jq -n -S \
        --rawfile nodes "$work/nodes.txt" \
        --rawfile edges "$work/edges.tsv" \
        --rawfile roots "$work/roots.tsv" \
        --rawfile lock "$work/lock.tsv" \
        --rawfile paths "$work/paths.tsv" \
        --rawfile risk "$SBOM_RISK_TSV" \
        --slurpfile boms "$work/boms.json" \
        --arg target "$target" \
        --arg ns "$SBOM_NS" \
        --arg spec "$CYCLONEDX_SPEC_VERSION" \
        --arg tool "$CARGO_CYCLONEDX_VERSION" \
        --arg format "$SBOM_FRAGMENT_FORMAT" \
        -f tools/sbom/fragment.jq \
        | sbom_strip_cr > "$work/out.json"
    mv "$work/out.json" "$out"
}

# A stale lock would make cargo-cyclonedx and cargo tree answer about
# different graphs, and cargo-cyclonedx has no --locked of its own.
cargo metadata --locked --format-version 1 --no-deps >/dev/null 2>&1 \
    || sbom_die 'cargo metadata --locked failed: Cargo.lock is stale or unreadable'

guard_package_names

if [ -n "$all" ]; then
    mkdir -p "$SBOM_DIR"
    for target in "${NOTICE_TARGETS[@]}"; do
        generate "$target" "$(sbom_output_for "$target")"
        echo "$SBOM_TOOL: wrote $(sbom_output_for "$target")"
    done
else
    generate "$target" "$output"
    echo "$SBOM_TOOL: wrote $output"
fi
