#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The ordinary, offline gate over the committed Rust notice inventory.
#
# `cargo-deny` answers whether a licence expression may be admitted. It writes
# no notice and cannot be read as one, so a package assembled from a green
# `cargo deny` run would ship no third-party notices at all. This gate owns the
# other half: that licences/rust/NOTICE-<target>.md exists, was produced by the
# pinned tool from the committed policy, and still describes the exact
# dependency graph of the two shipped binaries.
#
# It regenerates every target twice into a clean directory and refuses a
# committed file that differs from either run, so the notices cannot be edited
# by hand and cannot go stale behind a lock file, a manifest, a feature, the
# election policy or the tool pin.
#
# Regenerating and diffing is not on its own enough: it would only prove the
# generator agrees with itself. So the committed file is also read directly,
# and the claims a notice has to make are checked against Cargo.lock and the
# committed mappings rather than against the generator.
#
# It never reaches the network. tools/generate-rust-notices.sh runs cargo
# `--frozen`, and the texts upstream does not ship inside the published crate
# come from tools/notices/texts/.
#
# Run from the repository root:
#   tools/check-rust-notices.sh
#   tools/check-rust-notices.sh --release-ready
#
# The default validates that the inventory is accurate, including an explicit
# blocker when a publisher supplied no licence text. `--release-ready` applies
# the stricter package boundary and refuses any such blocker.

set -euo pipefail

NOTICE_TOOL='check-rust-notices'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/notices/lib.sh
. tools/notices/lib.sh

release_ready=0
case "${1:-}" in
    '') ;;
    --release-ready) release_ready=1 ;;
    *) echo "$NOTICE_TOOL: unknown argument '$1'" >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { echo "$NOTICE_TOOL: too many arguments" >&2; exit 2; }

checks=0
failures=0

pass() { checks=$((checks + 1)); }

fail() {
    checks=$((checks + 1))
    failures=$((failures + 1))
    echo "$NOTICE_TOOL: $*" >&2
}

