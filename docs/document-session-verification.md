# §30A — a working document session: verification

[The contract](document-session.md) and [the decision](decisions/0005-document-session.md).
This is the record of what was run, where, and what was not. It states the result of
one head; the CI of the PR that carries it is recorded in the PR, not here, since a
document cannot name the run of its own commit.

## Base and process

Base `2f89574f76e21b5c5741e65f5041f2da0f72375f` (merge of PR #80, §29D). Its
post-merge CI was still running when this slice started, so it is not claimed
complete here. Branch `document-session-save`; one PR; nothing is merged by the
author.

## What changed

* `ferritecad-document`: `Document::model_version` (logical content with only
  `meta.modified_at` set aside; `content_version` is byte-for-byte unchanged).
* `ferritecad-jobs`: `session.rs` (`DocumentSession`: private immutable snapshots,
  bounded history, two-phase steps, Undo/Redo, dirty by comparison) and `save.rs`
  (Save/Save As with a version guard, sidecar advisory lock, atomic publication,
  typed outcomes, race hooks).
* `ferritecad-app`: `sessions.rs` (the window's client of the session, its
  workers and the headless state tests), wiring in `main.rs`, `dialogs.rs`,
  `edits.rs`, `exports/stl.rs`.
* `ferritecad-ui`: toolbar, Activity line, Apply button, disabled-editor reason.
* No change to the CLI, its arguments, JSON, exit codes, the archive format,
  geometry, the kernel, the evaluator, the renderer or any other editor.

## Local results

Container: Linux, Open CASCADE 8.0.1 installed locally, a **local** PlaneGCS build
(not the pinned one), one cargo job at a time, `jobs = 2`, root user. These are not
the three-OS matrix.

* `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` clean; licence headers (407 files), the export
  boundary check (whose source greps the window must keep true: the first
  column-zero `#[cfg(test)]` in `main.rs` is the test module again), `git diff
  --check` and the YAML syntax of both workflows pass.
* Regression over document, jobs, eval, every CLI test file (the large STEP
  corpus and complex-FBX files excluded, as in earlier slices) and the whole app:
  **1237 passed, 2 failed, 1 ignored**. The two failures are
  `validate::validation_really_read_only_permissions` and
  `read_only_permissions_still_dump_when_the_file_can_be_read` in the CLI's
  untouched read-only-permission tests, which cannot hold for uid 0 (root can
  read and write a read-only file). They are not counted as passed and are not
  claimed; non-root CI runs them.
* The packed steps were extracted from the workflows and executed: the all-OS
  step on Linux in the stub build (its `exit 0`, every gate named and `ok`);
  the native step against the local Open CASCADE and PlaneGCS (three native gates
  `ok`, no `skipped:`); the no-solver tail against the Open CASCADE-without-PlaneGCS
  build (the refusal and the plain plate `ok`). The local runs used the debug
  profile; the workflows use `--release`, which was not run here.
* Not run here: Windows, macOS, the pinned PlaneGCS, `--release`, a real window.

## Review fixes — local results

Fixes after the independent review of `15a960c`, each with a regression test that
fails on the previous code (the first four were run against it): the alias/lock
key, a foreign lock-named file (file, empty, wrong header, link, dangling link),
late read-only / hard link / re-pointed link, a stale lock of ours, a lock name
replaced while held, Save As into the private folder, Cancel before binding for
Apply/Undo/Redo, old-editor folder and name with a copy surviving the old session
(through adoption and Drop), a Save in flight across adoption, an export's
working copy kept alive across adoption and a pending replace-question, a public
export into the working folder refused (FBX and STL), and the height form's copy
availability computed by the window's own `height_state`. Directed mutations
M30A-3 (adopt keeps the old operation) and M30A-4 (copy gated on the form being
closed) fail the named tests. Workspace clippy `-D warnings`, fmt, the export
boundary check and licence headers pass; app, jobs and UI tests pass locally.
The all-OS step was run as packed on Linux against the Open CASCADE build
(52 gates `ok`, `exit 0`); the stub-only refusal ran earlier in the stub build.
`App::guard` itself (a dialog) is not driven headlessly: its overlap rule is the
existing `settled()` predicate plus the Sessions-level interleaving test.

## Mutations — local, executed, restored

Each mutation was applied to a committed tree, the named tests were run, and the file
was restored (`git diff` clean afterwards). A mutation that failed to compile or
failed on a missing fixture would not count; both failed at an executed assertion.

| Mutation | Result |
|---|---|
| **M30A-1** `Sessions::export_source` returns the logical path (the file on disk) instead of the current private snapshot | `native_an_ordinary_plate_…` and `native_a_chamfered_plate_…` fail at `assertion 'left != right' failed` (the export source equals the user's file) |
| **M30A-2** a failed Save moves the saved checkpoint to the current version | `save_moves_the_checkpoint_only_when_the_file_was_published` fails (`a refused Save moved the checkpoint`) and `a_cancelled_save_changes_nothing_and_says_so` fails (`assertion failed: sessions.dirty()`) |

## What the gates measure

* `tests/session.rs` (8): one state machine from Open to Save; editing back to the
  saved value is clean and a no-op is not a step; a new step after Undo drops Redo
  only after it succeeded; history is bounded oldest-first and its files go with it;
  a failed, stale or dropped step changes nothing and leaves no file; the session
  removes everything it made and a holder keeps its version; a document that cannot
  be read creates no directory; a reader keeps a version through eviction and Undo.
* `tests/save.rs` (10, nine off Unix): an externally modified file is never
  replaced; a replaced, missing or foreign file is a typed conflict; Save As refuses
  an occupied path; a read-only file is refused by the session's own check (so an
  account that could have written it, such as root, is not a skip); a symlink is
  saved through and a hard-linked file is refused (Unix); a change between the first
  compare and the lock is caught by the second; two cooperating savers are
  serialised and the second never overwrites; a writer that ignores the lock can win
  the instant before the rename (documented, not claimed closed); cancellation is
  honoured until the rename and a late one is a success; a failed save leaves the
  session and the directory as they were. Each ends by listing the directory: no
  scratch, lock or temporary file is left behind.
* `sessions::tests` (app, headless): a version is current only when its picture is
  shown; a failed, stale or cancelled Apply changes nothing; a cancelled edit that
  still finished is dropped and leaves no file; a picture that cannot be shown
  drops the version; a no-op Apply is reported and adds nothing; Undo/Redo move only
  when their picture is shown; the checkpoint moves only when a file was published;
  replacing the document asks only when something would be lost; the name is the
  user's, never the private file; stopping joins the worker and removes every
  private file. Plus the chord function, the title, the dialog result mapping, the
  height form and the document commands on screen.
* Native (Open CASCADE and PlaneGCS), each against the real peer CLI process:
  `native_an_ordinary_plate_…` and `native_a_chamfered_plate_with_constraints_…`
  Apply, Undo, Redo, an unsaved export and Save and compare, cell for cell (only
  `meta.modified_at` set aside), with `edit-extrude`; the exports are byte-equal to
  `export-stl`/`export-fbx` of the peer result; every topology reference and every
  sketch constraint of the chamfered plate survives byte for byte; the SQL differs
  from the source only in the Extrude row's payload and hash; the user's file is
  not touched until Save; `native_open_draws_from_the_sessions_own_copy_…` opens a
  document and draws it from the session's private copy (not the user's path),
  with the same catalogue as a direct read, leaves nothing behind once the session
  is dropped, and a broken or absent file creates no private directory.
