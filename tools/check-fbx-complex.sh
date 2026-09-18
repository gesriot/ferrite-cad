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

# §25C supplies an actual saved-Sketch worker publication from the preceding
# runtime step. Reuse this reader build; no second complex STEP import.
if [ -n "${FCAD_SKETCH_DRAG_FBX:-}" ]; then
    "$reader" --identity "$FCAD_SKETCH_DRAG_FBX" | tee "$work/sketch-drag-reader.txt"
    count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/sketch-drag-reader.txt")"
    [ -n "$count" ] && [ "$count" -ge 6 ] || {
        echo "error: pinned reader did not verify the dragged Sketch FBX" >&2; exit 1;
    }
    echo "FCAD_SKETCH_DRAG_UFBX_EXECUTED"
fi

# §25E reuses this same pinned reader for the actual H/V publications;
# Later constraint slices add their actual process/worker publications to the
# same reader build; §25I includes the slanted relative-orientation models.
if [ -n "${FCAD_SKETCH_CONSTRAINT_FBX_DIR:-}" ]; then
    for name in horizontal vertical length-rectangle length-replaced length-slanted pinned pin-moved square smaller equal-ui equal-cli relations-rect relations-smaller relations-freed relation-ui relation-cli; do
        "$reader" --identity "$FCAD_SKETCH_CONSTRAINT_FBX_DIR/$name.fbx" | tee "$work/constraint-$name-reader.txt"
        count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/constraint-$name-reader.txt")"
        [ -n "$count" ] && [ "$count" -ge 6 ] || {
            echo "error: pinned reader did not verify constrained $name FBX" >&2; exit 1;
        }
    done
    echo "FCAD_SKETCH_CONSTRAINT_UFBX_EXECUTED"
fi

# §25J adds the analytic circle's actual publications — the CLI process, the
# same model after a height edit, and the UI/peer-CLI pair — to this same
# reader build, and §25K the copies its centre/radius edit publishes. Small
# files, actually read rather than merely produced.
if [ -n "${FCAD_CIRCLE_FBX_DIR:-}" ]; then
    for name in circle circle-taller circle-ui circle-cli circle-edited circle-edit-ui circle-edit-cli; do
        "$reader" --identity "$FCAD_CIRCLE_FBX_DIR/$name.fbx" | tee "$work/circle-$name-reader.txt"
        count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/circle-$name-reader.txt")"
        [ -n "$count" ] && [ "$count" -ge 6 ] || {
            echo "error: pinned reader did not verify circle $name FBX" >&2; exit 1;
        }
    done
    echo "FCAD_CIRCLE_UFBX_EXECUTED"
fi

# §25L adds the hollow part's actual publications to the same reader build: the
# CLI process, the same model after a height edit, and the UI/peer-CLI pair.
# Four more small files, actually read rather than merely produced.
if [ -n "${FCAD_ANNULUS_FBX_DIR:-}" ]; then
    for name in annulus annulus-taller annulus-ui annulus-cli; do
        "$reader" --identity "$FCAD_ANNULUS_FBX_DIR/$name.fbx" | tee "$work/annulus-$name-reader.txt"
        count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/annulus-$name-reader.txt")"
        [ -n "$count" ] && [ "$count" -ge 6 ] || {
            echo "error: pinned reader did not verify annular $name FBX" >&2; exit 1;
        }
    done
    echo "FCAD_ANNULUS_UFBX_EXECUTED"
fi

# §25M adds the copies its centre/radii edit publishes to the same reader build:
# the CLI process and the UI/peer-CLI pair. Three more small files, actually
# read rather than merely produced.
if [ -n "${FCAD_ANNULUS_EDIT_FBX_DIR:-}" ]; then
    for name in annulus-edited-cli annulus-edit-ui annulus-edit-cli; do
        "$reader" --identity "$FCAD_ANNULUS_EDIT_FBX_DIR/$name.fbx" | tee "$work/annulus-edit-$name-reader.txt"
        count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/annulus-edit-$name-reader.txt")"
        [ -n "$count" ] && [ "$count" -ge 6 ] || {
            echo "error: pinned reader did not verify edited annular $name FBX" >&2; exit 1;
        }
    done
    echo "FCAD_ANNULUS_EDIT_UFBX_EXECUTED"
fi

# §25N adds the copy its radius/centre constraints publish to the same reader
# build: one small file, actually read rather than merely produced.
if [ -n "${FCAD_CIRCLE_CONSTRAINT_FBX_DIR:-}" ]; then
    "$reader" --identity "$FCAD_CIRCLE_CONSTRAINT_FBX_DIR/circle-constraint-cli.fbx" | tee "$work/circle-constraint-reader.txt"
    count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/circle-constraint-reader.txt")"
    [ -n "$count" ] && [ "$count" -ge 6 ] || {
        echo "error: pinned reader did not verify circle constraint FBX" >&2; exit 1;
    }
    echo "FCAD_CIRCLE_CONSTRAINT_UFBX_EXECUTED"
fi

# §25O adds the copy its concentric/radii/pin constraints publish to the same
# reader build: one small file, actually read rather than merely produced.
if [ -n "${FCAD_ANNULAR_CONSTRAINT_FBX_DIR:-}" ]; then
    "$reader" --identity "$FCAD_ANNULAR_CONSTRAINT_FBX_DIR/annular-constraint-cli.fbx" \
        | tee "$work/annular-constraint-reader.txt"
    count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/annular-constraint-reader.txt")"
    [ -n "$count" ] && [ "$count" -ge 6 ] || {
        echo "error: pinned reader did not verify annular constraint FBX" >&2; exit 1;
    }
    echo "FCAD_ANNULAR_CONSTRAINT_UFBX_EXECUTED"
fi

# §26A adds the two parts its cut publishes to the same reader build: a through
# hole and a pocket, actually read rather than merely produced.
if [ -n "${FCAD_CUT_FBX_DIR:-}" ]; then
    for name in cut-holed-cli cut-pocket-cli; do
        "$reader" --identity "$FCAD_CUT_FBX_DIR/$name.fbx" | tee "$work/$name-reader.txt"
        count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/$name-reader.txt")"
        [ -n "$count" ] && [ "$count" -ge 6 ] || {
            echo "error: pinned reader did not verify cut $name FBX" >&2; exit 1;
        }
    done
    echo "FCAD_CUT_UFBX_EXECUTED"
fi

# §26B adds the copies its cut-parameter edit publishes to the same reader
# build: each independently edited parameter, the combined edit, and a hole
# shortened into a pocket. Every produced artifact is actually read.
if [ -n "${FCAD_CUT_EDIT_FBX_DIR:-}" ]; then
    for name in cut-edit-centre-cli cut-edit-radius-cli cut-edit-depth-cli cut-edit-all-cli cut-edit-shortened-cli; do
        "$reader" --identity "$FCAD_CUT_EDIT_FBX_DIR/$name.fbx" | tee "$work/$name-reader.txt"
        count="$(sed -n 's/^FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=\([0-9]*\) failures=0$/\1/p' "$work/$name-reader.txt")"
        [ -n "$count" ] && [ "$count" -ge 6 ] || {
            echo "error: pinned reader did not verify cut edit $name FBX" >&2; exit 1;
        }
    done
    echo "FCAD_CUT_EDIT_UFBX_EXECUTED"
fi
