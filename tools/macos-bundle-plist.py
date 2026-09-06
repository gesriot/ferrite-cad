# SPDX-License-Identifier: MIT
"""Read the requested string keys from the root dictionary of an Info.plist.

Used by macos-bundle.sh on every host. plistlib understands XML and binary
property lists, nesting and escaped text; a tag scan can mistake a nested
CFBundleExecutable for the application's entry point. Duplicate keys are
rejected instead of depending on a reader's first/last-value policy.
"""

import json
import plistlib
import sys
from xml.parsers.expat import ExpatError


class UniqueKeys(dict):
    def __setitem__(self, key, value):
        if key in self:
            raise ValueError(f"duplicate dictionary key: {key!r}")
        super().__setitem__(key, value)


def main():
    path, *keys = sys.argv[1:]
    try:
        with open(path, "rb") as source:
            properties = plistlib.load(source, dict_type=UniqueKeys)
        if not isinstance(properties, dict):
            raise ValueError("the root of Info.plist is not a dictionary")
    except (OSError, ValueError, TypeError, OverflowError, ExpatError, plistlib.InvalidFileException) as error:
        print(f"{path} is not a readable property list: {error}", file=sys.stderr)
        return 1

    result = {}
    for key in keys:
        value = properties.get(key)
        if not isinstance(value, str) or not value:
            print(f"{path} says nothing for {key} (expected a nonempty root string)", file=sys.stderr)
            return 1
        # These product identity fields are also consumed by shell callers.
        # Reject control characters rather than let command substitution or
        # host-specific line endings change the executable being checked.
        if any(ord(character) < 32 or ord(character) == 127 for character in value):
            print(f"{path} has a control character in {key}", file=sys.stderr)
            return 1
        result[key] = value
    json.dump(result, sys.stdout, ensure_ascii=True)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