* Refusal builds, executed in their own builds (a skipped test is not counted):
  `without_a_kernel_an_apply_is_refused_…` in the stub build and
  `without_the_solver_an_apply_to_a_constrained_plate_is_refused_…` in the Open
  CASCADE-without-PlaneGCS build. Both leave the document clean and the draft in
  place.

## macOS fixture and window scenario (for Codex on the Mac)

Nothing in this section was run in a window; the container has none. The generator
and the comparator use only the CLI. They were exercised with a script standing in
for the window (CLI calls and file copies), which says nothing about the window.
The stand-in run printed `FCAD_30A_GUI_COMPARE_OK`. Controls on stand-ins: an
unsaved export made from the file on disk instead of the accepted model
(`the unsaved export is not the accepted model`); an Apply that wrote the user's
file (`Apply wrote the user's file`); a foreign object row changed in a saved file
(`constrained Save: objects differs`); a missing window file
(`FCAD_30A_GUI_COMPARE_MISSING gui-unsaved.fbx …`, nothing created). Inside the
comparator, four further controls must reject, or it stops with
`FCAD_30A_GUI_COMPARE_CONTROL_PASSED`: the original height's export compared with
the unsaved one, a save made at a different height, a foreign row change against
the allowlist, and an absent output.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon Mac with a fresh arm64 bundle, and do not set
`FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `source/plain.fcad` (a plate 37.5 × 12.25 × 6.75 mm, drawn clockwise from
the upper right, offset and fractional: V0 (33, 15.5), V1 (33, 3.25), V2 (−4.5,
3.25), V3 (−4.5, 15.5)), `source/constrained.fcad` (the same plate with one 2.375 mm
Chamfer at V1 and every Line horizontal or vertical, V0 pinned, width 37.5, depth
12.25), byte copies of both in `work/`, and `facts.json`. Only `work/` is for the
window; `source/` is the comparator's reference and must not be opened in it.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/document-session-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_30A_GUI_FIXTURE\n", "ferrite-30a-fixture.py"),
                   ("# FCAD_30A_GUI_COMPARE\n", "ferrite-30a-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_30A_DIR="$PWD/document-session-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-30a-fixture.py "$FCAD_30A_DIR"
```

