# §27G — verification record

[Contract and agent recipe](revolve-profile-constraints.md).

This records what was executed for this slice and where. It does not repeat
the earlier slices' evidence.

## Where and how this was run

* **Base.** `origin/main` at `5caaa1ca293cdffc3e8daf5490bb03281120adf3`,
  the merge of PR #58. Its tree `94bf726…` equals the reviewed §27F head
  `731fea9`. The branch is `revolve-profile-constraints`.
* **Cloud container.** Linux x86_64, 4 CPUs, 15 GiB RAM, using the
  session's installed OCCT 8.0.1 and its existing native/stub targets.
  * **PlaneGCS is not available here.** The proxy refuses the pinned
    FreeCAD and Boost archives (HTTP 403), so no solver was built. Every
    test that solves therefore runs only in the named CI gates
    (Linux/macOS/Windows, `FERRITECAD_REQUIRE_PLANEGCS=1`, exact name, no
    skip).
  * **What did run locally:** the discovery, request, writer, UI-widget,
    worker-refusal and OCCT-without-solver evidence.
* **No window, GPU test or browser was launched.** The widget tests drive
  real egui widgets headlessly; that is not a window test. The macOS window
  smoke is the reviewer's (see the last section).

## What changed

* **Domain** (`ferritecad-document`):
  * `sketch_edit::constraint_frame` gives the constraint editor the owning
    feature explicitly, as the existing `SketchProfileUse`. A Sketch that a
    Revolve turns is judged by the shared `revolve_document` frame, never by
    the Extrude frame.
  * Axis-closed Revolve profiles are refused by name.
  * `constrained_lines` checks the **stored** inputs against the owner's
    policy.
  * `ConstraintSketchChoice` gains `profile_use`; `height_mm` is now `None`
    for a Revolve.
  * `PreparedSketchConstraints` carries a private `profile_use` with an
    accessor, in place of the public `height_mm`.
  * `SketchProfileUse::check` is public, so a solved profile is judged by
    the same function as a stored one.
  * `revolve_choices` no longer refuses a profile merely for carrying
    constraints. This is what lets the §27E angle edit keep them.
* **Copy job** (`ferritecad-jobs`): the solved-profile check passes the
  solved Lines, with their UUIDs, through `SketchProfileUse::check`.
  * For an Extrude this is exactly the previous `PolygonExtrusion::new`.
  * For a Revolve it is `stated_revolution`: class, axis clearance and a
    simple polygon.
  * A circle profile still uses the extrusion height, which a Revolve never
    reaches.
* **CLI:**
  * `constraint_edit.profile_feature` is additive, reusing the
    `ProfileFeature` kinds.
  * The `edit-sketch-constraints-copy` request now also refuses arrays in
    place of the request or an addition.
* **UI:** one line in the existing Edit constraints window names the
  owning Revolve and its kept turn.
* **Evaluator, kernel, OCCT bridge, topology, solver and storage are
  unchanged.** `git diff 5caaa1c -- crates/ferritecad-eval
  crates/ferritecad-kernel crates/ferritecad-occt
  crates/ferritecad-occt-bridge crates/ferritecad-topology
  crates/ferritecad-sketch-solver` is empty. There is no new dependency,
  capability or migration.

## New tests and gates

| Test (exact name) | Where it executes | What it shows |
| --- | --- | --- |
| `constraints::revolve_constraint_discovery_writer_and_refusals_without_solver` (CLI `revolve`) | local native, local stub-equivalent paths, ordinary CI (3 OS, no kernel), runtime layout | Discovery, refusals, the writer's allowlist and request shapes (below). |
| `constraints::occt_without_solver_refuses_revolve_constraints_honestly` | local (OCCT, no PlaneGCS), runtime layout OCCT-without-solver step | The honest no-solver behaviour (below). |
| `constraints::native_bushing_constraints_dimension_replace_remove_cache_and_exports` | runtime layout only (PlaneGCS) | The full-turn bushing (below). |
| `constraints::native_sector_constraints_measure_caps_names_angle_edit_and_exports` | runtime layout only (PlaneGCS) | The 137.5° sector (below). |
| `constraints::tests::revolve::revolve_constraint_widgets_name_the_revolve_build_and_replace_a_length` (app) | local, ordinary CI, runtime layout | The real widgets, headless (below). |
| `constraints::tests::revolve::native_revolve_constraint_worker_and_cli_publish_the_same_solved_sector` (app) | runtime layout only (PlaneGCS) | The real worker against the peer CLI (below). |

