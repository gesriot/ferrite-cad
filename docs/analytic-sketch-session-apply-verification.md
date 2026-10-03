# §30D — Circle and annulus geometry inside the open document: verification

[The contract](analytic-sketch-session-apply.md). Marker `FCAD_30D_ANALYTIC_SKETCH_SESSION_APPLY`.
Base: `main` `19757e4ba22e6d9157f87c003dbd3747886b9c06` (the merge of PR #83; tree equal to the
verified head `64edb69`, whose CI was 15/15). The post-merge CI of that base is a separate run
and is not claimed here. Branch `analytic-sketch-session-apply`. This record names no CI of its own head yet; the final section is filled once the code and workflow head has run.

## What changed

* `ferritecad-jobs`: `StepTicket::edit_circle` and `StepTicket::edit_annulus` — the reused
  `edit_circle_copy` / `edit_annulus_copy` as session steps; a request whose version is not the
  session's current one is refused.
* `ferritecad-app`: `sessions::spawn_apply_circle` / `spawn_apply_annulus`; the two forms gain
  **Apply circle** / **Apply annulus**; the former *Apply … change* is **Confirm draft numbers**;
  *Save edited … copy…* is withheld with words while the document has unsaved changes; the window
  computes `can_apply_analytic` (one predicate for the button and the command), opens the forms
  with unsaved changes when it says so, and starts the worker through the same `begin_apply`;
  the session's last line is drawn inside the forms; `Editor::dismiss` carries the new flag.
  Window titles and the held-back sentence no longer say *new copy* / *circles* where that is
  not true.
* No change to the CLI, `edit-circle`, `edit-annular`, any JSON, the document format, geometry,
  the kernel, the solver policy, the evaluator or the renderer.

## Tests

* Headless, the forms' real widgets and the window's own predicate (`sketch::tests::analytic_apply`,
  kernel-free: documents written through the document API): the open form applies and keeps its
  draft, with the typed numbers whether or not they were confirmed (Circle and annulus, the annulus
  by both curve UUIDs); an Open in flight, a running session operation, the height form, another
  editor's form, no form, no session and a Sketch of the wrong kind each block it; the stored
  numbers (also spelled differently) offer no Apply, and the press that asks for nothing leaves the
  draft's Redo; the draft confirmation asks for nothing and the old name is gone; with unsaved
  changes the copy is withheld and explained but Apply is not; a new accepted version ends the draft
  and the next form still has what the window said; the entries open on a dirty document only when
  the window says so; the session's line is painted inside the form's window; while a worker is busy
  a press starts nothing.
* Native (Open CASCADE), the window's real workers and the forms' real widgets against the real peer
  command line: height → geometry → unsaved export → Undo → Undo → Redo → Redo → Save → cold rebuild
  (every saved name resolves) → stale form refused → request the document will not store refused →
  stored numbers a no-op → new branch after Undo. A Circle with a fractional offset centre
  (12.5, -7.25) r 10.5 → (-3.5, 4.25) r 6.75; an annulus boundary 10.5 / bore 4.75 → 9.25 / 3.125,
  created by the command, and the same drawing stored **bore first**. At every step STL/FBX equal the
  command line's bytes; **Save equals the command line's copy in every SQL cell but `meta.modified_at`**
  (no object row or payload is set aside — there is no new UUID to excuse one); every table but
  `objects`/`meta` equals the one before the edit; the roles of the annulus are those of the curve
  UUIDs, the stored order is unchanged. The STL is read independently of the B-Rep: one closed oriented
  surface, the bounds, a volume only the inscribed polygon can lower, and a cavity exactly when there is
  a bore.
* The constraint editor's availability follows the accepted version: a Radius and a Fixed centre applied
  through the session (§30C) withdraw the Circle editor from the next catalogue, removing them through the
  session offers it again.
* Mixed (Open CASCADE, no solver): the three native gates above pass without PlaneGCS.
* Stub (no Open CASCADE): the widget gates run; the kernel-free refusal of the worker passes (a build with
  no kernel refuses Apply whole and stays clean).

## Mutations — local, executed, restored

