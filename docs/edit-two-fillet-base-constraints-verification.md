# §28K — verification record

[The contract and recipe](edit-two-fillet-base-constraints.md).

## Base and scope

Base `main` = `85953049921ee73bfe62139737477d08fad7e979` (PR #74, §28J),
tree `bf673ec73b4ccc95fed0d283e2c50ec07d910ffb`, the reviewed and corrected
tree. Its post-merge runs (CI, planegcs pin, runtime layout, notices) were
still running when this branch started; the SBOM runs had succeeded. They are
runs of the same tree and are told apart from the completed runs of the
PR. The branch is `edit-two-fillet-base-constraints`; §28J was not
reimplemented.

## Where and how this was run

Local, in the cloud container: Linux, Open CASCADE 8.0.1 and PlaneGCS built
from local inputs — **the local PlaneGCS is not pinned**; the pinned
`planegcs pin` workflow and the three-platform runtime layout are
authoritative. macOS is Apple Silicon (arm64) only; macOS Intel is not
supported. No window, GPU or Unity ran here. One heavy process at a time,
`-j2`–`-j3`; OCCT, Boost and PlaneGCS were not rebuilt.

## What changed

* `sketch_edit::constraint_frame` reads the base Sketch through
  `fillet_radius::fillets_over_plate` — the reader §28H–§28J use for two
  Fillets; one Fillet is §28E's frame exactly — and returns both Fillets in
  history order. `ConstraintSketchChoice` gains `second_fillet`;
  `PreparedSketchConstraints` carries it (`second_fillet()`), and the writer's
  `rederive` reads the frame again and compares both Fillets whole. The
  add-Fillet editor keeps reading `fillet_over_plate`.
* **No new solved-plate check.** The evaluator's `evaluable_fillet` already
  judges, on the built (solved) Lines of the same four UUIDs and at every
  rebuild, the sides, each saved corner, each radius against the solved
  shorter side and the pair in history order (`check_pair`, bounded bisection
  for the first radius's bound untouched). The copy's strict cold rebuild
  before publication therefore refuses a plate the solved Lines make too
  small; the new tests prove this instead of adding a second check. The
  reader asks of a constrained Sketch only that each radius is a finite number
  of at least the minimum, never the stored bound.
* JSON: `sketches[].constraint_edit.fillet_base.second_fillet`, additive. No
  capability, schema, payload version, command or copier. UI: the existing
  Edit constraints form names both Fillets in history order.
* Every text that listed what a two-Fillet history can edit now includes the
  constraints: the refusals of `saved_fillet`, `refuse_filleted` and the
  coordinate editor, the README, the capability and JSON notes and the
  contracts of §28G–§28J.

## New tests

* Domain (`fillet_radius::tests`, 3): constraints added, replaced and removed
  under adjacent and opposite Fillets in both windings change only the Sketch
  row (schema version, payload, hash), the one capability row and the stamp;
  both Fillet rows, the Extrude, names and dependencies stay; the closure links
  keep their UUIDs; after removal the coordinate editor (§28J) is offered; the
  writer refuses a moved curve, an extra constraint and a preparation made
  before Fillet 1's or Fillet 2's radius changed; the evaluator judges each
  radius on its own joint, the flat at the least float that leaves it and one
  below, opposite corners owing none.
* CLI (`sequential::constraints`, 6): discovery and the stub order of checks;
  a full dimensioning of a free plate rounded twice — adjacent and opposite
  corners, both windings, different radii, larger than stored and shrunk to
  the exact bound of a radius — each copy measured cold, through a real Miss
  and Hit, by an independent STL parser and as FBX, both cylinders by UUID
  under their own names on the solved axes; dimension and pin replaced with a
  warm cache missing the plate and both Fillets, then a radius and the height,
  then every user constraint removed (closure only, the stored plate again)
  and the coordinates edited again (§28J); each Fillet's own refusal naming
  its joint UUIDs and the flat naming the shared Line's, at exact bounds, on
  plates whose stored rectangle would still allow them; a dimensioned plate
  whose stored rectangle is too small for its radii widened and refused when
  narrowed; the real solver's conflict naming stored constraints and a
  redundancy naming the equality; atomic refusals, cancellation and a lost
  report.
* App (2): the form names both Fillets, keeps the draft through a cancelled
  Save and worker refusals naming a radius and the flat, with Undo/Redo over
  the whole request; the worker and the CLI publish one document with every
  SQL cell equal once only the new constraint UUIDs are mapped, and
  byte-identical STL/FBX.
* Updated expectations: the tests of §28G–§28J that asserted the constraint
  editor refused a two-Fillet history now assert it reads it.

## Local results

Solver build (Open CASCADE + local unpinned PlaneGCS), debug:

* `tests/fillet.rs`: **66 passed** — 64 executed plus 2 that are N/A here
  because they run only in the build without PlaneGCS (`skipped: the mixed
  gate needs a build without PlaneGCS`). The six `sequential::constraints`
  tests executed; none printed `skipped:`.
* Recipes §28A–§28K against the solver CLI extracted from Markdown: all eleven
  print `FCAD_28x_RECIPE_OK`. §28K: `FCAD_28K_RECIPE_OK
  dimensioned=2157.472546/2157.649522 replaced=1555.035046/1555.212022
  narrow=897.964734/898.141709 closure=3078.847572/3079.024522
  moved=1949.910066/1950.087022` (independently read mesh / exact volume,
  mm³, after the dimensioning, after the dimension and pin were replaced, at
  the depth of exactly 2 × 3.0625 mm, after every user constraint was removed,
  and after the coordinates were edited again). The §28J recipe's one
  assertion that the constraint editor refuses became "available" and passes.
* FBX: `tools/check-fbx-complex.sh --features planegcs` as CI runs it, over
  the artifacts of the whole `tests/fillet.rs` run: every Fillet marker
  including the new `FCAD_FILLET_TWO_CONSTRAINTS_UFBX_EXECUTED`; the four
  files `constraints-cw-adjacent`, `constraints-ccw-opposite`, `k-replaced`,
  `k-closure-moved` read by pinned ufbx 0.23.0 (`checks=6 failures=0` each)
  and joined with their STL (120, 136, 120, 120 triangles, worst 6.94e-18
  m); 64 joins in all.
* fmt, workspace clippy (`--all-targets --all-features -D warnings`) and
  `git diff --check`: clean.

Regression of the affected crates, solver build, debug (`--no-fail-fast`):
`ferritecad-document`, `-jobs`, `-eval` (lib and integration tests), every
`ferritecad-cli` test target except the heavy ones named below, the CLI
binaries, and `ferritecad-app`: **1123 passed, 2 failed, 1 ignored** (the 1112
of §28J plus the 11 new tests). The two failures are the root-only permission
tests (`dump_graph`'s `read_only_permissions_still_dump_when_the_file_can_be_read`
and `validate`'s `validation_really_read_only_permissions`): a directory made
read-only does not stop uid 0. Copied to a directory and run as `nobody`, both
pass (8/8 and 4/4). The ignored test is the timing benchmark. The heavy targets
`complex_step_pixels export_scene_complex occurrence_identity_complex
imported_step_pixels fillet_shell_corpus export_fbx_identity shared_step_import
import_step export_fbx_complex` were not run: nothing in this slice touches
STEP import, scene export or pixels.

OCCT without a solver (release, `--no-default-features`,
`FERRITECAD_REQUIRE_PLANEGCS=0`): `tests/fillet.rs` 66 passed, 19 of them N/A
(`skipped: constrained geometry requires PlaneGCS`), so 47 executed,
including `sequential::constraints::constraint_discovery_and_protocol_without_native`;
the recipe prints `FCAD_28K_RECIPE_NO_SOLVER` at its first constraint step.

Stub (no Open CASCADE, `CMAKE_TOOLCHAIN_FILE` hiding the native prefix):
`tests/fillet.rs` 66 passed, 54 N/A (`skipped: this build has no Open
CASCADE`), 12 executed; the extracted `ci.yml` step passes — the CLI
discovery gate, the app widget gate, the three document gates and
`FCAD_28K_RECIPE_NO_KERNEL`.

The worker-versus-CLI equality
(`constraints::tests::native_two_fillet_base_constraint_worker_and_cli_publish_the_same_part`)
compares every SQL row with only the eleven newly minted constraint UUIDs
matched off, and requires byte-identical STL and FBX and the analytic
volume of both rounded corners; it and the widget test executed.

### Mutations — local, executed, restored

Each compiled, ran the `fillet_radius::tests` (and `fillet::tests`), failed
where named and was restored byte for byte:

1. The evaluator's pair check dropped (`check_pair` in `evaluable_fillet`)
   → `the_evaluator_judges_both_fillets_and_the_pair_on_the_built_lines`,
   `the_pair_bound_is_exact_for_either_radius_and_absent_opposite` and
   `a_second_fillet_is_written_on_the_first_and_a_third_is_refused` fail.
2. Fillet 2's radius not judged on the solved corner (its check made with the
   minimum radius) → `the_evaluator_judges_both_fillets_and_the_pair_on_the_built_lines`
   fails on the opposite-corner case where only Fillet 2's own bound applies.
3. The writer's re-derivation of the Fillets removed →
   `the_constraint_writer_rederives_both_fillets_and_refuses_forgery` and
   `the_constraint_writer_rederives_the_edit_under_a_fillet` fail (a stale
   Fillet 2 radius and a stale Fillet 1 radius are written).

A fourth attempt — dropping only the comparison of `second_fillet` — survived,
and is an equivalent mutant, not counted: Fillet 1's `SavedFillet` carries
the other Fillet (`neighbour`: feature, edge, corner, radius), so every change
to Fillet 2 already changes the first comparison.

### Compatibility with the reader on `main`

`main` at `8595304` built with the same solver: it reads, validates and
cold-rebuilds a §28K copy (18 of 18 references) with byte-identical STL and
FBX, reports the constraint editor as unavailable, refuses a constraint edit
(`unsupported`, "…after 2 Fillets in all (§28G)…") and writes nothing; a
document written by that build is constrained by this one.
`FCAD_28K_COMPAT_OK`.

## CI

Head `556d02486054d33f593566d38d3c67db2ab6a965` (code head; this record is the only later commit). Base `main` 85953049921ee73bfe62139737477d08fad7e979 was green after its own merge (post-merge runs of #74 are separate from these): CI 36766477200, runtime layout 36766477275, planegcs pin 36766477309.

All runs on the head finished green:

- CI, run 36771197569 (`lint`, `test` on ubuntu, macos and windows, `sbom`, `notices`, `supply-chain`): https://github.com/gesriot/ferrite-cad/actions/runs/36771197569
- planegcs pin, run 36771168425 (linux, macos, windows, comparison): https://github.com/gesriot/ferrite-cad/actions/runs/36771168425
- combined runtime layout, run 36771168237 (linux, macos, windows, comparison): https://github.com/gesriot/ferrite-cad/actions/runs/36771168237

Counted in the real job logs (not inferred from a green job):

| job | `sequential::constraints::*` ok | document gates ok | app gates ok | `FCAD_28K_RECIPE_OK` | `FCAD_FILLET_TWO_CONSTRAINTS_UFBX_EXECUTED` |
|---|---|---|---|---|---|
| runtime linux | 6 | 3 | 2 | 1 | 1 |
| runtime macos | 6 | 3 | 2 | 1 | 1 |
| runtime windows | 6 | 3 | 2 | 1 | 1 |
| CI ubuntu (stub, explicit no-skip step) | 1 discovery | 3 | 1 widget gate | `FCAD_28K_RECIPE_NO_KERNEL` 1 | n/a |

The full Ubuntu suite also prints six harness `ok` lines, but its five
geometry tests return early without OCCT; those are not native executions.
The explicit stub step executes discovery, three domain gates and the
widget gate, then the no-kernel recipe. This was checked against the complete
log of final original CI run 36780956978 during independent review.

The runtime 6 are `constraint_discovery_and_protocol_without_native`, `native_dimensioning_a_plate_rounded_twice_solves_it_and_keeps_both_fillets`, `native_replacing_removing_and_editing_on_after_constraints`, `native_the_solved_plate_decides_each_radius_and_the_shared_flat`, `native_a_solved_plate_may_outgrow_its_stored_bound_and_not_shrink_below_it` and `native_solver_diagnoses_and_atomic_refusals_under_two_fillets`. The 3 document gates are `constraints_under_two_fillets_change_only_the_sketch_and_keep_both_fillets`, `the_constraint_writer_rederives_both_fillets_and_refuses_forgery` and `the_evaluator_judges_both_fillets_and_the_pair_on_the_built_lines`. The 2 app gates are `two_fillet_base_constraint_widgets_name_both_fillets_and_keep_the_draft` and `native_two_fillet_base_constraint_worker_and_cli_publish_the_same_part`. Every `FCAD_28K_RECIPE_OK` line carries the same values on all three platforms.

Truncation, stated plainly: the log tool used here returns at most the last 5000 lines of a job, so the start of each job (toolchain, cache, OCCT/PlaneGCS build, and the step that prints `FCAD_28K_RECIPE_NO_SOLVER`) is before the window and was not read line by line; the counts above come from the returned tail. That step is a required step of a job that finished green. The full logs exist (`gh run view --log` or the API) and were not downloaded here.

## Independent PR #75 review on macOS arm64 (2026-09-30)

Reviewed the frame, writer re-derivation, solved-radius/pair checks, JSON and
widget route. No execution logic needed changing. Corrected the stale
one-Fillet reader comment and the contract's claim that redundancy always
refuses: a consistent redundant request can publish with its real redundant
UUIDs in the solve report.

The GUI comparator had a real coverage hole: it excluded payload/hash/schema
for every object, not just the edited Sketch. It now compares all non-Sketch
cells and every schema version exactly, and compares the selected Sketch's
raw CBOR after matching only constraint UUIDs by their complete rules. A
control changed the peer Extrude from 6.75 to 7 mm through the real CLI:
the previous comparator accepted it; the corrected comparator refused
`objects.payload`. The unchanged GUI/CLI comparison still passed, 570 cells.

Local native release, pinned OCCT 8.0.1 and PlaneGCS, existing target reused:

- Fresh CLI/viewer build; document/jobs/eval: 481 harness passes, including
  two no-solver-only N/A cases; one old timing benchmark ignored.
- CLI fillet suite: 66 harness passes, including two mixed-only N/A cases.
- App constraints/sketch/edits: 27/46/13 executed passes, no skips.
- Total: **629 executed**, four explicit N/A, one ignored. No failures.
- Fmt, workspace clippy all targets/features `-D warnings`, diff whitespace:
  pass. No large STEP/pixel campaign duplicated locally.
- Fresh relocatable bundle from the reviewed release binaries, strict deep
  ad-hoc signature check and solver provenance probe passed without DYLD
  variables. The public §28K recipe printed the five expected volume pairs.

**Real window test passed**, PID 21914 under the 1536 MiB watchdog. The
fixture came from the bundle's CLI; every `gui-*` file came from native
window actions. Observed the two-Fillet context and disabled third-Fillet
action; H/V, pin and length additions; the three solved-depth refusals
(7.9 mm names Fillet 1's joint, 8 mm names Fillet 2's joint, 8.012 mm names
the shared Line and 0.007 mm flat); draft preservation; Undo/Redo; Save
Cancel; publication/async Open; replacement of both lengths and the pin;
STL/FBX exports; removal of all seven user constraints with four closure
UUIDs retained; and the available coordinate editor on the resulting copy.

The stricter comparator passed on all three window copies: SQL allowlists,
source SHA-256, stored coordinates, constraints/UUIDs, both Fillets,
validation and cold rebuild, plus byte-identical GUI/CLI STL and FBX.
Pinned ufbx read the actual window FBX with six checks and no failures;
the independent oriented-triangle join matched all **140 triangles**, worst
error **3.47e-18 m**. The viewer exited normally, code 0, after 318 seconds:
peak footprint **210.720 MiB**, pressure normal throughout, swap unchanged
at 665.3125 MiB. No CUA call addressed it after Quit. This does not explain
or close the earlier OOM report.

Downloaded the complete original runtime logs (36771168237), including the
previously unread start: each OS executed the 11 new distinct gates plus the
mixed discovery repeat (12 executions), the no-solver recipe, the native
recipe with matching values and the new ufbx marker. The review-comment
commit 05609d1 started runtime 36813814395. Ordinary CI on an earlier head
was superseded by the later documentation push; the final head's ordinary
CI and that runtime must pass before merge. This record is documentation only.

Small local evidence (logs, models, screenshots, memory record) is retained
outside the checkout in `ferrite-pr75-review` under the user's Codex
visualization artifacts. No foreign worktree, installation or process was
changed.

## Limits

* The local PlaneGCS is unpinned; authoritative CI is Linux, macOS arm64 and
  Windows. macOS Intel is not supported.
* No window, GPU or Unity ran in the cloud. The headless widget tests are not
  a GUI run. The previous OOM is not considered fixed.
* A third Fillet, arbitrary edges, Cut with Fillet, Chamfer, new constraint
  kinds, automatic radius fitting, retargeting a corner, saving solved
  coordinates, in-place Save and live preview are out of scope.

## macOS fixture and window scenario (for the user's Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised here with a
second CLI invocation standing in for the window, which says nothing about
the window: `FCAD_28K_GUI_COMPARE_OK cells=545`; without the window's files
the comparator stopped with `FCAD_28K_GUI_COMPARE_MISSING …` and created
nothing; a renamed Fillet 2 in `gui-narrow.fcad`, a stray `never.fcad`, one
flipped byte of the STL and a pin typed 0.01 mm off were caught. The refusals
of the scenario were probed through the CLI on the generated fixture: a solved
depth of 7.9 mm names Fillet 1, 8.0 mm Fillet 2, 8.012 mm the flat and the
shared Line's UUID; 12.5 mm publishes.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon (arm64) Mac, and do not set `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `twice.fcad` — a **free** plate (no constraint), 37.5 × 12.25 ×
6.75 mm, drawn clockwise from the upper right with offset, fractional
coordinates, rounded by `fillet-edge-copy` twice on adjacent corners: Fillet
1 at the stored corner (33, 3.25) — the start of Segment 2 — with r 4 mm and
Fillet 2 at (33, 15.5) — the start of Segment 1 — with r 4.005 mm — and
`facts.json`. The radii differ by less than the 0.01 mm flat, so one fixture
shows all three refusals on the **solved** depth of Segment 1 (the comments in
the script give the depths).

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-two-fillet-base-constraints-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28K_GUI_FIXTURE\n", "ferrite-28k-fixture.py"),
                   ("# FCAD_28K_GUI_COMPARE\n", "ferrite-28k-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_28K_DIR="$PWD/fillet-two-constraints-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28k-fixture.py "$FCAD_28K_DIR"
```

```python
# FCAD_28K_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-two-constraints-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# A free plate, clockwise from the upper right, offset and fractional, no
# constraint at all. Segment 1 runs down x = 33, Segment 2 along y = 3.25 toward
# -X, Segment 3 up x = -4.5, Segment 4 along y = 15.5. The stored corners
# (33, 3.25) and (33, 15.5) are the starts of Segments 2 and 1 and share the
# 12.25 mm Line of Segment 1.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
FIRST, SECOND, H = [33.0, 3.25], [33.0, 15.5], 6.75
# Fillet 1 r 4 mm, then Fillet 2 r 4.005 mm beside it. Their radii differ by
# less than the 0.01 mm flat the pair rule keeps, so on the SOLVED depth D of
# Segment 1 (the Line they share)
#   D < 8.0          : Fillet 1 refuses (2 r1 = 8),
#   8.0 <= D < 8.01  : Fillet 2 refuses (2 r2 = 8.01), Fillet 1 fits,
#   8.01 <= D < 8.015: each radius fits alone, the flat between them does not,
#   D >= 8.015       : both fit (4.005 <= D - 4 - 0.01).
R1, R2 = 4.0, 4.005
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H, "points_mm": PLATE}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
def fillet(source, corner, radius, dest):
    catalog = run("inspect", source, "--json")["result"]
    chosen = next(c for c in catalog["bodies"][0]["fillet_edge"]["target"]["candidates"]
                  if c["stored_corner_mm"] == corner)
    request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": radius}))
    run("fillet-edge-copy", source, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
        catalog["content_version"], "--request", request, "-o", dest, "--json")
once = out / "once.fcad"
fillet(plate, FIRST, R1, once)
source = out / "twice.fcad"
fillet(once, SECOND, R2, source)
for p in (request, plate, once):
    p.unlink()
catalog = run("inspect", source, "--json")["result"]
(sketch,) = catalog["sketches"]
edit = sketch["constraint_edit"]
ctx = edit["fillet_base"]
two = ctx["second_fillet"]
assert edit["available"] is True and ctx["stored_corner_mm"] == FIRST, edit
assert two["previous_feature_id"] == ctx["fillet_feature_id"] and two["history_index"] == 2, ctx
assert two["stored_corner_mm"] == SECOND and edit["constraints"] == [], edit
(base,) = catalog["features"]
# The solved plates the scenario publishes: the start of Segment 2 (Fillet 1's
# corner) pinned at PIN, Segment 2 WIDTH long (X), Segment 1 DEPTH long (Y).
def plate_at(pin, width, depth):
    x0, y0 = pin[0] - width, pin[1]
    return [x0, y0, width, depth]
facts = {
    "sketch_id": sketch["sketch_id"], "base_feature_id": base["feature_id"],
    "curve_ids": [c["curve_id"] for c in edit["curves"]],
    "stored_starts_mm": [c["start_mm"] for c in edit["curves"]],
    "content_version": catalog["content_version"],
    "first_fillet_id": ctx["fillet_feature_id"], "second_fillet_id": two["fillet_feature_id"],
    "saved_corners_mm": [FIRST, SECOND], "radii_mm": [R1, R2], "height_mm": H,
    "dimensioned": {"pin_mm": [38.75, 1.0], "width_mm": 40.0, "depth_mm": 12.5},
    "narrow": {"pin_mm": [22.25, -3.5], "width_mm": 20.25, "depth_mm": 8.02},
    "refused_depths_mm": [7.9, 8.0, 8.012],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
facts["dimensioned"]["rect_mm"] = plate_at(**{"pin": facts["dimensioned"]["pin_mm"], "width": 40.0, "depth": 12.5})
facts["narrow"]["rect_mm"] = plate_at(**{"pin": facts["narrow"]["pin_mm"], "width": 20.25, "depth": 8.02})
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_28K_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_28K_DIR/../watch-28k.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

If system memory pressure aborts or stops the run, it does not count; start
again from a fresh fixture directory. **Do not address the viewer after Quit
through the automation**: it may relaunch the app outside the watchdog. If it
was relaunched, quit that instance too and read only the original PID's exit
and watchdog record.

The plate runs clockwise from the upper right: Segment 1 down x = 33 (along
Y), Segment 2 along y = 3.25 toward -X, Segment 3 up x = -4.5, Segment 4 along
y = 15.5. Type numbers exactly as written.

1. **Open** `$FCAD_28K_DIR/twice.fcad` (asynchronously). **Edit constraints…**
   is enabled for the base Sketch; **Edit Sketch** reads the same two Fillets;
   **Fillet edge of …** is refused ("third").
2. **The form.** Open the constraint form on the base Sketch: it reads
   "History: Extrude -> Fillet 1 <first_fillet_id> at the corner of Lines … |
   …, r 4 mm (stored corner (33, 3.25)) -> Fillet 2 <second_fillet_id> at the
   corner of Lines … | …, r 4.005 mm (stored corner (33, 15.5)). Both Fillets
   keep their corners and radii … each side at a corner at least 8 mm (Fillet
   1) or 8.01 mm (Fillet 2), and adjacent arcs still leave a flat between
   them."
3. **The request.** **Segment 1** → **Add Vertical**; **Segment 2** → **Add
   Horizontal**; **Segment 3** → **Add Vertical**; **Segment 4** → **Add
   Horizontal**; **Segment 2**, Fixed X `38.75`, Fixed Y `1` → **Add Fixed
   point**; **Segment 2**, length `40` → **Add length**; **Segment 1**, length
   `7.9` → **Add length**.
4. **Fillet 1 refused.** **Save constraints copy…** → `never.fcad`. The worker
   refuses: the solved plate has sides of 7.9 and 40 mm at the rounded corner,
   too short; the reason names Fillet 1's corner (its two Line UUIDs). The
   form keeps the draft; `never.fcad` does not exist.
5. **Fillet 2 refused.** **Undo** (the 7.9 mm length goes), **Segment 1**,
   length `8` → **Add length** → **Save** → `never.fcad`: the reason names
   Fillet 2's corner (4.005 mm). Draft kept, nothing written.
6. **The flat refused.** **Undo**, length `8.012` → **Add length** → **Save**
   → `never.fcad`: each radius fits alone, "…would leave 0.007… mm of it
   flat…", naming Segment 1's Line UUID. Draft kept, nothing written.
7. **Undo/Redo over the request.** **Undo**, then **Redo**: the 8.012 mm
   length returns; **Undo** again and add length `12.5`.
8. **Save Cancel.** **Save constraints copy…** → **Cancel** in the file
   dialog: nothing starts; the draft stays.
9. **Publish, async Open.** **Save** → `$FCAD_28K_DIR/gui-dimensioned.fcad`.
   The viewer opens the copy asynchronously; the form lists the four
   Coincident closure links, the H/V rules, the pin and both lengths.
10. **Replace and move.** In the opened copy: **Segment 2** → **Replace
    length** `20.25`; **Segment 1** → **Replace length** `8.02` (accepted: the
    flat is 0.015 mm); tick **Remove** on the Fixed row; **Segment 2**, Fixed X
    `22.25`, Fixed Y `-3.5` → **Add Fixed point**; **Save** →
    `$FCAD_28K_DIR/gui-narrow.fcad`; it opens asynchronously.
11. **Exports.** From the window export `gui-narrow.stl` and `gui-narrow.fbx`
    of `gui-narrow.fcad` into `$FCAD_28K_DIR`, at the default tessellation.
12. **Removal.** In the opened `gui-narrow.fcad` tick **Remove** on every user
    constraint (four H/V, the pin, two lengths); **Save** →
    `$FCAD_28K_DIR/gui-closure.fcad`; it opens asynchronously: the four
    closure links remain, the part is the stored 37.5 × 12.25 plate again, and
    **Edit Sketch** (§28J) is offered.
13. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28k-compare.py "$FCAD_28K_DIR"
```

It first requires `gui-dimensioned.fcad`, `gui-narrow.fcad`,
`gui-closure.fcad`, `gui-narrow.stl` and `gui-narrow.fbx` (and never creates
them), refuses a `never.fcad`, checks the source's SHA-256, the allowlist of
each window edit (the Sketch row's schema version, payload and hash, the one
capability row and the stamp only), the constraints each copy holds (kinds,
values, closure UUIDs kept through every step, untouched H/V UUIDs kept,
replaced rules re-minted), both Fillets and their stored corners, the stored
coordinates unchanged, validation and a cold rebuild, makes the CLI peers
(only if absent), compares every SQL cell except the stamp and the selected
Sketch's payload hash, comparing that payload byte-for-byte after matching
constraint UUIDs by their complete rules (and preserving any source UUIDs),
and requires byte-equal STL/FBX, then reads the window's STL
itself: the solved narrow plate's bounds and volume, exactly the two saved
corners rounded, each wall at its own radius about its own axis. It prints
`FCAD_28K_GUI_COMPARE_OK cells=N`.

```python
# FCAD_28K_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-dimensioned.fcad", "gui-narrow.fcad", "gui-closure.fcad", "gui-narrow.stl", "gui-narrow.fbx")
missing = [n for n in GUI if not (out / n).is_file()]
if missing:
    sys.exit(f"FCAD_28K_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first")
assert not (out / "never.fcad").exists(), "a refused Save published something"
source = out / "twice.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
SKETCH, F1, F2 = facts["sketch_id"], facts["first_fillet_id"], facts["second_fillet_id"]
LINES, STORED = facts["curve_ids"], facts["stored_starts_mm"]
r1, r2 = facts["radii_mm"]
H = facts["height_mm"]
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout
def tables(path):
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    got = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        got[t] = ([d[0] for d in cur.description], sorted(cur.fetchall(), key=repr))
    db.close()
    return got
SK = bytes.fromhex(SKETCH.replace("-", ""))
def allowlist(before, after):
    """Only the Sketch row's schema version, payload and hash, the stamp and the
    one sketch.constraints capability row moved."""
    a, b = tables(before), tables(after)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        if t == "capabilities":
            assert all(r in brows for r in arows), "a capability changed"
            assert all("sketch.constraints.v1" in repr(r) for r in brows if r not in arows)
            continue
        assert len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                assert u == v or (t == "objects" and c in ("schema_version", "payload", "payload_hash")
                                  and x[ac.index("id")] == SK) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c}"
def constraints(path):
    edit = run("inspect", path, "--json")["result"]["sketches"][0]["constraint_edit"]
    assert [c["start_mm"] for c in edit["curves"]] == STORED, "the stored guess was replaced"
    return edit["constraints"], edit
def shape(c):
    r = dict(c["rule"])
    return json.dumps(r, sort_keys=True)
def describe(path, pin, width, depth, rect):
    listed, edit = constraints(path)
    kinds = sorted(c["rule"]["kind"] for c in listed)
    assert kinds.count("coincident") == 4, kinds
    if pin is None:
        assert kinds == ["coincident"] * 4, kinds
    else:
        assert kinds.count("horizontal") == 2 and kinds.count("vertical") == 2, kinds
        assert kinds.count("fixed") == 1 and kinds.count("distance") == 2, kinds
        fixed = next(c["rule"] for c in listed if c["rule"]["kind"] == "fixed")
        assert [fixed["x"], fixed["y"]] == pin, fixed
        lengths = sorted(c["rule"]["distance"] for c in listed if c["rule"]["kind"] == "distance")
        assert lengths == sorted([width, depth]), lengths
    fb = edit["fillet_base"]
    assert (fb["fillet_feature_id"], fb["radius_mm"], fb["stored_corner_mm"]) == (F1, r1, facts["saved_corners_mm"][0])
    sf = fb["second_fillet"]
    assert (sf["fillet_feature_id"], sf["previous_feature_id"], sf["history_index"], sf["radius_mm"],
            sf["stored_corner_mm"]) == (F2, F1, 2, r2, facts["saved_corners_mm"][1]), sf
    assert run("validate", path, "--json")["result"]["valid"] is True
    n = len(tables(path)["topology_refs"][1])
    assert f"{n} of {n} stored references resolved" in run("rebuild", path, "--cold")
    return listed
dim, nar = facts["dimensioned"], facts["narrow"]
g_dim, g_nar, g_clo = out / "gui-dimensioned.fcad", out / "gui-narrow.fcad", out / "gui-closure.fcad"
allowlist(source, g_dim)
allowlist(g_dim, g_nar)
allowlist(g_nar, g_clo)
l_dim = describe(g_dim, dim["pin_mm"], dim["width_mm"], dim["depth_mm"], dim["rect_mm"])
l_nar = describe(g_nar, nar["pin_mm"], nar["width_mm"], nar["depth_mm"], nar["rect_mm"])
l_clo = describe(g_clo, None, None, None, None)
ids = lambda l, pred: {c["constraint_id"] for c in l if pred(c)}
closure_ids = ids(l_dim, lambda c: c["rule"]["kind"] == "coincident")
assert closure_ids == ids(l_nar, lambda c: c["rule"]["kind"] == "coincident") == ids(l_clo, lambda c: True), \
    "the closure links lost their UUIDs"
hv = lambda l: ids(l, lambda c: c["rule"]["kind"] in ("horizontal", "vertical"))
assert hv(l_dim) == hv(l_nar), "the untouched H/V rules lost their UUIDs"
assert not ids(l_dim, lambda c: c["rule"]["kind"] in ("distance", "fixed")) & ids(l_nar, lambda c: True), \
    "a replaced rule kept its UUID"
# The same three edits through the shipped CLI, and its exports.
def peer(src, remove, add, dest):
    if not dest.exists():
        catalog = run("inspect", src, "--json")["result"]
        request = out / "peer-request.json"
        request.write_text(json.dumps({"request_version": 1, "remove": remove, "add": add}))
        run("edit-sketch-constraints-copy", src, "--sketch", SKETCH, "--expect-version",
            catalog["content_version"], "--request", request, "-o", dest, "--json")
        request.unlink()
def hv_rule(i):
    return {"curve_id": LINES[i], "rule": "vertical" if i in (0, 2) else "horizontal"}
def length(i, mm):
    return {"curve_id": LINES[i], "rule": "distance", "distance_mm": mm}
def pin(p):
    return {"curve_id": LINES[1], "rule": "fixed", "at": "start", "x_mm": p[0], "y_mm": p[1]}
# Segment 2 (index 1) is horizontal: its length is the width, its start the pin;
# Segment 1 (index 0) is vertical: its length is the depth.
peer_dim, peer_nar, peer_clo = out / "peer-dimensioned.fcad", out / "peer-narrow.fcad", out / "peer-closure.fcad"
peer(source, [], [hv_rule(i) for i in range(4)] + [pin(dim["pin_mm"]), length(1, dim["width_mm"]),
     length(0, dim["depth_mm"])], peer_dim)
listed = constraints(peer_dim)[0]
replaced = [c["constraint_id"] for c in listed if c["rule"]["kind"] in ("distance", "fixed")]
peer(peer_dim, replaced, [length(1, nar["width_mm"]), length(0, nar["depth_mm"]), pin(nar["pin_mm"])], peer_nar)
user = [c["constraint_id"] for c in constraints(peer_nar)[0] if c["rule"]["kind"] != "coincident"]
peer(peer_nar, user, [], peer_clo)
for fmt in ("stl", "fbx"):
    if not (out / f"peer-narrow.{fmt}").exists():
        run(f"export-{fmt}", peer_nar, "-o", out / f"peer-narrow.{fmt}", "--json")
def same(gui, other):
    """All cells except the stamp and the selected Sketch's payload hash;
    compare that payload byte-for-byte after matching only constraint UUIDs
    by their complete rules. Every other object's payload/hash stays exact."""
    a, b = constraints(gui)[0], constraints(other)[0]
    assert len(a) == len(b)
    old = {c["constraint_id"] for c in constraints(source)[0]}
    ordered_a, ordered_b = [sorted(items, key=shape) for items in (a, b)]
    assert [shape(c) for c in ordered_a] == [shape(c) for c in ordered_b]
    assert len({c["constraint_id"] for c in a}) == len(a)
    assert len({c["constraint_id"] for c in b}) == len(b)
    def normalized(payload, listed):
        # The document ID serializer uses CBOR byte strings of length 16.
        # Refuse an unexpected encoding or extra occurrence, not a broad
        # JSON normalization that could discard a stored field.
        for i, c in enumerate(listed):
            u = c["constraint_id"]
            encoded = b"\x50" + bytes.fromhex(u.replace("-", ""))
            assert payload.count(encoded) == 1, ("constraint UUID encoding", u)
            if u not in old:
                payload = payload.replace(encoded, b"\x50" + i.to_bytes(16, "big"))
        return payload
    left, right = tables(gui), tables(other)
    assert left.keys() == right.keys()
    cells = 0
    for t in left:
        (cols, lrows), (other_cols, rrows) = left[t], right[t]
        assert cols == other_cols and len(lrows) == len(rrows), t
        if t == "objects":
            k = cols.index("id")
            lrows, rrows = [sorted(rows, key=lambda r: r[k]) for rows in (lrows, rrows)]
        for lrow, rrow in zip(lrows, rrows):
            for col, lval, rval in zip(cols, lrow, rrow):
                if t == "meta" and col == "modified_at":
                    continue
                selected = t == "objects" and lrow[cols.index("id")] == SK
                if selected and col == "payload_hash":
                    continue
                if selected and col == "payload":
                    lval, rval = normalized(lval, ordered_a), normalized(rval, ordered_b)
                assert lval == rval, (t, col)
                cells += 1
    return cells

cells = same(g_dim, peer_dim) + same(g_nar, peer_nar) + same(g_clo, peer_clo)
for fmt in ("stl", "fbx"):
    assert (out / f"gui-narrow.{fmt}").read_bytes() == (out / f"peer-narrow.{fmt}").read_bytes(), fmt
# The window's mesh, read here: the solved narrow plate with exactly the two
# saved corners rounded, each wall at its own radius about its own axis.
data = (out / "gui-narrow.stl").read_bytes()
(count,) = struct.unpack_from("<I", data, 80)
assert len(data) == 84 + 50 * count
tri = [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)] for i in range(count)]
pts = [p for t in tri for p in t]
six = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2])
          + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in tri)
