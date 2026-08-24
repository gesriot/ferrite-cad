#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Lays a measured runtime closure out the way a relocatable release would have
# to, in a directory that is not a release.
#
# §21A-2b2a measures; §21A-2b2b packages. The difference is not cosmetic. What
# this script produces goes into the runner's temporary directory, carries no
# version, no archive, no signature anyone should rely on and no installer, and
# exists so the layout can be *run* from a clean environment before anyone
# writes the packager that would produce it for real. A candidate layout that
# has never been started with the build tree taken away is a guess.
#
# The rules here are the real ones rather than a placeholder, because a
# placeholder would prove nothing: the whole question is whether an executable
# that finds its libraries through `$ORIGIN`, `@rpath` or its own directory
# actually starts.
#
#   Linux    bin/<executables>, lib/<libraries>
#            executables look through $ORIGIN/../lib
#            shipped libraries look through $ORIGIN, so a toolkit finds its
#            neighbours relative to itself rather than to whoever loaded it
#
#   macOS    <name>.app/Contents/MacOS, <name>.app/Contents/Frameworks
#            LC_RPATH @executable_path/../Frameworks on the executables and
#            @loader_path on the libraries; every shipped library's install
#            name rewritten to @rpath/<file>
#            re-signed ad hoc, because editing load commands invalidates the
#            signature and arm64 refuses to start an image whose signature does
#            not match. That is a fact about running from a temporary
#            directory; it is not a notarisation claim and not a distribution
#            signature.
#
#   Windows  bin/ with the executables and the DLLs beside them, which is the
#            only layout the loader resolves without an environment variable
#
# It refuses to stage a library nobody has accounted for. An unexpected member
# of the closure is a licence question and a packaging question at once, and
# copying it quietly would answer both by accident.
#
# Usage:
#   tools/stage-runtime-layout.sh --platform linux|macos|windows \
#       --staging DIR --occt-lib-dir DIR --planegcs-dir DIR \
#       --closure closure-viewer.txt [--closure closure-cli.txt]... \
#       --executable path/to/ferritecad-viewer [--executable ...]...

set -euo pipefail

platform=''
staging=''
occt_lib_dir=''
planegcs_dir=''
closures=()
executables=()

die() {
    echo "stage-runtime-layout: $*" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --platform)      platform="${2:-}"; shift 2 ;;
        --staging)       staging="${2:-}"; shift 2 ;;
        --occt-lib-dir)  occt_lib_dir="${2:-}"; shift 2 ;;
        --planegcs-dir)  planegcs_dir="${2:-}"; shift 2 ;;
        --closure)       closures+=("${2:-}"); shift 2 ;;
        --executable)    executables+=("${2:-}"); shift 2 ;;
        *) die "unknown argument $1" ;;
    esac
done

[ -n "$platform" ] || die 'no --platform'
[ -n "$staging" ] || die 'no --staging'
[ -n "$occt_lib_dir" ] || die 'no --occt-lib-dir'
[ -n "$planegcs_dir" ] || die 'no --planegcs-dir'
[ "${#closures[@]}" -gt 0 ] || die 'no --closure report to stage from'
[ "${#executables[@]}" -gt 0 ] || die 'no --executable to stage'
[ -d "$occt_lib_dir" ] || die "no such directory: $occt_lib_dir"
[ -d "$planegcs_dir" ] || die "no such directory: $planegcs_dir"

for closure in "${closures[@]}"; do
    [ -f "$closure" ] || die "no such closure report: $closure"
done
for executable in "${executables[@]}"; do
    [ -f "$executable" ] || die "no such executable: $executable"
done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# ---------------------------------------------------------------------------
# What the closure said has to be carried.
# ---------------------------------------------------------------------------

cat "${closures[@]}" | awk '$1 == "dep"' > "$work/deps"
[ -s "$work/deps" ] || die 'the closure reports name no dependency at all'

awk '$4 == "unexpected" { print $5 }' "$work/deps" | LC_ALL=C sort -u > "$work/unexpected"
if [ -s "$work/unexpected" ]; then
    echo "stage-runtime-layout: the closure holds libraries nobody has accounted for:" >&2
    sed 's/^/  /' "$work/unexpected" >&2
    echo >&2
    echo "Each is a licence question and a packaging question at the same time." >&2
    echo "§21A-2b2a stops here and reports rather than copying them quietly." >&2
    exit 1
fi

awk '$4 == "occt" || $4 == "planegcs" { print $4 "\t" $5 }' "$work/deps" \
    | LC_ALL=C sort -u > "$work/carry"
[ -s "$work/carry" ] || die 'the closure names no Open CASCADE and no planegcs'
grep -q '^occt' "$work/carry" || die 'the closure names no Open CASCADE toolkit'
grep -q '^planegcs' "$work/carry" || die 'the closure names no planegcs'

# ---------------------------------------------------------------------------
# The layout.
# ---------------------------------------------------------------------------

rm -rf "$staging"
case "$platform" in
    linux)
        bin_dir="$staging/bin"
        lib_dir="$staging/lib" ;;
    macos)
        bin_dir="$staging/FerriteCAD.app/Contents/MacOS"
        lib_dir="$staging/FerriteCAD.app/Contents/Frameworks" ;;
    windows)
        bin_dir="$staging/bin"
        lib_dir="$staging/bin" ;;
    *) die "unknown platform $platform" ;;
