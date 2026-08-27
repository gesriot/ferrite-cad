#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Extracts a FerriteCAD release archive somewhere it has never been and says
# whether what came out is the product.
#
# §21A-2b2a proved a staging directory could be started with the build trees
# taken away. That is a statement about a directory the build made and the
# build still owns. This is the other question, and it is the one a recipient
# actually asks: after the archive has been moved to a machine that has no
# staging directory, no build tree and no repository, does what unpacks from it
# start, is every byte of it accounted for, and is it the delivery that
# answered rather than something the host already had?
#
# Everything below is asked of the extracted tree. The staging directory that
# produced the archive must be gone before this runs - a run that could reach
# it would answer the previous slice's question again while looking like this
# one.
#
#   The archive is inspected before it is unpacked. An absolute path, a `..`, a
#   symlink or a device node is a property of the archive; a checker that
#   extracted first would be reporting what its own extraction had already
#   done, and on some of these it would have written outside the directory it
#   was told to use.
#
#   The manifest is checked against the bytes on disk rather than against the
#   bytes it was written from. Re-hashing what was extracted is the only
#   version of this question that a lost executable bit, a truncated file or a
#   manifest describing a different build can fail.
#
#   Both halves of the product run. `--solver-info` says a great deal about
#   planegcs and nothing whatever about Open CASCADE; a rebuild says the
#   opposite.
#
#   Taking a library away stops the process, and putting it back starts it
#   again. A gate that only ever saw one of those two states cannot tell a
#   package that stopped working from one that never did.
#
# Which file is the viewer, which is the command line tool, which is planegcs
# and which are the Open CASCADE toolkits is read out of the manifest's owners.
# This script knows no layout rule: tools/stage-runtime-layout.sh owns those,
# and a second opinion here would be a second layout.
#
# Usage:
#   tools/check-release-package.sh --platform linux|macos|windows \
#       --archive FILE --extract-to DIR --output facts.txt \
#       [--document path/to/plate.fcad] [--forbidden DIR]... [--no-execute]
#
# --forbidden names the directories that must be gone: the build and install
# trees as they were spelled when the binaries were produced, and the staging
# directory the archive was made from.
#
# --no-execute measures everything that does not need the product to run, and
# says so in its facts. The three-platform workflow does not pass it, and the
# comparison there requires facts only a real run produces.

set -euo pipefail

PACKAGE_TOOL='check-release-package'
RUNTIME_PROBE_TOOL='check-release-package'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/package/lib.sh
. tools/package/lib.sh
# shellcheck source=tools/runtime-probe.sh
. tools/runtime-probe.sh

platform=''
archive=''
extract_to=''
output=''
document=''
execute=yes
forbidden=()

while [ $# -gt 0 ]; do
    case "$1" in
        --platform)   platform="${2:?--platform needs a name}"; shift 2 ;;
        --archive)    archive="${2:?--archive needs a file}"; shift 2 ;;
        --extract-to) extract_to="${2:?--extract-to needs a directory}"; shift 2 ;;
        --output)     output="${2:?--output needs a file}"; shift 2 ;;
        --document)   document="${2:?--document needs a file}"; shift 2 ;;
        --forbidden)  forbidden+=("${2:?--forbidden needs a directory}"); shift 2 ;;
        --no-execute) execute=no; shift ;;
        *) package_die "unknown argument: $1" ;;
    esac
done

[ -n "$platform" ] || package_die 'no --platform'
[ -n "$archive" ] || package_die 'no --archive'
[ -n "$extract_to" ] || package_die 'no --extract-to'
[ -n "$output" ] || package_die 'no --output'
case " ${NATIVE_PLATFORMS[*]} " in
    *" $platform "*) ;;
    *) package_die "unknown platform $platform" ;;
esac
if [ "$execute" = yes ]; then
    [ -n "$document" ] || package_die '--document is needed to cross the Open CASCADE boundary'
    [ -f "$document" ] || package_die "no such document: $document"
fi

# Converted before anything opens them. On Windows these arrive spelled with
# backslashes and a drive letter, and GNU tar reads a colon in a file name as
# host:path.
archive="$(package_posix_path "$archive")"
extract_to="$(package_posix_path "$extract_to")"

triple="$(package_triple_for "$platform")"
native_require_jq

work="$(mktemp -d)"
# shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

: > "$output"
fact() { printf '%s\n' "$*" >> "$output"; }

