#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
commit="fcc5d6ba444cfd3eb80677dba5e37e493941abe5"
cache="$project/scripts/.cache/ufbx-$commit"
mkdir -p "$cache"

fetch() {
  local name="$1"
  local expected="$2"
  local path="$cache/$name"
  if [ ! -f "$path" ]; then
    curl -fsSL "https://raw.githubusercontent.com/ufbx/ufbx/$commit/$name" -o "$path"
  fi
  local actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$path" | cut -d' ' -f1)"
  else
    actual="$(shasum -a 256 "$path" | cut -d' ' -f1)"
  fi
  if [ "$actual" != "$expected" ]; then
    echo "ufbx $name digest mismatch: $actual" >&2
    exit 1
  fi
}

fetch ufbx.c 7d8d6ae4373f71692f295ff49ee0826466306ebcaa80b0e587c13ed047b98cea
fetch ufbx.h 942481725372d2ac4da5e77a062b47c20054a3440e7ee09a6043f99fe1f130ed
printf '%s\n' "$cache"
