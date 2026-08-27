# SPDX-License-Identifier: MIT
# shellcheck shell=bash
# Everything here is read by the scripts that source this file.
# shellcheck disable=SC2034
#
# Shared definitions for the release packager. Sourced, never run.
#
# The packager turns a staging directory that tools/stage-runtime-layout.sh has
# already produced, and that tools/check-staged-layout.sh has already started
# from a clean environment, into one versioned archive per target. It does not
# decide anything about that layout. It does not walk a runtime closure, look
# for a library, rewrite a run path, link anything, or work out which Open
# CASCADE toolkit the product needs; every one of those questions already has
# an owner, and a second answer here would be a second product.
#
# What it reads, and from whom:
#
#   the staging directory              tools/stage-runtime-layout.sh
#   platform -> target triple          tools/native/lib.sh
#   the product version                sbom/native/native-assets-inventory.json
#   which component owns which file    the same inventory's stagedFiles
#   the product SBOM of that target    sbom/product/ferritecad-product-<t>.cdx.json
#   the bytes of the staged files      the staging directory itself
#
# Nothing else. A fact this file computed for itself would be a fact two
# documents could disagree about while both looking right.

# shellcheck source=tools/product/lib.sh
. tools/product/lib.sh

# The shape of the package manifest. Bumped when the document changes in a way
# a consumer would have to notice. Deliberately not any of the SBOM format
# numbers: the manifest describes one build's files and the SBOM describes the
# product's components, and a reader of one must not be told the other changed.
#
# 2 adds sourceRevision, and it is a bump rather than an optional field because
# a consumer that read a manifest without one would have no way to tell a
# package built from this commit from a package built from another commit that
# carried the same product version. Version 1 packages are still packages; they
# are not members of a release set, and the gates say so rather than quietly
# treating a missing revision as a matching one.
readonly PACKAGE_FORMAT=2
readonly PACKAGE_KIND='ferritecad-release-package-manifest'
readonly PACKAGE_SCHEMA='tools/package/manifest.schema.json'

# Where package metadata lives inside the archive, and what it is called. The
# runtime layout keeps its own relative paths untouched; everything this slice
# adds goes under one directory that the layout does not use, so a reader can
# tell at a glance which files a release carries because it has to run and
# which it carries because it has to be describable.
readonly PACKAGE_METADATA_DIR='package'
readonly PACKAGE_MANIFEST_NAME='ferritecad-package.json'

# The archive, and every property of it that has to be decided once.
#
# Measured on the three runners rather than read out of a manual, because the
# question is what those machines have and what survives a round trip on them:
#
#   GNU tar 1.35 is on all three - /usr/bin/tar on Linux and in the Windows Git
#   Bash, /opt/homebrew/bin/gtar on macOS, where /usr/bin/tar is bsdtar 3.5.3
#   and rejects --sort. So one tool, one set of flags, one code path.
#
#   `zip` is absent on the Windows runner, which disqualifies it outright.
#   7-Zip is on all three but at 17.05, 23.01 and 26.02, and a `.7z` needs a
#   7-Zip on the receiving machine; a `.tar.gz` does not.
#
#   ustar accepted the longest real release path on all three, the executable
#   bit survived extraction on all three, and on macOS `codesign -v` still
#   verified the extracted binary and it still ran. That last one is the
#   property that matters most here: editing load commands invalidated the
#   signature, staging re-signed ad hoc to make the image startable, and an
#   archive that lost the signature would produce a package killed by the
#   kernel before dyld got a turn.
#
#   Two packings of the same bytes on one host were byte-identical.
readonly PACKAGE_ARCHIVE_SUFFIX='.tar.gz'
readonly PACKAGE_TAR_FORMAT='ustar'
# Every entry gets this and only this. A timestamp that came from the file
# system would make the archive depend on when the build ran, and the whole
# claim is that it depends only on the bytes.
readonly PACKAGE_MTIME_EPOCH=0
readonly PACKAGE_GZIP_FLAGS='-n -9'

# The one owner is the inventory; this is only the spelling of the question.
package_triple_for() { # platform
    native_triple_for "$1"
}

package_root_for() { # version triple
    printf 'ferritecad-%s-%s\n' "$1" "$2"
}

package_archive_for() { # version triple
    printf 'ferritecad-%s-%s%s\n' "$1" "$2" "$PACKAGE_ARCHIVE_SUFFIX"
}

