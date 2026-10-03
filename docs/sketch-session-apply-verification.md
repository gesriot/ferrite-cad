# §30B — vertices of a saved Line Sketch inside the open document: verification

[The contract](sketch-session-apply.md). Marker `FCAD_30B_SKETCH_SESSION_APPLY`.
Base `915ec1b2a396e6afce96870145a66b3bf6e66bb2` (merge of PR #81, tree
`d9dc6719f0ba98405387c3303bbf2f5517dccd69`, equal to the final head of #81). Its
post-merge CI is a separate run and is not claimed here. Branch
`sketch-session-apply`. This record names no CI of its own head.

## What changed

* `ferritecad-jobs`: `StepTicket::edit_sketch_vertices` — the reused
  `edit_sketch_copy` as a session step; refuses a request whose version is not the
  session's current one.
* `ferritecad-app`: `sessions::spawn_apply_sketch`; `sketch::Editor` gains **Apply
  vertices** (and withholds *Save edited copy…* with words while the document has
  unsaved changes); the window takes the request like Apply height, begins *Edit
  Sketch…* with unsaved changes, ends the forms about saved objects whenever the
  accepted scene changes, and holds document Undo/Redo while any form is open.
* No change to the CLI, `edit-sketch-copy`, any JSON, the document format, geometry,
  the kernel, the evaluator or the renderer.

## Tests (headless and native)

* `ferritecad-jobs` (mock kernel): a vertex edit is a session step beside a height
  step and reaches the command line's model (`model_version` equal), Undo/Redo over
  both kinds, a new branch drops the Redo; a form made from an older version is
  refused with no file left, a no-op is not a step, a collapsed plate is refused.
* `ferritecad-app` headless: the Sketch form offers Apply only when it would work
  and keeps the draft; with unsaved changes the copy workflow is withheld and
  explained but Apply is not; a new accepted version ends the saved-object forms
  and not a New drawing; a vertex Apply is one step, a stale or unchanged form is
  not, a cancellation between computing and commit and a picture that cannot be
  shown change nothing; document Undo waits for an open form.
* Native (Open CASCADE, PlaneGCS where applicable), the window's real workers and
  the form's real widgets against the real peer CLI process: height → vertices →
  Undo → Undo → Redo → Redo → unsaved export → Save → cold rebuild (every saved name
  resolves) → new branch after Undo, on the offset fractional polygon and on the
  rectangle under a Chamfer; STL/FBX bytes equal the CLI's at every step, `Save`
  equals the CLI's second copy cell for cell (only `meta.modified_at` set aside).
  A refused edit (the Chamfer no longer fits) is refused in the form with the draft
  kept and, past the form, by the worker with state, history and files unchanged; a
  dimensioned plate offers no vertex form.
* Old `edit-sketch-copy` routes and their gates are unchanged and still run.

## Mutations — local, executed, restored

| Mutation | Result |
|---|---|
| **M30B-1** `edit_sketch_vertices` stops comparing the form's version with the session's | `a_vertex_edit_made_from_an_older_version_is_refused_and_changes_nothing` fails at `an old form must not apply to a newer version: Accepted` |
| **M30B-2** `finish_session_change` no longer ends the saved-object forms | `a_new_accepted_version_ends_the_forms_about_the_old_one_and_only_those` (`a form about the replaced version survived`) and both native vertex gates (`the form outlived the picture it described`) fail |

Two earlier invocations of these runs were rejected as evidence because the test
command itself was wrong (a missing feature flag, a misplaced option): a run that does
not execute the assertions is not a mutation result.

## Local results

Container: Linux, Open CASCADE 8.0.1 installed locally, a **local** PlaneGCS (not
the pinned one), root user, one cargo job at a time, debug profile.

* `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, the export-boundary, solver-ownership and licence
  checks pass; the solver-ownership check's single exception for
  `app/src/macos_quit.rs` is not extended to any new file.
* The workflow steps were extracted from the workflow files and executed as packed
  (`set -e`, the debug profile instead of `--release`):
  * *Open, edit, Undo, Redo and Save one document without native geometry*
    (`ci.yml`): once against the Open CASCADE build (65 gates `ok`, `exit 0`) and once
    in a **real stub build** (no Open CASCADE, `OpenCASCADE_DIR` and every `REQUIRE_*`
    unset, its own target directory): 65 gates `ok`, `exit 0`, no `skipped:`.
  * *Edit, Undo, Redo, export and Save one open document through the native window
    state* (`runtime-layout.yml`): six native gates `ok` against Open CASCADE and the
    local PlaneGCS, `exit 0`.
  * the Open CASCADE-**without**-solver tail of the *Chamfer a plate…* step (mixed): the
    solver refusal, the §30A plain plate and the §30B plain polygon, `ok`, `exit 0`.
* Affected packages, full: `ferritecad-document` (263), `ferritecad-jobs` (86),
  `ferritecad-ui` (104), `ferritecad-app` (424 including the 419 viewer tests), all
  passed; `ferritecad-cli --test edit_sketch` (5) passed. The unchanged heavy CLI/STEP
  corpus was not repeated for this change (no CLI, document-format or kernel file
  changed).
* Not run here: Windows, macOS, `--release`, the pinned PlaneGCS, any window.

## macOS fixture and window scenario (for Codex on the Mac)

Nothing here was run in a window; the container has none, and headless widget tests
are not a window check. The generator and the comparator use only the CLI. They were
exercised with a script standing in for the window (CLI calls and file copies),
which says nothing about the window: `FCAD_30B_GUI_COMPARE_OK`. Controls on
stand-ins: a missing file (`FCAD_30B_GUI_COMPARE_MISSING gui-chamfer.fbx …`); an
Apply that wrote the user's file; an unsaved export made without the vertices; a
foreign object row changed in a saved file. Inside the comparator, seven further
controls must reject (or it stops with `FCAD_30B_GUI_COMPARE_CONTROL_PASSED`).

Use the bundled CLI and a fresh arm64 bundle, no `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/sketch-session-apply-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_30B_GUI_FIXTURE\n", "ferrite-30b-fixture.py"),
                   ("# FCAD_30B_GUI_COMPARE\n", "ferrite-30b-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_30B_DIR="$PWD/sketch-session-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-30b-fixture.py "$FCAD_30B_DIR"
```

```python
# FCAD_30B_GUI_FIXTURE
import hashlib, json, os, pathlib, shutil, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "sketch-session-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
(out / "source").mkdir()
(out / "work").mkdir()
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# One offset, fractional plate, clockwise from the upper right: V0 (33, 15.5),
# V1 (33, 3.25), V2 (-4.5, 3.25), V3 (-4.5, 15.5); 37.5 x 12.25 mm, 6.75 mm tall.
# polygon.fcad is it as it is; chamfered.fcad has one 2.375 mm Chamfer at V1;
# constrained.fcad is the chamfered plate dimensioned through the command line.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
H0, DIST = 6.75, 2.375
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H0, "points_mm": PLATE}))
polygon = out / "source" / "polygon.fcad"
run("create-sketch-extrude", request, "-o", polygon, "--json")
catalog = run("inspect", polygon, "--json")["result"]
candidate = next(c for c in catalog["bodies"][0]["chamfer_edge"]["target"]["candidates"]
                 if c["corner_mm"] == PLATE[1])
