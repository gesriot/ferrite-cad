#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Runs the shared STEP durable-identity implementation against the complete
# corpus and the real AP203 interoperability fixture.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
probe=""
cleanup=""

if [ "${1:-}" = "--probe" ]; then
  if [ "$#" -ne 2 ]; then
    echo "usage: $0 [--probe /path/to/step_key_probe]" >&2
    exit 2
  fi
  probe="$2"
elif [ "$#" -ne 0 ]; then
  echo "usage: $0 [--probe /path/to/step_key_probe]" >&2
  exit 2
fi

stale="$({
  find "$root" -maxdepth 1 \( -name '*.bak' -o -name '*.mutbak' \) -print
  find "$root/crates" "$root/tools" "$root/docs" "$root/fixtures" \
    "$root/.github" \( -name '*.bak' -o -name '*.mutbak' \) -print
} | head -1)"
if [ -n "$stale" ]; then
  echo "stale backup prevents the identity gate: $stale" >&2
  exit 1
fi

if [ -z "$probe" ]; then
  cleanup="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-step-identity.XXXXXX")"
  trap 'rm -rf "$cleanup"' EXIT
  cmake -S "$root/tools/step-key-probe" -B "$cleanup/build" \
    -DCMAKE_BUILD_TYPE=Release
  cmake --build "$cleanup/build" --parallel
  probe="$(find "$cleanup/build" -type f \
    \( -name step_key_probe -o -name step_key_probe.exe \) -print -quit)"
fi

if [ -z "$probe" ] || [ ! -x "$probe" ]; then
  echo "no executable step-key-probe at $probe" >&2
  exit 1
fi

if [ "$(rg -c '^#include "step_identity.hpp"$' \
    "$root/crates/ferritecad-occt-bridge/src/bridge.cpp")" != 1 ] ||
   [ "$(rg -c '^#include "step_identity.hpp"$' \
    "$root/tools/step-key-probe/src/main.cpp")" != 1 ]; then
  echo "the bridge and probe must each include the one shared identity implementation" >&2
  exit 1
fi
if [ "$(rg -c 'ferritecad::definition_keys\(' \
    "$root/crates/ferritecad-occt-bridge/src/bridge.cpp")" != 1 ] ||
   [ "$(rg -c 'ferritecad::definition_keys\(' \
    "$root/tools/step-key-probe/src/main.cpp")" != 1 ]; then
  echo "the bridge and probe must each call the shared key resolver once" >&2
  exit 1
fi
if rg -n '#include <Interface_Graph.hxx>|[.]Sharings[(]' \
    "$root/crates/ferritecad-occt-bridge/src/step_identity.hpp"; then
  echo "the identity implementation contains a graph-wide traversal" >&2
  exit 1
fi

report="$(mktemp "${TMPDIR:-/tmp}/ferritecad-step-identity-report.XXXXXX")"
trap 'rm -rf "$cleanup" "$report"' EXIT
inputs=(
  "$root"/fixtures/step/canonical/*.step
  "$root"/fixtures/step/damaged/*.step
  "$root"/fixtures/step/interoperability/*.stp
)
"$probe" "${inputs[@]}" > "$report"

require_line() {
  local line="$1"
  local count
  count="$(grep -Fxc "$line" "$report" || true)"
  if [ "$count" -ne 1 ]; then
    echo "identity report expected one line, found $count: $line" >&2
    exit 1
  fi
}

require_line '# typed ambiguity rejected yes'
require_line '    files examined 14'
require_line '    files with definitions 12'
require_line '    product definition usable on 11/12 files'
require_line '        unusable on: 06-duplicate-product-definition.step'

complex="$(sed -n '/^c3d-ap203-complex-assembly.stp$/,/^$/p' "$report")"
require_complex() {
  if ! grep -Fq "$1" <<<"$complex"; then
    echo "complex AP203 report is missing: $1" >&2
    printf '%s\n' "$complex" >&2
    exit 1
  fi
}
require_complex '    definitions 46'
require_complex '    roots 1'
require_complex '    placed occurrences 139'
require_complex '    assemblies with equal children but distinct products 1 pair(s)'
require_complex '    same after reversed traversal yes'
require_complex '    foreign source entity rejected yes'
require_complex 'identity index model scans 1  entities'
require_complex 'non-transforming relationships 35'
require_complex 'transforming relationships ignored 67'
require_complex 'XDE product associations 46'
require_complex 'ambiguous source identities 0'
require_complex '        durable key         step.product_definition#1'
require_complex '        durable key         step.product_definition#1764'
require_complex '        durable key         step.product_definition#2927'
require_complex '    product definition  present 46/46  unique yes  same on a second read yes'

echo "step identity: 46 definitions, 1 root and 139 occurrences are durable"
echo "step identity: shared one-scan implementation rejects source and typed ambiguity"