| Mutation | Result |
|---|---|
| **M30D-1** `Creates::can_apply_analytic` uses `!self.busy()` (the form blocks its own Apply) | 4 widget tests fail on executed assertions (`the form's own Apply must not be blocked by the form` and the three that go through the predicate) |
| **M30D-2** `finish_session_change` no longer ends the Circle and annulus drafts (a stale draft survives the new version) | the widget test (`the draft outlived the picture it described`) and all three native gates (`the form outlived the picture it described`) fail |
| **M30D-3** `StepTicket::edit_circle` drops the version guard (a form is applied to a version it was not opened on) | `native_a_circle_with_a_fractional_offset…` fails at its executed assertion that a stale form is refused |

All three compiled. The sources were restored (`git diff` shows only the intended change).

## Local results

Container: Linux x86_64, Open CASCADE 8.0.1 installed locally, a **local** PlaneGCS (not the pinned one),
debug profile, root user. `macOS arm64`, Windows and `--release` were not run here.

* `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  pass; so do the export-boundary, solver-ownership and licence-header checks.
* The workflow steps were extracted from the workflow files and executed as packed (`set -e`, debug instead
  of `--release`):
  * *Edit, Undo, Redo, export and Save one open document through the native window state*
    (`runtime-layout.yml`): 15 gates `ok`, `exit 0`, no `skipped:`, including the four new ones, and the
    artifact directory it hands to the pinned reader was written;
  * *Open, edit, Undo, Redo and Save one document without native geometry* (`ci.yml`) in a **real stub
    build** (no Open CASCADE): 82 gates `ok`, `exit 0`, no `skipped:`, including the ten new ones.
* The pinned **strict ufbx** reader (`read_production --identity`, `--triangles` and the STL/FBX join of
  `tools/fbx/stl-matches-fbx.py`) over the saved model's export of the three documents: `checks=6 failures=0`
  twice each, and `FCAD_STL_FBX_MATCH triangles=324` (Circle), `608` and `608` (annulus, bore last and bore
  first). The same block (`FCAD_ANALYTIC_SESSION_UFBX_EXECUTED`) now runs in `tools/check-fbx-complex.sh` for
  the artifacts the native step writes. (Compiled here with gcc without `-Werror`: the unused-variable warning
  in an unrelated function is a gcc-only error; CI uses clang.)
* Affected packages: `ferritecad-jobs` (88 across targets), `ferritecad-app` (445 + 5, including every native
  session gate of §30A–C), `ferritecad-document` (264), `ferritecad-ui` (104), `ferritecad-cli --test
  edit_circle --test edit_annular --test edit_constraints --test edit_sketch` (25) all passed.
* Not run here: Windows, macOS, `--release`, the pinned PlaneGCS, any window.

## macOS fixture and window scenario (for the Mac)

Nothing here was run in a window; the container has none, and headless widget tests are not a window check.
The generator and the comparator use only the command line; they were exercised with a script standing in for
the window (CLI calls and file copies), which says nothing about the window: `FCAD_30D_GUI_COMPARE_OK cells=12
triangles=324`. Controls on stand-ins: a missing file (`FCAD_30D_GUI_COMPARE_MISSING …`); the generator
inside a git checkout (`FCAD_30D_GUI_FIXTURE_REFUSED`); an Apply that wrote the user's file; a Save As that changed
the original; the branch file in place of the saved one; the source in place of the saved one; an export of the
wrong step; the Circle's mesh in place of the annulus'; the source in place of the saved annulus; a refused edit that
wrote. Inside the comparator, eight further controls must reject (or it stops with
`FCAD_30D_GUI_COMPARE_CONTROL_PASSED`).

Use the bundled CLI and a fresh arm64 bundle, no `FCAD_ALLOW_LOADER_FAILURE_PROBES`. The directory must be
**outside the checkout**; sources and working copies are separate.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/analytic-sketch-session-apply-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_30D_GUI_FIXTURE\n", "ferrite-30d-fixture.py"),
                   ("# FCAD_30D_GUI_COMPARE\n", "ferrite-30d-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_30D_DIR="$HOME/ferrite-30d-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-30d-fixture.py "$FCAD_30D_DIR"
```

