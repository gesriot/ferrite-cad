# §27H — verification record

[Contract and agent recipe](axis-closed-revolve-constraints.md).

This records what was executed for this slice and where. It does not repeat
the earlier slices' evidence.

## Where and how this was run

* **Base.** `origin/main` at `a57dded48f4d16ca14c1c9c81a695dfd00854fd8`,
  the merge of PR #59. Its tree `45c61ad…` equals the reviewed §27G head
  `a3e4237`. The branch is `axis-closed-revolve-constraints`.
* **Cloud container.** Linux x86_64, 4 CPUs, 15 GiB RAM, the session's
  OCCT 8.0.1 and its existing native/stub targets.
* **A local, unpinned PlaneGCS.** The proxy still refuses the pinned
  FreeCAD/Boost archives. So a library was built from the FreeCAD 1.0.1
  `planegcs` sources (fetched file by file), Ubuntu's Eigen 3.4.0 and
  Boost 1.83, through the repository's own `tools/planegcs` CMake project
  and shim. Its provenance string says so: "LOCAL UNPINNED planegcs from
  FreeCAD 1.0.1 raw sources with Ubuntu Eigen 3.4.0 and Boost 1.83 - not
  the pinned delivery".
  * It is linked with `FCAD_PLANEGCS_DIR` and `LD_LIBRARY_PATH` only. Nothing
    in the repository points at it.
  * Every solver result below is from that library. **It is not the pinned
    delivery**; CI's named gates on Linux, macOS and Windows are the
    authoritative solver evidence.
* **No window, GPU test or browser was launched.** The widget tests drive
  real egui widgets headlessly; that is not a window test.

## The solver at the axis — probed before the refusal was widened

`probe.cpp` calls the shim's own session API (`fc_gcs_session_create`,
`diagnose`, `prepare`, `solve`, `state`) the way the evaluator does: two
points per Line, closure Coincidents first, then the additions. Built with:

```sh
g++ -std=c++17 -O2 -w -DFCAD_PLANEGCS_BUILDING -DFCAD_PLANEGCS_PROVENANCE='"probe"' \
  -I App/planegcs -I . -I /usr/include/eigen3 \
  App/planegcs/{Constraints,GCS,Geo,qp_eq,SubSystem}.cpp glue/provenance.cpp \
  planegcs_shim.cpp probe.cpp -o probe
```

Output, axis Line marked `*`, x also in hex:

| Case | status, DOF | solved axis Line |
| --- | --- | --- |
| cylinder, rigid as stored | 0, 0 | x `0x0p+0` both ends |
| cylinder, radius 10 → 12.5 | 0, 0 | x `0x0p+0` both ends |
| cylinder, height 15 → 18.25 | 0, 0 | x `0x0p+0` both ends |
| fractional cylinder 7.25 → 8.5, 15.25 → 16, y −1.5 | 0, 0 | x `0x0p+0` both ends |
| the same, pin on the axis Line's start | 0, 0 | x `0x0p+0`; y −2.2500000000000004 |
| cone 8.5 → 9.75, 13.25 → 14 | 0, 0 | x `0x0p+0` both ends |
| stepped shaft 8 → 9.25 | 0, 0 | x `0x0p+0` both ends |
| cylinder, **no pin** | 0, **2** | x −1.5807611844574883 |
| cylinder, **outer corner pinned only** | 0, 0 | x −2.5 |

