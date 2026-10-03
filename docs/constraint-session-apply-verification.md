# §30C — constraints of a saved Sketch inside the open document: verification

[The contract](constraint-session-apply.md). Marker `FCAD_30C_CONSTRAINT_SESSION_APPLY`.
Base `6df2e0a8789d959022696f85dc5bbe2f6fd80aa2` (merge of PR #82, tree
`cd8e472db25b71e485851860cc41e7530da7a20d`, equal to the verified head of #82). The
post-merge CI of the base is a separate run and is not claimed here. Branch
`constraint-session-apply`. This record names no CI of its own head.

## What changed

* `ferritecad-jobs`: `StepTicket::edit_sketch_constraints` — the reused
  `edit_sketch_constraints_copy` as a session step; refuses a request whose version is
  not the session's current one.
* `ferritecad-app`: `sessions::spawn_apply_constraints`; `constraints::Editor` gains
  **Apply constraints** (and withholds *Save constraints copy…* with words while the
  document has unsaved changes); the window computes `can_apply_constraints` (one
  predicate for the button and the command), opens the form with unsaved changes, and
  starts the worker through the same `begin_apply` as the other Applies.
  `sketch::Editor::dismiss` no longer forgets the constraints editor's session flags.
* No change to the CLI, `edit-sketch-constraints-copy`, any JSON, the document format,
  geometry, the kernel, the solver policy, the evaluator or the renderer.

## Tests

* `ferritecad-jobs` (real PlaneGCS where the sketch is rebuilt): a constraint edit is
  a session step beside a height step and reaches the command line's model
  (`model_version` equal), Undo/Redo over both kinds by UUID, a new branch drops the
  Redo; a form made from an older version is refused with no file left, an empty
  request and an absent UUID are refused whole.
* `ferritecad-app` headless, through the window's production predicate and the form's
  real widgets (`constraints::tests::session_apply`): the open form applies and keeps its
  draft; an Open in flight, a running session operation, the height form, no form and
  no session each block the command; an empty draft offers no Apply and keeps the
  draft's Redo; with unsaved changes the copy is withheld and explained but Apply is
  not; the form opens on a dirty document only when the window says so; a new accepted
  version ends the draft and the next form still has what the window said.
* Native (Open CASCADE and PlaneGCS), the window's real workers and the form's real
  widgets against the real peer CLI process: height → constraints → unsaved export →
  Undo → Undo → Redo → Redo → Save → cold rebuild (every saved name resolves) → refused
  edit that does not cut the Redo → new branch, on a dimensioned offset plate under a
  Chamfer (**Replace length**: exact remove of the stored UUID and add) and on a circle
  (**Radius** and **Fixed centre**). STL/FBX bytes equal the CLI's at every step. `Save`
  equals the CLI's second copy cell for cell except the Sketch's own row; there, every
  rule is equal, each old UUID is unchanged, and each new one is paired with the CLI's
  at the same place (the pairing is asserted to be exactly the added constraints).
  Undo restores the old UUIDs, Redo returns the very same new ones.
* Refusals (native): a horizontal Line forced parallel to a vertical one (a real solver
  conflict, past the form) and a width the Chamfer does not fit are refused by the
  worker with the session's state, history, Redo and files unchanged; the form keeps
  its draft.
* Mixed (Open CASCADE, no solver): the apply of a new constraint is refused, nothing
  becomes dirty, no file changes.
* The old `edit-sketch-constraints-copy` routes and their gates are unchanged and still
  run.

## Mutations — local, executed, restored

| Mutation | Result |
|---|---|
| **M30C-1** `Creates::can_apply_constraints` uses `!self.busy()` (the form blocks its own Apply) | 4 of 6 `session_apply` tests fail on executed assertions (`the form's own Apply must not be blocked by the form`, `nothing else running…`, `unsaved…`, `new accepted version…`) |
| **M30C-2** `finish_session_change` no longer ends the constraints draft (a stale draft survives the new version) | `a_new_accepted_version_ends_the_draft_and_keeps_what_the_window_said` (`the draft outlived the picture it described`) and both native gates (`the form outlived the picture it described`) fail |

Both mutations compiled. The sources were restored (`git diff` shows only the intended
change).

## Local results

Container: Linux x86_64, Open CASCADE 8.0.1 installed locally, a **local** PlaneGCS (not the
pinned one), root user, debug profile, one cargo job at a time. `macOS arm64`, Windows and
`--release` were not run here.

* `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  the export-boundary, solver-ownership and licence-header checks pass.
* The workflow steps were extracted from the workflow files and executed as packed
  (`set -e`, debug instead of `--release`):
  * *Open, edit, Undo, Redo and Save one document without native geometry* (`ci.yml`) in a
    **real stub build** (no Open CASCADE: `OpenCASCADE_DIR-NOTFOUND` in the CMake cache and
    every `REQUIRE_*` unset; its own target directory): 72 gates `ok`, `exit 0`, no
    `skipped:`, including the six new `session_apply` gates.
  * *Edit, Undo, Redo, export and Save one open document through the native window state*
    (`runtime-layout.yml`): nine native gates and the two constraint-step job gates `ok`,
    `exit 0`, against Open CASCADE and the local PlaneGCS.
  * the Open CASCADE-**without**-solver tail (mixed): the §30A refusal, the new constraint
    refusal, the §30A plain plate and the §30B polygon, `ok`.
* Affected packages: `ferritecad-document` (154 + 110 in integration targets),
  `ferritecad-jobs` (88 across targets), `ferritecad-ui` (104), `ferritecad-app` (430 + 5,
  including the native session gates), `ferritecad-cli --test edit_constraints --test
  edit_sketch` (14), all passed.
* Not run here: Windows, macOS, `--release`, the pinned PlaneGCS, any window.

## macOS fixture and window scenario (for Codex on the Mac)

Nothing here was run in a window; the container has none, and headless widget tests are
not a window check. The generator and the comparator use only the CLI. They were
exercised with a script standing in for the window (CLI calls and file copies), which
says nothing about the window: `FCAD_30C_GUI_COMPARE_OK cells=25 triangles=16`. Controls on
stand-ins: a missing file (`FCAD_30C_GUI_COMPARE_MISSING …`), an Apply that wrote the user's
file, a Save As that changed the original, the branch file in place of the saved one, the
source in place of the saved one, an export of the wrong step, a circle file in place of
the branch. Inside the comparator, six further controls must reject (or it stops with
`FCAD_30C_GUI_COMPARE_CONTROL_PASSED`).

Use the bundled CLI and a fresh arm64 bundle, no `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/constraint-session-apply-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_30C_GUI_FIXTURE\n", "ferrite-30c-fixture.py"),
                   ("# FCAD_30C_GUI_COMPARE\n", "ferrite-30c-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_30C_DIR="$PWD/constraint-session-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-30c-fixture.py "$FCAD_30C_DIR"
```

```python
# FCAD_30C_GUI_FIXTURE
import hashlib, json, os, pathlib, shutil, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "constraint-session-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
(out / "source").mkdir()
(out / "work").mkdir()
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# plate.fcad: offset, fractional, 37.5 x 12.25 mm, 6.75 mm tall, one equal-distance
# Chamfer (2.375 mm) at V1, every Line horizontal or vertical, V0 pinned, a width
# (Segment 2, 37.5 mm) and a depth (Segment 1, 12.25 mm).
# circle.fcad: one analytic circle, centre (12.5, -7.25), radius 10.5, 15.25 mm tall.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
H0, DIST = 6.75, 2.375
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H0, "points_mm": PLATE}))
base = out / "base.fcad"
run("create-sketch-extrude", request, "-o", base, "--json")
catalog = run("inspect", base, "--json")["result"]
candidate = next(c for c in catalog["bodies"][0]["chamfer_edge"]["target"]["candidates"]
                 if c["corner_mm"] == PLATE[1])
