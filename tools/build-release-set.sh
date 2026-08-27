#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Assembles three finished platform archives into one release set.
#
# The three archives already exist when this runs, and it is handed them. It
# does not build, stage, repack, rename or rewrite anything: an assembler that
# could produce an archive could produce one that no platform job ever ran, and
# the only thing that makes a release archive worth anything is that the
# platform it is for extracted it and started it. So the archives are read,
# hashed and copied, and never opened for writing.
#
# What it decides, and from what. Nothing here is derived from the repository
# it is standing in. The product version and the source revision come out of
# the package manifests inside the archives, because those are what the
# machines that built them recorded; a version read from the inventory or a
# revision read from `git rev-parse HEAD` would describe the checkout doing the
# assembling, and the whole point of the document is to describe the archives.
# The three expected targets come from tools/native/lib.sh, which already owns
# which platforms this product has.
#
# What it refuses. Fewer than three targets, the same target twice, a target
# the product does not have, two product versions, two source revisions, a
# package manifest of a format that does not record a revision, an archive
# whose name does not match what its own manifest says it is, and an archive
# that carries no manifest at all. Every one of those is a set that a recipient
# would take for a release of this product and that is not one.
#
# It publishes nothing. The output is a directory: three archives, one release
# set document and one checksums file. There is no GitHub Release, no tag, no
# installer, no signature and no notarisation here, and nothing downstream of
# this slice creates one.
#
# Usage:
#   tools/build-release-set.sh --archive FILE --archive FILE --archive FILE \
#       --output-dir DIR

set -euo pipefail

PACKAGE_TOOL='build-release-set'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/package/lib.sh
. tools/package/lib.sh

archives=()
output_dir=''

while [ $# -gt 0 ]; do
    case "$1" in
        --archive)    archives+=("${2:?--archive needs a file}"); shift 2 ;;
        --output-dir) output_dir="${2:?--output-dir needs a directory}"; shift 2 ;;
        *) package_die "unknown argument: $1" ;;
    esac
done

[ -n "$output_dir" ] || package_die 'no --output-dir'
[ "${#archives[@]}" -eq 3 ] || package_die \
    "given ${#archives[@]} archives; a release set is the three platform archives of one build"
output_dir="$(package_posix_path "$output_dir")"

native_require_jq
work="$(mktemp -d)"
# shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

# ---------------------------------------------------------------------------
# The three archives, as files.
# ---------------------------------------------------------------------------

: > "$work/inputs"
for archive in "${archives[@]}"; do
    archive="$(package_posix_path "$archive")"
    [ -f "$archive" ] || package_die "no such archive: $archive"
    printf '%s\n' "$archive" >> "$work/inputs"
done
if LC_ALL=C sort "$work/inputs" | uniq -d | grep .; then
    package_die 'the same archive was given twice'
fi

# The output directory is made rather than added to. Assembling into a
# directory that already held something would put a file into the release set
# that nothing in this run produced.
if [ -e "$output_dir" ] && [ -n "$(ls -A "$output_dir" 2>/dev/null)" ]; then
    package_die "$output_dir is not empty; a release set is assembled into a directory of its own"
fi
mkdir -p "$output_dir"

# ---------------------------------------------------------------------------
# What each archive says it is, read out of the archive.
# ---------------------------------------------------------------------------

: > "$work/entries"
while IFS= read -r archive; do
    name="$(basename "$archive")"
    root="$(package_read_manifest "$archive" "$work")"
    manifest="$work/package-manifest.json"
    say() { jq -r "$1" "$manifest" | native_strip_cr; }

    kind="$(say '.kind')"
    [ "$kind" = "$PACKAGE_KIND" ] \
        || package_die "the manifest in $name calls itself '$kind' rather than $PACKAGE_KIND"
    format="$(say '.formatVersion')"
    [ "$format" = "$PACKAGE_FORMAT" ] || package_die \
        "$name carries a format $format package manifest and a release set is made of format \
$PACKAGE_FORMAT ones; before $PACKAGE_FORMAT a manifest did not say which commit it was built \
from, so a set of them could not refuse an archive from another commit"

    target="$(say '.target')"
    version="$(say '.productVersion')"
    revision="$(say '.sourceRevision')"
    said_archive="$(say '.archive')"
    said_root="$(say '.root')"

    [ -n "$target" ] && [ "$target" != null ] || package_die "$name does not say which target it is for"
    [ -n "$version" ] && [ "$version" != null ] || package_die "$name does not say which version it is"
    package_valid_revision "$revision" \
        || package_die "$name says it was built from '$revision', which is not a full lower-case commit object name"

    # The name on disk against the name the archive was written under. An
    # archive renamed on its way here is one a recipient reads the target and
    # the version off and gets the wrong answer for.
    [ "$said_archive" = "$name" ] \
        || package_die "the manifest in $name says its archive is '$said_archive'"
    [ "$said_root" = "$root" ] \
        || package_die "the manifest in $name says it extracts into '$said_root' and it extracts into '$root'"
    [ "$root" = "$(package_root_for "$version" "$target")" ] \
        || package_die "$name extracts into '$root', which is not version $version of $target"
    [ "$name" = "$(package_archive_for "$version" "$target")" ] \
        || package_die "$name is not what version $version of $target is called"

    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$target" "$name" "$version" "$revision" \
        "$root" "$(package_sha256 "$archive")" "$(package_size "$archive")" \
        "$(package_sha256 "$manifest")" >> "$work/entries"
    printf '%s\t%s\n' "$target" "$archive" >> "$work/sources"
done < "$work/inputs"

