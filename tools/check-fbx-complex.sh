#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The independent gate on the production FBX written from the real complex
# STEP assembly.
#
# The Rust gate beside it already compares the file with the `ExportScene` it
# came from, node for node and transform for transform. This asks pinned ufbx
# 0.23.0, in strict mode, the questions only an outside reader can answer
# about the whole file: that 46 definitions are still represented, that there
# are 140 nodes below one root, that there are 34 geometries rather than the
# 112 draws a flattened picture of this document has, that `#2428`'s
# placements are connected to one geometry object, and that `#2583` is a node
# with no triangles carrying the omission properties.
#
# Needs Open CASCADE: the scene is built by importing the committed STEP
# fixture through the shipped command and reopening the stored bytes. The
# written file is a temporary artefact of one build's tessellation and is
# never committed.
#
# Run from the repository root:
#   tools/check-fbx-complex.sh

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
smoke="$root/tools/unity-fbx-smoke"
work="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-fbx-complex.XXXXXX")"
trap 'rm -rf "$work"' EXIT

command -v cargo >/dev/null || { echo "error: cargo is not on PATH" >&2; exit 1; }

suffix=''
link_math=('-lm')
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) suffix='.exe'; link_math=() ;;
    Darwin) link_math=() ;;
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
[ -n "$compiler" ] || { echo "error: no C compiler for the independent reader; set CC" >&2; exit 1; }

artefact="$work/complex.fbx"
# A missing kernel makes the gate skip itself rather than fail, exactly as the
# Rust test does; the OCCT pin sets this and so cannot be skipped.
export FERRITECAD_REQUIRE_OCCT="${FERRITECAD_REQUIRE_OCCT:-}"
FCAD_FBX_COMPLEX_OUT="$(native "$artefact")" \
    cargo test -p ferritecad-cli --test export_fbx_complex -- --nocapture 2>&1 \
    | tee "$work/rust.log"

if ! grep -q '^FCAD_EXPORT_FBX_COMPLEX ' "$work/rust.log"; then
    if grep -q 'skipped: this build has no Open CASCADE' "$work/rust.log"; then
        echo "skipped: this build has no Open CASCADE"
        exit 0
    fi
    echo "error: the complex writer gate did not run" >&2
    exit 1
fi
[ -s "$artefact" ] || { echo "error: the gate left no FBX to read" >&2; exit 1; }

cache="$("$smoke/scripts/fetch_ufbx.sh")"
reader="$work/read_production$suffix"
"$compiler" -std=c11 -O2 -Wall -Wextra -Werror \
    -I "$(native "$cache")" "$(native "$smoke/scripts/read_production.c")" \
    "$(native "$cache/ufbx.c")" \
    "${link_math[@]+"${link_math[@]}"}" -o "$(native "$reader")"

output="$work/report.txt"
if ! "$reader" --complex "$(native "$artefact")" | tee "$output"; then
    echo "error: the independent reader refused the complex production bytes" >&2
    exit 1
fi

count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) .*$/\1/p' "$output")"
if [ -z "$count" ] || [ "$count" -lt 250 ]; then
    echo "error: the independent reader performed ${count:-0} checks" >&2
    exit 1
fi
echo "complex FBX: pinned ufbx accepted the writer's bytes over $count checks"