x0, y0, w, d = nar["rect_mm"]
xs, ys, zs = ([p[k] for p in pts] for k in range(3))
for got, want in ((min(xs), x0), (max(xs), x0 + w), (min(ys), y0), (max(ys), y0 + d),
                  (min(zs), 0.0), (max(zs), H)):
    assert abs(got - want) < 1e-4, (got, want)
exact = (w * d - (1 - math.pi / 4) * (r1 * r1 + r2 * r2)) * H
slack = sum(math.pi / 2 * r * 0.01 * H for r in (r1, r2))
assert exact - slack - 1e-3 <= six / 6 <= exact + 1e-3, (six / 6, exact)
# Segment 2's start (the stored corner of Fillet 1) is pinned at nar["pin_mm"].
rounded = [(nar["pin_mm"], r1), ([nar["pin_mm"][0], y0 + d], r2)]
for c in ([x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]):
    near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 for p in pts)
    assert near == all(c != at for at, _ in rounded), c
for (cx, cy), r in rounded:
    ax = cx + (r if abs(cx - x0) < 1e-9 else -r)
    ay = cy + (r if abs(cy - y0) < 1e-9 else -r)
    wall = [p for p in pts if abs(p[0] - cx) < r - 1e-4 and abs(p[1] - cy) < r - 1e-4]
    assert len(wall) >= 4, (cx, cy)
    assert all(abs(math.hypot(p[0] - ax, p[1] - ay) - r) < 1e-3 for p in wall), (cx, cy, r)
print("FCAD_28K_GUI_COMPARE_OK", f"cells={cells}")
```
