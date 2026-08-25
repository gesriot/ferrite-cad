#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The one script here that is allowed online.
#
# Some publishers ship no licence file inside the .crate. Asked for a text it
# has no file for, cargo-about substitutes a canonical SPDX template still
# reading `Copyright (c) <year> <copyright holders>` and mentions it only at
# debug level, so the placeholder would travel into a release looking like a
# notice. This fetches the real text from the upstream repository instead, at
# the exact commit the published crate names in its own .cargo_vcs_info.json,
# and commits it under tools/notices/texts/ with a manifest binding it to the
# package identity.
#
# What is committed is the normalised licence text and the manifest, never a
# git cache and never a checkout. After this has run, generation and the
# ordinary gate are offline again: they read the payload, not GitHub.
#
# It guesses nothing. A crate whose upstream file does not actually contain the
# operative text of the licence being elected is reported and left out, so a
# prose page about licensing cannot become "the MIT text".
#
# Run from the repository root, with network:
#   tools/refresh-notice-texts.sh

set -euo pipefail

NOTICE_TOOL='refresh-notice-texts'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/notices/lib.sh
. tools/notices/lib.sh

notice_load_pin
notice_require_about
command -v curl >/dev/null 2>&1 || notice_die 'curl is required'

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "$NOTICE_TOOL: asking cargo-about which packages publish no licence text"
: > "$work/unresolved.tsv"
tools/generate-rust-notices.sh --discover-unresolved "$work/unresolved.tsv"
sort -u -o "$work/unresolved.tsv" "$work/unresolved.tsv"

[ -s "$work/unresolved.tsv" ] || {
    echo "$NOTICE_TOOL: every package establishes its own licence text; nothing to refresh"
    exit 0
}

# Registry source directory of an unpacked crate.
crate_src_dir() {
    local dir
    for dir in "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/"$1-$2"; do
        if [ -d "$dir" ]; then printf '%s\n' "$dir"; return 0; fi
    done
    return 1
}

mkdir -p "$NOTICE_TEXT_DIR"
: > "$work/rows.tsv"
: > "$work/nothing.tsv"
kept=0

while IFS=$'\t' read -r name version source id checksum; do
    [ -n "$name" ] || continue
    src="$(crate_src_dir "$name" "$version")" || notice_die "$name $version is not unpacked"

    repo="$(sed -n 's/^repository = "\(.*\)"$/\1/p' "$src/Cargo.toml" | head -1)"
    [ -n "$repo" ] || { printf '%s\t%s\t%s\t%s\tno repository in the published manifest\n' \
        "$name" "$version" "$id" "$checksum" >> "$work/nothing.tsv"; continue; }

    slug="$(notice_github_slug "$repo")" || {
        printf '%s\t%s\t%s\t%s\trepository %s is not a github project\n' \
            "$name" "$version" "$id" "$checksum" "$repo" >> "$work/nothing.tsv"; continue; }

    commit="$(jq -r '.git.sha1 // ""' "$src/.cargo_vcs_info.json" 2>/dev/null || true)"
    [ -n "$commit" ] || { printf '%s\t%s\t%s\t%s\tthe published crate records no vcs commit\n' \
        "$name" "$version" "$id" "$checksum" >> "$work/nothing.tsv"; continue; }
    subdir="$(jq -r '.path_in_vcs // ""' "$src/.cargo_vcs_info.json" 2>/dev/null || true)"

    found="$(notice_find_upstream_text "$slug" "$commit" "$subdir" "$id" "$work/candidate")" || found=''

    if [ -z "$found" ]; then
        printf '%s\t%s\t%s\t%s\tno file containing the %s text at %s in %s\n' \
            "$name" "$version" "$id" "$checksum" "$id" "$commit" "$slug" >> "$work/nothing.tsv"
        continue
    fi

    digest="$(notice_sha256 "$work/candidate")"
    cp "$work/candidate" "$NOTICE_TEXT_DIR/$digest.txt"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$source" "$name" "$version" "$checksum" "$id" \
        "https://github.com/$slug" "$commit" "$found" "$digest" >> "$work/rows.tsv"
    kept=$((kept + 1))
    echo "  $name $version  $id  <- $slug@${commit:0:12}:$found"
done < "$work/unresolved.tsv"

# Keep the committed comment block, replace the rows.
sed -n '/^#/p' "$NOTICE_UPSTREAM_TSV" > "$work/header"
cat "$work/header" > "$NOTICE_UPSTREAM_TSV"
sort "$work/rows.tsv" >> "$NOTICE_UPSTREAM_TSV"

# Payload files nothing points at any more are removed, so the committed set is
# exactly what the manifests use.
cut -f9 "$work/rows.tsv" | sort -u > "$work/live"
awk -F'\t' 'substr($1,1,1) != "#" && NF > 1 { print $7; if ($11 != "-") print $11 }' \
    "$NOTICE_DECLARED_TSV" | sort -u >> "$work/live"
sort -u -o "$work/live" "$work/live"
for f in "$NOTICE_TEXT_DIR"/*.txt; do
    [ -e "$f" ] || continue
    base="$(basename "$f" .txt)"
    grep -qxF "$base" "$work/live" || { echo "  removing unused $f"; rm -f "$f"; }
done

echo "$NOTICE_TOOL: bound $kept package(s) to an upstream text"

if [ -s "$work/nothing.tsv" ]; then
    {
        echo
        echo "$NOTICE_TOOL: these packages publish no licence text anywhere that could be bound."
        echo "Nothing was invented for them. Each needs an explicit, checksum-bound entry in"
        echo "$NOTICE_DECLARED_TSV, and that list is meant to shrink, not grow:"
        echo
        awk -F'\t' '{ printf "  %s %s  licence=%s  registry checksum=%s\n      %s\n", $1, $2, $3, $4, $5 }' \
            "$work/nothing.tsv"
    } >&2
fi

echo "$NOTICE_TOOL: regenerating the notices from the refreshed payload"
tools/generate-rust-notices.sh
