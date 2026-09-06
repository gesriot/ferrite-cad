# SPDX-License-Identifier: MIT
# shellcheck shell=bash
#
# A staging directory with the real names in it and invented bytes. Sourced,
# never run.
#
# Two gates need one. tools/check-packager.sh breaks a package one way at a
# time and requires the real gate to name what broke; the release set builder's
# gate makes three archives and breaks the set. Neither of them runs a product
# binary, and neither needs to: what they gate is arithmetic over a directory
# and over three files, and waiting for a runner to build Open CASCADE before
# asking about any of it would mean asking about it roughly never.
#
# They had a copy each of this loop, which was fine while every staged file was
# a program with made-up bytes. It stopped being fine when the macOS layout
# gained files that are neither: `Contents/Info.plist` is read rather than only
# hashed, and it and the bundle signature beside it are not executable, which
# is the property that tells a product root's application apart from the other
# files it owns. Two copies would be two fixtures that could disagree about
# that, and the one that was wrong would be the one that passed.
#
# The names come from the inventory rather than from a list here, so a target
# that gains or loses a library is a fixture that gains or loses it too.
#
# What is deliberately not reproduced is the ad-hoc bundle signature. Nothing
# either gate asks reads it, because a signature is a statement about the real
# product: the workflow that stages the real bundle is where it is written and
# where it is verified.

# shellcheck source=tools/macos-bundle.sh
. tools/macos-bundle.sh

# The Info.plist a fixture bundle carries.
#
# Written here rather than borrowed from tools/stage-runtime-layout.sh on
# purpose: a fixture that called the real writer could not tell a checker that
# had stopped checking from a writer that had stopped writing, which is the
# same reason tools/native/lib.sh restates the layout directories instead of
# importing them. The identifier is made up and says so.
package_fixture_plist() { # destination version
    local executable
    executable="$(jq -r '.productRoots[] | select(.package == "ferritecad-app") | .binary' \
        "$NATIVE_INVENTORY" | native_strip_cr)"
    [ -n "$executable" ] \
        || package_die "$NATIVE_INVENTORY does not name a product root built from ferritecad-app"
    cat > "$1" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>$executable</string>
	<key>CFBundleIdentifier</key>
	<string>example.fixture.FerriteCAD</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>FerriteCAD</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$2</string>
	<key>CFBundleVersion</key>
	<string>$2</string>
</dict>
</plist>
PLIST
}

package_fixture_staging() { # platform directory version
    local platform="$1" directory="$2" version="$3" triple path
    triple="$(package_triple_for "$platform")"
    rm -rf "$directory"

    local bundle_files="$directory.bundle-files"
    native_bundle_files_for "$platform" > "$bundle_files"
    local paths="$directory.paths"
    jq -r --arg t "$triple" \
        '.targets[] | select(.triple == $t) | .stagedFiles[] | .path' \
        "$NATIVE_INVENTORY" | native_strip_cr | LC_ALL=C sort > "$paths"
    [ -s "$paths" ] || package_die "the inventory stages nothing for $triple"

    while IFS= read -r path; do
        mkdir -p "$directory/$(dirname "$path")"
        if grep -Fxq "$path" "$bundle_files"; then
            case "$path" in
                *"/$MACOS_BUNDLE_PLIST") package_fixture_plist "$directory/$path" "$version" ;;
                *) printf 'fixture bytes for %s\n' "$path" > "$directory/$path" ;;
            esac
            # Not a program, and the manifest has to say so. A fixture that
            # made every delivered file executable would hide exactly the
            # ambiguity the bundle introduced: a product root that owns three
            # files, one of which is its application.
            chmod 644 "$directory/$path"
            continue
        fi
        # Distinct per path, so a gate that mixed two files up would see two
        # different digests rather than one that happened to match.
        printf 'fixture bytes for %s\n' "$path" > "$directory/$path"
        chmod 755 "$directory/$path"
    done < "$paths"

    rm -f "$bundle_files" "$paths"
}
