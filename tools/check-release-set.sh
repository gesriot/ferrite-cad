#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Says whether a directory is one release set: the three platform archives of
# one product version, built from one revision of this repository, with every
# byte of every archive pinned by a document that travels with them.
#
# What was there before this, and why it could not answer. Three archives were
# uploaded by three jobs as three separate artifacts. Each of them verified
# beautifully on its own: extracted somewhere it had never been, re-hashed
# against its manifest, started with the build trees taken away. And nothing
# anywhere said the other two existed. A set with two targets in it looked
# exactly like a set with three. A set with the Linux archive in it twice
# looked exactly like a set with Linux and macOS. And an archive packed from
# another commit that happened to carry the same product version was
# indistinguishable from the right one, because nothing in a package manifest
# said which commit it came from.
#
# So this is asked of the output rather than of the inputs. It re-hashes the
# archives that are actually in the directory, reads each package manifest out
# of the archive it came in, and requires the release set document to agree
# with the bytes rather than with whatever the builder was told. A checker that
# read the builder's inputs would be checking the builder's arithmetic twice
# and the delivery not at all.
#
# It runs offline and it does not verify a package itself: each archive is
# handed to tools/check-release-package.sh --no-execute, which already owns
# that question. A second implementation here would be a package that passed
# one of them and failed the other while both went on looking right.
#
# Usage:
#   tools/check-release-set.sh --release-set DIR --output facts.txt \
#       --forbidden DIR [--forbidden DIR]...
#
# --forbidden names the build trees that must not exist on the machine asking,
# and is passed on: an archive verified on the machine that built it says less
# than the same archive verified where nothing it needs was ever installed.

set -euo pipefail

PACKAGE_TOOL='check-release-set'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/package/lib.sh
. tools/package/lib.sh

release_set=''
output=''
forbidden=()

while [ $# -gt 0 ]; do
    case "$1" in
        --release-set) release_set="${2:?--release-set needs a directory}"; shift 2 ;;
        --output)      output="${2:?--output needs a file}"; shift 2 ;;
        --forbidden)   forbidden+=("${2:?--forbidden needs a directory}"); shift 2 ;;
        *) package_die "unknown argument: $1" ;;
    esac
done

[ -n "$release_set" ] || package_die 'no --release-set'
[ -n "$output" ] || package_die 'no --output'
[ "${#forbidden[@]}" -gt 0 ] || package_die \
    'no --forbidden directory, so nothing says the machine asking is not the machine that built'
release_set="$(package_posix_path "$release_set")"
[ -d "$release_set" ] || package_die "no such directory: $release_set"

native_require_jq

# Spelled as the pairs tools/check-release-package.sh reads, once, here.
forbidden_arguments=()
for directory in "${forbidden[@]}"; do
    forbidden_arguments+=(--forbidden "$directory")
done

work="$(mktemp -d)"
# shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

: > "$output"
fact() { printf '%s\n' "$*" >> "$output"; }

# ---------------------------------------------------------------------------
# There has to be a release set at all.
#
# Asked first and out loud, because this is the defect the slice exists to fix
# rather than a missing file. Three archives in a directory are three files.
# ---------------------------------------------------------------------------

document="$release_set/$RELEASE_SET_NAME"
checksums="$release_set/$RELEASE_SET_CHECKSUMS"
if [ ! -f "$document" ] || [ ! -f "$checksums" ]; then
    echo "$PACKAGE_TOOL: there is no release set in $release_set." >&2
    echo >&2
    echo "  What is there is a directory with archives in it, and a directory with" >&2
    echo "  archives in it cannot be refused. Nothing in it requires all three" >&2
    echo "  targets to be present, so a set that lost one looks like a set. Nothing" >&2
    echo "  requires them to be different targets, so the same archive twice looks" >&2
    echo "  like two. Nothing requires one product version across them, and nothing" >&2
    echo "  at all requires one source revision - so an archive built from another" >&2
    echo "  commit that happens to carry the same version is accepted by every gate" >&2
    echo "  this repository has, because no package manifest says which commit it" >&2
    echo "  was built from. And nothing pins the bytes: an archive replaced after" >&2
    echo "  the fact is still an archive." >&2
    echo >&2
    echo "  tools/build-release-set.sh is what would produce one." >&2
    exit 1
fi
fact "release-set document-present=true"

# ---------------------------------------------------------------------------
# Exactly the five files a release set is, and nothing else.
# ---------------------------------------------------------------------------
#
# A release set is what is handed on. An extra file in it is a file a recipient
# would receive without anything saying what it is, and a directory in it is a
# whole tree of them.

: > "$work/present"
while IFS= read -r found; do
    printf '%s\n' "${found#"$release_set"/}" >> "$work/present"
