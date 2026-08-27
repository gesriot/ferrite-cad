#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Starts a staged FerriteCAD from a clean environment and says whether it was
# the package that answered.
#
# Everything §21A-2b1 established about the application was established with
# the build tree still on disk and the loader pointed at it by an environment
# variable. That is a statement about loading. This is the other question: with
# the environment emptied and the directories that produced the binaries taken
# away, does the layout start at all, and is what it says coming from the files
# beside it?
#
# Four things have to be true at once, and each of them fails in a way that
# looks like success if it is not asked about separately.
#
#   The environment must really be empty. LD_LIBRARY_PATH, DYLD_LIBRARY_PATH
#   and a PATH still holding a build directory are each enough to make a
#   package that cannot stand on its own look like one that can.
#
#   The directories that produced the binaries must really be gone. A staged
#   executable whose load commands still name an absolute path into the build
#   tree runs perfectly on the machine that built it and nowhere else, and
#   nothing short of taking the directory away distinguishes the two.
#
#   Both halves of the product have to run. `--solver-info` says a great deal
#   about planegcs and nothing whatever about Open CASCADE; a rebuild says the
#   opposite. A run that did only one of them is half a measurement being
#   reported as a whole one.
#
#   Taking a library away has to stop the process. If it does not, the library
#   that answered was some other copy, and every check above was measuring the
#   machine rather than the package.
#
# Usage:
#   tools/check-staged-layout.sh --platform linux|macos|windows \
#       --staging DIR --document path/to/plate.fcad \
#       --forbidden DIR [--forbidden DIR]... --output facts.txt
#
# --forbidden names the build and install directories as they were spelled when
# the binaries were produced. They must not exist any more, and no staged file
# may contain their names.

set -euo pipefail

RUNTIME_PROBE_TOOL='check-staged-layout'
# shellcheck source=tools/runtime-probe.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/runtime-probe.sh"

platform=''
staging=''
document=''
output=''
forbidden=()

die() {
    echo "check-staged-layout: $*" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --platform)  platform="${2:-}"; shift 2 ;;
        --staging)   staging="${2:-}"; shift 2 ;;
        --document)  document="${2:-}"; shift 2 ;;
        --output)    output="${2:-}"; shift 2 ;;
        --forbidden) forbidden+=("${2:-}"); shift 2 ;;
        *) die "unknown argument $1" ;;
    esac
done

[ -n "$platform" ] || die 'no --platform'
[ -n "$staging" ] || die 'no --staging'
[ -n "$document" ] || die 'no --document'
[ -n "$output" ] || die 'no --output'
[ -d "$staging" ] || die "no such staging directory: $staging"
[ -f "$document" ] || die "no such document: $document"
[ "${#forbidden[@]}" -gt 0 ] || die 'no --forbidden directory, so nothing was taken away'

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

: > "$output"
fact() { printf '%s\n' "$*" >> "$output"; }

# ---------------------------------------------------------------------------
# The environment. Emptied here rather than trusted to have been emptied by
# whoever called: this is the property being asserted.
# ---------------------------------------------------------------------------

# Asserted, not fixed. Emptying these here would make the check pass for a
# caller that had left them set, and "the package was started with the loader
# pointed at the build tree" is precisely the thing that must not be reportable
# as a clean-environment run.
for variable in LD_LIBRARY_PATH DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH; do
    eval "value=\${${variable}:-}"
    if [ -n "$value" ]; then
        die "$variable is set to '$value'; a run with the loader environment still \
pointing somewhere says nothing about whether the package can stand on its own"
    fi
done

for directory in "${forbidden[@]}"; do
    if [ -e "$directory" ]; then
        die "$directory still exists, so a run that succeeds says nothing about the package"
    fi
done
fact "clean-environment forbidden-directories-present=0"

# PATH is the loader's search list on Windows and merely a convenience
# elsewhere, so it is checked everywhere and enforced on the platform where it
# decides the answer.
printf '%s\n' "$PATH" | tr ':' '\n' > "$work/path-entries"
for directory in "${forbidden[@]}"; do
    if grep -Fxq "$directory" "$work/path-entries"; then
        die "PATH still holds $directory"
    fi
