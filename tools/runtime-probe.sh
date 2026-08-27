# SPDX-License-Identifier: MIT
# shellcheck shell=bash
# Set by the functions below and read by the gates that source them.
# shellcheck disable=SC2034
#
# How to start a shipped binary and ask whether the files beside it are the
# ones that answered. Sourced, never run.
#
# Two gates need this and they must not answer it differently.
# tools/check-staged-layout.sh asks it of a staging directory and
# tools/check-release-package.sh asks it of an extracted archive; if one of
# them had its own copy, a package could pass the question the other gate would
# have failed it on, and both files would go on looking correct. Everything
# here was measured by the staged gate first and moved rather than rewritten.
#
# A caller sets RUNTIME_PROBE_TOOL to its own name before sourcing, so a
# failure reads as that gate's finding.

# The inspector that says which libraries a binary names for itself. Windows
# has no run path, so its import table is also the only way to ask there.
runtime_probe_dumpbin=''
runtime_probe_require_inspector() { # platform
    [ "$1" = windows ] || return 0
    if [ -n "${FCAD_MSVC_BIN:-}" ] && [ -x "$FCAD_MSVC_BIN/dumpbin.exe" ]; then
        runtime_probe_dumpbin="$FCAD_MSVC_BIN/dumpbin.exe"
    else
        runtime_probe_dumpbin="$(command -v dumpbin.exe 2>/dev/null \
            || command -v dumpbin 2>/dev/null || true)"
    fi
    [ -n "$runtime_probe_dumpbin" ] || {
        echo "${RUNTIME_PROBE_TOOL:-runtime-probe}: no dumpbin, so no import table can be read" >&2
        exit 1
    }
}

runtime_probe_names_directly() { # platform binary name
    local platform="$1" binary="$2" name="$3"
    case "$platform" in
        linux)   readelf -d "$binary" | grep -qF "$name" ;;
        macos)   otool -L "$binary" | grep -qF "$name" ;;
        windows) "$runtime_probe_dumpbin" //DEPENDENTS "$(cygpath -w "$binary")" \
                     | tr -d '\r' | grep -qiF "$name" ;;
    esac
}

# A deadline, and a child that is killed and reaped rather than left behind.
# `--solver-info` answers before any window exists, so a run that has not
# returned has not answered; on a machine with a window server the difference
# between those two is a process that waits forever.
# Deliberately does not touch `set -e`. An earlier version restored errexit
# just before returning the child's status, which made every non-zero exit
# kill the caller at the point of the call - including the ones it exists to
# observe. The run that found it left a library renamed away and the next run
# reported the layout as incomplete, which is exactly the confusion a stale
# backup causes.
runtime_probe_run_with_deadline() { # seconds output-file command...
    local seconds="$1" out="$2"; shift 2
    local pid waited=0 status=0
    "$@" > "$out" 2>&1 &
    pid=$!
    while kill -0 "$pid" 2>/dev/null; do
        if [ "$waited" -ge "$seconds" ]; then
            kill -9 "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
            echo "TIMEOUT" >> "$out"
            return 124
        fi
        sleep 1
        waited=$((waited + 1))
    done
    wait "$pid" || status=$?
    return "$status"
}

# Take a library away, and require the process to stop. If it does not, the
# library that answered was some other copy and every other check was measuring
# the machine rather than the delivery.
runtime_probe_hidden_must_stop() { # scratch-file library who binary args...
    local scratch="$1" library="$2" who="$3" binary="$4"; shift 4
    local status=0
    mv "$library" "$library.hidden"
    set +e
    runtime_probe_run_with_deadline 60 "$scratch" "$binary" "$@"
    status=$?
    set -e
    mv "$library.hidden" "$library"
    echo "--- with $(basename "$library") hidden, ${who} exited ${status} ---"
    head -5 "$scratch"
    if [ "$status" -eq 0 ]; then
        echo "${RUNTIME_PROBE_TOOL:-runtime-probe}: ${who} ran with $(basename "$library") \
taken away, so some other copy answered and none of this is about the delivery" >&2
        exit 1
    fi
    if grep -q 'sketch solver:' "$scratch"; then
        echo "${RUNTIME_PROBE_TOOL:-runtime-probe}: ${who} answered a question about the solver \
with no library to load; the loader must stop it rather than let it fall back to anything" >&2
        exit 1
    fi
}

# Which Open CASCADE toolkit to take away.
#
# The last one in a sorted list is as arbitrary as any other and does not
# depend on which release this is. Preferred is a toolkit the product binary
# does not name for itself, so that hiding it tests the half of the closure a
# run with the build tree still there never has to think about. Chosen by
# inspection rather than written down, because a name written here is a name
# that stops being transitive one release later.
#
# Whether such a toolkit exists at all is a platform difference and a finding.
# ferritecad-occt's build script hands the linker every toolkit the bridge
# reports, and Mach-O records all of them whether or not a symbol is used from
# each; a linker defaulting to --as-needed drops the ones that are not, and the
# same closure then has a transitive half. Which case this platform is in is
# recorded rather than assumed, because a gate that quietly stopped having a
# transitive library to hide would go on passing while having stopped asking
# the question.
#
# Sets runtime_probe_toolkit and runtime_probe_reach.
# Both are read by the gates that source this file rather than by anything here.
# shellcheck disable=SC2034
runtime_probe_toolkit=''
runtime_probe_reach=''
runtime_probe_choose_toolkit() { # platform binary sorted-toolkit-list-file
    local platform="$1" binary="$2" list="$3" toolkit
    runtime_probe_toolkit=''
    runtime_probe_reach=''
    while IFS= read -r toolkit; do
        [ -n "$toolkit" ] || continue
        if runtime_probe_names_directly "$platform" "$binary" "$(basename "$toolkit")"; then
            continue
        fi
        runtime_probe_toolkit="$toolkit"
        runtime_probe_reach=transitive
        return 0
    done < "$list"

    # Every toolkit is named directly. Hiding one is still the property that
    # matters - a required library taken away must stop the process - and the
    # reach says which kind was tested, so a platform that loses its transitive
    # half is visible in the comparison rather than silently equivalent.
    runtime_probe_toolkit="$(head -1 "$list")"
    runtime_probe_reach=direct-only
    [ -n "$runtime_probe_toolkit" ] || {
        echo "${RUNTIME_PROBE_TOOL:-runtime-probe}: there is no Open CASCADE toolkit to take away" >&2
        exit 1
    }
}
