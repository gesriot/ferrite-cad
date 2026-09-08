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
# Needs Open CASCADE: the committed STEP fixture is imported through the
# shipped `import-step`, the external file is deleted, and the FBX is written
# by the shipped `export-fbx` from the stored bytes alone. So the bytes handed
# to ufbx here are the bytes a person gets. The written file is a temporary
# artefact of one build's tessellation and is never committed.
#
# Run from the repository root:
#   tools/check-fbx-complex.sh [--release --features planegcs]

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
FCAD_FBX_COMPLEX_OUT="$(native "$artefact")" FCAD_FBX_JSON_OUT="$(native "$work")" \
    cargo test "$@" -p ferritecad-cli --test export_fbx_complex -- --nocapture 2>&1 \
    | tee "$work/rust.log"

if ! grep -q '^FCAD_EXPORT_FBX_COMPLEX ' "$work/rust.log"; then
    if [ "$FERRITECAD_REQUIRE_OCCT" != 1 ] && \
       grep -q 'skipped: this build has no Open CASCADE' "$work/rust.log"; then
        echo "skipped: this build has no Open CASCADE"
        exit 0
    fi
    echo "error: the complex writer gate did not run" >&2
    exit 1
fi
if grep -q 'skipped:' "$work/rust.log"; then
    echo "error: a required FBX process route skipped" >&2
    exit 1
fi
for gate in json::native_json_fbx_complete_publication \
    json::json_fbx_refusals_preserve_files_and_protocol_in_native_and_stub_builds \
    the_complex_assembly_becomes_one_fbx_that_keeps_every_definition_and_says_what_is_missing
do
    if ! grep -q "^test ${gate} \.\.\. ok$" "$work/rust.log"; then
        echo "error: ${gate} did not execute and pass" >&2
        exit 1
    fi
done
grep -q '^FCAD_EXPORT_FBX_JSON_COMPLETE native=1 imported=1 delivery=8$' "$work/rust.log"
grep -q '^FCAD_EXPORT_FBX_JSON_PARTIAL omissions=1 delivery=4$' "$work/rust.log"
[ -s "$artefact" ] || { echo "error: the gate left no FBX to read" >&2; exit 1; }

cache="$("$smoke/scripts/fetch_ufbx.sh")"
reader="$work/read_production$suffix"
# Two compilations, not one. Warnings as errors are for the reader written
# here; ufbx is a pinned third-party file whose warning-cleanliness is a
# property of somebody else's compiler. Clang targeting MSVC does not give
# `ufbxi_unused` the GCC attribute, so four of its deliberately unused
# helpers become errors on Windows and nowhere else, which says nothing
# about the bytes this gate is reading.
"$compiler" -std=c11 -O2 -Wall -Wextra -Werror \
    -I "$(native "$cache")" -c "$(native "$smoke/scripts/read_production.c")" \
    -o "$(native "$work/read_production.o")"
"$compiler" -std=c11 -O2 -I "$(native "$cache")" \
    -c "$(native "$cache/ufbx.c")" -o "$(native "$work/ufbx.o")"
"$compiler" "$(native "$work/read_production.o")" "$(native "$work/ufbx.o")" \
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

# These are JSON-process publications, not files generated by a writer example.
# The Rust scanner compares every report counter and each omitted placement;
# strict pinned ufbx retains the independent format/identity/complex checks.
for name in native imported partial; do
    [ -s "$work/$name.fbx" ] || { echo "error: missing JSON FBX $name" >&2; exit 1; }
    mode=--identity
    minimum=5
    if [ "$name" = partial ]; then mode=--complex; minimum=250; fi
    "$reader" "$mode" "$(native "$work/$name.fbx")" | tee "$work/$name-reader.txt"
    count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/$name-reader.txt")"
    [ -n "$count" ] && [ "$count" -ge "$minimum" ] || {
        echo "error: pinned reader did not verify JSON FBX $name" >&2; exit 1;
    }
done
