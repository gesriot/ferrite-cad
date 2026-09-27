#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Real Mach-O fixtures, never launched. Intel/universal images exist only to
# prove the delivery gate rejects them, in either executable and in a dylib.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || {
    echo 'check-macos-architecture requires an Apple Silicon macOS host' >&2
    exit 1
}
# shellcheck source=tools/macos-bundle.sh
. tools/macos-bundle.sh
scratch="$(mktemp -d "${TMPDIR:-/tmp}/fcad-arm64.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
printf 'int main(void) { return 0; }\n' > "$scratch/probe.c"
for arch in arm64 x86_64; do
    clang -arch "$arch" "$scratch/probe.c" -o "$scratch/$arch"
    clang -arch "$arch" -dynamiclib "$scratch/probe.c" -o "$scratch/$arch.dylib"
done
lipo -create "$scratch/arm64" "$scratch/x86_64" -output "$scratch/universal"
lipo -create "$scratch/arm64.dylib" "$scratch/x86_64.dylib" -output "$scratch/universal.dylib"
bundle="$scratch/FerriteCAD.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Frameworks"
cp "$scratch/arm64" "$bundle/Contents/MacOS/ferritecad"
cp "$scratch/arm64" "$bundle/Contents/MacOS/ferritecad-viewer"
cp "$scratch/arm64.dylib" "$bundle/Contents/Frameworks/libprobe.dylib"
macos_bundle_arm64_ok "$bundle"
checks=1
for member in Contents/MacOS/ferritecad Contents/MacOS/ferritecad-viewer Contents/Frameworks/libprobe.dylib; do
    suffix=''
    case "$member" in *.dylib) suffix=.dylib ;; esac
    for arch in x86_64 universal; do
        cp "$scratch/$arch$suffix" "$bundle/$member"
        if macos_bundle_arm64_ok "$bundle" > "$scratch/refusal" 2>&1; then
            echo "accepted $arch in $member" >&2; exit 1
        fi
        grep -Fq "$member has architecture" "$scratch/refusal"
        grep -Fq 'requires arm64 only' "$scratch/refusal"
        checks=$((checks + 1))
    done
    cp "$scratch/arm64$suffix" "$bundle/$member"
done
# Explicit wrong CMake targets must fail at our policy before any dependency
# discovery. No OCCT/PlaneGCS source build is needed for these negative cases.
for project in crates/ferritecad-occt-bridge crates/ferritecad-sketch-solver/planegcs-bridge tools/planegcs; do
    for arch in x86_64 'arm64;x86_64'; do
        if cmake -S "$project" -B "$scratch/cmake-$checks" \
            "-DCMAKE_OSX_ARCHITECTURES=$arch" > "$scratch/refusal" 2>&1; then
            echo "accepted CMake $arch in $project" >&2; exit 1
        fi
        grep -Fq 'FerriteCAD macOS supports only arm64' "$scratch/refusal"
        checks=$((checks + 1))
    done
done
macos_bundle_arm64_ok "$bundle"
echo "macOS architecture policy: $checks checks passed"
