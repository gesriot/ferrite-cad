# SPDX-License-Identifier: MIT
# shellcheck shell=bash
# Everything here is read by the scripts that source this file.
# shellcheck disable=SC2034
#
# Shared definitions for the three Rust-notice scripts. Sourced, never run.
#
# What lives here is the small set of facts that all three must agree on. A
# second copy of any of them would be a second answer: a notice generated from
# a different root, feature or target than the one the release builds is a
# description of a product nobody ships, and it would go on looking correct.

# The two shipped binaries, the manifests they are built from, and the features
# the release turns on. This is the same selection .github/workflows/runtime-
# layout.yml builds, and tools/check-notice-ownership.sh refuses a workflow or
# script that names a product root of its own.
#
#   <binary>|<manifest>|<comma-separated features>
NOTICE_ROOTS=(
    'ferritecad-viewer|crates/ferritecad-app/Cargo.toml|planegcs'
    'ferritecad|crates/ferritecad-cli/Cargo.toml|'
)

# The product targets. Each gets its own notice, generated with that one triple
# and nothing else: cargo-about evaluates several --target values together, so
# passing all three at once admits crates that no single target can build.
NOTICE_TARGETS=(
    x86_64-unknown-linux-gnu
    x86_64-pc-windows-msvc
    aarch64-apple-darwin
)

readonly NOTICE_DIR='licences/rust'
readonly NOTICE_ABOUT_CONFIG='tools/notices/about.toml'
readonly NOTICE_UPSTREAM_TSV='tools/notices/upstream-texts.tsv'
readonly NOTICE_DECLARED_TSV='tools/notices/declared-only.tsv'
readonly NOTICE_TEXT_DIR='tools/notices/texts'

# Byte-for-byte reproducibility starts with not asking the C library what
# "sorted" means. Every sort in these scripts is a C-locale sort.
export LC_ALL=C

notice_die() {
    echo "${NOTICE_TOOL:-notices}: $*" >&2
    exit 1
}

# The pinned tool identity has exactly one owner, and every script reads it
# from there rather than carrying its own copy.
notice_load_pin() {
    # shellcheck source=tools/notices/pin.env
    . tools/notices/pin.env
    [ -n "${CARGO_ABOUT_VERSION:-}" ] || notice_die 'pin.env does not set CARGO_ABOUT_VERSION'
    [ -n "${CARGO_ABOUT_THRESHOLD:-}" ] || notice_die 'pin.env does not set CARGO_ABOUT_THRESHOLD'
}

# A tool of the wrong version writes a file that still looks like a notice, so
# the version is checked rather than assumed.
notice_require_about() {
    local found
    command -v cargo-about >/dev/null 2>&1 || notice_die \
        "cargo-about is not installed; run: cargo install cargo-about --version $CARGO_ABOUT_VERSION --locked --features cli"
    found="$(cargo about --version 2>/dev/null | awk '{print $2}')"
    [ "$found" = "$CARGO_ABOUT_VERSION" ] || notice_die \
        "cargo-about $found is installed but $CARGO_ABOUT_VERSION is pinned; run: cargo install cargo-about --version $CARGO_ABOUT_VERSION --locked --features cli"
}

# Candidate file names, most specific first. A crate-relative file beats the
# repository root: a workspace member that carries its own licence is making a
# narrower statement than the repository is.
candidates_for() {
    case "$1" in
        MIT)        printf 'LICENSE-MIT\nLICENSE-MIT.md\nLICENSE-MIT.txt\nlicense-mit\nLICENSE.txt\nLICENSE\nLICENSE.md\nCOPYING\n' ;;
        Apache-2.0) printf 'LICENSE-APACHE\nLICENSE-APACHE.md\nLICENSE-APACHE.txt\nlicense-apache-2.0\nLICENSE.txt\nLICENSE\nLICENSE.md\nCOPYING\n' ;;
        *)          printf 'LICENSE.txt\nLICENSE\nLICENSE.md\nCOPYING\n' ;;
    esac
}

# The operative sentence of the licence being elected. A file that does not
# contain it is not that licence's text, whatever it is called.
operative_phrase() {
    case "$1" in
        MIT)        printf '%s' 'Permission is hereby granted, free of charge' ;;
        Apache-2.0) printf '%s' 'TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION' ;;
        *)          return 1 ;;
    esac
}

# Fetch one file from a github project at one commit. Only used by the two
# scripts that are allowed online.
notice_fetch_upstream() { # slug commit path out
    curl -sSfL "https://raw.githubusercontent.com/$1/$2/$3" -o "$4" 2>/dev/null
}

# The github owner/name of a package's repository field, or empty.
notice_github_slug() {
    local slug="$1"
    slug="${slug#http://github.com/}"
    slug="${slug#https://github.com/}"
    slug="${slug%%/tree/*}"
    slug="${slug%/}"
    slug="${slug%.git}"
    case "$slug" in
        */*) printf '%s\n' "$slug" ;;
        *) return 1 ;;
    esac
}

# The first file at this commit that actually contains the operative text of
# the licence being elected, searched crate-relative first. Empty when the
# publisher has published no such file, which is a fact the notices record
# rather than paper over.
notice_find_upstream_text() { # slug commit subdir licence out
    local slug="$1" commit="$2" subdir="$3" id="$4" out="$5" dir base rel phrase
    phrase="$(operative_phrase "$id")" || return 2
    for dir in "$subdir" ''; do
        for base in $(candidates_for "$id"); do
            if [ -n "$dir" ]; then rel="$dir/$base"; else rel="$base"; fi
            notice_fetch_upstream "$slug" "$commit" "$rel" "$out" || continue
            if grep -qF "$phrase" "$out"; then printf '%s\n' "$rel"; return 0; fi
        done
    done
    return 1
}

notice_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}
