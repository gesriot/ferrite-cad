# SPDX-License-Identifier: MIT
# shellcheck shell=bash
# Everything here is read by the scripts that source this file.
# shellcheck disable=SC2034
#
# Shared definitions for the native/assets inventory scripts. Sourced, never
# run.
#
# The inventory this supports is not a product SBOM and says so in its own
# first field. It answers six questions the Rust fragment cannot: which native
# components a target carries, which staged runtime file belongs to which of
# them, which inputs only ever take part in a build, which assets are embedded
# in a product binary, which product root loads a native component, and which
# libraries the loader shows that the package deliberately does not carry.
#
# What this file does not own: the product roots, their features and the
# product targets. tools/notices/lib.sh has owned those since §21A-2b2b0b2b1's
# two predecessors, and a second copy would let the native inventory describe a
# product the notices and the Rust fragment do not.

# shellcheck source=tools/notices/lib.sh
. tools/notices/lib.sh

# The three platforms the combined runtime layout measures, and the product
# triple each one produces. The triples are NOTICE_TARGETS' - named here only
# to say which platform builds which, which is a fact about the runners rather
# than about the product.
readonly NATIVE_PLATFORMS=(linux macos windows)
native_triple_for() { # platform
    case "$1" in
        linux)   printf 'x86_64-unknown-linux-gnu\n' ;;
        macos)   printf 'aarch64-apple-darwin\n' ;;
        windows) printf 'x86_64-pc-windows-msvc\n' ;;
        *) native_die "unknown platform $1" ;;
    esac
}

# Where a staged layout puts executables and libraries. The same rules
# tools/stage-runtime-layout.sh applies, written independently so that the two
# drifting apart is a failure rather than a silent agreement to differ.
native_bin_dir_for() { # platform
    case "$1" in
        linux | windows) printf 'bin\n' ;;
        macos)           printf 'FerriteCAD.app/Contents/MacOS\n' ;;
        *) native_die "unknown platform $1" ;;
    esac
}
native_lib_dir_for() { # platform
    case "$1" in
        linux)   printf 'lib\n' ;;
        windows) printf 'bin\n' ;;
        macos)   printf 'FerriteCAD.app/Contents/Frameworks\n' ;;
        *) native_die "unknown platform $1" ;;
    esac
}

# What a shared library is called on each platform, as a shell pattern. A
# staged library that does not match its platform's pattern is a file from
# another target, which is the failure this catches.
native_library_pattern_for() { # platform
    case "$1" in
        linux)   printf '*.so*\n' ;;
        macos)   printf '*.dylib\n' ;;
        windows) printf '*.dll\n' ;;
        *) native_die "unknown platform $1" ;;
    esac
}

# The measured ownership map: which component owns which staged file.
native_staged_map_for() { # platform
    printf 'tools/native/staged-%s.tsv\n' "$1"
}

# The Windows import library that the planegcs build produces beside the DLL,
# and the one thing that reads it.
#
# Which way this relationship points is a question, not a convention, and the
# first inventory answered it backwards: it recorded the `.lib` as a build
# input of planegcs, which would mean planegcs is built from it. It is not.
# tools/build-planegcs.sh produces `planegcs.lib` from the planegcs sources on
# Windows, and the thing that consumes it is the Windows linker, on behalf of
# the one crate that links planegcs. So the file is produced by planegcs and
# is a build input of that crate, and both halves are measured below rather
# than written down.
readonly NATIVE_IMPORT_LIBRARY='planegcs.lib'

# The workspace crates whose build script requires the import library. A crate
# is on this list because its own build.rs names the file, so a second crate
# growing a copy of the link is a failure rather than an unnoticed second
# opinion - which is the same rule tools/check-solver-ownership.sh already
# applies to the boundary itself.
native_import_library_consumers() {
    local f
    find crates -mindepth 2 -maxdepth 2 -name build.rs -type f | LC_ALL=C sort \
        | while IFS= read -r f; do
            if grep -qF "$NATIVE_IMPORT_LIBRARY" "$f"; then
                printf '%s\n' "${f%/build.rs}"
            fi
        done
}

readonly NATIVE_INVENTORY='sbom/native/native-assets-inventory.json'
# The workflow that stages a layout and is therefore the only thing that can
# check the file boundary. Named here because the gate asks it which platforms
# it measures.
readonly NATIVE_WORKFLOW='.github/workflows/runtime-layout.yml'
readonly NATIVE_SCHEMA='tools/native/inventory.schema.json'
readonly NATIVE_OCCT_PIN='tools/occt/pin.env'
readonly NATIVE_PLANEGCS_PIN='tools/planegcs/pin.env'

# The shape of the inventory document itself. Bumped when the layout changes
# in a way a consumer would have to notice; the packager of §21A-2b2b reads it
# to know which inventory it is joining to the Rust fragment.
readonly NATIVE_INVENTORY_FORMAT=1

# The crates whose vendored font files this inventory lifts out into components
# of their own.
#
# The rule, and it was decided by measurement rather than by what was
# convenient to generate. A typeface is a separate work with a name of its own
# that the crate's component identity does not carry: a reader of
# `epaint_default_fonts@0.36.1` cannot learn from it which typefaces the
# product embeds, and the product embeds four. Every other embedded data file
# measured in the product graph - `egui.wgsl`, wgpu-core's two shaders - is the
# embedding crate's own source, and its component already names it; giving it a
# second component would put one file under two identities.
#
# This list is not the inventory's evidence. tools/check-native-inventory.sh
# walks every package in the product graph for font files reached through an
# `include_bytes!` and refuses a font it finds that no component declares, so a
# crate dropped from this list is a failure rather than a smaller answer.
readonly NATIVE_FONT_CRATES=(epaint_default_fonts)

