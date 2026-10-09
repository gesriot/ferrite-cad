#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§30Q inputs and comparison for real window artifacts. Never creates outputs.

Inputs: a sample plate (`a.fcad`, 80x40x12) and a circle extrusion (`b.fcad`)
made by the shipped CLI; a third plate (`child/plate.fcad`) and a crash copy of
it, 22 mm high, left in the root's own `recovery/` folder by the controlled
child of the recovery tests (`named-dirty`), killed by its own PID; pristine
copies in `inputs/`. The viewer is pointed at that folder with
FERRITECAD_RECOVERY_DIR: the person's own recovery folder is never touched.
Outputs are made by the real window only; `--compare` refuses before any build
or peer job when one is missing or `a.fcad` was never saved.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

OUTPUTS = ("a.fcad", "c.fcad", "n.fcad", "a-unsaved.stl", "a-unsaved.fbx")
INPUTS = ("a.fcad", "b.fcad", "plate.fcad")
CIRCLE = {"schema_version": 1, "center_mm": [12.5, -7.25], "radius_mm": 10.5,
          "height_mm": 15.25}
CHILD = "child_process_entry"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command):
    p = subprocess.run(command, capture_output=True, text=True)
    assert p.returncode == 0, (command, p.stdout, p.stderr)
    return p.stdout


def crash_copy(root):
    """The recovery tests' controlled child: opens child/plate.fcad, applies 22 mm,
    waits for its crash copy, says READY and hangs; it is then killed by PID."""
    built = run(["cargo", "test", "--features", "planegcs", "-p", "ferritecad-jobs", "--test",
                 "recovery", "--no-run", "--message-format=json"])
    binary = [json.loads(line).get("executable") for line in built.splitlines()
              if '"reason":"compiler-artifact"' in line]
    binary = [path for path in binary if path]
    assert len(binary) == 1, binary
    env = dict(os.environ, FCAD_RECOVERY_CHILD_MODE="named-dirty",
               FCAD_RECOVERY_CHILD_ROOT=str(root / "recovery"),
               FCAD_RECOVERY_CHILD_WORK=str(root / "child"))
    child = subprocess.Popen([binary[0], CHILD, "--exact", "--ignored", "--nocapture",
                              "--test-threads=1"], env=env, stdout=subprocess.PIPE, text=True)
    try:
        for line in child.stdout:
            if "FCAD-CHILD READY" in line:
                break
        else:
            sys.exit("FCAD_30Q_GUI_FIXTURE_REFUSED: the child wrote no crash copy")
    finally:
        child.kill()
        child.wait()


def main():
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[1] != "--compare"):
        sys.exit("usage: open-new-recover-gui.py [--compare] DIRECTORY_OUTSIDE_CHECKOUT")
    root = Path(sys.argv[-1]).resolve()
    if len(sys.argv) == 3:
        facts = json.loads((root / "facts.json").read_text())
        for name in INPUTS:
            assert digest(root / "inputs" / name) == facts[name], f"inputs changed: {name}"
        missing = [name for name in OUTPUTS if not (root / name).is_file()]
        if missing:
            sys.exit("FCAD_30Q_GUI_COMPARE_REFUSED: missing real GUI output: " + ", ".join(missing))
        if digest(root / "a.fcad") == facts["a.fcad"]:
            sys.exit("FCAD_30Q_GUI_COMPARE_REFUSED: a.fcad was never saved")
        command = ["cargo", "test", "--release", "--features", "planegcs", "-p", "ferritecad-app",
                   "--bin", "ferritecad-viewer",
                   "sessions::tests::tabs::drafts::open_new_recover::"
                   "native_compare_real_open_new_recover_gui_artifacts_with_negative_controls",
                   "--", "--exact", "--nocapture", "--test-threads=1"]
        sys.exit(subprocess.run(command, env=dict(os.environ, FCAD_30Q_GUI_DIR=str(root))).returncode)
    cli = os.environ["FERRITECAD"]
    existing = root
    while not existing.exists():
        existing = existing.parent
    probe = subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"],
                           capture_output=True)
    if probe.returncode == 0:
        sys.exit("FCAD_30Q_GUI_FIXTURE_REFUSED: fixtures must be outside a checkout")
    root.mkdir(parents=True, exist_ok=False)
    (root / "inputs").mkdir()
    (root / "child").mkdir()
    (root / "recovery").mkdir(mode=0o700)
    run([cli, "create", str(root / "a.fcad"), "--sample", "--size", "80", "40", "12"])
    request = root / "inputs" / "circle.json"
    request.write_text(json.dumps(CIRCLE))
    run([cli, "create-circle-extrude", str(request), "-o", str(root / "b.fcad")])
    request.unlink()
    run([cli, "create", str(root / "child" / "plate.fcad"), "--sample", "--size", "80", "40",
         "12"])
    for name in ("a.fcad", "b.fcad"):
        shutil.copyfile(root / name, root / "inputs" / name)
    shutil.copyfile(root / "child" / "plate.fcad", root / "inputs" / "plate.fcad")
    crash_copy(root)
    listing = json.loads(run([cli, "list-recovery", "--recovery-dir", str(root / "recovery"),
                              "--json"]))
    (root / "facts.json").write_text(json.dumps(
        dict({name: digest(root / "inputs" / name) for name in INPUTS}, recovery=listing),
        indent=2))
    print(f"FCAD_30Q_GUI_FIXTURE_OK {root}")
    print(f"FERRITECAD_RECOVERY_DIR={root / 'recovery'}")


if __name__ == "__main__":
    main()
