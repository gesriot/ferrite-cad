# SPDX-License-Identifier: MIT
"""Unitful transaction builders for executable examples."""
import os
import time
from uuid import UUID


def new_id():
    raw = bytearray(int(time.time() * 1000).to_bytes(6, "big") + os.urandom(10))
    raw[6] = (raw[6] & 0x0F) | 0x70
    raw[8] = (raw[8] & 0x3F) | 0x80
    return str(UUID(bytes=bytes(raw)))


def number(value, unit="mm"):
    return {"value": value, "unit": unit}


def point(value):
    return {"x": number(value[0]), "y": number(value[1])}


def point_ref(curve, at):
    return {"curve": curve, "at": at}


def segment(curve):
    return {"from": point_ref(curve, "start"), "to": point_ref(curve, "end")}


def line_rule(curve, kind):
    return {"kind": kind, "a": point_ref(curve, "start"), "b": point_ref(curve, "end")}


def distance(curve, value):
    return dict(line_rule(curve, "distance"), distance=number(value))


def fixed(curve, at, x, y):
    return {"kind": "fixed", "point": point_ref(curve, at), "x": number(x), "y": number(y)}


def relation(a, b, kind):
    return {"kind": kind, "a": segment(a), "b": segment(b)}


def line_request(sketch, curves, points):
    assert len(curves) == len(points)
    return {"request_version": 1, "changes": [
        {"kind": "set_curve_geometry", "sketch": sketch, "curve": curve,
         "geometry": {"kind": "line", "start": point(points[i]),
                      "end": point(points[(i + 1) % len(points)])}}
        for i, curve in enumerate(curves)]}


def constraint_request(sketch, remove, rules):
    changes = [{"kind": "remove_constraint", "sketch": sketch["sketch_id"], "constraint": id}
               for id in remove]
    add = []
    if rules and not sketch["constraints"]:
        curves = sketch["curves"]
        for i, curve in enumerate(curves):
            following = curves[(i + 1) % len(curves)]
            if (curve["geometry"]["kind"] == "line"
                    and curve["geometry"]["end_mm"] == following["geometry"]["start_mm"]):
                add.append({"kind": "coincident", "a": point_ref(curve["curve_id"], "end"),
                            "b": point_ref(following["curve_id"], "start")})
    for rule in [*add, *rules]:
        changes.append({"kind": "add_constraint", "sketch": sketch["sketch_id"],
                        "constraint": {"id": new_id(), "rule": rule}})
    return {"request_version": 1, "changes": changes}
