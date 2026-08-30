#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Checks the committed STEP corpus against the checksums in its PROVENANCE.md.
#
# The provenance file answers "has the committed input changed?", and a claim
# nobody verifies stops being one. This is the verification; the semantic
# manifest, which answers the different question of whether the same model can
# still be produced, is checked by the pinned-OCCT workflow.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
provenance="${root}/fixtures/step/PROVENANCE.md"

if [ ! -f "${provenance}" ]; then
  echo "no ${provenance}" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  digest() { sha256sum "$1" | cut -d' ' -f1; }
else
  digest() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi

checked=0
failed=0

# Rows read `| `name` | size | `sha` |`, which is the only shape this has to
# parse and is checked against the file list below so a row that stops being
# read is not mistaken for a row that passed.
while IFS= read -r line; do
  name="$(printf '%s' "${line}" | sed -n 's/^| `\([^`]*\)` | [0-9]* | `\([0-9a-f]*\)` |$/\1/p')"
  want="$(printf '%s' "${line}" | sed -n 's/^| `\([^`]*\)` | [0-9]* | `\([0-9a-f]*\)` |$/\2/p')"
  [ -z "${name}" ] && continue

  path=""
  for candidate in "${root}/fixtures/step/canonical/${name}" \
                   "${root}/fixtures/step/damaged/${name}" \
                   "${root}/fixtures/step/interoperability/${name}"; do
    [ -f "${candidate}" ] && path="${candidate}" && break
  done
  if [ -z "${path}" ]; then
    echo "PROVENANCE.md names ${name}, which is not in the corpus" >&2
    failed=$((failed + 1))
    continue
  fi

  got="$(digest "${path}")"
  checked=$((checked + 1))
  if [ "${got}" != "${want}" ]; then
    echo "${name} has changed" >&2
    echo "  recorded ${want}" >&2
    echo "  actual   ${got}" >&2
    failed=$((failed + 1))
  fi
done < "${provenance}"

# Every file in the corpus must be accounted for, or a file could be added
# without provenance and the check would still pass.
present="$(find "${root}/fixtures/step/canonical" \
                  "${root}/fixtures/step/damaged" \
                  "${root}/fixtures/step/interoperability" \
  -type f \( -name '*.step' -o -name '*.stp' -o -name '*.txt' \) | wc -l | tr -d ' ')"
if [ "${checked}" -ne "${present}" ]; then
  echo "the corpus holds ${present} files and PROVENANCE.md records ${checked}" >&2
  failed=$((failed + 1))
fi

if [ "${failed}" -ne 0 ]; then
  echo "step corpus: ${failed} problem(s)" >&2
  exit 1
fi
echo "step corpus: ${checked} file(s) checked, all match PROVENANCE.md"
