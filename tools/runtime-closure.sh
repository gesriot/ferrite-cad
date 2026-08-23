#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Says what a FerriteCAD executable needs at run time, and where each of those
# things would have to come from in a package.
#
# This is a measuring instrument, not a packager. It answers the question
# §21A-2b2a exists to answer: a release that carries the sketch solver has to
# carry Open CASCADE at the same time, and until the two closures have been
# walked together on all three platforms nobody knows what the layout of that
# release is. It walks the graph, resolves every edge the way the platform's
# loader would, and classifies each member as a system library the package must
# not copy, an Open CASCADE toolkit, planegcs, or something nobody has
# accounted for yet.
#
# Three properties matter more than the walk itself.
#
# An inspector that fails must not look like a binary that needs nothing.
# `ldd`, `otool` and `dumpbin` all report "no dependencies" by printing very
# little, and so does a command that could not run. Every invocation here is
# checked for status and for output that parses, and a report that names no
# dependency at all is refused: every real executable names libc, or libSystem,
# or kernel32.
#
# An unresolved edge must not be silently dropped. A name the loader could not
# find is the difference between a package that runs and one that does not, and
# it is exactly what a build-tree run hides.
#
# System libraries are decided by where they live, not by a list of names. A
# name list is a list somebody has to remember to extend; a library the
# operating system ships lives in the operating system's own directories, and
# that is the property the packaging decision actually rests on. Open CASCADE
# and planegcs are matched by name first, so a distribution that happened to
# install a libTK into /usr/lib is still reported as Open CASCADE and still
# counted against the licence obligations.
#
# Usage:
#   tools/runtime-closure.sh --platform linux|macos|windows \
#       --label viewer --binary path/to/ferritecad-viewer \
#       [--search DIR]... --output closure-viewer.txt
#
# --search names directories the loader would find libraries in that are not
# reachable from the binary itself: the Open CASCADE install's library
# directory, the planegcs delivery. They stand in for the loader environment,
# and naming them is deliberate. This script never reads LD_LIBRARY_PATH,
# DYLD_LIBRARY_PATH or PATH, so its answer does not change with the shell it
# happened to be run from.

set -euo pipefail

platform=''
label=''
binary=''
output=''
searches=()

die() {
    echo "runtime-closure: $*" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --platform) platform="${2:-}"; shift 2 ;;
        --label)    label="${2:-}"; shift 2 ;;
        --binary)   binary="${2:-}"; shift 2 ;;
        --output)   output="${2:-}"; shift 2 ;;
        --search)   searches+=("${2:-}"); shift 2 ;;
        *) die "unknown argument $1" ;;
    esac
done

[ -n "$platform" ] || die 'no --platform'
[ -n "$label" ] || die 'no --label'
[ -n "$binary" ] || die 'no --binary'
[ -n "$output" ] || die 'no --output'
case "$platform" in
    linux | macos | windows) ;;
    *) die "unknown platform $platform" ;;
esac
[ -f "$binary" ] || die "no such binary: $binary"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

TAB="$(printf '\t')"

: > "$work/edges"    # from <TAB> name <TAB> resolved
: > "$work/queue"
: > "$work/seen"
: > "$work/forms"    # kind <TAB> file <TAB> value
: > "$work/rpaths"   # owner <TAB> entry
: > "$work/abspaths" # owner <TAB> where <TAB> path

root="$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")"
root_dir="$(dirname "$root")"

# ---------------------------------------------------------------------------
# Inspectors. Each refuses to return an empty answer, because an empty answer
# from a failed inspector is indistinguishable from a binary that needs
# nothing, and only one of those is a finding.
# ---------------------------------------------------------------------------

inspect() {
    local what="$1"; shift
    local out status
    set +e
    out="$("$@" 2>"$work/inspect.err")"
    status=$?
    set -e
    if [ "$status" -ne 0 ]; then
        echo "runtime-closure: $what exited $status" >&2
        printf '  %s\n' "$*" >&2
        sed -n '1,20p' "$work/inspect.err" >&2
        exit 1
    fi
    if [ -z "${out//[[:space:]]/}" ]; then
        echo "runtime-closure: $what produced no output" >&2
        printf '  %s\n' "$*" >&2
        echo "  an inspector that says nothing is a broken gate, not a binary" >&2
        echo "  without dependencies" >&2
        exit 1
    fi
    printf '%s\n' "$out"
}