```python
# FCAD_30D_GUI_FIXTURE
import hashlib, json, os, pathlib, shutil, subprocess, sys
cli = os.environ["FERRITECAD"]
if len(sys.argv) != 2:
    sys.exit("usage: ferrite-30d-fixture.py DIRECTORY_OUTSIDE_THE_CHECKOUT")
out = pathlib.Path(sys.argv[1]).resolve()
probe = out.parent
inside = subprocess.run(["git", "-C", str(probe), "rev-parse", "--show-toplevel"],
                        capture_output=True, encoding="utf-8")
if inside.returncode == 0:
    sys.exit(f"FCAD_30D_GUI_FIXTURE_REFUSED {out} is inside the git checkout {inside.stdout.strip()}")
out.mkdir(parents=True, exist_ok=False)
(out / "source").mkdir()
(out / "work").mkdir()
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# circle.fcad: one circle, centre (12.5, -7.25), radius 10.5, 15.25 mm tall.
# annulus.fcad: the same centre, boundary 10.5, bore 4.75, 15.25 mm tall.
request = out / "request.json"
request.write_text(json.dumps({"schema_version": 1, "center_mm": [12.5, -7.25], "radius_mm": 10.5, "height_mm": 15.25}))
run("create-circle-extrude", request, "-o", out / "source" / "circle.fcad", "--json")
request.write_text(json.dumps({"schema_version": 1, "center_mm": [12.5, -7.25], "outer_radius_mm": 10.5,
                               "inner_radius_mm": 4.75, "height_mm": 15.25}))
run("create-annular-extrude", request, "-o", out / "source" / "annulus.fcad", "--json")
request.unlink()
facts = {}
for name in ("circle", "annulus"):
    path = out / "source" / f"{name}.fcad"
    r = run("inspect", path, "--json")["result"]
    (feature,) = r["features"]
    (sk,) = r["sketches"]
    edit = sk["circle_edit"] if name == "circle" else sk["annulus_edit"]
    assert edit["available"] is True and edit["refusal"] is None
    facts[name] = {"feature_id": feature["feature_id"], "sketch_id": sk["sketch_id"],
                   "content_version": r["content_version"],
                   "saved": edit[name], "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    shutil.copyfile(path, out / "work" / f"{name}.fcad")
facts["numbers"] = {"height_mm": 8.5, "branch_height_mm": 11.25, "centre_mm": [-3.5, 4.25],
                    "circle_radius_mm": 6.75, "outer_radius_mm": 9.25, "inner_radius_mm": 3.125,
                    "refused_inner_radius_mm": 12.0, "annulus_height_mm": 15.25}
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_30D_GUI_FIXTURE_OK", out)
```

### Window scenario

`source/` is the comparator's reference: never open it in the window. Open only files in `work/`. `cp` below
is a shell copy made at that moment. `circle.fcad` is one circle, centre (12.5, -7.25), radius 10.5 mm, 15.25 mm
tall; `annulus.fcad` has the same centre, boundary 10.5 mm and bore 4.75 mm.

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_30D_DIR/../watch-30d.jsonl" --limit-mib 1536 --seconds 2400 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

One viewer per watchdog log. If system memory pressure stops a run it does not count: start again from a fresh
fixture directory. **After Quit read only the owned PID's exit and the watchdog log** (no `getApp`, no
`getAXState`, no restart).

1. **Open** `work/circle.fcad`. *Edit extrusion…*, height `8.5`, **Apply**: no dialog, `*circle.fcad` in the
   title.
2. *Edit circle…* is enabled although the document is unsaved. The form has **Apply circle**,
   **Confirm draft numbers** and *Save edited circle copy…* (withheld, with its explanation). Type X `-3.5`, Y `4.25`,
   Radius `6.75`; press **Confirm draft numbers** once, then *Undo draft* and *Redo draft* once each. Press **Apply
   circle**: no dialog, the circle moves and shrinks, the form closes, the star stays. `cp work/circle.fcad
   gui-circle-after-apply.fcad` (still the source). Export STL → `gui-circle-unsaved.stl`, FBX →
   `gui-circle-unsaved.fbx`.
3. With a form open, `Cmd+Z` must not change the document and the toolbar Undo is disabled (open *Edit circle…* to
   see it; *Cancel draft*). `Cmd+Z` once: the circle is back, height `8.5` kept; export STL →
   `gui-circle-undo1.stl`. `Cmd+Z` again: 15.25 mm and clean; `Cmd+Shift+Z` twice returns.