done
if [ "$platform" = windows ]; then
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        # Asked separately. `ls a b` fails when either is missing, so asking
        # about both at once would let a directory holding only one of them
        # through, and one is enough for the loader.
        for pattern in planegcs.dll 'TK*.dll'; do
            # shellcheck disable=SC2086
            if [ -n "$(find "$entry" -maxdepth 1 -name $pattern -print -quit 2>/dev/null)" ]; then
                die "PATH entry $entry holds ${pattern}, so the loader need not use the package"
            fi
        done
    done < "$work/path-entries"
fi
fact "clean-environment path-holds-runtime=0"

# ---------------------------------------------------------------------------
# The layout, asserted rather than assumed. tools/stage-runtime-layout.sh built
# it; this says independently what it should have built, so the two drifting
# apart is a failure rather than a silent agreement to differ.
# ---------------------------------------------------------------------------

case "$platform" in
    linux)   expected_bin='bin'; expected_lib='lib' ;;
    macos)   expected_bin='FerriteCAD.app/Contents/MacOS'
             expected_lib='FerriteCAD.app/Contents/Frameworks' ;;
    windows) expected_bin='bin'; expected_lib='bin' ;;
    *) die "unknown platform $platform" ;;
esac

suffix=''
[ "$platform" != windows ] || suffix='.exe'
viewer="$staging/$expected_bin/ferritecad-viewer$suffix"
cli="$staging/$expected_bin/ferritecad$suffix"
[ -f "$viewer" ] || die "no staged viewer at $viewer"
[ -f "$cli" ] || die "no staged command line tool at $cli"

case "$platform" in
    macos)   planegcs="$staging/$expected_lib/libplanegcs.dylib" ;;
    linux)   planegcs="$staging/$expected_lib/libplanegcs.so" ;;
    windows) planegcs="$staging/$expected_lib/planegcs.dll" ;;
esac
[ -f "$planegcs" ] || die "no staged planegcs at $planegcs"

find "$staging/$expected_lib" -type f \( -name 'libTK*' -o -name 'TK*.dll' \) \
    | LC_ALL=C sort > "$work/toolkits"
[ -s "$work/toolkits" ] || die 'the staged layout carries no Open CASCADE toolkit'
fact "layout toolkits=$(wc -l < "$work/toolkits" | tr -d ' ')"

# A run that died between taking a library away and putting it back leaves a
# layout that is not the one it claims to be, and every check below would be
# measuring the leftovers. Refused rather than tidied up: which library is
# missing is information, and quietly restoring it throws that away.
find "$staging" -name '*.hidden' | LC_ALL=C sort > "$work/stale"
if [ -s "$work/stale" ]; then
    echo "check-staged-layout: the staging directory holds libraries an earlier run \
took away and never put back:" >&2
    sed 's/^/  /' "$work/stale" >&2
    echo "Stage it again rather than running against the leftovers." >&2
    exit 1
fi

# The inspector that says which libraries a binary names for itself, and on
# Windows the only way to ask at all. tools/runtime-probe.sh owns both.
runtime_probe_require_inspector "$platform"

# ---------------------------------------------------------------------------
# No staged file may name a directory that no longer exists.
# ---------------------------------------------------------------------------

: > "$work/leaks"
find "$staging" -type f | while IFS= read -r file; do
    for directory in "${forbidden[@]}"; do
        if LC_ALL=C grep -aqF "$directory" "$file"; then
            printf '%s\t%s\n' "${file#"$staging"/}" "$directory" >> "$work/leaks"
        fi
    done
done
if [ -s "$work/leaks" ]; then
    echo "check-staged-layout: staged files still name directories that are gone:" >&2
    sed 's/^/  /' "$work/leaks" >&2
    exit 1
fi
fact "clean-environment staged-files-naming-build-tree=0"

# ---------------------------------------------------------------------------
# Running it.
# ---------------------------------------------------------------------------

set +e
runtime_probe_run_with_deadline 60 "$work/solver-info.txt" "$viewer" --solver-info
viewer_status=$?
set -e
echo "--- staged ferritecad-viewer --solver-info (exit ${viewer_status}) ---"
cat "$work/solver-info.txt"

if [ "$viewer_status" -ne 0 ]; then
    echo "check-staged-layout: the staged viewer exited ${viewer_status}" >&2
    echo "a relocatable layout that does not start is the finding, not a detail" >&2
    exit 1
fi
grep -qx 'sketch solver: available' "$work/solver-info.txt" \
    || die 'the staged viewer did not say it has a solver'