# ---------------------------------------------------------------------------
# There has to be an archive at all.
#
# This is the defect the whole slice exists to fix, and it is asked first and
# out loud. Before §21A-2b2b1a the relocatable layout existed only as a
# directory in the runner's temporary space: it could be started, and it could
# not be handed to anybody. A missing archive here is not a missing file, it is
# a product that has no delivery.
# ---------------------------------------------------------------------------

if [ ! -f "$archive" ]; then
    echo "$PACKAGE_TOOL: there is no release archive at $archive." >&2
    echo >&2
    echo "  A staged runtime layout is not a delivery. It lives in the temporary" >&2
    echo "  directory of the run that produced it, carries no version in its name," >&2
    echo "  has no archive around it, and is deleted with the runner. Nothing that" >&2
    echo "  exists can be extracted somewhere else and started there, so the" >&2
    echo "  question this gate asks cannot be answered at all." >&2
    echo >&2
    echo "  Produce one with tools/package-release.sh." >&2
    exit 1
fi
fact "package archive-exists=true"

# ---------------------------------------------------------------------------
# Nothing that produced the archive may still be reachable.
# ---------------------------------------------------------------------------

# Asserted, not fixed. Emptying these here would make the check pass for a
# caller that had left them set, and "the package was started with the loader
# pointed at the build tree" is precisely the thing that must not be reportable
# as a clean-environment run.
for variable in LD_LIBRARY_PATH DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH; do
    eval "value=\${${variable}:-}"
    if [ -n "$value" ]; then
        package_die "$variable is set to '$value'; a run with the loader environment still \
pointing somewhere says nothing about whether the package can stand on its own"
    fi
done

[ "${#forbidden[@]}" -gt 0 ] || package_die \
    'no --forbidden directory, so nothing - not even the staging the archive was made from - was taken away'
for directory in "${forbidden[@]}"; do
    if [ -e "$directory" ]; then
        package_die "$directory still exists, so a run that succeeds says nothing about the package"
    fi
done
fact "package forbidden-directories-present=0"

printf '%s\n' "$PATH" | tr ':' '\n' > "$work/path-entries"
for directory in "${forbidden[@]}"; do
    if grep -Fxq "$directory" "$work/path-entries"; then
        package_die "PATH still holds $directory"
    fi
done
if [ "$platform" = windows ]; then
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        for pattern in planegcs.dll 'TK*.dll'; do
            # shellcheck disable=SC2086
            if [ -n "$(find "$entry" -maxdepth 1 -name $pattern -print -quit 2>/dev/null)" ]; then
                package_die "PATH entry $entry holds ${pattern}, so the loader need not use the package"
            fi
        done
    done < "$work/path-entries"
fi
fact "package path-holds-runtime=0"

# ---------------------------------------------------------------------------
# What the archive says it holds, before anything is written to disk.
# ---------------------------------------------------------------------------

tar_bin="$(package_gnu_tar)"
"$tar_bin" -tvf "$archive" > "$work/listing" 2>"$work/listing.err" \
    || { echo "$PACKAGE_TOOL: $archive could not be listed:" >&2
         sed 's/^/  /' "$work/listing.err" >&2; exit 1; }
[ -s "$work/listing" ] || package_die "$archive holds nothing at all"

# The name is the last field, and a name with a space in it would break that.
# Refused rather than parsed around: a release path with a space in it is a
# finding about the layout, not something this should quietly cope with.
sed -n 's/^\([^ ]*\) .* \([^ ]*\)$/\1 \2/p' "$work/listing" > "$work/entries"
if [ "$(wc -l < "$work/entries" | tr -d ' ')" -ne "$(wc -l < "$work/listing" | tr -d ' ')" ]; then
    package_die "$archive holds an entry whose name could not be read; a name with a space in it is a finding"
fi

# Regular files and directories, and nothing else. A symlink in a release is a
# path the archiver resolved on the machine that packed it; a device node is
# not a payload at all.
if awk '$1 !~ /^[-d]/ { print }' "$work/entries" | grep .; then
    package_die "$archive holds an entry that is neither a regular file nor a directory"
fi
fact "package archive-entry-kinds=regular-and-directory-only"

