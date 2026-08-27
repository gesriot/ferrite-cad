#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The release set builder and its checker, on archives this script makes.
#
# The three-platform workflow is the only thing that can say a release set is
# made of archives that really ran, and it says so by running them. But almost
# everything that makes a set of three archives not a release of one product is
# arithmetic over three files and two documents: a target missing, a target
# twice, a fourth target, two product versions, two source revisions, a package
# manifest too old to record a revision at all, an archive renamed, an archive
# replaced after the set was assembled, a checksums file that does not say what
# the archives hash to. Waiting for three runners to build Open CASCADE before
# asking any of those would mean asking them roughly never.
#
# So this builds the three archives from staging fixtures with the real names
# and made-up bytes, assembles them, and then breaks the result one way at a
# time and requires the real builder or the real checker to name what broke.
#
# The revision the fixtures are packed with is made up and is deliberately not
# this checkout's HEAD: a builder that worked the revision out for itself
# instead of reading it out of the archives would produce a document naming a
# commit nobody built, and the only way to see that is for the two to differ.
#
# Runs no network. Runs no product binary: the archives carry fixture bytes,
# and tools/check-release-package.sh is called with --no-execute, which says so
# in its own facts.
#
# Run from the repository root:
#   tools/check-release-set-builder.sh

set -euo pipefail

PACKAGE_TOOL='check-release-set-builder'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/package/lib.sh
. tools/package/lib.sh

