#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§30L inputs and comparison for real window artifacts. Never creates outputs."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

OUTPUTS = ("unsaved.stl", "unsaved.fbx", "first-save.fcad", "undo.stl", "after-guard.fcad",
           "plate.fcad", "annulus.stl", "annulus.fbx", "empty.fcad")


def main():
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[1] != "--compare"):
        sys.exit("usage: new-document-session-gui.py [--compare] DIRECTORY_OUTSIDE_CHECKOUT")
    root = Path(sys.argv[-1]).resolve()
    if len(sys.argv) == 3:
        facts = json.loads((root / "facts.json").read_text())
        pristine = root / "inputs/occupied.fcad"
        assert hashlib.sha256(pristine.read_bytes()).hexdigest() == facts["sha256"], "inputs changed"
        # Check presence BEFORE invoking any build or peer job. Missing output is refusal;
        # the Rust comparator refuses again before its first peer job.
        missing = [name for name in OUTPUTS if not (root / name).is_file()]
        if missing:
            sys.exit("FCAD_30L_GUI_COMPARE_REFUSED: missing real GUI output: " + ", ".join(missing))
        command = ["cargo", "test", "--release", "--features", "planegcs", "-p", "ferritecad-app",
                   "--bin", "ferritecad-viewer",
                   "sessions::tests::new_document::native_compare_real_new_document_gui_artifacts_with_negative_controls",
                   "--", "--exact", "--nocapture", "--test-threads=1"]
        sys.exit(subprocess.run(command, env=dict(os.environ, FCAD_30L_GUI_DIR=str(root))).returncode)
    cli = os.environ["FERRITECAD"]
    existing = root
    while not existing.exists():
        existing = existing.parent
    probe = subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"], capture_output=True)
    if probe.returncode == 0:
        sys.exit("FCAD_30L_GUI_FIXTURE_REFUSED: fixtures must be outside a checkout")
    root.mkdir(parents=True, exist_ok=False)
    (root / "inputs").mkdir()
    # The one input: a file already at the name the first Save must refuse.
    occupied = root / "occupied.fcad"
    p = subprocess.run([cli, "create", str(occupied), "--json"], capture_output=True, text=True)
    assert p.returncode == 0, (p.stdout, p.stderr)
    shutil.copyfile(occupied, root / "inputs/occupied.fcad")
    (root / "facts.json").write_text(json.dumps(
        {"sha256": hashlib.sha256(occupied.read_bytes()).hexdigest()}, indent=2))
    print(f"FCAD_30L_GUI_FIXTURE_OK {root}")


if __name__ == "__main__":
    main()
