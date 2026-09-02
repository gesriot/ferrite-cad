#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The independent gate on the production FBX 7.4 ASCII writer.
#
# The Rust gates beside the writer read what the writer wrote with a reader
# written next to it, which cannot notice a misunderstanding the two share.
# This one hands the same bytes to pinned ufbx 0.23.0 in strict mode, which
# has never heard of FerriteCAD, and asks it whether the §22B-1a contract
# survived: FBX 7400 ASCII, the axis and unit metadata, the hierarchy and its
# exact parents, one geometry connected to both placements, the converted
# vertices, the authored normals, the polygon order, the material slots, the
# local and world matrices, the omission properties, and the one string
# escaping rule.
#
# The bytes are produced here, from the production writer, into a temporary
# directory. No committed measurement fixture takes part.
#
# Run from the repository root:
#   tools/check-fbx-writer.sh

set -euo pipefail

record=0
if [ "${1:-}" = "--record" ]; then
    record=1
elif [ "$#" -ne 0 ]; then
    echo "usage: $0 [--record]" >&2
    exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
smoke="$root/tools/unity-fbx-smoke"
work="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-fbx-writer.XXXXXX")"
trap 'rm -rf "$work"' EXIT

command -v cargo >/dev/null || { echo "error: cargo is not on PATH" >&2; exit 1; }
command -v jq >/dev/null || { echo "error: jq is not on PATH" >&2; exit 1; }

suffix=''
link_math=('-lm')
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        suffix='.exe'
        # clang targeting MSVC has no separate maths library.
        link_math=()
        ;;
    Darwin)
        link_math=()
        ;;
esac

# A path as a native program sees it. Git Bash hands a POSIX path to a Windows
# binary and hopes its heuristics convert it; `cygpath` says so instead. The
# clang that builds the reader and the Rust binaries that write and read the
# FBX are all native programs, so every path they are given goes through here.
native() {
    if [ -n "$suffix" ] && command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$1"
    else
        printf '%s' "$1"
    fi
}

compiler="${CC:-}"
if [ -z "$compiler" ]; then
    for candidate in clang gcc cc; do
        if command -v "$candidate" >/dev/null 2>&1; then
            compiler="$candidate"
            break
        fi
    done
fi
if [ -z "$compiler" ]; then
    echo "error: no C compiler for the independent reader; set CC" >&2
    exit 1
fi
echo "independent reader compiler: $compiler"

# The production bytes. This is the writer, reached the only way §22B-1b2
# offers: a gate instrument, not a command. There is deliberately no
# `export-fbx`, no destination path policy and no filesystem publication yet.
artefacts="$(cargo build -p ferritecad-export --example fbx_gate_artefacts \
    --message-format=json 2>/dev/null \
    | jq -r 'select(.reason == "compiler-artifact")
             | select(.target.name == "fbx_gate_artefacts")
             | .executable // empty' \
    | head -1)"
if [ -z "$artefacts" ] || [ ! -x "$artefacts" ]; then
    echo "error: the gate artefact writer was not built" >&2
    exit 1
fi

"$artefacts" "$(native "$work")"
for name in fcad-measured.fbx fcad-escaping.fbx; do
    if [ ! -s "$work/$name" ]; then
        echo "error: the writer produced no bytes for $name" >&2
        exit 1
    fi
done

# The same pinned, digest-checked ufbx the §22B-1a measurement used. It is a
# gate, not a production dependency.
cache="$("$smoke/scripts/fetch_ufbx.sh")"

reader="$work/read_production$suffix"
"$compiler" -std=c11 -O2 -Wall -Wextra -Werror \
    -I "$(native "$cache")" "$(native "$smoke/scripts/read_production.c")" \
    "$(native "$cache/ufbx.c")" \
    "${link_math[@]+"${link_math[@]}"}" -o "$(native "$reader")"

# Written twice from one scene, and the second copy must be the same bytes.
mkdir -p "$work/again"
"$artefacts" "$(native "$work/again")" >/dev/null
for name in fcad-measured.fbx fcad-escaping.fbx; do
    if ! cmp -s "$work/$name" "$work/again/$name"; then
        echo "error: two writes of one scene produced different $name" >&2
        exit 1
    fi
done
echo "two writes of each scene are byte-identical"

# And the same bytes on every platform. Both scenes are arithmetic over an
# in-memory value: no kernel, no document and no tessellator takes part, so a
# digest that differs between Linux, macOS and Windows is a writer that is no
# longer a function of its input.
readonly DIGESTS="$root/tools/fbx/digests.tsv"
digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}
if [ "$record" -eq 1 ]; then
    {
        for name in fcad-measured.fbx fcad-escaping.fbx; do
            printf '%s\t%s\n' "$(digest "$work/$name")" "$name"
        done
    } >"$DIGESTS"
    echo "recorded $DIGESTS"
else
    [ -f "$DIGESTS" ] || { echo "error: $DIGESTS is missing" >&2; exit 1; }
    while IFS=$'\t' read -r expected name; do
        [ -n "$name" ] || continue
        actual="$(digest "$work/$name")"
        if [ "$actual" != "$expected" ]; then
            echo "error: $name is $actual here and $expected in $DIGESTS" >&2
            exit 1
        fi
        echo "  $name matches the recorded digest"
    done <"$DIGESTS"
fi

output="$work/report.txt"
if ! "$reader" "$(native "$work/fcad-measured.fbx")" "$(native "$work/fcad-escaping.fbx")" \
    | tee "$output"; then
    echo "error: the independent reader refused the production bytes" >&2
    exit 1
fi

# A report without the live anchor, or with no checks, is not a result.
anchor="$(grep -c '^FCAD_PRODUCTION_FBX_UFBX_EXECUTED ' "$output" || true)"
if [ "$anchor" != "1" ]; then
    echo "error: the independent reader did not run to the end" >&2
    exit 1
fi
count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) .*$/\1/p' "$output")"
if [ -z "$count" ] || [ "$count" -lt 100 ]; then
    echo "error: the independent reader performed ${count:-0} checks" >&2
    exit 1
fi
echo "production FBX: pinned ufbx accepted the writer's bytes over $count checks"
