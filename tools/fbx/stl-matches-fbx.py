#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Join an STL of one Body against the triangles pinned ufbx read from its FBX.

§27D. `read_production --triangles FILE.fbx` prints every triangle of the FBX
in world space, in the file's winding. This script reads the binary STL itself
(its own parser, nothing shared with the writers) and requires the two to be
the same set of oriented triangles under the documented conversion: an STL
point (x, y, z) in millimetres is the FBX point (x, z, -y) * 0.001 in metres.
A triangle is compared as a cyclic sequence, so its winding must agree; the
two files may list triangles in any order. Equal counts or sizes alone are
not accepted as equal geometry.

usage: stl-matches-fbx.py BODY.stl READER-OUTPUT.txt
"""
import math
import re
import struct
import sys

TOLERANCE_M = 1e-9  # 1 nm: far below float32 spacing at these sizes


def stl(path):
    data = open(path, "rb").read()
    if len(data) < 84:
        sys.exit(f"error: {path} has no binary STL header")
    (count,) = struct.unpack_from("<I", data, 80)
    if count == 0 or len(data) != 84 + 50 * count:
        sys.exit(f"error: {path} is not a binary STL of {count} triangles")
    out = []
    for i in range(count):
        corners = [struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)]
        if not all(math.isfinite(v) for p in corners for v in p):
            sys.exit(f"error: {path} has a non-finite STL position")
        out.append(tuple((x * 0.001, z * 0.001, -y * 0.001) for x, y, z in corners))
    return out


def fbx(path):
    out, summary, executed = [], None, False
    for line in open(path):
        if line.startswith("FCAD_TRIANGLE "):
            v = [float(t) for t in line.split()[1:]]
            if len(v) != 9 or not all(math.isfinite(x) for x in v):
                sys.exit(f"error: malformed triangle line: {line!r}")
            out.append((tuple(v[0:3]), tuple(v[3:6]), tuple(v[6:9])))
        elif line.startswith("FCAD_TRIANGLE_SUMMARY"):
            match = re.fullmatch(r"FCAD_TRIANGLE_SUMMARY instances=([1-9][0-9]*) triangles=([1-9][0-9]*)\n?", line)
            if summary is not None or match is None:
                sys.exit("error: malformed or repeated triangle summary")
            summary = int(match[2])
        elif line.startswith("FCAD_PRODUCTION_FBX_UFBX_EXECUTED"):
            match = re.fullmatch(r"FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=([1-9][0-9]*) failures=0\n?", line)
            if executed or match is None or int(match[1]) < 5:
                sys.exit(f"error: invalid reader result: {line.strip()}")
            executed = True
    if summary != len(out) or not executed:
        sys.exit("error: missing or inconsistent completed reader report")
    return out


def canonical(t):
    """The same oriented triangle whichever corner it is listed from."""
    k = min(range(3), key=lambda i: t[i])
    return t[k:] + t[:k]


def main():
    a = sorted(canonical(t) for t in stl(sys.argv[1]))
    b = sorted(canonical(t) for t in fbx(sys.argv[2]))
    if len(a) != len(b):
        sys.exit(f"error: STL has {len(a)} triangles and FBX {len(b)}")
    worst = 0.0
    for s, f in zip(a, b):
        for p, q in zip(s, f):
            worst = max(worst, max(abs(x - y) for x, y in zip(p, q)))
    if worst > TOLERANCE_M:
        sys.exit(f"error: STL and FBX differ by {worst} m, beyond {TOLERANCE_M} m")
    print(f"FCAD_STL_FBX_MATCH triangles={len(a)} worst_m={worst:.3g}")


if __name__ == "__main__":
    main()
