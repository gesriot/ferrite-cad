# §28E — verification record

[Contract and agent recipe](fillet-base-constraints.md).

What was executed for this slice, where, and what was not. It does not repeat
the evidence of §28A–§28D or of the Line constraint editor.

## Where and how this was run

* **Base.** Freshly fetched `origin/main` at
  `bcd0d37f0d147bfb13ebb382fe686067fafac190` (PR #67). `main` locally was
  behind and was not used; the branch `fillet-base-constraints` was created
  from `origin/main`. PR #65 (§28D, merged with the reviewer's fix to the
  side rule), #66 and #67 are merged. PR #68 was open at `6ffb4a1` while this
  work was done; it merged as `fd83f09` before publication and was brought in
  by an ordinary merge (`c11136a`, see [CI](#ci)). The local results below
  were measured on the `bcd0d37` base; after the merge the affected checks
  were rerun (see [CI](#ci)).
* **Merge-triggered CI on the base** is reported in [CI](#ci), separately from
  this change's runs.
* **Cloud container.** Linux x86_64, 4 CPUs, 15 GiB RAM, the session's existing
  OCCT 8.0.1 install and native/stub targets, one build at a time,
  `CARGO_BUILD_JOBS=2`. OCCT and Boost were not rebuilt.
* **PlaneGCS: local, unpinned.** The proxy refuses the pinned FreeCAD archive
  (HTTP 403) and the pinned Boost archive (no answer), so the pinned library
  cannot be built here and no network restriction was worked around. The
  library used is the one an earlier slice (§27H) built from the FreeCAD
  1.0.1 `planegcs` sources with Ubuntu's Eigen 3.4.0 and Boost 1.83, through
  the repository's own shim; its provenance string reads "LOCAL UNPINNED
  planegcs from FreeCAD 1.0.1 raw sources with Ubuntu Eigen 3.4.0 and Boost
  1.83 - not the pinned delivery". It is linked only through
  `FCAD_PLANEGCS_DIR`/`LD_LIBRARY_PATH`; nothing in the repository points at
  it. **Every local solver result below is from that library; the named CI
  gates on Linux, macOS (Apple Silicon) and Windows with the pinned delivery
  are the authoritative solver evidence.**
* **No window, GPU test, browser or large STEP corpus was run.** The widget
  tests drive real egui widgets headlessly; that is not a window test. The
  complex-STEP test targets were excluded from the local runs.

## What changed

* `document`:
  * `fillet::evaluable_fillet(objects, fillet, built)` separates the
    structure (read from the saved objects: the predecessor, the stored
    rectangle, the joint, the managed constraint family) from the geometry,
    judged on `built` — the Lines the predecessor was built from. For a
    constrained profile, `solved_corner` requires the same four Lines in
    stored order, the shared rectangle reader (`corners_of_lines`, unchanged
    tolerance), every Line on its side (`keeps_every_side`, now shared with
    §28D), the joint by its two UUIDs and the radius bound, with refusals
    worded for the form. `rectangle_corners` (Fillet creation) still refuses
    constraints.
  * `cut_edit::read_history` admits the managed Line family on the base only
    under a Fillet; `saved_fillet` reports `constrained` and applies the radius
    bound to stored Lines only when unconstrained (`check_radius_value` holds
    everywhere).
  * `sketch_edit::constraint_frame` returns the Fillet frame for its base
    Sketch; `ConstraintSketchChoice` and `PreparedSketchConstraints` carry the
    `SavedFillet`.
  * `write_sketch_constraints` re-derives inside its transaction
    (`sketch_constraints::rederive`): frame and family read again, curves and
    plane unchanged, removals removable, additions new, the written list equal
    to the stored list minus removals plus additions, result managed. This
    applies to every constraint write, not only under a Fillet.
* `eval`: the Fillet branch passes the profile presentation the rebuild already
  built (solved or stored) to `evaluable_fillet`, before the cache lookup, so
  cold and cached rebuilds ask the same question; no second solve.
* `cli`: additive `constraint_edit.fillet_base` and `profile_constrained`;
  `radius_edit.max_radius_mm` is `null` for a constrained plate.
* `app`: one context line in the existing constraint form; the radius form
  says the bound is the solved plate's for a constrained plate.
* No command, request, capability, payload version, schema or cache key.

## New and changed tests

* **Domain** (`fillet_radius::tests`, kernel-free):
  `constraints_on_the_rounded_plate_change_only_the_sketch_and_defer_the_bound`
  (the SQL allowlist, names, stored corner, a 50 mm radius not refused by
  stored lengths, the coordinate editor's refusal, the height keeping the
  Sketch bytes, §28A creation still refusing constraints);
  `the_constraint_writer_rederives_the_edit_under_a_fillet` (moved curves, a
  dropped closure link, an extra constraint, a Fillet changed after
  preparation — each refused, nothing written);
  `the_evaluator_judges_the_fillet_on_the_built_lines` (both sides of the 2 r
  bound, both sides of the 1e-7 mm rectangle tolerance, a reversed Line, a
  reordered loop). Two earlier assertions that the constraint editor refuses a
  rounded plate now assert it is available.
* **CLI** (`tests/fillet.rs`, `mod constraints`):
  `constraint_discovery_and_protocol_without_native`,
  `occt_without_solver_refuses_a_constrained_rounded_plate`,
  `native_constraints_solve_the_rounded_plate_and_keep_every_identity` (CCW,
  CW from the upper right, CCW from the third Line; two corners; translated
  to (-9.25, -2.5), 41.125 × 15.625, r 3.0625),
  `native_replace_length_and_exact_removals_resize_the_rounded_plate`,
  `native_radius_and_height_after_constraints_answer_to_the_solved_plate`,
  `native_constraint_refusals_under_a_fillet_are_atomic`,
  `native_the_radius_bound_is_exact_on_the_solved_plate`. Each published copy
  is checked for: the SQL allowlist, names, stored Lines and Line UUIDs, the
  Fillet row, DOF from the result and from a cold rebuild, the solved starts
  against the expected rectangle, 7 faces, volume `(W·D−(1−π/4)r²)·h` to 1e-9,
  the named face a cylinder of r about an axis r inward of the solved corner,
  cold = Miss = Hit, the STL with its own reader, and FBX.
* **App** (`constraints::tests`):
  `fillet_base_constraint_widgets_name_the_fillet_and_keep_the_draft`
  (kernel-free) and
  `native_fillet_base_constraint_worker_and_cli_publish_the_same_part`
  (worker vs peer CLI: every SQL row equal once the minted constraint UUIDs
  are matched off, stored Lines unchanged, STL and FBX byte-identical, the
  solved volume).

## Local results

**OCCT + local unpinned PlaneGCS** (`--features planegcs`, debug,
`FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=1`):

* `--test fillet`: 29 passed (7 §28A, 5 §28B, 5 §28C, 5 §28D, 7 §28E), no
  `skipped:` but the mixed gate's (N/A where a solver is linked).
* Solved against expected, worst coordinate error: 3.55e-15 mm (CCW), 0 in
  the other five copies — nine orders of magnitude inside the rectangle
  reader's 1e-7 mm; the first CI run printed the same values on macOS with
  the pinned PlaneGCS. The class is not widened and nothing is snapped. The
  tests assert `< 1e-9 mm`; the diagnostic print was removed because it broke
  the gate's `test … ... ok` line (see [CI](#ci)).
* DOF measured (not computed): 0 fully dimensioned, 2 after removing the
  exact pin UUID, 8 with the four closure links alone.
* Refusals (typed, nothing written): a solved plate 4.5 mm deep for r 2.375
  (`input`, "too short"), depth 2 r − 1e-7 (`input`) while exactly 2 r
  publishes; the first Line pinned beyond its own end (`input`, "must keep its
  side": +X would run −X); one length without H/V (`unsupported`, "no longer an
  axis-aligned rectangle"); both horizontal sides dimensioned differently
  (`constraint`, `constraint_conflict` with UUIDs); stale version and occupied
  destination (`input`).
* The radius after constraints: 7 mm publishes on a solved plate 41.25 ×
  15.5 (the stored rectangle would allow only 6.125 mm); 5 mm is refused on a
  solved plate 30 × 8.5 (the stored one would allow it). The height keeps the
  Sketch row byte for byte.
* Wider suites with the same solver: CLI (`fillet`, `edit_constraints`,
  `circle_constraints`, `annular_constraints`, `revolve`, `edit_sketch`,
  `edit_extrude`, `sketch_extrude`, `rebuild`, `validate`) 108 passed, 2
  failed: the mixed gate (since made an explicit skip in solver builds) and
  the root-only `validation_really_read_only_permissions`. App: 359 passed,
  including the 18 tests that fail without a solver. eval/jobs/document: 466
  passed, 1 ignored.
* Recipe (extracted from Markdown, real CLI): `FCAD_28E_RECIPE_OK
  dimensioned=4329.158834/4329.231546 replaced=3182.186177/3182.258890
  radius7=3119.207964/3119.450130 taller=4505.522614/4505.872410` (mesh volume
  against the exact one).
* **FBX.** The whole `tools/check-fbx-complex.sh` as CI runs it, with the
  §28A–§28E artifacts (60 files): `FCAD_FILLET_UFBX_EXECUTED`, `_RADIUS_`,
  `_HEIGHT_`, `_SKETCH_` and `FCAD_FILLET_CONSTRAINTS_UFBX_EXECUTED`, exit 0,
  6 min 7 s. The six new files each read `checks=6 failures=0` and joined
  their STL at 64–68 triangles, worst 6.94e-18 m.
* SQL, measured on a constraint copy: the Sketch row's `schema_version`,
  `payload`, `payload_hash` and one new `capabilities` row
  (`sketch.constraints.v1`); a further constraint copy moves only `payload`
  and `payload_hash`. `meta.modified_at` is not stamped by this writer.

**OCCT without PlaneGCS** (`--no-default-features`,
`FERRITECAD_REQUIRE_PLANEGCS=0`), the workflow's exact form: the eight
§28A–§28D no-solver gates and the two §28E ones
(`constraint_discovery_and_protocol_without_native`,
`occt_without_solver_refuses_a_constrained_rounded_plate`) each `ok`, no
`skipped:`; the recipe printed `FCAD_28E_RECIPE_NO_SOLVER` with the typed
`unsupported` ("this sketch carries 11 constraint(s) and this build did not
link planegcs"). The CLI imports no PlaneGCS.

**Stub (no OCCT, no PlaneGCS)**, proved by its build:

* CMake was configured explicitly — no change to `HOME` — with
  `CMAKE_TOOLCHAIN_FILE` naming a file that sets
  `CMAKE_FIND_USE_PACKAGE_REGISTRY FALSE`,
  `CMAKE_FIND_USE_SYSTEM_PACKAGE_REGISTRY FALSE` and `CMAKE_IGNORE_PREFIX_PATH`
  to the session's native prefixes; the stub's `ferritecad-occt` build
  directories were removed so the configure ran again. Its
  `bridge-build/CMakeCache.txt` records that toolchain file and
  `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`.
* `ldd` of the stub `ferritecad`, the `fillet` test binary and the viewer's
  test binary: 0 `libTK*` and 0 PlaneGCS imports (5 shared objects each).
* The new `ci.yml` step, run as written: the discovery gate, the widget gate
  and the three domain gates `ok` with no `skipped:`; the recipe printed
  `FCAD_28E_RECIPE_NO_KERNEL`.
* The whole stub `--test fillet`: 29 passed of which **23 are geometry skips
  (`skipped: this build has no Open CASCADE`), N/A and not passes** —
  including the six `constraints::native_*` tests and the mixed gate. Stub
  document `fillet` tests 19 and app `constraints::` 25 passed.

**Workspace regression without a solver** (OCCT, `FERRITECAD_REQUIRE_OCCT=1`,
`FERRITECAD_REQUIRE_PLANEGCS=0`, the peer CLI rebuilt without PlaneGCS first;
the complex-STEP targets excluded — none of the baseline failures is in them):
2113 passed, 21 failed, 2 ignored. Compared line by line with the earlier
baseline of this container (19: 18 tests that need a linked PlaneGCS while
`FERRITECAD_REQUIRE_OCCT=1`, and the root-only
`validation_really_read_only_permissions`), the two extra were:

* `constraints::tests::native_fillet_base_constraint_worker_and_cli_publish_the_same_part`
  — this slice's app test, which asserted `FERRITECAD_REQUIRE_OCCT` when only
  the solver was missing, the same pattern as the 18. It now asserts each
  requirement only for what is missing, and in this configuration prints
  `skipped:` (N/A); it passed with the solver. Only this test was run again
  after that change, not the whole regression.
* `read_only_permissions_still_dump_when_the_file_can_be_read` — new on the
  base with PR #67, root-only like the validation one. Both pass under UID
  65534 (`setpriv --reuid=65534 --regid=65534 --clear-groups`).

A first attempt at this regression was invalid and is not counted: the app's
peer CLI was still the solver-linked binary, which cannot load
`libplanegcs.so` without that library on the path.

`cargo fmt --all -- --check` and `git diff --check` are clean. `cargo clippy
--workspace --all-targets -- -D warnings` is clean natively; so is clippy of
CLI, app, eval and jobs with `planegcs`, and the stub target with
`--all-features`. Clippy asked for `CopyWrite::Constraints` to be boxed (it
now carries the Fillet); it is.

### Mutations — local, executed, restored byte for byte

Each was applied by a script, compiled and run with the local solver; the file
was restored from a copy and checked with `sha256sum -c` (`fillet.rs`
b277c2a1…, `sketch_constraints.rs` bc9edcef…); the positive gates were run
again afterwards (document `fillet` 19, CLI `--test fillet` 28 + the mixed
N/A, app `fillet` 10).

| Mutation | Caught by (executed assertion) |
| --- | --- |
| M1 — the Fillet judged on the **stored** Lines instead of the built (solved) ones | domain `the_evaluator_judges_the_fillet_on_the_built_lines`; CLI `native_radius_and_height_after_constraints…` (radius 7 refused, exit 2 where 0 was due), `native_the_radius_bound_is_exact…` and `native_constraint_refusals_under_a_fillet…` (a too-narrow solved plate published, exit 0 where 2 was due) |
| M2 — the strict re-check that every Line keeps its side on the solved plate removed | domain `the_evaluator_judges_the_fillet_on_the_built_lines` (reversed Line accepted); CLI `native_constraint_refusals_under_a_fillet…` (the reversed plate published, exit 0 where 2 was due) |
| M3 — the writer's re-derivation no longer compares curves and plane | domain `the_constraint_writer_rederives_the_edit_under_a_fillet` (moved curves written) |

Considered and not counted: finding the corner by position instead of by the
joint's UUIDs is equivalent here, because the same four Lines in the same order
give the same joints at the same positions.

### Compatibility with the base's reader

The CLI of the base `bcd0d37`, built from `git archive` into a separate target
with the same OCCT and local PlaneGCS, on a copy whose rounded plate's Sketch
carries seven user constraints: `rebuild --cold` and `export-stl` refuse with
`unsupported: this slice rounds an edge of an unconstrained profile; the
profile carries constraints` (exit 2, no STL written); `inspect` offers neither
the constraint editor nor the radius edit ("this slice edits an unconstrained
part"). This build rebuilds the same file (10 of 10 references) and exports 64
triangles. No silent wrong shape, so no new capability or payload version.

## CI

* **Base `bcd0d37`, merge-triggered, checked separately:** CI
  ([run 36410560616](https://github.com/gesriot/ferrite-cad/actions/runs/36410560616)),
  combined runtime layout
  ([run 36410560478](https://github.com/gesriot/ferrite-cad/actions/runs/36410560478)),
  product sbom, rust sbom and rust notices concluded success. PR #67 did not
  touch `planegcs-pin.yml`'s paths, so the base has no pin run; the last one on
  `main`, on `4e0e5ca`
  ([run 36402274919](https://github.com/gesriot/ferrite-cad/actions/runs/36402274919)),
  succeeded.
* **PR #68** merged into `main` as `fd83f09` while this branch was in CI.
  Its merge-triggered CI
  ([run 36455639717](https://github.com/gesriot/ferrite-cad/actions/runs/36455639717)),
  combined runtime layout
  ([run 36455639383](https://github.com/gesriot/ferrite-cad/actions/runs/36455639383)),
  product sbom, rust sbom and rust notices concluded success. `origin/main`
  was fetched again and merged into this branch with an ordinary merge
  commit, `c11136a` (no rebase, amend or cherry-pick). It touched only the
  CLI's `render.rs`/`dump_graph` test and two docs this change also edits;
  there was no conflict. After the merge, locally: `cargo fmt --check`,
  `cargo clippy -p ferritecad-cli --features planegcs --all-targets -D
  warnings`, and `tests/fillet.rs` with the solver (29 passed). PR #68's
  `read_only_permissions_still_dump_when_the_file_can_be_read` fails in
  this container only because it runs as root (the test refuses privileged
  chmod as evidence); it is unchanged by this branch and green in CI.
* **This change, first run (`9a5c514`):** CI
  ([run 36455428739](https://github.com/gesriot/ferrite-cad/actions/runs/36455428739))
  and planegcs pin
  ([run 36455391325](https://github.com/gesriot/ferrite-cad/actions/runs/36455391325))
  succeeded. The combined runtime layout
  ([run 36455391479](https://github.com/gesriot/ferrite-cad/actions/runs/36455391479))
  failed on Linux and macOS in "Round one vertical edge of a saved plate
  into a named Fillet": `fillet process gate
  constraints::native_constraints_solve_the_rounded_plate_and_keep_every_identity
  did not execute`. The test passed, but its diagnostic `eprintln!`
  (`FCAD_28E_SOLVED … worst_mm=3.55e-15 / 0 / 0`, the same values as the
  local run) was interleaved into the `test … ... ok` line, which the
  exact-name gate reads. `d034074` removed those prints; no gate was
  weakened. The run was then cancelled by the newer push.
* **This change, code head `c11136a`** (the merge with `fd83f09`): CI
  ([run 36458511064](https://github.com/gesriot/ferrite-cad/actions/runs/36458511064))
  and the combined runtime layout
  ([run 36458509339](https://github.com/gesriot/ferrite-cad/actions/runs/36458509339):
  Linux, macOS on Apple Silicon and Windows, and the platform comparison)
  concluded success, including the no-solver step and the Fillet step with
  the new exact-name gates and the FBX campaign. No planegcs-pin path
  changed after `9a5c514`, so its run 36455391325 is the pin evidence for
  this code. This documentation-only commit changes no workflow input of the
  runtime layout or the pin; CI runs on it.

## Limits

* The class: one Fillet over one XY axis-aligned rectangular Blind/NewBody
  plate, the constraint editor's existing Line family. Not supported:
  creating a Fillet on a constrained profile, a second Fillet, Cut with
  Fillet, Chamfer, another quadrilateral or plane, new constraint families,
  live preview, picking edges, in-place Save. The 5C milestone is not closed.
* The coordinate editor refuses a constrained Sketch, including one left with
  only its closure links; there is no "remove all constraints" route.
* Local solver evidence is from an unpinned PlaneGCS; the pinned one runs
  only in CI.
* A mesh check is chord-bounded; exact claims are the B-Rep's.
* No window, GPU, browser or STEP-corpus test was run here; the historical
  fillet-corpus OOM is not explained. The workflow job logs cannot be
  downloaded through this container's proxy, so no gate count is claimed from
  them.

## Independent review on macOS arm64 (2026-09-28)

Reviewed code head `68bef1fa1decf3268d89265e46b1ec307aaeca6c`; no production
defect found. The review follow-up changes documentation only. All commits
have the user's author and committer identity and no AI trailers; the generated
attribution footer was removed from the PR description.

The pinned local OCCT/PlaneGCS release build passed 466 document/jobs/eval
tests (one existing timing benchmark ignored), 28 executed CLI Fillet tests
plus the explicit mixed-build N/A, and 25 app constraint tests without skips.
The linked solver reports FreeCAD 1.0.1 archive
`f62bc07c477544eff62b6ab0fc3bb63fa7f1e6f94763c51b0049507842d444f3`.
Workspace all-target/all-feature clippy with `-D warnings` and fmt passed.

A freshly staged, strictly signature-verified bundle ran the window scenario
below: Open, seven additions, Undo/Redo, Save Cancel, too-small-plate refusal
(no `never.fcad`), corrected publication and async Open, both exports, Replace
length, second publication and both exports. The stored-corner explanation and
fully-constrained/zero-DOF result were visible. The comparator reported
`FCAD_28E_GUI_COMPARE_OK cells=242 constraints=11/11`: source unchanged, SQL
allowlists satisfied, GUI/CLI STL and FBX byte-identical. Pinned ufbx 0.23.0
read both window FBX files (6 checks/0 failures each); the oriented-triangle
join matched each to its 64-triangle STL, worst error `6.94e-18 m`.

One viewer, PID 82536, was watched throughout: peak footprint **206.111 MiB**,
normal pressure, swap 0 throughout, exit 0, watchdog not triggered. After Quit
only the PID was checked; no CUA app lookup relaunched it. The first sandboxed
watchdog attempt was refused access to memory sysctl before launching a viewer;
the completed run used the same watchdog with that access. The old OOM remains
unexplained. Local evidence: `/private/tmp/ferrite-pr69-review/`.

The reviewer downloaded all three runtime job logs from run `36458509339` and
confirmed the new exact-name tests, no-solver gate, recipe and constrained-FBX
reader marker on each OS. Its code matches the reviewed head; only this
verification file changed after `c11136a`. The paths of planegcs-pin are also
unchanged after its successful `9a5c514` run. The final-head ordinary CI is
tracked separately from that native evidence.

## macOS fixtures and window scenario (for the reviewer's Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised here with a
second CLI copy standing in for the window, which says nothing about the
window: `FCAD_28E_GUI_COMPARE_OK cells=242 constraints=11/11`; a tampered
`objects.name` cell in `gui2.fcad` was caught; a missing `gui.stl` was refused
without anything being created.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon (arm64) Mac, and do not set `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `rounded.fcad` (drawn clockwise from (33, 15.5), 37.5 × 12.25 ×
6.75 mm, the §28A Fillet r = 2.375 mm at (33, 3.25)) and `facts.json`:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/fillet-base-constraints-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28E_GUI_FIXTURE\n", "ferrite-28e-fixture.py"),
                   ("# FCAD_28E_GUI_COMPARE\n", "ferrite-28e-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28e-fixture.py "$FCAD_28E_DIR"
```

```python
# FCAD_28E_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-constraints-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# Clockwise from the upper right, as the viewer's own widget tests draw it:
# Segment 1 runs -Y, 2 runs -X, 3 runs +Y, 4 runs +X.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
CORNER, R, H = [33.0, 3.25], 2.375, 6.75
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H, "points_mm": PLATE}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
catalog = run("inspect", plate, "--json")["result"]
body = catalog["bodies"][0]
chosen = next(c for c in body["fillet_edge"]["target"]["candidates"] if c["corner_mm"] == CORNER)
request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": R}))
source = out / "rounded.fcad"
run("fillet-edge-copy", plate, "--body", body["body_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", source, "--json")
request.unlink()
plate.unlink()
catalog = run("inspect", source, "--json")["result"]
(sketch,) = catalog["sketches"]
editor = sketch["constraint_edit"]
assert editor["available"] is True and editor["fillet_base"] is not None, editor
lines = [c["curve_id"] for c in editor["curves"]]
assert [c["start_mm"] for c in editor["curves"]] == PLATE
# The window's first copy: V, H, V, H on Segments 1..4; Segment 2's start
# (the rounded corner) fixed at (36.5, 1.25); Segment 2 = 41 mm; Segment 1 =
# 14.25 mm. Its second copy, from the first: Segment 2's length replaced by
# 30.25 mm.
first = [{"curve_id": lines[0], "rule": "vertical"},
         {"curve_id": lines[1], "rule": "horizontal"},
         {"curve_id": lines[2], "rule": "vertical"},
         {"curve_id": lines[3], "rule": "horizontal"},
         {"curve_id": lines[1], "rule": "fixed", "at": "start", "x_mm": 36.5, "y_mm": 1.25},
         {"curve_id": lines[1], "rule": "distance", "distance_mm": 41.0},
         {"curve_id": lines[0], "rule": "distance", "distance_mm": 14.25}]
(out / "facts.json").write_text(json.dumps({
    "sketch_id": sketch["sketch_id"], "content_version": catalog["content_version"],
    "lines": lines, "first": first, "replaced_mm": 30.25, "replaced_line": lines[1],
    "fillet_feature_id": editor["fillet_base"]["fillet_feature_id"],
    "edge": editor["fillet_base"]["edge"], "radius_mm": R, "height_mm": H,
    "stored_mm": PLATE,
    "gui_rect_mm": [-4.5, 1.25, 41.0, 14.25], "gui_corner_mm": [36.5, 1.25],
    "gui2_rect_mm": [6.25, 1.25, 30.25, 14.25], "gui2_corner_mm": [36.5, 1.25],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28E_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_28E_DIR/../watch-28e.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

1. **Open** `$FCAD_28E_DIR/rounded.fcad`. **Edit constraints <Sketch name> —
   <UUID>…** is enabled (before §28E it was refused naming the Fillet).
2. **Form.** Press it. It shows "Coordinates below are stored inputs, not the
   solved drawing." and "Rounded by Fillet <UUID> at the corner of Lines
   <UUID> | <UUID>, r 2.375 mm (stored corner (33, 3.25)). The Fillet keeps its
   corner and radius: the new copy is saved only if the solved plate is still
   a rectangle with every Line on its side and each side at that corner at
   least 4.75 mm."
3. **Draft.** Select Segment 1, **Add Vertical**; Segment 2, **Add
   Horizontal**; Segment 3, **Add Vertical**; Segment 4, **Add Horizontal**.
   Select Segment 2, Fixed X `36.5`, Fixed Y `1.25`, **Add Fixed point** (its
   Start is the rounded corner). With Segment 2 selected, Line length `41`,
   **Add length**. Select Segment 1, Line length `4.5`, **Add length**.
4. **Undo/Redo.** **Undo**, **Redo**: the 4.5 mm length is pending again.
5. **Save Cancel.** **Save constraints copy…** → **Cancel**: nothing starts;
   the draft stays.
6. **Refusal.** **Save constraints copy…** → `$FCAD_28E_DIR/never.fcad`. The
   job is refused: the form comes back with its draft, and the status line
   reads "Could not save edited model: invalid input: as its constraints solve it, the rounded plate has sides of 4.5 and 41 mm at the
   rounded corner, too short for the saved radius: a fillet of 2.375 mm at
   corner the joint of segments <UUID> and <UUID> is too large: this build
   rounds up to 0.5 × the shorter adjacent side (4.5 mm), which is 2.25 mm;
   nothing is clamped". `never.fcad` does not exist.
7. **Fix and publish.** **Undo** (the 4.5 mm length goes), select Segment 1,
   Line length `14.25`, **Add length**, **Save constraints copy…** →
   `$FCAD_28E_DIR/gui.fcad`. The viewer opens the copy asynchronously: the
   plate is 41 × 14.25 mm with the rounded corner at (36.5, 1.25).
8. **Exports.** From the window export `gui.stl` and `gui.fbx` of `gui.fcad`
   into `$FCAD_28E_DIR`, at the default tessellation.
9. **Replace length.** In the opened `gui.fcad`, **Edit constraints…**, select
   Segment 2 ("Stored length 41 mm · <UUID>" appears), Line length `30.25`,
   **Replace length**, **Save constraints copy…** → `$FCAD_28E_DIR/gui2.fcad`;
   export `gui2.stl` and `gui2.fbx`. The plate is 30.25 × 14.25, corner still
   at (36.5, 1.25).
10. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28e-compare.py "$FCAD_28E_DIR"
```

The comparator never creates or overwrites a `gui*` file; a missing one is a
failure. It makes the peer copies with `edit-sketch-constraints-copy` from the
same requests and exports them; compares every table of `gui.fcad`/`gui2.fcad`
with the peer's, allowing only the stamp and the Sketch payload/hash that carry
the newly minted constraint UUIDs, whose rules must then be equal in order;
checks each window copy against its own source by the allowlist; requires the
stored Lines unchanged, the Fillet's UUID, edge and radius, the solved extents
from the STL and byte-identical STL and FBX against the peer's; and prints
`FCAD_28E_GUI_COMPARE_OK`.

```python
# FCAD_28E_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
source = out / "rounded.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
for name in ("gui.fcad", "gui.stl", "gui.fbx", "gui2.fcad", "gui2.stl", "gui2.fbx"):
    assert (out / name).exists(), f"produce {name} from the window first"

def run(*args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == code, (args, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def peer(src, name, remove, add):
    """The same request through the shipped CLI, exported at the defaults."""
    target = out / f"{name}.fcad"
    if not target.exists():
        catalog = run("inspect", src, "--json")["result"]
        request = out / f"{name}-request.json"
        request.write_text(json.dumps({"request_version": 1, "remove": remove, "add": add}))
        run("edit-sketch-constraints-copy", src, "--sketch", facts["sketch_id"],
            "--expect-version", catalog["content_version"], "--request", request,
            "-o", target, "--json")
    for fmt in ("stl", "fbx"):
        if not (out / f"{name}.{fmt}").exists():
            run(f"export-{fmt}", target, "-o", out / f"{name}.{fmt}", "--json")
    return target

def tables(path):
    db = sqlite3.connect(f"{path.as_uri()}?mode=ro", uri=True)
    result = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        result[t] = ([d[0] for d in cur.description], sorted(cur.fetchall(), key=repr))
    db.close()
    return result

sketch = bytes.fromhex(facts["sketch_id"].replace("-", ""))
def differing(a, b):
    """Every cell that differs, as (table, column, id), comparing rows by id
    where there is one. New capability rows are reported as such."""
    left, right = tables(a), tables(b)
    assert left.keys() == right.keys()
    moved, compared = set(), 0
    for t in left:
        (lc, lrows), (rc, rrows) = left[t], right[t]
        assert lc == rc, t
        if t == "capabilities":
            for row in set(rrows) - set(lrows):
                moved.add(("capabilities", "row", row[0]))
            assert set(lrows) <= set(rrows), "a capability changed"
            continue
        assert len(lrows) == len(rrows), t
        if "id" in lc:
            k = lc.index("id")
            lrows, rrows = sorted(lrows, key=lambda r: r[k]), sorted(rrows, key=lambda r: r[k])
        for lr, rr in zip(lrows, rrows):
            for c, x, y in zip(lc, lr, rr):
                compared += 1
                if x != y:
                    moved.add((t, c, lr[lc.index("id")] if "id" in lc else None))
    return moved, compared

stamp = {("meta", "modified_at", 1)}
sketch_cells = {("objects", c, sketch) for c in ("schema_version", "payload", "payload_hash")}

def constraints(path):
    row = run("inspect", path, "--json")["result"]["sketches"][0]["constraint_edit"]
    return row["curves"], row["constraints"]

def same_model(gui, other):
    """The window's copy and the peer's: every SQL cell equal except the
    Sketch payload/hash that carry the newly minted constraint UUIDs, which
    are matched off rule by rule; the constraint lists equal once mapped."""
    moved, compared = differing(gui, other)
    assert moved <= stamp | {("objects", "payload", sketch), ("objects", "payload_hash", sketch)}, moved
    gc, gk = constraints(gui)
    pc, pk = constraints(other)
    assert gc == pc, "the stored Lines differ"
    assert [c["rule"] for c in gk] == [c["rule"] for c in pk], "the constraints differ"
    return compared

def stl_extent(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    pts = [struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k)
           for i in range(count) for k in range(3)]
    return [min(p[0] for p in pts), min(p[1] for p in pts),
            max(p[0] for p in pts), max(p[1] for p in pts)]

def solved(path, rect, corner):
    x0, y0, w, d = rect
    lo_x, lo_y, hi_x, hi_y = stl_extent(path.with_suffix(".stl"))
    for got, want in ((lo_x, x0), (lo_y, y0), (hi_x, x0 + w), (hi_y, y0 + d)):
        assert abs(got - want) < 1e-4, (path, got, want)
    result = run("inspect", path, "--json")["result"]
    (fillet,) = result["fillets"]
    assert fillet["feature_id"] == facts["fillet_feature_id"] and fillet["edge"] == facts["edge"]
    assert fillet["radius_mm"] == facts["radius_mm"] and fillet["profile_constrained"] is True
    assert result["sketches"][0]["constraint_edit"]["fillet_base"]["stored_corner_mm"] == [33.0, 3.25]

# First copy, against the source and against its peer.
moved, _ = differing(source, out / "gui.fcad")
assert moved - stamp - sketch_cells <= {("capabilities", "row", "sketch.constraints.v1")}, moved
assert sketch_cells & moved, moved
p1 = peer(source, "peer", [], facts["first"])
cells = same_model(out / "gui.fcad", p1)
for fmt in ("stl", "fbx"):
    assert (out / f"gui.{fmt}").read_bytes() == (out / f"peer.{fmt}").read_bytes(), fmt
curves, listed = constraints(out / "gui.fcad")
assert [c["start_mm"] for c in curves] == facts["stored_mm"], "the stored guess moved"
solved(out / "gui.fcad", facts["gui_rect_mm"], facts["gui_corner_mm"])

# Second copy (Replace length), from the first window copy.
width = next(c["constraint_id"] for c in listed
             if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == 41.0)
p2 = peer(out / "gui.fcad", "peer2", [width],
          [{"curve_id": facts["replaced_line"], "rule": "distance",
            "distance_mm": facts["replaced_mm"]}])
moved, _ = differing(out / "gui.fcad", out / "gui2.fcad")
assert moved - stamp <= sketch_cells, moved
cells += same_model(out / "gui2.fcad", p2)
for fmt in ("stl", "fbx"):
    assert (out / f"gui2.{fmt}").read_bytes() == (out / f"peer2.{fmt}").read_bytes(), fmt
_, listed2 = constraints(out / "gui2.fcad")
assert not any(c["constraint_id"] == width for c in listed2)
solved(out / "gui2.fcad", facts["gui2_rect_mm"], facts["gui2_corner_mm"])
print("FCAD_28E_GUI_COMPARE_OK", f"cells={cells}", f"constraints={len(listed)}/{len(listed2)}")
```