[ $# -eq 0 ] || package_die "unknown argument: $1"

native_require_jq
package_gnu_tar > /dev/null

# Made up, and shaped like the real thing. Two of them, because the defect this
# slice exists to refuse is two commits behind one product version.
FIXTURE_REVISION='a1b2c3d4e5f60718293a4b5c6d7e8f9012345678'
OTHER_REVISION='0fedcba987654321fedcba9876543210fedcba98'

if [ ! -x tools/build-release-set.sh ]; then
    echo "$PACKAGE_TOOL: there is nothing that assembles a release set." >&2
    echo >&2
    echo "  Three platform archives uploaded by three jobs are three files. Nothing" >&2
    echo "  requires all three targets to be present, nothing requires them to carry" >&2
    echo "  one product version, nothing at all requires them to have been built from" >&2
    echo "  one commit, and nothing pins the bytes of any of them." >&2
    exit 1
fi

work="$(mktemp -d)"
# shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

# Nothing that produced the archives may be reachable when a package is
# verified, and on this fixture nothing ever was: the staging directories are
# removed as soon as they are packed, and this is a path that has never
# existed. Passed on so the gate that owns that question can ask it.
absent="$work/there-is-no-build-tree"

checks=0
failures=0
fail() { failures=$((failures + 1)); echo "$PACKAGE_TOOL: $*" >&2; }

# Every way out of this script says how many checks it made, including the ones
# that stop half way. A run that gave up without a count cannot be told from a
# run that asserted nothing, and the difference is the whole value of counting.
report_and_exit() { # status
    echo
    if [ "$checks" -eq 0 ]; then
        echo "$PACKAGE_TOOL: this run asserted nothing at all" >&2
        exit 1
    fi
    if [ "$failures" -ne 0 ]; then
        echo "$PACKAGE_TOOL: $failures of $checks checks failed" >&2
        exit 1
    fi
    [ "$1" -eq 0 ] || { echo "$PACKAGE_TOOL: $checks checks, and the run stopped" >&2; exit "$1"; }
    echo "$PACKAGE_TOOL: $checks checks, all of them passed"
    exit 0
}

expect_pass() { # description command...
    local what="$1"; shift
    local status=0
    checks=$((checks + 1))
    "$@" > "$work/out.txt" 2>&1 || status=$?
    if [ "$status" -eq 0 ]; then
        echo "  ok      $what"
    else
        fail "$what: expected success, got exit $status"
        sed 's/^/      /' "$work/out.txt" >&2
    fi
}

expect_fail() { # description expected-substring command...
    local what="$1" wanted="$2"; shift 2
    local status=0
    checks=$((checks + 1))
    "$@" > "$work/out.txt" 2>&1 || status=$?
    if [ "$status" -eq 0 ]; then
        fail "$what: it passed, and it must not"
        sed 's/^/      /' "$work/out.txt" >&2
        return
    fi
    if ! LC_ALL=C grep -qF "$wanted" "$work/out.txt"; then
        fail "$what: failed for some other reason than '$wanted'"
        sed 's/^/      /' "$work/out.txt" >&2
        return
    fi
    echo "  ok      $what"
}

# ---------------------------------------------------------------------------
# Three archives, made the way the real ones are made.
# ---------------------------------------------------------------------------
#
# The staged names come from the inventory, so a target that gains or loses a
# library is a fixture that gains or loses it too. The bytes are invented and
# do not pretend otherwise.

version="$(jq -r '.productVersion' "$NATIVE_INVENTORY" | native_strip_cr)"
[ -n "$version" ] && [ "$version" != null ] \
    || package_die "$NATIVE_INVENTORY does not say what version the product is"

pack_fixture() { # platform revision output-dir
    local platform="$1" revision="$2" out="$3" triple staging path
    triple="$(package_triple_for "$platform")"
    staging="$work/staging-$platform-$revision"
    rm -rf "$staging"
    jq -r --arg t "$triple" \
        '.targets[] | select(.triple == $t) | .stagedFiles[] | .path' \
        "$NATIVE_INVENTORY" | native_strip_cr | LC_ALL=C sort > "$work/fixture-paths"
    [ -s "$work/fixture-paths" ] || package_die "the inventory stages nothing for $triple"
    while IFS= read -r path; do
        mkdir -p "$staging/$(dirname "$path")"
        printf 'fixture bytes for %s\n' "$path" > "$staging/$path"
        chmod 755 "$staging/$path"
    done < "$work/fixture-paths"
    mkdir -p "$out"
    tools/package-release.sh --platform "$platform" --staging "$staging" \
        --output-dir "$out" --source-revision "$revision" > /dev/null
    # Taken away at once. Everything below is asked of archives, and a run that
    # could reach the directory one was made from would be asking the previous
    # slice's question.
    rm -rf "$staging"
}

echo "== the packager, on the revision it is handed"

# The revision is an input, and a packager with no revision produces a package
# that cannot be told from a package of another commit.
checks=$((checks + 1))
if tools/package-release.sh --platform linux --staging "$work" --output-dir "$work/never" \
        > "$work/out.txt" 2>&1; then
    fail 'the packager packed with no --source-revision'
elif LC_ALL=C grep -qF 'no --source-revision' "$work/out.txt"; then
    echo "  ok      the packager refuses to pack without a source revision"
else
    fail 'the packager refused a missing --source-revision for some other reason'
    sed 's/^/      /' "$work/out.txt" >&2
fi

expect_fail 'the packager refuses a revision that is not a commit object name' \
    'is not a full lower-case commit object name' \
    tools/package-release.sh --platform linux --staging "$work" \
        --output-dir "$work/never" --source-revision 'HEAD'

expect_fail 'the packager refuses an abbreviated revision' \
    'is not a full lower-case commit object name' \
    tools/package-release.sh --platform linux --staging "$work" \
        --output-dir "$work/never" --source-revision "${FIXTURE_REVISION:0:12}"

good="$work/archives"
for platform in "${NATIVE_PLATFORMS[@]}"; do
    pack_fixture "$platform" "$FIXTURE_REVISION" "$good"
done
pack_fixture windows "$OTHER_REVISION" "$work/other-commit"

archive_for() { # triple
    printf '%s/%s\n' "$good" "$(package_archive_for "$version" "$1")"
}
linux_archive="$(archive_for "$(package_triple_for linux)")"
macos_archive="$(archive_for "$(package_triple_for macos)")"
windows_archive="$(archive_for "$(package_triple_for windows)")"
other_windows="$work/other-commit/$(package_archive_for "$version" "$(package_triple_for windows)")"

build_set_into() { # output-dir archive...
    local out="$1"; shift
    local arguments=()
    local archive
    for archive in "$@"; do arguments+=(--archive "$archive"); done
    tools/build-release-set.sh "${arguments[@]}" --output-dir "$out"
}
build_set() { # output-dir archive...
    rm -rf "$1"
    build_set_into "$@"
}
check_set() { # directory
    tools/check-release-set.sh --release-set "$1" --forbidden "$absent" \
        --output "$work/facts.txt"
}
check_hand() { # linux-archive macos-archive windows-archive
    hand_assemble "$work/hand-good" \
        "$(package_triple_for linux):$1" \
        "$(package_triple_for macos):$2" \
        "$(package_triple_for windows):$3"
    check_set "$work/hand-good"
}

# ---------------------------------------------------------------------------
# The three archives, assembled.
# ---------------------------------------------------------------------------

echo "== the builder, on three archives of one build"

expect_pass 'three archives of one build become one release set' \
    build_set "$work/set" "$linux_archive" "$macos_archive" "$windows_archive"

if [ ! -f "$work/set/$RELEASE_SET_NAME" ]; then
    checks=$((checks + 1))
    fail 'no release set was produced, so nothing below can be asked'
    report_and_exit 1
fi

# The document names the commit the archives were packed with, and this
# checkout is standing on a different one. A builder that ran `git rev-parse
# HEAD`, or read the version out of the inventory instead of out of the
# archives, would be describing the tree it is in rather than the archives it
# was given.
checks=$((checks + 1))
head_revision="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
said_revision="$(jq -r '.sourceRevision' "$work/set/$RELEASE_SET_NAME" | native_strip_cr)"
if [ "$said_revision" != "$FIXTURE_REVISION" ]; then
    fail "the release set says it was built from $said_revision and the archives say $FIXTURE_REVISION"
elif [ "$FIXTURE_REVISION" = "$head_revision" ]; then
    fail 'the fixture revision is this checkout HEAD, so the two cannot be told apart'
else
    echo "  ok      the release set takes the revision from the archives, not from this checkout"
fi

# Twice, on the same three archives, byte for byte. Both documents are what a
# recipient compares against, and a document that depended on the order three
# artifacts happened to be downloaded in could not be compared with anything.
checks=$((checks + 1))
if build_set "$work/set-again" "$windows_archive" "$linux_archive" "$macos_archive" \
        > "$work/out.txt" 2>&1 \
    && cmp -s "$work/set/$RELEASE_SET_NAME" "$work/set-again/$RELEASE_SET_NAME" \
    && cmp -s "$work/set/$RELEASE_SET_CHECKSUMS" "$work/set-again/$RELEASE_SET_CHECKSUMS"; then
    echo "  ok      the same three archives in another order assemble to the same bytes"
else
    fail 'two assemblies of the same three archives differ'
    sed 's/^/      /' "$work/out.txt" >&2
fi

# Three archives and two documents. Nothing else, and nothing repacked: each
# archive in the set is byte for byte the one that was handed in.
checks=$((checks + 1))
find "$work/set" -mindepth 1 -maxdepth 1 -type f -exec basename {} \; \
    | LC_ALL=C sort > "$work/produced"
{ printf '%s\n%s\n' "$RELEASE_SET_NAME" "$RELEASE_SET_CHECKSUMS"
  for triple in $(package_expected_targets); do package_archive_for "$version" "$triple"; done
} | LC_ALL=C sort > "$work/produced-expected"
if cmp -s "$work/produced-expected" "$work/produced" \
    && cmp -s "$linux_archive" "$work/set/$(basename "$linux_archive")" \
    && cmp -s "$macos_archive" "$work/set/$(basename "$macos_archive")" \
    && cmp -s "$windows_archive" "$work/set/$(basename "$windows_archive")"; then
    echo "  ok      the set is the three archives handed in and two documents"
else
    fail 'the set is not the three archives handed in and two documents'
    diff -u "$work/produced-expected" "$work/produced" >&2 || true
fi

expect_pass 'the assembled release set verifies' check_set "$work/set"

checks=$((checks + 1))
if grep -q '^release-set archives-verified=3' "$work/facts.txt" \
    && grep -q '^release-set targets=3 order=sorted' "$work/facts.txt" \
    && grep -q '^release-set files=5 extra=0' "$work/facts.txt" \
    && grep -q '^release-set checksums=3 order=sorted escaping=none' "$work/facts.txt" \
    && grep -q "^release-set version=${version} source-revision=${FIXTURE_REVISION}\$" "$work/facts.txt"; then
    echo "  ok      the facts name the three archives, the version and the revision"
else
    fail 'the facts do not say what was verified'
    sed 's/^/      /' "$work/facts.txt" >&2
fi

# The shape of the document, by a real validator rather than by a parser
# written here, pinned for the reason a gate that cannot fail is not a gate.
checks=$((checks + 1))
if ! command -v jsonschema-cli >/dev/null 2>&1; then
    fail 'jsonschema-cli is not installed; the shape of the release set cannot be checked'
else
    . tools/sbom/pin.env
    found="$(jsonschema-cli --version 2>/dev/null | awk '{print $NF}')"
    if [ "$found" != "$JSONSCHEMA_CLI_VERSION" ]; then
        fail "jsonschema-cli $found is installed but $JSONSCHEMA_CLI_VERSION is pinned"
    elif jsonschema-cli validate "$RELEASE_SET_SCHEMA" \
            -i "$work/set/$RELEASE_SET_NAME" > "$work/schema.log" 2>&1; then
        echo "  ok      the release set matches $RELEASE_SET_SCHEMA"
    else
        fail "the release set does not match $RELEASE_SET_SCHEMA:"
        sed 's/^/      /' "$work/schema.log" >&2
    fi
fi

# ---------------------------------------------------------------------------
# Sets that are not one product.
# ---------------------------------------------------------------------------

echo "== the builder, on three archives that are not one build"

# Into a directory of its own. Assembling on top of something else would put a
# file into the release set that nothing in the run produced, and the checker
# would then be asked about a set the builder did not make.
mkdir -p "$work/occupied"
printf 'something else\n' > "$work/occupied/a-file"
expect_fail 'assembling into a directory that already holds something is refused' \
    'is not empty' \
    build_set_into "$work/occupied" "$linux_archive" "$macos_archive" "$windows_archive"

expect_fail 'an archive built from another commit at the same version is refused' \
    'more than one source revision' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$other_windows"

expect_fail 'two archives are refused' 'a release set is the three platform archives' \
    build_set "$work/reject" "$linux_archive" "$macos_archive"

expect_fail 'four archives are refused' 'a release set is the three platform archives' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$windows_archive" "$other_windows"

expect_fail 'the same file given twice is refused' 'the same archive was given twice' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$macos_archive"

# And the same target as two separate files, which a check on the paths would
# not catch: this is what a set missing a target actually looks like when
# something downloaded the same artifact twice.
mkdir -p "$work/duplicate"
cp "$macos_archive" "$work/duplicate/"
expect_fail 'two archives for the same target are refused' 'the same target' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" \
        "$work/duplicate/$(basename "$macos_archive")"

# ---------------------------------------------------------------------------
# Archives that are not what they say they are.
# ---------------------------------------------------------------------------
#
# Each starts from a real archive, changes exactly one thing inside it, and is
# packed again the way the packager packs.

tree="$work/tree"
rebuilt="$work/rebuilt"
mkdir -p "$rebuilt"

explode() { # archive
    local root
    rm -rf "$tree"; mkdir -p "$tree"
    "$(package_gnu_tar)" -C "$tree" -xzf "$1"
    root="$(find "$tree" -mindepth 1 -maxdepth 1 -type d -exec basename {} \;)"
    printf '%s\n' "$root"
}
edit_manifest() { # root jq-program
    jq -S "$2" "$tree/$1/$(package_manifest_path)" | native_strip_cr > "$work/m.json"
    mv "$work/m.json" "$tree/$1/$(package_manifest_path)"
}
# Each repacking gets a directory of its own, so an archive built here is still
# the archive it was when something further down refers to it. One output path
# reused by every case would quietly make every earlier variable point at the
# last archive written, and the cases would go on looking like they passed.
# Made with mktemp rather than a counter: this function is called through
# command substitution, and a counter incremented in a subshell is a counter
# that never moves.
repack_as() { # root name
    local where
    where="$(mktemp -d "$rebuilt/repack-XXXXXX")"
    package_create_archive "$tree" "$1" "$where/$2"
    printf '%s\n' "$where/$2"
}

# A package manifest of the format that had no revision in it. Format 1
# packages are packages; they are not members of a release set, and a set that
# accepted one could not say which commit a third of it came from.
root="$(explode "$windows_archive")"
edit_manifest "$root" '.formatVersion = 1 | del(.sourceRevision)'
old_format="$(repack_as "$root" "$(basename "$windows_archive")")"
expect_fail 'an archive carrying a format 1 package manifest is refused' \
    'a release set is made of format' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$old_format"

root="$(explode "$windows_archive")"
edit_manifest "$root" '.sourceRevision = "not-a-commit"'
bad_revision="$(repack_as "$root" "$(basename "$windows_archive")")"
expect_fail 'an archive whose manifest carries no real revision is refused' \
    'is not a full lower-case commit object name' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$bad_revision"

root="$(explode "$windows_archive")"
printf 'this is not a manifest\n' > "$tree/$root/$(package_manifest_path)"
broken_manifest="$(repack_as "$root" "$(basename "$windows_archive")")"
expect_fail 'an archive whose package manifest is not JSON is refused' 'is not JSON' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$broken_manifest"

root="$(explode "$windows_archive")"
rm -f "$tree/$root/$(package_manifest_path)"
no_manifest="$(repack_as "$root" "$(basename "$windows_archive")")"
expect_fail 'an archive with no package manifest at all is refused' 'carries no' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$no_manifest"

# A target the product does not have. The archive is internally consistent:
# renamed root, renamed file, manifest agreeing with both.
root="$(explode "$windows_archive")"
unknown_target='aarch64-unknown-linux-gnu'
unknown_root="$(package_root_for "$version" "$unknown_target")"
unknown_name="$(package_archive_for "$version" "$unknown_target")"
mv "$tree/$root" "$tree/$unknown_root"
jq -S --arg t "$unknown_target" --arg r "$unknown_root" --arg a "$unknown_name" \
    '.target = $t | .root = $r | .archive = $a' \
    "$tree/$unknown_root/$(package_manifest_path)" | native_strip_cr > "$work/m.json"
mv "$work/m.json" "$tree/$unknown_root/$(package_manifest_path)"
fourth_target="$(repack_as "$unknown_root" "$unknown_name")"
expect_fail 'an archive for a target the product does not have is refused' 'unknown:' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$fourth_target"

# Another product version, consistently renamed the same way.
root="$(explode "$windows_archive")"
other_version="$version-other"
other_root="$(package_root_for "$other_version" "$(package_triple_for windows)")"
other_name="$(package_archive_for "$other_version" "$(package_triple_for windows)")"
mv "$tree/$root" "$tree/$other_root"
jq -S --arg v "$other_version" --arg r "$other_root" --arg a "$other_name" \
    '.productVersion = $v | .root = $r | .archive = $a' \
    "$tree/$other_root/$(package_manifest_path)" | native_strip_cr > "$work/m.json"
mv "$work/m.json" "$tree/$other_root/$(package_manifest_path)"
other_version_archive="$(repack_as "$other_root" "$other_name")"
expect_fail 'an archive of another product version is refused' 'more than one product version' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$other_version_archive"

# Renamed on the way here, and nothing inside it changed.
renamed="$work/ferritecad-0.0.0-x86_64-pc-windows-msvc.tar.gz"
cp "$windows_archive" "$renamed"
expect_fail 'an archive renamed after it was built is refused' 'says its archive is' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$renamed"

# An archive that no packager would write. Each of these has to be refused
# before anything inside it is extracted.
craft() { # deformation output
    local root
    rm -rf "$tree"; mkdir -p "$tree"
    "$(package_gnu_tar)" -C "$tree" -xzf "$windows_archive"
    root="$(find "$tree" -mindepth 1 -maxdepth 1 -type d -exec basename {} \;)"
    python3 tools/package/craft-archive.py "$tree" "$root" "$2" "$1"
}
mkdir -p "$rebuilt/crafted"
crafted="$rebuilt/crafted/$(basename "$windows_archive")"

craft absolute-path "$crafted"
expect_fail 'an archive holding an absolute path is refused before it is opened' \
    'absolute path' build_set "$work/reject" "$linux_archive" "$macos_archive" "$crafted"

craft parent-traversal "$crafted"
expect_fail 'an archive holding a parent traversal is refused before it is opened' \
    'parent traversal' build_set "$work/reject" "$linux_archive" "$macos_archive" "$crafted"

craft symlink-payload "$crafted"
expect_fail 'an archive holding a symlink is refused before it is opened' \
    'neither a regular file nor a directory' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$crafted"

craft two-roots "$crafted"
expect_fail 'an archive with two top-level directories is refused' \
    'does not extract into one directory' \
    build_set "$work/reject" "$linux_archive" "$macos_archive" "$crafted"

# The product SBOM inside the archive is the committed one, and the gate that
# owns that question is the one that answers it here.
root="$(explode "$windows_archive")"
windows_triple="$(package_triple_for windows)"
other_triple="$(package_triple_for linux)"
packaged_sbom="$tree/$root/$(package_sbom_path_for "$windows_triple")"
cp "$(product_output_for "$other_triple")" "$packaged_sbom"
jq -S --arg p "$(package_sbom_path_for "$windows_triple")" \
      --arg d "$(package_sha256 "$packaged_sbom")" \
      --argjson s "$(package_size "$packaged_sbom")" \
    '.productSbom.sha256 = $d
     | .packageMetadata = [.packageMetadata[] | if .path == $p then .sha256 = $d | .size = $s else . end]' \
    "$tree/$root/$(package_manifest_path)" | native_strip_cr > "$work/m.json"
mv "$work/m.json" "$tree/$root/$(package_manifest_path)"
wrong_sbom="$(repack_as "$root" "$(basename "$windows_archive")")"
expect_pass 'an archive carrying the wrong product SBOM still assembles' \
    build_set "$work/set-wrong-sbom" "$linux_archive" "$macos_archive" "$wrong_sbom"
expect_fail 'and the checker refuses it, through the gate that owns packages' \
    'byte for byte' check_set "$work/set-wrong-sbom"

# ---------------------------------------------------------------------------
# Release sets broken after they were assembled.
# ---------------------------------------------------------------------------
#
# The checker verifies the output rather than the builder's inputs, so every
# one of these is applied to a finished set.

echo "== the checker, on a release set that has been changed since"

broken="$work/broken"
break_set() { # command...
    rm -rf "$broken"
    cp -R "$work/set" "$broken"
    "$@"
}
edit_document() { # jq-program
    jq -S "$1" "$broken/$RELEASE_SET_NAME" | native_strip_cr > "$work/d.json"
    mv "$work/d.json" "$broken/$RELEASE_SET_NAME"
}
a_member="$(package_archive_for "$version" "$(package_triple_for linux)")"

break_set cp "$other_windows" "$broken/$(basename "$windows_archive")"
expect_fail 'an archive replaced after the set was assembled is caught' \
    'hashes to' check_set "$broken"

break_set true
printf 'appended\n' >> "$broken/$a_member"
expect_fail 'an archive changed after the set was assembled is caught' \
    'hashes to' check_set "$broken"

break_set edit_document \
    '.targets = [.targets[] | if .target == "x86_64-unknown-linux-gnu" then .size = 1 else . end]'
expect_fail 'a size the archive does not have is caught' 'bytes and the release set says' \
    check_set "$broken"

break_set rm -f "$broken/$a_member"
expect_fail 'an archive missing from the set is caught' 'named and not there' check_set "$broken"

break_set touch "$broken/a-spare-file"
expect_fail 'a file in the set that nothing names is caught' 'there and not named' \
    check_set "$broken"

break_set mv "$broken/$a_member" "$broken/ferritecad-elsewhere.tar.gz"
expect_fail 'an archive renamed inside the set is caught' 'named and not there' check_set "$broken"

break_set edit_document '.targets = [.targets[0], .targets[1]]'
expect_fail 'a set naming two targets is caught' 'a release set is three' check_set "$broken"

break_set edit_document '.targets = [.targets[0], .targets[0], .targets[1]]'
expect_fail 'a set naming one target twice is caught' 'more than once' check_set "$broken"

break_set edit_document '.targets = (.targets | reverse)'
expect_fail 'a set whose targets are not sorted is caught' 'sorted order' check_set "$broken"

break_set edit_document '.sourceRevision = "0000000000000000000000000000000000000000"'
expect_fail 'a set naming a revision its archives do not carry is caught' \
    'and the release set says' check_set "$broken"

break_set edit_document '.sourceRevision = "HEAD"'
expect_fail 'a set naming something that is not a commit is caught' \
    'not a source revision' check_set "$broken"

break_set edit_document '.productVersion = "9.9.9"'
expect_fail 'a set naming a version its archives do not carry is caught' \
    'is named' check_set "$broken"

break_set edit_document '.packageManifestFormat = 1'
expect_fail 'a set expecting the format that had no revision is caught' \
    'expects package manifest format' check_set "$broken"

break_set edit_document '.runId = "17264881234"'
expect_fail 'a set carrying a run identifier is caught' 'unknown: runId' check_set "$broken"

break_set edit_document '.packageManifestPath = "/home/runner/work/package.json"'
expect_fail 'a set carrying an absolute path is caught' 'belongs to a machine' \
    check_set "$broken"

break_set edit_document '.productVersion = "2026-08-27T10:11:12Z"'
expect_fail 'a set carrying a timestamp is caught' 'belongs to a machine' check_set "$broken"

break_set edit_document \
    '.targets = [.targets[] | if .target == "x86_64-unknown-linux-gnu" then .packageManifestSha256 = "0000000000000000000000000000000000000000000000000000000000000000" else . end]'
expect_fail 'a set that does not pin the package manifest inside an archive is caught' \
    'the package manifest in' check_set "$broken"

# ---- the checksums file ------------------------------------------------
break_set true
printf 'appended\n' >> "$broken/$RELEASE_SET_CHECKSUMS"
expect_fail 'a checksums file with a spare line is caught' 'is not lines of a digest and a name' \
    check_set "$broken"

break_set true
grep -v "$a_member" "$work/set/$RELEASE_SET_CHECKSUMS" > "$broken/$RELEASE_SET_CHECKSUMS"
expect_fail 'a checksums file missing a line is caught' 'does not say what the archives hash to' \
    check_set "$broken"

break_set true
LC_ALL=C sort -r "$work/set/$RELEASE_SET_CHECKSUMS" > "$broken/$RELEASE_SET_CHECKSUMS"
expect_fail 'a checksums file in another order is caught' 'does not say what the archives hash to' \
    check_set "$broken"

break_set true
sed 's/^/\\/' "$work/set/$RELEASE_SET_CHECKSUMS" > "$broken/$RELEASE_SET_CHECKSUMS"
expect_fail 'a checksums file whose lines are escaped is caught' 'escaped line' check_set "$broken"

break_set true
sed 's/$/\r/' "$work/set/$RELEASE_SET_CHECKSUMS" > "$broken/$RELEASE_SET_CHECKSUMS"
expect_fail 'a checksums file written in text mode is caught' 'carriage returns' check_set "$broken"

break_set true
sed 's/^[0-9a-f]\{4\}/0000/' "$work/set/$RELEASE_SET_CHECKSUMS" > "$broken/$RELEASE_SET_CHECKSUMS"
expect_fail 'a checksums file with a digest nothing hashes to is caught' \
    'does not say what the archives hash to' check_set "$broken"

# ---- and the documents themselves --------------------------------------
break_set rm -f "$broken/$RELEASE_SET_NAME"
expect_fail 'a set with no release set document is caught' 'there is no release set' \
    check_set "$broken"

break_set rm -f "$broken/$RELEASE_SET_CHECKSUMS"
expect_fail 'a set with no checksums file is caught' 'there is no release set' check_set "$broken"

checks=$((checks + 1))
rm -rf "$work/three-archives-only"
mkdir -p "$work/three-archives-only"
cp "$linux_archive" "$macos_archive" "$windows_archive" "$work/three-archives-only/"
if tools/check-release-set.sh --release-set "$work/three-archives-only" --forbidden "$absent" \
        --output "$work/facts.txt" > "$work/out.txt" 2>&1; then
    fail 'the checker accepted a directory that is only three archives'
elif LC_ALL=C grep -qF 'there is no release set' "$work/out.txt"; then
    echo "  ok      the checker says what is missing when there are only archives"
else
    fail 'the checker refused three loose archives for some other reason'
    sed 's/^/      /' "$work/out.txt" >&2
fi

# ---------------------------------------------------------------------------
# Release sets the builder did not assemble.
# ---------------------------------------------------------------------------
#
# Everything above hands the checker something the builder made. That is the
# easy half: the builder has already refused most of these, so the checker is
# only being asked to agree. The question this section asks is the one the
# checker exists for - it verifies an output, and an output can arrive from
# anywhere. Each of these is a directory written here, from archives the
# builder would have refused, with a document and a checksums file that agree
# with the bytes perfectly. Nothing about them is malformed. What they are is
# not a release set.

echo "== the checker, on a set nothing in this repository assembled"

hand_assemble() { # directory triple:archive...
    local where="$1"; shift
    local pair triple archive root
    rm -rf "$where"; mkdir -p "$where"
    : > "$work/hand-targets"
    : > "$where/$RELEASE_SET_CHECKSUMS"
    for pair in "$@"; do
        triple="${pair%%:*}"
        archive="${pair#*:}"
        cp "$archive" "$where/$(basename "$archive")"
        root="$(package_root_for "$version" "$triple")"
        jq -n --arg target "$triple" \
              --arg archive "$(basename "$archive")" \
              --arg sha256 "$(package_sha256 "$archive")" \
              --argjson size "$(package_size "$archive")" \
              --arg root "$root" \
              --arg packageManifestSha256 "$(manifest_digest_of "$archive")" \
              '{$target, $archive, $sha256, $size, $root, $packageManifestSha256}' \
              >> "$work/hand-targets"
        printf '%s  %s\n' "$(package_sha256 "$archive")" "$(basename "$archive")" \
            >> "$where/$RELEASE_SET_CHECKSUMS"
    done
    LC_ALL=C sort -k2 -o "$where/$RELEASE_SET_CHECKSUMS" "$where/$RELEASE_SET_CHECKSUMS"
    jq -s -S --arg kind "$RELEASE_SET_KIND" \
          --argjson formatVersion "$RELEASE_SET_FORMAT" \
          --arg productVersion "$version" \
          --arg sourceRevision "$FIXTURE_REVISION" \
          --argjson packageManifestFormat "$PACKAGE_FORMAT" \
          --arg packageManifestPath "$(package_manifest_path)" \
          --arg checksums "$RELEASE_SET_CHECKSUMS" \
          --slurpfile targets "$work/hand-targets" \
          '{kind: $kind, formatVersion: $formatVersion, productVersion: $productVersion,
            sourceRevision: $sourceRevision, packageManifestFormat: $packageManifestFormat,
            packageManifestPath: $packageManifestPath, checksums: $checksums,
            targets: ($targets | sort_by(.target))}' /dev/null \
        | native_strip_cr > "$where/$RELEASE_SET_NAME"
}

