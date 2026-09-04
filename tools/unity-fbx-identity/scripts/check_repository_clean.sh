#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Refuses if a measurement left anything Unity produced inside the repository.
#
# The temporary projects live outside it and `measurement-output` is ignored,
# but neither of those is a guarantee: an ignore rule that stops matching, or a
# probe that writes somewhere else, would put an imported `.fbx`, its `.meta`
# or an editor cache into a commit without anyone noticing. This is the check
# that would notice.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$root"

leaked="$(git status --porcelain --untracked-files=all \
  | cut -c4- \
  | grep -E '\.(fbx|meta)$|(^|/)(Library|Temp|Logs|UserSettings)/' || true)"

if [ -n "$leaked" ]; then
  echo "a measurement left files Unity produced in the repository:" >&2
  printf '  %s\n' $leaked >&2
  exit 1
fi
echo "FCAD_IDENTITY_REPOSITORY_CLEAN"
