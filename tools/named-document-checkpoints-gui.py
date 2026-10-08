#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§30N inputs and comparison for real window artifacts. Never creates outputs.

Inputs: a sample plate made by the shipped CLI, the committed schema v3 plate as
`old.fcad`, their pristine copies, and an empty recovery folder the viewer is
pointed at with FERRITECAD_RECOVERY_DIR (an explicit test folder: the person's
own recovery folder is never touched). Outputs are made by the real window only;
`--compare` refuses before any build or peer job when one is missing.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

OUTPUTS = ("plate.fcad", "restored.stl", "restored.fbx", "old-saved.fcad")
INPUTS = ("plate.fcad", "old.fcad")
OLD = Path(__file__).resolve().parent.parent / "crates/ferritecad-fixtures/plate/plate.fcad"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[1] != "--compare"):
        sys.exit("usage: named-document-checkpoints-gui.py [--compare] DIRECTORY_OUTSIDE_CHECKOUT")
    root = Path(sys.argv[-1]).resolve()
    if len(sys.argv) == 3:
        facts = json.loads((root / "facts.json").read_text())
        for name in INPUTS:
            assert digest(root / "inputs" / name) == facts[name], f"inputs changed: {name}"
        missing = [name for name in OUTPUTS if not (root / name).is_file()]
        if missing:
            sys.exit("FCAD_30N_GUI_COMPARE_REFUSED: missing real GUI output: " + ", ".join(missing))
        if digest(root / "plate.fcad") == facts["plate.fcad"]:
            sys.exit("FCAD_30N_GUI_COMPARE_REFUSED: plate.fcad was never saved")
        command = ["cargo", "test", "--release", "--features", "planegcs", "-p", "ferritecad-app",
                   "--bin", "ferritecad-viewer",
                   "sessions::tests::checkpoints::"
                   "native_compare_real_checkpoint_gui_artifacts_with_negative_controls",
                   "--", "--exact", "--nocapture", "--test-threads=1"]
        sys.exit(subprocess.run(command, env=dict(os.environ, FCAD_30N_GUI_DIR=str(root))).returncode)
    cli = os.environ["FERRITECAD"]
    existing = root
    while not existing.exists():
        existing = existing.parent
    probe = subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"],
                           capture_output=True)
    if probe.returncode == 0:
        sys.exit("FCAD_30N_GUI_FIXTURE_REFUSED: fixtures must be outside a checkout")
    root.mkdir(parents=True, exist_ok=False)
    (root / "inputs").mkdir()
    (root / "recovery").mkdir(mode=0o700)
    p = subprocess.run([cli, "create", str(root / "plate.fcad"), "--sample", "--size", "80", "40",
                        "12", "--json"], capture_output=True, text=True)
    assert p.returncode == 0, (p.stdout, p.stderr)
    shutil.copyfile(OLD, root / "old.fcad")
    for name in INPUTS:
        shutil.copyfile(root / name, root / "inputs" / name)
    (root / "facts.json").write_text(json.dumps(
        {name: digest(root / name) for name in INPUTS}, indent=2))
    print(f"FCAD_30N_GUI_FIXTURE_OK {root}")
    print(f"FERRITECAD_RECOVERY_DIR={root / 'recovery'}")


if __name__ == "__main__":
    main()