package_manifest_path() {
    printf '%s/%s\n' "$PACKAGE_METADATA_DIR" "$PACKAGE_MANIFEST_NAME"
}

package_sbom_path_for() { # triple
    printf '%s/%s\n' "$PACKAGE_METADATA_DIR" "$(basename "$(product_output_for "$1")")"
}

# The release set: the three platform archives of one product version, built
# from one source revision, as a single object somebody can be handed.
#
# One archive is a delivery for one machine. Three of them uploaded separately
# by three jobs are three files that happen to be in the same run: nothing in
# any of them says the other two exist, that all three targets are present,
# that they carry one version, or that they were built from one revision of
# this repository. Two of those questions cannot even be asked of a single
# archive, and the one that can - "is this the same product as that one" - was
# answerable before only by comparing version strings, which an archive from
# another commit carrying the same version answers correctly.
readonly RELEASE_SET_KIND='ferritecad-release-set'
readonly RELEASE_SET_FORMAT=1
readonly RELEASE_SET_NAME='ferritecad-release-set.json'
readonly RELEASE_SET_CHECKSUMS='SHA256SUMS'
readonly RELEASE_SET_SCHEMA='tools/package/release-set.schema.json'

# The targets a release set must carry, derived from the platform list rather
# than written out again. A second list here could go on naming three targets
# after the product had four.
package_expected_targets() {
    local platform
    for platform in "${NATIVE_PLATFORMS[@]}"; do
        package_triple_for "$platform"
    done | LC_ALL=C sort
}

# Which platform builds a target, which is the same question the other way
# round and has the same one owner.
package_platform_for_triple() { # triple
    local platform
    for platform in "${NATIVE_PLATFORMS[@]}"; do
        if [ "$(package_triple_for "$platform")" = "$1" ]; then
            printf '%s\n' "$platform"
            return 0
        fi
    done
    package_die "no platform builds $1"
}

# A source revision is a full commit object name, lower case, and nothing else.
#
# Checked rather than trusted because everything downstream treats it as the
# answer to "which commit is this product": an abbreviated name is ambiguous by
# construction, a branch name is a moving target, and an empty string is what a
# lookup that found nothing leaves behind.
package_valid_revision() { # revision
    case "$1" in
        *[!0-9a-f]* | '') return 1 ;;
    esac
    [ "${#1}" -eq 40 ]
}

# A path every MSYS tool can take.
#
# RUNNER_TEMP on Windows is spelled D:\a\_temp. A backslash is an escape to
# sed and to a shell pattern, and a colon makes GNU tar read the name as
# host:path and try to reach a remote machine. Both have cost this repository
# a run before, so the paths that reach an archiver are converted once, here.
package_posix_path() { # path
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -u "$1"
    else
        printf '%s\n' "$1"
    fi
}

package_die() {
    echo "${PACKAGE_TOOL:-package}: $*" >&2
    exit 1
}

# GNU tar, wherever this host keeps it.
#
# Asserted rather than assumed. bsdtar is what `tar` means on macOS, it has no
# --sort, and an archive whose entry order came from readdir is an archive that
# is not reproducible for a reason nothing in it records.
package_gnu_tar() {
    local candidate found
    for candidate in gtar tar; do
        found="$(command -v "$candidate" 2>/dev/null || true)"
        [ -n "$found" ] || continue
        if "$found" --version 2>/dev/null | head -1 | grep -q 'GNU tar'; then
            printf '%s\n' "$found"
            return 0
        fi
    done
    package_die 'no GNU tar on this host; bsdtar cannot sort entries and the archive would not be reproducible'
}

# SHA-256 of a file, and the same answer on all three runners.
package_sha256() { # file
    native_sha256 "$1"
}

package_size() { # file
    wc -c < "$1" | tr -d ' '
}

# Whether the file system says this file may be executed.
#
# Measured, never decided. Which staged files are programs is a question the
# layout owner already answered by producing them; repeating the answer here as
# a rule about directory names would be a second layout.
package_is_executable() { # file
    if [ -x "$1" ]; then printf 'true\n'; else printf 'false\n'; fi
}

