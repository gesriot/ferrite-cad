#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Turns a staged runtime layout into one versioned release archive.
#
# It is handed a staging directory that tools/stage-runtime-layout.sh produced
# and that tools/check-staged-layout.sh has already started with the build
# trees taken away, and it does exactly three things to it: it puts the layout
# under one versioned directory without moving anything inside it, it puts the
# target's product SBOM and one manifest of this build beside it, and it packs
# the result deterministically.
#
# What it does not do, and must never start doing. It does not walk a runtime
# closure, look for a library, rewrite a run path or an install name, link or
# build anything, decide which Open CASCADE toolkit or which planegcs the
# product carries, or work out a component's identity. Every one of those has
# an owner already - tools/stage-runtime-layout.sh for the layout,
# sbom/native/native-assets-inventory.json for the components and their staged
# files, sbom/product/ for what the product is made of - and a second answer
# here would be a second product that could go on agreeing with itself.
#
# What the manifest is for. The SBOM says what the product is built from;
# component identities are pinned sources and are the same on every host. The
# manifest says which files this build produced and what they hash to, and
# those digests are host-dependent by measurement: the same pinned sources give
# different bytes on a developer machine and on a runner. So the two are
# separate documents. The SBOM travels inside the archive byte for byte
# unchanged - no bom-ref, purl, hash, property or dependency edge of it is
# touched - and the manifest sits beside it.
#
# This is not an installer, not a release, not a signature and not a
# notarisation. The ad-hoc signature the macOS staging applies is what makes a
# rewritten Mach-O startable at all; it is a technical condition of running and
# says nothing about distribution.
#
# Which commit this is. The revision is handed in and never worked out here.
# A packager that ran `git rev-parse HEAD` would be describing the tree it
# happened to be standing in rather than the tree the binaries were built from,
# and those are the same thing only until somebody packs an archive in a
# checkout that has moved on. The workflow passes GITHUB_SHA, which is the
# commit the job checked out and built; the local gates pass a fixed made-up
# revision, and their fixtures are made-up bytes.
#
# Usage:
#   tools/package-release.sh --platform linux|macos|windows \
#       --staging DIR --output-dir DIR --source-revision SHA

set -euo pipefail

PACKAGE_TOOL='package-release'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/package/lib.sh
. tools/package/lib.sh

platform=''
staging=''
output_dir=''
source_revision=''

while [ $# -gt 0 ]; do
    case "$1" in
        --platform)   platform="${2:?--platform needs a name}"; shift 2 ;;
        --staging)    staging="${2:?--staging needs a directory}"; shift 2 ;;
        --output-dir) output_dir="${2:?--output-dir needs a directory}"; shift 2 ;;
        --source-revision) source_revision="${2:?--source-revision needs a commit}"; shift 2 ;;
        *) package_die "unknown argument: $1" ;;
    esac
done

[ -n "$platform" ] || package_die 'no --platform'
[ -n "$staging" ] || package_die 'no --staging'
[ -n "$output_dir" ] || package_die 'no --output-dir'
[ -n "$source_revision" ] || package_die \
    'no --source-revision, so nothing in the package would say which commit it was built from'
package_valid_revision "$source_revision" || package_die \
    "--source-revision '$source_revision' is not a full lower-case commit object name"
[ -d "$staging" ] || package_die "no such staging directory: $staging"
# Spelled the way this shell's own tools can read it. RUNNER_TEMP on Windows is
# D:\a\_temp: a backslash is an escape to sed and to a shell pattern, and a
# colon makes GNU tar read a name as host:path.
staging="$(package_posix_path "$staging")"
output_dir="$(package_posix_path "$output_dir")"
case " ${NATIVE_PLATFORMS[*]} " in
    *" $platform "*) ;;
    *) package_die "unknown platform $platform" ;;
esac

native_require_jq
triple="$(package_triple_for "$platform")"

work="$(mktemp -d)"
# shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

# ---------------------------------------------------------------------------
# The authoritative inputs, each from the one file that owns it.
# ---------------------------------------------------------------------------

[ -f "$NATIVE_INVENTORY" ] || package_die "no native inventory at $NATIVE_INVENTORY"
version="$(jq -r '.productVersion' "$NATIVE_INVENTORY" | native_strip_cr)"
[ -n "$version" ] && [ "$version" != null ] \
    || package_die "$NATIVE_INVENTORY does not say what version the product is"

