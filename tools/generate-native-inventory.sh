#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Generates the deterministic native/assets inventory for the FerriteCAD
# product.
#
# This is an intermediate manifest and says so in its own first field. It is
# not a product SBOM and does not merge anything into one: it describes the
# native components a release carries, the native inputs that only take part in
# a build, and the assets a product binary embeds. §21A-2b2b0b2b2 is the merge.
#
# Nothing here is invented and nothing here is typed twice.
#
#   * Versions, source URLs and source digests come from the two pin files.
#     tools/occt/pin.env owns Open CASCADE; tools/planegcs/pin.env owns
#     planegcs, Eigen and Boost, and tools/check-planegcs-pins.sh already
#     refuses a second copy of any of its digests anywhere that runs.
#   * Which staged runtime file belongs to which component comes from the three
#     measured ownership maps under tools/native, which the combined runtime
#     layout workflow is what produced.
#   * Which assets are embedded, and in which binary, comes from walking the
#     product graph cargo's own feature-aware resolver reports and reading the
#     files it names. Their digests are taken from those files here rather than
#     written down by hand.
#   * The product roots and the product targets come from tools/notices/lib.sh,
#     which has owned them since the notices needed them first.
#   * Which crate the Windows import library is a build input of comes from
#     reading the build scripts for the one that names the file. It is the one
#     relationship in this document whose direction is not obvious, and the
#     first version of the document had it backwards.
#
# What it deliberately does not carry is the digest of any built binary. Two
# clean builds of the same target on the same host reproduce their bytes; the
# same pinned sources on a different host do not, measured on macOS against the
# runner. A committed inventory that named those bytes would be describing a
# machine. The packager adds build-local file digests for one build; the
# component identities here do not change when it does.
#
# Runs no network.
#
# Run from the repository root:
#   tools/generate-native-inventory.sh [--output <file>]

set -euo pipefail

NATIVE_TOOL='generate-native-inventory'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/native/lib.sh
. tools/native/lib.sh

output="$NATIVE_INVENTORY"
while [ $# -gt 0 ]; do
    case "$1" in
        --output) output="${2:?--output needs a path}"; shift 2 ;;
        *) native_die "unknown argument: $1" ;;
    esac
done

native_load_pins
native_require_jq

work="$(mktemp -d)"
# The status is saved and handed back explicitly. bash 3.2, which is what
# macOS ships, lets the last command of an EXIT trap decide the shell's exit
# status: a run that died on an unbound variable and never wrote a file exited
# 0 because `rm -rf` had succeeded, and the caller reported a confusing
# staleness instead of a generator that failed. Found by a mutation.
# shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

# A stale lock would make cargo's resolver and the committed fragments answer
# about different graphs.
cargo metadata --locked --format-version 1 --no-deps >/dev/null 2>&1 \
    || native_die 'cargo metadata --locked failed: Cargo.lock is stale or unreadable'

# ---------------------------------------------------------------------------
# The measured ownership maps.
# ---------------------------------------------------------------------------

: > "$work/staged.tsv"
for platform in "${NATIVE_PLATFORMS[@]}"; do
    map="$(native_staged_map_for "$platform")"
    [ -f "$map" ] || native_die "$map is missing, and it is what says who owns what"
    native_map_rows "$map" | awk -F'\t' -v p="$platform" -v OFS='\t' \
        'NF != 2 { exit 1 } { print p, $1, $2 }' >> "$work/staged.tsv" \
        || native_die "$map has a row that is not two tab separated columns"
done
sort -u "$work/staged.tsv" -o "$work/staged.tsv"

# ---------------------------------------------------------------------------
# Where every package in the product graph lives, and which roots reach it.
# ---------------------------------------------------------------------------

# `--no-deps` for the workspace paths, taken against cargo's own workspace root
# so the answer is the same on a host that spells absolute paths with a drive
# letter. The full metadata is needed too, for the registry source directories
# that hold the vendored fonts, and those are host paths that must be read and
# never written down.
cargo metadata --locked --format-version 1 2>/dev/null > "$work/metadata.json" \
    || native_die 'cargo metadata failed'

jq -r '(.workspace_root | gsub("\\\\"; "/")) as $root
       | .packages[]
       | [(.name + "@" + .version),
          (.manifest_path | gsub("\\\\"; "/") | rtrimstr("/Cargo.toml")),
          (if (.manifest_path | gsub("\\\\"; "/") | startswith($root + "/"))
           then (.manifest_path | gsub("\\\\"; "/") | ltrimstr($root + "/")
                 | rtrimstr("/Cargo.toml"))
           else "" end)]
       | @tsv' "$work/metadata.json" | native_strip_cr \
    | sort -u > "$work/packages.tsv"

