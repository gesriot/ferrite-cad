#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The §22B-1e3b gate: the identity a document recorded is the identity the
# shipped FBX carries, read back by a program that has never heard of
# FerriteCAD.
#
# The bytes come from the shipped route and nothing here fabricates them: the
# committed STEP fixture is imported through `import-step`, the external file
# is deleted, and `export-fbx` writes the FBX from the stored bytes alone —
# twice, in two processes, so a value minted at export time cannot survive.
# Then pinned ufbx 0.23.0 reads the file in strict mode, prints every node's
# identity properties verbatim, and an independent implementation of the wire
# grammar joins them to what the document says it recorded.
#
# The Rust gate beside it makes the same join inside the process. This one
# matters because it does not: two readers that share a misunderstanding agree,
# and two that share nothing do not.
#
# Needs Open CASCADE. Without it the gate skips itself exactly as the Rust test
# does; the OCCT pin sets FERRITECAD_REQUIRE_OCCT and so cannot skip.
#
# Run from the repository root:
#   tools/check-fbx-identity.sh

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
smoke="$root/tools/unity-fbx-smoke"
join="$root/tools/fbx-identity/scripts/join_identity.py"
work="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-fbx-identity.XXXXXX")"
trap 'rm -rf "$work"' EXIT

command -v cargo >/dev/null || { echo "error: cargo is not on PATH" >&2; exit 1; }
command -v python3 >/dev/null || { echo "error: python3 is not on PATH" >&2; exit 1; }

suffix=''
link_math=('-lm')
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) suffix='.exe'; link_math=() ;;
    Darwin) link_math=() ;;
esac

# A path as a native program sees it. Git Bash hands a POSIX path to a Windows
# binary and hopes its heuristics convert it; `cygpath` says so instead.
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

artefact="$work/identity.fbx"
export FERRITECAD_REQUIRE_OCCT="${FERRITECAD_REQUIRE_OCCT:-}"

# The Rust half runs first and its result is kept rather than acted on
# immediately: it publishes the artefact as soon as the model is proven to have
# arrived, so the outside reader can be run on the same bytes even when the
# inside join fails. Both halves are then reported together.
set +e
FCAD_FBX_IDENTITY_OUT="$(native "$artefact")" \
    cargo test -p ferritecad-cli --test export_fbx_identity -- --nocapture \
    >"$work/rust.log" 2>&1
rust_status=$?
set -e
cat "$work/rust.log"

if grep -q 'skipped: this build has no Open CASCADE' "$work/rust.log"; then
    echo "skipped: this build has no Open CASCADE"
    exit 0
fi
if [ ! -s "$artefact" ]; then
    echo "error: the gate left no FBX to read" >&2
    exit 1
fi
payload="${artefact%.fbx}.payload"
if [ ! -s "$payload" ]; then
    echo "error: the gate left no recorded payload to join against" >&2
    exit 1
fi

# The independent grammar can still tell a wrong file from a right one. Run
# before the editorless join below, because a joiner that had stopped comparing
# would agree with anything.
"$root/tools/fbx-identity/scripts/check_join.py" "$work/join"

cache="$("$smoke/scripts/fetch_ufbx.sh")"
reader="$work/read_production$suffix"
# Two compilations, not one. Warnings as errors are for the reader written
# here; ufbx is a pinned third-party file whose warning-cleanliness is a
# property of somebody else's compiler.
"$compiler" -std=c11 -O2 -Wall -Wextra -Werror \
    -I "$(native "$cache")" -c "$(native "$smoke/scripts/read_production.c")" \
    -o "$(native "$work/read_production.o")"
"$compiler" -std=c11 -O2 -I "$(native "$cache")" \
    -c "$(native "$cache/ufbx.c")" -o "$(native "$work/ufbx.o")"
"$compiler" "$(native "$work/read_production.o")" "$(native "$work/ufbx.o")" \
    "${link_math[@]+"${link_math[@]}"}" -o "$(native "$reader")"

output="$work/reader.txt"
set +e
"$reader" --identity "$(native "$artefact")" >"$output" 2>"$work/reader.err"
reader_status=$?
set -e
cat "$work/reader.err" >&2
grep -E '^reader ufbx |^FCAD_IDENTITY_SUMMARY |^FCAD_PRODUCTION_FBX_UFBX_EXECUTED ' "$output" || true

if [ "$reader_status" -ne 0 ]; then
    echo "error: the independent reader refused the production bytes" >&2
    exit 1
fi
anchor="$(grep -c '^FCAD_PRODUCTION_FBX_UFBX_EXECUTED ' "$output" || true)"
if [ "$anchor" != "1" ]; then
    echo "error: the independent reader did not run to the end" >&2
    exit 1
fi

set +e
"$join" --reader "$output" --payload "$payload" \
    --expect-nodes 140 --expect-definitions 46
join_status=$?
set -e

if [ "$rust_status" -ne 0 ]; then
    echo "error: the in-process identity gate failed" >&2
    exit 1
fi
if [ "$join_status" -ne 0 ]; then
    echo "error: the independent join refused the production bytes" >&2
    exit 1
fi
echo "identity FBX: pinned ufbx and an independent grammar joined all 140 placements"