sbom="$(product_output_for "$triple")"
[ -f "$sbom" ] || package_die "no product SBOM for $triple at $sbom"
sbom_target="$(jq -r '.metadata.properties[] | select(.name == "ferritecad:sbom:target") | .value' \
    "$sbom" | native_strip_cr)"
[ "$sbom_target" = "$triple" ] \
    || package_die "$sbom says its target is '$sbom_target' and this is the $triple package"
sbom_complete="$(jq -r '.metadata.properties[] | select(.name == "ferritecad:sbom:complete") | .value' \
    "$sbom" | native_strip_cr)"
[ "$sbom_complete" = true ] \
    || package_die "$sbom does not claim to describe the whole product"
sbom_version="$(jq -r '.metadata.component.version' "$sbom" | native_strip_cr)"
[ "$sbom_version" = "$version" ] \
    || package_die "$sbom describes version $sbom_version and the inventory says $version"

root="$(package_root_for "$version" "$triple")"
archive_name="$(package_archive_for "$version" "$triple")"

# ---------------------------------------------------------------------------
# What is in the staging directory, and who owns each of it.
#
# The ownership map is read, never derived. A packager that decided for itself
# that a file called libTK-something belongs to Open CASCADE would be a second
# opinion about the boundary the inventory exists to fix, and it would keep
# agreeing with itself while the inventory moved.
# ---------------------------------------------------------------------------

# Made relative by parameter expansion rather than by sed, for the reason
# tools/check-native-inventory.sh records: a backslash in a sed pattern is an
# escape rather than a separator.
: > "$work/irregular"
while IFS= read -r found; do
    printf '%s\n' "${found#"$staging"/}" >> "$work/irregular"
done < <(find "$staging" ! -type d ! -type f)
if [ -s "$work/irregular" ]; then
    echo "$PACKAGE_TOOL: the staging directory holds something that is not a regular file:" >&2
    sed 's/^/  /' "$work/irregular" >&2
    echo "A symlink or a device node in a release is a path resolved on the machine" >&2
    echo "that packed it. tools/stage-runtime-layout.sh copies with -L for that reason." >&2
    exit 1
fi

: > "$work/staged"
while IFS= read -r found; do
    printf '%s\n' "${found#"$staging"/}" >> "$work/staged"
done < <(find "$staging" -type f)
LC_ALL=C sort -o "$work/staged" "$work/staged"
[ -s "$work/staged" ] || package_die "the staging directory $staging holds no file at all"

# The import library is produced by the planegcs build and read by the Windows
# linker on behalf of one crate. It is never loaded, so it is never delivered.
if awk -F/ -v name="$NATIVE_IMPORT_LIBRARY" '$NF == name' "$work/staged" | grep .; then
    package_die "the staging directory carries $NATIVE_IMPORT_LIBRARY, which is a build input and not a runtime file"
fi

jq -r --arg t "$triple" \
    '.targets[] | select(.triple == $t) | .stagedFiles[] | [.path, .owner, .ownerKind] | @tsv' \
    "$NATIVE_INVENTORY" | native_strip_cr | LC_ALL=C sort > "$work/owned"
[ -s "$work/owned" ] || package_die "$NATIVE_INVENTORY stages nothing for $triple"

cut -f1 "$work/owned" > "$work/owned-paths"
if uniq -d "$work/owned-paths" | grep .; then
    package_die "$NATIVE_INVENTORY gives some staged file of $triple more than one owner"
fi

if ! diff -u "$work/owned-paths" "$work/staged" > "$work/set-diff"; then
    echo "$PACKAGE_TOOL: the staging directory and the inventory do not describe the same files:" >&2
    grep '^-[^-]' "$work/set-diff" \
        | sed 's/^-/  the inventory promises and staging lacks: /' >&2 || true
    grep '^+[^+]' "$work/set-diff" \
        | sed 's/^+/  staged with no owner:                     /' >&2 || true
    echo >&2
    echo "Nothing is packed until every staged file has exactly one owner. A file with" >&2
    echo "none would be shipped without anything saying what it is." >&2
    exit 1
fi

# Nothing staged may already live where package metadata goes, or the two would
# be indistinguishable inside the archive.
if grep -q "^$PACKAGE_METADATA_DIR/" "$work/staged"; then
    package_die "the staging directory already holds $PACKAGE_METADATA_DIR/, which is where package metadata goes"
fi