# ---------------------------------------------------------------------------
# Classification: occt | planegcs | system | unexpected.
# ---------------------------------------------------------------------------

# Answers into CLASS rather than onto stdout. A closure of this size has a few
# thousand edges in it, and a command substitution per edge is a forked shell
# per edge; the walk took minutes before this was a plain assignment.
CLASS=''
classify() {
    local name="$1" path="$2" base lowered
    base="${name##*/}"

    case "$base" in
        libplanegcs.so* | libplanegcs*.dylib | planegcs.dll | planegcs.DLL | PLANEGCS.DLL)
            CLASS=planegcs; return ;;
        libTK*.so* | libTK*.dylib | TK*.dll | TK*.DLL)
            CLASS=occt; return ;;
    esac

    case "$platform" in
        macos)
            # The dyld shared cache means these have no file on disk at all, so
            # where the load command points is the only thing that can be asked
            # about them.
            case "$name" in
                /usr/lib/* | /System/Library/*) CLASS=system; return ;;
            esac
            ;;
        linux)
            # The virtual DSO and the program interpreter belong to the kernel
            # and to the loader, not to the package.
            case "$base" in
                linux-vdso.so* | linux-gate.so* | ld-linux*.so* | ld64.so*)
                    CLASS=system; return ;;
            esac
            case "$path" in
                /lib/* | /lib64/* | /usr/lib/* | /usr/lib64/* | /usr/libexec/*)
                    CLASS=system; return ;;
            esac
            ;;
        windows)
            # An API set is a name the loader redirects; there is no such file
            # anywhere, and demanding one would report the whole C runtime
            # missing.
            case "$base" in
                api-ms-win-* | API-MS-WIN-* | ext-ms-* | EXT-MS-*)
                    CLASS=system; return ;;
            esac
            # The second character of the first set is an escaped backslash,
            # which is what a Windows path separator is; shellcheck reads it as
            # a quoting mistake.
            # shellcheck disable=SC1003
            lowered="$(printf '%s' "$path" | tr 'A-Z\\' 'a-z/')"
            case "$lowered" in
                */windows/system32/* | */windows/syswow64/* | */windows/winsxs/*)
                    CLASS=system; return ;;
            esac
            ;;
    esac

    CLASS=unexpected
}

HIT=''
search_dirs() {
    local name="$1" dir
    for dir in ${searches[@]+"${searches[@]}"}; do
        if [ -f "$dir/$name" ]; then
            HIT="$dir/$name"
            return 0
        fi
    done
    return 1
}

record_edge() {
    printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$work/edges"
}

# ---------------------------------------------------------------------------
# Linux. readelf says which edges this node owns and what run path it carries;
# ldd says where each of them resolved. Neither alone says both things.
# ---------------------------------------------------------------------------

edges_linux() {
    local node="$1" name resolved

    inspect 'readelf -d' readelf -d "$node" > "$work/dyn.txt"

    sed -n 's/.*(R\(UN\)\?PATH).*\[\(.*\)\]/\2/p' "$work/dyn.txt" \
        | tr ':' '\n' | grep -v '^$' > "$work/node-rpaths" || true
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        printf '%s\t%s\n' "$node" "$entry" >> "$work/rpaths"
        case "$entry" in
            /*) printf '%s\trunpath\t%s\n' "$node" "$entry" >> "$work/abspaths" ;;
        esac
    done < "$work/node-rpaths"

    local soname
    soname="$(sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p' "$work/dyn.txt" | head -1)"
    [ -z "$soname" ] || printf 'soname\t%s\t%s\n' "${node##*/}" "$soname" >> "$work/forms"

    inspect ldd ldd "$node" > "$work/ldd.txt"
    if grep -q 'not a dynamic executable' "$work/ldd.txt"; then
        die "$node is not a dynamic executable"
    fi

    sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' "$work/dyn.txt" > "$work/needed"
    while IFS= read -r name; do
        [ -n "$name" ] || continue
        resolved="$(awk -v want="$name" '
            { gsub(/^[ \t]+|[ \t]+$/, "") }
            $1 != want { next }
            /not found/ { print "!unresolved"; found = 1; exit }
            /=>/ { path = $0; sub(/.*=> /, "", path); sub(/ \(0x.*/, "", path)
                   print path; found = 1; exit }
            { print "!bare"; found = 1; exit }
        ' "$work/ldd.txt")"
        if [ -z "$resolved" ] || [ "$resolved" = '!bare' ]; then
            case "$name" in
                linux-vdso.so* | ld-linux* | ld64.so*) resolved='!system' ;;
                *) resolved='!unresolved' ;;
            esac
        fi
        # ldd answers with the loader environment this script refuses to read,
        # and the product executables carry no run path of their own: cargo
        # applies a link argument to the emitting package's own targets and not
        # to a dependent. So every toolkit is "not found" to ldd, and the
        # directories the caller named are what stands in for the loader
        # environment - deliberately, so the answer does not change with the
        # shell the measurement was run from.
        if [ "$resolved" = '!unresolved' ] && search_dirs "$name"; then
            resolved="$HIT"
        fi
        record_edge "$node" "$name" "$resolved"
    done < "$work/needed"
}

