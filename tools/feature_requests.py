#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Build unitful edit requests from the stored facts used by GUI fixtures."""
import os
import re
import subprocess
import time
import uuid


def new_id():
    raw = bytearray(int(time.time() * 1000).to_bytes(6, "big") + os.urandom(10))
    raw[6] = (raw[6] & 15) | 112
    raw[8] = (raw[8] & 63) | 128
    return str(uuid.UUID(bytes=bytes(raw)))


def base_profile(catalog):
    base_id = catalog["bodies"][0]["history"][0]
    base = next(f for f in catalog["features"] if f["feature_id"] == base_id)
    sketch = next(s for s in catalog["sketches"] if s["sketch_id"] == base["profile_sketch_id"])
    return base_id, sketch


def edge_at(catalog, corner, path):
    base, sketch = base_profile(catalog)
    joint = sorted(c["curve_id"] for c in sketch["curves"] if not c["construction"]
                   and c["geometry"]["kind"] == "line"
                   and corner in [c["geometry"]["start_mm"], c["geometry"]["end_mm"]])
    assert len(joint) == 2, (corner, joint)
    # The declared fixture corner supplies exact curve identities. Resolve its
    # named incidence from public topology; never search B-Rep coordinates.
    result = subprocess.run([os.environ["FERRITECAD"], "print-topology", str(path)],
                            capture_output=True, text=True)
    assert result.returncode == 0, (result.stdout, result.stderr)
    lines = result.stdout.splitlines()
    # A kept name is printed under every feature whose result holds it, with
    # the producer that minted it, so the same digest may appear several times.
    digests = set()
    for header, description in zip(lines, lines[1:]):
        match = re.fullmatch(r"\s*producer=(\S+) kind=edge digest=(\w{64}) entities=1", header)
        if match and match[1] == base:
            curves = set(re.findall(r" = curve ([0-9a-f-]+),", description))
            if curves == set(joint) and " cap," not in description:
                digests.add(match[2])
    assert len(digests) == 1, (joint, digests)
    return {"producer": base, "kind": "edge", "digest": digests.pop(), "selection": "one"}


def edge_addition(catalog, kind, corner, value, path):
    field = "radius" if kind == "fillet" else "distance"
    return {"request_version": 1, "changes": [{"kind": "add_feature", "object": new_id(),
        "body": catalog["bodies"][0]["body_id"], "name": kind.title(),
        "feature": {"kind": kind, "edges": [edge_at(catalog, corner, path)],
                    field: {"value": value, "unit": "mm"}}}]}


def circular_cut(catalog, center, radius, depth=None):
    _, base = base_profile(catalog)
    sketch, curve = new_id(), new_id()
    mm = lambda value: {"value": value, "unit": "mm"}
    end = {"kind": "through_all"} if depth is None else {"kind": "blind", "distance": mm(depth)}
    return {"request_version": 1, "changes": [
        {"kind": "add_sketch", "object": sketch, "name": "Cut tool", "plane": base["plane_id"],
         "curves": [{"id": curve, "construction": False, "geometry": {"kind": "circle",
                     "center": {"x": mm(center[0]), "y": mm(center[1])}, "radius": mm(radius)}}],
         "constraints": []},
        {"kind": "add_feature", "object": new_id(), "body": catalog["bodies"][0]["body_id"],
         "name": "Cut", "feature": {"kind": "extrude", "operation": "cut", "profile": sketch,
          "end_condition": end, "reversed": False}}]}