# What counts as a font, in one place. The generator and the gate ask the same
# question and a second list would let them answer it differently.
native_is_font_file() { # path
    case "$1" in
        *.ttf | *.otf | *.ttc | *.woff | *.woff2) return 0 ;;
        *) return 1 ;;
    esac
}

native_die() {
    echo "${NATIVE_TOOL:-native-inventory}: $*" >&2
    exit 1
}

# The pinned identities have exactly one owner each and are read from there.
# tools/check-planegcs-pins.sh already refuses a second copy of the planegcs
# digests anywhere that runs, and the Open CASCADE pin moved out of the
# workflow's `env:` block for the same reason.
native_load_pins() {
    # shellcheck source=tools/occt/pin.env
    . "$NATIVE_OCCT_PIN"
    # shellcheck source=tools/planegcs/pin.env
    . "$NATIVE_PLANEGCS_PIN"
    local v
    for v in OCCT_TAG OCCT_VERSION OCCT_COMMIT OCCT_SHA256 OCCT_ARCHIVE_URL \
             FCAD_PLANEGCS_FREECAD_TAG FCAD_PLANEGCS_FREECAD_URL \
             FCAD_PLANEGCS_ARCHIVE_SHA256 \
             FCAD_PLANEGCS_EIGEN_VERSION FCAD_PLANEGCS_EIGEN_URL \
             FCAD_PLANEGCS_EIGEN_SHA256 \
             FCAD_PLANEGCS_BOOST_VERSION FCAD_PLANEGCS_BOOST_URL \
             FCAD_PLANEGCS_BOOST_SHA256; do
        [ -n "${!v:-}" ] || native_die "the pins do not set $v"
    done
}

native_sha256() { # file
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

native_require_jq() {
    command -v jq >/dev/null 2>&1 || native_die 'jq is not installed'
}

# A path a native Windows program can open.
#
# The shell running this on Windows is MSYS and spells the checkout
# /d/a/ferrite-cad/...; a program that is not an MSYS program cannot open that
# name, and the failure is a missing file rather than a bad path. This is the
# third time the boundary between the two has cost this repository something:
# jq could not read a process substitution, jq wrote CRLF to stdout, and now
# Python could not open a binary the shell had just checked was there.
#
# `-m` rather than `-w`: forward slashes, so the answer can travel through a
# tab separated file without a backslash being read as an escape.
native_path() { # path
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -m "$1"
    else
        printf '%s\n' "$1"
    fi
}

# jq on Windows opens stdout in text mode and writes CRLF. A digest taken over
# those bytes is a different digest for a reason that has nothing to do with
# the product, so every jq result written to a file goes through here. The
# notices and the Rust fragment were both bitten by this before.
native_strip_cr() {
    tr -d '\r'
}

# Rows of a measured ownership map, comments and blank lines dropped.
native_map_rows() { # file
    grep -v '^[[:space:]]*#' "$1" | grep -v '^[[:space:]]*$'
}

# ---------------------------------------------------------------------------
# Reading what a package embeds.
# ---------------------------------------------------------------------------

# Every `include_bytes!`/`include_str!` of a file that is not Rust source,
# resolved against the directory of the file that names it, printed once per
# occurrence. An include whose file is not on disk cannot be in a binary: the
# crates in this graph that have one keep it behind `#[test]` and publish
# neither the file nor a way to reach it.
native_scan_includes() { # package-dir
    local dir="$1" hit file literal resolved
    [ -d "$dir/src" ] || return 0
    grep -rEo 'include_(bytes|str)!\([[:space:]]*"[^"]+"' "$dir/src" 2>/dev/null \
        | while IFS= read -r hit; do
            file="${hit%%:include_*}"
            literal="${hit#*\"}"
            literal="${literal%\"}"
            case "$literal" in
                *.rs) continue ;;
            esac
            resolved="$(dirname "$file")/$literal"
            [ -f "$resolved" ] || continue
            printf '%s\n' "$resolved"
        done
}

# The include macros in a package that the scanner above cannot read - a
# `concat!`, a macro argument, anything that is not one string literal - and
# that are not inside a `#[cfg(test)]` module.
#
# A form nobody can read must not pass for an absence. The one occurrence in
# this workspace today is a STEP fixture inside ferritecad-occt's test module,
# which is not in a product binary and is why the test annotation is part of
# the question rather than a separate one.
native_unreadable_includes() { # package-dir
    local dir="$1"
    [ -d "$dir/src" ] || return 0
    find "$dir/src" -name '*.rs' -type f | LC_ALL=C sort | while IFS= read -r file; do
        awk -v file="$file" '
            # Brace depth, and where the innermost #[cfg(test)] item started.
            # Good enough for Rust that is rustfmt-formatted, which this
            # workspace is: a brace inside a string literal on a line that also
            # carries an include macro is the case it would misread, and there
            # is none.
            {
                line = $0
                if (line ~ /^[[:space:]]*#\[cfg\(test\)\]/) { pending = 1 }
                opens = gsub(/\{/, "{", line)
                closes = gsub(/\}/, "}", line)
                if (pending && opens > 0) { test_depth = depth; in_test = 1; pending = 0 }
                if (!in_test && $0 ~ /include_(bytes|str)!/ \
                    && $0 !~ /include_(bytes|str)!\([[:space:]]*"[^"]+"/) {
                    printf "%s:%d: %s\n", file, NR, $0
                }
                depth += opens - closes
                if (in_test && depth <= test_depth) { in_test = 0 }
            }
        ' "$file"
    done
}
