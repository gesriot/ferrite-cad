#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§30J inputs and comparison for real window artifacts. Never creates outputs."""
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
        sys.exit("usage: add-fillet-session-gui.py [--compare] DIRECTORY_OUTSIDE_CHECKOUT")
    root = Path(sys.argv[-1]).resolve()
    if len(sys.argv) == 3:
        facts = json.loads((root / "facts.json").read_text())
        source = root / "source/plate.fcad"
        assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["sha256"], "source changed"
        # Check presence BEFORE invoking any build or peer job. Missing output is refusal;
        # the Rust comparator refuses again before its first peer job.
        missing = [name for name in OUTPUTS if not (root / name).is_file()]
        if missing:
            sys.exit("FCAD_30J_GUI_COMPARE_REFUSED: missing real GUI output: " + ", ".join(missing))
        command = ["cargo", "test", "--release", "--features", "planegcs", "-p", "ferritecad-app",
                   "--bin", "ferritecad-viewer",
                   "sessions::tests::add_fillet::native_compare_real_add_fillet_gui_artifacts_with_negative_controls",
                   "--", "--exact", "--nocapture", "--test-threads=1"]
        sys.exit(subprocess.run(command, env=dict(os.environ, FCAD_30J_GUI_DIR=str(root))).returncode)
    cli = os.environ["FERRITECAD"]
    existing = root
    while not existing.exists():
        existing = existing.parent
    probe = subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"], capture_output=True)
    if probe.returncode == 0:
        sys.exit("FCAD_30J_GUI_FIXTURE_REFUSED: fixtures must be outside a checkout")
    root.mkdir(parents=True, exist_ok=False)
    (root / "source").mkdir()
    (root / "work").mkdir()

    def run(*args):
        p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
        assert p.returncode == 0, (args, p.stdout, p.stderr)
        return json.loads(p.stdout)["result"]

    # The asymmetric, translated plate of the native gates, drawn clockwise.
    request = root / "plate.json"
    request.write_text(json.dumps({"request_version": 1, "points_mm": [
        [33, 15.5], [33, 3.25], [-4.5, 3.25], [-4.5, 15.5]], "height_mm": 6.75}))
    source = root / "source/plate.fcad"
    run("create-sketch-extrude", request, "-o", source, "--json")
    shutil.copyfile(source, root / "work/plate.fcad")
    (root / "facts.json").write_text(json.dumps({"sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                                                "catalog": run("inspect", source, "--json")}, indent=2))
    print(f"FCAD_30J_GUI_FIXTURE_OK {root}")


if __name__ == "__main__":
    main()