**`revolve_constraint_discovery_writer_and_refusals_without_solver`**
* **Discovery.** A full turn and a sector with a bore: `constraint_edit`
  is available with `profile_feature` equal to the row's (the sector's
  with `angle_deg` 137.5), with no `height_mm`, empty constraints and 4
  curves.
* **Refused by name** ("closed on the axis along Line"): axis-closed v2 and
  v4.
* **Refused, stored inputs outside the class:** a constrained profile whose
  stored inputs touch the axis, with its full closure so that the axis is
  the only reason; and a Circle profile ("Lines").
* **Writer, with no kernel or solver.** The prepared edit writes exactly
  the allowlisted cells. A document that has moved on refuses the same
  preparation ("changed") and stays byte-identical.
* **The other editors' policies:**
  * a constrained profile is not coordinate-editable ("unconstrained");
  * `revolves[].profile` is available;
  * `sketch.constraints.v1` is required;
  * a constrained sector's angle edit is offered.
* **Request shapes, refused before any kernel in every build:** a request
  array, an addition array, a duplicate key, an escaped duplicate
  `curve_id`, an escaped duplicate `rule`, an unknown field and
  `request_version` 2. Nothing is written and the source is byte-identical.

**`occt_without_solver_refuses_revolve_constraints_honestly`**
* **The class is offered, but the copy is refused.** A constraint copy of
  a sector is refused with `unsupported` naming planegcs. Nothing is
  written and the source is unchanged.
* **A constrained sector is still a document.** Written directly, it
  validates and inspects. `export-stl` exits 2 naming planegcs and writes
  no file. The angle edit is offered, then refused naming planegcs at the
  baseline rebuild.

**`native_bushing_constraints_dimension_replace_remove_cache_and_exports`**
The profile is `[[4.25,-1.5],[10.5,-1.5],[10.5,13.75],[4.25,13.75]]`, a
full turn.
* **Rigid set** (H/V on all four Lines, a Fixed start (4.25, −1.5),
  D0 6.25, D1 15.25):
  * 4 closure + 7 added, DOF 0, nothing redundant;
  * the SQL allowlist holds, the stored curves, refs and the Revolve
    payload are the source's;
  * the solved profile equals the stored one within 1e-7;
  * volume = π(10.5² − 4.25²)·15.25 within 1e-7;
  * cache: Miss, then Hit.
* **Changed dimension.** The exact D0 UUID is removed and 7.5 added:
  * one new UUID, every other UUID kept, DOF 0;
  * solved `[[4.25,-1.5],[11.75,-1.5],[11.75,13.75],[4.25,13.75]]`;
  * volume = π(11.75² − 4.25²)·15.25;
  * the stored curves stay byte-identical;
  * **Miss** through the store the rigid copy filled, then Hit;
  * cold equals cached.
* **Every face under its own UUID.** Each saved face resolves to exactly
  one face:
  * horizontal Lines turn into planes, vertical ones into cylinders of
    their solved radius (within 1e-9) about the Y axis;
  * each face goes all the way round.
* **Exports.** An independent STL check (closed, oriented, inside the
  turned solved profile, within the chord band) and a complete FBX.
* **Redundancy** (EqualLength of the opposite sides): published, DOF 0,
  and the solver names the added equality.