4. **Save** (`Cmd+S`): no dialog. `cp work/circle.fcad gui-circle-saved.fcad`.
5. `Cmd+Z` once (the circle step undone, dirty), *Edit extrusion…* height `11.25`, **Apply**: Redo is gone. **Save
   As** `work/circle-b.fcad`; `cp work/circle-b.fcad gui-circle-branch.fcad`; `cp work/circle.fcad
   gui-circle-after-saveas.fcad`.
6. **Open** `work/annulus.fcad`. *Edit annulus…*: type Inner radius `12` (not less than the boundary): the form
   refuses in words and offers no **Apply annulus**. `cp work/annulus.fcad gui-annulus-after-refusal.fcad`. Type
   X `-3.5`, Y `4.25`, Outer radius `9.25`, Inner radius `3.125`; **Apply annulus**; export STL →
   `gui-annulus-unsaved.stl`, FBX → `gui-annulus-unsaved.fbx`; **Save** (`Cmd+S`); `cp work/annulus.fcad
   gui-annulus-saved.fcad`.
7. Optionally, with unsaved changes, `Cmd+Q` → Cancel keeps the dirty document; `Cmd+Q` → Discard exits. On the
   active Russian layout the Command chords must behave as on the US one. Quit; read the PID's exit and the
   watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-30d-compare.py "$FCAD_30D_DIR"
```

It requires every `gui-*` file (and never creates one), checks the two sources by SHA-256, then: the user's file
after Apply and after a refused edit is byte-identical to its source and after Save As to the earlier Save; what was
saved has **every SQL cell** of the peer CLI result (only the write stamp `meta.modified_at` set aside — no row, no
payload, no hash), and every table but the object payloads and the stamp equals the document before the edit, so no
UUID, name, parent, order or topology reference is new; the annulus keeps its two curve UUIDs in their roles; the
exports are byte-equal to `export-stl`/`export-fbx` of the peer result and each STL is read here — closed, one
winding, the bounds, the volume and (for the annulus) the cavity. It then runs its negative controls and prints
`FCAD_30D_GUI_COMPARE_OK cells=N triangles=N`. The pinned ufbx reader is a separate step, as before.

```python
# FCAD_30D_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-circle-after-apply.fcad", "gui-circle-unsaved.stl", "gui-circle-unsaved.fbx",
       "gui-circle-undo1.stl", "gui-circle-saved.fcad", "gui-circle-after-saveas.fcad",
       "gui-circle-branch.fcad", "gui-annulus-after-refusal.fcad", "gui-annulus-unsaved.stl",
       "gui-annulus-unsaved.fbx", "gui-annulus-saved.fcad")
def require(directory):
    missing = [n for n in GUI if not (directory / n).is_file()]
    assert not missing, f"FCAD_30D_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first"
try:
    require(out)
except AssertionError as error:
    sys.exit(str(error))
S = out / "source"
for name in ("circle", "annulus"):
    assert hashlib.sha256((S / f"{name}.fcad").read_bytes()).hexdigest() == facts[name]["sha256"], \
        f"source/{name}.fcad changed"
work = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-30d-compare-"))
N, C, A = facts["numbers"], facts["circle"], facts["annulus"]
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
    """Every cell of every table. Only the write stamp is set aside: it is the instant
    a writer ran, which no two runs share. No object row and no payload is."""
    got = {}
    for t, (columns, rows) in tables(path).items():
        stamp = columns.index("modified_at") if t == "meta" and "modified_at" in columns else None
        got[t] = (columns, sorted(([None if i == stamp else v for i, v in enumerate(r)] for r in rows), key=repr))
    return got
def same_model(a, b, why):
    x, y = cells(a), cells(b)
    for t in sorted(set(x) | set(y)):
        assert x.get(t) == y.get(t), f"{why}: {t} differs ({a.name} vs {b.name})"
def identity_kept(before, after, why):
    """No new identity anywhere: every table but the object payloads and the stamp is
    the same, and every object keeps its id, parent, name and order."""
    x, y = tables(before), tables(after)
    assert x.keys() == y.keys()
    for t in x:
        if t in ("objects", "meta"):
            continue
        assert x[t] == y[t], f"{why}: {t} changed"
    (xc, xr), (yc, yr) = x["objects"], y["objects"]
    assert xc == yc and len(xr) == len(yr), f"{why}: objects"
    for u, v in zip(xr, yr):
        for column, a, b in zip(xc, u, v):
            assert a == b or column in ("payload", "payload_hash"), f"{why}: objects.{column} moved"
