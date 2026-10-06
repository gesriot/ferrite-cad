#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§30H real-window inputs and comparison. Generator never creates outputs."""
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import sqlite3
import struct
import subprocess
import sys
import tempfile
import time

CORNER = [-4.5, 3.25]


def uuid7():
    """A fresh RFC 9562 UUIDv7, the only kind a document stores."""
    raw = bytearray(int(time.time() * 1000).to_bytes(6, "big") + os.urandom(10))
    raw[6] = (raw[6] & 0x0F) | 0x70
    raw[8] = (raw[8] & 0x3F) | 0x80
    return bytes(raw)


def main():
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[1] != "--compare"):
        sys.exit("usage: chamfer-session-gui.py [--compare] DIRECTORY_OUTSIDE_CHECKOUT")
    root = Path(sys.argv[-1]).resolve()
    cli = os.environ["FERRITECAD"]

    def run(*args):
        p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
        assert p.returncode == 0, (args, p.returncode, p.stdout, p.stderr)
        return json.loads(p.stdout)["result"]

    def inspect(path):
        return run("inspect", path, "--json")

    def resolved(path):
        assert run("validate", path, "--json")["valid"]
        p = subprocess.run([cli, "rebuild", str(path), "--cold"], capture_output=True, text=True)
        with sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True) as db:
            (n,) = db.execute("SELECT count(*) FROM topology_refs").fetchone()
        assert p.returncode == 0 and f"{n} of {n} stored references resolved" in p.stdout, (p.stdout, p.stderr)

    if len(sys.argv) == 2:
        existing = root
        while not existing.exists():
            existing = existing.parent
        probe = subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"], capture_output=True)
        if probe.returncode == 0:
            sys.exit("FCAD_30H_GUI_FIXTURE_REFUSED: fixtures must be outside a checkout")
        root.mkdir(parents=True, exist_ok=False)
        (root / "source").mkdir()
        (root / "work").mkdir()
        request = root / "base.json"
        # Clockwise, fractional and offset; the Chamfer is at the second corner,
        # between the second and third Lines, neither of them the first.
        request.write_text(json.dumps({"request_version": 1, "points_mm": [
            [33, 15.5], [33, 3.25], [-4.5, 3.25], [-4.5, 15.5]], "height_mm": 6.75}))
        plate = root / "source/plate.fcad"
        run("create-sketch-extrude", request, "-o", plate, "--json")
        catalog = inspect(plate)
        body = catalog["bodies"][0]
        chosen = next(c for c in body["chamfer_edge"]["target"]["candidates"] if c["corner_mm"] == CORNER)
        request = root / "chamfer.json"
        request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "distance_mm": 2.375}))
        current = root / "source/chamfer.fcad"
        run("chamfer-edge-copy", plate, "--body", body["body_id"], "--expect-version",
            catalog["content_version"], "--request", request, "-o", current, "--json")
        base = bytes.fromhex(chosen["edge"]["feature_id"].replace("-", ""))
        with sqlite3.connect(current) as db:
            # One more name owned by the base Extrude that resolves, then names,
            # rowids and ordinals that disagree with the feature history.
            db.execute("INSERT INTO topology_refs SELECT ?,owner_id,producer_feature,"
                       "expected_kind,payload,payload_hash FROM topology_refs WHERE owner_id=? LIMIT 1",
                       (uuid7(), base))
            db.executescript("UPDATE objects SET rowid=-rowid,ordinal=100-ordinal,name='same';"
                             "UPDATE capabilities SET rowid=rowid+100;"
                             "INSERT INTO capabilities(rowid,name,required) VALUES(900,'optional.extension',0);"
                             "CREATE TABLE extra_data(id INTEGER PRIMARY KEY,bytes BLOB);"
                             "INSERT INTO extra_data VALUES(91,x'010203');")
        resolved(current)
        saved = inspect(current)["chamfers"][0]
        assert saved["corner_mm"] == CORNER and saved["distance_mm"] == 2.375
        shutil.copyfile(current, root / "work/chamfer.fcad")
        (root / "facts.json").write_text(json.dumps({"sha256": hashlib.sha256(current.read_bytes()).hexdigest(),
                                                    "chamfer_uuid": saved["feature_id"],
                                                    "catalog": inspect(current)}, indent=2))
        print(f"FCAD_30H_GUI_FIXTURE_OK {root} chamfer={saved['feature_id']}")
        return

    facts = json.loads((root / "facts.json").read_text())
    source = root / "source/chamfer.fcad"
    assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["sha256"], "source changed"
    # Presence is checked before ANY peer job. CLI peers cannot fill GUI outputs.
    for name in ("after-apply.fcad", "after-refusal.fcad", "unsaved.stl", "unsaved.fbx",
                 "undo.stl", "saved.fcad", "branch.fcad", "after-saveas.fcad"):
        assert (root / name).is_file(), f"missing real GUI output: {name}"
    for name in ("after-apply.fcad", "after-refusal.fcad"):
        assert (root / name).read_bytes() == source.read_bytes(), f"source changed before Save: {name}"
    assert (root / "after-saveas.fcad").read_bytes() == (root / "saved.fcad").read_bytes(), "Save As changed prior file"

    def sql(path):
        with sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True) as db:
            result = {}
            for (table,) in db.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name"):
                quoted = '"' + table.replace('"', '""') + '"'
                try:
                    cur = db.execute(f"SELECT rowid,* FROM {quoted}")
                except sqlite3.OperationalError:
                    cur = db.execute(f"SELECT * FROM {quoted}")
                columns = [d[0] for d in cur.description]
                rows = []
                for row in cur:
                    row = list(row)
                    if table == "meta":
                        row[columns.index("modified_at")] = None
                    rows.append(tuple(row))
                result[table] = (columns, sorted(rows, key=repr))
            return result

    with tempfile.TemporaryDirectory(prefix="ferrite-30h-gui-peer-") as tmp:
        peer_root = Path(tmp)
        catalog = inspect(source)
        chamfer = catalog["chamfers"][0]
        # Every selection uses the saved UUID; duplicate names/ordinals are irrelevant.
        assert chamfer["feature_id"] == facts["chamfer_uuid"]
        base = chamfer["edge"]["feature_id"]
        current = peer_root / "height.fcad"
        run("edit-extrude", source, "--feature", base, "--expect-version", catalog["content_version"],
            "--distance-mm", 9.25, "-o", current, "--json")
        catalog = inspect(current)
        sketch = next(s for s in catalog["sketches"] if s["editable"] and s.get("vertices"))
        vertices = sketch["vertices"]
        for v in vertices:
            if v["start_mm"][0] == -4.5:
                v["start_mm"][0] = -5.75
        request = peer_root / "vertices.json"
        request.write_text(json.dumps({"request_version": 1, "vertices": vertices}))
        out = peer_root / "vertices.fcad"
        run("edit-sketch-copy", current, "--sketch", sketch["sketch_id"], "--expect-version",
            catalog["content_version"], "--request", request, "-o", out, "--json")
        current = out

        def distance_edit(current, distance, name):
            catalog = inspect(current)
            request = peer_root / f"{name}.json"
            request.write_text(json.dumps({"request_version": 1, "distance_mm": distance}))
            out = peer_root / f"{name}.fcad"
            run("edit-chamfer-distance", current, "--feature", facts["chamfer_uuid"], "--expect-version",
                catalog["content_version"], "--request", request, "-o", out, "--json")
            return out

        current = distance_edit(current, 3.0625, "first")
        current = distance_edit(current, 4.4375, "middle")
        before_last = current
        current = distance_edit(current, 6.125, "last")
        assert sql(root / "saved.fcad") == sql(current), "saved GUI differs from CLI"
        branch = distance_edit(before_last, 12.25 - 0.01, "branch")
        assert sql(root / "branch.fcad") == sql(branch), "branch GUI differs from CLI"
        for gui_name, model, kind in (("unsaved.stl", current, "stl"), ("unsaved.fbx", current, "fbx"),
                                       ("undo.stl", before_last, "stl")):
            output = peer_root / gui_name
            run(f"export-{kind}", model, "-o", output, "--json")
            assert (root / gui_name).read_bytes() == output.read_bytes(), f"GUI/CLI bytes: {gui_name}"
        for name in ("saved.fcad", "branch.fcad"):
            resolved(root / name)

        def measured(path, distance):
            """Volume, and the one plane of the chosen corner: its triangles face
            out of that corner and add up to d·√2·H."""
            data = path.read_bytes()
            count = struct.unpack_from("<I", data, 80)[0]
            assert len(data) == 84 + count * 50
            x0, y0, x1, y1, h = -5.75, 3.25, 33.0, 15.5, 9.25
            corner = (x0, y0)
            volume = flat = 0.0
            for i in range(count):
                a, b, c = (struct.unpack_from("<3f", data, 84 + i * 50 + 12 + j * 12) for j in range(3))
                volume += (a[0] * (b[1]*c[2]-b[2]*c[1]) + a[1] * (b[2]*c[0]-b[0]*c[2])
                           + a[2] * (b[0]*c[1]-b[1]*c[0])) / 6
                if all(abs(-(p[0] - corner[0]) - (p[1] - corner[1]) + distance) < 1e-4 for p in (a, b, c)):
                    u = [b[k] - a[k] for k in range(3)]
                    v = [c[k] - a[k] for k in range(3)]
                    n = [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]]
                    length = math.sqrt(sum(x * x for x in n))
                    assert (-n[0] - n[1]) / length / math.sqrt(2) > 1 - 1e-4, "the flat faces the corner"
                    flat += length / 2
            exact = ((x1 - x0) * (y1 - y0) - distance * distance / 2) * h
            assert abs(volume - exact) < 1e-3, (volume, exact)
            assert abs(flat - distance * math.sqrt(2) * h) < 1e-3, flat
            return volume, exact

        volume, exact = measured(root / "unsaved.stl", 6.125)
        measured(root / "undo.stl", 4.4375)
        print(f"FCAD_30H_GUI_COMPARE_OK analytical_mm3={exact:.9f} stl_mm3={volume:.9f}")


if __name__ == "__main__":
    main()
