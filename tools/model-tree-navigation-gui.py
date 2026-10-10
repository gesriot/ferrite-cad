#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§31A: input-only fixtures; comparator requires actual window outputs."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

NAMES = ("cuts", "copy", "fillets", "chamfer", "revolve")
OUTPUTS = ("work/cuts.fcad", "unsaved.stl", "unsaved.fbx", "undo.stl", "tabs/last-window")
GATE = "sessions::tests::tabs::drafts::model_tree::native_compare_real_model_tree_window_outputs"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[1] != "--compare"):
        sys.exit("usage: model-tree-navigation-gui.py [--compare] DIRECTORY_OUTSIDE_CHECKOUT")
    root = Path(sys.argv[-1]).resolve()
    existing = root
    while not existing.exists():
        existing = existing.parent
    if subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"],
                      capture_output=True).returncode == 0:
        sys.exit("FCAD_31A_GUI_REFUSED: fixtures and outputs must be outside a checkout")
    if len(sys.argv) == 3:
        facts = json.loads((root / "facts.json").read_text())
        for name in NAMES:
            assert digest(root / "inputs" / f"{name}.fcad") == facts[name]["sha256"], name
        missing = [name for name in OUTPUTS if not (root / name).is_file()]
        if missing:
            sys.exit("FCAD_31A_GUI_REFUSED: missing real GUI outputs: " + ", ".join(missing))
        assert digest(root / "work/cuts.fcad") != facts["cuts"]["sha256"], "cuts were never saved"
        for name in NAMES[1:]:
            assert digest(root / "work" / f"{name}.fcad") == facts[name]["sha256"], f"{name} changed"
        result = subprocess.run(["cargo", "test", "--release", "--features", "planegcs", "-p",
                                 "ferritecad-app", "--bin", "ferritecad-viewer", GATE,
                                 "--", "--exact", "--nocapture", "--test-threads=1"],
                                env=dict(os.environ, FCAD_31A_GUI_DIR=str(root)), capture_output=True, text=True)
        print(result.stdout, end="")
        print(result.stderr, end="", file=sys.stderr)
        evidence = result.stdout + result.stderr
        assert (result.returncode == 0 and "skipped:" not in evidence and
                f"test {GATE} ... " in evidence and "test result: ok. 1 passed; 0 failed" in evidence and
                "FCAD_31A_GUI_COMPARE_OK all_SQL_cells=true controls=3" in evidence), "exact comparator did not execute"
        return
    cli = os.environ["FERRITECAD"]
    root.mkdir(parents=True, exist_ok=False)
    for name in ("inputs", "work", "recovery", "tabs"):
        (root / name).mkdir(mode=0o700)

    def run(*args):
        result = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
        assert result.returncode == 0, (args, result.stdout, result.stderr)
        return json.loads(result.stdout)["result"]

    def request(name, content):
        path = root / "inputs" / f"{name}.json"
        path.write_text(json.dumps(content))
        return path

    base = root / "inputs/base.fcad"
    req = request("base", {"request_version": 1, "points_mm": [
        [-2.5, -1.25], [84, -1.25], [84, 28], [54, 28], [54, 55], [-2.5, 55]], "height_mm": 12.75})
    run("create-sketch-extrude", req, "-o", base, "--json")
    current = base
    for i in range(3):
        catalog = run("inspect", current, "--json")
        slot = i * 7 % 16
        req = request(f"cut-{i}", {"request_version": 2,
            "center_mm": [7.125 + slot % 4 * 12, 6.375 + slot // 4 * 12], "radius_mm": 1.375 + i % 3 * .25,
            "extent": {"kind": "through_all"} if i % 2 == 0 else {"kind": "blind", "depth_mm": 4.375 + i % 3}})
        out = root / "inputs" / ("cuts.fcad" if i == 2 else f"cut-{i}.fcad")
        run("cut-circular-copy", current, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
            catalog["content_version"], "--request", req, "-o", out, "--json")
        current = out
    shutil.copyfile(current, root / "inputs/copy.fcad")
    corner_plate = root / "inputs/corner-plate.fcad"
    req = request("corner-plate", {"request_version": 1, "points_mm": [[33, 15.5], [33, 3.25], [-4.5, 3.25], [-4.5, 15.5]], "height_mm": 6.75})
    run("create-sketch-extrude", req, "-o", corner_plate, "--json")
    for feature, corner, value in (("fillet", [33, 3.25], 2.375), ("chamfer", [-4.5, 3.25], 2.375)):
        catalog = run("inspect", corner_plate, "--json")
        body = catalog["bodies"][0]
        chosen = next(c for c in body[f"{feature}_edge"]["target"]["candidates"] if c["corner_mm"] == corner)
        req = request(feature, {"request_version": 1, "edge": chosen["edge"],
                               "radius_mm" if feature == "fillet" else "distance_mm": value})
        out = root / "inputs" / ("fillets.fcad" if feature == "fillet" else "chamfer.fcad")
        run(f"{feature}-edge-copy", corner_plate, "--body", body["body_id"], "--expect-version",
            catalog["content_version"], "--request", req, "-o", out, "--json")
    req = request("revolve", {"request_version": 2, "points_mm": [[2, 0], [8, 0], [8, 10], [2, 10]],
                              "axis": "sketch_y", "extent": {"kind": "angle", "degrees": 210}})
    run("create-sketch-revolve", req, "-o", root / "inputs/revolve.fcad", "--json")
    facts = {}
    for name in NAMES:
        path = root / "inputs" / f"{name}.fcad"
        shutil.copyfile(path, root / "work" / path.name)
        graph = subprocess.run([cli, "dump-graph", str(path)], capture_output=True, text=True)
        assert graph.returncode == 0, graph.stderr
        facts[name] = {"sha256": digest(path), "catalog": run("inspect", path, "--json"),
                       "graph": graph.stdout}
    (root / "facts.json").write_text(json.dumps(facts, indent=2))
    print(f"FCAD_31A_GUI_FIXTURE_OK inputs_only=true {root}")


if __name__ == "__main__":
    main()