def version(path):
    return run("inspect", path, "--json")["result"]["content_version"]
def edit_height(source, feature, height, dest):
    run("edit-extrude", source, "--feature", feature, "--distance-mm", str(height),
        "--expect-version", version(source), "-o", dest, "--json")
    return dest
def peer(operation, source, sketch, request, dest):
    file = work / f"{dest.stem}.json"
    file.write_text(json.dumps({"request_version": 1, **request}))
    run(operation, source, "--sketch", sketch, "--expect-version", version(source),
        "--request", file, "-o", dest, "--json")
    return dest
def circle_request(radius):
    return {"curve_id": C["saved"]["curve_id"], "center_mm": N["centre_mm"], "radius_mm": radius}
def annulus_request(outer, inner):
    return {"outer_curve_id": A["saved"]["outer_curve_id"], "inner_curve_id": A["saved"]["inner_curve_id"],
            "center_mm": N["centre_mm"], "outer_radius_mm": outer, "inner_radius_mm": inner}
def mesh(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    return [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)] for i in range(count)]
def measure(stl, centre, radius, bore, height):
    """Read here, independently of the B-Rep: one closed oriented surface, the bounds,
    the volume of a cylinder or a tube (an inscribed polygon can only lower it), and a
    cavity exactly when there is a bore."""
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
    for k, (lo, hi) in enumerate(((centre[0] - radius, centre[0] + radius), (centre[1] - radius, centre[1] + radius),
                                  (0.0, height))):
        assert abs(min(p[k] for p in pts) - lo) < 2e-2 and abs(max(p[k] for p in pts) - hi) < 2e-2, ("bounds", k)
    exact = math.pi * (radius ** 2 - (bore or 0.0) ** 2) * height
    assert 0.97 * exact < six / 6 <= exact + 1e-3, ("volume", six / 6, exact)
    radial = [math.hypot(p[0] - centre[0], p[1] - centre[1]) for p in pts]
    assert (bore is not None) == any(abs(r - (bore or -1.0)) < 1e-2 for r in radial), "the cavity"
    assert all(abs(r - radius) < 1e-2 or (bore is not None and abs(r - bore) < 1e-2) for r in radial), "a stray vertex"
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
    sys.exit(f"FCAD_30D_GUI_COMPARE_CONTROL_PASSED {label}: the comparison cannot tell a wrong result")
def read(name):
    return (out / name).read_bytes()

# 1. The user's file is untouched by Apply, Undo, Redo and a refused edit, and Save As
# leaves the original as the earlier Save made it.
assert read("gui-circle-after-apply.fcad") == (S / "circle.fcad").read_bytes(), "Apply wrote the user's file"
assert read("gui-annulus-after-refusal.fcad") == (S / "annulus.fcad").read_bytes(), "a refused edit wrote"
assert read("gui-circle-after-saveas.fcad") == read("gui-circle-saved.fcad"), "Save As changed the original"

# 2. What the window saved is the command line's model in every cell, with no new identity.
h1 = edit_height(S / "circle.fcad", C["feature_id"], N["height_mm"], work / "h1.fcad")
c1 = peer("edit-circle", h1, C["sketch_id"], circle_request(N["circle_radius_mm"]), work / "c1.fcad")
same_model(out / "gui-circle-saved.fcad", c1, "circle Save")
identity_kept(h1, out / "gui-circle-saved.fcad", "circle Save")
branch = edit_height(h1, C["feature_id"], N["branch_height_mm"], work / "branch.fcad")
same_model(out / "gui-circle-branch.fcad", branch, "the new branch")
a1 = peer("edit-annular", S / "annulus.fcad", A["sketch_id"],
          annulus_request(N["outer_radius_mm"], N["inner_radius_mm"]), work / "a1.fcad")
same_model(out / "gui-annulus-saved.fcad", a1, "annulus Save")
identity_kept(S / "annulus.fcad", out / "gui-annulus-saved.fcad", "annulus Save")
for name in ("gui-circle-saved", "gui-circle-branch", "gui-annulus-saved"):
    run("validate", out / f"{name}.fcad", "--json")
    run("inspect", out / f"{name}.fcad", "--json")
