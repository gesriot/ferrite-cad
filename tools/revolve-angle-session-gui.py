#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§30E fixtures for real window output. Never creates the window's result files."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def main():
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[1] != "--compare"):
        sys.exit("usage: revolve-angle-session-gui.py [--compare] DIRECTORY_OUTSIDE_CHECKOUT")
    root = Path(sys.argv[-1]).resolve()
    if len(sys.argv) == 3:
        facts = json.loads((root / "facts.json").read_text())
        for name in ("radial", "axis", "constrained"):
            actual = hashlib.sha256((root / "source" / f"{name}.fcad").read_bytes()).hexdigest()
            assert actual == facts[name]["sha256"], f"immutable source/{name}.fcad changed"
        env = dict(os.environ, FCAD_30E_GUI_DIR=str(root))
        command = ["cargo", "test", "--release", "--features", "planegcs", "-p", "ferritecad-app",
                   "--bin", "ferritecad-viewer",
                   "sessions::tests::angle::native_compare_real_angle_gui_artifacts_with_negative_controls",
                   "--", "--exact", "--nocapture", "--test-threads=1"]
        sys.exit(subprocess.run(command, env=env).returncode)
    cli = os.environ["FERRITECAD"]
    existing = root
    while not existing.exists():
        existing = existing.parent
    inside = subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"],
                            capture_output=True, text=True)
    if inside.returncode == 0:
        sys.exit("FCAD_30E_GUI_FIXTURE_REFUSED: fixtures must be outside a checkout")
    root.mkdir(parents=True, exist_ok=False)
    (root / "source").mkdir()
    (root / "work").mkdir()

    def run(*args):
        p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
        assert p.returncode == 0, (args, p.stdout, p.stderr)
        return json.loads(p.stdout)["result"]

    def create(name, inner):
        req = root / f"{name}.json"
        req.write_text(json.dumps({"request_version": 2, "points_mm": [
            [inner, -3.25], [7.5, -3.25], [7.5, 9.5], [inner, 9.5]],
            "axis": "sketch_y", "extent": {"kind": "angle", "degrees": 137.5}}))
        path = root / "source" / f"{name}.fcad"
        run("create-sketch-revolve", req, "-o", path, "--json")
        return path

    radial = create("radial", 2.75)
    create("axis", 0.0)
    before = create("constrained-before", 2.75)
    catalog = run("inspect", before, "--json")
    row = catalog["sketches"][0]
    lines = row["constraint_edit"]["curves"]
    ids = [c["curve_id"] for c in lines]
    add = [{"curve_id": ids[i], "rule": "horizontal" if i % 2 == 0 else "vertical"} for i in range(4)]
    add += [{"curve_id": ids[0], "rule": "fixed", "at": "start", "x_mm": 2.75, "y_mm": -3.25},
            {"curve_id": ids[0], "rule": "distance", "distance_mm": 4.75},
            {"curve_id": ids[1], "rule": "distance", "distance_mm": 12.75}]
    req = root / "constraints.json"
    req.write_text(json.dumps({"request_version": 1, "remove": [], "add": add}))
    run("edit-sketch-constraints-copy", before, "--sketch", row["sketch_id"], "--expect-version",
        catalog["content_version"], "--request", req, "-o", root / "source/constrained.fcad", "--json")
    facts = {}
    for name in ("radial", "axis", "constrained"):
        src = root / "source" / f"{name}.fcad"
        shutil.copyfile(src, root / "work" / src.name)
        facts[name] = {"sha256": hashlib.sha256(src.read_bytes()).hexdigest(),
                       "catalog": run("inspect", src, "--json")}
    (root / "facts.json").write_text(json.dumps(facts, indent=2))
    assert radial.exists()
    print(f"FCAD_30E_GUI_FIXTURE_OK {root}")


if __name__ == "__main__":
    main()