# Normalised relative paths, asserted rather than assumed. A `..` or an
# absolute name here would become one in the archive.
while IFS= read -r path; do
    case "$path" in
        /* | */../* | ../* | */.. | .. | *\\*)
            package_die "the staging directory holds the unnormalised path '$path'" ;;
    esac
done < "$work/staged"

# ---------------------------------------------------------------------------
# The bom-ref each owner has in the product SBOM.
#
# Read out of the two documents rather than constructed, so a manifest can
# never name a component the SBOM travelling beside it does not have. Nothing
# in the SBOM is edited: this only points at it.
# ---------------------------------------------------------------------------

jq -r '.components[].["bom-ref"], .metadata.component.["bom-ref"]' "$sbom" \
    | native_strip_cr | LC_ALL=C sort -u > "$work/sbom-refs"

jq -r '.productRoots[] | [.binary, .rustFragmentRef] | @tsv' "$NATIVE_INVENTORY" \
    | native_strip_cr | LC_ALL=C sort > "$work/roots"

ref_for_owner() { # owner ownerKind
    local owner="$1" kind="$2" ref=''
    case "$kind" in
        component)
            ref="$owner" ;;
        product-root)
            ref="$(awk -F'\t' -v b="$owner" '$1 == b { print $2 }' "$work/roots")" ;;
        *) package_die "the inventory gives $owner an owner kind of '$kind'" ;;
    esac
    [ -n "$ref" ] || package_die "nothing in $NATIVE_INVENTORY says which component '$owner' is"
    grep -Fxq "$ref" "$work/sbom-refs" \
        || package_die "'$owner' resolves to $ref, which $sbom does not carry"
    printf '%s\n' "$ref"
}

# ---------------------------------------------------------------------------
# The package tree.
# ---------------------------------------------------------------------------

mkdir -p "$output_dir"
build="$work/build"
mkdir -p "$build/$root"

# Copied one measured file at a time rather than by copying the directory,
# because the executable bit is the property the archive has to carry and
# leaving it to whatever `cp` does with a mode and a umask is leaving it to the
# host. Read from the staging, written on the copy, and re-measured after the
# archive is extracted.
while IFS= read -r path; do
    mkdir -p "$build/$root/$(dirname "$path")"
    cp "$staging/$path" "$build/$root/$path"
    if [ -x "$staging/$path" ]; then
        chmod 755 "$build/$root/$path"
    else
        chmod 644 "$build/$root/$path"
    fi
    if ! cmp -s "$staging/$path" "$build/$root/$path"; then
        package_die "copying $path into the package changed its bytes"
    fi
done < "$work/staged"

metadata="$build/$root/$PACKAGE_METADATA_DIR"
mkdir -p "$metadata"
sbom_in_package="$build/$root/$(package_sbom_path_for "$triple")"
cp "$sbom" "$sbom_in_package"
chmod 644 "$sbom_in_package"
cmp -s "$sbom" "$sbom_in_package" \
    || package_die 'the product SBOM changed on its way into the package'

# ---------------------------------------------------------------------------
# The manifest.
#
# Digests of the files of this build, attached to the files rather than to any
# component. Two clean builds of one target on one host reproduce them; the
# same pinned sources on another host do not, which is measured rather than
# assumed and is exactly why they are here and not in the SBOM.
# ---------------------------------------------------------------------------

# The digest and the size come from the file that is going into the archive.
# The executable bit comes from the staged original, so that a packager which
# lost it on the way in is a manifest that disagrees with the archive rather
# than a manifest that agreed with its own mistake.
entry_for() { # relative-path owner ownerKind
    local path="$1" owner="$2" kind="$3" file="$build/$root/$1"
    jq -n --arg path "$path" \
          --arg sha256 "$(package_sha256 "$file")" \
          --argjson size "$(package_size "$file")" \
          --argjson executable "$(package_is_executable "$staging/$path")" \
          --arg owner "$owner" \
          --arg ownerKind "$kind" \
          --arg ownerRef "$(ref_for_owner "$owner" "$kind")" \
          '{$path, $sha256, $size, $executable, $owner, $ownerKind, $ownerRef}'
}

: > "$work/runtime-entries"
while IFS="$(printf '\t')" read -r path owner kind; do
    entry_for "$path" "$owner" "$kind" >> "$work/runtime-entries"
done < "$work/owned"

