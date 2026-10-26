#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Compare the measured ThroughAll corpus, independently of test names and IDs."""
import csv
import math
from pathlib import Path
import sys

KEYS = ("target", "reversed", "operation")
COUNTS = ("faces", "edges", "vertices", "ambiguous")
NUMBERS = ("low", "high", "length", "volume")


def read(path):
    with path.open(encoding="utf-8", newline="") as stream:
        reader = csv.DictReader(stream, delimiter="\t")
        if reader.fieldnames != [*KEYS, *NUMBERS, *COUNTS]:
            raise ValueError(f"{path}: unknown measurement schema")
        rows = {}
        for row in reader:
            key = tuple(row[k] for k in KEYS)
            if key in rows:
                raise ValueError(f"{path}: duplicate case {key}")
            values = {k: float(row[k]) for k in NUMBERS}
            if not all(math.isfinite(v) for v in values.values()):
                raise ValueError(f"{path}: nonfinite measurement")
            values.update({k: int(row[k]) for k in COUNTS})
            rows[key] = values
    if not rows:
        raise ValueError(f"{path}: empty corpus")
    return rows


def main():
    root = Path(sys.argv[1])
    measurements = {}
    for mode in (0, 1):
        for platform in ("linux", "macos", "windows"):
            name = f"through-all-{mode}-{platform}.tsv"
            files = list(root.rglob(name))
            if len(files) != 1:
                raise ValueError(f"expected one {name}, found {len(files)}")
            measurements[(mode, platform)] = read(files[0])
    reference = measurements[(0, "linux")]
    for identity, rows in measurements.items():
        if rows.keys() != reference.keys():
            raise ValueError(f"{identity}: corpus cases differ")
        for case, expected in reference.items():
            for key, value in expected.items():
                actual = rows[case][key]
                # Compare geometric measurements at sub-tolerance precision;
                # topology and ambiguity cardinalities must agree exactly.
                equal = (actual == value if key in COUNTS else
                         math.isclose(actual, value, rel_tol=1e-10, abs_tol=1e-8))
                if not equal:
                    raise ValueError(f"{identity} {case} {key}: {actual} != {value}")
    print("ThroughAll bounds, tool lengths, volume and topology "
          "agree across platforms and runtimes")


if __name__ == "__main__":
    main()
