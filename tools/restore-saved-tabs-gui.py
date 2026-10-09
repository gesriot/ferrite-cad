#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""§30R inputs, the step between the two windows, and the comparison of real
window artifacts. Never creates a window output and never writes the list.

Inputs: a sample plate (`a.fcad`, 80x40x12) and a circle extrusion (`b.fcad`)
made by the shipped CLI, pristine copies in `inputs/`, an empty private
`recovery/` folder, and no `tabs/` folder: the first window makes it at its Quit.
The viewer is pointed at both with FERRITECAD_RECOVERY_DIR and
FERRITECAD_TABS_DIR; the person's own folders are never touched.

`--between` runs after the first viewer ended: it checks that window's outputs
and copies the list it kept to `first-window` (the list itself is not changed).
`--compare` refuses before any build or peer job when an output is missing or a
document was never saved, then runs the Rust comparator.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

INPUTS = ("a.fcad", "b.fcad")
OUTPUTS = ("a.fcad", "b.fcad", "b-unsaved.stl", "b-unsaved.fbx", "first-window",
           "tabs/last-window")
CIRCLE = {"schema_version": 1, "center_mm": [12.5, -7.25], "radius_mm": 10.5,
          "height_mm": 15.25}
MAGIC = "FERRITECAD-LAST-TABS 1"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command):
    p = subprocess.run(command, capture_output=True, text=True)
    assert p.returncode == 0, (command, p.stdout, p.stderr)
    return p.stdout


def refuse(stage, why):
    sys.exit(f"FCAD_30R_GUI_{stage}_REFUSED: {why}")


def listed(path):
    """The files and the shown index of a Unix descriptor (this recipe is macOS)."""
    lines = path.read_bytes().decode("ascii").split("\n")
    if lines[:2] != [MAGIC, "platform unix"] or lines[-1] != "":
        refuse("BETWEEN", f"{path} is not a list of this format")
    count = int(lines[2].removeprefix("count "))
    active = lines[3].removeprefix("active ")
    paths = [os.fsdecode(bytes.fromhex(line.removeprefix("path "))) for line in lines[4:-1]]
    if len(paths) != count:
        refuse("BETWEEN", f"{path} is cut short")
    return paths, None if active == "none" else int(active)


def between(root, facts):
    if (root / "first-window").exists():
        refuse("BETWEEN", "first-window exists already; the step runs once")
    if digest(root / "a.fcad") == facts["a.fcad"]:
        refuse("BETWEEN", "the first window did not save a.fcad")
    if digest(root / "b.fcad") != facts["b.fcad"]:
        refuse("BETWEEN", "the first window wrote b.fcad")
    kept = root / "tabs" / "last-window"
    if not kept.is_file() or kept.is_symlink():
        refuse("BETWEEN", "the first window kept no list (did it Quit?)")
    paths, active = listed(kept)
    names = [root / name for name in INPUTS]
    if len(paths) != 2 or active != 0 or not all(
            os.path.samefile(p, n) for p, n in zip(paths, names)):
        refuse("BETWEEN", f"the first window's list is not a.fcad (shown), b.fcad: {paths} {active}")
    shutil.copyfile(kept, root / "first-window")
    print(f"FCAD_30R_GUI_BETWEEN_OK {root / 'first-window'}")


def main():
    usage = "usage: restore-saved-tabs-gui.py [--between|--compare] DIRECTORY_OUTSIDE_CHECKOUT"
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and
                                       sys.argv[1] not in ("--between", "--compare")):
        sys.exit(usage)
    root = Path(sys.argv[-1]).resolve()
    if len(sys.argv) == 3:
        facts = json.loads((root / "facts.json").read_text())
        for name in INPUTS:
            if digest(root / "inputs" / name) != facts[name]:
                refuse("COMPARE", f"inputs changed: {name}")
        if sys.argv[1] == "--between":
            return between(root, facts)
        missing = [name for name in OUTPUTS if not (root / name).is_file()]
        if missing:
            refuse("COMPARE", "missing real GUI output: " + ", ".join(missing))
        for name in INPUTS:
            if digest(root / name) == facts[name]:
                refuse("COMPARE", f"{name} was never saved")
        command = ["cargo", "test", "--release", "--features", "planegcs", "-p", "ferritecad-app",
                   "--bin", "ferritecad-viewer",
                   "sessions::tests::tabs::restore::"
                   "native_compare_real_reopen_gui_artifacts_with_negative_controls",
                   "--", "--exact", "--nocapture", "--test-threads=1"]
        sys.exit(subprocess.run(command, env=dict(os.environ, FCAD_30R_GUI_DIR=str(root))).returncode)
    cli = os.environ["FERRITECAD"]
    existing = root
    while not existing.exists():
        existing = existing.parent
    probe = subprocess.run(["git", "-C", str(existing), "rev-parse", "--show-toplevel"],
                           capture_output=True)
    if probe.returncode == 0:
        refuse("FIXTURE", "fixtures must be outside a checkout")
    root.mkdir(parents=True, exist_ok=False)
    (root / "inputs").mkdir()
    (root / "recovery").mkdir(mode=0o700)
    run([cli, "create", str(root / "a.fcad"), "--sample", "--size", "80", "40", "12"])
    request = root / "inputs" / "circle.json"
    request.write_text(json.dumps(CIRCLE))
    run([cli, "create-circle-extrude", str(request), "-o", str(root / "b.fcad")])
    request.unlink()
    for name in INPUTS:
        shutil.copyfile(root / name, root / "inputs" / name)
    (root / "facts.json").write_text(json.dumps(
        {name: digest(root / "inputs" / name) for name in INPUTS}, indent=2))
    print(f"FCAD_30R_GUI_FIXTURE_OK {root}")
    print(f"FERRITECAD_RECOVERY_DIR={root / 'recovery'}")
    print(f"FERRITECAD_TABS_DIR={root / 'tabs'}")


if __name__ == "__main__":
    main()