if grep -qi 'skip' "$work/solver-info.txt"; then
    die 'the staged viewer skipped something instead of answering'
fi

# The words the staged library gave, compared against the copy that is actually
# beside the executable. Comparing against the delivery in the build tree would
# be comparing against a file that is gone.
named="$(sed -n 's/^provenance: //p' "$work/solver-info.txt" | head -1)"
[ -n "$named" ] || die 'the staged viewer named no provenance'
if ! LC_ALL=C grep -aqF "$named" "$planegcs"; then
    die "the staged viewer said '${named}', and the staged planegcs does not contain \
those words, so what it printed did not come from the library beside it"
fi
fact "staged viewer solver-info exit=0 answer=available"
fact "staged viewer provenance-from-staged-library=true"

# ---------------------------------------------------------------------------
# The other half: Open CASCADE, through the command line tool that already
# crosses that boundary. No new viewer command is added to measure this.
# ---------------------------------------------------------------------------

cp "$document" "$work/document.fcad"
before="$(cksum < "$work/document.fcad")"

set +e
runtime_probe_run_with_deadline 300 "$work/rebuild.txt" "$cli" rebuild "$work/document.fcad" --cold
cli_status=$?
set -e
echo "--- staged ferritecad rebuild --cold (exit ${cli_status}) ---"
cat "$work/rebuild.txt"

if [ "$cli_status" -ne 0 ]; then
    echo "check-staged-layout: the staged command line tool exited ${cli_status}" >&2
    exit 1
fi
if grep -q 'no Open CASCADE' "$work/rebuild.txt"; then
    die 'the staged build has no kernel, so the rebuild proved nothing about Open CASCADE'
fi
kernel="$(sed -n 's/^ *kernel //p' "$work/rebuild.txt" | head -1)"
case "$kernel" in
    occt\ *) ;;
    *) die "the rebuild named kernel '${kernel}', which is not Open CASCADE" ;;
esac
grep -q 'shape.* built' "$work/rebuild.txt" \
    || die 'the rebuild built no shape, so no Open CASCADE operation actually ran'

after="$(cksum < "$work/document.fcad")"
[ "$before" = "$after" ] || die 'the rebuild changed the document it was diagnosing'
if [ -e "$work/document.fcad-cache" ]; then
    die 'the rebuild wrote a cache sidecar beside the document'
fi
fact "staged cli rebuild-cold exit=0 kernel=${kernel%% *}"
fact "staged cli document-unchanged=true"

# ---------------------------------------------------------------------------
# And that the package is really what answered.
# ---------------------------------------------------------------------------

runtime_probe_hidden_must_stop "$work/hidden.txt" "$planegcs" 'the staged viewer' \
    "$viewer" --solver-info
fact "staged viewer hidden-planegcs started=false"

# Which toolkit to take away, and whether this platform still has a transitive
# one to take. tools/runtime-probe.sh owns that choice so that this gate and
# the one on the extracted archive cannot answer it differently.
runtime_probe_choose_toolkit "$platform" "$cli" "$work/toolkits"
echo "the Open CASCADE toolkit under test is $(basename "$runtime_probe_toolkit") (${runtime_probe_reach})"

runtime_probe_hidden_must_stop "$work/hidden.txt" "$runtime_probe_toolkit" \
    'the staged command line tool' "$cli" rebuild "$work/document.fcad" --cold
fact "staged cli hidden-occt-toolkit started=false reach=${runtime_probe_reach}"

# ---------------------------------------------------------------------------
# Restored, and passing again. A gate that only ever saw the broken state
# cannot tell a package that stopped working from one that never did.
# ---------------------------------------------------------------------------

set +e
runtime_probe_run_with_deadline 60 "$work/again-viewer.txt" "$viewer" --solver-info
again_viewer=$?
runtime_probe_run_with_deadline 300 "$work/again-cli.txt" "$cli" rebuild "$work/document.fcad" --cold
again_cli=$?
set -e
[ "$again_viewer" -eq 0 ] \
    || die "with the libraries back the viewer still exited ${again_viewer}"
[ "$again_cli" -eq 0 ] \
    || die "with the libraries back the command line tool still exited ${again_cli}"
fact "staged both restored=true"

echo "--- facts ---"
LC_ALL=C sort -o "$output" "$output"
cat "$output"