Why: planegcs turns H, V and Coincident into parameter equalities and
reduces them in `initSolution`, so a V on the axis Line makes both ends one
x parameter; a Fixed is `CoordinateX/Y` against a constant and is solved.
That is the contract recorded in the
[contract](axis-closed-revolve-constraints.md#the-numerical-contract-at-the-axis):
exact 0.0 is required, nothing is snapped, and the request states the pin
and the V.

## What changed

* **Domain** (`ferritecad-document`): `sketch_edit::constraint_frame` no
  longer refuses a Revolve that states an `axis_segment`. The use it
  returns carries that stated Line, so the stored inputs
  (`constrained_lines`) and the copy job's solved profile are both judged by
  `stated_revolution` against it. That is one removed `if`; nothing else in
  the domain changed.
* **UI** (`ferritecad-app`): the owner line of the existing window names the
  axis Line and says what keeps it on the axis.
* **Unchanged:** evaluator, kernel, OCCT bridge, topology, solver, storage,
  jobs, CLI source, JSON types and capabilities.
  `git diff a57dded -- crates/ferritecad-eval crates/ferritecad-kernel
  crates/ferritecad-occt crates/ferritecad-occt-bridge
  crates/ferritecad-topology crates/ferritecad-sketch-solver
  crates/ferritecad-jobs crates/ferritecad-cli/src` is empty.
* **Tests:** §27G's measure of a constrained Revolve now reads the Revolve's
  `axis_segment`. For an axis-closed part it requires both solved ends of
  that Line at exactly 0.0 and every other vertex beyond the clearance, and
  it expects no face and no name for that Line. §27G's kernel-free test no
  longer asserts the axis-closed refusal; that is now `axis_constraints`.

## New tests and gates

CLI (`crates/ferritecad-cli/tests/revolve/axis_constraints.rs`):

* **`axis_closed_constraint_discovery_writer_and_refusals_without_solver`**
  (any build):
  * v2 and v4 rows are available, with the axis-closed kinds and
    `axis_curve_id` equal to the stated Line and to `revolves[]`;
  * forged documents are refused by the shared class rule: the Revolve
    states a Line that is not on the axis ("the axis Line cannot change"),
    its stated Line was moved to x 0.5 ("no longer touches the axis"), or a
    vertex sits 1e-12 from the axis;
  * the writer, with no kernel or solver, prepares a use that carries the
    stated Line and writes only the allowlisted cells; the Revolve payload
    and the face names are byte-identical;
  * constrained, coordinate editing refuses, the Revolve profile discovery
    and the sector's angle edit stay available;
  * a request with a literal `\u0063urve_id` escaped duplicate (the test
    asserts the backslash-u is in the bytes it writes) and an unknown field
    are refused with nothing written.
* **`occt_without_solver_refuses_axis_closed_constraints_honestly`**: a
  cylinder and a sector are offered and refused with the typed `planegcs`
  error; nothing is written.
* **`native_axis_closed_cylinder_and_cone_dimension_replace_remove_and_refuse`**
  (OCCT + PlaneGCS):
  * cylinder `[[0,-1.5],[7.25,-1.5],[7.25,13.75],[0,13.75]]`, H/V, V and
    Fixed (0, −1.5) on axis Line 4, lengths 7.25 and 15.25: DOF 0, no
    redundancy, solved = stored, volume π·7.25²·15.25 to 1e-7;
  * Replace length 7.25 → 8.5 by exact UUID: other UUIDs kept, stored inputs
    unchanged, Miss then Hit through a shared cache, cold = cached, solved
    `[[0,-1.5],[8.5,-1.5],[8.5,13.75],[0,13.75]]`, π·8.5²·15.25;
  * refused, source untouched, no conflict attached: radius changed with
    the pin removed; only the outer corner pinned; the axis Line pinned at
    x 0.5 (moved away) and x −1 (crossing); a typed `constraint` conflict;
    a collapsed Line;
  * every user constraint removed: 4 closure Coincidents left, DOF 8,
    `editable` false, solved = stored;
  * cone `[[0,-1.5],[8.5,-1.5],[0,13.25]]`: base and height replaced in
    one request, solved `[[0,-1.5],[9.75,-1.5],[0,14]]`, π·9.75²·15.5/3.
* **`native_axis_closed_sector_caps_names_angle_edit_and_exports`**: a
  137.5° stepped shaft
  `[[0,-2.25],[8,-2.25],[8,3.5],[5.5,3.5],[5.5,12.75],[0,12.75]]`, rigid
  (eleven additions), base 8 → 9.25 so the step follows to 6.75. Both caps
  keep their names and are measured in plane, facing out, covering the
  solved profile's area. The angle edit to 212.25° keeps the Sketch
  byte-identical and DOF 0; pinning the axis end at x 0.75 is refused.

Every constrained copy is measured by the shared measure: one solve report,
the solved Lines closed in stored order, the stated Line at exactly 0.0, one
face per other Line on the right surface (plane, cylinder of the solved
radius, cone), n − 1 + caps faces, every name resolving to one face, and
Pappus volume to 1e-8. STL is checked independently against the solved
profile, and FBX is exported and kept as a CI artifact.

App (`crates/ferritecad-app/src/constraints/tests/revolve.rs`):

* **`axis_closed_constraint_widgets_name_the_axis_line_and_keep_the_draft`**
  (no kernel): the owner line text, eleven widget additions in order,
  Undo/Redo, a refused second V, Save → request, Save Cancel keeps the
  draft and its history and Save offers the same request again, the real
  worker's refusal without a solver (in an OCCT build), and Replace length
  as one exact removal plus one addition.
* **`native_axis_closed_worker_and_cli_publish_the_same_solved_shaft`**:
  the widgets' request is published by the real worker, after a refusal
  for an occupied destination that keeps the draft, and by the peer CLI.
  The two copies are compared on every SQL table cell, rowids included;
  in `objects`, only the selected Sketch's payload and hash may differ, and
  there only in the new constraint UUIDs. STL and FBX are byte-identical.
  The UI copy is reopened through async Open, its base replaced the same
  two ways, and the solved axis ends are exactly 0.0.

Gates: the three native CLI tests and the two app tests are in the
runtime-layout Revolve step's exact-name, no-skip lists; the OCCT-without-
solver test is in the no-solver step; the two kernel-free tests are in the
ordinary CI step; the §27H recipe is extracted from the Markdown and run
there with `FCAD_EXPECT_SOLVER=1`; `tools/check-fbx-complex.sh` reads the
three new FBX with pinned ufbx and joins their triangles with the STL, and
the job greps `FCAD_AXIS_CONSTRAINT_UFBX_EXECUTED`.

## Local results

With the local unpinned PlaneGCS, OCCT 8.0.1, debug:

* `cargo test -p ferritecad-cli --features planegcs --test revolve`: 40
  passed, 0 failed, no skip.
* `cargo test -p ferritecad-app --features planegcs --bin ferritecad-viewer
  constraints::tests::revolve`: 4 passed.
* The §27H recipe with `FCAD_EXPECT_SOLVER=1` printed
  `FCAD_27H_RECIPE_OK {"cylinder_rigid_mm3": 2506.773, "cylinder_wide_mm3": 3447.913, "cylinder_wide_r_mm": 8.5, "shaft_wide_mm3": 1091.615, "shaft_wide_r_mm": 9.25, "shaft_turned_mm3": 1685.133}`.
  These are STL volumes at 0.05 mm, below the exact values within the chord
  band the recipe checks.

OCCT without a solver (native env, no `planegcs` feature):

* the §27G and §27H `constraints` CLI tests: 8 passed, of which 4 printed
  `skipped:` (the solver tests: 2 of §27G, 2 of §27H). The two
  OCCT-without-solver tests ran.
* the app revolve tests: 4 passed, of which 2 printed `skipped:` (the
  native worker tests); the new widget test exercised the real worker's
  `planegcs` refusal.

Kernel-free stub build: 8 passed, of which 6 printed `skipped:` (4 solver,
2 OCCT-without-solver). The two discovery/writer tests ran.

REGRESSION_PLACEHOLDER

### Mutations — local, executed, restored byte for byte

Each mutation was applied by a script, compiled, run against the §27H CLI
tests with the local PlaneGCS, and the file restored from `HEAD`; the
restored files' SHA-256 matched the originals
(`sketch_edit.rs` 715b3122…, `cold.rs` b056be8b…).

| Mutation | Caught by (executed) |
| --- | --- |
| M1 — ignore the saved axis identity: `revolve_frame` takes the Line the stored geometry puts on the axis instead of the Revolve's `axis_segment` | `axis_closed_constraint_discovery_writer_and_refusals_without_solver`: the forged document stating another Line became `available: true` with `axis_curve_id` of the geometric axis Line |
| M2 — use stored geometry after a dimension change: the evaluator's Revolve turns the stored profile (constraints cleared) instead of the solved one | both native §27H tests: after the radius/base change the shared cache answered `Hit` where the solved profile must key a `Miss`; the measure of the solved Lines would follow |

After each restore, `--test revolve` (40 passed) and the app revolve tests
(4 passed) were rerun with the solver.

### The prior-main CLI (a57dded) on §27H documents — local

The worktree `/home/user/base` was moved to `a57dded` (detached) and its
debug CLI built with the same local PlaneGCS
(`CARGO_TARGET_DIR=/home/user/base-target`). This head's CLI made four
documents: a solid cylinder and a 137.5° solid shaft sector, and a
constrained copy of each (DOF 0, the pin on the axis).

| | prior main a57dded | this head |
| --- | --- | --- |
| `validate --json` | ok | ok |
| `inspect --json` outside `constraint_edit` | identical | identical |
| `constraint_edit` | refused: "constraint editing supports a Revolve profile with a bore; this profile is closed on the axis along Line …, which is not supported" | available |
| `edit-sketch-constraints-copy` on a constrained copy | exit 2, the same refusal, nothing written | — |
| `rebuild --cold` | exit 0 | exit 0 |
| `export-stl`, `export-fbx` | byte-identical to this head's | — |

So a prior-main reader reads, validates, rebuilds (with a solver) and
exports these documents exactly as this head does, and names its read-only
boundary for the constraint edit. `inspect` being identical outside
`constraint_edit` also shows that no other editor was switched on.
**Limits:** that reader was linked against the same local, unpinned
PlaneGCS; the pinned library is CI's. A reader older than §27C cannot open
axis-closed documents at all (capability `feature.revolve.axis-closed.v1`),
as before.

CI_PLACEHOLDER

## Limits

* The local solver evidence is from an unpinned library. The pinned one is
  exercised only by CI.
* The exact-0.0 contract is a measured property of planegcs's parameter
  aliasing. A platform that returned a non-zero x for a pinned axis end
  would refuse publication rather than publish a wrong part.
* A solution with the pin missing or elsewhere is refused, not repaired; the
  UI does not add the pin for the user.
* The cone-sector mesh keeps the documented T-junctions, and the STL check
  still allows only T-junctions. No strict manifold is claimed.
* Out of scope: Circles/Arcs, new axes, booleans, multibody, live solving in
  the draft, in-place Save.
* No window, GPU or browser test was run here. The historical OOM is not
  declared fixed.

## macOS fixtures and window scenario (for the reviewer's Mac)

Nothing here was run in this cloud container: it has no window system. The
generator uses only the staged CLI.

### Fixture generator

It writes to `$FCAD_27H_DIR`:
* `shaft.fcad`: the 137.5° solid stepped shaft, closed on the axis along
  its sixth Line;
* `cylinder.fcad`: a solid full-turn cylinder;
* the peer CLI copies of exactly what the window scenario builds:
  * `peer-rigid.fcad`: eleven additions in widget order;
  * `peer-wide.fcad`: the base replaced by exact UUID, built in the compare
    step from the window's own `gui-rigid.fcad`;
* the peers' STL and FBX exports.

The compare step checks that `gui-rigid.fcad` and `gui-wide.fcad` are the
same model as their peers. It compares every table, `objects` included:
every cell with rowids, and the whole encoded Sketch payload after mapping
only the newly added constraint UUIDs. It requires `gui-*.stl` and
`gui-*.fbx` exported **by the window**. It never creates, re-exports or
overwrites a GUI file, and a missing one is a failure.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/axis-closed-revolve-constraints-verification.md").read_text(encoding="utf-8")
code = text.split("# FCAD_27H_MAC_FIXTURES\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27h-fixtures.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/staged/ferritecad FCAD_27H_DIR=/tmp/fcad-27h python3 ferrite-27h-fixtures.py
# … run the window scenario, saving and exporting gui-rigid and gui-wide there …
FERRITECAD=/path/to/staged/ferritecad FCAD_27H_DIR=/tmp/fcad-27h python3 ferrite-27h-fixtures.py compare
```

```python
# FCAD_27H_MAC_FIXTURES
import json, os, pathlib, sqlite3, subprocess, sys, uuid
cli = os.environ["FERRITECAD"]
out = pathlib.Path(os.environ["FCAD_27H_DIR"])
out.mkdir(parents=True, exist_ok=True)

def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run("inspect", path, "--json")["result"]

def constrain(source, edits, target):
    assert not target.exists(), f"refusing to overwrite {target}"
    catalog = inspect(source)
    request = target.with_suffix(".json")
    request.write_text(json.dumps({"request_version": 1, **edits}))
    return run("edit-sketch-constraints-copy", source, "--sketch",
               catalog["sketches"][0]["sketch_id"], "--expect-version",
               catalog["content_version"], "--request", request, "-o", target, "--json")

def peer_exports(path):
    assert path.name.startswith("peer-"), "only peer copies are exported here"
    for op, suffix in (("export-stl", ".stl"), ("export-fbx", ".fbx")):
        run(op, path, "-o", path.with_suffix(suffix), "--json")

def model(path):
    """Every SQL cell, including rowids where the table has them."""
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    cells = {}
    for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table'"):
        quoted = table.replace('"', '""')
        try:
            rows = db.execute(f'SELECT rowid,* FROM "{quoted}" ORDER BY rowid')
        except sqlite3.OperationalError:
            rows = db.execute(f'SELECT * FROM "{quoted}"')
        columns = [c[0] for c in rows.description]
        cells[table] = (columns, sorted(rows.fetchall(), key=repr))
    db.close()
    return cells, inspect(path)["sketches"][0]

def compare_sql(gui, peer, source):
    (a, x), (b, y) = model(gui), model(peer)
    assert a.keys() == b.keys()
    for table in a:
        assert a[table][0] == b[table][0], (table, "columns")
        if table != "objects":
            assert a[table] == b[table], table
    assert x["sketch_id"] == y["sketch_id"]
    assert x["constraint_edit"]["profile_feature"] == y["constraint_edit"]["profile_feature"]
    assert x["constraint_edit"]["profile_feature"]["kind"] == "partial_turn_revolve_axis_closed"
    selected = uuid.UUID(x["sketch_id"]).bytes
    original = inspect(source)["sketches"][0]["constraint_edit"]["constraints"]
    kept = {r["constraint_id"] for r in original}
    left = x["constraint_edit"]["constraints"]
    right = y["constraint_edit"]["constraints"]
    assert len(left) == len(right)
    replacements = []
    for u, v in zip(left, right):
        assert u["rule"] == v["rule"], "ordered rules"
        u, v = u["constraint_id"], v["constraint_id"]
        if u in kept or v in kept:
            assert u == v, "preserved constraint UUID"
        else:
            replacements.append((uuid.UUID(v).bytes, uuid.UUID(u).bytes))
    columns, left_rows = a["objects"]
    right_rows = b["objects"][1]
    assert len(left_rows) == len(right_rows), "object count"
    for left, right in zip(left_rows, right_rows):
        left, right = dict(zip(columns, left)), dict(zip(columns, right))
        assert left["id"] == right["id"]
        if left["id"] == selected:
            # UUIDs are stored as CBOR byte strings. Map only the newly added
            # constraint identities, then compare the entire encoded envelope.
            payload = right["payload"]
            assert not {v for v, _ in replacements} & {u for _, u in replacements}
            for old, new in replacements:
                assert payload.count(old) == 1, "one new constraint UUID in payload"
                payload = payload.replace(old, new)
            assert left["payload"] == payload, "whole Sketch payload after UUID mapping"
            for column in columns:
                if column not in ("payload", "payload_hash"):
                    assert left[column] == right[column], ("objects", column)
        else:
            assert left == right, "every non-selected object cell"

if sys.argv[1:] != ["compare"]:
    for name, body in (
        ("shaft", {"request_version": 2, "points_mm": [[0, -2.25], [8, -2.25], [8, 3.5],
                   [5.5, 3.5], [5.5, 12.75], [0, 12.75]], "axis": "sketch_y",
                   "extent": {"kind": "angle", "degrees": 137.5}}),
        ("cylinder", {"request_version": 1, "points_mm": [[0, -1.5], [7.25, -1.5],
                      [7.25, 13.75], [0, 13.75]], "axis": "sketch_y", "angle": "full_turn"})):
        request = out / f"{name}-create.json"
        request.write_text(json.dumps(body))
        target = out / f"{name}.fcad"
        if not target.exists():
            run("create-sketch-revolve", request, "-o", target, "--json")
    shaft = out / "shaft.fcad"
    curves = [c["curve_id"] for c in
              inspect(shaft)["sketches"][0]["constraint_edit"]["curves"]]
    line = lambda i, rule, **k: {"curve_id": curves[i], "rule": rule, **k}
    rigid = [line(i, "horizontal" if i % 2 == 0 else "vertical") for i in range(6)]
    rigid += [line(0, "fixed", at="start", x_mm=0.0, y_mm=-2.25),
              line(0, "distance", distance_mm=8.0), line(1, "distance", distance_mm=5.75),
              line(2, "distance", distance_mm=2.5), line(3, "distance", distance_mm=9.25)]
    peer_rigid = out / "peer-rigid.fcad"
    if not peer_rigid.exists():
        constrain(shaft, {"remove": [], "add": rigid}, peer_rigid)
        peer_exports(peer_rigid)
    print("shaft and cylinder written; peer-rigid published; run the window scenario")
else:
    for gui, peer in (("gui-rigid", "peer-rigid"), ("gui-wide", "peer-wide")):
        gui, peer = out / f"{gui}.fcad", out / f"{peer}.fcad"
        assert gui.is_file(), f"missing GUI copy: {gui}"
        if peer.stem == "peer-wide" and not peer.exists():
            # The same replacement, from the GUI's own rigid copy, so the
            # removed UUID is the one the window removed.
            source = out / "gui-rigid.fcad"
            catalog = inspect(source)["sketches"][0]["constraint_edit"]
            base = next(c["constraint_id"] for c in catalog["constraints"]
                        if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == 8.0)
            constrain(source, {"remove": [base], "add": [{"curve_id": catalog["curves"][0]["curve_id"],
                               "rule": "distance", "distance_mm": 9.25}]}, peer)
            peer_exports(peer)
        source = out / ("shaft.fcad" if peer.stem == "peer-rigid" else "gui-rigid.fcad")
        compare_sql(gui, peer, source)
        for suffix in (".stl", ".fbx"):
            # These must have been produced through the window. Comparison
            # never calls an exporter for a GUI path or repairs missing proof.
            artifact = gui.with_suffix(suffix)
            assert artifact.is_file(), f"missing GUI export: {artifact}"
            assert artifact.read_bytes() == peer.with_suffix(suffix).read_bytes(), suffix
        print(f"{gui.name} == {peer.name}: every SQL cell but new constraint UUIDs, STL and FBX bytes")
```

### Window scenario

Run `ferritecad-viewer` built with `--features planegcs` against the staged
PlaneGCS/OCCT, under the usual watchdog.

1. **Open.** Open `shaft.fcad`. In **Saved objects**, choose **Edit
   constraints Profile — …**. The window reads "Profile of Revolve …:
   137.5° about the sketch Y axis, closed on the axis along Line …. The turn
   and that Line are kept; the solved Line must lie exactly on the axis —
   pin one of its ends at X 0 and keep it vertical." No height is shown.
2. **Dimension.** Add, in this order:
   * **Segment 1** → **Add Horizontal**, **Segment 2** → **Add Vertical**,
     **Segment 3** → **Add Horizontal**, **Segment 4** → **Add Vertical**,
     **Segment 5** → **Add Horizontal**, **Segment 6** → **Add Vertical**;
   * **Segment 1** → **Pin Start**, Fixed X `0`, Fixed Y `-2.25` →
     **Add Fixed point**;
   * lengths, each with its own **Add length**: Segment 1 `8`, Segment 2
     `5.75`, Segment 3 `2.5`, Segment 4 `9.25`.

   The pending list shows eleven entries.
3. **Undo/Redo.** **Undo** removes the last length; **Redo** restores it.
4. **Refusal.** **Segment 6** → **Add Vertical** again is refused. The
   pending list and Undo are unchanged.
5. **Save Cancel.** **Save constraints copy…** → **Cancel** in the file
   dialog. The draft and its history stay.
6. **Save and Open.** **Save constraints copy…** → `gui-rigid.fcad`. The
   viewer opens the copy asynchronously; the Body looks unchanged.
7. **Replace length.** On `gui-rigid.fcad`, **Edit constraints** →
   **Segment 1**, `9.25` → **Replace length**. The pending list shows one
   Remove and one length. **Save constraints copy…** → `gui-wide.fcad`.
   The base widens by 1.25 mm, the upper step follows, the axis stays
   closed and both end faces stay flat.
8. **Refusal and draft recovery.** On `gui-wide.fcad`, **Edit
   constraints** → **Remove** on the Fixed row; **Segment 1** → **Pin
   Start**, X `0.75`, Y `-2.25` → **Add Fixed point**; **Save constraints
   copy…** → `gui-off.fcad`. The save is refused with the axis reason,
   nothing is written and the draft is still open. **Cancel constraints
   draft**.
9. **Exports.** From the window, export **STL** and **FBX** of
   `gui-rigid.fcad` and `gui-wide.fcad` into `$FCAD_27H_DIR` as
   `gui-rigid.stl`, `gui-rigid.fbx`, `gui-wide.stl` and `gui-wide.fbx`.
10. **Compare.** Quit the viewer normally, then run the generator's
    `compare` step.
