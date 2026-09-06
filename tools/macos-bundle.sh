# SPDX-License-Identifier: MIT
# shellcheck shell=bash
# Set by the function below and read by the gates that source it.
# shellcheck disable=SC2034
#
# What makes a delivered FerriteCAD.app an application the desktop can start,
# and how to ask a bundle whether it is one. Sourced, never run.
#
# §21A-2b2a laid the macOS closure out as `FerriteCAD.app/Contents/MacOS` and
# `FerriteCAD.app/Contents/Frameworks` and started it by naming the executable
# on a command line, which is all that measurement needed. A directory shaped
# like a bundle is not a bundle: with no `Contents/Info.plist` in it, opening
# the delivery from the Finder starts no process at all and reports nothing,
# because LaunchServices has nothing that says which of the two executables
# beside each other is the application. That is the defect §23B closes, and it
# is invisible to every check that starts a binary by path.
#
# Two gates need to ask this and they must not answer it differently.
# tools/check-staged-layout.sh asks it of a staging directory and
# tools/check-release-package.sh asks it of an extracted archive, the same way
# they already share tools/runtime-probe.sh.
#
# This is the reader. tools/stage-runtime-layout.sh is the writer, and the two
# are deliberately separate spellings of the same rules - the same arrangement
# tools/native/lib.sh already has with the layout directories - so that a
# stager that started writing something else is a failure here rather than a
# silent agreement to differ.
#
# Nothing here claims a distribution signature. The ad-hoc signature staging
# applies is what makes a rewritten Mach-O startable at all and what makes the
# bundle verify against itself once it has an Info.plist in it; there is no
# identity, no certificate and no notarisation anywhere in this delivery, and
# macos_bundle_signed_ok below asks only whether the ad-hoc signature the
# delivery does carry still answers for the bytes that came with it.
#
# A caller sets MACOS_BUNDLE_TOOL to its own name before sourcing, so a failure
# reads as that gate's finding.

# Where the bundle keeps the file that says what it is, and where the
# executable it names has to be. Relative to the bundle, and written here
# independently of the stager.
readonly MACOS_BUNDLE_PLIST='Contents/Info.plist'
readonly MACOS_BUNDLE_MACOS_DIR='Contents/MacOS'

# The keys a bundle has to carry for this product. `CFBundleExecutable` is the
# one the whole slice is about; the rest are what a bundle needs before
# LaunchServices will treat the directory as an application rather than as a
# folder that happens to be named `.app`.
readonly MACOS_BUNDLE_REQUIRED_KEYS=(
    CFBundleExecutable
    CFBundleIdentifier
    CFBundleInfoDictionaryVersion
    CFBundleName
    CFBundlePackageType
    CFBundleShortVersionString
    CFBundleVersion
)

macos_bundle_say() {
    echo "${MACOS_BUNDLE_TOOL:-macos-bundle}: $*" >&2
}

# One `<string>` value out of an XML property list.
#
# Read with awk rather than with plutil, because two of the three gates that
# need the answer run on hosts that have no plutil: tools/check-packager.sh
# gates all three platforms' packaging arithmetic wherever it runs, and a check
# that could only be asked on macOS would be a check that mostly is not.
# Where plutil does exist it is used as well, and for the thing awk cannot do -
# saying whether the document is a property list at all.
macos_bundle_string() { # plist key
    # One tag per line first, so the answer does not depend on how the document
    # was laid out. `tr` rather than a `sed` that inserts a newline: BSD sed
    # writes a literal `n` for `\n` in a replacement, and this has to give the
    # same answer on all three runners.
    tr '<' '\n' < "$1" | awk -v want="$2" '
        /^key>/    { found = (substr($0, 5) == want); next }
        /^string>/ { if (found) { print substr($0, 8); exit } next }
        /^\// || /^[[:space:]]*$/ { next }
        # Any other opening tag between the key and a string is the value, and
        # it is not a string. The pairing ends there rather than running on to
        # whatever string comes next.
        /^[A-Za-z]/ { found = 0 }
    '
}

