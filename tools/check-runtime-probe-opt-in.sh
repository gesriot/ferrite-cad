#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Check refusal before any binary or mutable staging directory is needed.
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
unset FCAD_ALLOW_LOADER_FAILURE_PROBES

for value in '' 0 true; do
    if FCAD_ALLOW_LOADER_FAILURE_PROBES="$value" bash -c \
        '. tools/runtime-probe.sh; runtime_probe_require_failure_opt_in macos' \
        > "$work/log" 2>&1; then
        echo 'macOS failure experiment accepted without explicit 1' >&2
        exit 1
    fi
    grep -qF 'may show macOS crash dialogs' "$work/log"
done
FCAD_ALLOW_LOADER_FAILURE_PROBES=1 bash -c \
    '. tools/runtime-probe.sh; runtime_probe_require_failure_opt_in macos'
for platform in linux windows; do
    bash -c '. tools/runtime-probe.sh; runtime_probe_require_failure_opt_in "$1"' _ "$platform"
done

# Sentinel facts must survive, and nonexistent inputs ensure nothing can launch.
printf 'existing facts\n' > "$work/facts"
cp "$work/facts" "$work/before"
for gate in staged release; do
    if [ "$gate" = staged ]; then
        args=(tools/check-staged-layout.sh --platform macos --staging "$work/missing"
            --document "$work/missing.fcad" --output "$work/facts")
    else
        args=(tools/check-release-package.sh --platform macos --archive "$work/missing.tar.gz"
            --extract-to "$work/extracted" --output "$work/facts")
    fi
    if bash "${args[@]}" > "$work/log" 2>&1; then
        echo "$gate gate unexpectedly ran without opt-in" >&2
        exit 1
    fi
    grep -qF 'may show macOS crash dialogs' "$work/log"
    cmp "$work/facts" "$work/before"
    test ! -e "$work/extracted"
done
echo 'runtime loader opt-in: policy and both entry points passed; no product launched'