# Which product root reaches which package, per target. Normal edges only and
# the release's own features, exactly as tools/generate-rust-sbom.sh asks it:
# a second spelling of the same question is a second answer.
: > "$work/reach.tsv"
for target in "${NOTICE_TARGETS[@]}"; do
    for root in "${NOTICE_ROOTS[@]}"; do
        bin="${root%%|*}"
        manifest="${root#*|}"; manifest="${manifest%%|*}"
        features="${root##*|}"
        tree=(cargo tree --locked --target "$target" -e normal
              --prefix depth --format '{p}' --manifest-path "$manifest")
        [ -z "$features" ] || tree+=(--features "$features")
        "${tree[@]}" 2>/dev/null | native_strip_cr \
            | awk -v bin="$bin" -v target="$target" -v OFS='\t' '
                {
                    if (match($0, /^[0-9]+/) == 0) next
                    rest = substr($0, RLENGTH + 1)
                    if (match(rest, /^[^ ]+ v[^ ]+/) == 0) next
                    head = substr(rest, 1, RLENGTH)
                    i = index(head, " v")
                    print target, bin, substr(head, 1, i - 1) "@" substr(head, i + 2)
                    seen = 1
                }
                END { if (!seen) exit 1 }' >> "$work/reach.tsv" \
            || native_die "cargo tree produced no graph for $bin on $target"
    done
done
sort -u "$work/reach.tsv" -o "$work/reach.tsv"

# ---------------------------------------------------------------------------
# The embedded assets.
# ---------------------------------------------------------------------------
#
# Two kinds, and the rule that separates them is in tools/native/lib.sh: a file
# of this repository that a product binary embeds, and a font a product binary
# embeds wherever it came from. Both are found by reading the sources cargo
# says are in the graph, never by consulting the inventory being written.

