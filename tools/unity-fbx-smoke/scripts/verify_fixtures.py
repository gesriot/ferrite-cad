#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", type=Path, required=True)
    args = parser.parse_args()
    project = args.project.resolve()
    committed = project / "Assets" / "Fixtures"
    provenance = json.loads((committed / "PROVENANCE.json").read_text(encoding="utf-8"))
    with tempfile.TemporaryDirectory(prefix="ferritecad-fbx-fixtures-") as directory:
        generated = Path(directory)
        subprocess.run(
            [str(project / "scripts" / "generate_fixtures.py"), "--output", str(generated)],
            check=True,
        )
        manifest = json.loads((generated / "fixture-manifest.json").read_text(encoding="utf-8"))
        checked = 0
        for item in manifest["fixtures"]:
            name = item["file"]
            if (generated / name).read_bytes() != (committed / name).read_bytes():
                raise SystemExit(f"fixture is not generator-reproducible: {name}")
            checked += 1
        if (generated / "fixture-manifest.json").read_bytes() != (committed / "fixture-manifest.json").read_bytes():
            raise SystemExit("fixture manifest is not generator-reproducible")
        checked += 1
    generator = project / provenance["generator"]["file"]
    if hashlib.sha256(generator.read_bytes()).hexdigest() != provenance["generator"]["sha256"]:
        raise SystemExit("generator source digest differs from provenance")
    checked += 1
    manifest = committed / "fixture-manifest.json"
    if hashlib.sha256(manifest.read_bytes()).hexdigest() != provenance["generated_fixture_manifest"]["sha256"]:
        raise SystemExit("fixture manifest digest differs from provenance")
    checked += 1
    binary = committed / "unity_builtin_disc_binary7400.fbx"
    if hashlib.sha256(binary.read_bytes()).hexdigest() != provenance["trusted_binary_probe"]["sha256"]:
        raise SystemExit("trusted binary digest differs from provenance")
    checked += 1
    repository = Path(subprocess.run(
        ["git", "-C", str(project), "rev-parse", "--show-toplevel"],
        check=True, capture_output=True, text=True,
    ).stdout.strip())
    committed_generator = subprocess.run(
        ["git", "-C", str(repository), "show", provenance["generator"]["commit"] + ":tools/unity-fbx-smoke/scripts/generate_fixtures.py"],
        check=True, capture_output=True,
    ).stdout
    if committed_generator != generator.read_bytes():
        raise SystemExit("generator commit does not contain the recorded generator bytes")
    checked += 1
    if checked == 0:
        raise SystemExit("zero fixture checks")
    print(f"FCAD_FBX_FIXTURES_REPRODUCED checks={checked}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
