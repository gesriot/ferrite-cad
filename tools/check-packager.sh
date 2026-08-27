#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The packager's gates, on a staging fixture this script builds itself.
#
# The real proof is the three-platform one: a package that has never been
# extracted on a machine with no build tree and started there proves nothing.
# But most of the ways a packager goes wrong are not about running. A file with
# two owners, a file with none, a manifest that describes the bytes it was
# written from rather than the bytes that came out, an absolute path, a symlink
# where a payload file should be, a lost executable bit, an archive with two
# top-level directories - every one of those is decided by arithmetic over a
# directory, and waiting for a runner to build Open CASCADE before asking about
# them would mean asking about them roughly never.
#
# So this builds a staging directory with the real names and fake bytes, runs
# the real packager over it, and then breaks the result in one specific way at
# a time and requires the real gate to say which way. Nothing here runs a
# product binary: --no-execute says so in the facts, and the workflow's
# comparison requires facts only a real run can produce.
#
# Runs no network.
#
# Run from the repository root:
#   tools/check-packager.sh              # all three platforms
#   tools/check-packager.sh --platform macos

set -euo pipefail

PACKAGE_TOOL='check-packager'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/package/lib.sh
. tools/package/lib.sh

platforms=("${NATIVE_PLATFORMS[@]}")
while [ $# -gt 0 ]; do
    case "$1" in
        --platform) platforms=("${2:?--platform needs a name}"); shift 2 ;;
        *) package_die "unknown argument: $1" ;;
    esac
done

native_require_jq
# Asserted here, so a host without it says so before it builds a fixture.
package_gnu_tar > /dev/null

# Asked before a fixture is built, and named as the defect rather than as a
# missing file. Up to §21A-2b2b1a the relocatable layout existed only as a
# directory in the temporary space of the run that measured it: it could be
# started with the build trees taken away, and there was nothing to hand
# anybody. That is what is missing here, not a script.
if [ ! -x tools/package-release.sh ]; then
    echo "$PACKAGE_TOOL: there is no packager, so a measured runtime layout has no delivery." >&2
    echo >&2
    echo "  tools/stage-runtime-layout.sh produces a directory in the runner's temporary" >&2
    echo "  space. It carries no version in its name, there is no archive around it," >&2
    echo "  nothing verifies it once it has been moved, the product SBOM is not inside" >&2
    echo "  it, and it is deleted with the runner. Nothing exists that can be extracted" >&2
    echo "  somewhere else and started there." >&2
    echo >&2
    echo "  tools/package-release.sh is what would produce one." >&2
    exit 1
fi

work="$(mktemp -d)"
# shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

checks=0
failures=0
fail() { failures=$((failures + 1)); echo "$PACKAGE_TOOL: $*" >&2; }