esac
mkdir -p "$bin_dir" "$lib_dir"

for executable in "${executables[@]}"; do
    cp "$executable" "$bin_dir/"
    chmod u+w "$bin_dir/$(basename "$executable")"
done

# Copied under the name the closure said was referenced, with symlinks
# followed. A shipped library has to answer to the name its dependents wrote
# down — the soname on Linux, the install name on macOS — and a chain of links
# in a package is a chain of things that can be lost by an archiver.
while IFS="$(printf '\t')" read -r class name; do
    [ -n "$name" ] || continue
    case "$class" in
        occt)     source_dir="$occt_lib_dir" ;;
        planegcs) source_dir="$planegcs_dir" ;;
        *) die "no source directory for a $class library" ;;
    esac
    if [ ! -f "$source_dir/$name" ]; then
        die "the closure names $name and $source_dir has no such file"
    fi
    cp -L "$source_dir/$name" "$lib_dir/$name"
    chmod u+w "$lib_dir/$name"
done < "$work/carry"

# ---------------------------------------------------------------------------
# Making it find them.
# ---------------------------------------------------------------------------

stage_linux() {
    command -v patchelf >/dev/null 2>&1 \
        || die 'patchelf is needed to set a relative run path and is not installed'

    # $ORIGIN is the loader's own token and has to reach it unexpanded; a
    # shell that substituted it would write the build machine's path.
    # shellcheck disable=SC2016
    {
        local file
        for file in "$bin_dir"/*; do
            [ -f "$file" ] || continue
            # --set-rpath replaces rather than appends, so whatever absolute
            # RUNPATH the build tree left is gone rather than merely outvoted.
            patchelf --set-rpath '$ORIGIN/../lib' "$file"
        done
        for file in "$lib_dir"/*; do
            [ -f "$file" ] || continue
            patchelf --set-rpath '$ORIGIN' "$file"
        done
    }
}

stage_macos() {
    local file base name existing

    for file in "$lib_dir"/*; do
        [ -f "$file" ] || continue
        base="$(basename "$file")"
        install_name_tool -id "@rpath/$base" "$file"
    done

    for file in "$bin_dir"/* "$lib_dir"/*; do
        [ -f "$file" ] || continue

        # Every absolute or otherwise build-tree-relative reference to a
        # library this package carries becomes a reference to the package.
        otool -L "$file" | sed -n '2,$p' | sed 's/ (compatibility.*//; s/^[[:space:]]*//' \
            | while IFS= read -r dependency; do
                [ -n "$dependency" ] || continue
                name="$(basename "$dependency")"
                [ -f "$lib_dir/$name" ] || continue
                [ "$dependency" != "@rpath/$name" ] || continue
                install_name_tool -change "$dependency" "@rpath/$name" "$file"
            done

        # One run path, and this one. Whatever the build tree left is deleted
        # first, so the package cannot be resolved by an absolute path that
        # happens to still exist on the machine that built it.
        otool -l "$file" | awk '
            /^ *cmd LC_RPATH$/ { want = 1; next }
            want && $1 == "path" { print $2; want = 0 }
        ' > "$work/existing-rpaths"
        while IFS= read -r existing; do
            [ -n "$existing" ] || continue
            install_name_tool -delete_rpath "$existing" "$file" 2>/dev/null || true
        done < "$work/existing-rpaths"

        case "$file" in
            "$bin_dir"/*) install_name_tool -add_rpath '@executable_path/../Frameworks' "$file" ;;
            *)            install_name_tool -add_rpath '@loader_path' "$file" ;;
        esac
    done

    # Last, and only after every load command is final. Editing a Mach-O
    # invalidates its signature, and an arm64 image whose signature does not
    # match is killed by the kernel before dyld gets a turn — which looks
    # exactly like a missing library and is not one.
    for file in "$bin_dir"/* "$lib_dir"/*; do
        [ -f "$file" ] || continue
        codesign --force --sign - --timestamp=none "$file" >/dev/null 2>&1 \
            || die "ad-hoc signing failed for $file"
    done
}

stage_windows() {
    # Nothing to rewrite. A PE image carries no run path: the loader searches
    # the directory of the executable first, and the layout above is what makes
    # that enough.
    :
}

case "$platform" in
    linux)   stage_linux ;;
    macos)   stage_macos ;;
    windows) stage_windows ;;
esac

# ---------------------------------------------------------------------------
# What was staged.
# ---------------------------------------------------------------------------

# Printed, not written into the staging directory. What is staged is what a
# package would carry, and a report about the package is not part of it: a
# later step greps every staged file for paths into the build tree, and a
# listing that mentioned them would be the only file that failed.
echo "# FerriteCAD candidate runtime layout"
echo "platform $platform"
echo "note this is a measurement staging directory, not a release"
find "$staging" -type f | LC_ALL=C sort | while IFS= read -r file; do
    printf 'staged %s %s\n' "${file#"$staging"/}" "$(wc -c < "$file" | tr -d ' ')"
done
# `wc -c` prints a "total" line per batch of arguments, and xargs decides how
# many batches there are. Counting those lines as files would make the answer
# depend on the length of the staging path.
find "$staging" -type f -print0 | xargs -0 wc -c \
    | awk '$2 != "total" { files++; bytes += $1 }
           END { printf "staged-total files=%d bytes=%d\n", files, bytes }'
