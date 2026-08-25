#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Writes the third-party Rust notice for one product target, or for all of them.
#
# cargo-about resolves one root crate at a time and FerriteCAD ships two
# binaries, so this runs it once per binary and unions the results here rather
# than pointing it at the workspace: the workspace also holds the solver bench
# and the fixtures, and a notice describing those would describe something
# nobody installs. The union is taken over the full package identity (source,
# name, version), and a package reached from both roots must agree, from both,
# on its declared expression and on every licence text.
#
# Nothing here reaches the network: cargo runs `--frozen`, and a text the
# publisher left out of the published crate comes from the committed payload
# under tools/notices/texts/, bound to the upstream commit by
# tools/notices/upstream-texts.tsv. tools/refresh-notice-texts.sh is the only
# script allowed online, and it is what produces that payload.
#
# A package whose licence text cannot be established is a refusal. Asked for a
# licence it recognised no file for, cargo-about substitutes a canonical SPDX
# template that still reads `Copyright (c) <year> <copyright holders>`, and it
# says so only at debug level, so a notice built without this check would carry
# unfilled placeholders and look finished.
#
# Usage:
#   tools/generate-rust-notices.sh                         # all three targets
#   tools/generate-rust-notices.sh --target T --output F   # one target
#   tools/generate-rust-notices.sh --discover-unresolved F # refresh support

set -euo pipefail

NOTICE_TOOL='generate-rust-notices'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/notices/lib.sh
. tools/notices/lib.sh

target=''
output=''
about_config="$NOTICE_ABOUT_CONFIG"
discover=''
cargo_lock_mode='--frozen'

while [ $# -gt 0 ]; do
    case "$1" in
        --target) target="${2:?--target needs a triple}"; shift 2 ;;
        --output) output="${2:?--output needs a path}"; shift 2 ;;
        --about) about_config="${2:?--about needs a path}"; shift 2 ;;
        --discover-unresolved) discover="${2:?--discover-unresolved needs a path}"; shift 2 ;;
        # Only tools/refresh-notice-texts.sh passes this, and only to prove the
        # committed payload still matches what upstream publishes.
        --network) cargo_lock_mode='--locked'; shift ;;
        *) notice_die "unknown argument '$1'" ;;
    esac
done

notice_load_pin
notice_require_about

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# ---------------------------------------------------------------------------
# Facts that do not depend on the target.
# ---------------------------------------------------------------------------