```python
# FCAD_30A_GUI_FIXTURE
import hashlib, json, os, pathlib, shutil, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "document-session-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
(out / "source").mkdir()
(out / "work").mkdir()
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# Two documents the window opens, offset and fractional. Plain: V0 (33, 15.5),
# V1 (33, 3.25), V2 (-4.5, 3.25), V3 (-4.5, 15.5); 37.5 x 12.25 mm, 6.75 mm tall.
# Constrained: the same plate with one equal-distance Chamfer (2.375 mm) at V1 and
# every Line horizontal or vertical, V0 pinned where it is, width 37.5, depth 12.25.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
H0, DIST = 6.75, 2.375
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H0, "points_mm": PLATE}))
plain = out / "source" / "plain.fcad"
run("create-sketch-extrude", request, "-o", plain, "--json")
catalog = run("inspect", plain, "--json")["result"]
candidate = next(c for c in catalog["bodies"][0]["chamfer_edge"]["target"]["candidates"]
                 if c["corner_mm"] == PLATE[1])
request.write_text(json.dumps({"request_version": 1, "edge": candidate["edge"], "distance_mm": DIST}))
chamfered = out / "chamfered.fcad"
run("chamfer-edge-copy", plain, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
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
chamfered.unlink()
facts = {}
for name in ("plain", "constrained"):
    r = run("inspect", out / "source" / f"{name}.fcad", "--json")["result"]
    (feature,) = r["features"]
    facts[name] = {"feature_id": feature["feature_id"], "body_id": r["bodies"][0]["body_id"],
                   "height_mm": feature["distance_mm"], "content_version": r["content_version"],
                   "sha256": hashlib.sha256((out / "source" / f"{name}.fcad").read_bytes()).hexdigest()}
    shutil.copyfile(out / "source" / f"{name}.fcad", out / "work" / f"{name}.fcad")
assert facts["plain"]["height_mm"] == H0
facts["plate"] = {"x0": -4.5, "y0": 3.25, "width": 37.5, "depth": 12.25, "chamfer_mm": DIST}
facts["heights_mm"] = {"plain_saved": 9.5, "plain_as": 11.25, "plain_unsaved": 12.5,
                       "plain_external": 3.0, "constrained": 8.5}
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_30A_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_30A_DIR/../watch-30a.jsonl" --limit-mib 1536 --seconds 2400 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

If system memory pressure aborts or stops the run, it does not count; start again
from a fresh fixture directory. **Do not address the viewer after Quit through the
automation** (no `getApp`, no `getAXState`, no restart): read only that PID's exit
and the watchdog log. `cp` below means a shell `cp` made at that moment, outside
the window. Type numbers exactly as written. Heights: first `9.5`, then `11.25`,
`12.5`, and `8.5` on the chamfered plate.

1. **Open** `$FCAD_30A_DIR/work/plain.fcad`. Title `plain.fcad — FerriteCAD`, no
   marker; Undo and Redo are disabled.
2. **Apply.** **Edit extrusion…**, type `9.5`, **Apply**. No file dialog appears.
   The plate is redrawn at 9.5 mm; the title is `*plain.fcad — FerriteCAD`; the
   panel says the file on disk is unchanged. `cp work/plain.fcad
   gui-plain-after-apply.fcad`.
3. **Unsaved export.** Export STL → `$FCAD_30A_DIR/gui-unsaved.stl`, FBX →
   `gui-unsaved.fbx`. The marker stays.
4. **Undo / Redo.** `Cmd+Z`: the 6.75 mm plate, no marker, Redo enabled. `Cmd+Shift+Z`:
   9.5 mm, marker back.
5. **Save.** `Cmd+S`: no dialog, the marker goes. `cp work/plain.fcad
   gui-plain-saved.fcad`.
6. **Save As.** Apply `11.25`, `Cmd+Shift+S` → `$FCAD_30A_DIR/work/plain-as.fcad`. The
   title names `plain-as.fcad`, no marker. `cp work/plain-as.fcad gui-plain-as.fcad`;
   `cp work/plain.fcad gui-plain-after-saveas.fcad`.
7. **Occupied.** Apply `12.5`, `Cmd+Shift+S`, choose `work/plain.fcad` and confirm any
   system replace prompt. The window refuses (Save As does not replace), the marker and
   the draft stay. `cp work/plain.fcad gui-plain-after-occupied.fcad`.
8. **External change.** In a shell: `ferritecad edit-extrude work/plain-as.fcad
   --feature <plain.feature_id from facts.json> --distance-mm 3 -o work/ext.fcad
   --expect-version <its content version>` and `mv work/ext.fcad work/plain-as.fcad`.
   `cp work/plain-as.fcad gui-external.fcad`. Then `Cmd+S`: refused as changed
   outside FerriteCAD; the marker stays and the 12.5 mm plate is still shown.
   `cp work/plain-as.fcad gui-plain-after-conflict.fcad`.
9. **Close with unsaved changes.** Close the window: Save / Discard / Cancel.
   **Cancel** keeps the window and the marker. Close again → **Discard**: the window
   closes without writing.
10. **Constrained plate.** Start a **second** viewer run the same way (a fresh
    watchdog log, `watch-30a-2.jsonl`), **Open** `work/constrained.fcad` (Extrude →
    Chamfer, constraints present), **Edit extrusion…** `8.5` → **Apply**, export STL →
    `gui-constrained.stl`, FBX → `gui-constrained.fbx`, `Cmd+S`. `cp
    work/constrained.fcad gui-constrained-saved.fcad`.
11. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-30a-compare.py "$FCAD_30A_DIR"
```