# The SBOM belongs to the product as a whole rather than to either shipped
# binary, so its owner is the SBOM's own root and that ref is checked to exist
# in the document it points into.
sbom_root_ref="$(jq -r '.metadata.component.["bom-ref"]' "$sbom" | native_strip_cr)"
grep -Fxq "$sbom_root_ref" "$work/sbom-refs" \
    || package_die "$sbom has no root component to own itself"
jq -n --arg path "$(package_sbom_path_for "$triple")" \
      --arg sha256 "$(package_sha256 "$sbom_in_package")" \
      --argjson size "$(package_size "$sbom_in_package")" \
      --argjson executable "$(package_is_executable "$sbom_in_package")" \
      --arg owner "$sbom_root_ref" \
      --arg ownerKind 'product-sbom-root' \
      --arg ownerRef "$sbom_root_ref" \
      '{$path, $sha256, $size, $executable, $owner, $ownerKind, $ownerRef}' \
      > "$work/metadata-entries"

jq -s -S \
    --arg kind "$PACKAGE_KIND" \
    --argjson formatVersion "$PACKAGE_FORMAT" \
    --arg productVersion "$version" \
    --arg sourceRevision "$source_revision" \
    --arg target "$triple" \
    --arg root "$root" \
    --arg archive "$archive_name" \
    --arg manifestPath "$(package_manifest_path)" \
    --arg sbomPath "$(package_sbom_path_for "$triple")" \
    --arg sbomSha256 "$(package_sha256 "$sbom_in_package")" \
    --arg tarFormat "$PACKAGE_TAR_FORMAT" \
    --arg gzipFlags "$PACKAGE_GZIP_FLAGS" \
    --argjson mtimeEpoch "$PACKAGE_MTIME_EPOCH" \
    --slurpfile runtime "$work/runtime-entries" \
    --slurpfile metadata "$work/metadata-entries" \
    '{
        kind: $kind,
        formatVersion: $formatVersion,
        productVersion: $productVersion,
        sourceRevision: $sourceRevision,
        target: $target,
        root: $root,
        archive: $archive,
        productSbom: { path: $sbomPath, sha256: $sbomSha256 },
        runtimeFiles: ($runtime | sort_by(.path)),
        packageMetadata: ($metadata | sort_by(.path)),
        selfDescription: {
            path: $manifestPath,
            hashed: false,
            note: "A manifest cannot carry the digest of its own bytes: writing the digest would change them. This is the only file in the package with no digest, and it is named here so that a checker can require exactly one exception rather than tolerate any."
        },
        totals: {
            runtimeFiles: ($runtime | length),
            runtimeBytes: ([$runtime[].size] | add // 0),
            packageMetadataFiles: ($metadata | length),
            packageMetadataBytes: ([$metadata[].size] | add // 0)
        },
        archiveFormat: {
            tar: ("GNU tar --format=" + $tarFormat),
            compression: ("gzip " + $gzipFlags),
            entryOrder: "by name, C locale",
            entryMtimeEpoch: $mtimeEpoch,
            entryOwner: "0:0, numeric"
        }
     }' /dev/null \
    | native_strip_cr > "$build/$root/$(package_manifest_path)"
chmod 644 "$build/$root/$(package_manifest_path)"

jq -e . "$build/$root/$(package_manifest_path)" > /dev/null \
    || package_die 'the manifest that was just written is not JSON'

# ---------------------------------------------------------------------------
# The archive.
# ---------------------------------------------------------------------------

package_create_archive "$build" "$root" "$output_dir/$archive_name"

# ---------------------------------------------------------------------------
# What this produced. Named, so a run that packed a smaller product than the
# last one says so rather than only being green.
# ---------------------------------------------------------------------------

manifest="$build/$root/$(package_manifest_path)"
echo "# FerriteCAD release archive"
echo "version        $version"
echo "revision       $source_revision"
echo "target         $triple"
echo "root           $root"
echo "archive        $archive_name"
echo "runtime files  $(jq -r '.totals.runtimeFiles' "$manifest")"
echo "runtime bytes  $(jq -r '.totals.runtimeBytes' "$manifest")"
echo "metadata files $(jq -r '.totals.packageMetadataFiles' "$manifest")"
echo "sbom sha256    $(jq -r '.productSbom.sha256' "$manifest")"
echo "archive sha256 $(package_sha256 "$output_dir/$archive_name")"
echo "archive bytes  $(package_size "$output_dir/$archive_name")"