# Whether a bundle is an application the desktop can start, and whether the
# executable it points at is one the delivery actually carries.
#
# The version is compared only when the caller has an authoritative one to
# compare against. The gate on the extracted archive does: the package manifest
# says which product version this is. The gate on a staging directory does not,
# and asserting a version it had to invent would be asserting nothing.
#
# Sets macos_bundle_executable to the name the bundle says is its application.
macos_bundle_executable=''
macos_bundle_check() { # bundle-directory [product-version]
    local bundle="$1" version="${2:-}"
    local plist name value expected bad=0

    macos_bundle_executable=''
    case "$bundle" in
        *.app) ;;
        *) macos_bundle_say "$bundle is not named like an application bundle"; return 1 ;;
    esac
    [ -d "$bundle" ] || { macos_bundle_say "there is no bundle at $bundle"; return 1; }

    plist="$bundle/$MACOS_BUNDLE_PLIST"
    if [ ! -f "$plist" ]; then
        macos_bundle_say "$bundle carries no $MACOS_BUNDLE_PLIST, so the desktop has nothing \
that says it is an application or which executable beside it to start"
        return 1
    fi
    [ -s "$plist" ] || { macos_bundle_say "$plist is empty"; return 1; }

    # The only question awk cannot answer: is this a property list at all.
    # Absent off macOS, where a malformed document still fails every key below.
    if command -v plutil >/dev/null 2>&1; then
        if ! plutil -lint "$plist" > /dev/null 2>&1; then
            macos_bundle_say "$plist is not a readable property list:"
            plutil -lint "$plist" 2>&1 | sed 's/^/  /' >&2
            return 1
        fi
    fi

    for name in "${MACOS_BUNDLE_REQUIRED_KEYS[@]}"; do
        value="$(macos_bundle_string "$plist" "$name")"
        if [ -z "$value" ]; then
            macos_bundle_say "$plist says nothing for $name"
            bad=1
        fi
    done
    [ "$bad" -eq 0 ] || return 1

    value="$(macos_bundle_string "$plist" CFBundlePackageType)"
    [ "$value" = APPL ] || {
        macos_bundle_say "$plist calls the bundle a '$value' and an application is APPL"
        bad=1
    }

    expected="$(basename "$bundle")"; expected="${expected%.app}"
    value="$(macos_bundle_string "$plist" CFBundleName)"
    [ "$value" = "$expected" ] || {
        macos_bundle_say "$plist names the bundle '$value' and it is $expected.app"
        bad=1
    }

    # A reverse-DNS identity, which is what the desktop keys an application on.
    # Its exact value is the writer's to choose; that it is one is not.
    value="$(macos_bundle_string "$plist" CFBundleIdentifier)"
    case "$value" in
        *' '*) macos_bundle_say "the bundle identifier '$value' holds a space"; bad=1 ;;
        *.*.*) ;;
        *) macos_bundle_say "$plist gives the bundle the identifier '$value', which is not a \
reverse-DNS name"; bad=1 ;;
    esac

    if [ -n "$version" ]; then
        for name in CFBundleShortVersionString CFBundleVersion; do
            value="$(macos_bundle_string "$plist" "$name")"
            [ "$value" = "$version" ] || {
                macos_bundle_say "$plist gives $name as '$value' and this is version $version"
                bad=1
            }
        done
    fi

    # The point of the whole file. A bundle whose CFBundleExecutable names
    # something that is not there starts nothing, and the two executables of
    # this delivery sit beside each other, so naming the wrong one starts the
    # command line tool with no terminal to write to.
    value="$(macos_bundle_string "$plist" CFBundleExecutable)"
    case "$value" in
        */* | '') macos_bundle_say "$plist gives CFBundleExecutable as '$value', which is not \
a file name in $MACOS_BUNDLE_MACOS_DIR"; bad=1 ;;
        *)
            if [ ! -f "$bundle/$MACOS_BUNDLE_MACOS_DIR/$value" ]; then
                macos_bundle_say "$plist says the application is '$value' and \
$MACOS_BUNDLE_MACOS_DIR holds no such file, so opening the bundle starts nothing"
                bad=1
            elif [ ! -x "$bundle/$MACOS_BUNDLE_MACOS_DIR/$value" ]; then
                macos_bundle_say "$MACOS_BUNDLE_MACOS_DIR/$value is what the bundle says to \
start and it is not executable"
                bad=1
            fi ;;
    esac

    [ "$bad" -eq 0 ] || return 1
    macos_bundle_executable="$value"
    return 0
}

# Whether the ad-hoc signature the delivery carries still answers for it.
#
# Asked separately from macos_bundle_check, and only of a real product. The
# packaging gate builds its fixtures out of made-up bytes, which no signature
# covers and none should; this is a question about the bundle that was really
# staged and the one that really came out of the archive.
#
# --deep, because the shipped libraries and the second executable are sealed
# into CodeResources rather than into the main image, and a seal that stopped
# covering them would verify perfectly at the top level.
#
# Returns 2, distinctly, where there is no codesign to ask - a Linux or Windows
# host running this platform's arithmetic - so that "not asked" cannot be read
# as "asked and answered".
macos_bundle_signed_ok() { # bundle-directory scratch-file
    command -v codesign >/dev/null 2>&1 || return 2
    # The scratch file is the caller's, the way tools/runtime-probe.sh takes
    # one: nothing this asks may leave a file inside the delivery it is asking
    # about.
    if ! codesign --verify --deep "$1" > "$2" 2>&1; then
        macos_bundle_say "the ad-hoc signature of $1 does not answer for what is in it:"
        sed 's/^/  /' "$2" >&2
        return 1
    fi
    return 0
}