# The annulus keeps its roles by curve UUID: the same two UUIDs, the boundary one holding
# the new boundary radius and the bore one the new bore radius.
saved = run("inspect", out / "gui-annulus-saved.fcad", "--json")["result"]["sketches"][0]["annulus_edit"]["annulus"]
assert saved["outer_curve_id"] == A["saved"]["outer_curve_id"] and saved["inner_curve_id"] == A["saved"]["inner_curve_id"]
assert saved["outer_radius_mm"] == N["outer_radius_mm"] and saved["inner_radius_mm"] == N["inner_radius_mm"]

# 3. The exports are the accepted model, byte for byte what the command line exports for it,
# and the mesh is read here.
stl_c, fbx_c = exported(c1, "circle")
assert read("gui-circle-unsaved.stl") == stl_c and read("gui-circle-unsaved.fbx") == fbx_c, \
    "the unsaved export is not the accepted model"
stl_h, _ = exported(h1, "circle-height")
assert read("gui-circle-undo1.stl") == stl_h, "Undo did not give the height step"
stl_a, fbx_a = exported(a1, "annulus")
assert read("gui-annulus-unsaved.stl") == stl_a and read("gui-annulus-unsaved.fbx") == fbx_a, \
    "the unsaved annulus export differs"
measure(out / "gui-circle-unsaved.stl", N["centre_mm"], N["circle_radius_mm"], None, N["height_mm"])
measure(out / "gui-annulus-unsaved.stl", N["centre_mm"], N["outer_radius_mm"], N["inner_radius_mm"], N["annulus_height_mm"])

# 4. Negative controls: each comparison above must reject the wrong thing.
original_stl, _ = exported(S / "circle.fcad", "original")
def equal(a, b):
    assert a == b, "different bytes"
must_fail("an export of the disk original", lambda: equal(original_stl, read("gui-circle-unsaved.stl")))
wrong = peer("edit-circle", h1, C["sketch_id"], circle_request(N["circle_radius_mm"] + 0.125), work / "wrong.fcad")
must_fail("a different radius in the save", lambda: same_model(out / "gui-circle-saved.fcad", wrong, "control"))
must_fail("the branch compared with the replaced model", lambda: same_model(out / "gui-circle-branch.fcad", c1, "control"))
must_fail("the height step compared with the geometry step", lambda: same_model(h1, c1, "control"))
must_fail("the original identity compared with a different document",
          lambda: identity_kept(S / "circle.fcad", S / "annulus.fcad", "control"))
must_fail("a missing output", lambda: require(work))
must_fail("a solid measured as a tube", lambda: measure(out / "gui-circle-unsaved.stl", N["centre_mm"],
          N["circle_radius_mm"], N["circle_radius_mm"] / 2, N["height_mm"]))
must_fail("an old-radius mesh", lambda: measure(work / "original.stl", N["centre_mm"], N["circle_radius_mm"], None, N["height_mm"]))
print("FCAD_30D_GUI_COMPARE_OK", f"cells={sum(len(r) for _, r in tables(out / 'gui-circle-saved.fcad').values())}",
      f"triangles={len(mesh(out / 'gui-circle-unsaved.stl'))}")
```

## CI of the code and workflow head

Code and workflow head: `5b08f3422693039d16d94f40ee58bf1bcc93dedb` (the PR's parent commit
`9861325` carries the forms, the step and the widget tests; this one the native, stub and mixed gates, the workflow
gates and the documents). All 15 checks succeeded on it:

* CI, run `37153185071`: `lint`, `notices`, `sbom`, `supply-chain`, `test (ubuntu-latest)`, `test (macos-latest)`,
  `test (windows-latest)` — the stub step with the ten new gates by exact name;
* Open CASCADE and PlaneGCS runtime layout, run `37153173720`: `linux`, `macos`, `windows` and `Compare what the
  three platforms measured` — the native step with the four new gates and the strict ufbx block
  (`FCAD_ANALYTIC_SESSION_UFBX_EXECUTED`, which the step greps for);
* PlaneGCS pin, run `37153173718`: `linux`, `macos`, `windows` and `Compare what the three platforms concluded`.

The log lines of the new gates were not read one by one here: each workflow step fails on a `skipped:` line and on a
missing `test <name> ... ok` line, and the ufbx step fails without its marker, so a green step is the evidence that they
ran and passed on that system. This commit only records the result; its own checks are tracked in the checks of the PR.