request.write_text(json.dumps({"request_version": 1, "edge": candidate["edge"], "distance_mm": DIST}))
chamfered = out / "source" / "chamfered.fcad"
run("chamfer-edge-copy", polygon, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", chamfered, "--json")
c = run("inspect", chamfered, "--json")["result"]
(sketch,) = c["sketches"]
edit = sketch["constraint_edit"]
lines = [v["curve_id"] for v in edit["curves"]]
starts = [v["start_mm"] for v in edit["curves"]]
assert starts == PLATE and edit["available"] is True
across = [starts[i][1] == starts[(i + 1) % 4][1] for i in range(4)]
add = [{"curve_id": lines[i], "rule": "horizontal" if across[i] else "vertical"} for i in range(4)]
add.append({"curve_id": lines[0], "rule": "fixed", "at": "start", "x_mm": PLATE[0][0], "y_mm": PLATE[0][1]})
add.append({"curve_id": lines[across.index(True)], "rule": "distance", "distance_mm": 37.5})
add.append({"curve_id": lines[across.index(False)], "rule": "distance", "distance_mm": 12.25})
request.write_text(json.dumps({"request_version": 1, "remove": [], "add": add}))
constrained = out / "source" / "constrained.fcad"
run("edit-sketch-constraints-copy", chamfered, "--sketch", sketch["sketch_id"], "--expect-version",
    c["content_version"], "--request", request, "-o", constrained, "--json")
request.unlink()
facts = {}
for name in ("polygon", "chamfered", "constrained"):
    path = out / "source" / f"{name}.fcad"
    r = run("inspect", path, "--json")["result"]
    (feature,) = r["features"]
    (sk,) = r["sketches"]
    facts[name] = {"feature_id": feature["feature_id"], "sketch_id": sk["sketch_id"],
                   "editable": sk["editable"], "vertices": sk["vertices"],
                   "height_mm": feature["distance_mm"], "content_version": r["content_version"],
                   "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    shutil.copyfile(path, out / "work" / f"{name}.fcad")
assert facts["polygon"]["editable"] and facts["chamfered"]["editable"]
assert not facts["constrained"]["editable"], "a dimensioned plate must offer no vertex form"
facts["numbers"] = {"old_x": "33", "new_x": "41.25", "refused_x": "-4.4", "heights_mm":
                    {"first": 9.5, "branch": 11.25, "constrained": 8.5}, "chamfer_mm": DIST,
                    "plate": {"x0": -4.5, "y0": 3.25, "depth": 12.25, "right": 33.0}}
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_30B_GUI_FIXTURE_OK", out)
```

### Window scenario

`source/` is the comparator's reference: never open it in the window. Open only
files in `work/`. `cp` below is a shell copy made at that moment.

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_30B_DIR/../watch-30b.jsonl" --limit-mib 1536 --seconds 2400 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

One viewer per watchdog log. If system memory pressure stops a run it does not count:
start again from a fresh fixture directory. **After Quit read only the owned PID's exit
and the watchdog log** (no `getApp`, no `getAXState`, no restart).

1. **Open** `work/polygon.fcad`. *Edit extrusion…*, height `9.5`, **Apply**: no
   dialog, `*polygon.fcad` in the title. `cp work/polygon.fcad gui-poly-after-apply.fcad`.
2. *Edit Sketch…* is enabled although the document is unsaved. In the form type
   `41.25` in the X of the two vertices that show `33`. Try the draft's *Undo draft*,
   *Redo draft* and *Restore saved vertices* once each, then retype. **Apply
   vertices**: no dialog, the plate widens, the form closes, the star stays.
   *Save edited copy…* was withheld with its explanation while the form was open.
3. Export STL → `gui-poly-unsaved.stl`, FBX → `gui-poly-unsaved.fbx`.
4. With a form open, `Cmd+Z` must not change the document and the toolbar Undo is
   disabled. Close the form (*Cancel draft*). `Cmd+Z`: the plate narrows (height
   9.5 kept); `Cmd+Z` again: 6.75 mm and clean; `Cmd+Shift+Z` twice returns.
5. **Save** (`Cmd+S`): no dialog. `cp work/polygon.fcad gui-poly-saved.fcad`.
6. `Cmd+Z` once (vertices undone, dirty), *Edit extrusion…* height `11.25`,
   **Apply**: Redo is gone. **Save As** `work/polygon-b.fcad`.
   `cp work/polygon-b.fcad gui-poly-branch.fcad`;
   `cp work/polygon.fcad gui-poly-after-saveas.fcad`.
7. **Open** `work/chamfered.fcad`. *Edit Sketch…*, type `-4.4` in the two boxes that
   show `33`: the form refuses in words and offers no *Apply vertices*; the draft stays.
   `cp work/chamfered.fcad gui-chamfer-after-refusal.fcad`. Retype `41.25`, **Apply
   vertices**, export STL → `gui-chamfer.stl`, FBX → `gui-chamfer.fbx`, **Save**
   (`Cmd+S`). `cp work/chamfered.fcad gui-chamfer-saved.fcad`.
8. **Open** `work/constrained.fcad`. *Edit Sketch…* is unavailable and says why.
   *Edit extrusion…* `8.5`, **Apply**, export STL → `gui-constrained.stl`, **Save**.
   `cp work/constrained.fcad gui-constrained-saved.fcad`.
9. Optionally, with unsaved changes, `Cmd+Q` → Cancel keeps the dirty document;
   `Cmd+Q` → Discard exits. On the active Russian layout the Command chords must
   behave as on the US one. Quit; read the PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-30b-compare.py "$FCAD_30B_DIR"
```

It requires every `gui-*` file (and never creates one), checks the three sources by
SHA-256, then: the user's file after Apply and after a refused edit is byte-identical
to its source and after Save As to the earlier Save; what was saved has every SQL cell
of the peer CLI result (only `meta.modified_at` set aside) and differs from its source
only in the named object rows (the Extrude and Sketch rows after height + vertices; the
Extrude row after the branch; the Sketch row on the chamfered plate; the Extrude row on
the dimensioned plate, whose Sketch row stays byte for byte), with topology
references unchanged; the order of the two edits does not change the model; the
exports are byte-equal to `export-stl`/`export-fbx` of the peer result, and each STL is
read here — closed, one winding, the bounds and the exact volume (`W·D·h`, and
`(W·D − d²/2)·h` under the Chamfer). It then runs its negative controls and prints
`FCAD_30B_GUI_COMPARE_OK cells=N triangles=N`. The pinned ufbx reader is a separate
step, as before.

```python
# FCAD_30B_GUI_COMPARE
import hashlib, json, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-poly-after-apply.fcad", "gui-poly-unsaved.stl", "gui-poly-unsaved.fbx",
       "gui-poly-saved.fcad", "gui-poly-branch.fcad", "gui-poly-after-saveas.fcad",
       "gui-chamfer-after-refusal.fcad", "gui-chamfer-saved.fcad", "gui-chamfer.stl",
       "gui-chamfer.fbx", "gui-constrained-saved.fcad", "gui-constrained.stl")
def require(directory):
    missing = [n for n in GUI if not (directory / n).is_file()]
    assert not missing, f"FCAD_30B_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first"
try:
    require(out)
except AssertionError as error:
    sys.exit(str(error))
for name in ("polygon", "chamfered", "constrained"):
    assert hashlib.sha256((out / "source" / f"{name}.fcad").read_bytes()).hexdigest() == facts[name]["sha256"], \
        f"source/{name}.fcad changed"
work = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-30b-compare-"))
N = facts["numbers"]
OLD, NEW = float(N["old_x"]), float(N["new_x"])
H, DIST, PLATE = N["heights_mm"], N["chamfer_mm"], N["plate"]
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
def cells(path):
    """Every cell of every table, the modification stamp aside."""
    got = {}
    for t, (columns, rows) in tables(path).items():
        stamp = columns.index("modified_at") if t == "meta" and "modified_at" in columns else None
        got[t] = (columns, sorted(([None if i == stamp else v for i, v in enumerate(r)] for r in rows), key=repr))
    return got
def same_cells(a, b, why):
    x, y = cells(a), cells(b)
    for t in sorted(set(x) | set(y)):
        assert x.get(t) == y.get(t), f"{why}: {t} differs ({a.name} vs {b.name})"
def version(path):
    return run("inspect", path, "--json")["result"]["content_version"]
def edit_height(source, feature, height, dest):
    run("edit-extrude", source, "--feature", feature, "--distance-mm", str(height),
        "--expect-version", version(source), "-o", dest, "--json")
    return dest
def moved(vertices, old, new):
    return [{"curve_id": v["curve_id"], "start_mm": [new if v["start_mm"][0] == old else v["start_mm"][0], v["start_mm"][1]]}
            for v in vertices]
def edit_vertices(source, sketch, vertices, dest):
    request = work / f"{dest.stem}.json"
    request.write_text(json.dumps({"request_version": 1, "vertices": vertices}))
    run("edit-sketch-copy", source, "--sketch", sketch, "--expect-version", version(source),
        "--request", request, "-o", dest, "--json")
    return dest
def allowlist(before, after, rows):
    """Only the named object rows' payloads and hashes, and the stamp, moved; every
    other row of every table (other objects, topology references, constraints) is
    the same, and no row was added or lost."""
    ids = {bytes.fromhex(r.replace("-", "")) for r in rows}
    a, b = tables(before), tables(after)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        assert len(arows) == len(brows), f"{t}: a row was added or lost"
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                assert u == v or (t == "objects" and c in ("payload", "payload_hash")
                                  and x[ac.index("id")] in ids) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c} moved"
def mesh(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    return [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)] for i in range(count)]
def measure(stl, right, height, chamfered):
    """Read here, independently of the B-Rep: one closed oriented surface, the
    plate's bounds and the exact analytic volume."""
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
    xs, ys, zs = ([p[k] for p in pts] for k in range(3))
    x0, y0, depth = PLATE["x0"], PLATE["y0"], PLATE["depth"]
    for got, want in ((min(xs), x0), (max(xs), right), (min(ys), y0), (max(ys), y0 + depth),
                      (min(zs), 0.0), (max(zs), height)):
        assert abs(got - want) < 1e-4, ("bounds", got, want)
    cut = DIST ** 2 / 2 if chamfered else 0.0
    exact = ((right - x0) * depth - cut) * height
    assert abs(six / 6 - exact) < 1e-3, ("volume", six / 6, exact)
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
    sys.exit(f"FCAD_30B_GUI_COMPARE_CONTROL_PASSED {label}: the comparison cannot tell a wrong result")
def read(name):
    return (out / name).read_bytes()
S = out / "source"
PG, CH, CO = facts["polygon"], facts["chamfered"], facts["constrained"]

# 1. The user's file is untouched by Apply, Undo, Redo, and by a refused edit.
assert read("gui-poly-after-apply.fcad") == (S / "polygon.fcad").read_bytes(), "Apply wrote the user's file"
assert read("gui-poly-after-saveas.fcad") == read("gui-poly-saved.fcad"), "Save As changed the original"
assert read("gui-chamfer-after-refusal.fcad") == (S / "chamfered.fcad").read_bytes(), "a refused edit wrote"

# 2. What the window saved is what the command line makes: height 9.5 then the
# vertices (the order does not matter); a new branch after Undo; the Chamfer's
# plate; the dimensioned plate's height.
h1 = edit_height(S / "polygon.fcad", PG["feature_id"], H["first"], work / "h1.fcad")
v1 = edit_vertices(h1, PG["sketch_id"], moved(PG["vertices"], OLD, NEW), work / "v1.fcad")
v1b = edit_vertices(S / "polygon.fcad", PG["sketch_id"], moved(PG["vertices"], OLD, NEW), work / "v1b.fcad")
v1c = edit_height(v1b, PG["feature_id"], H["first"], work / "v1c.fcad")
same_cells(v1, v1c, "the order of two edits does not change the model")
same_cells(out / "gui-poly-saved.fcad", v1, "Save of height + vertices")
branch = edit_height(S / "polygon.fcad", PG["feature_id"], H["branch"], work / "branch.fcad")
same_cells(out / "gui-poly-branch.fcad", branch, "the new branch")
allowlist(S / "polygon.fcad", out / "gui-poly-saved.fcad", [PG["feature_id"], PG["sketch_id"]])
allowlist(S / "polygon.fcad", out / "gui-poly-branch.fcad", [PG["feature_id"]])
c1 = edit_vertices(S / "chamfered.fcad", CH["sketch_id"], moved(CH["vertices"], OLD, NEW), work / "c1.fcad")
same_cells(out / "gui-chamfer-saved.fcad", c1, "the chamfered plate")
allowlist(S / "chamfered.fcad", out / "gui-chamfer-saved.fcad", [CH["sketch_id"]])
k1 = edit_height(S / "constrained.fcad", CO["feature_id"], H["constrained"], work / "k1.fcad")
same_cells(out / "gui-constrained-saved.fcad", k1, "the dimensioned plate")
allowlist(S / "constrained.fcad", out / "gui-constrained-saved.fcad", [CO["feature_id"]])
for name in ("gui-poly-saved", "gui-poly-branch", "gui-chamfer-saved", "gui-constrained-saved"):
    run("validate", out / f"{name}.fcad", "--json")
    run("inspect", out / f"{name}.fcad", "--json")

# 3. The exports are the accepted model, byte for byte what the command line
# exports for it, and the mesh is read here.
stl_p, fbx_p = exported(v1, "poly")
assert read("gui-poly-unsaved.stl") == stl_p and read("gui-poly-unsaved.fbx") == fbx_p, \
    "the unsaved export is not the accepted model"
stl_c, fbx_c = exported(c1, "chamfer")
assert read("gui-chamfer.stl") == stl_c and read("gui-chamfer.fbx") == fbx_c, "the chamfered export differs"
stl_k, _ = exported(k1, "constrained")
assert read("gui-constrained.stl") == stl_k, "the dimensioned export differs"
measure(out / "gui-poly-unsaved.stl", NEW, H["first"], False)
measure(out / "gui-chamfer.stl", NEW, CH["height_mm"], True)
measure(out / "gui-constrained.stl", PLATE["right"], H["constrained"], True)

# 4. Negative controls: each comparison above must reject the wrong thing.
height_only_stl, _ = exported(h1, "height-only")
def equal(a, b):
    assert a == b, "different bytes"
must_fail("an export without the vertices", lambda: equal(height_only_stl, read("gui-poly-unsaved.stl")))
other = edit_vertices(h1, PG["sketch_id"], moved(PG["vertices"], OLD, NEW + 0.25), work / "wrong.fcad")
must_fail("a different vertex in the save", lambda: same_cells(out / "gui-poly-saved.fcad", other, "control"))
foreign = work / "foreign.fcad"
foreign.write_bytes(read("gui-poly-saved.fcad"))
db = sqlite3.connect(foreign)
db.execute("UPDATE objects SET payload_hash = zeroblob(length(payload_hash)) WHERE id NOT IN (?, ?)",
           (bytes.fromhex(PG["feature_id"].replace("-", "")), bytes.fromhex(PG["sketch_id"].replace("-", ""))))
db.commit()
db.close()
must_fail("a foreign object row changed", lambda: allowlist(S / "polygon.fcad", foreign, [PG["feature_id"], PG["sketch_id"]]))
must_fail("a sketch row outside the allowlist", lambda: allowlist(S / "polygon.fcad", out / "gui-poly-saved.fcad", [PG["feature_id"]]))
must_fail("a missing output", lambda: require(work))
must_fail("the old plate in the mesh", lambda: measure(out / "gui-poly-unsaved.stl", PLATE["right"], H["first"], False))
print("FCAD_30B_GUI_COMPARE_OK", f"cells={sum(len(r) for _, r in tables(out / 'gui-poly-saved.fcad').values())}",
      f"triangles={len(mesh(out / 'gui-poly-unsaved.stl'))}")
```

## Limits

Not run in this container: the window, macOS, Windows, a GPU, the pinned PlaneGCS,
`--release`. The independent STL reader here is the comparator's, not a CI step.
Circle, annulus, Revolve-angle, Cut, Fillet, Chamfer and constraint editors are
unchanged copy workflows and stay unavailable while the document has unsaved
changes. Milestone 5C, the historical OOM investigation and the general beta stay
open.

## Independent macOS review — 2026-10-03

The real window exposed a blocking App integration defect: opening the saved
Sketch made `Creates::busy()` true, so `can_begin_new` disabled **Apply vertices**
and the command handler would refuse it as well. The standalone widget/worker
checks had granted permission directly and did not exercise that composition.
The initial arm64 window reproduced this with valid coordinates; pressing Apply
left the form and document unchanged. It quit normally under the 1536 MiB guard.

The corrected button and command use one `can_apply_sketch` predicate. The saved
vertex form is its caller, while creation, another form, document loading,
exports and session operations still exclude Apply. New/Open/Save and document
Undo/Redo retain their existing guards. The executed regression
`sketch::tests::the_open_sketch_form_can_apply_through_the_windows_busy_guard`
failed at `the open form must allow Apply` before the correction and passes
with it; the existing non-native CI step now requires its exact execution.
The unsaved-editor explanation no longer lists Sketch as unavailable.

The same window run also found a previously existing height-entry defect: after
Undo, the dirty document's **Edit extrusion…** entry still used the copy-only
`can_edit` condition. The panel now also permits opening when session Apply is
available, while an existing form/worker still excludes another. The existing
App regression `an_open_height_form_keeps_the_copy_workflow_available_on_a_clean_document`
was extended to click the actual opening button after a real accepted step; it
failed `Waiting != Begin` before this correction. The Sketch form title now says
*Edit saved Sketch*, since both Apply and saving a copy are supported.


The corrected arm64 bundle was built from `346bdfb` with the existing pinned
OCCT/PlaneGCS runtime, staged with the normal closure tool, and checked with its
bundled `--solver-info`. The subsequent `0b7b335` change only clears the regression
test's unused egui texture deltas; CI's debug assertions caught their drop, which
release tests had not. No application code changed after the final bundle.

The complete window scenario ran on private copies in
`/private/tmp/ferrite-pr82-review/scenario-complete`: height 6.75 → 9.5, both right
vertices 33 → 41.25, draft Undo/Redo/Restore, unsaved STL/FBX, document Undo twice
and Redo twice, Save, Undo then height 11.25 and Save As. Opening the height form
on that dirty version succeeded. The chamfered plate refused right X = −4.4 with
the actual Chamfer UUID and retained the draft; X = 41.25 applied and saved.
The constrained plate withheld vertex editing, retained fully constrained status,
and accepted height 8.5. Save and Save As used native dialogs where appropriate;
Apply did not. The first saved file remained unchanged by Save As. Quit ended
the owned process normally; no UI query was made after exit.

The extracted comparator consumed those real window artifacts and printed
`FCAD_30B_GUI_COMPARE_OK cells=12 triangles=12`. It checked unchanged disk state
before Save and after refusal, all SQL cells against the CLI and explicit edit
allowlists, history branching, independent closed/oriented STL measurements, and
byte-identical GUI/CLI STL/FBX. Its negative controls rejected incorrect artifacts.
Pinned ufbx 0.23.0 read both GUI FBX files: 6 checks / 0 failures each.

The final watchdog run lasted 719.56 s, peak footprint 207.892 MiB, pressure 1
throughout, swap 1702035456 bytes at both ends, exit 0, no watchdog abort.
This is a bounded smoke observation, not a resolution of the historical OOM.

Local review checks: jobs session 10; App session workers 26; Sketch 54;
height/edit workers 17; document commands 2; both availability regressions;
workspace all-target/all-feature clippy with `-D warnings`, fmt and diff checks.
Native checks required OCCT and PlaneGCS; no native skip was counted as evidence.
The review logs, failed-first assertions, bundle and watchdog samples are under
`/private/tmp/ferrite-pr82-review`. Cross-platform CI is checked separately on the
published revision; earlier base/head successes are not substituted for it.
