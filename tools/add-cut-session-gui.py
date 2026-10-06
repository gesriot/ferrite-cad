#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§30I inputs and comparison for real window artifacts. Never creates outputs."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

OUTPUTS = ("after-add.fcad", "after-refusal.fcad", "unsaved.stl", "unsaved.fbx",
           "undo.stl", "saved.fcad", "branch.fcad", "after-saveas.fcad")


def main():
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[1] != "--compare"):
        sys.exit("usage: add-cut-session-gui.py [--compare] DIRECTORY_OUTSIDE_CHECKOUT")
    root = Path(sys.argv[-1]).resolve()
    if len(sys.argv) == 3:
        facts = json.loads((root / "facts.json").read_text())
        source = root / "source/cuts.fcad"
        assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["sha256"], "source changed"
        # Check presence BEFORE invoking any peer jobs. Missing output is refusal.
        missing = [name for name in OUTPUTS if not (root / name).is_file()]
        if missing:
            sys.exit("FCAD_30I_GUI_COMPARE_REFUSED: missing real GUI output: " + ", ".join(missing))
        command = ["cargo", "test", "--release", "--features", "planegcs", "-p", "ferritecad-app",
                   "--bin", "ferritecad-viewer",
                   "sessions::tests::add_cut::native_compare_real_add_cut_gui_artifacts_with_negative_controls",
                   "--", "--exact", "--nocapture", "--test-threads=1"]
        sys.exit(subprocess.run(command, env=dict(os.environ, FCAD_30I_GUI_DIR=str(root))).returncode)
    cli = os.environ["FERRITECAD"]
    existing = root
    while not existing.exists():
        existing = existing.parent
    probe = subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"], capture_output=True)
    if probe.returncode == 0:
        sys.exit("FCAD_30I_GUI_FIXTURE_REFUSED: fixtures must be outside a checkout")
    root.mkdir(parents=True, exist_ok=False)
    (root / "source").mkdir()
    (root / "work").mkdir()

    def run(*args):
        p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
        assert p.returncode == 0, (args, p.stdout, p.stderr)
        return json.loads(p.stdout)["result"]

    request = root / "base.json"
    request.write_text(json.dumps({"request_version": 1, "points_mm": [
        [-2.5, -1.25], [84, -1.25], [84, 28], [54, 28], [54, 55], [-2.5, 55]], "height_mm": 12.75}))
    current = root / "source/base.fcad"
    run("create-sketch-extrude", request, "-o", current, "--json")
    for i in range(3):
        catalog = run("inspect", current, "--json")
        slot = i * 7 % 16
        request = root / f"cut-{i}.json"
        request.write_text(json.dumps({"request_version": 2,
            "center_mm": [7.125 + slot % 4 * 12, 6.375 + slot // 4 * 12],
            "radius_mm": 1.375 + i % 3 * 0.25,
            "extent": {"kind": "through_all"} if i % 2 == 0 else {"kind": "blind", "depth_mm": 4.375 + i % 3}}))
        out = root / "source" / ("cuts.fcad" if i == 2 else f"cuts-{i}.fcad")
        run("cut-circular-copy", current, "--body", catalog["bodies"][0]["body_id"],
            "--expect-version", catalog["content_version"], "--request", request, "-o", out, "--json")
        current = out
    shutil.copyfile(current, root / "work/cuts.fcad")
    (root / "facts.json").write_text(json.dumps({"sha256": hashlib.sha256(current.read_bytes()).hexdigest(),
                                                "catalog": run("inspect", current, "--json")}, indent=2))
    print(f"FCAD_30I_GUI_FIXTURE_OK {root}")


if __name__ == "__main__":
    main()