# The archive, made the one way it is ever made.
#
# --sort=name so the order is a property of the names; --mtime so it is not a
# property of when the build ran; --owner/--group/--numeric-owner so it is not
# a property of who ran it; --no-xattrs so it is not a property of what the
# file system happened to be carrying. gzip -n leaves the original name and
# timestamp out of the compressed stream, which is the other half of the same
# claim.
package_create_archive() { # parent-directory root-name output-file
    local parent="$1" root="$2" out="$3" tar_bin
    tar_bin="$(package_gnu_tar)"
    # shellcheck disable=SC2086  # the gzip flags are a fixed word list
    "$tar_bin" --sort=name --mtime="@$PACKAGE_MTIME_EPOCH" \
        --owner=0 --group=0 --numeric-owner --no-xattrs \
        --format="$PACKAGE_TAR_FORMAT" -C "$parent" -cf - "$root" \
        | gzip $PACKAGE_GZIP_FLAGS > "$out"
}

# The one member of an archive that has to be read before anything can be said
# about the archive at all, taken out without trusting the rest of it.
#
# This is not a second opinion about whether the archive is a package. That
# question belongs to tools/check-release-package.sh, which extracts the whole
# thing, re-hashes every byte of it and runs what came out; both the release
# set builder and its checker hand each archive to it. What is needed first is
# narrower and comes earlier: a name inside the archive cannot be extracted
# safely until the archive has been shown to hold no absolute path, no parent
# traversal and nothing that is not a regular file or a directory, because each
# of those writes outside the directory the extraction was told to use, and
# after extracting it is too late to ask.
#
# Prints the archive's one top-level directory and writes the package manifest
# to WORK/package-manifest.json.
package_read_manifest() { # archive work-directory
    local archive="$1" work="$2" tar_bin root count
    tar_bin="$(package_gnu_tar)"
    "$tar_bin" -tf "$archive" > "$work/members" 2>"$work/members.err" \
        || { echo "${PACKAGE_TOOL:-package}: $archive could not be listed:" >&2
             sed 's/^/  /' "$work/members.err" >&2; exit 1; }
    [ -s "$work/members" ] || package_die "$archive holds nothing at all"

    local name
    while IFS= read -r name; do
        case "$name" in
            /*)                   package_die "$archive holds the absolute path $name" ;;
            ../* | */../* | */.. | ..) package_die "$archive holds a parent traversal: $name" ;;
            ./* | .)              package_die "$archive holds the unnormalised path $name" ;;
            [A-Za-z]:*)           package_die "$archive holds the drive-qualified path $name" ;;
            *\\*)                 package_die "$archive holds the backslash path $name" ;;
        esac
    done < "$work/members"

    # Deliberately -tvf and not -tf: the first field says what kind of entry it
    # is, and a symlink or a device node is not payload.
    "$tar_bin" -tvf "$archive" | awk '$1 !~ /^[-d]/ { print }' > "$work/odd-members"
    if [ -s "$work/odd-members" ]; then
        echo "${PACKAGE_TOOL:-package}: $archive holds an entry that is neither a regular file nor a directory:" >&2
        sed 's/^/  /' "$work/odd-members" >&2
        exit 1
    fi

    sed 's|/.*||' "$work/members" | LC_ALL=C sort -u > "$work/archive-roots"
    count="$(wc -l < "$work/archive-roots" | tr -d ' ')"
    if [ "$count" -ne 1 ]; then
        echo "${PACKAGE_TOOL:-package}: $archive does not extract into one directory:" >&2
        sed 's/^/  /' "$work/archive-roots" >&2
        exit 1
    fi
    root="$(cat "$work/archive-roots")"

    rm -rf "$work/manifest-only"
    mkdir -p "$work/manifest-only"
    "$tar_bin" -C "$work/manifest-only" -xzf "$archive" "$root/$(package_manifest_path)" \
        2>"$work/extract.err" \
        || { echo "${PACKAGE_TOOL:-package}: $archive carries no $(package_manifest_path):" >&2
             sed 's/^/  /' "$work/extract.err" >&2; exit 1; }
    [ -f "$work/manifest-only/$root/$(package_manifest_path)" ] \
        || package_die "$archive carries no $(package_manifest_path)"
    cp "$work/manifest-only/$root/$(package_manifest_path)" "$work/package-manifest.json"
    jq -e . "$work/package-manifest.json" > /dev/null 2>&1 \
        || package_die "the package manifest inside $archive is not JSON"
    printf '%s\n' "$root"
}
