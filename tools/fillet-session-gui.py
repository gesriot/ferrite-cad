#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§30G real-window inputs and comparison. Generator never creates outputs."""
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


def main():
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[1] != "--compare"):
        sys.exit("usage: fillet-session-gui.py [--compare] DIRECTORY_OUTSIDE_CHECKOUT")
    root = Path(sys.argv[-1]).resolve()
    cli = os.environ["FERRITECAD"]

    def run(*args):
        p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
        assert p.returncode == 0, (args, p.returncode, p.stdout, p.stderr)
        return json.loads(p.stdout)["result"]

    def inspect(path):
        return run("inspect", path, "--json")

    if len(sys.argv) == 2:
        existing = root
        while not existing.exists():
            existing = existing.parent
        probe = subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"], capture_output=True)
        if probe.returncode == 0:
            sys.exit("FCAD_30G_GUI_FIXTURE_REFUSED: fixtures must be outside a checkout")
        root.mkdir(parents=True, exist_ok=False)
        (root / "source").mkdir()
        (root / "work").mkdir()
        request = root / "base.json"
        request.write_text(json.dumps({"request_version": 1, "points_mm": [
            [33, 15.5], [33, 3.25], [-4.5, 3.25], [-4.5, 15.5]], "height_mm": 6.75}))
        current = root / "source/base.fcad"
        run("create-sketch-extrude", request, "-o", current, "--json")
        for i, (corner, radius) in enumerate((([33, 3.25], 2.375), ([-4.5, 15.5], 3.0625),
                                            ([33, 15.5], 1.5), ([-4.5, 3.25], 4.25))):
            catalog = inspect(current)
            body = catalog["bodies"][0]
            chosen = next(c for c in body["fillet_edge"]["target"]["candidates"] if c["corner_mm"] == corner)
            request = root / f"fillet-{i}.json"
            request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": radius}))
            out = root / "source" / ("fillets.fcad" if i == 3 else f"fillets-{i}.fcad")
            run("fillet-edge-copy", current, "--body", body["body_id"], "--expect-version",
                catalog["content_version"], "--request", request, "-o", out, "--json")
            current = out
        with sqlite3.connect(current) as db:
            db.executescript("UPDATE objects SET rowid=-rowid,ordinal=100-ordinal,name='same';"
                             "UPDATE capabilities SET rowid=rowid+100;"
                             "INSERT INTO capabilities(rowid,name,required) VALUES(900,'optional.extension',0);"
                             "CREATE TABLE extra_data(id INTEGER PRIMARY KEY,bytes BLOB);"
                             "INSERT INTO extra_data VALUES(91,x'010203');")
        shutil.copyfile(current, root / "work/fillets.fcad")
        (root / "facts.json").write_text(json.dumps({"sha256": hashlib.sha256(current.read_bytes()).hexdigest(),
                                                    "catalog": inspect(current)}, indent=2))
        print(f"FCAD_30G_GUI_FIXTURE_OK {root}")
        return

    facts = json.loads((root / "facts.json").read_text())
    source = root / "source/fillets.fcad"
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

    with tempfile.TemporaryDirectory(prefix="ferrite-30g-gui-peer-") as tmp:
        peer_root = Path(tmp)
        catalog = inspect(source)
        first = next(f for f in catalog["fillets"] if f["history_index"] == 1)
        # Every selection uses the saved UUID; duplicate names/ordinals are irrelevant.
        base = first["edge"]["feature_id"]
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

        def radius_edit(current, index, radius, name):
            catalog = inspect(current)
            chosen = next(f for f in catalog["fillets"] if f["history_index"] == index)
            request = peer_root / f"{name}.json"
            request.write_text(json.dumps({"request_version": 1, "radius_mm": radius}))
            out = peer_root / f"{name}.fcad"
            run("edit-fillet-radius", current, "--feature", chosen["feature_id"], "--expect-version",
                catalog["content_version"], "--request", request, "-o", out, "--json")
            return out

        current = radius_edit(current, 1, 2.75, "first")
        current = radius_edit(current, 2, 3.4375, "middle")
        before_last = current
        current = radius_edit(current, 4, 4.625, "last")
        assert sql(root / "saved.fcad") == sql(current), "saved GUI differs from CLI"
        branch = radius_edit(before_last, 4, 4.75, "branch")
        assert sql(root / "branch.fcad") == sql(branch), "branch GUI differs from CLI"
        for gui_name, model, kind in (("unsaved.stl", current, "stl"), ("unsaved.fbx", current, "fbx"),
                                       ("undo.stl", before_last, "stl")):
            output = peer_root / gui_name
            run(f"export-{kind}", model, "-o", output, "--json")
            assert (root / gui_name).read_bytes() == output.read_bytes(), f"GUI/CLI bytes: {gui_name}"
        for name in ("saved.fcad", "branch.fcad"):
            assert run("validate", root / name, "--json")["valid"]
            p = subprocess.run([cli, "rebuild", str(root / name), "--cold"], capture_output=True, text=True)
            assert p.returncode == 0 and "stored references resolved" in p.stdout, (p.stdout, p.stderr)
        data = (root / "unsaved.stl").read_bytes()
        count = struct.unpack_from("<I", data, 80)[0]
        assert len(data) == 84 + count * 50
        volume = 0
        for i in range(count):
            a, b, c = (struct.unpack_from("<3f", data, 84 + i * 50 + 12 + j * 12) for j in range(3))
            volume += (a[0] * (b[1]*c[2]-b[2]*c[1]) + a[1] * (b[2]*c[0]-b[0]*c[2])
                       + a[2] * (b[0]*c[1]-b[1]*c[0])) / 6
        exact = 38.75 * 12.25 * 9.25 - (1-math.pi/4) * sum(r*r for r in (2.75, 3.4375, 1.5, 4.625)) * 9.25
        assert 0.995 * exact < volume < 1.005 * exact, (volume, exact)
        print(f"FCAD_30G_GUI_COMPARE_OK analytical_mm3={exact:.9f} stl_mm3={volume:.9f}")


if __name__ == "__main__":
    main()