done < <(find "$release_set" -mindepth 1 -maxdepth 1 -type f)
LC_ALL=C sort -o "$work/present" "$work/present"

: > "$work/present-odd"
while IFS= read -r found; do
    printf '%s\n' "${found#"$release_set"/}" >> "$work/present-odd"
done < <(find "$release_set" -mindepth 1 ! -type f)
if [ -s "$work/present-odd" ]; then
    echo "$PACKAGE_TOOL: $release_set holds something that is not a regular file:" >&2
    sed 's/^/  /' "$work/present-odd" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# What the document says it is.
# ---------------------------------------------------------------------------

jq -e . "$document" > /dev/null 2>&1 || package_die "$RELEASE_SET_NAME is not JSON"

# The field names, exactly. Everything this document is allowed to carry is
# listed here, so a run identifier, a timestamp, a hostname or a runner path
# added to it is a failure rather than a field a reader has to know to ignore.
jq -r 'keys_unsorted[]' "$document" | native_strip_cr | LC_ALL=C sort > "$work/keys"
printf '%s\n' checksums formatVersion kind packageManifestFormat packageManifestPath \
    productVersion sourceRevision targets | LC_ALL=C sort > "$work/keys-expected"
if ! diff -u "$work/keys-expected" "$work/keys" > "$work/keys-diff"; then
    echo "$PACKAGE_TOOL: $RELEASE_SET_NAME does not carry the fields a release set carries:" >&2
    grep '^-[^-]' "$work/keys-diff" | sed 's/^-/  missing: /' >&2 || true
    grep '^+[^+]' "$work/keys-diff" | sed 's/^+/  unknown: /' >&2 || true
    exit 1
fi

jq -r '.targets[] | keys_unsorted[]' "$document" | native_strip_cr \
    | LC_ALL=C sort -u > "$work/target-keys"
printf '%s\n' archive packageManifestSha256 root sha256 size target \
    | LC_ALL=C sort > "$work/target-keys-expected"
if ! diff -u "$work/target-keys-expected" "$work/target-keys" > "$work/target-keys-diff"; then
    echo "$PACKAGE_TOOL: the targets in $RELEASE_SET_NAME do not carry the fields a target carries:" >&2
    sed 's/^/  /' "$work/target-keys-diff" >&2
    exit 1
fi

# And that no string in it is host-local. A release set that named a runner's
# absolute path, or carried a colon, would be a document about the machine that
# assembled it rather than about the product.
jq -r '[.. | strings] | .[]' "$document" | native_strip_cr > "$work/strings"
: > "$work/host-local"
while IFS= read -r value; do
    case "$value" in
        [A-Za-z0-9]*) ;;
        *) printf '%s\n' "$value" >> "$work/host-local"; continue ;;
    esac
    case "$value" in
        *[!A-Za-z0-9._/+-]* | *..*) printf '%s\n' "$value" >> "$work/host-local" ;;
    esac
done < "$work/strings"
if [ -s "$work/host-local" ]; then
    echo "$PACKAGE_TOOL: $RELEASE_SET_NAME carries something that belongs to a machine \
rather than to the product:" >&2
    sed 's/^/  /' "$work/host-local" >&2
    echo "An absolute path, a drive letter, a timestamp or a run identifier makes the" >&2
    echo "document describe the run that assembled it rather than what was assembled." >&2
    exit 1
fi
fact "release-set host-local-values=0"

said() { jq -r "$1" "$document" | native_strip_cr; }
set_kind="$(said '.kind')"
set_format="$(said '.formatVersion')"
set_version="$(said '.productVersion')"
set_revision="$(said '.sourceRevision')"
set_manifest_format="$(said '.packageManifestFormat')"
set_manifest_path="$(said '.packageManifestPath')"
set_checksums="$(said '.checksums')"

[ "$set_kind" = "$RELEASE_SET_KIND" ] \
    || package_die "the document calls itself '$set_kind' rather than $RELEASE_SET_KIND"
[ "$set_format" = "$RELEASE_SET_FORMAT" ] \
    || package_die "the release set is format $set_format and this gate reads $RELEASE_SET_FORMAT"
[ -n "$set_version" ] && [ "$set_version" != null ] \
    || package_die 'the release set does not say which product version it is'
package_valid_revision "$set_revision" \
    || package_die "'$set_revision' is not a source revision; a full lower-case commit object name is"
[ "$set_manifest_format" = "$PACKAGE_FORMAT" ] \
    || package_die "the release set expects package manifest format $set_manifest_format \
and this gate reads $PACKAGE_FORMAT"
[ "$set_manifest_path" = "$(package_manifest_path)" ] \
    || package_die "the release set points at '$set_manifest_path' for the package manifest"
[ "$set_checksums" = "$RELEASE_SET_CHECKSUMS" ] \
    || package_die "the release set names its checksums file '$set_checksums'"