* **Refused, sources unchanged, nothing written:**
  * a real conflict (EqualLength of unequal sides): `constraint` with
    `constraint_conflict` naming stored UUIDs and the equality;
  * the pin moved to x = −1, crossing the axis;
  * the pin moved to x = 0, reaching the axis;
  * on the unconstrained source, one pinned corner:
    * a self-intersection ("polygon edges intersect or touch within
      0.000001 mm");
    * a collapsed Line ("a line segment needs two distinct endpoints").
* **All seven user constraints removed** by exact UUID:
  * 4 Coincident remain, DOF 8;
  * the solved profile is the stored one again;
  * `editable:false` ("unconstrained"), `constraint_edit` still available;
  * the allowlist holds against the source.
* **Guards:**
  * exit 7 with stdout closed, still published;
  * stale version ("changed");
  * source as output ("different files");
  * occupied output ("exists");
  * the job cancelled at ≥ 0.9, after the solved copy was checked: no copy
    and no scratch;
  * a publication race: the other writer's bytes are kept.

**`native_sector_constraints_measure_caps_names_angle_edit_and_exports`**
The profile is the stepped
`[[2.75,-3.25],[7.5,-3.25],[7.5,2.125],[4.25,9.5],[2.75,9.5]]` at 137.5°,
with seven saved names (5 faces + 2 caps).
* **Rigid** (H/V on 4 Lines, the pin, four lengths):
  * DOF 0;
  * the allowlist holds;
  * refs and the Revolve payload are the source's;
  * solved = stored;
  * cache Miss.
* **Replaced wall height** (5.375 → 7), which also moves the cone above
  it:
  * DOF 0, solved
    `[[2.75,-3.25],[7.5,-3.25],[7.5,3.75],[4.25,9.5],[2.75,9.5]]`;
  * volume equals this file's Pappus × 137.5/360;
  * cache Miss, then Hit;
  * the angle is kept.
* **Caps.** Each cap lies on its plane, faces out and covers the solved
  profile's area. Each face of revolution runs exactly from 0° to 137.5°.
* **Exports.** An independent STL check through T-junctions only, and a
  complete FBX.
* **Angle edit to 212.25°.** The Sketch payload is byte-identical,
  constraints included, and the refs are the source's. The solved profile
  is unchanged at DOF 0, measured at the new angle.
* **Conflict:** EqualLength(0, 3).
* **Removal of the new wall height:** DOF 1. The solved profile is the
  stored one, and the volume equals the rigid copy's.

**`revolve_constraint_widgets_name_the_revolve_build_and_replace_a_length`**
(headless egui, not a window)
* **The window.** It reads exactly "Profile of Revolve \<uuid\>: 137.5°
  about the sketch Y axis. The turn and the axis are kept; the solved
  profile must keep its bore clear of the axis." `height_mm` is `None`.
* **The widgets build the rigid nine-addition request:**
  * Segment picks;
  * Add Horizontal/Vertical;
  * Pin Start with the Fixed X/Y fields;
  * lengths.

  Each addition is one checkpoint. Undo and Redo work, and a refused
  second H keeps the whole draft. Save produces exactly that request with
  the source, version and Sketch.
* **Where there is OCCT and no solver** (this container), the real worker
  refuses naming planegcs. The draft and history are unchanged, and no copy
  is written.
* **Replace length.** On a document whose lengths were written without a
  solver, it is one checkpoint: remove the exact wall UUID, add 7 mm.

**`native_revolve_constraint_worker_and_cli_publish_the_same_solved_sector`**
* **Worker.** The widgets' rigid request goes through the real worker:
  * an occupied destination refuses and keeps the draft and history;
  * then it publishes.
* **Peer CLI.** It publishes the same additions from the same source.
  Across the two copies, every SQL table other than `objects` is equal cell
  for cell, rowids included; `meta` and the refs are equal. Every
  non-Sketch object is equal, and the Sketch has the same curves and
  rules. Only the new constraint UUIDs differ; kept UUIDs are equal.
* **Exports.** STL and FBX bytes are equal.
* **Replace.** The UI copy is reopened through the async Open route and
  its wall height replaced through Replace length. The same comparison
  holds against the peer CLI replacement. The solved profiles are
  `SECTOR_P`, then `SECTOR_TALL`, at DOF 0, and every ref resolves.

## Local results (OCCT 8.0.1, no PlaneGCS)

* **Five crates.** `cargo test --no-fail-fast -p ferritecad-document -p
  ferritecad-jobs -p ferritecad-eval -p ferritecad-cli -p ferritecad-app`
  gives 1014 passed, 1 ignored and 19 failed. The 19 failures are the
  known N/A set:
  * 18 PlaneGCS suites, which refuse to skip with
    `FERRITECAD_REQUIRE_OCCT=1`;
  * `validate::validation_really_read_only_permissions` (root).

  The app test `the_complex_assembly_…` ran for several minutes in debug
  and passed.
* **New tests.** The four non-solver tests pass; the two solver tests print
  `skipped:` here, by design.
* **Lint.** `cargo fmt --all -- --check` and `cargo clippy --workspace
  --all-targets --all-features -- -D warnings` are clean.
* **Recipe.** It was extracted from the Markdown and run with the debug
  CLI. It printed `FCAD_27G_RECIPE_NO_SOLVER "unsupported: this sketch
  carries 11 constraint(s) and this build did not link planegcs…"` (exit
  0) after the discovery and the three shape refusals.
  `FCAD_EXPECT_SOLVER=1` turns that into exit 1.

### Mutations — local, executed, restored byte for byte

| Mutation | Caught by (executed) |
| --- | --- |
| M1 — `sketch_edit::constraint_frame` accepts axis-closed Revolve profiles | `revolve_constraint_discovery_writer_and_refusals_without_solver`: the v2/v4 row became `available: true` |
| M2 — `supported_family` skips the stored-input policy for a Revolve | the same test: the axis-touching constrained profile (full closure) became `available: true` |

After each restore, the `constraints::` CLI tests (4/4), the app revolve
tests (2/2) and `edit_constraints` (3 passed; the 6 known PlaneGCS
failures) were rerun.

**Not executed.** The mutations "turn the stored rather than the solved
profile" and "cache key ignores the solved profile" need a solver and were
not executed here. The job's solved-profile check for a Revolve is
shadowed by the evaluator, which applies the same `stated_revolution` to
the solved Lines first, so a mutation removing only the job check would be
equivalent for the Revolve refusals. The CI tests above would catch the
two others: exact solved coordinates, and a Miss through a shared store.

### The prior-main CLI (5caaa1c) on constrained Revolve documents — local

The debug CLI was built in the worktree `/home/user/base` at `5caaa1c`
with `CARGO_TARGET_DIR=/home/user/base-target`. The documents were the two
the no-solver test writes through the shared writer
(`FCAD_REVOLVE_CONSTRAINT_FIXTURES`): a full turn with closure + H, and a
sector with an H.

| | prior main 5caaa1c | this head |
| --- | --- | --- |
| `validate --json` | ok, 0 errors | ok, 0 errors |
| `constraint_edit` | refused: "requires one forward Blind Extrude/NewBody" | full turn available; sector refused (missing closure) |
| coordinate edit | refused: "unconstrained Line segments" | same |
| `revolves[].profile` | refused: "carries constraints" | available |
| angle edit (sector) | refused: "carries constraints" | available |
| `export-stl` | exit 2, `unsupported`: "carries N constraint(s) … did not link planegcs" | the same error |

The prior reader therefore reads and validates these documents and gives
the honest read-only answer for every editor. Its rebuild reaches the same
solver path. The evaluator, kernel, bridge, topology and solver are
unchanged between the two (the diff is empty). **Not executed here:** the
prior reader with PlaneGCS actually solving a constrained Revolve. That is
left to the reviewer; the fixture generator below makes the documents.

## CI on `f12694b` (runtime layout, exact name, no skip)

* **Ordinary CI** ([run 36262366412](https://github.com/gesriot/ferrite-cad/actions/runs/36262366412)):
  lint, sbom, notices, supply-chain and test on Linux, macOS and Windows
  are green. That includes the new step "Discover Revolve profile
  constraints without native geometry".
* **Native runtime and packaging** ([run 36262363899](https://github.com/gesriot/ferrite-cad/actions/runs/36262363899)):
  * Linux ([job](https://github.com/gesriot/ferrite-cad/actions/runs/36262363899/job/108460495997))
    and macOS ([job](https://github.com/gesriot/ferrite-cad/actions/runs/36262363899/job/108460495884))
    are green.
  * **What the tool returned.** It gives only the last 5000 lines of each
    job log, and the direct blob download is blocked, so only that tail
    was read here.
  * **In that tail, on both OSes:**
    * the five new exact-name gates of the Revolve step `... ok`;
    * `FCAD_REVOLVE_CONSTRAINT_UFBX_EXECUTED`;
    * no `FAILED` and no panic;
    * the recipe line
      `FCAD_27G_RECIPE_OK {"bushing_rigid_mm3": 4406.983, "bushing_wide_mm3": 5737.758, "bushing_wide_outer_r_mm": 11.75, "sector_tall_mm3": 598.535, "sector_turned_mm3": 923.991}`.

      These are the STL volumes at 0.05 mm, below the exact 4416.637,
      5749.115, 600.982 and 927.697 mm³ by less than the chord band.
  * **Outside that tail, and not read here:** the OCCT-without-solver
    gate. The failing-step semantics prove it passed, since each gate
    exits 1 on a missing `ok` or a `skipped:`.
  * The complete logs are for the reviewer.
* **Earlier heads.** On `32b8ddb` and `0f3de70` the Linux runtime job
  failed:
  * first, the collapsed-Line refusal text (a test expectation);
  * then, diagnostic output splitting the gate's result line.

  Both were fixed in the test only; product code did not change.

## Limits

* No solver test runs in this container. The evidence that solves is CI's.
* The cone-sector mesh keeps the documented T-junctions, and the STL check
  still allows only T-junctions. No strict manifold is claimed.
* Out of scope:
  * axis-closed profiles, Circles/Arcs, new axes, booleans and multibody;
  * live solving in the draft;
  * in-place Save.
* No window, GPU or browser test was run here. The macOS window smoke is
  the reviewer's. The historical OOM is not declared fixed.

## macOS fixtures and window scenario (for the reviewer's Mac)

Nothing here was run in this cloud container: it has no window system and
no PlaneGCS. The generator uses only the staged CLI.

### Fixture generator

It writes to `$FCAD_27G_DIR`:
* `sector.fcad`: 137.5°, the fractional stepped profile with a bore;
* `bushing.fcad`: a full turn;
* the peer CLI copies of exactly what the window scenario builds:
  * `peer-rigid.fcad`: nine additions in widget order;
  * `peer-tall.fcad`: the wall height replaced by exact UUID;
* their STL and FBX exports.

It also checks that `gui-rigid.fcad` and `gui-tall.fcad`, when present, are
the same model as their peers. Only new constraint UUIDs may differ. STL and
FBX must be byte-identical.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/revolve-profile-constraints-verification.md").read_text(encoding="utf-8")
code = text.split("# FCAD_27G_MAC_FIXTURES\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27g-fixtures.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/staged/ferritecad FCAD_27G_DIR=/tmp/fcad-27g python3 ferrite-27g-fixtures.py
# … run the window scenario, saving gui-rigid.fcad and gui-tall.fcad there …
FERRITECAD=/path/to/staged/ferritecad FCAD_27G_DIR=/tmp/fcad-27g python3 ferrite-27g-fixtures.py compare
```

```python
# FCAD_27G_MAC_FIXTURES
import json, os, pathlib, sqlite3, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(os.environ["FCAD_27G_DIR"])
out.mkdir(parents=True, exist_ok=True)

def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run("inspect", path, "--json")["result"]

def constrain(source, edits, target):
    catalog = inspect(source)
    request = target.with_suffix(".json")
    request.write_text(json.dumps({"request_version": 1, **edits}))
    return run("edit-sketch-constraints-copy", source, "--sketch",
               catalog["sketches"][0]["sketch_id"], "--expect-version",
               catalog["content_version"], "--request", request, "-o", target, "--json")

def exports(path):
    for op, suffix in (("export-stl", ".stl"), ("export-fbx", ".fbx")):
        run(op, path, "-o", path.with_suffix(suffix), "--json")

def model(path):
    """Every SQL cell, and the Sketch row as discovery reports it."""
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    cells = {t: sorted(db.execute(f'SELECT * FROM "{t}"').fetchall(), key=repr)
             for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    db.close()
    catalog = inspect(path)["sketches"][0]
    return cells, catalog

if sys.argv[1:] != ["compare"]:
    for name, body in (
        ("sector", {"request_version": 2, "points_mm": [[2.75, -3.25], [7.5, -3.25],
                    [7.5, 2.125], [4.25, 9.5], [2.75, 9.5]], "axis": "sketch_y",
                    "extent": {"kind": "angle", "degrees": 137.5}}),
        ("bushing", {"request_version": 1, "points_mm": [[4.25, -1.5], [10.5, -1.5],
                     [10.5, 13.75], [4.25, 13.75]], "axis": "sketch_y", "angle": "full_turn"})):
        request = out / f"{name}-create.json"
        request.write_text(json.dumps(body))
        target = out / f"{name}.fcad"
        if not target.exists():
            run("create-sketch-revolve", request, "-o", target, "--json")
    sector = out / "sector.fcad"
    curves = [c["curve_id"] for c in
              inspect(sector)["sketches"][0]["constraint_edit"]["curves"]]
    line = lambda i, rule, **k: {"curve_id": curves[i], "rule": rule, **k}
    rigid = [line(0, "horizontal"), line(1, "vertical"), line(3, "horizontal"),
             line(4, "vertical"), line(0, "fixed", at="start", x_mm=2.75, y_mm=-3.25),
             line(0, "distance", distance_mm=4.75), line(1, "distance", distance_mm=5.375),
             line(3, "distance", distance_mm=1.5), line(4, "distance", distance_mm=12.75)]
    peer_rigid = out / "peer-rigid.fcad"
    if not peer_rigid.exists():
        constrain(sector, {"remove": [], "add": rigid}, peer_rigid)
    exports(peer_rigid)
    print("sector and bushing written; peer-rigid published; run the window scenario")
else:
    for gui, peer in (("gui-rigid", "peer-rigid"), ("gui-tall", "peer-tall")):
        gui, peer = out / f"{gui}.fcad", out / f"{peer}.fcad"
        if peer.stem == "peer-tall" and not peer.exists():
            # The same replacement, from the GUI's own rigid copy, so the
            # removed UUID is the one the window removed.
            source = out / "gui-rigid.fcad"
            catalog = inspect(source)["sketches"][0]["constraint_edit"]
            wall = next(c["constraint_id"] for c in catalog["constraints"]
                        if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == 5.375)
            constrain(source, {"remove": [wall], "add": [{"curve_id": catalog["curves"][1]["curve_id"],
                               "rule": "distance", "distance_mm": 7.0}]}, peer)
            exports(peer)
        exports(gui)
        (a, x), (b, y) = model(gui), model(peer)
        assert a.keys() == b.keys()
        for table in a:
            if table != "objects":
                assert a[table] == b[table], table
        assert x["constraint_edit"]["curves"] == y["constraint_edit"]["curves"], "stored inputs"
        rules = lambda c: [r["rule"] for r in c["constraint_edit"]["constraints"]]
        assert rules(x) == rules(y), "the same rules in the same order"
        for suffix in (".stl", ".fbx"):
            assert gui.with_suffix(suffix).read_bytes() == peer.with_suffix(suffix).read_bytes(), suffix
        print(f"{gui.name} == {peer.name}: SQL outside the Sketch payload, rules, STL and FBX bytes")
```

The compare step builds `peer-tall.fcad` from the GUI's own `gui-rigid.fcad`,
so the removed UUID is the one the window removed. The two rigid copies
differ only in new UUIDs. That shows in the Sketch payload/hash and nowhere
else: every other SQL cell, the ordered rules and the export bytes are
compared exactly.

### Window scenario

Run `ferritecad-viewer` built with `--features planegcs` against the staged
PlaneGCS/OCCT, under the usual watchdog.

1. **Open.** Open `sector.fcad`. In **Saved objects**, choose **Edit
   constraints Profile — …**. The window reads "Profile of Revolve …:
   137.5° about the sketch Y axis. The turn and the axis are kept; the
   solved profile must keep its bore clear of the axis." No height is shown.
2. **Dimension.** Add, in this order:
   * **Segment 1** → **Add Horizontal**;
   * **Segment 2** → **Add Vertical**;
   * **Segment 4** → **Add Horizontal**;
   * **Segment 5** → **Add Vertical**;
   * **Segment 1** → **Pin Start**, Fixed X `2.75`, Fixed Y `-3.25` →
     **Add Fixed point**;
   * lengths, each with its own **Add length**: Segment 1 `4.75`,
     Segment 2 `5.375`, Segment 4 `1.5`, Segment 5 `12.75`.

   The pending list shows nine entries.
3. **Undo/Redo.** **Undo** removes the last length; **Redo** restores it.
4. **Refusal.** **Segment 1** → **Add Horizontal** again is refused ("only
   one H/V"). The pending list and Undo are unchanged.
5. **Save Cancel.** **Save constraints copy…** → **Cancel** in the file
   dialog. The draft and its history stay.
6. **Save and Open.** **Save constraints copy…** → `gui-rigid.fcad`. The
   viewer opens the copy asynchronously. The Body looks unchanged, because
   the rigid dimensions equal the stored coordinates.
7. **Replace length.** On `gui-rigid.fcad`, **Edit constraints** →
   **Segment 2**, `7` → **Replace length**. The pending list shows one
   Remove and one length. **Save constraints copy…** → `gui-tall.fcad`. The
   outer wall grows by 1.625 mm and the cone above it moves up.
8. **Refusal and draft recovery.** On `gui-tall.fcad`, **Edit
   constraints**:
   * **Remove** on the Fixed row;
   * **Segment 1** → **Pin Start**, X `-1`, Y `-3.25` → **Add Fixed point**;
   * **Save constraints copy…** → `gui-cross.fcad`.

   The save is refused with the axis reason, nothing is written and the
   draft is still open. **Cancel constraints draft**.
9. **Exports.** Export **STL** and **FBX** of `gui-tall.fcad` into
   `$FCAD_27G_DIR` as `gui-tall.stl` and `gui-tall.fbx`, and the same for
   `gui-rigid`.
10. **Compare.** Quit the viewer normally, then run the generator's
    `compare` step.