The comparator requires every `gui-*` file (and never creates one), checks both
sources by SHA-256, and then: the user's file after Apply, after Save As, after the
refused Save As and after the refused Save is byte-identical to what it should be;
what Save and Save As wrote has every SQL cell of the peer `edit-extrude` result
(only `meta.modified_at` set aside) and differs from its source only in the Extrude
row's payload and hash; the same model is reached directly and through two steps;
the chamfered plate's topology references and every other object row are unchanged;
every saved file opens cold (`validate`, `inspect`); the window's exports are
byte-equal to `export-stl`/`export-fbx` of the peer result, and its STL is read
independently — closed, one winding, the plate's bounds and the exact volume
(`W·D·h`, and `(W·D − d²/2)·h` under the Chamfer). It then runs the negative
controls described above and prints `FCAD_30A_GUI_COMPARE_OK cells=N triangles=N`.

```python
# FCAD_30A_GUI_COMPARE
import hashlib, json, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-plain-after-apply.fcad", "gui-unsaved.stl", "gui-unsaved.fbx", "gui-plain-saved.fcad",
       "gui-plain-as.fcad", "gui-plain-after-saveas.fcad", "gui-plain-after-occupied.fcad",
       "gui-external.fcad", "gui-plain-after-conflict.fcad", "gui-constrained-saved.fcad",
       "gui-constrained.stl", "gui-constrained.fbx")
def require(directory):
    missing = [n for n in GUI if not (directory / n).is_file()]
    assert not missing, f"FCAD_30A_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first"
try:
    require(out)
except AssertionError as error:
    sys.exit(str(error))
for name in ("plain", "constrained"):
    assert hashlib.sha256((out / "source" / f"{name}.fcad").read_bytes()).hexdigest() == facts[name]["sha256"], \
        f"source/{name}.fcad changed"
work = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-30a-compare-"))
H = facts["heights_mm"]
P, C, PLATE = facts["plain"], facts["constrained"], facts["plate"]
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
        got[t] = (columns, sorted([None if i == stamp else v for i, v in enumerate(r)] for r in rows))
        got[t] = (columns, sorted(got[t][1], key=repr))
    return got
def same_cells(a, b, why):
    x, y = cells(a), cells(b)
    for t in sorted(set(x) | set(y)):
        assert x.get(t) == y.get(t), f"{why}: {t} differs ({a.name} vs {b.name})"
def version(path):
    return run("inspect", path, "--json")["result"]["content_version"]
def edit(source, feature, height, dest):
    run("edit-extrude", source, "--feature", feature, "--distance-mm", str(height),
        "--expect-version", version(source), "-o", dest, "--json")
    return dest
def allowlist(before, after, row):
    """Only the one Extrude row's payload and hash, and the stamp, moved; every
    other row of every table (payloads, hashes, schema versions, topology refs,
    constraints) is the same, and no row was added or lost."""
    rid = bytes.fromhex(row.replace("-", ""))
    a, b = tables(before), tables(after)
    assert a.keys() == b.keys()
    moved = 0
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        assert len(arows) == len(brows), f"{t}: a row was added or lost"
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u != v:
                    moved += 1
                assert u == v or (t == "objects" and c in ("payload", "payload_hash")
                                  and x[ac.index("id")] == rid) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c} moved"
    assert moved >= 2, "the Extrude row did not change"
def mesh(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    return [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)] for i in range(count)]
def measure(stl, height, chamfered):
    """The mesh is read here, independently of the B-Rep: one closed oriented
    surface, the plate's bounds and the exact analytic volume."""
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
    x0, y0, w, d = PLATE["x0"], PLATE["y0"], PLATE["width"], PLATE["depth"]
    for got, want in ((min(xs), x0), (max(xs), x0 + w), (min(ys), y0), (max(ys), y0 + d),
                      (min(zs), 0.0), (max(zs), height)):
        assert abs(got - want) < 1e-4, ("bounds", got, want)
    cut = PLATE["chamfer_mm"] ** 2 / 2 if chamfered else 0.0
    exact = (w * d - cut) * height
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
    sys.exit(f"FCAD_30A_GUI_COMPARE_CONTROL_PASSED {label}: the comparison cannot tell a wrong result")
def read(name):
    return (out / name).read_bytes()
src_plain, src_constrained = out / "source" / "plain.fcad", out / "source" / "constrained.fcad"

# 1. The user's file is untouched by Apply, Undo, Redo and a refused Save/Save As.
assert read("gui-plain-after-apply.fcad") == src_plain.read_bytes(), "Apply wrote the user's file"
assert read("gui-plain-after-saveas.fcad") == read("gui-plain-saved.fcad"), "Save As changed the original"
assert read("gui-plain-after-occupied.fcad") == read("gui-plain-saved.fcad"), "a refused Save As wrote"
assert read("gui-plain-after-conflict.fcad") == read("gui-external.fcad"), "Save overwrote an external change"

# 2. What Save wrote is what the command line makes: same model, every cell but the stamp.
peer_a = edit(src_plain, P["feature_id"], H["plain_saved"], work / "peer-a.fcad")
peer_b = edit(peer_a, P["feature_id"], H["plain_as"], work / "peer-b.fcad")
direct_b = edit(src_plain, P["feature_id"], H["plain_as"], work / "direct-b.fcad")
same_cells(out / "gui-plain-saved.fcad", peer_a, "Save")
same_cells(out / "gui-plain-as.fcad", peer_b, "Save As")
same_cells(peer_b, direct_b, "history does not change the model")
allowlist(src_plain, out / "gui-plain-saved.fcad", P["feature_id"])
allowlist(out / "gui-plain-saved.fcad", out / "gui-plain-as.fcad", P["feature_id"])
external = edit(out / "gui-plain-as.fcad", P["feature_id"], H["plain_external"], work / "external.fcad")
same_cells(out / "gui-external.fcad", external, "the external change")
peer_c = edit(src_constrained, C["feature_id"], H["constrained"], work / "peer-c.fcad")
same_cells(out / "gui-constrained-saved.fcad", peer_c, "constrained Save")
allowlist(src_constrained, out / "gui-constrained-saved.fcad", C["feature_id"])
before, after = tables(src_constrained), tables(out / "gui-constrained-saved.fcad")
assert before["topology_refs"] == after["topology_refs"], "a name moved"
for t in before:
    if t != "objects":
        continue
    keep = lambda rows, columns: sorted(r for r in rows if r[columns.index("id")] != bytes.fromhex(C["feature_id"].replace("-", "")))
    assert keep(before[t][1], before[t][0]) == keep(after[t][1], after[t][0]), "another object changed"
assert version(out / "gui-plain-saved.fcad") != version(src_plain)
for name in ("gui-plain-saved", "gui-plain-as", "gui-constrained-saved"):
    run("validate", out / f"{name}.fcad", "--json")
    run("inspect", out / f"{name}.fcad", "--json")

# 3. The exports are the accepted model: the unsaved 9.5 mm plate, not the file's
# 6.75 mm, byte for byte what the command line exports for it; and the mesh is read here.
stl_a, fbx_a = exported(peer_a, "peer-a")
assert read("gui-unsaved.stl") == stl_a and read("gui-unsaved.fbx") == fbx_a, "the unsaved export is not the accepted model"
stl_c, fbx_c = exported(peer_c, "peer-c")
assert read("gui-constrained.stl") == stl_c and read("gui-constrained.fbx") == fbx_c, "the constrained export differs"
measure(out / "gui-unsaved.stl", H["plain_saved"], False)
measure(out / "gui-constrained.stl", H["constrained"], True)

# 4. Negative controls: each comparison above must reject the wrong thing.
original_stl, original_fbx = exported(src_plain, "original")
def equal(a, b):
    assert a == b, "different bytes"
must_fail("an export of the disk original", lambda: equal(original_stl, read("gui-unsaved.stl")))
must_fail("a different height in the save", lambda: same_cells(
    out / "gui-plain-saved.fcad", edit(src_plain, P["feature_id"], H["plain_saved"] + 0.25, work / "wrong.fcad"), "control"))
foreign = work / "foreign.fcad"
foreign.write_bytes(read("gui-plain-saved.fcad"))
db = sqlite3.connect(foreign)
db.execute("UPDATE objects SET payload_hash = zeroblob(length(payload_hash)) WHERE id != ?",
           (bytes.fromhex(P["feature_id"].replace("-", "")),))
db.commit()
db.close()
must_fail("a foreign object row changed", lambda: allowlist(src_plain, foreign, P["feature_id"]))
must_fail("a missing output", lambda: require(work))
must_fail("an old-height mesh", lambda: measure(work / "original.stl", H["plain_saved"], False))
print("FCAD_30A_GUI_COMPARE_OK", f"cells={sum(len(r) for _, r in tables(out / 'gui-plain-saved.fcad').values())}",
      f"triangles={len(mesh(out / 'gui-unsaved.stl'))}")
```