# A run that asserted nothing is not a pass. Counted rather than trusted,
# because the cheapest way for this file to stop being a gate is for its cases
# to stop being reached.
expect_pass() { # description command...
    local what="$1"; shift
    checks=$((checks + 1))
    if "$@" > "$work/out.txt" 2>&1; then
        echo "  ok      $what"
    else
        fail "$what: expected success, got exit $?"
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
# A staging directory with the real names in it.
#
# The names come from the inventory rather than from a list here, so a target
# that gains or loses a library is a fixture that gains or loses it too. The
# bytes are made up and are not pretending otherwise: what is being gated is
# the arithmetic over the directory, and the three-platform workflow packs the
# real ones.
# ---------------------------------------------------------------------------

make_fixture() { # platform directory
    local platform="$1" directory="$2" triple path
    triple="$(package_triple_for "$platform")"
    rm -rf "$directory"
    jq -r --arg t "$triple" \
        '.targets[] | select(.triple == $t) | .stagedFiles[] | .path' \
        "$NATIVE_INVENTORY" | native_strip_cr | LC_ALL=C sort > "$work/fixture-paths"
    [ -s "$work/fixture-paths" ] || package_die "the inventory stages nothing for $triple"
    while IFS= read -r path; do
        mkdir -p "$directory/$(dirname "$path")"
        # Distinct per path, so a gate that mixed two files up would see two
        # different digests rather than one that happened to match.
        printf 'fixture bytes for %s\n' "$path" > "$directory/$path"
        chmod 755 "$directory/$path"
    done < "$work/fixture-paths"
}

# ---------------------------------------------------------------------------
# One platform, from a staging directory to an archive and then to every way
# the archive can be wrong.
# ---------------------------------------------------------------------------

gate_platform() { # platform
    local platform="$1" triple version root archive
    triple="$(package_triple_for "$platform")"
    version="$(jq -r '.productVersion' "$NATIVE_INVENTORY" | native_strip_cr)"
    root="$(package_root_for "$version" "$triple")"
    archive="$(package_archive_for "$version" "$triple")"

    local staging="$work/$platform/staging"
    local out="$work/$platform/out"
    local scratch="$work/$platform/scratch"
    rm -rf "${work:?}/$platform"
    mkdir -p "$out" "$scratch"
    make_fixture "$platform" "$staging"

    echo "== $platform ($triple)"

    # ---- the packager, on a staging directory that is exactly right --------
    expect_pass 'the packager turns a staged layout into a versioned archive' \
        tools/package-release.sh --platform "$platform" --staging "$staging" --output-dir "$out"

    if [ ! -f "$out/$archive" ]; then
        checks=$((checks + 1))
        fail "no archive at $out/$archive, so nothing below can be asked"
        return
    fi

    # ---- two packings of the same bytes ------------------------------------
    checks=$((checks + 1))
    local second="$work/$platform/second"
    mkdir -p "$second"
    if ! tools/package-release.sh --platform "$platform" --staging "$staging" \
            --output-dir "$second" > "$work/out.txt" 2>&1; then
        fail 'the second packing failed'
        sed 's/^/      /' "$work/out.txt" >&2
    elif cmp -s "$out/$archive" "$second/$archive"; then
        echo "  ok      two packings of the same bytes are byte-identical"
    else
        fail 'two packings of the same input bytes produced different archives'
    fi

    # ---- and it verifies ---------------------------------------------------
    local checker=(tools/check-release-package.sh --platform "$platform" --no-execute
                   --forbidden "$staging")
    verify() { # archive extract-dir
        rm -rf "$2"
        "${checker[@]}" --archive "$1" --extract-to "$2" --output "$work/facts.txt"
    }

    # The staging directory goes away first. A gate that could still reach it
    # would be measuring the previous slice's question.
    local kept="$work/$platform/staging-kept"
    mv "$staging" "$kept"

    expect_pass 'the extracted archive verifies against its manifest' \
        verify "$out/$archive" "$scratch/good"

    # Everything below reads the manifest of the package that was just
    # verified. A run that got this far without one must say so and stop
    # rather than fail at the first `jq`: a gate that dies mid-way reports no
    # count, and a run that asserted an unknown number of things is not
    # evidence about anything.
    if [ ! -f "$scratch/good/$root/$(package_manifest_path)" ]; then
        checks=$((checks + 1))
        fail "no verified package to mutate; the cases below were never asked"
        return
    fi

    # The schema is the definition of the manifest's shape, and it is a real
    # validator rather than a parser written here. Pinned like the CycloneDX
    # one, for the reason a gate that cannot fail is not a gate.
    checks=$((checks + 1))
    if ! command -v jsonschema-cli >/dev/null 2>&1; then
        fail "jsonschema-cli is not installed; the shape of the manifest cannot be checked"
    else
        . tools/sbom/pin.env
        local found
        found="$(jsonschema-cli --version 2>/dev/null | awk '{print $NF}')"
        if [ "$found" != "$JSONSCHEMA_CLI_VERSION" ]; then
            fail "jsonschema-cli $found is installed but $JSONSCHEMA_CLI_VERSION is pinned"
        elif jsonschema-cli validate "$PACKAGE_SCHEMA" \
                -i "$scratch/good/$root/$(package_manifest_path)" \
                > "$work/schema.log" 2>&1; then
            echo "  ok      the manifest matches $PACKAGE_SCHEMA"
        else
            fail "the manifest does not match $PACKAGE_SCHEMA:"
            sed 's/^/      /' "$work/schema.log" >&2
        fi
    fi

    # What it said, so a silently smaller answer is visible.
    checks=$((checks + 1))
    if grep -q "^package runtime-files=[1-9]" "$work/facts.txt" \
        && grep -q '^package product-sbom=byte-identical' "$work/facts.txt" \
        && grep -q '^package manifest-verified-against-extracted-bytes=true' "$work/facts.txt" \
        && grep -q '^package execution=skipped' "$work/facts.txt"; then
        echo "  ok      the facts name the payload, the SBOM and the skipped run"
    else
        fail 'the facts do not say what was measured'
        sed 's/^/      /' "$work/facts.txt" >&2
    fi

    # ---- the staging directory, broken in the ways a staging can be --------
    local broken="$work/$platform/broken"
    break_staging() { # mutate-command...
        rm -rf "$broken"
        cp -R "$kept" "$broken"
        "$@"
    }
    pack_broken() {
        tools/package-release.sh --platform "$platform" --staging "$broken" \
            --output-dir "$work/$platform/reject"
    }

    mutate_unowned()  { touch "$broken/an-unaccounted-file"; }
    mutate_missing()  { rm -f "$broken/$first_staged"; }
    mutate_import()   { touch "$broken/$NATIVE_IMPORT_LIBRARY"; }
    mutate_symlink()  { ln -s "$first_staged" "$broken/a-link"; }
    mutate_metadata() { mkdir -p "$broken/$PACKAGE_METADATA_DIR"; \
                        printf 'x\n' > "$broken/$PACKAGE_METADATA_DIR/x"; }

    local first_staged
    first_staged="$(jq -r --arg t "$triple" \
        '[.targets[] | select(.triple == $t) | .stagedFiles[] | .path] | sort | .[0]' \
        "$NATIVE_INVENTORY" | native_strip_cr)"

    break_staging mutate_unowned
    expect_fail 'a staged file nobody owns is refused' 'no owner' pack_broken

    break_staging mutate_missing
    expect_fail 'a staged file the inventory promises and staging lacks is refused' \
        'promises' pack_broken

    break_staging mutate_import
    expect_fail 'the import library in the staging is refused' \
        "$NATIVE_IMPORT_LIBRARY" pack_broken

    break_staging mutate_symlink
    expect_fail 'a symlink in the staging is refused' 'regular file' pack_broken

    break_staging mutate_metadata
    expect_fail 'a staged file where package metadata goes is refused' \
        "$PACKAGE_METADATA_DIR" pack_broken

    # ---- the archive, broken in the ways an archive can be -----------------
    #
    # Each of these starts from the package the real packager produced, changes
    # exactly one thing, packs it again the way the packager packs, and requires
    # the real gate to name what changed.

    local tree="$work/$platform/tree"
    # Named what the manifest names, so a mutation is tested for the thing it
    # mutated rather than caught by the archive's own filename.
    local baddir="$work/$platform/badpack"
    local bad="$baddir/$archive"
    mkdir -p "$baddir"
    local manifest_rel
    manifest_rel="$root/$(package_manifest_path)"
    local a_runtime
    a_runtime="$(jq -r '.runtimeFiles[0].path' "$scratch/good/$manifest_rel" | native_strip_cr)"

    reset_tree() {
        rm -rf "$tree"; mkdir -p "$tree"
        cp -R "$scratch/good/$root" "$tree/$root"
    }
    edit_manifest() { # jq-program
        jq -S "$1" "$tree/$manifest_rel" | native_strip_cr > "$work/m.json"
        mv "$work/m.json" "$tree/$manifest_rel"
    }
    repack() { package_create_archive "$tree" "$root" "$bad"; }
    check_bad() {
        rm -rf "$scratch/bad"
        "${checker[@]}" --archive "$bad" --extract-to "$scratch/bad" --output "$work/facts.txt"
    }
    break_archive() { # mutate-function
        reset_tree; "$1"; repack
    }

    mutate_bytes()    { printf 'tampered\n' > "$tree/$root/$a_runtime"; }
    mutate_truncate() { : > "$tree/$root/$a_runtime"; }
    mutate_remove()   { rm -f "$tree/$root/$a_runtime"; }
    mutate_extra()    { printf 'extra\n' > "$tree/$root/an-extra-file"; }
    mutate_twice()    { edit_manifest '.runtimeFiles += [.runtimeFiles[0]]'; }
    mutate_no_owner() { edit_manifest '.runtimeFiles[0].owner = ""'; }
    mutate_bad_kind() { edit_manifest '.runtimeFiles[0].ownerKind = "somebody"'; }
    mutate_target()   { edit_manifest '.target = "aarch64-unknown-linux-gnu"'; }
    mutate_self()     { edit_manifest '.selfDescription.path = "package/somewhere-else.json"'; }
    mutate_selfhash() { edit_manifest '.selfDescription.hashed = true'; }
    mutate_import_l() {
        local file="$tree/$root/$NATIVE_IMPORT_LIBRARY"
        printf 'an import library\n' > "$file"
        edit_manifest "$(printf '.runtimeFiles += [{"path":"%s","sha256":"%s","size":%s,"executable":%s,"owner":"native+planegcs-import-library@1","ownerKind":"component"}]' \
            "$NATIVE_IMPORT_LIBRARY" "$(package_sha256 "$file")" "$(package_size "$file")" \
            "$(package_is_executable "$file")")"
    }
    mutate_wrong_sbom() {
        local other packaged
        other="$(jq -r --arg t "$triple" \
            '[.targets[].triple | select(. != $t)] | sort | .[0]' "$NATIVE_INVENTORY" | native_strip_cr)"
        packaged="$tree/$root/$(package_sbom_path_for "$triple")"
        cp "$(product_output_for "$other")" "$packaged"
        edit_manifest "$(printf '.productSbom.sha256 = "%s" | (.packageMetadata[] | select(.path == "%s") | .sha256) = "%s" | (.packageMetadata[] | select(.path == "%s") | .size) = %s' \
            "$(package_sha256 "$packaged")" "$(package_sbom_path_for "$triple")" \
            "$(package_sha256 "$packaged")" "$(package_sbom_path_for "$triple")" \
            "$(package_size "$packaged")")"
    }

    break_archive mutate_bytes
    expect_fail 'a payload byte changed after the manifest was written is caught' \
        'digest' check_bad

    break_archive mutate_truncate
    expect_fail 'a truncated payload file is caught' 'size' check_bad

    break_archive mutate_remove
    expect_fail 'a payload file missing from the archive is caught' \
        'only in the manifest' check_bad

    break_archive mutate_extra
    expect_fail 'a payload file the manifest does not describe is caught' \
        'only in the archive' check_bad

    break_archive mutate_twice
    expect_fail 'a payload file claimed twice is caught' 'two owners' check_bad

    break_archive mutate_no_owner
    expect_fail 'a payload file with no owner is caught' 'no owner' check_bad

    break_archive mutate_bad_kind
    expect_fail 'a payload file whose owner is of no known kind is caught' \
        'no known kind' check_bad

    break_archive mutate_target
    expect_fail "a manifest for another target is caught" 'the manifest is for' check_bad

    break_archive mutate_self
    expect_fail 'the no-digest exception claimed for another file is caught' \
        'exempts' check_bad

    break_archive mutate_selfhash
    expect_fail 'a manifest claiming to hash its own bytes is caught' \
        'impossible' check_bad

    break_archive mutate_import_l
    expect_fail 'the import library inside the archive is caught' \
        "$NATIVE_IMPORT_LIBRARY" check_bad

    break_archive mutate_wrong_sbom
    expect_fail "another target's product SBOM inside the package is caught" \
        'byte for byte' check_bad

    # ---- archives no packager would write, and the gate must still refuse ---
    #
    # Written directly rather than through package_create_archive, because the
    # question is whether the gate refuses an archive that came from somewhere
    # else. A gate that could only fail archives its own packager produced
    # would pass anything that did not come from it.

    craft() { # deformation
        reset_tree
        python3 tools/package/craft-archive.py "$tree" "$root" "$bad" "$1"
    }

    craft no-exec-bit
    expect_fail 'an archive that lost the executable bit is caught' \
        'executable' check_bad

    craft two-roots
    expect_fail 'an archive with two top-level directories is caught' \
        'more than one top-level' check_bad

    craft unsorted
    expect_fail 'an archive whose entries are not in normalised order is caught' \
        'normalised order' check_bad

    craft varying-mtimes
    expect_fail 'an archive with more than one entry timestamp is caught' \
        'more than one entry timestamp' check_bad

    craft absolute-path
    expect_fail 'an archive holding an absolute path is caught' \
        'absolute path' check_bad

    craft parent-traversal
    expect_fail 'an archive holding a parent traversal is caught' \
        'parent traversal' check_bad

    craft symlink-payload
    expect_fail 'a symlink where a payload file should be is caught' \
        'neither a regular file nor a directory' check_bad

    # ---- and the gate must not be satisfiable by the staging directory -----
    checks=$((checks + 1))
    rm -rf "$scratch/nostaging"
    if tools/check-release-package.sh --platform "$platform" --no-execute \
            --forbidden "$kept" --archive "$out/$archive" \
            --extract-to "$scratch/nostaging" --output "$work/facts.txt" \
            > "$work/out.txt" 2>&1; then
        fail 'the gate accepted a run in which the staging directory still existed'
    elif LC_ALL=C grep -qF 'still exists' "$work/out.txt"; then
        echo "  ok      the gate refuses to run while the staging directory is still there"
    else
        fail 'the gate refused the still-present staging directory for some other reason'
        sed 's/^/      /' "$work/out.txt" >&2
    fi

    checks=$((checks + 1))
    rm -rf "$scratch/noarchive"
    if tools/check-release-package.sh --platform "$platform" --no-execute \
            --forbidden "$staging" --archive "$work/$platform/there-is-none.tar.gz" \
            --extract-to "$scratch/noarchive" --output "$work/facts.txt" \
            > "$work/out.txt" 2>&1; then
        fail 'the gate passed with no archive at all'
    elif LC_ALL=C grep -qF 'there is no release archive' "$work/out.txt"; then
        echo "  ok      the gate says what is missing when there is no archive"
    else
        fail 'the gate failed without saying there was no archive'
        sed 's/^/      /' "$work/out.txt" >&2
    fi
}

for platform in "${platforms[@]}"; do
    gate_platform "$platform"
done

echo
if [ "$checks" -eq 0 ]; then
    package_die 'this run asserted nothing at all'
fi
if [ "$failures" -ne 0 ]; then
    echo "$PACKAGE_TOOL: $failures of $checks checks failed" >&2
    exit 1
fi
echo "$PACKAGE_TOOL: $checks checks, on ${#platforms[@]} platform(s), all of them passed"
