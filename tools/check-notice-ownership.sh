#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# One owner per fact about the Rust notices.
#
# A second copy of the generator version, of the product roots or of the
# election policy path does not break anything visibly: both copies keep
# working, and the one that is not the pinned one still produces a file that
# reads like a notice. That is the failure this refuses, so it is checked here
# rather than remembered.
#
# It costs no toolchain and no network, so it runs in the ordinary lint job
# next to tools/check-planegcs-pins.sh, which does the same job for planegcs.
#
# Run from the repository root:
#   tools/check-notice-ownership.sh

set -euo pipefail

NOTICE_TOOL='check-notice-ownership'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/notices/lib.sh
. tools/notices/lib.sh

notice_load_pin

failures=0
checks=0

check() { checks=$((checks + 1)); }

fail() {
    failures=$((failures + 1))
    echo "$NOTICE_TOOL: $*" >&2
}

# Executable places only. The generated notices name the tool version on
# purpose, and are not a second owner of it because nothing reads them back.
# This file is excluded from its own searches: it has to spell out every string
# it is looking for, and a checker that fails on its own patterns checks
# nothing else.
executables() {
    git ls-files -z .github tools \
        | tr '\0' '\n' \
        | grep -vE '^tools/notices/texts/' \
        | grep -vxF 'tools/check-notice-ownership.sh'
}

# Files whose *code* matches, ignoring comments. Prose that explains where a
# thing lives is not a second owner of it; a line that reads it is.
naming() { # pattern [extra-file-to-exclude]
    local pattern="$1" allowed="${2:-}" f
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        [ "$f" = "$allowed" ] && continue
        if sed -e 's/^[[:space:]]*#.*$//' "$f" | grep -qE -- "$pattern"; then
            printf '%s\n' "$f"
        fi
    done < <(executables)
}

# --- the tool version -------------------------------------------------------

check
hits="$(naming "$CARGO_ABOUT_VERSION" tools/notices/pin.env)"
if [ -n "$hits" ]; then
    fail "the pinned cargo-about version $CARGO_ABOUT_VERSION is copied outside tools/notices/pin.env:"
    # One path per line is exactly what should be split here.
    # shellcheck disable=SC2086
    printf '  %s\n' $hits >&2
    echo "  read it from the pin instead: . tools/notices/pin.env" >&2
fi

check
hits="$(naming 'CARGO_ABOUT_VERSION=' tools/notices/pin.env)"
if [ -n "$hits" ]; then
    fail "CARGO_ABOUT_VERSION is assigned outside tools/notices/pin.env: $hits"
fi

# --- who may run the generator ---------------------------------------------

check
hits="$(naming 'cargo +about +generate' tools/generate-rust-notices.sh)"
if [ -n "$hits" ]; then
    fail "cargo-about is invoked outside tools/generate-rust-notices.sh: $hits"
    echo "  a second invocation is a second set of flags, and a second answer" >&2
fi

# --- the election policy and the output paths ------------------------------

check
hits="$(naming 'tools/notices/about\.toml' tools/notices/lib.sh)"
if [ -n "$hits" ]; then
    fail "the election policy path is named outside tools/notices/lib.sh: $hits"
fi

check
hits="$(naming 'licences/rust/NOTICE-' tools/notices/lib.sh)"
if [ -n "$hits" ]; then
    fail "a notice output path is named outside tools/notices/lib.sh: $hits"
fi

# --- the roots must be the ones the release actually builds ----------------

readonly RELEASE_WORKFLOW='.github/workflows/runtime-layout.yml'
for root in "${NOTICE_ROOTS[@]}"; do
    binary="${root%%|*}"
    features="${root##*|}"
    check
    grep -qE -- "--bin +$binary( |\$)" "$RELEASE_WORKFLOW" || fail \
        "tools/notices/lib.sh calls '$binary' a shipped binary but $RELEASE_WORKFLOW does not build it"
    if [ -n "$features" ]; then
        check
        grep -qE -- "--features +$features( |\$)" "$RELEASE_WORKFLOW" || fail \
            "the notice for '$binary' is generated with features '$features' but $RELEASE_WORKFLOW does not build it that way"
    fi
done

# --- the targets must be the ones the dependency policy already describes --

for target in "${NOTICE_TARGETS[@]}"; do
    check
    grep -qF "\"$target\"" deny.toml || fail \
        "notices are generated for $target but deny.toml does not describe it"
done

if [ "$failures" -gt 0 ]; then
    echo >&2
    echo "$NOTICE_TOOL: $failures of $checks checks failed" >&2
    exit 1
fi

echo "$NOTICE_TOOL: $checks checks passed"