fact "release-set version=$set_version source-revision=$set_revision"

# ---------------------------------------------------------------------------
# Three targets, and the three the product has.
# ---------------------------------------------------------------------------
#
# The set is compared rather than counted. Three entries all saying
# x86_64-unknown-linux-gnu satisfy a count, and mean that two thirds of the
# product is missing while the same third was shipped three times.

jq -r '.targets[].target' "$document" | native_strip_cr > "$work/targets"
count="$(wc -l < "$work/targets" | tr -d ' ')"
[ "$count" -eq 3 ] || package_die "the release set names $count targets and a release set is three"
LC_ALL=C sort "$work/targets" > "$work/targets-sorted"
if uniq -d "$work/targets-sorted" | grep .; then
    package_die 'the release set names one target more than once'
fi
package_expected_targets > "$work/targets-expected"
if ! diff -u "$work/targets-expected" "$work/targets-sorted" > "$work/targets-diff"; then
    echo "$PACKAGE_TOOL: the release set is not the three targets the product has:" >&2
    grep '^-[^-]' "$work/targets-diff" | sed 's/^-/  missing: /' >&2 || true
    grep '^+[^+]' "$work/targets-diff" | sed 's/^+/  unknown: /' >&2 || true
    exit 1
fi

# In one order that depends only on the names, so two assemblies of the same
# three archives cannot differ because artifacts were downloaded in a different
# order.
if ! cmp -s "$work/targets" "$work/targets-sorted"; then
    package_die 'the targets of the release set are not in sorted order, so the document depends on the order its inputs arrived in'
fi
fact "release-set targets=3 order=sorted"

# ---------------------------------------------------------------------------
# The archives that are really there, against the archives it names.
# ---------------------------------------------------------------------------

jq -r '.targets[].archive' "$document" | native_strip_cr | LC_ALL=C sort > "$work/named-archives"
{ printf '%s\n' "$RELEASE_SET_NAME" "$RELEASE_SET_CHECKSUMS"; cat "$work/named-archives"; } \
    | LC_ALL=C sort > "$work/expected-files"
if ! diff -u "$work/expected-files" "$work/present" > "$work/files-diff"; then
    echo "$PACKAGE_TOOL: $release_set is not the files a release set is:" >&2
    grep '^-[^-]' "$work/files-diff" | sed 's/^-/  named and not there: /' >&2 || true
    grep '^+[^+]' "$work/files-diff" | sed 's/^+/  there and not named: /' >&2 || true
    echo "A release set is three archives and two files describing them. Anything else" >&2
    echo "in it is something a recipient receives with nothing saying what it is." >&2
    exit 1
fi
fact "release-set files=5 extra=0"

# ---------------------------------------------------------------------------
# Every archive, re-hashed, and every package manifest read out of it.
# ---------------------------------------------------------------------------

: > "$work/checksum-lines"
jq -r '.targets[] | [.target, .archive, .sha256, (.size|tostring), .root, .packageManifestSha256] | @tsv' \
    "$document" | native_strip_cr > "$work/claimed"

