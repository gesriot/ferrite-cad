#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run the documented CLI recipes against the built executable."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

RECIPES = [('docs/generic-sweeps.md',
  'UP_TO_END_RECIPE',
  {'no-solver': 'UP_TO_END_OK ', 'solver': 'UP_TO_END_OK ',
   'stub': 'UP_TO_END_NO_KERNEL '}),
 ('docs/datum-planes.md',
  'DATUM_PLANES_RECIPE',
  {'no-solver': 'DATUM_PLANES_OK ', 'solver': 'DATUM_PLANES_OK ',
   'stub': 'DATUM_PLANES_NO_KERNEL '}),
 ('docs/fillets-and-chamfers.md',
  'NAMED_EDGES_RECIPE',
  {'no-solver': 'NAMED_EDGES_OK ', 'solver': 'NAMED_EDGES_OK ',
   'stub': 'NAMED_EDGES_NO_KERNEL '}),
 ('docs/directed-sketch-angle.md',
  'FCAD_31F_AGENT_RECIPE',
  {'no-solver': 'FCAD_31F_RECIPE_NO_SOLVER ', 'solver': 'FCAD_31F_RECIPE_OK '}),
 ('docs/collinear-sketch-lines.md',
  'FCAD_31G_AGENT_RECIPE',
  {'no-solver': 'FCAD_31G_RECIPE_NO_SOLVER ', 'solver': 'FCAD_31G_RECIPE_OK '}),
 ('docs/revolve-profile-constraints.md',
  'FCAD_27G_AGENT_RECIPE',
  {'solver': 'FCAD_27G_RECIPE_OK '}),
 ('docs/axis-closed-revolve-constraints.md',
  'FCAD_27H_AGENT_RECIPE',
  {'solver': 'FCAD_27H_RECIPE_OK '})]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("stub", "no-solver", "solver"))
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    binary = args.binary.with_suffix(".exe") if os.name == "nt" else args.binary
    environment = dict(os.environ, FERRITECAD=str(binary.resolve()),
                       FCAD_EXPECT_SOLVER=str(int(args.mode == "solver")))
    with tempfile.TemporaryDirectory(prefix="ferritecad-recipes-") as directory:
        for index, (document, marker, expectations) in enumerate(RECIPES):
            if args.mode not in expectations:
                continue
            text = Path(document).read_text(encoding="utf-8")
            code = text.split("# " + marker + "\n", 1)[1].split("\n```", 1)[0]
            source = Path(directory) / f"recipe-{index}.py"
            source.write_text(code, encoding="utf-8")
            result = subprocess.run([sys.executable, str(source)], env=environment,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            print(result.stdout, end="", flush=True)
            if result.returncode or not re.search("^" + expectations[args.mode], result.stdout, re.M):
                raise SystemExit(f"{document}: {args.mode} recipe failed ({result.returncode})")


if __name__ == "__main__":
    main()