# ---------------------------------------------------------------------------
# macOS. dyld's own rules, in dyld's own order.
# ---------------------------------------------------------------------------

RESOLVED=''
resolve_macos() {
    local node="$1" name="$2" dir leaf entry candidate
    dir="${node%/*}"

    case "$name" in
        @rpath/*)
            leaf="${name#@rpath/}"
            while IFS="$TAB" read -r owner entry; do
                [ "$owner" = "$node" ] || continue
                candidate="${entry//@loader_path/$dir}"
                candidate="${candidate//@executable_path/$root_dir}"
                if [ -f "$candidate/$leaf" ]; then
                    RESOLVED="$candidate/$leaf"
                    return
                fi
            done < "$work/rpaths"
            if search_dirs "$leaf"; then RESOLVED="$HIT"; return; fi
            RESOLVED='!unresolved'; return ;;
        @loader_path/*)   candidate="$dir/${name#@loader_path/}" ;;
        @executable_path/*) candidate="$root_dir/${name#@executable_path/}" ;;
        /*)
            case "$name" in
                /usr/lib/* | /System/Library/*) RESOLVED='!system'; return ;;
            esac
            candidate="$name" ;;
        *)
            if search_dirs "$name"; then RESOLVED="$HIT"; return; fi
            RESOLVED='!unresolved'; return ;;
    esac

    if [ -f "$candidate" ]; then RESOLVED="$candidate"; else RESOLVED='!unresolved'; fi
}

edges_macos() {
    local node="$1" own name

    # A dylib's own LC_ID_DYLIB appears in `otool -L` exactly where a
    # dependency would. It is asked for by name rather than skipped by
    # position, because an executable has no such line and the offsets differ.
    own=''
    if otool -D "$node" > "$work/id.txt" 2>/dev/null; then
        own="$(sed -n '2p' "$work/id.txt" | sed 's/[[:space:]]*$//')"
    fi
    [ -z "$own" ] || printf 'installname\t%s\t%s\n' "${node##*/}" "$own" >> "$work/forms"

    otool -l "$node" | awk '
        /^ *cmd LC_RPATH$/ { want = 1; next }
        want && $1 == "path" { print $2; want = 0 }
    ' > "$work/node-rpaths"
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        printf '%s\t%s\n' "$node" "$entry" >> "$work/rpaths"
        case "$entry" in
            /*) printf '%s\trpath\t%s\n' "$node" "$entry" >> "$work/abspaths" ;;
        esac
    done < "$work/node-rpaths"

    inspect 'otool -L' otool -L "$node" \
        | sed -n '2,$p' \
        | sed 's/ (compatibility.*//; s/^[[:space:]]*//; s/[[:space:]]*$//' \
        | grep -v '^$' > "$work/node-deps" || true

    while IFS= read -r name; do
        [ -n "$name" ] || continue
        [ "$name" != "$own" ] || continue
        case "$name" in
            /*) printf '%s\tinstallname\t%s\n' "$node" "$name" >> "$work/abspaths" ;;
        esac
        resolve_macos "$node" "$name"
        record_edge "$node" "$name" "$RESOLVED"
    done < "$work/node-deps"
}

# ---------------------------------------------------------------------------
# Windows. There is no run path on this platform: the loader searches the
# directory of the image and then PATH, and the point of a package here is
# that the first of those is enough.
# ---------------------------------------------------------------------------

resolve_windows() {
    local node="$1" name="$2" dir found
    dir="${node%/*}"

    case "$name" in
        api-ms-win-* | API-MS-WIN-* | ext-ms-* | EXT-MS-*) RESOLVED='!system'; return ;;
    esac

    if [ -f "$dir/$name" ]; then RESOLVED="$dir/$name"; return; fi
    if [ -f "$root_dir/$name" ]; then RESOLVED="$root_dir/$name"; return; fi
    if search_dirs "$name"; then RESOLVED="$HIT"; return; fi

    # Windows file names are case-insensitive and the import table carries
    # whatever case the linker recorded, which is not always the case on disk.
    found="$(find "$system32" -maxdepth 1 -iname "$name" -print -quit 2>/dev/null || true)"
    if [ -n "$found" ]; then RESOLVED="$found"; return; fi

    RESOLVED='!unresolved'
}

edges_windows() {
    local node="$1" name
    inspect 'dumpbin //DEPENDENTS' "$dumpbin" //DEPENDENTS "$(cygpath -w "$node")" \
        | tr -d '\r' \
        | awk '
            /following dependencies/ { want = 1; next }
            /^ *Summary/ { want = 0 }
            want && tolower($0) ~ /\.dll[ \t]*$/ {
                gsub(/^[ \t]+|[ \t]+$/, ""); print
            }
        ' > "$work/node-deps"

    while IFS= read -r name; do
        [ -n "$name" ] || continue
        resolve_windows "$node" "$name"
        record_edge "$node" "$name" "$RESOLVED"
    done < "$work/node-deps"
}

# ---------------------------------------------------------------------------
# The walk.
# ---------------------------------------------------------------------------

if [ "$platform" = windows ]; then
    dumpbin=''
    if [ -n "${FCAD_MSVC_BIN:-}" ] && [ -x "$FCAD_MSVC_BIN/dumpbin.exe" ]; then
        dumpbin="$FCAD_MSVC_BIN/dumpbin.exe"
    else
        dumpbin="$(command -v dumpbin.exe 2>/dev/null || command -v dumpbin 2>/dev/null || true)"
    fi
    [ -n "$dumpbin" ] || die 'no dumpbin, so no import table can be read'
    system32="$(cygpath -u "${SYSTEMROOT:-C:/Windows}")/System32"
fi

printf '%s\n' "$root" > "$work/queue"
while [ -s "$work/queue" ]; do
    node="$(head -1 "$work/queue")"
    tail -n +2 "$work/queue" > "$work/queue.next"
    mv "$work/queue.next" "$work/queue"
    [ -n "$node" ] || continue
    if grep -Fxq "$node" "$work/seen"; then continue; fi
    printf '%s\n' "$node" >> "$work/seen"

    case "$platform" in
        linux)   edges_linux "$node" ;;
        macos)   edges_macos "$node" ;;
        windows) edges_windows "$node" ;;
    esac

    # Everything this node named that has a file and is not the system's gets
    # walked in turn. The transitive half of the closure is the half a
    # build-tree run never has to think about, and the half that breaks a copy.
    awk -F"$TAB" -v node="$node" '$1 == node { print $3 }' "$work/edges" \
        | LC_ALL=C sort -u > "$work/next"
    while IFS= read -r resolved; do
        case "$resolved" in
            '' | '!system' | '!unresolved') continue ;;
        esac
        [ -f "$resolved" ] || continue
        classify "$resolved" "$resolved"
        [ "$CLASS" != system ] || continue
        if grep -Fxq "$resolved" "$work/seen"; then continue; fi
        printf '%s\n' "$resolved" >> "$work/queue"
    done < "$work/next"
done

# ---------------------------------------------------------------------------
# The report. One line per closure member, in a stable order, so two runs of
# the same layout produce the same text and a difference between them is
# evidence rather than noise.
# ---------------------------------------------------------------------------

# One size per distinct file rather than one per edge: the same toolkit is
# named by twenty others, and asking the filesystem twenty times says nothing
# more than asking once.
: > "$work/sizes"
awk -F"$TAB" '$3 ~ /^\// { print $3 }' "$work/edges" | LC_ALL=C sort -u \
    | while IFS= read -r path; do
        [ -f "$path" ] || continue
        printf '%s\t%s\n' "$path" "$(wc -c < "$path" | tr -d ' ')" >> "$work/sizes"
    done

: > "$work/members"
: > "$work/unresolved"
while IFS="$TAB" read -r from name resolved; do
    [ -n "$name" ] || continue
    if [ "$resolved" = '!unresolved' ]; then
        printf 'unresolved %s %s named-by=%s\n' "$label" "${name##*/}" "${from##*/}" \
            >> "$work/unresolved"
        continue
    fi
    kind=transitive
    [ "$from" != "$root" ] || kind=direct
    classify "$name" "$resolved"
    printf '%s\t%s\t%s\t%s\n' "${name##*/}" "$CLASS" "$kind" "$resolved" \
        >> "$work/members"
done < "$work/edges"

# A member the application names directly and something else names too is
# direct: what matters for a layout is whether the application itself declares
# it.
# The two inputs are told apart by a marker rather than by awk's NR == FNR,
# which asks whether the first file is still being read and answers "yes" for
# the whole second file when the first one was empty. A closure whose members
# all live in the dyld shared cache has no file to measure and so no sizes at
# all, and that made every dep line disappear: the report said the binary
# needed nothing, which is the one thing this walker must never say by
# accident. Found by the mutation that stops the inspector's exit status being
# checked, which left exactly that closure behind.
{
    awk -F"$TAB" '{ print "S" FS $1 FS $2 }' "$work/sizes"
    LC_ALL=C sort -u "$work/members" | awk -F"$TAB" '{ print "M" FS $0 }'
} | awk -F"$TAB" -v label="$label" '
    $1 == "S" { size[$2] = $3; next }
    $1 == "M" {
        class[$2] = $3; where[$2] = $5; seen[$2] = 1
        if ($4 == "direct") { direct[$2] = 1 }
    }
    END {
        for (name in seen) {
            bytes = (where[name] in size) ? size[where[name]] : "-"
            printf "dep %s %s %s %s %s\n", label,
                (name in direct ? "direct" : "transitive"), class[name], name, bytes
        }
    }
' | LC_ALL=C sort > "$work/final"

{
    echo "# FerriteCAD runtime closure"
    echo "platform $platform"
    echo "binary $label ${root##*/} $(wc -c < "$root" | tr -d ' ')"

    awk -F"$TAB" -v root="$root" '$1 == root { print "rpath " $2 }' "$work/rpaths" \
        | LC_ALL=C sort -u > "$work/root-rpaths"
    if [ -s "$work/root-rpaths" ]; then cat "$work/root-rpaths"; else echo "rpath (none)"; fi

    cat "$work/final"

    LC_ALL=C sort -u "$work/forms" | awk -F"$TAB" 'NF == 3 { print $1 " " $2 " " $3 }'

    LC_ALL=C sort -u "$work/abspaths" \
        | awk -F"$TAB" 'NF == 3 { n = split($1, p, "/"); print "abspath " p[n] " " $2 " " $3 }'

    LC_ALL=C sort -u "$work/unresolved" 2>/dev/null || true

    for class in system occt planegcs unexpected; do
        awk -v c="$class" -v l="$label" '
            $4 == c { files++; if ($6 != "-") bytes += $6 }
            END { printf "total %s %s files=%d bytes=%d\n", l, c, files, bytes }
        ' "$work/final"
    done
    awk -v l="$label" '
        $4 != "system" { files++; if ($6 != "-") bytes += $6 }
        END { printf "total %s shipped files=%d bytes=%d\n", l, files, bytes }
    ' "$work/final"
} > "$output"

cat "$output"

# Refusals come last, so the report exists to be read even when the run stops
# here. A closure with a hole in it is not a smaller closure.
status=0
if [ -s "$work/unresolved" ]; then
    echo "runtime-closure: $label has unresolved runtime dependencies" >&2
    status=1
fi
if ! grep -q '^dep ' "$output"; then
    echo "runtime-closure: $label named no runtime dependency at all, which no real" >&2
    echo "executable does; what this measured is the inspector, not the binary" >&2
    status=1
fi
exit "$status"
