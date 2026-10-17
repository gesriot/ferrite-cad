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

RECIPES = [('docs/single-edge-fillet.md',
  'FCAD_28A_AGENT_RECIPE',
  {'no-solver': 'FCAD_28A_RECIPE_OK ', 'stub': 'FCAD_28A_RECIPE_NO_KERNEL '}),
 ('docs/edit-fillet-radius.md',
  'FCAD_28B_AGENT_RECIPE',
  {'no-solver': 'FCAD_28B_RECIPE_OK ', 'stub': 'FCAD_28B_RECIPE_NO_KERNEL '}),
 ('docs/edit-fillet-base-height.md',
  'FCAD_28C_AGENT_RECIPE',
  {'no-solver': 'FCAD_28C_RECIPE_OK ', 'stub': 'FCAD_28C_RECIPE_NO_KERNEL '}),
 ('docs/edit-fillet-base-sketch.md',
  'FCAD_28D_AGENT_RECIPE',
  {'no-solver': 'FCAD_28D_RECIPE_OK ', 'stub': 'FCAD_28D_RECIPE_NO_KERNEL '}),
 ('docs/fillet-base-constraints.md',
  'FCAD_28E_AGENT_RECIPE',
  {'no-solver': 'FCAD_28E_RECIPE_NO_SOLVER ',
   'solver': 'FCAD_28E_RECIPE_OK ',
   'stub': 'FCAD_28E_RECIPE_NO_KERNEL '}),
 ('docs/fillet-constrained-plate.md',
  'FCAD_28F_AGENT_RECIPE',
  {'no-solver': 'FCAD_28F_RECIPE_NO_SOLVER ',
   'solver': 'FCAD_28F_RECIPE_OK ',
   'stub': 'FCAD_28F_RECIPE_NO_KERNEL '}),
 ('docs/sequential-edge-fillets.md',
  'FCAD_28G_AGENT_RECIPE',
  {'no-solver': 'FCAD_28G_RECIPE_OK ',
   'solver': 'FCAD_28G_RECIPE_OK ',
   'stub': 'FCAD_28G_RECIPE_NO_KERNEL '}),
 ('docs/edit-sequential-fillet-radii.md',
  'FCAD_28H_AGENT_RECIPE',
  {'no-solver': 'FCAD_28H_RECIPE_OK ',
   'solver': 'FCAD_28H_RECIPE_OK ',
   'stub': 'FCAD_28H_RECIPE_NO_KERNEL '}),
 ('docs/edit-two-fillet-base-height.md',
  'FCAD_28I_AGENT_RECIPE',
  {'no-solver': 'FCAD_28I_RECIPE_OK ',
   'solver': 'FCAD_28I_RECIPE_OK ',
   'stub': 'FCAD_28I_RECIPE_NO_KERNEL '}),
 ('docs/edit-two-fillet-base-sketch.md',
  'FCAD_28J_AGENT_RECIPE',
  {'no-solver': 'FCAD_28J_RECIPE_OK ',
   'solver': 'FCAD_28J_RECIPE_OK ',
   'stub': 'FCAD_28J_RECIPE_NO_KERNEL '}),
 ('docs/edit-two-fillet-base-constraints.md',
  'FCAD_28K_AGENT_RECIPE',
  {'no-solver': 'FCAD_28K_RECIPE_NO_SOLVER ',
   'solver': 'FCAD_28K_RECIPE_OK ',
   'stub': 'FCAD_28K_RECIPE_NO_KERNEL '}),
 ('docs/rectangular-fillet-history.md',
  'FCAD_28L_AGENT_RECIPE',
  {'no-solver': 'FCAD_28L_RECIPE_NO_SOLVER ',
   'solver': 'FCAD_28L_RECIPE_OK ',
   'stub': 'FCAD_28L_RECIPE_NO_KERNEL '}),
 ('docs/rectangular-corner-chamfer.md',
  'FCAD_29A_AGENT_RECIPE',
  {'no-solver': 'FCAD_29A_RECIPE_OK ',
   'solver': 'FCAD_29A_RECIPE_OK ',
   'stub': 'FCAD_29A_RECIPE_NO_KERNEL '}),
 ('docs/edit-chamfer-base-height.md',
  'FCAD_29B_AGENT_RECIPE',
  {'no-solver': 'FCAD_29B_RECIPE_OK ',
   'solver': 'FCAD_29B_RECIPE_OK ',
   'stub': 'FCAD_29B_RECIPE_NO_KERNEL '}),
 ('docs/edit-chamfer-base-sketch.md',
  'FCAD_29C_AGENT_RECIPE',
  {'no-solver': 'FCAD_29C_RECIPE_OK ',
   'solver': 'FCAD_29C_RECIPE_OK ',
   'stub': 'FCAD_29C_RECIPE_NO_KERNEL '}),
 ('docs/edit-chamfer-base-constraints.md',
  'FCAD_29D_AGENT_RECIPE',
  {'no-solver': 'FCAD_29D_RECIPE_NO_SOLVER ',
   'solver': 'FCAD_29D_RECIPE_OK ',
   'stub': 'FCAD_29D_RECIPE_NO_KERNEL '}),
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