want() { # description, condition already evaluated by the caller
    if [ "$1" = 0 ]; then pass; else fail "$2"; fi
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

notice_load_pin
notice_require_about
pass

# ---------------------------------------------------------------------------
# The inputs must exist before anything is compared.
# ---------------------------------------------------------------------------

for f in "$NOTICE_ABOUT_CONFIG" "$NOTICE_UPSTREAM_TSV" "$NOTICE_DECLARED_TSV" \
         tools/notices/pin.env tools/generate-rust-notices.sh tools/refresh-notice-texts.sh; do
    if [ -f "$f" ]; then pass; else fail "missing $f"; fi
done

# ---------------------------------------------------------------------------
# Regenerate twice from a clean directory. Same lock file, same target, same
# feature set, so the two runs and the committed file must be byte identical.
# ---------------------------------------------------------------------------

for target in "${NOTICE_TARGETS[@]}"; do
    committed="$NOTICE_DIR/NOTICE-$target.md"
    if [ ! -f "$committed" ]; then
        fail "no committed notice for the $target product graph"
        continue
    fi
    if [ ! -s "$committed" ]; then
        fail "$committed is empty"
        continue
    fi

    for run in 1 2; do
        if ! tools/generate-rust-notices.sh --target "$target" \
             --output "$work/$target.$run" >"$work/gen.log" 2>&1; then
            cat "$work/gen.log" >&2
            fail "the $target notice could not be regenerated"
            continue 2
        fi
    done

    if diff -q "$work/$target.1" "$work/$target.2" >/dev/null; then
        pass
    else
        fail "two runs on the same inputs produced different $target notices, so the output is not reproducible"
    fi

    if diff -q "$committed" "$work/$target.1" >/dev/null; then
        pass
    else
        fail "$committed is stale or was edited by hand; regenerate with tools/generate-rust-notices.sh"
        # `diff` exits non-zero when it finds a difference, which is the
        # only reason it is being run. Under `set -o pipefail` that would
        # end the script here and silently skip every check below.
        diff -u "$committed" "$work/$target.1" | head -40 >&2 || true
    fi
done

# ---------------------------------------------------------------------------
# Read the committed files directly. Everything below is checked against
# Cargo.lock and the committed mappings, not against the generator, so a
# generator that agreed with itself about the wrong thing still fails here.
# ---------------------------------------------------------------------------

# See the note in tools/generate-rust-notices.sh: Windows checks out Cargo.lock
# with CRLF and the git for Windows jq writes CRLF.
tr -d '\r' < Cargo.lock | awk '
    /^\[\[package\]\]$/ { name=""; version=""; next }
    /^name = /     { name=$3;     gsub(/"/, "", name) }
    /^version = /  { version=$3;  gsub(/"/, "", version) }
    /^checksum = / { checksum=$3; gsub(/"/, "", checksum)
                     printf "%s\t%s\t%s\n", name, version, checksum }
' | sort > "$work/lock.tsv"

cargo metadata --locked --format-version 1 --no-deps 2>/dev/null \
    | jq -r '.packages[].name' | tr -d '\r' | sort -u > "$work/workspace-names.txt"

declared_in_product=0
for target in "${NOTICE_TARGETS[@]}"; do
    committed="$NOTICE_DIR/NOTICE-$target.md"
    [ -s "$committed" ] || continue

    # Package rows, as the committed file states them.
    grep '^| `' "$committed" \
        | sed -e 's/^| //' -e 's/ |$//' -e 's/`//g' -e 's/ | /\t/g' \
        > "$work/rows-$target.tsv"

    rows="$(wc -l < "$work/rows-$target.tsv" | tr -d ' ')"
    claimed="$(sed -n 's/^- third-party packages: //p' "$committed" | head -1)"
    if [ "$rows" = "$claimed" ] && [ "$rows" -gt 0 ]; then pass; else
        fail "$committed says $claimed packages and lists $rows"
    fi

    declared_rows="$(awk -F'\t' '$6 == "declared only, see below" { n++ } END { print n + 0 }' \
        "$work/rows-$target.tsv")"
    if [ "$declared_rows" -gt 0 ]; then
        declared_in_product=$((declared_in_product + declared_rows))
        if grep -qF '**RELEASE BLOCKED for this target.**' "$committed"; then pass; else
            fail "$committed has $declared_rows unresolved package(s) but does not block release"
        fi
    elif grep -qF '**RELEASE BLOCKED for this target.**' "$committed"; then
        fail "$committed claims release is blocked although it has no unresolved package"
    else
        pass
    fi

    # A notice for one binary only would still look like a notice, so both are
    # required to be named as roots.
    missing_root=0
    for root in "${NOTICE_ROOTS[@]}"; do
        grep -qF "\`${root%%|*}\`, built from" "$committed" || missing_root=1
    done
    want "$missing_root" "$committed does not name both shipped binaries as roots"

    if grep -qF 'registry+https://github.com/rust-lang/crates.io-index' "$committed"; then
        pass
    else
        fail "$committed does not say where the packages it lists come from"
    fi

    if grep -qF "target: \`$target\`" "$committed"; then pass; else
        fail "$committed does not name the target it was generated for"
    fi

    if grep -qF "cargo-about $CARGO_ABOUT_VERSION" "$committed"; then pass; else
        fail "$committed does not name the pinned generator version"
    fi

    # Placeholders, absolute paths, timestamps and traversal-order artefacts.
    if grep -qE '<year>|<copyright holders>|\[year\] \[fullname\]' "$committed"; then
        fail "$committed contains an unfilled licence template"
    else pass; fi

    if grep -nE '/Users/|/home/[a-z]|/private/tmp|/runner/work|[A-Z]:\\\\Users|\.cargo/registry' "$committed" >/dev/null; then
        fail "$committed contains an absolute path from the machine that generated it"
        grep -nE '/Users/|/home/[a-z]|/private/tmp|/runner/work|[A-Z]:\\\\Users|\.cargo/registry' "$committed" | head -5 >&2
    else pass; fi

    if grep -qE '20[0-9][0-9]-[0-9][0-9]-[0-9][0-9]T|[0-9][0-9]:[0-9][0-9]:[0-9][0-9]|[Gg]enerated (on|at)' "$committed"; then
        fail "$committed contains a timestamp"
    else pass; fi

    # No FerriteCAD crate may be listed as a third party.
    if cut -f1 "$work/rows-$target.tsv" | sort -u \
        | comm -12 - "$work/workspace-names.txt" | grep . >&2; then
        fail "$committed lists a FerriteCAD workspace crate as third party"
    else pass; fi

    # One row per package identity.
    if cut -f1,2 "$work/rows-$target.tsv" | sort | uniq -d | grep . >&2; then
        fail "$committed lists the same package identity more than once"
    else pass; fi

    # Sorted, so the output cannot depend on the order a directory was walked.
    if diff -q "$work/rows-$target.tsv" <(sort "$work/rows-$target.tsv") >/dev/null; then
        pass
    else
        fail "$committed lists packages in an unsorted order"
    fi

    # Every row must carry a version and the registry checksum Cargo.lock pins.
    bad=0
    while IFS=$'\t' read -r name version checksum _; do
        [ -n "$version" ] || { bad=$((bad + 1)); continue; }
        grep -qxF "$name	$version	$checksum" "$work/lock.tsv" || bad=$((bad + 1))
    done < "$work/rows-$target.tsv"
    want "$([ "$bad" = 0 ] && echo 0 || echo 1)" \
        "$committed has $bad package row(s) whose version or registry checksum is not the one Cargo.lock pins"

    # Every listed package must be pointed at a text, and every text section
    # must carry something.
    if grep -q '^## Licence texts$' "$committed"; then pass; else
        fail "$committed has no licence text section"
    fi
    empty_text="$(awk '/^```text$/ { started = NR; next }
                       /^```$/ { if (started && NR == started + 1) n++; started = 0 }
                       END { print n + 0 }' "$committed")"
    want "$([ "$empty_text" = 0 ] && echo 0 || echo 1)" \
        "$committed has $empty_text empty licence text block(s)"

    fenced="$(grep -c '^```text$' "$committed" || true)"
    want "$([ "$fenced" -gt 0 ] && echo 0 || echo 1)" \
        "$committed quotes no licence text at all"
done

# ---------------------------------------------------------------------------
# The committed mappings, checked against the lock file and the payload.
# ---------------------------------------------------------------------------

used_texts="$work/used-texts.txt"
: > "$used_texts"

bad=0
while IFS=$'\t' read -r _source name version checksum licence _repo commit _path digest; do
    case "$_source" in '#'*|'') continue ;; esac
    printf '%s\n' "$digest" >> "$used_texts"
    grep -qxF "$name	$version	$checksum" "$work/lock.tsv" \
        || { echo "$NOTICE_TOOL: upstream-texts.tsv pins a registry checksum for $name $version that Cargo.lock does not" >&2; bad=$((bad + 1)); }
    [ -n "$commit" ] && [ ${#commit} = 40 ] \
        || { echo "$NOTICE_TOOL: upstream-texts.tsv has no upstream commit for $name $version" >&2; bad=$((bad + 1)); }
    [ -n "$licence" ] \
        || { echo "$NOTICE_TOOL: upstream-texts.tsv has no licence for $name $version" >&2; bad=$((bad + 1)); }
    if [ -f "$NOTICE_TEXT_DIR/$digest.txt" ]; then
        got="$(notice_sha256 "$NOTICE_TEXT_DIR/$digest.txt")"
        [ "$got" = "$digest" ] \
            || { echo "$NOTICE_TOOL: committed text for $name $version hashes to $got, not $digest" >&2; bad=$((bad + 1)); }
    else
        echo "$NOTICE_TOOL: committed text $digest.txt for $name $version is missing" >&2
        bad=$((bad + 1))
    fi
done < "$NOTICE_UPSTREAM_TSV"
want "$([ "$bad" = 0 ] && echo 0 || echo 1)" "$NOTICE_UPSTREAM_TSV has $bad broken binding(s)"

bad=0
declared_count=0
while IFS=$'\t' read -r _source name version checksum _lic decl terms srepo scommit _spath sdigest; do
    case "$_source" in '#'*|'') continue ;; esac
    declared_count=$((declared_count + 1))
    printf '%s\n' "$terms" >> "$used_texts"
    grep -qxF "$name	$version	$checksum" "$work/lock.tsv" \
        || { echo "$NOTICE_TOOL: declared-only.tsv pins a registry checksum for $name $version that Cargo.lock does not" >&2; bad=$((bad + 1)); }
    case "$decl" in
        'license = "'*) ;;
        *) echo "$NOTICE_TOOL: declared-only.tsv records no manifest declaration for $name $version" >&2; bad=$((bad + 1)) ;;
    esac
    if [ -f "$NOTICE_TEXT_DIR/$terms.txt" ]; then
        grep -qE '<year>|<copyright holders>' "$NOTICE_TEXT_DIR/$terms.txt" \
            && { echo "$NOTICE_TOOL: the declared-only terms for $name $version are still a template" >&2; bad=$((bad + 1)); }
    else
        echo "$NOTICE_TOOL: declared-only terms $terms.txt are missing" >&2
        bad=$((bad + 1))
    fi
    if [ "$srepo" != '-' ]; then
        printf '%s\n' "$sdigest" >> "$used_texts"
        [ ${#scommit} = 40 ] \
            || { echo "$NOTICE_TOOL: declared-only.tsv has no upstream commit for the $name $version statement" >&2; bad=$((bad + 1)); }
        if [ -f "$NOTICE_TEXT_DIR/$sdigest.txt" ]; then
            got="$(notice_sha256 "$NOTICE_TEXT_DIR/$sdigest.txt")"
            [ "$got" = "$sdigest" ] \
                || { echo "$NOTICE_TOOL: the committed statement for $name $version hashes to $got, not $sdigest" >&2; bad=$((bad + 1)); }
        else
            echo "$NOTICE_TOOL: committed statement $sdigest.txt for $name $version is missing" >&2
            bad=$((bad + 1))
        fi
    fi
done < "$NOTICE_DECLARED_TSV"
want "$([ "$bad" = 0 ] && echo 0 || echo 1)" "$NOTICE_DECLARED_TSV has $bad broken binding(s)"

# The blocker inventory is closed: it is meant to shrink, and it may not grow
# without someone changing this number on purpose.
readonly DECLARED_ONLY_BUDGET=11
if [ "$declared_count" -le "$DECLARED_ONLY_BUDGET" ]; then pass; else
    fail "$declared_count packages need declared-only recording but the closed blocker budget is $DECLARED_ONLY_BUDGET; a new one needs a decision, not a row"
fi

if [ "$release_ready" = 1 ]; then
    if [ "$declared_in_product" -eq 0 ]; then
        pass
    else
        fail "$declared_in_product unresolved package occurrence(s) remain in the product notices; release is blocked"
    fi
fi

# Every committed payload must be referenced, and every reference must exist.
sort -u -o "$used_texts" "$used_texts"
orphans=0
for f in "$NOTICE_TEXT_DIR"/*.txt; do
    [ -e "$f" ] || continue
    base="$(basename "$f" .txt)"
    grep -qxF "$base" "$used_texts" || { echo "$NOTICE_TOOL: $f is referenced by nothing" >&2; orphans=$((orphans + 1)); }
done
want "$([ "$orphans" = 0 ] && echo 0 || echo 1)" "$orphans committed licence text(s) are unreferenced"

# The declared-only packages named in each notice must be exactly the blocker
# inventory entries that apply to that target, and no others.
awk -F'\t' 'substr($1,1,1) != "#" && NF > 1 { printf "%s %s\n", $2, $3 }' \
    "$NOTICE_DECLARED_TSV" | sort -u > "$work/allowlist.txt"
for target in "${NOTICE_TARGETS[@]}"; do
    committed="$NOTICE_DIR/NOTICE-$target.md"
    [ -s "$committed" ] || continue
    # The backticks are Markdown from the notice, not command substitution.
    # shellcheck disable=SC2016
    sed -n 's/^- `\([^`]*\)` \([^,]*\), `[^`]*`, declared in the published manifest.*/\1 \2/p' \
        "$committed" | sort -u > "$work/declared-$target.txt"
    if comm -13 "$work/allowlist.txt" "$work/declared-$target.txt" | grep . >&2; then
        fail "$committed treats a package as declared-only that the allowlist does not name"
    else pass; fi
done

if [ "$failures" -gt 0 ]; then
    echo >&2
    echo "$NOTICE_TOOL: $failures of $checks checks failed" >&2
    exit 1
fi

if [ "$checks" -lt 30 ]; then
    echo "$NOTICE_TOOL: only $checks checks ran, which is fewer than this gate is made of" >&2
    exit 1
fi

if [ "$release_ready" = 1 ]; then
    echo "$NOTICE_TOOL: $checks release-readiness checks passed over ${#NOTICE_TARGETS[@]} targets"
else
    echo "$NOTICE_TOOL: $checks inventory checks passed over ${#NOTICE_TARGETS[@]} targets"
fi