LC_ALL=C sort -o "$work/entries" "$work/entries"

# ---------------------------------------------------------------------------
# Three targets, and the three this product has.
# ---------------------------------------------------------------------------

cut -f1 "$work/entries" > "$work/targets"
if uniq -d "$work/targets" | grep .; then
    package_die 'two of the archives are for the same target, so the set is missing one'
fi
package_expected_targets > "$work/targets-expected"
if ! diff -u "$work/targets-expected" "$work/targets" > "$work/targets-diff"; then
    echo "$PACKAGE_TOOL: these archives are not the three targets the product has:" >&2
    grep '^-[^-]' "$work/targets-diff" | sed 's/^-/  missing: /' >&2 || true
    grep '^+[^+]' "$work/targets-diff" | sed 's/^+/  unknown: /' >&2 || true
    exit 1
fi

# ---------------------------------------------------------------------------
# One product, which is one version and one commit.
# ---------------------------------------------------------------------------

cut -f3 "$work/entries" | LC_ALL=C sort -u > "$work/versions"
if [ "$(wc -l < "$work/versions" | tr -d ' ')" -ne 1 ]; then
    echo "$PACKAGE_TOOL: these archives carry more than one product version:" >&2
    awk -F'\t' '{ printf "  %s  %s\n", $3, $2 }' "$work/entries" >&2
    exit 1
fi
version="$(cat "$work/versions")"

cut -f4 "$work/entries" | LC_ALL=C sort -u > "$work/revisions"
if [ "$(wc -l < "$work/revisions" | tr -d ' ')" -ne 1 ]; then
    echo "$PACKAGE_TOOL: these archives were built from more than one source revision:" >&2
    awk -F'\t' '{ printf "  %s  %s\n", $4, $2 }' "$work/entries" >&2
    echo >&2
    echo "  They carry one product version, so nothing about their names, their sizes or" >&2
    echo "  their contents says they are not one product. They are three builds of two" >&2
    echo "  different trees, and a recipient given them would be running a Windows build" >&2
    echo "  of one commit against a Linux build of another." >&2
    exit 1
fi
revision="$(cat "$work/revisions")"

# ---------------------------------------------------------------------------
# The two documents, and the three archives beside them.
# ---------------------------------------------------------------------------
#
# Sorted by target and written with sorted keys, so assembling the same three
# archives twice produces the same bytes whatever order the artifacts arrived
# in. Nothing here is measured from the machine doing the assembling: no
# timestamp, no run identifier, no absolute path, no hostname. A release set
# that carried any of those would be a document about a run.

: > "$work/target-objects"
while IFS="$(printf '\t')" read -r target name _version _revision root digest size manifest_digest; do
    jq -n --arg target "$target" \
          --arg archive "$name" \
          --arg sha256 "$digest" \
          --argjson size "$size" \
          --arg root "$root" \
          --arg packageManifestSha256 "$manifest_digest" \
          '{$target, $archive, $sha256, $size, $root, $packageManifestSha256}' \
          >> "$work/target-objects"
done < "$work/entries"

jq -s -S \
    --arg kind "$RELEASE_SET_KIND" \
    --argjson formatVersion "$RELEASE_SET_FORMAT" \
    --arg productVersion "$version" \
    --arg sourceRevision "$revision" \
    --argjson packageManifestFormat "$PACKAGE_FORMAT" \
    --arg packageManifestPath "$(package_manifest_path)" \
    --arg checksums "$RELEASE_SET_CHECKSUMS" \
    --slurpfile targets "$work/target-objects" \
    '{
        kind: $kind,
        formatVersion: $formatVersion,
        productVersion: $productVersion,
        sourceRevision: $sourceRevision,
        packageManifestFormat: $packageManifestFormat,
        packageManifestPath: $packageManifestPath,
        checksums: $checksums,
        targets: ($targets | sort_by(.target))
     }' /dev/null \
    | native_strip_cr > "$output_dir/$RELEASE_SET_NAME"

jq -e . "$output_dir/$RELEASE_SET_NAME" > /dev/null \
    || package_die 'the release set document that was just written is not JSON'

# The checksums a recipient can use with nothing but sha256sum, sorted by name
# in the C locale so the file depends only on the names. Written by hand rather
# than by `sha256sum FILE`, because GNU checksum tools escape a name that
# contains a backslash and mark it with a leading backslash - a release archive
# has no backslash in its name, and a checker that tolerated the escape would
# tolerate the name that needed it.
awk -F'\t' '{ printf "%s  %s\n", $6, $2 }' "$work/entries" \
    | LC_ALL=C sort -k2 > "$output_dir/$RELEASE_SET_CHECKSUMS"

# And the archives themselves, copied and compared. Three inputs in, three
# archives out, and nothing repacked on the way.
while IFS="$(printf '\t')" read -r target archive; do
    name="$(basename "$archive")"
    cp "$archive" "$output_dir/$name"
    cmp -s "$archive" "$output_dir/$name" \
        || package_die "copying $name into the release set changed its bytes"
    chmod 644 "$output_dir/$name"
    : "$target"
done < "$work/sources"

# ---------------------------------------------------------------------------
# What this produced.
# ---------------------------------------------------------------------------

echo "# FerriteCAD release set"
echo "version         $version"
echo "source revision $revision"
echo "targets         3"
awk -F'\t' '{ printf "  %-26s %s  %s bytes\n", $1, $6, $7 }' "$work/entries"
echo "document        $RELEASE_SET_NAME"
echo "checksums       $RELEASE_SET_CHECKSUMS"
echo "published       no; this is a directory of files and not a release"