# The relative path of a resolved include inside its own package, with any `..`
# removed by asking the filesystem rather than by string arithmetic.
relative_in_package() { # package-dir resolved-file
    local dir base
    dir="$(cd "$1" && pwd -P)"
    base="$(cd "$(dirname "$2")" && pwd -P)/$(basename "$2")"
    case "$base" in
        "$dir"/*) printf '%s\n' "${base#"$dir"/}" ;;
        *) return 1 ;;
    esac
}

# Which product binaries a package's asset is embedded in: the roots whose
# graph reaches the package, on every target. A package reached on one target
# and not another would make the asset target-specific, which nothing in the
# product is today and which this refuses rather than flattening.
embedded_in() { # package-key
    local key="$1" per_target
    per_target="$(awk -F'\t' -v k="$key" '$3 == k { print $1 "\t" $2 }' "$work/reach.tsv" \
        | sort -u | awk -F'\t' '{ bins[$1] = bins[$1] "," $2 } END { for (t in bins) print bins[t] }' \
        | sort -u)"
    [ "$(printf '%s\n' "$per_target" | wc -l | tr -d ' ')" -eq 1 ] \
        || native_die "the package $key is reached by different binaries on different targets"
    printf '%s\n' "${per_target#,}"
}

: > "$work/assets.tsv"

# (a) this repository's own embedded files, from the workspace packages the
#     product graph reaches.
while IFS=$'\t' read -r key dir repo_rel; do
    [ -n "$repo_rel" ] || continue
    awk -F'\t' -v k="$key" '$3 == k { found = 1 } END { exit found ? 0 : 1 }' "$work/reach.tsv" \
        || continue
    while IFS= read -r resolved; do
        [ -n "$resolved" ] || continue
        rel="$(relative_in_package "$dir" "$resolved")" \
            || native_die "$resolved is embedded by $key from outside the package"
        # On its own line, so that a refusal inside it is the script's exit
        # status. Inside a printf argument it would only be a subshell that
        # exited, and the field would go out empty.
        embedded="$(embedded_in "$key")"
        printf 'repository\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$key" "$repo_rel" "$repo_rel/$rel" "$(basename "$rel")" \
            "$(native_sha256 "$resolved")" "$(wc -c < "$resolved" | tr -d ' ')" \
            "$embedded" >> "$work/assets.tsv"
    done < <(native_scan_includes "$dir")
done < "$work/packages.tsv"

# (b) the vendored fonts.
for crate in ${NATIVE_FONT_CRATES[@]+"${NATIVE_FONT_CRATES[@]}"}; do
    while IFS=$'\t' read -r key dir repo_rel; do
        [ -z "$repo_rel" ] || continue
        [ "${key%@*}" = "$crate" ] || continue
        awk -F'\t' -v k="$key" '$3 == k { found = 1 } END { exit found ? 0 : 1 }' \
            "$work/reach.tsv" || continue
        while IFS= read -r resolved; do
            [ -n "$resolved" ] || continue
            native_is_font_file "$resolved" || continue
            rel="$(relative_in_package "$dir" "$resolved")" \
                || native_die "$resolved is embedded by $key from outside the package"
            embedded="$(embedded_in "$key")"
            printf 'crate\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
                "$key" "" "$rel" "$(basename "$rel")" \
                "$(native_sha256 "$resolved")" "$(wc -c < "$resolved" | tr -d ' ')" \
                "$embedded" >> "$work/assets.tsv"
        done < <(native_scan_includes "$dir")
    done < "$work/packages.tsv"
done

sort -u "$work/assets.tsv" -o "$work/assets.tsv"
[ -s "$work/assets.tsv" ] || native_die \
    'the product graph named no embedded asset at all, which is the traversal failing rather \
than a product without assets'

# ---------------------------------------------------------------------------
# The product roots, and the Rust fragment this inventory is joined to.
# ---------------------------------------------------------------------------

: > "$work/roots.tsv"
for root in "${NOTICE_ROOTS[@]}"; do
    bin="${root%%|*}"
    manifest="${root#*|}"; manifest="${manifest%%|*}"
    dir="${manifest%/Cargo.toml}"
    key="$(awk -F'\t' -v d="$dir" '$3 == d { print $1 }' "$work/packages.tsv")"
    [ -n "$key" ] || native_die "no workspace package is built from $manifest"
    # The reference the Rust fragment already gave this package. Reused rather
    # than rebuilt, so the join cannot drift from the thing it joins to.
    printf '%s\t%s\t%s\t%s\n' "$bin" "${key%@*}" "${key##*@}" "path+$dir#$key" \
        >> "$work/roots.tsv"
done
sort -u "$work/roots.tsv" -o "$work/roots.tsv"

# The one crate whose build reads the Windows import library, and therefore the
# thing that import library is a build input of. Measured from the build script
# that names the file rather than assumed: the direction was recorded backwards
# once, and a value nothing measures cannot notice being wrong again.
consumers="$(native_import_library_consumers)"
consumer_count="$(printf '%s\n' "$consumers" | grep -c . || true)"
[ "$consumer_count" -eq 1 ] || native_die \
    "$consumer_count crates name $NATIVE_IMPORT_LIBRARY in a build script, and exactly one must"
consumer_key="$(awk -F'\t' -v d="$consumers" '$3 == d { print $1 }' "$work/packages.tsv")"
[ -n "$consumer_key" ] || native_die \
    "$consumers names $NATIVE_IMPORT_LIBRARY and is not a package in this workspace"
importlib_consumer="path+$consumers#$consumer_key"

: > "$work/fragments.tsv"
for target in "${NOTICE_TARGETS[@]}"; do
    fragment="sbom/rust/rust-fragment-${target}.cdx.json"
    [ -f "$fragment" ] || native_die "the Rust fragment $fragment is missing"
    printf '%s\t%s\t%s\n' "$target" "$fragment" "$(native_sha256 "$fragment")" \
        >> "$work/fragments.tsv"
done
sort -u "$work/fragments.tsv" -o "$work/fragments.tsv"

: > "$work/targets.tsv"
for platform in "${NATIVE_PLATFORMS[@]}"; do
    printf '%s\t%s\t%s\t%s\n' "$platform" "$(native_triple_for "$platform")" \
        "$(native_bin_dir_for "$platform")" "$(native_lib_dir_for "$platform")" \
        >> "$work/targets.tsv"
done
sort -u "$work/targets.tsv" -o "$work/targets.tsv"

# ---------------------------------------------------------------------------
# The document.
# ---------------------------------------------------------------------------

jq -n -S \
    --rawfile staged "$work/staged.tsv" \
    --rawfile loadedby tools/native/loaded-by.tsv \
    --rawfile assets "$work/assets.tsv" \
    --rawfile roots "$work/roots.tsv" \
    --rawfile fragments "$work/fragments.tsv" \
    --rawfile targets "$work/targets.tsv" \
    --arg format "$NATIVE_INVENTORY_FORMAT" \
    --arg occt_version "$OCCT_VERSION" \
    --arg occt_tag "$OCCT_TAG" \
    --arg occt_commit "$OCCT_COMMIT" \
    --arg occt_sha256 "$OCCT_SHA256" \
    --arg occt_url "$OCCT_ARCHIVE_URL" \
    --arg planegcs_tag "$FCAD_PLANEGCS_FREECAD_TAG" \
    --arg planegcs_url "$FCAD_PLANEGCS_FREECAD_URL" \
    --arg planegcs_sha256 "$FCAD_PLANEGCS_ARCHIVE_SHA256" \
    --arg eigen_version "$FCAD_PLANEGCS_EIGEN_VERSION" \
    --arg eigen_url "$FCAD_PLANEGCS_EIGEN_URL" \
    --arg eigen_sha256 "$FCAD_PLANEGCS_EIGEN_SHA256" \
    --arg boost_version "$FCAD_PLANEGCS_BOOST_VERSION" \
    --arg boost_url "$FCAD_PLANEGCS_BOOST_URL" \
    --arg boost_sha256 "$FCAD_PLANEGCS_BOOST_SHA256" \
    --arg importlib_consumer "$importlib_consumer" \
    -f tools/native/inventory.jq \
    | native_strip_cr > "$work/out.json"

[ -s "$work/out.json" ] || native_die 'the document was not written'
mkdir -p "$(dirname "$output")"
mv "$work/out.json" "$output"
echo "$NATIVE_TOOL: wrote $output"