# No absolute path, no parent traversal, no drive letter, no backslash. Each of
# these is a way for an extraction to write outside the directory it was told
# to use, and none of them can be answered after extracting.
while read -r _mode name; do
    case "$name" in
        /*)          package_die "$archive holds the absolute path $name" ;;
        ../* | */../* | */..) package_die "$archive holds the parent traversal $name" ;;
        ..)          package_die "$archive holds a parent traversal" ;;
        ./* | .)     package_die "$archive holds the unnormalised path $name" ;;
        [A-Za-z]:*)  package_die "$archive holds the drive-qualified path $name" ;;
        *\\*)        package_die "$archive holds the backslash path $name" ;;
    esac
done < "$work/entries"
fact "package archive-paths-normalised=true"

# Exactly one top-level directory, and its name carries the version and the
# target. Two roots would extract two things into whatever directory a
# recipient happened to be standing in.
awk '{ print $2 }' "$work/entries" | sed 's|/.*||' | LC_ALL=C sort -u > "$work/roots"
if [ "$(wc -l < "$work/roots" | tr -d ' ')" -ne 1 ]; then
    echo "$PACKAGE_TOOL: $archive has more than one top-level entry:" >&2
    sed 's/^/  /' "$work/roots" >&2
    exit 1
fi
archive_root="$(cat "$work/roots")"
fact "package archive-top-level-roots=1"

# In one order that depends only on the names, so two packings of the same
# bytes cannot differ because a file system handed its entries over differently.
#
# The key is the name with any trailing slash removed and every separator
# replaced by a byte below every printable one. That is what "sorted" means to
# an archiver walking a tree: a directory's own entry comes before its
# children, and a sibling whose name starts with the same letters comes after
# them. Comparing the printed names directly would call `Contents/x` unsorted
# next to `Contents.txt`, because `.` is below `/` and the walk is not.
awk '{ print $2 }' "$work/entries" > "$work/order-actual"
sed 's|/$||' "$work/order-actual" | tr '/' '\001' > "$work/order-keys"
# Sorted on the key alone. Without -k1,1 the tab that joins the two columns
# would take part in the comparison, and a tab is above the separator byte.
paste "$work/order-keys" "$work/order-actual" \
    | LC_ALL=C sort -t "$(printf '\t')" -k1,1 \
    | cut -f2 > "$work/order-expected"
if ! cmp -s "$work/order-actual" "$work/order-expected"; then
    echo "$PACKAGE_TOOL: the entries of $archive are not in normalised order, so two \
packings of the same bytes need not agree:" >&2
    diff "$work/order-expected" "$work/order-actual" | head -10 >&2
    exit 1
fi
fact "package archive-entry-order=normalised"

# One fixed timestamp on every entry. A file system mtime would make the
# archive depend on when the build ran.
"$tar_bin" --utc -tvf "$archive" | awk '{ print $4, $5 }' | LC_ALL=C sort -u > "$work/stamps"
if [ "$(wc -l < "$work/stamps" | tr -d ' ')" -ne 1 ]; then
    echo "$PACKAGE_TOOL: $archive carries more than one entry timestamp, so it was not \
normalised:" >&2
    sed 's/^/  /' "$work/stamps" >&2
    exit 1
fi
fact "package archive-entry-timestamps=1 value=$(tr -d '\n' < "$work/stamps")"

# ---------------------------------------------------------------------------
# Extracted somewhere it has never been.
# ---------------------------------------------------------------------------

if [ -e "$extract_to" ] && [ -n "$(ls -A "$extract_to" 2>/dev/null)" ]; then
    package_die "$extract_to is not empty; extracting over something else would let \
a file that is not in the archive answer for it"
fi
mkdir -p "$extract_to"
"$tar_bin" -C "$extract_to" -xzf "$archive"

root="$extract_to/$archive_root"
[ -d "$root" ] || package_die "extracting $archive produced no $archive_root"
ls -A "$extract_to" > "$work/extracted-top"
if [ "$(wc -l < "$work/extracted-top" | tr -d ' ')" -ne 1 ]; then
    echo "$PACKAGE_TOOL: extracting produced more than one top-level entry:" >&2
    sed 's/^/  /' "$work/extracted-top" >&2
    exit 1
fi

manifest="$root/$(package_manifest_path)"
[ -f "$manifest" ] || package_die "the extracted package carries no manifest at $(package_manifest_path)"

# ---------------------------------------------------------------------------
# What the manifest says it is.
# ---------------------------------------------------------------------------

jq -e . "$manifest" > /dev/null 2>&1 || package_die "$manifest is not JSON"

said_kind="$(jq -r '.kind' "$manifest" | native_strip_cr)"
said_format="$(jq -r '.formatVersion' "$manifest" | native_strip_cr)"
said_target="$(jq -r '.target' "$manifest" | native_strip_cr)"
said_root="$(jq -r '.root' "$manifest" | native_strip_cr)"
said_archive="$(jq -r '.archive' "$manifest" | native_strip_cr)"
said_version="$(jq -r '.productVersion' "$manifest" | native_strip_cr)"
said_revision="$(jq -r '.sourceRevision' "$manifest" | native_strip_cr)"

[ "$said_kind" = "$PACKAGE_KIND" ] \
    || package_die "the manifest calls itself '$said_kind' rather than $PACKAGE_KIND"
[ "$said_format" = "$PACKAGE_FORMAT" ] \
    || package_die "the manifest is format $said_format and this gate reads $PACKAGE_FORMAT"
# Which commit the packaged binaries were built from. Checked for shape and
# reported; which revision is the right one is not a question a single package
# can answer, and tools/check-release-set.sh asks it of all three at once.
package_valid_revision "$said_revision" \
    || package_die "the manifest says it was built from '$said_revision', which is not a \
full lower-case commit object name"
[ "$said_target" = "$triple" ] \
    || package_die "the manifest is for $said_target and this is the $platform package, which is $triple"
[ "$said_root" = "$archive_root" ] \
    || package_die "the manifest calls its root '$said_root' and the archive's root is '$archive_root'"
[ "$said_archive" = "$(basename "$archive")" ] \
    || package_die "the manifest names the archive '$said_archive' and this file is '$(basename "$archive")'"

# The version and the target are in the name, so a recipient with two of these
# in one directory can tell them apart without opening either.
expected_root="$(package_root_for "$said_version" "$triple")"
[ "$archive_root" = "$expected_root" ] \
    || package_die "the top-level directory is '$archive_root' and version $said_version of $triple is '$expected_root'"
[ "$(basename "$archive")" = "$(package_archive_for "$said_version" "$triple")" ] \
    || package_die "the archive is not named for version $said_version of $triple"
fact "package version=$said_version target=$triple root=$archive_root"
fact "package source-revision=$said_revision"

# The product version is the inventory's, not the manifest's own idea of one.
inventory_version="$(jq -r '.productVersion' "$NATIVE_INVENTORY" | native_strip_cr)"
[ "$said_version" = "$inventory_version" ] \
    || package_die "the manifest says version $said_version and $NATIVE_INVENTORY says $inventory_version"

# ---------------------------------------------------------------------------
# The one file that has no digest, and why.
# ---------------------------------------------------------------------------
#
# A manifest cannot carry the digest of its own bytes: writing the digest
# changes them. That is the only exception in the document, so it is written
# down in the document, and this gate refuses any other file that tries to use
# it.

self_path="$(jq -r '.selfDescription.path' "$manifest" | native_strip_cr)"
self_hashed="$(jq -r '.selfDescription.hashed' "$manifest" | native_strip_cr)"
[ "$self_path" = "$(package_manifest_path)" ] \
    || package_die "the manifest exempts '$self_path' from being hashed and it is itself '$(package_manifest_path)'"
[ "$self_hashed" = false ] \
    || package_die 'the manifest claims to carry the digest of its own bytes, which is impossible'
jq -e '.selfDescription.note | type == "string" and length > 0' "$manifest" > /dev/null \
    || package_die 'the manifest does not say why it is the one file with no digest'
fact "package manifest-self-hash=excluded-and-declared"

# ---------------------------------------------------------------------------
# Every file that came out, re-hashed.
# ---------------------------------------------------------------------------
#
# Against the bytes on disk rather than against the bytes the manifest was
# written from. A manifest that hashed the build tree, a manifest written
# before the load commands were rewritten, a file the archiver truncated and a
# file that lost its executable bit all pass a comparison the manifest makes
# with itself and fail this one.

jq -r '
    (.runtimeFiles[]    | ["runtime", .path, .sha256, (.size|tostring), (.executable|tostring), .owner, .ownerKind]),
    (.packageMetadata[] | ["metadata", .path, .sha256, (.size|tostring), (.executable|tostring), .owner, .ownerKind])
    | @tsv' "$manifest" | native_strip_cr | LC_ALL=C sort > "$work/claimed"
[ -s "$work/claimed" ] || package_die 'the manifest describes no file at all'

# Refused before anything is compared: a path claimed twice is a file with two
# owners even when both entries agree about its bytes, and a comparison of
# sorted sets would not notice.
cut -f2 "$work/claimed" | LC_ALL=C sort > "$work/claimed-paths"
if cut -f2 "$work/claimed" | LC_ALL=C sort | uniq -d | grep .; then
    package_die 'the manifest claims a payload file more than once, so some file has two owners'
fi
if grep -Fxq "$self_path" "$work/claimed-paths"; then
    package_die "the manifest lists $self_path as a payload file as well as exempting it from being hashed"
fi

# And that each one really has an owner. An empty owner is the shape of a
# lookup that found nothing and carried on.
if awk -F'\t' '$6 == "" || $7 == ""' "$work/claimed" | grep .; then
    package_die 'the manifest carries a payload file with no owner'
fi
if awk -F'\t' '$7 != "component" && $7 != "product-root" && $7 != "product-sbom-root"' \
        "$work/claimed" | grep .; then
    package_die 'the manifest carries a payload file whose owner is of no known kind'
fi
fact "package manifest-files=$(wc -l < "$work/claimed" | tr -d ' ') owners=one-each"

# What is actually there.
# Made relative by parameter expansion rather than by sed, and walked from an
# absolute path rather than from a `cd`. RUNNER_TEMP on Windows is spelled with
# backslashes, and a backslash in a sed pattern is an escape rather than a
# separator; tools/check-native-inventory.sh already paid for that once.
: > "$work/on-disk-paths"
while IFS= read -r found; do
    printf '%s\n' "${found#"$root"/}" >> "$work/on-disk-paths"
done < <(find "$root" -type f)
LC_ALL=C sort -o "$work/on-disk-paths" "$work/on-disk-paths"

: > "$work/on-disk-odd"
while IFS= read -r found; do
    printf '%s\n' "${found#"$root"/}" >> "$work/on-disk-odd"
done < <(find "$root" ! -type f ! -type d)
if [ -s "$work/on-disk-odd" ]; then
    echo "$PACKAGE_TOOL: the extracted package holds something that is neither a file nor a directory:" >&2
    sed 's/^/  /' "$work/on-disk-odd" >&2
    exit 1
fi

# The manifest itself is the one file on disk that is not claimed.
grep -Fxv "$self_path" "$work/on-disk-paths" > "$work/on-disk-payload" || true

if ! diff -u "$work/claimed-paths" "$work/on-disk-payload" > "$work/set-diff"; then
    echo "$PACKAGE_TOOL: the manifest and the extracted package do not describe the same files:" >&2
    grep '^-[^-]' "$work/set-diff" | sed 's/^-/  only in the manifest: /' >&2 || true
    grep '^+[^+]' "$work/set-diff" | sed 's/^+/  only in the archive:  /' >&2 || true
    exit 1
fi
fact "package manifest-covers-archive=exactly"

: > "$work/mismatches"
while IFS="$(printf '\t')" read -r section path claimed_digest claimed_size claimed_exec _owner _kind; do
    file="$root/$path"
    actual_digest="$(package_sha256 "$file")"
    actual_size="$(package_size "$file")"
    actual_exec="$(package_is_executable "$file")"
    [ "$actual_digest" = "$claimed_digest" ] \
        || printf '%s\tdigest\t%s\t%s\n' "$path" "$claimed_digest" "$actual_digest" >> "$work/mismatches"
    [ "$actual_size" = "$claimed_size" ] \
        || printf '%s\tsize\t%s\t%s\n' "$path" "$claimed_size" "$actual_size" >> "$work/mismatches"
    # The executable bit is a property the archive had to carry rather than one
    # a recipient should have to restore. A gate that chmod'ed after extracting
    # would be hiding exactly the failure it exists to find.
    [ "$actual_exec" = "$claimed_exec" ] \
        || printf '%s\texecutable\t%s\t%s\n' "$path" "$claimed_exec" "$actual_exec" >> "$work/mismatches"
    printf '%s\t%s\n' "$section" "$path" >> "$work/sections"
done < "$work/claimed"

if [ -s "$work/mismatches" ]; then
    echo "$PACKAGE_TOOL: the extracted bytes are not the ones the manifest describes:" >&2
    awk -F'\t' '{ printf "  %s: %s says %s and the extracted file is %s\n", $1, $2, $3, $4 }' \
        "$work/mismatches" >&2
    exit 1
fi
fact "package manifest-verified-against-extracted-bytes=true"

runtime_count="$(awk -F'\t' '$1 == "runtime"' "$work/sections" | wc -l | tr -d ' ')"
metadata_count="$(awk -F'\t' '$1 == "metadata"' "$work/sections" | wc -l | tr -d ' ')"
runtime_bytes="$(awk -F'\t' '$1 == "runtime" { n += $4 } END { print n + 0 }' "$work/claimed")"
[ "$runtime_count" -gt 0 ] || package_die 'the manifest separates nothing into runtime files'
[ "$metadata_count" -gt 0 ] || package_die 'the manifest separates nothing into package metadata'

# The two halves are declared, and they really are disjoint. A runtime file
# filed as metadata would be carried without being described as something the
# product needs in order to run.
if awk -F'\t' '$1 == "runtime" && $2 ~ /^package\//' "$work/sections" | grep .; then
    package_die "a runtime file is under $PACKAGE_METADATA_DIR/, where only package metadata belongs"
fi
if awk -F'\t' '$1 == "metadata" && $2 !~ /^package\//' "$work/sections" | grep .; then
    package_die "package metadata is outside $PACKAGE_METADATA_DIR/"
fi
fact "package runtime-files=${runtime_count} runtime-bytes=${runtime_bytes} metadata-files=${metadata_count}"

# ---------------------------------------------------------------------------
# The product SBOM travels with the product, and it is the same document.
# ---------------------------------------------------------------------------

sbom_in_package="$root/$(package_sbom_path_for "$triple")"
[ -f "$sbom_in_package" ] || package_die \
    "the extracted package carries no product SBOM at $(package_sbom_path_for "$triple")"

committed_sbom="$(product_output_for "$triple")"
[ -f "$committed_sbom" ] || package_die "no committed product SBOM at $committed_sbom"
if ! cmp -s "$committed_sbom" "$sbom_in_package"; then
    package_die "the product SBOM inside the package is not byte for byte $committed_sbom"
fi

sbom_target="$(jq -r '.metadata.properties[] | select(.name == "ferritecad:sbom:target") | .value' \
    "$sbom_in_package" | native_strip_cr)"
[ "$sbom_target" = "$triple" ] \
    || package_die "the packaged SBOM is for $sbom_target and this is the $triple package"
sbom_complete="$(jq -r '.metadata.properties[] | select(.name == "ferritecad:sbom:complete") | .value' \
    "$sbom_in_package" | native_strip_cr)"
[ "$sbom_complete" = true ] \
    || package_die 'the packaged SBOM does not claim to describe the whole product'

sbom_digest="$(package_sha256 "$sbom_in_package")"
claimed_sbom_digest="$(jq -r '.productSbom.sha256' "$manifest" | native_strip_cr)"
claimed_sbom_path="$(jq -r '.productSbom.path' "$manifest" | native_strip_cr)"
[ "$claimed_sbom_path" = "$(package_sbom_path_for "$triple")" ] \
    || package_die "the manifest points at '$claimed_sbom_path' for the SBOM of $triple"
[ "$claimed_sbom_digest" = "$sbom_digest" ] \
    || package_die "the manifest says the SBOM hashes to $claimed_sbom_digest and it hashes to $sbom_digest"
fact "package product-sbom=byte-identical target=${sbom_target} sha256=${sbom_digest}"

# ---------------------------------------------------------------------------
# What must not be in a runtime archive.
# ---------------------------------------------------------------------------

# The Windows import library is read by the linker on behalf of one crate and
# by nothing at run time. Shipping it would put a build input in a delivery.
if find "$root" -name "$NATIVE_IMPORT_LIBRARY" -print -quit | grep .; then
    package_die "the package carries $NATIVE_IMPORT_LIBRARY, which is a build input and never loaded"
fi
fact "package import-library-present=0"

# Nor may anything in it name a directory that no longer exists. A file that
# does was written against the build tree and runs only on the machine that
# built it.
: > "$work/leaks"
while IFS= read -r relative; do
    for directory in "${forbidden[@]}"; do
        if LC_ALL=C grep -aqF "$directory" "$root/$relative"; then
            printf '%s\t%s\n' "$relative" "$directory" >> "$work/leaks"
        fi
    done
done < "$work/on-disk-paths"
if [ -s "$work/leaks" ]; then
    echo "$PACKAGE_TOOL: extracted files still name directories that are gone:" >&2
    sed 's/^/  /' "$work/leaks" >&2
    exit 1
fi
fact "package extracted-files-naming-build-tree=0"

# And no build tree, cache, source tree or workflow evidence came along.
if awk -F'\t' '{ print $2 }' "$work/claimed" \
        | grep -Ei '(^|/)(target|build|\.git|node_modules|vendor|src|crates|\.cargo)(/|$)|\.(rs|cpp|h|hxx|o|obj|pdb|d|rlib|rmeta)$|closure-.*\.txt|staged-facts|native-evidence' \
        | grep .; then
    package_die 'the package carries something that is a build tree, a source tree or workflow evidence'
fi
fact "package build-and-source-trees-present=0"

# ---------------------------------------------------------------------------
# Which extracted file is what, taken from the owners rather than from a rule.
# ---------------------------------------------------------------------------
#
# tools/stage-runtime-layout.sh decides where a shipped file goes and
# sbom/native/native-assets-inventory.json decides which component owns it. A
# second opinion here - a directory name, a filename pattern, a list of toolkit
# names - would be a second layout that could go on agreeing with itself while
# the real one moved.

root_loading_planegcs="$(jq -r '.productRoots[] | select(.loads | index("planegcs")) | .binary' \
    "$NATIVE_INVENTORY" | native_strip_cr)"
root_not_loading_planegcs="$(jq -r '.productRoots[] | select((.loads | index("planegcs")) | not) | .binary' \
    "$NATIVE_INVENTORY" | native_strip_cr)"
[ "$(printf '%s\n' "$root_loading_planegcs" | wc -l | tr -d ' ')" -eq 1 ] \
    || package_die 'the inventory names more than one product root that loads the solver'
[ "$(printf '%s\n' "$root_not_loading_planegcs" | wc -l | tr -d ' ')" -eq 1 ] \
    || package_die 'the inventory names more than one product root that does not load the solver'

# The component identity behind a product root's short key, taken the way the
# inventory builds it. A key with no component is a failure rather than a
# smaller answer.
component_for_key() { # key
    local id
    id="$(jq -r --arg prefix "native+$1@" \
        '.components[] | select(.role == "runtime-native") | select(.id | startswith($prefix)) | .id' \
        "$NATIVE_INVENTORY" | native_strip_cr)"
    [ "$(printf '%s\n' "$id" | wc -l | tr -d ' ')" -eq 1 ] && [ -n "$id" ] \
        || package_die "the inventory has no single runtime-native component for '$1'"
    printf '%s\n' "$id"
}
planegcs_owner="$(component_for_key planegcs)"
occt_owner="$(component_for_key occt)"

one_file_owned_by() { # owner description
    local owner="$1" what="$2" found
    found="$(awk -F'\t' -v o="$owner" '$1 == "runtime" && $6 == o { print $2 }' "$work/claimed")"
    [ -n "$found" ] || package_die "the package carries no $what (nothing is owned by $owner)"
    [ "$(printf '%s\n' "$found" | wc -l | tr -d ' ')" -eq 1 ] \
        || package_die "the package carries more than one $what"
    printf '%s\n' "$found"
}

viewer="$root/$(one_file_owned_by "$root_loading_planegcs" 'shipped application')"
cli="$root/$(one_file_owned_by "$root_not_loading_planegcs" 'shipped command line tool')"
planegcs="$root/$(one_file_owned_by "$planegcs_owner" 'sketch solver library')"

awk -F'\t' -v o="$occt_owner" '$1 == "runtime" && $6 == o { print $2 }' "$work/claimed" \
    | LC_ALL=C sort > "$work/toolkit-paths"
[ -s "$work/toolkit-paths" ] || package_die "the package carries no Open CASCADE toolkit"
: > "$work/toolkits"
while IFS= read -r toolkit; do
    printf '%s\n' "$root/$toolkit" >> "$work/toolkits"
done < "$work/toolkit-paths"
fact "package toolkits=$(wc -l < "$work/toolkits" | tr -d ' ')"

for binary in "$viewer" "$cli"; do
    [ -x "$binary" ] || package_die "$binary came out of the archive without its executable bit"
done
fact "package executables-extracted-runnable=true"

if [ "$execute" = no ]; then
    fact "package execution=skipped"
    echo "--- facts ---"
    LC_ALL=C sort -o "$output" "$output"
    cat "$output"
    echo
    echo "$PACKAGE_TOOL: the archive was measured but not run; a run needs the real product"
    exit 0
fi

# ---------------------------------------------------------------------------
# Running what came out of the archive.
# ---------------------------------------------------------------------------

runtime_probe_require_inspector "$platform"

set +e
runtime_probe_run_with_deadline 60 "$work/solver-info.txt" "$viewer" --solver-info
viewer_status=$?
set -e
echo "--- extracted ferritecad-viewer --solver-info (exit ${viewer_status}) ---"
cat "$work/solver-info.txt"

if [ "$viewer_status" -ne 0 ]; then
    echo "$PACKAGE_TOOL: the extracted application exited ${viewer_status}" >&2
    echo "an archive that does not start once unpacked is the finding, not a detail" >&2
    exit 1
fi
grep -qx 'sketch solver: available' "$work/solver-info.txt" \
    || package_die 'the extracted application did not say it has a solver'
if grep -qi 'skip' "$work/solver-info.txt"; then
    package_die 'the extracted application skipped something instead of answering'
fi

# The words the extracted library gave, compared against the copy that is
# actually beside the executable inside the extracted package.
named="$(sed -n 's/^provenance: //p' "$work/solver-info.txt" | head -1)"
[ -n "$named" ] || package_die 'the extracted application named no provenance'
if ! LC_ALL=C grep -aqF "$named" "$planegcs"; then
    package_die "the extracted application said '${named}', and the planegcs inside the package \
does not contain those words, so what it printed did not come from the library beside it"
fi
fact "package extracted viewer solver-info exit=0 answer=available"
fact "package extracted viewer provenance-from-extracted-library=true"

# The other half: Open CASCADE, through the command line tool that already
# crosses that boundary. No new command is added to measure this.
cp "$document" "$work/document.fcad"
before="$(cksum < "$work/document.fcad")"

set +e
runtime_probe_run_with_deadline 300 "$work/rebuild.txt" "$cli" rebuild "$work/document.fcad" --cold
cli_status=$?
set -e
echo "--- extracted ferritecad rebuild --cold (exit ${cli_status}) ---"
cat "$work/rebuild.txt"

[ "$cli_status" -eq 0 ] || package_die "the extracted command line tool exited ${cli_status}"
if grep -q 'no Open CASCADE' "$work/rebuild.txt"; then
    package_die 'the extracted build has no kernel, so the rebuild proved nothing about Open CASCADE'
fi
kernel="$(sed -n 's/^ *kernel //p' "$work/rebuild.txt" | head -1)"
case "$kernel" in
    occt\ *) ;;
    *) package_die "the rebuild named kernel '${kernel}', which is not Open CASCADE" ;;
esac
grep -q 'shape.* built' "$work/rebuild.txt" \
    || package_die 'the rebuild built no shape, so no Open CASCADE operation actually ran'

after="$(cksum < "$work/document.fcad")"
[ "$before" = "$after" ] || package_die 'the rebuild changed the document it was diagnosing'
if [ -e "$work/document.fcad-cache" ]; then
    package_die 'the rebuild wrote a cache sidecar beside the document'
fi
fact "package extracted cli rebuild-cold exit=0 kernel=${kernel%% *}"
fact "package extracted cli document-unchanged=true"

# ---------------------------------------------------------------------------
# And that the extracted package is really what answered.
# ---------------------------------------------------------------------------

runtime_probe_hidden_must_stop "$work/hidden.txt" "$planegcs" \
    'the extracted application' "$viewer" --solver-info
fact "package extracted viewer hidden-planegcs started=false"

runtime_probe_choose_toolkit "$platform" "$cli" "$work/toolkits"
echo "the Open CASCADE toolkit under test is $(basename "$runtime_probe_toolkit") (${runtime_probe_reach})"
runtime_probe_hidden_must_stop "$work/hidden.txt" "$runtime_probe_toolkit" \
    'the extracted command line tool' "$cli" rebuild "$work/document.fcad" --cold
fact "package extracted cli hidden-occt-toolkit started=false reach=${runtime_probe_reach}"

# Restored, and passing again.
set +e
runtime_probe_run_with_deadline 60 "$work/again-viewer.txt" "$viewer" --solver-info
again_viewer=$?
runtime_probe_run_with_deadline 300 "$work/again-cli.txt" "$cli" rebuild "$work/document.fcad" --cold
again_cli=$?
set -e
[ "$again_viewer" -eq 0 ] \
    || package_die "with the libraries back the extracted application still exited ${again_viewer}"
[ "$again_cli" -eq 0 ] \
    || package_die "with the libraries back the extracted command line tool still exited ${again_cli}"
fact "package extracted both restored=true"

# Putting a library back must not have left the package different from the one
# the manifest describes. A run that died between the two would otherwise be
# reported as a package that verified.
: > "$work/after-run"
while IFS="$(printf '\t')" read -r _section path claimed_digest _size _exec _owner _kind; do
    [ "$(package_sha256 "$root/$path")" = "$claimed_digest" ] \
        || printf '%s\n' "$path" >> "$work/after-run"
done < "$work/claimed"
if [ -s "$work/after-run" ]; then
    echo "$PACKAGE_TOOL: running the package changed files inside it:" >&2
    sed 's/^/  /' "$work/after-run" >&2
    exit 1
fi
if find "$root" -name '*.hidden' -print -quit | grep .; then
    package_die 'a library taken away during this run was never put back'
fi
fact "package unchanged-by-running=true"
fact "package execution=performed"

echo "--- facts ---"
LC_ALL=C sort -o "$output" "$output"
cat "$output"