manifest_digest_of() { # archive
    local root
    rm -rf "$work/peek"; mkdir -p "$work/peek"
    root="$("$(package_gnu_tar)" -tf "$1" | sed 's|/.*||' | LC_ALL=C sort -u | head -1)"
    "$(package_gnu_tar)" -C "$work/peek" -xzf "$1" "$root/$(package_manifest_path)"
    package_sha256 "$work/peek/$root/$(package_manifest_path)"
}

# A hand-assembled set of the three real archives verifies, so the cases below
# fail for the reason they are about rather than because they were written here.
expect_pass 'a set assembled by hand out of the three real archives verifies' \
    check_hand "$linux_archive" "$macos_archive" "$windows_archive"

# One archive of the format that carried no revision, in a set whose document
# is otherwise perfect. The builder refuses to make this; a checker that only
# ever saw the builder's output would never be asked.
hand_assemble "$work/hand-old-format" \
    "$(package_triple_for linux):$linux_archive" \
    "$(package_triple_for macos):$macos_archive" \
    "$(package_triple_for windows):$old_format"
expect_fail 'a hand-assembled set carrying a format 1 archive is caught' \
    'which are the ones that say' check_set "$work/hand-old-format"

# An archive whose manifest says another product version, in a set named for
# this one. Its file name and its root are the right ones, so nothing outside
# the manifest gives it away.
root="$(explode "$windows_archive")"
jq -S --arg v "9.9.9" '.productVersion = $v' \
    "$tree/$root/$(package_manifest_path)" | native_strip_cr > "$work/m.json"