request.write_text(json.dumps({"request_version": 1, "edge": candidate["edge"], "distance_mm": DIST}))
chamfered = out / "chamfered.fcad"
run("chamfer-edge-copy", base, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", chamfered, "--json")
c = run("inspect", chamfered, "--json")["result"]
(sketch,) = c["sketches"]
edit = sketch["constraint_edit"]
lines = [v["curve_id"] for v in edit["curves"]]
starts = [v["start_mm"] for v in edit["curves"]]
assert starts == PLATE and edit["available"] is True
across = [starts[i][1] == starts[(i + 1) % 4][1] for i in range(4)]
assert across == [False, True, False, True]
add = [{"curve_id": lines[i], "rule": "horizontal" if across[i] else "vertical"} for i in range(4)]
add.append({"curve_id": lines[0], "rule": "fixed", "at": "start", "x_mm": PLATE[0][0], "y_mm": PLATE[0][1]})
add.append({"curve_id": lines[1], "rule": "distance", "distance_mm": 37.5})
add.append({"curve_id": lines[0], "rule": "distance", "distance_mm": 12.25})
request.write_text(json.dumps({"request_version": 1, "remove": [], "add": add}))
plate = out / "source" / "plate.fcad"
run("edit-sketch-constraints-copy", chamfered, "--sketch", sketch["sketch_id"], "--expect-version",
    c["content_version"], "--request", request, "-o", plate, "--json")
request.write_text(json.dumps({"schema_version": 1, "center_mm": [12.5, -7.25], "radius_mm": 10.5, "height_mm": 15.25}))
circle = out / "source" / "circle.fcad"
run("create-circle-extrude", request, "-o", circle, "--json")
for temporary in (request, base, chamfered):
    temporary.unlink()
facts = {}
for name in ("plate", "circle"):
    path = out / "source" / f"{name}.fcad"
    r = run("inspect", path, "--json")["result"]
    (feature,) = r["features"]
    (sk,) = r["sketches"]
    ce = sk["constraint_edit"]
    assert ce["available"] is True
    facts[name] = {"feature_id": feature["feature_id"], "sketch_id": sk["sketch_id"],
                   "height_mm": feature["distance_mm"], "content_version": r["content_version"],
                   "curves": [v["curve_id"] for v in ce["curves"]] or [v["curve_id"] for v in ce["circles"]],
                   "constraint_ids": [k["constraint_id"] for k in ce["constraints"]],
                   "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    shutil.copyfile(path, out / "work" / f"{name}.fcad")
width = next(k for k in run("inspect", plate, "--json")["result"]["sketches"][0]["constraint_edit"]["constraints"]
             if k["rule"]["kind"] == "distance" and k["rule"].get("distance") == 37.5)
facts["plate"]["width_constraint"] = width["constraint_id"]
facts["numbers"] = {"heights_mm": {"first": 8.5, "branch": 10.25}, "new_width_mm": 33.25, "old_width_mm": 37.5,
                    "radius_mm": 6.75, "centre_mm": [-3.5, 4.25], "chamfer_mm": DIST,
                    "plate": {"y0": 3.25, "depth": 12.25, "right": 33.0}}
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_30C_GUI_FIXTURE_OK", out)
```

### Window scenario

`source/` is the comparator's reference: never open it in the window. Open only files in
`work/`. `cp` below is a shell copy made at that moment. `plate.fcad` is the offset 37.5 x
12.25 mm plate, 6.75 mm tall, with one 2.375 mm Chamfer, every Line horizontal or vertical,
V0 pinned, a width on *Segment 2* (37.5 mm) and a depth on *Segment 1* (12.25 mm).
`circle.fcad` is one circle, centre (12.5, -7.25), radius 10.5 mm, 15.25 mm tall.

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_30C_DIR/../watch-30c.jsonl" --limit-mib 1536 --seconds 2400 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

One viewer per watchdog log. If system memory pressure stops a run it does not count:
start again from a fresh fixture directory. **After Quit read only the owned PID's exit
and the watchdog log** (no `getApp`, no `getAXState`, no restart).

1. **Open** `work/plate.fcad`. *Edit extrusion…*, height `8.5`, **Apply**: no dialog,
   `*plate.fcad` in the title.
2. *Edit constraints…* is enabled although the document is unsaved. Select *Segment 2*
   (it shows `Stored length 37.5 mm`), type `33.25`, **Replace length**; try the draft's
   *Undo* and *Redo* once each, then Replace again. **Apply constraints**: no dialog,
   the plate narrows to 33.25 mm with its right edge where it was, the form closes.
   *Save constraints copy…* was withheld with its explanation while the form was open.
   `cp work/plate.fcad gui-plate-after-apply.fcad`. Export STL → `gui-plate-unsaved.stl`,
   FBX → `gui-plate-unsaved.fbx`.
3. With a form open, `Cmd+Z` must not change the document and the toolbar Undo is
   disabled; leave it. `Cmd+Z` once: the plate is 37.5 mm wide again (height 8.5 kept).
   Export STL → `gui-plate-undo1.stl`. `Cmd+Z` again: 6.75 mm and clean;
   `Cmd+Shift+Z` twice returns to 33.25 mm.
4. **Save** (`Cmd+S`): no dialog. `cp work/plate.fcad gui-plate-saved.fcad`.
5. `Cmd+Z` once (the width step undone, dirty), *Edit extrusion…* height `10.25`,
   **Apply**: Redo is gone. **Save As** `work/plate-b.fcad`;
   `cp work/plate-b.fcad gui-plate-branch.fcad`;
   `cp work/plate.fcad gui-plate-after-saveas.fcad`.
6. **Open** `work/circle.fcad`. *Edit constraints…*, select the circle, Radius `6.75`, **Add radius**; Fixed X `-3.5`, Fixed Y `4.25`,
   **Add Fixed centre**; **Apply constraints**. `cp work/circle.fcad gui-circle-after-apply.fcad`.
   Export STL → `gui-circle-unsaved.stl`. **Save** (`Cmd+S`);
   `cp work/circle.fcad gui-circle-saved.fcad`.
7. Optionally, with unsaved changes, `Cmd+Q` → Cancel keeps the dirty document; `Cmd+Q` →
   Discard exits. On the active Russian layout the Command chords must behave as on the US
   one. Quit; read the PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-30c-compare.py "$FCAD_30C_DIR"
```

It requires every `gui-*` file (and never creates one), checks the two sources by SHA-256,
then: the user's file after Apply is byte-identical to its source, and after Save As to the
earlier Save; what was saved has every SQL cell of the peer CLI result (only
`meta.modified_at` and the Sketch's own row set aside), and the Sketch's constraints agree
rule for rule with exactly the added constraints carrying different UUIDs (an old
UUID unchanged, the replaced one gone, the pairing exact); the branch after Undo has
all the original UUIDs; the exports are byte-equal to `export-stl`/`export-fbx` of the
peer result and each STL is read here — closed, one winding, the bounds (the right
edge stays at x = 33) and the exact volume `(W·D − d²/2)·h` for the plate and the
inscribed-cylinder bound for the circle. It then runs its negative controls and prints
`FCAD_30C_GUI_COMPARE_OK cells=N triangles=N`. The pinned ufbx reader is a separate step,
as before.

```python
# FCAD_30C_GUI_COMPARE
import hashlib, json, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-plate-after-apply.fcad", "gui-plate-unsaved.stl", "gui-plate-unsaved.fbx",
       "gui-plate-undo1.stl", "gui-plate-saved.fcad", "gui-plate-after-saveas.fcad",
       "gui-plate-branch.fcad", "gui-circle-after-apply.fcad", "gui-circle-unsaved.stl",
       "gui-circle-saved.fcad")
def require(directory):
    missing = [n for n in GUI if not (directory / n).is_file()]
    assert not missing, f"FCAD_30C_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first"
try:
    require(out)
except AssertionError as error:
    sys.exit(str(error))
S = out / "source"
for name in ("plate", "circle"):
    assert hashlib.sha256((S / f"{name}.fcad").read_bytes()).hexdigest() == facts[name]["sha256"], \
        f"source/{name}.fcad changed"
work = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-30c-compare-"))
N, P, C = facts["numbers"], facts["plate"], facts["circle"]
def run(*args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout
def tables(path):
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    got = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        got[t] = ([d[0] for d in cur.description], sorted(cur.fetchall(), key=repr))
    db.close()
    return got
def cells(path, skip_object=None):
    """Every cell of every table, the modification stamp aside, and (when given) one
    object's whole row: the Sketch's, whose constraint UUIDs are new in a new run."""
    skip = bytes.fromhex(skip_object.replace("-", "")) if skip_object else None
    got = {}
    for t, (columns, rows) in tables(path).items():
        stamp = columns.index("modified_at") if t == "meta" and "modified_at" in columns else None
        if t == "objects" and skip is not None:
            k = columns.index("id")
            kept = [r for r in rows if r[k] != skip]
            assert len(kept) + 1 == len(rows), "the Sketch row is not there"
            rows = kept
        got[t] = (columns, sorted(([None if i == stamp else v for i, v in enumerate(r)] for r in rows), key=repr))
    return got
def version(path):
    return run("inspect", path, "--json")["result"]["content_version"]
def constraints(path):
    (sk,) = run("inspect", path, "--json")["result"]["sketches"]
    return [(k["constraint_id"], k["rule"]) for k in sk["constraint_edit"]["constraints"]]
def same_model(a, b, sketch, before, why):
    """Every cell but the stamp and the Sketch's own row is equal; the Sketch's
    constraints are equal rule for rule, an old constraint keeps its UUID, and the
    new ones are exactly the ones that are not old, each paired with the other
    document's at the same place. Returns that pairing."""
    x, y = cells(a, sketch), cells(b, sketch)
    for t in sorted(set(x) | set(y)):
        assert x.get(t) == y.get(t), f"{why}: {t} differs ({a.name} vs {b.name})"
    ca, cb = constraints(a), constraints(b)
    assert len(ca) == len(cb), f"{why}: {len(ca)} constraints against {len(cb)}"
    pairs = []
    for (ia, ra), (ib, rb) in zip(ca, cb):
        assert ra == rb, f"{why}: a rule differs"
        if ia in before:
            assert ia == ib, f"{why}: an old UUID changed"
        else:
            assert ib not in before, f"{why}: a new UUID is an old one"
            pairs.append((ia, ib))
    return pairs
def edit_height(source, feature, height, dest):
    run("edit-extrude", source, "--feature", feature, "--distance-mm", str(height),
        "--expect-version", version(source), "-o", dest, "--json")
    return dest
def edit_constraints(source, sketch, request, dest):
    file = work / f"{dest.stem}.json"
    file.write_text(json.dumps({"request_version": 1, **request}))
    run("edit-sketch-constraints-copy", source, "--sketch", sketch, "--expect-version", version(source),
        "--request", file, "-o", dest, "--json")
    return dest
def width_request(curve, millimetres):
    return {"remove": [P["width_constraint"]],
            "add": [{"curve_id": curve, "rule": "distance", "distance_mm": millimetres}]}
def circle_request():
    return {"remove": [], "add": [
        {"curve_id": C["curves"][0], "rule": "radius", "radius_mm": N["radius_mm"]},
        {"curve_id": C["curves"][0], "rule": "fixed", "at": "center", "x_mm": N["centre_mm"][0],
         "y_mm": N["centre_mm"][1]}]}
def mesh(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    return [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)] for i in range(count)]
def closed_surface(stl):
    """Read here, independently of the B-Rep: one closed oriented surface; returns the
    triangles, the signed volume and the bounds."""
    tri = mesh(stl)
    key = lambda v: tuple(round(x * 1e4) for x in v)
    directed = {}
    for a, b, c in tri:
        for u, v in ((a, b), (b, c), (c, a)):
            directed[(key(u), key(v))] = directed.get((key(u), key(v)), 0) + 1
    assert all(n == 1 for n in directed.values()), "not one oriented surface"
    assert all((v, u) in directed for u, v in directed), "the mesh is open"
    six = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2])
              + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in tri)
    pts = [q for t in tri for q in t]
    return tri, six / 6, [(min(p[k] for p in pts), max(p[k] for p in pts)) for k in range(3)]
def measure_plate(stl, width, height):
    """The plate keeps its pinned right edge x = 33; bounds and exact volume (W x D - d^2/2) x h."""
    tri, volume, box = closed_surface(stl)
    right, y0, depth = N["plate"]["right"], N["plate"]["y0"], N["plate"]["depth"]
    for (lo, hi), a, b in zip(box, (right - width, y0, 0.0), (right, y0 + depth, height)):
        assert abs(lo - a) < 1e-4 and abs(hi - b) < 1e-4, ("bounds", box)
    exact = (width * depth - N["chamfer_mm"] ** 2 / 2) * height
    assert abs(volume - exact) < 1e-3, ("volume", volume, exact)
    return len(tri)
def measure_circle(stl):
    """A cylinder of the pinned centre and radius: bounds to the tessellation, and a
    volume within what a polygon inscribed in that circle can differ by."""
    import math
    tri, volume, box = closed_surface(stl)
    (cx, cy), r, h = N["centre_mm"], N["radius_mm"], C["height_mm"]
    for (lo, hi), a, b in zip(box, (cx - r, cy - r, 0.0), (cx + r, cy + r, h)):
        assert abs(lo - a) < 0.02 and abs(hi - b) < 0.02, ("bounds", box)
    exact = math.pi * r * r * h
    assert 0.98 * exact < volume <= exact + 1e-3, ("volume", volume, exact)
    return len(tri)
def exported(path, tag):
    stl, fbx = work / f"{tag}.stl", work / f"{tag}.fbx"
    run("export-stl", path, "-o", stl, "--json")
    run("export-fbx", path, "-o", fbx, "--json")
    return stl.read_bytes(), fbx.read_bytes()
def must_fail(label, thunk):
    try:
        thunk()
    except AssertionError:
        return
    sys.exit(f"FCAD_30C_GUI_COMPARE_CONTROL_PASSED {label}: the comparison cannot tell a wrong result")
def read(name):
    return (out / name).read_bytes()
H, W0, W1 = N["heights_mm"], N["old_width_mm"], N["new_width_mm"]
before_p, before_c = set(P["constraint_ids"]), set(C["constraint_ids"])
width_curve = P["curves"][1]

# 1. The user's file is untouched by Apply, Undo, Redo, and Save As leaves the original.
assert read("gui-plate-after-apply.fcad") == (S / "plate.fcad").read_bytes(), "Apply wrote the user's file"
assert read("gui-circle-after-apply.fcad") == (S / "circle.fcad").read_bytes(), "Apply wrote the user's file"
assert read("gui-plate-after-saveas.fcad") == read("gui-plate-saved.fcad"), "Save As changed the original"

# 2. What the window saved is what the command line makes, UUIDs of the new
# constraints aside (the window and the command line each make their own).
h1 = edit_height(S / "plate.fcad", P["feature_id"], H["first"], work / "h1.fcad")
w1 = edit_constraints(h1, P["sketch_id"], width_request(width_curve, W1), work / "w1.fcad")
pairs = same_model(out / "gui-plate-saved.fcad", w1, P["sketch_id"], before_p, "Save")
assert len(pairs) == 1 and P["width_constraint"] not in {i for i, _ in constraints(out / "gui-plate-saved.fcad")}, \
    "the replaced length is still stored or more than one constraint is new"
branch = edit_height(h1, P["feature_id"], H["branch"], work / "branch.fcad")
assert not same_model(out / "gui-plate-branch.fcad", branch, P["sketch_id"], before_p, "branch"), \
    "the branch made a new constraint"
assert [i for i, _ in constraints(out / "gui-plate-branch.fcad")] == P["constraint_ids"], \
    "Undo/Redo or the branch changed the original constraints' UUIDs"
k1 = edit_constraints(S / "circle.fcad", C["sketch_id"], circle_request(), work / "k1.fcad")
cpairs = same_model(out / "gui-circle-saved.fcad", k1, C["sketch_id"], before_c, "circle Save")
assert len(cpairs) == 2
for name in ("gui-plate-saved", "gui-plate-branch", "gui-circle-saved"):
    run("validate", out / f"{name}.fcad", "--json")
    run("inspect", out / f"{name}.fcad", "--json")

# 3. The exports are the accepted model: byte for byte what the command line exports.
stl_w, fbx_w = exported(w1, "plate")
assert read("gui-plate-unsaved.stl") == stl_w and read("gui-plate-unsaved.fbx") == fbx_w, \
    "the unsaved export is not the accepted model"
stl_h, _ = exported(h1, "plate-height")
assert read("gui-plate-undo1.stl") == stl_h, "Undo did not give the height step"
stl_k, _ = exported(k1, "circle")
assert read("gui-circle-unsaved.stl") == stl_k, "the circle's export differs"
measure_plate(out / "gui-plate-unsaved.stl", W1, H["first"])
measure_plate(out / "gui-plate-undo1.stl", W0, H["first"])
measure_circle(out / "gui-circle-unsaved.stl")

# 4. Negative controls: each comparison above must reject the wrong thing.
original_stl, _ = exported(S / "plate.fcad", "original")
def equal(a, b):
    assert a == b, "different bytes"
must_fail("an export of the disk original", lambda: equal(original_stl, read("gui-plate-unsaved.stl")))
wrong = edit_constraints(h1, P["sketch_id"], width_request(width_curve, W1 + 0.25), work / "wrong.fcad")
must_fail("a different length in the save",
          lambda: same_model(out / "gui-plate-saved.fcad", wrong, P["sketch_id"], before_p, "control"))
must_fail("the branch compared with the replaced model",
          lambda: same_model(out / "gui-plate-branch.fcad", w1, P["sketch_id"], before_p, "control"))
must_fail("a new UUID taken for an old one",
          lambda: same_model(out / "gui-plate-saved.fcad", w1, P["sketch_id"], before_p | {pairs[0][0]}, "control"))
must_fail("a missing output", lambda: require(work))
must_fail("an old-width mesh", lambda: measure_plate(work / "original.stl", W1, H["first"]))
print("FCAD_30C_GUI_COMPARE_OK", f"cells={sum(len(r) for _, r in tables(out / 'gui-plate-saved.fcad').values())}",
      f"triangles={len(mesh(out / 'gui-plate-unsaved.stl'))}")
```