# The registry checksum of every locked package. The lock file is the only
# place that knows it; cargo-about's model does not carry it.
# `tr -d` on the way in, and after every jq below. A Windows runner checks out
# Cargo.lock with CRLF, and the jq build shipped with git for Windows writes
# CRLF too, so a checksum or a base64 field would arrive with a carriage return
# welded to its end. That is how the first Windows run failed: `base64: invalid
# input`, on a string that was valid base64 followed by one \r.
tr -d '\r' < Cargo.lock | awk '
    /^\[\[package\]\]$/ { name=""; version=""; source=""; next }
    /^name = /     { name=$3;     gsub(/"/, "", name) }
    /^version = /  { version=$3;  gsub(/"/, "", version) }
    /^source = /   { source=$3;   gsub(/"/, "", source) }
    /^checksum = / { checksum=$3; gsub(/"/, "", checksum)
                     if (name != "" && source != "")
                         printf "%s\t%s\t%s\t%s\n", name, version, source, checksum }
' | sort > "$work/lock.tsv"

# The workspace's own crates are not third party. Taken from cargo rather than
# from a list here, so a new member cannot be forgotten into the notice.
cargo metadata --locked --format-version 1 --no-deps 2>/dev/null \
    | jq -r '.packages[].name' | tr -d '\r' | sort -u > "$work/workspace-names.txt"

# Registry source directory of an unpacked crate, needed to re-check the facts
# a committed mapping is bound to.
crate_src_dir() {
    local dir
    for dir in "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/"$1-$2"; do
        if [ -d "$dir" ]; then printf '%s\n' "$dir"; return 0; fi
    done
    return 1
}

lock_checksum() {
    awk -F'\t' -v n="$1" -v v="$2" -v s="$3" \
        '$1 == n && $2 == v && $3 == s { print $4; exit }' "$work/lock.tsv"
}

# ---------------------------------------------------------------------------
# Ask cargo-about for each shipped binary, then union.
# ---------------------------------------------------------------------------

collect_graph() {
    local target="$1" root name manifest features
    local -a about

    for root in "${NOTICE_ROOTS[@]}"; do
        name="${root%%|*}"
        manifest="${root#*|}"; manifest="${manifest%%|*}"
        features="${root##*|}"

        about=(cargo about generate
            -c "$about_config"
            --format json
            -m "$manifest"
            --target "$target"
            --threshold "$CARGO_ABOUT_THRESHOLD"
            --fail
            "$cargo_lock_mode"
            -o "$work/about-$name.json")
        if [ -n "$features" ]; then
            about+=(--features "${features//,/ }")
        fi

        if ! "${about[@]}" 2>"$work/about-$name.err"; then
            cat "$work/about-$name.err" >&2
            notice_die "cargo-about failed for $name on $target"
        fi

        # One row per package and per licence text it is used under. FALLBACK
        # marks a text cargo-about invented because it recognised no file in
        # the crate.
        jq -r '
            .licenses[] as $l
            | $l.used_by[]
            | select(.crate.source != null)
            | [ .crate.name, .crate.version, .crate.source, $l.id,
                (if $l.source_path == null then "FALLBACK" else "FILE" end),
                ($l.text | @base64) ]
            | @tsv' "$work/about-$name.json" | tr -d '\r' | sort -u > "$work/rows-$name.tsv"

        jq -r '
            .crates[]
            | select(.package.source != null)
            | [ .package.name, .package.version, .package.source, .license ]
            | @tsv' "$work/about-$name.json" | tr -d '\r' | sort -u > "$work/pkgs-$name.tsv"

        [ -s "$work/rows-$name.tsv" ] || notice_die \
            "cargo-about returned no third-party package for $name on $target"
    done
}

union_graph() {
    local target="$1" a b r
    a="${NOTICE_ROOTS[0]%%|*}"
    b="${NOTICE_ROOTS[1]%%|*}"

    comm -12 <(cut -f1-3 "$work/rows-$a.tsv" | sort -u) \
             <(cut -f1-3 "$work/rows-$b.tsv" | sort -u) > "$work/shared-ids.tsv"

    for r in "$a" "$b"; do
        awk -F'\t' 'NR==FNR { keep[$0]=1; next } (($1 "\t" $2 "\t" $3) in keep)' \
            "$work/shared-ids.tsv" "$work/rows-$r.tsv" | sort > "$work/shared-rows-$r.tsv"
        awk -F'\t' 'NR==FNR { keep[$0]=1; next } (($1 "\t" $2 "\t" $3) in keep)' \
            "$work/shared-ids.tsv" "$work/pkgs-$r.tsv" | sort > "$work/shared-pkgs-$r.tsv"
    done

    if ! diff -q "$work/shared-rows-$a.tsv" "$work/shared-rows-$b.tsv" >/dev/null; then
        diff "$work/shared-rows-$a.tsv" "$work/shared-rows-$b.tsv" | cut -c1-120 >&2 || true
        notice_die "$a and $b disagree about the licence text of a shared package on $target"
    fi
    if ! diff -q "$work/shared-pkgs-$a.tsv" "$work/shared-pkgs-$b.tsv" >/dev/null; then
        diff "$work/shared-pkgs-$a.tsv" "$work/shared-pkgs-$b.tsv" >&2 || true
        notice_die "$a and $b disagree about a shared package's declared expression on $target"
    fi

    # A notice that lost one of the binaries would still look like a notice, so
    # each root is required to contribute something the other does not.
    comm -23 <(cut -f1-3 "$work/pkgs-$a.tsv" | sort -u) \
             <(cut -f1-3 "$work/pkgs-$b.tsv" | sort -u) > "$work/only-$a.tsv"
    comm -13 <(cut -f1-3 "$work/pkgs-$a.tsv" | sort -u) \
             <(cut -f1-3 "$work/pkgs-$b.tsv" | sort -u) > "$work/only-$b.tsv"
    [ -s "$work/only-$a.tsv" ] || notice_die \
        "no package is unique to $a on $target, so this notice cannot be a union of both binaries"
    [ -s "$work/only-$b.tsv" ] || notice_die \
        "no package is unique to $b on $target, so this notice cannot be a union of both binaries"

    cat "$work/rows-$a.tsv" "$work/rows-$b.tsv" | sort -u > "$work/rows.tsv"
    cat "$work/pkgs-$a.tsv" "$work/pkgs-$b.tsv" | sort -u > "$work/pkgs.tsv"

    # ADR 0002: no GPL, AGPL or LGPL term is ever elected out of an expression
    # a permissive alternative already satisfies, and nothing GPL-family may be
    # linked into the product at all. tools/notices/about.toml enforces that by
    # not accepting those identifiers, but a list is easy to edit and the
    # consequence would be invisible: the notice would simply say GPL and stay
    # green. So the elected licences are read back here.
    if awk -F'\t' '$4 ~ /^(AGPL|GPL|LGPL)(-|$)/ { print $1 " " $2 " -> " $4 }' \
        "$work/rows.tsv" | sort -u | grep . >&2; then
        notice_die "a GPL-family licence was elected on $target, which ADR 0002 forbids"
    fi

    if awk -F'\t' 'NR==FNR { ws[$0]=1; next } ($1 in ws) { print $1 }' \
        "$work/workspace-names.txt" "$work/pkgs.tsv" | sort -u | grep . >&2; then
        notice_die "a FerriteCAD workspace crate reached the third-party list on $target"
    fi

    # The notice states the source once, under the package table, because every
    # package has the same one. That sentence has to be true: a git dependency
    # would be described as a crates.io release and its checksum column would
    # be empty.
    if awk -F'\t' '$3 != "registry+https://github.com/rust-lang/crates.io-index" \
                    { printf "%s %s from %s\n", $1, $2, $3 }' "$work/pkgs.tsv" | sort -u | grep . >&2; then
        notice_die "a package on $target does not come from the crates.io registry the notice names"
    fi
}

# ---------------------------------------------------------------------------
# The independent answer. krates resolved the graph above; cargo resolves it
# again here, and the two must agree before anything is written.
# ---------------------------------------------------------------------------

oracle_graph() {
    local target="$1" root name manifest features kind
    local -a tree

    # Two answers from cargo, because one of them cannot be had.
    #
    # krates filters every edge by the target triple, build edges included, so
    # what cargo-about measured is a function of the target alone. cargo does
    # not work that way: build dependencies are compiled for the machine doing
    # the building, so `cargo tree -e normal,build --target X` answers partly
    # about the host. Measured: on a macOS host the aarch64-apple-darwin graph
    # is 153 packages either way, and on a Linux host cargo additionally
    # reports cpufeatures 0.3.0, which blake3 takes only on x86.
    #
    # So cargo is asked for a bound in each direction instead:
    #
    #   normal        every package actually linked into the two binaries,
    #                 filtered by target cfg only, and therefore the same on
    #                 every host. Nothing here may be missing from the notice.
    #   normal,build  everything cargo can reach at all on this host. Nothing
    #                 outside this may appear in the notice.
    for kind in normal normal,build; do
        : > "$work/oracle-raw.txt"
        for root in "${NOTICE_ROOTS[@]}"; do
            name="${root%%|*}"
            manifest="${root#*|}"; manifest="${manifest%%|*}"
            features="${root##*|}"

            # `normal` and `normal,build` are one argument to cargo.
            # shellcheck disable=SC2054
            tree=(cargo tree --locked --target "$target" -e "$kind"
                  --prefix none --format '{p}'
                  --manifest-path "$manifest")
            if [ -n "$features" ]; then
                tree+=(--features "$features")
            fi
            "${tree[@]}" 2>/dev/null | tr -d '\r' >> "$work/oracle-raw.txt" \
                || notice_die "cargo tree failed for $name on $target"
        done

        sed -e 's/ (\*)$//' -e 's/ (proc-macro)$//' \
            -e 's/^\([^ ][^ ]*\) v\([^ ][^ ]*\).*$/\1\t\2/' "$work/oracle-raw.txt" \
            | awk -F'\t' 'NF == 2' \
            | awk -F'\t' 'NR==FNR { ws[$0]=1; next } !($1 in ws)' \
                "$work/workspace-names.txt" - \
            | sort -u > "$work/oracle-${kind//,/-}.tsv"
    done

    cut -f1,2 "$work/pkgs.tsv" | sort -u > "$work/measured.tsv"

    if comm -23 "$work/oracle-normal.tsv" "$work/measured.tsv" | grep . >&2; then
        notice_die "the $target notice is missing packages that cargo links into the two binaries"
    fi
    if comm -13 "$work/oracle-normal-build.tsv" "$work/measured.tsv" | grep . >&2; then
        notice_die "the $target notice names packages that cargo does not reach from the two binaries"
    fi
}

# ---------------------------------------------------------------------------
# Establish a text for every package, or refuse.
# ---------------------------------------------------------------------------

resolve_texts() {
    local target="$1"
    mkdir -p "$work/bytext"

    # Every distinct text cargo-about did establish, keyed by its own digest so
    # that a text found inside a crate and the same text carried in the payload
    # cannot reach the notice twice.
    cut -f6 "$work/rows.tsv" | sort -u > "$work/texts.b64"
    : > "$work/b64-to-digest.tsv"
    local b64 digest
    while IFS= read -r b64; do
        printf '%s' "$b64" | base64 -d > "$work/decoded"
        digest="$(notice_sha256 "$work/decoded")"
        cp "$work/decoded" "$work/bytext/$digest"
        printf '%s\t%s\n' "$b64" "$digest" >> "$work/b64-to-digest.tsv"
    done < "$work/texts.b64"

    # Rows whose text cargo-about really did read out of the crate.
    awk -F'\t' -v OFS='\t' '
        NR==FNR { digest[$1]=$2; next }
        $5 == "FILE" { print $1, $2, $3, $4, "FILE", digest[$6] }
    ' "$work/b64-to-digest.tsv" "$work/rows.tsv" | sort -u > "$work/resolved.tsv"

    awk -F'\t' '$5 == "FALLBACK"' "$work/rows.tsv" | sort -u > "$work/fallback.tsv"

    : > "$work/unresolved.tsv"
    : > "$work/declared.tsv"

    local name version source id key row
    local ck commit rpath tdigest lock_ck vcs payload got decl src
    local srepo scommit spath sdigest
    while IFS=$'\t' read -r name version source id _ _; do
        key="$source|$name|$version|$id"

        # A text may only replace an invented one if it is bound to this exact
        # package identity.
        row="$(awk -F'\t' -v k="$key" \
            'substr($1,1,1) != "#" && ($1 "|" $2 "|" $3 "|" $5) == k' "$NOTICE_UPSTREAM_TSV" | head -1)"
        if [ -n "$row" ]; then
            ck="$(printf '%s' "$row" | cut -f4)"
            commit="$(printf '%s' "$row" | cut -f7)"
            rpath="$(printf '%s' "$row" | cut -f8)"
            tdigest="$(printf '%s' "$row" | cut -f9)"

            lock_ck="$(lock_checksum "$name" "$version" "$source")"
            [ "$lock_ck" = "$ck" ] || notice_die \
                "upstream-texts.tsv binds $name $version to registry checksum $ck but Cargo.lock says ${lock_ck:-<none>}"

            src="$(crate_src_dir "$name" "$version")" || notice_die \
                "$name $version is not unpacked; run cargo fetch before generating notices"
            vcs="$(jq -r '.git.sha1 // ""' "$src/.cargo_vcs_info.json" 2>/dev/null | tr -d '\r' || true)"
            [ "$vcs" = "$commit" ] || notice_die \
                "upstream-texts.tsv binds $name $version to commit $commit but the published crate records ${vcs:-<none>}"

            payload="$NOTICE_TEXT_DIR/$tdigest.txt"
            [ -f "$payload" ] || notice_die \
                "$name $version needs committed text $tdigest ($rpath) but $payload is missing"
            got="$(notice_sha256 "$payload")"
            [ "$got" = "$tdigest" ] || notice_die \
                "committed text $payload hashes to $got, not the $tdigest bound for $name $version"

            cp "$payload" "$work/bytext/$tdigest"
            printf '%s\t%s\t%s\t%s\tPAYLOAD\t%s\n' \
                "$name" "$version" "$source" "$id" "$tdigest" >> "$work/resolved.tsv"
            continue
        fi

        row="$(awk -F'\t' -v k="$key" \
            'substr($1,1,1) != "#" && ($1 "|" $2 "|" $3 "|" $5) == k' "$NOTICE_DECLARED_TSV" | head -1)"
        if [ -n "$row" ]; then
            ck="$(printf '%s' "$row" | cut -f4)"
            decl="$(printf '%s' "$row" | cut -f6)"
            tdigest="$(printf '%s' "$row" | cut -f7)"
            srepo="$(printf '%s' "$row" | cut -f8)"
            scommit="$(printf '%s' "$row" | cut -f9)"
            spath="$(printf '%s' "$row" | cut -f10)"
            sdigest="$(printf '%s' "$row" | cut -f11)"

            lock_ck="$(lock_checksum "$name" "$version" "$source")"
            [ "$lock_ck" = "$ck" ] || notice_die \
                "declared-only.tsv binds $name $version to registry checksum $ck but Cargo.lock says ${lock_ck:-<none>}"

            src="$(crate_src_dir "$name" "$version")" || notice_die \
                "$name $version is not unpacked; run cargo fetch before generating notices"
            grep -qxF "$decl" "$src/Cargo.toml" || notice_die \
                "declared-only.tsv expects '$decl' in the published Cargo.toml of $name $version and it is not there"

            payload="$NOTICE_TEXT_DIR/$tdigest.txt"
            [ -f "$payload" ] || notice_die "declared-only terms $payload is missing"
            got="$(notice_sha256 "$payload")"
            [ "$got" = "$tdigest" ] || notice_die \
                "declared-only terms $payload hashes to $got, not $tdigest"
            grep -qE '<year>|<copyright holders>' "$payload" && notice_die \
                "the declared-only terms in $payload still contain a template placeholder"
            cp "$payload" "$work/bytext/$tdigest"

            # The stronger of the two evidence classes: a file the copyright
            # holder publishes at the same commit the crate was published from.
            if [ "$srepo" != '-' ]; then
                vcs="$(jq -r '.git.sha1 // ""' "$src/.cargo_vcs_info.json" 2>/dev/null | tr -d '\r' || true)"
                [ "$vcs" = "$scommit" ] || notice_die \
                    "declared-only.tsv binds the $name $version statement to commit $scommit but the published crate records ${vcs:-<none>}"
                payload="$NOTICE_TEXT_DIR/$sdigest.txt"
                [ -f "$payload" ] || notice_die \
                    "$name $version cites $srepo@$scommit:$spath but $payload is missing"
                got="$(notice_sha256 "$payload")"
                [ "$got" = "$sdigest" ] || notice_die \
                    "committed statement $payload hashes to $got, not the $sdigest bound for $name $version"
                cp "$payload" "$work/bytext/$sdigest"
            fi

            printf '%s\t%s\t%s\t%s\tDECLARED\t%s\n' \
                "$name" "$version" "$source" "$id" "$tdigest" >> "$work/resolved.tsv"
            printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
                "$name" "$version" "$source" "$id" "$decl" "$tdigest" \
                "$srepo" "$scommit" "$spath" "$sdigest" >> "$work/declared.tsv"
            continue
        fi

        printf '%s\t%s\t%s\t%s\t%s\n' "$name" "$version" "$source" "$id" \
            "$(lock_checksum "$name" "$version" "$source")" >> "$work/unresolved.tsv"
    done < "$work/fallback.tsv"

    sort -u -o "$work/resolved.tsv" "$work/resolved.tsv"
    sort -u -o "$work/unresolved.tsv" "$work/unresolved.tsv"
    sort -u -o "$work/declared.tsv" "$work/declared.tsv"

    if [ -n "$discover" ]; then
        cat "$work/unresolved.tsv" >> "$discover"
        return 0
    fi

    if [ -s "$work/unresolved.tsv" ]; then
        {
            echo "$NOTICE_TOOL: cargo-about established no licence text for these packages on $target"
            echo "and substituted an unfilled canonical template. Each needs a checksum-bound entry in"
            echo "$NOTICE_UPSTREAM_TSV (run tools/refresh-notice-texts.sh), or, when the publisher"
            echo "shipped no text anywhere at all, an explicit entry in $NOTICE_DECLARED_TSV."
            echo
            awk -F'\t' '{ printf "  %s %s  licence=%s  source=%s  registry checksum=%s\n",
                          $1, $2, $4, $3, $5 }' "$work/unresolved.tsv"
        } >&2
        notice_die "$(wc -l < "$work/unresolved.tsv" | tr -d ' ') package(s) have no verifiable licence text on $target"
    fi
}

# ---------------------------------------------------------------------------
# Render.
# ---------------------------------------------------------------------------

# The format strings below are Markdown: the backticks in them are literal, and
# single quotes are what keeps them that way.
# shellcheck disable=SC2016
render_notice() {
    local target="$1" output="$2"
    local packages texts root name manifest features

    # One line per package identity: the licences in force, joined with AND
    # because cargo-about lists every licence an expression actually requires.
    awk -F'\t' -v OFS='\t' '
        { key = $1 OFS $2 OFS $3
          if (!((key OFS $4) in seen)) {
              seen[key OFS $4] = 1
              ids[key] = (key in ids ? ids[key] " AND " : "") $4
          }
          if ($5 == "DECLARED") declared[key] = 1 }
        END { for (k in ids) print k, ids[k], (k in declared ? "declared" : "text") }
    ' "$work/resolved.tsv" | sort > "$work/elected.tsv"

    awk -F'\t' -v OFS='\t' '
        FILENAME == ARGV[1] { ck[$1 OFS $2 OFS $3] = $4; next }
        FILENAME == ARGV[2] { expr[$1 OFS $2 OFS $3] = $4; next }
        { key = $1 OFS $2 OFS $3
          printf "| `%s` | %s | `%s` | `%s` | `%s` | %s |\n",
                 $1, $2, ck[key], expr[key], $4,
                 ($5 == "declared" ? "declared only, see below" : "text below") }
    ' "$work/lock.tsv" "$work/pkgs.tsv" "$work/elected.tsv" > "$work/table.md"

    packages="$(wc -l < "$work/elected.tsv" | tr -d ' ')"

    # Distinct licence texts, each carried once and pointed at by every package
    # it covers.
    # The declared-only terms are not one of these: they belong in their own
    # section, where the notice can say what is and is not known about them.
    awk -F'\t' -v OFS='\t' '$5 != "DECLARED" { print $4, $6 }' "$work/resolved.tsv" \
        | sort -u > "$work/textgroups.tsv"
    texts="$(wc -l < "$work/textgroups.tsv" | tr -d ' ')"

    {
        printf '# Third-party Rust notices for FerriteCAD on %s\n\n' "$target"
        cat <<'PREAMBLE'
FerriteCAD ships two executables. This file lists every third-party Rust
package linked into either of them on this target, the licence FerriteCAD
takes that package under, and the text of that licence.

This file is generated and must not be edited by hand.
`tools/check-rust-notices.sh` regenerates it and refuses any difference.

This is an engineering artefact, not legal advice. It records what the
published crates say about themselves and what FerriteCAD elected under the
policy in ADR 0002.

## What produced this file

PREAMBLE
        printf -- '- generator: `cargo-about %s`, licence detection threshold `%s`\n' \
            "$CARGO_ABOUT_VERSION" "$CARGO_ABOUT_THRESHOLD"
        printf -- '- election policy: `%s`\n' "$NOTICE_ABOUT_CONFIG"
        printf -- '- target: `%s`\n' "$target"
        printf -- '- shipped binaries this union covers:\n'
        for root in "${NOTICE_ROOTS[@]}"; do
            name="${root%%|*}"
            manifest="${root#*|}"; manifest="${manifest%%|*}"
            features="${root##*|}"
            if [ -n "$features" ]; then
                printf -- '  - `%s`, built from `%s` with features `%s`\n' "$name" "$manifest" "$features"
            else
                printf -- '  - `%s`, built from `%s` with default features\n' "$name" "$manifest"
            fi
        done
        cat <<'KINDS'
- dependency kinds: normal and build. Dev-dependencies are excluded, so the
  solver bench, the fixtures and the test-only crates are not listed here.
KINDS
        printf -- '- third-party packages: %s\n' "$packages"
        printf -- '- distinct licence texts: %s\n\n' "$texts"

        printf '## Packages\n\n'
        printf '| package | version | registry checksum | declared expression | in force here | licence text |\n'
        printf '|---|---|---|---|---|---|\n'
        cat "$work/table.md"
        printf '\n'
        printf 'Every package above is published at `registry+https://github.com/rust-lang/crates.io-index`\n'
        printf 'and is identified by the SHA-256 in the checksum column, which is the one `Cargo.lock` pins.\n\n'

        printf '## Licence texts\n\n'
        local n=0 id digest
        while IFS=$'\t' read -r id digest; do
            n=$((n + 1))
            printf '### %s. %s\n\n' "$n" "$id"
            printf 'Applies to:\n\n'
            awk -F'\t' -v i="$id" -v d="$digest" \
                '$4 == i && $6 == d { printf "- `%s` %s\n", $1, $2 }' \
                "$work/resolved.tsv" | sort -u
            printf '\n'
            if grep -q '```' "$work/bytext/$digest"; then
                notice_die "licence text $digest contains a code fence and cannot be quoted verbatim"
            fi
            printf '```text\n'
            cat "$work/bytext/$digest"
            # Licence files do not all end with a newline, and a missing one
            # would swallow the closing fence.
            [ -s "$work/bytext/$digest" ] && [ -n "$(tail -c1 "$work/bytext/$digest")" ] && printf '\n'
            printf '```\n\n'
        done < "$work/textgroups.tsv"

        if [ -s "$work/declared.tsv" ]; then
            cat <<'DECLARED'
## Packages whose publisher shipped no licence text

For the packages below no copyright notice was published: not in the crate
archive, and not in the upstream repository at the commit the crate was
published from. What is known is recorded here and nothing else. FerriteCAD has
not recovered a licence text for them, and has not inferred a year, a copyright
holder or a notice from the authors field, the git history or the repository
owner's name.

Two kinds of evidence appear, and they are not the same strength:

- the `license` field of the published `Cargo.toml`, inside the crate archive
  whose SHA-256 `Cargo.lock` pins;
- for some packages, additionally a licensing statement that the copyright
  holder publishes in the repository at that same commit, quoted below and
  bound by SHA-256. It states which licence applies. It is not a licence text.

The terms reproduced below are the licence's own terms. They are quoted because
the licence applies and its terms are fixed, not because they were found in the
package.

This list is closed and exhaustive. A package that is not named here and has no
established licence text fails generation.

DECLARED
            local dname dversion did ddecl drepo dcommit dpath
            while IFS=$'\t' read -r dname dversion _ did ddecl _ drepo dcommit dpath _; do
                if [ "$drepo" = '-' ]; then
                    printf -- '- `%s` %s, `%s`, declared in the published manifest as `%s`. No upstream statement exists.\n' \
                        "$dname" "$dversion" "$did" "$ddecl"
                else
                    printf -- '- `%s` %s, `%s`, declared in the published manifest as `%s`, and covered by the statement at `%s` `%s` `%s`.\n' \
                        "$dname" "$dversion" "$did" "$ddecl" "$drepo" "$dcommit" "$dpath"
                fi
            done < "$work/declared.tsv"
            printf '\n'

            cut -f4,6 "$work/declared.tsv" | sort -u > "$work/declared-terms.tsv"
            local tid tdig
            while IFS=$'\t' read -r tid tdig; do
                printf '### Terms of %s\n\n' "$tid"
                printf 'Reproduced for the packages above. No copyright notice accompanies them,\n'
                printf 'because none was published.\n\n'
                printf '```text\n'
                cat "$work/bytext/$tdig"
                [ -s "$work/bytext/$tdig" ] && [ -n "$(tail -c1 "$work/bytext/$tdig")" ] && printf '\n'
                printf '```\n\n'
            done < "$work/declared-terms.tsv"

            # Grouped by the text, not by the commit that serves it. The
            # objc2 crates are published from four different commits carrying
            # one identical statement, and quoting it four times would say
            # nothing four times.
            awk -F'\t' '$7 != "-" { print $10 }' "$work/declared.tsv" \
                | sort -u > "$work/declared-statements.tsv"
            local sdig
            while IFS= read -r sdig; do
                printf '### Licensing statement published by the copyright holder\n\n'
                printf 'This is the publisher stating which licence applies. It is not a licence\n'
                printf 'text. SHA-256 `%s`.\n\n' "$sdig"
                printf 'Published at:\n\n'
                awk -F'\t' -v d="$sdig" '$10 == d { printf "- `%s` `%s` `%s`\n", $7, $8, $9 }' \
                    "$work/declared.tsv" | sort -u
                printf '\nApplies to:\n\n'
                awk -F'\t' -v d="$sdig" '$10 == d { printf "- `%s` %s\n", $1, $2 }' \
                    "$work/declared.tsv" | sort -u
                printf '\n'
                printf '```text\n'
                cat "$work/bytext/$sdig"
                [ -s "$work/bytext/$sdig" ] && [ -n "$(tail -c1 "$work/bytext/$sdig")" ] && printf '\n'
                printf '```\n\n'
            done < "$work/declared-statements.tsv"
        fi
    } > "$output"
}


# A licence file that is still a template carries no copyright notice, and MIT
# requires the notice, not only the terms. cargo-about invents such a template
# when it recognises no file; publishers also ship them, REUSE-style, in a
# vendored LICENSES/ directory next to the real one. Both reach the notice
# looking like a licence, so both are refused here rather than in one place.
reject_placeholder_texts() {
    local target="$1" digest hits
    : > "$work/placeholder.tsv"
    cut -f6 "$work/resolved.tsv" | sort -u | while IFS= read -r digest; do
        [ -n "$digest" ] || continue
        if grep -qE '<year>|<copyright holders>|\[year\] \[fullname\]' "$work/bytext/$digest"; then
            printf '%s\n' "$digest" >> "$work/placeholder.tsv"
        fi
    done
    [ -s "$work/placeholder.tsv" ] || return 0

    {
        echo "$NOTICE_TOOL: these licence texts are unfilled templates on $target."
        echo "A template states the terms but carries no copyright notice, and MIT requires the"
        echo "notice. Name the crate's real licence file in a checksum-bound clarify block in"
        echo "$NOTICE_ABOUT_CONFIG so the template stops being a candidate."
        echo
        while IFS= read -r digest; do
            printf '  text %s, used for:\n' "$digest"
            awk -F'\t' -v d="$digest" '$6 == d { printf "    %s %s  licence=%s\n", $1, $2, $4 }' \
                "$work/resolved.tsv" | sort -u
        done < "$work/placeholder.tsv"
    } >&2
    hits="$(wc -l < "$work/placeholder.tsv" | tr -d ' ')"
    notice_die "$hits licence text(s) on $target are unfilled templates"
}

generate_one() {
    local target="$1" output="$2"
    collect_graph "$target"
    union_graph "$target"
    oracle_graph "$target"
    resolve_texts "$target"
    [ -n "$discover" ] || reject_placeholder_texts "$target"
    [ -n "$discover" ] && return 0
    mkdir -p "$(dirname "$output")"
    render_notice "$target" "$output"
    echo "$NOTICE_TOOL: wrote $output"
}

if [ -n "$target" ]; then
    [ -n "$output" ] || [ -n "$discover" ] || notice_die '--target needs --output'
    generate_one "$target" "${output:-/dev/null}"
else
    [ -z "$output" ] || notice_die '--output needs --target'
    for t in "${NOTICE_TARGETS[@]}"; do
        generate_one "$t" "$NOTICE_DIR/NOTICE-$t.md"
    done
fi