mv "$work/m.json" "$tree/$root/$(package_manifest_path)"
lying_version="$(repack_as "$root" "$(basename "$windows_archive")")"
hand_assemble "$work/hand-lying-version" \
    "$(package_triple_for linux):$linux_archive" \
    "$(package_triple_for macos):$macos_archive" \
    "$(package_triple_for windows):$lying_version"
expect_fail 'a hand-assembled set whose archive says another version is caught' \
    'and the release set says' check_set "$work/hand-lying-version"

# And a set whose third member is for a target the product does not have.
hand_assemble "$work/hand-unknown-target" \
    "$(package_triple_for linux):$linux_archive" \
    "$(package_triple_for macos):$macos_archive" \
    "$unknown_target:$fourth_target"
expect_fail 'a hand-assembled set naming a target the product does not have is caught' \
    'unknown:' check_set "$work/hand-unknown-target"

# ---------------------------------------------------------------------------
# And that something consumes all this.
# ---------------------------------------------------------------------------
#
# A checker nothing runs is a file. The combined runtime layout workflow is the
# only thing that has three real platform archives, so it is the only place a
# release set can be assembled from them, and these are the facts it has to
# require of the result.

echo "== the workflow that consumes it"

workflow='.github/workflows/runtime-layout.yml'
for wanted in 'tools/build-release-set.sh' 'tools/check-release-set.sh' \
              'release-set archives-verified=3' 'release-set files=5 extra=0' \
              'release-set checksums=3 order=sorted escaping=none' \
              'package source-revision='; do
    checks=$((checks + 1))
    if LC_ALL=C grep -qF "$wanted" "$workflow"; then
        echo "  ok      $workflow uses '$wanted'"
    else
        fail "$workflow does not use '$wanted', so nothing consumes it"
    fi
done

report_and_exit 0