## CI

`ci.yml` (all three OSes): *Open, edit, Undo, Redo and Save one document without
native geometry* runs the 8 session, 9 (10 on Unix) Save, 1 model-version, 17 window and 2
interface gates by exact name and refuses a skip. `runtime-layout.yml`: *Edit, Undo,
Redo, export and Save one open document through the native window state* runs the
three native gates with both libraries required; the Open CASCADE-without-solver
step runs the solver refusal and the plain-plate gate. Run steps stay under the
21000-character limit. This record does not name the CI of its own head.

## Limits

Not run in the original cloud container: the window itself, macOS, Windows, a
second GPU or Cmd+Q. The later macOS review below supplies Quit evidence. The
lock-ignoring external-writer interval remains tested and documented, not closed;
crash recovery does not exist. History above the
bounds in a window (the bound is tested in the library). Milestone 5C, the older OOM
investigation and the general beta remain open.


## Independent macOS review follow-up (PR #81)

The reviewer reproduced the original alias-lock race, deletion of an unrelated
lock-name file, and acceptance after scene-phase cancellation with executed
assertion failures. The cloud corrections are code `1731ccf`, followed by docs
`44ed231`. Review then caught three further failures: an outward leaf symlink
inside the working folder bypassed the publication guard; a failed first advisory
lock left an empty, unrecoverable sidecar; cleanup removed a replacement symlink
that pointed back to the held inode. Each compiled and failed an executed test
before correction. The guard now checks both the resolved target and the location
of the directory entry, including filesystem identity of ancestors. A lock's
cleanup ownership starts at exclusive creation or validated header, and cleanup
requires a regular directory entry still naming the held inode. The existing
non-cooperating-writer interval is not claimed closed.

