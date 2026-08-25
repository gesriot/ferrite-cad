#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The networked compliance gate over the committed licence payload.
#
# The ordinary gate is offline on purpose: it reads tools/notices/texts/ and
# never contacts a git host, so an everyday build does not depend on GitHub
# being up or on seven upstream repositories still existing. That leaves one
# thing unproven, which is exactly the thing worth proving occasionally: that
# the committed payload is still what upstream publishes.
#
# So this fetches every bound file again, into a clean directory, from the
# repository and the commit the manifests name, and requires it to match the
# committed bytes. It also re-runs the search for the packages on the
# declared-only allowlist and refuses if upstream has since published a licence
# text for one of them, because that row exists only while there is nothing to
# bind and the list is meant to shrink.
#
# Needs network. Run from the repository root:
#   tools/check-notice-upstream.sh

set -euo pipefail

NOTICE_TOOL='check-notice-upstream'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/notices/lib.sh
. tools/notices/lib.sh

command -v curl >/dev/null 2>&1 || notice_die 'curl is required'

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

checks=0
failures=0

fail() { failures=$((failures + 1)); echo "$NOTICE_TOOL: $*" >&2; }

crate_src_dir() {
    local dir
    for dir in "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/"$1-$2"; do
        if [ -d "$dir" ]; then printf '%s\n' "$dir"; return 0; fi
    done
    return 1
}

# --- every bound upstream text, fetched again -------------------------------

while IFS=$'\t' read -r _source name version _ck licence repo commit path digest; do
    case "$_source" in '#'*|'') continue ;; esac
    checks=$((checks + 1))
    slug="$(notice_github_slug "$repo")" || { fail "$name $version: $repo is not a github project"; continue; }
    if ! notice_fetch_upstream "$slug" "$commit" "$path" "$work/fetched"; then
        fail "$name $version: $slug@$commit:$path could not be fetched"
        continue
    fi
    got="$(notice_sha256 "$work/fetched")"
    if [ "$got" != "$digest" ]; then
        fail "$name $version: $slug@$commit:$path now hashes to $got, not the bound $digest"
        continue
    fi
    if ! cmp -s "$work/fetched" "$NOTICE_TEXT_DIR/$digest.txt"; then
        fail "$name $version: the committed payload differs from what $slug@$commit:$path serves"
        continue
    fi
    phrase="$(operative_phrase "$licence")" || { fail "$name $version: no operative phrase for $licence"; continue; }
    grep -qF "$phrase" "$work/fetched" \
        || fail "$name $version: $path no longer contains the operative text of $licence"
done < "$NOTICE_UPSTREAM_TSV"

# --- the declared-only rows, and whether they are still justified -----------

while IFS=$'\t' read -r _source name version _ck licence _decl _terms srepo scommit spath sdigest; do
    case "$_source" in '#'*|'') continue ;; esac

    if [ "$srepo" != '-' ]; then
        checks=$((checks + 1))
        slug="$(notice_github_slug "$srepo")" || { fail "$name $version: $srepo is not a github project"; continue; }
        if ! notice_fetch_upstream "$slug" "$scommit" "$spath" "$work/fetched"; then
            fail "$name $version: the licensing statement $slug@$scommit:$spath could not be fetched"
        else
            got="$(notice_sha256 "$work/fetched")"
            if [ "$got" != "$sdigest" ]; then
                fail "$name $version: the statement now hashes to $got, not the bound $sdigest"
            elif ! cmp -s "$work/fetched" "$NOTICE_TEXT_DIR/$sdigest.txt"; then
                fail "$name $version: the committed statement differs from what upstream serves"
            fi
        fi
    fi

    # The row is only allowed to exist while upstream publishes no text.
    checks=$((checks + 1))
    src="$(crate_src_dir "$name" "$version")" || { fail "$name $version is not unpacked"; continue; }
    repo="$(sed -n 's/^repository = "\(.*\)"$/\1/p' "$src/Cargo.toml" | head -1)"
    if [ -z "$repo" ]; then continue; fi
    slug="$(notice_github_slug "$repo")" || continue
    commit="$(jq -r '.git.sha1 // ""' "$src/.cargo_vcs_info.json" 2>/dev/null || true)"
    [ -n "$commit" ] || continue
    subdir="$(jq -r '.path_in_vcs // ""' "$src/.cargo_vcs_info.json" 2>/dev/null || true)"
    if found="$(notice_find_upstream_text "$slug" "$commit" "$subdir" "$licence" "$work/fetched")"; then
        fail "$name $version is on the declared-only allowlist, but $slug@$commit now serves the $licence text at $found; bind it with tools/refresh-notice-texts.sh and drop the row"
    fi
done < "$NOTICE_DECLARED_TSV"

if [ "$failures" -gt 0 ]; then
    echo >&2
    echo "$NOTICE_TOOL: $failures of $checks checks failed" >&2
    exit 1
fi

[ "$checks" -gt 0 ] || notice_die 'no binding was checked, so this gate proved nothing'

echo "$NOTICE_TOOL: $checks upstream binding(s) still match the committed payload"
