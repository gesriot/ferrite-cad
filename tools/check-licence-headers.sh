#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Checks that every FerriteCAD-authored source and build file declares its
# licence.
#
# The repository is MIT and says so in LICENSE, in README and in every crate
# manifest. That is enough for a person reading the repository and not enough
# for a file that has been copied out of it: a source file lifted into another
# project carries no licence at all unless it says so itself.
#
# Run from the repository root:
#   tools/check-licence-headers.sh

set -euo pipefail

readonly SPDX='SPDX-License-Identifier: MIT'

missing=()
checked=0

while IFS= read -r -d '' file; do
    case "$file" in
        # Cargo manifests declare `license` as a structured field, which is what
        # cargo, crates.io and cargo-deny actually read. A comment would be a
        # second, unchecked copy of the same fact.
        */Cargo.toml | Cargo.toml) continue ;;
    esac

    case "$file" in
        *.rs | *.c | *.cc | *.cpp | *.cxx | *.h | *.hpp | *.hxx | *.cmake | \
            *.yml | *.yaml | *.toml | *.sh | */CMakeLists.txt | CMakeLists.txt) ;;
        *) continue ;;
    esac

    checked=$((checked + 1))
    # Only the opening lines: a mention buried in the middle of a file is
    # prose about licensing, not a declaration of this file's licence.
    if ! head -n 5 "./$file" | grep -qF "$SPDX"; then
        missing+=("$file")
    fi
done < <(git ls-files -z)

if [ ${#missing[@]} -gt 0 ]; then
    echo "error: ${#missing[@]} file(s) do not declare '$SPDX' in their first 5 lines:" >&2
    printf '  %s\n' "${missing[@]}" >&2
    echo >&2
    echo "Add a comment header, e.g. '// $SPDX' or '# $SPDX'." >&2
    exit 1
fi

echo "licence headers: $checked file(s) checked, all declare MIT"