The macOS-only Quit adapter registers the previously absent
`applicationShouldTerminate:` delegate method, declines immediate AppKit
termination and queues the normal guarded exit. No delegate method is replaced,
no dependency added. On `15a960c` plus this isolated fix, a fresh arm64 bundle
under watchdog performed a real window run: Apply 6.75 → 9.5 mm, Cmd+Q → Cancel
kept the dirty title and left the source byte-identical; a second Cmd+Q → Save
closed with exit 0, and the bundled CLI reopened the saved 9.5 mm model. Owned
PID 35973, peak footprint 190.83 MiB, pressure normal, swap unchanged at 909 MiB.
The initial pre-test pressure level 2 was waited out without launching a viewer.
Evidence: `/private/tmp/ferrite-pr81-review/watch-quit.jsonl`, `quit-saved.json`,
`quit-prompt-ax.txt`, and the failing regression logs in that directory.

The focused Quit test above does not stand in for the complete scenario. The
subsequent runs and their exact code provenance are recorded below; remote CI
results are attached to the PR rather than inferred from these local runs.

The full window review additionally reproduced broken Cmd+S/Shift+S/Z/Shift+Z
with the active Russian keyboard layout. Instrumented winit events carried
logical `ы`/`я` but `text_with_all_modifiers` of `s`/`z`. Document commands now
use that macOS command mapping, including Shift; other platforms retain logical
keys. The existing chord gate first failed on these actual event values, then
passed after correction. Diagnostic logging was removed before publication.
Save As also updates the displayed logical filename, not only the title.