while IFS="$(printf '\t')" read -r target archive claimed_digest claimed_size claimed_root claimed_manifest_digest; do
    file="$release_set/$archive"
    platform="$(package_platform_for_triple "$target")"

    # The name, from the version and the target rather than from the document's
    # own idea of one. An archive renamed after it was built is a file a
    # recipient reads the target off and gets the wrong answer.
    [ "$archive" = "$(package_archive_for "$set_version" "$target")" ] \
        || package_die "the archive of $target is named '$archive' and version $set_version of $target is '$(package_archive_for "$set_version" "$target")'"

    actual_digest="$(package_sha256 "$file")"
    actual_size="$(package_size "$file")"
    [ "$actual_digest" = "$claimed_digest" ] \
        || package_die "$archive hashes to $actual_digest and the release set says $claimed_digest"
    [ "$actual_size" = "$claimed_size" ] \
        || package_die "$archive is $actual_size bytes and the release set says $claimed_size"

    root="$(package_read_manifest "$file" "$work")"
    [ "$root" = "$claimed_root" ] \
        || package_die "$archive extracts into '$root' and the release set says '$claimed_root'"
    [ "$root" = "$(package_root_for "$set_version" "$target")" ] \
        || package_die "$archive extracts into '$root', which is not version $set_version of $target"

    manifest_digest="$(package_sha256 "$work/package-manifest.json")"
    [ "$manifest_digest" = "$claimed_manifest_digest" ] \
        || package_die "the package manifest in $archive hashes to $manifest_digest and the release set says $claimed_manifest_digest"

    # And the package manifest itself, on the four things the set asserts about
    # it. Read out of the archive rather than taken from the builder: the whole
    # question is whether the document describes the bytes that are here.
    read_manifest() { jq -r "$1" "$work/package-manifest.json" | native_strip_cr; }
    [ "$(read_manifest '.kind')" = "$PACKAGE_KIND" ] \
        || package_die "the manifest in $archive calls itself '$(read_manifest '.kind')'"
    [ "$(read_manifest '.formatVersion')" = "$PACKAGE_FORMAT" ] \
        || package_die "the manifest in $archive is format $(read_manifest '.formatVersion') \
and a release set is made of format $PACKAGE_FORMAT packages, which are the ones that say \
which commit they were built from"
    [ "$(read_manifest '.target')" = "$target" ] \
        || package_die "the manifest in $archive is for $(read_manifest '.target') and the release set files it under $target"
    [ "$(read_manifest '.productVersion')" = "$set_version" ] \
        || package_die "the manifest in $archive says version $(read_manifest '.productVersion') and the release set says $set_version"
    [ "$(read_manifest '.sourceRevision')" = "$set_revision" ] \
        || package_die "the manifest in $archive was built from $(read_manifest '.sourceRevision') \
and the release set says $set_revision; two archives of one version from two commits are not one product"
    [ "$(read_manifest '.archive')" = "$archive" ] \
        || package_die "the manifest in $archive names its archive '$(read_manifest '.archive')'"

    printf '%s  %s\n' "$actual_digest" "$archive" >> "$work/checksum-lines"
    fact "release-set target=$target archive=$archive sha256=$actual_digest size=$actual_size"

    # The archive itself, by the gate that already owns that question.
    rm -rf "$work/extracted"
    if ! tools/check-release-package.sh --platform "$platform" --no-execute \
            --archive "$file" --extract-to "$work/extracted" \
            "${forbidden_arguments[@]}" \
            --output "$work/package-facts.txt" > "$work/package.log" 2>&1; then
        echo "$PACKAGE_TOOL: $archive is not a package:" >&2
        sed 's/^/  /' "$work/package.log" >&2
        exit 1
    fi
    grep -q '^package manifest-verified-against-extracted-bytes=true' "$work/package-facts.txt" \
        || package_die "verifying $archive produced no fact saying its bytes were checked"
    grep -q '^package product-sbom=byte-identical' "$work/package-facts.txt" \
        || package_die "verifying $archive produced no fact about the product SBOM inside it"
done < "$work/claimed"
fact "release-set archives-verified=3"

# ---------------------------------------------------------------------------
# The checksums file, which is the half of this a recipient can use with
# nothing but sha256sum.
# ---------------------------------------------------------------------------
#
# Written from the digests measured above rather than compared line by line
# against the file: a comparison of the whole file catches a line that is
# missing, a line that is spare, an order that came from somewhere else and a
# name that was escaped, all of which are ways a recipient's `sha256sum -c`
# ends up checking something other than what is here.

LC_ALL=C sort -k2 -o "$work/checksum-lines" "$work/checksum-lines"
if LC_ALL=C grep -q $'\r' "$checksums"; then
    package_die "$RELEASE_SET_CHECKSUMS carries carriage returns, so it was written by a tool in text mode"
fi
if LC_ALL=C grep -q '^\\' "$checksums"; then
    package_die "$RELEASE_SET_CHECKSUMS has an escaped line; GNU checksum tools escape a name \
with a backslash in it and mark it with a leading backslash, and a release archive has no \
backslash in its name"
fi
if ! LC_ALL=C grep -qE '^[0-9a-f]{64}  [A-Za-z0-9][A-Za-z0-9._+-]*$' "$checksums" \
    || LC_ALL=C grep -vqE '^[0-9a-f]{64}  [A-Za-z0-9][A-Za-z0-9._+-]*$' "$checksums"; then
    echo "$PACKAGE_TOOL: $RELEASE_SET_CHECKSUMS is not lines of a digest and a name:" >&2
    sed 's/^/  /' "$checksums" >&2
    exit 1
fi
if ! diff -u "$work/checksum-lines" "$checksums" > "$work/checksum-diff"; then
    echo "$PACKAGE_TOOL: $RELEASE_SET_CHECKSUMS does not say what the archives hash to:" >&2
    grep '^-[^-]' "$work/checksum-diff" | sed 's/^-/  measured here:  /' >&2 || true
    grep '^+[^+]' "$work/checksum-diff" | sed 's/^+/  in the file:    /' >&2 || true
    exit 1
fi
fact "release-set checksums=3 order=sorted escaping=none"

echo "--- facts ---"
LC_ALL=C sort -o "$output" "$output"
cat "$output"
echo
echo "$PACKAGE_TOOL: $release_set is one release set: three targets, version \
$set_version, source revision $set_revision"