The solver-ownership check now exempts only `app/src/macos_quit.rs` from generic
C/unsafe spelling checks: that adapter links Objective-C/AppKit, not PlaneGCS.
The solver-symbol ban still covers the adapter; an injected `fc_gcs_` symbol was
rejected, restored, and the positive check repeated. Other app files retain the
old C/unsafe ban. This fixes the check failure on `fc5a397` without relocating a
platform callback into the solver crate.


### Completed window scenario and independent artifact comparison

The plain-polygon run used the complete review code `fc5a397`: Apply 6.75 →
9.5 mm left the source unchanged; document Undo/Redo changed the accepted scene;
STL/FBX exported the unsaved 9.5 mm model; Save wrote that model. Apply 11.25 mm
and Save As created a separate file and preserved the prior file. A later dirty
12.5 mm edit survived both an occupied Save As refusal (including the native
panel's Replace confirmation) and a Save conflict after the current file was
externally replaced with a 3 mm copy. Cmd+Q → Cancel preserved the dirty scene;
Cmd+Q → Discard exited normally. A clean height form's old Save-new-file panel
opened in the logical document directory and was cancelled; that check does not
claim a copy was published. An initial missed field focus produced an extra
9.5 mm Save As file; inspection caught it, and the intended 11.25 mm Apply/Save As
was repeated and independently measured. No incorrect artifact was substituted
by the comparator.

The final product code `95374ed` fixes the Command-key mapping and Save As label.
On the active Russian layout, the real window exercised Cmd+Shift+S → Cancel,
Cmd+Z → clean/Undone, Cmd+Shift+Z → dirty/Redone, and exported the unsaved
constrained Chamfer model at 8.5 mm to FBX. That run was subsequently stopped by
the watchdog on **system pressure level 2**, at a sampled viewer peak of
206.02 MiB, below the 1536 MiB cap. It is not counted as a completed scenario.
CUA automatically relaunched an empty instance; its exact PID was stopped.
After pressure returned to normal, a fresh watched process on the same final
code repeated Apply 6.75 → 8.5 mm, exported unsaved STL, saved with Cmd+S
(the star disappeared and Saved was visible), and quit cleanly. Coordinate
interaction worked in this retry; the earlier CUA `noWindowsAvailable` errors
had required keyboard navigation and were not attributed to the application.

The extracted `FCAD_30A_GUI_COMPARE` script ran with the final bundled CLI over
all twelve required GUI artifacts, returning
`FCAD_30A_GUI_COMPARE_OK cells=12 triangles=12`. It checked immutable original
hashes, every SQL table with the explicit changed-cell allowlist, saved heights,
Save As/source/conflict preservation, byte-equal CLI STL/FBX, independent closed
oriented STL bounds/volume, and the negative controls. The two actual GUI FBX
files were also read by freshly compiled pinned ufbx 0.23.0 in strict mode:
6 checks / 0 failures each. The constrained STL has 16 triangles / 884 bytes;
its constraints and Chamfer retain their stored identities.

Watchdog records (one owned viewer per run): plain scenario PID 54315, exit 0,
peak 269.63 MiB, pressure normal, swap unchanged at 909.06 MiB; final constrained
retry PID 78241, exit 0, peak 193.88 MiB, pressure normal, swap unchanged at
1480.56 MiB. The increased system swap **between** these runs is recorded, not
claimed as zero growth across the whole review. No viewer was queried through
CUA after the successful Quit; completion came from the watchdog's PID/exit.
The historical OOM cause remains unknown. No Windows/Linux window smoke or
crash recovery is claimed.

Review logs, scripts, model artifacts and memory summaries are retained under
`/private/tmp/ferrite-pr81-review/` and in the local review evidence directory
`~/.codex/visualizations/2026/09/05/01a0722f-a4b4-7532-b72b-07ef6b78698d/ferrite-pr81-review/`.
The measured binaries predate this documentation-only update.
