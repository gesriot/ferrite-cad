# §29C — verification record

[The contract and recipe](edit-chamfer-base-sketch.md).

## Base and scope

Base `main` = `15ad4d5adc631fefb07eb9f1700d055add971571`, tree
`61d329d9`… (PR #78, §29B merged). The branch is `edit-chamfer-base-sketch`.
§29B was not redone and `main` was not rolled back. The base's own post-merge CI
had completed `success` when this work began and is not mixed with this PR's
runs below. Milestone 5C is **not** declared complete.

## Where and how this was run

Local, in the cloud container: Linux, Open CASCADE 8.0.1 and PlaneGCS built from
local inputs — **the local PlaneGCS is not the pinned one**; the pinned
`planegcs pin` workflow and the three-platform runtime layout are
authoritative. macOS is Apple Silicon (arm64) only. No window, GPU or Unity ran
here. One heavy process at a time, 2 jobs; OCCT, Boost and PlaneGCS were not
rebuilt. Disk was measured before heavy work and the build caches (`incremental`
directories) of this session's own targets were removed to make room.

## What changed

The existing `edit-sketch-copy` and **Edit saved Sketch — new copy** accept the
four stored vertices of the base Sketch under the one §29A Chamfer. The class is
read by the distance edit's own reader (`saved_chamfer`); `coordinate_choice`
carries it, `SketchChoice::validate_coordinates` is the one validator the form,
the job and the writer call, and `write_sketch_geometry` re-derives the prepared
payload inside its transaction as it did. There is no new command, copier,
request format, payload, capability or archive/SQLite version. The only wire
addition is `sketches[].chamfer_base`. The refusal of every other Sketch editor
of a chamfered plate is unchanged (`refuse_chamfered` stays; its text now says
the coordinates are editable).

Rules the validator applies on top of the existing polygon and winding policy:
the candidate is still an axis-aligned rectangle of four Lines; every Line keeps
its side (so the corner follows its vertex, and a half turn or a mirror is
refused); the saved joint is still a corner by the two Line UUIDs; and the
saved distance fits the new adjacent sides under the §29A expression — exact at
the bound, never reduced.

## New tests

- Document (`chamfer::tests`, 2 new): a candidate's every rule (the exact bound
  and the next float below it on both adjacent sides, a half turn, a mirror, a
  degenerate and a crooked plate, a reordered one), only the Sketch's row moves
  on three accepted edits (other payloads, references and dependencies equal,
  the distance never reduced, the joint unchanged), and the writer refusing a
  forged payload (a vertex off the plate, a plate too shallow for the distance)
  and a stale prepared payload with nothing written.
- CLI (`tests/fillet/base_sketch.rs`, 5): every corner in three drawing orders
  made larger and moved (B-Rep: volume, plane, outward normal, area at the chosen
  UUID corner of the NEW plate; independently read STL; SQL allowlist: only the
  Sketch's payload and hash and the stamp; refs, objects and UUIDs equal; source
  byte-identical; cold rebuild resolves every name), then for one corner per
  order smaller, only moved, only resized, twice, the real cache Miss/Hit (the
  old plate's entry is not returned for the new one and the old copy still hits
  its own) and the later height and distance edits on the edited plate (the
  distance bound follows the NEW sides); the chosen corner against another with
  the same volume (the other corner's mesh test, the old coordinates' test and
  the corner above must fail); the exact bound and every refused candidate
  (just below the bound on each adjacent side, zero depth, mirror, flip, half
  turn, not a rectangle, reordered, foreign Line, missing Line, a non-finite
  number) with the Chamfer named, the directory and source untouched; refusals,
  races and guards (stale version, no clobber, alias, foreign Sketch, closed
  pipe → exit 7) and an extra resolving base reference that is kept against an
  unresolved one that is never published; discovery and the protocol without a
  kernel.
- App (3): the form names the Chamfer, its corner and distance and the smallest
  side; a half-typed draft is refused honestly with Undo/Redo/Restore alive and
  no request until valid; Save Cancel, a stale reply and a worker refusal keep
  the draft; the worker's copy and the peer CLI's are SQL-cell-equal with
  byte-equal STL/FBX and a failed async Open restores the draft; a drag is one
  Undo step, moves no neighbouring vertex, and Restore returns the saved plate.
- Flipped, not weakened: the §29A/§29B assertions that the Sketch editor refuses a
  chamfered plate now assert it is editable (`editable`, `chamfer_base`); the
  constraint editor's refusal by name is kept and now asserted through
  `edit-sketch-constraints-copy`. The §29B rule that every saved reference must
  resolve (`native_chamfer_height_requires_every_saved_reference_to_resolve`) is
  unchanged and still passes.

## Local results

Full regression (document, jobs, eval, every CLI test target except the large
STEP corpus targets, app, solver info; planegcs, one run, after the last code
change but the one clippy fix below): 1184 passed, 1 old ignored benchmark, 2
failed — the same two tests that need a read-only file to be unreadable and fail
as root in this container (`read_only_permissions_still_dump_when_the_file_can_be_read`,
`validation_really_read_only_permissions`; they fail on `main` here too). The two
mutations below were each restored before this run. `cargo fmt --check`,
workspace clippy (`--all-targets --features planegcs -- -D warnings`), the
licence-header (401 files) and export-boundary scripts and `git diff --check` are
clean; clippy found one collapsible `if` in a new test, fixed in its own commit,
after which the 12 `chamfer::` document tests were run again. `actionlint` is not
installed here; both edited workflows were parsed as YAML, and no `run:` step is
over GitHub's 21 000-character limit (the Chamfer native step is 8.6 kB, the
no-solver one 2 kB, the stub one 5.7 kB).

Packed argv, executed:

- The stub step of `ci.yml` ("Discover and refuse one Chamfer without native
  geometry") was extracted and run against a real no-kernel build
  (`CARGO_TARGET_DIR=/home/user/stub-target`, no `OpenCASCADE_DIR`): every gate
  `ok` and `FCAD_29A_RECIPE_NO_KERNEL`, `FCAD_29B_RECIPE_NO_KERNEL`,
  `FCAD_29C_RECIPE_NO_KERNEL`. **A stub's "unsupported" is not claimed as proof
  of the geometry guard**: the guard is proved by the document-layer and native
  gates, which compare numbers and bytes.
- The Chamfer step of `runtime-layout.yml` ("Chamfer one vertical edge of a saved
  plate into a named flat") was extracted and run on the native debug build (the
  flags `--release`, library paths and artifact directory adapted; nothing else):
  exit 0, every named gate `ok` (five new CLI gates, two new document gates,
  three new app gates), `FCAD_29A_RECIPE_OK`, `FCAD_29B_RECIPE_OK`,
  `FCAD_29C_RECIPE_OK`, 72 FBX/STL artifacts written.
- Pinned ufbx: the `check-fbx-complex.sh` Chamfer loops, including the new nine
  Sketch pairs (`sketch-{0,1,2}-{big,small,shifted}`), were run against those
  artifacts: `FCAD_CHAMFER_UFBX_EXECUTED`, `FCAD_CHAMFER_HEIGHT_UFBX_EXECUTED`,
  `FCAD_CHAMFER_SKETCH_UFBX_EXECUTED`; each new file read twice by ufbx (`checks=6
  failures=0`) and joined with its STL (`FCAD_STL_FBX_MATCH triangles=16`, worst
  error 6.9e-18 m). (Run with those loops alone: the earlier slices' artifacts
  were not regenerated here, and their loops need all of them.)
- OCCT without the solver (debug, `--no-default-features`,
  `FERRITECAD_REQUIRE_PLANEGCS=0`, `FCAD_PLANEGCS_DIR` unset): the extracted
  "Chamfer a plate with Open CASCADE and no solver" step ran all three recipes
  (`FCAD_29A/B/C_RECIPE_OK`) and four new native gates and two old ones
  (including the constraint editor refusing by the Chamfer's UUID) passed. This
  class needs no solver, and that is shown, not assumed.
- All twelve recipes 28A–28L and 29A, 29B and 29C ran against this build: all
  `…_RECIPE_OK`.

### Mutations — local, executed, restored

Each compiled, failed on an executed assertion (not a compile error and not zero
tests) and was restored byte for byte (`git status` clean), after which the
positive gates passed again:

1. **The wrong corner / a stale downstream corner.** The side rule is dropped from
   the Chamfer branch of the coordinate validator
   (`keeps_every_side(...)` → `Ok(())`): a plate turned half a turn keeps its
   winding and the saved joint is still found by its two Line UUIDs, so the
   Chamfer would silently land on the opposite corner of the part. Failed:
   `a_chamfered_plates_sketch_keeps_its_corner_and_distance_exactly` (document) and
   `native_sketch_bounds_and_candidates_are_exact_under_a_chamfer` (CLI, "a plate
   turned half a turn"). No equivalent mutation was left standing: an earlier idea
   (choose the corner by index in `corner_on`) is *equivalent* for the bound — all
   four corners of a rectangle have the same two adjacent sides — and was replaced
   by this one.
2. **Bypass of the transactional re-derivation.** `write_sketch_geometry` accepts
   the prepared payload without re-deriving it from the document
   (`let checked = prepared.clone()`): a forged payload (a vertex off the plate)
   and a plate too shallow for the saved distance are written. Failed:
   `the_coordinate_writer_rederives_under_a_chamfer_and_refuses_forgery`
   (`assertion failed: d.write_sketch_geometry(&forged).is_err()`).

The cache-key mutation of §29B (the Chamfer's key ignoring the plate under it) was
not repeated: the Sketch edit changes the profile the Extrude's and, through it,
the Chamfer's key follow, and the new cache gate would fail the same way
(`the new plate must not hit the old one`); it is asserted, not re-mutated.

### Compatibility with the reader on `main`

`main` (`15ad4d5`) built with the same solver, against a copy this build made
(`FCAD_29C_COMPAT_OK`): `main`'s own `edit-sketch-copy` of its chamfered plate is
refused naming the Chamfer; the copy this build makes opens in `main`:
`inspect` works (without `sketches[].chamfer_base`, which it does not know) and
still reports the Sketch not editable, `validate` is valid, `rebuild --cold`
resolves every name, `export-stl` and `export-fbx` are byte-identical to this
build's, `main`'s `edit-chamfer-distance` works on it, and the copy's bytes are
unchanged.

## CI

Pull request [#79](https://github.com/gesriot/ferrite-cad/pull/79), **code head**
`4dbadc936f49c5ee036f11572c72d03e87ef67b2` (later commits change only documents).
These are the PR's runs, not post-merge runs of `main`; the base `15ad4d5`'s own
post-merge runs had completed `success` before this work began.

- Ordinary CI [36995498706](https://github.com/gesriot/ferrite-cad/actions/runs/36995498706): success (includes the stub step with the §29C discovery/protocol, widget, drag and document gates and `FCAD_29C_RECIPE_NO_KERNEL`).
- Combined runtime layout [36995480290](https://github.com/gesriot/ferrite-cad/actions/runs/36995480290): success on Linux, macOS and Windows plus the comparison (the OCCT-without-solver step with `FCAD_29C_RECIPE_OK`, the native Chamfer step with the new `base_sketch` CLI, document and app gates and the recipe, and the pinned-ufbx step with `FCAD_CHAMFER_SKETCH_UFBX_EXECUTED`). The Windows job took about 1 h 28 min; it was not counted as passed while it ran.
- planegcs pin [36995480272](https://github.com/gesriot/ferrite-cad/actions/runs/36995480272): success.

Only run and job conclusions were read; the full job logs were not downloaded, so
per-step test counts are an inference from the gates (each fails its job unless its
exact test passed and each marker was grepped). **Final docs head:** the CI of the
commit that carries this section is reported in the PR and not here, since a
document cannot name the run of its own commit.

## Limits

Out of scope and refused: a dimension or any non-closure constraint on the plate
(the constraint editors still name the Chamfer), a second Chamfer, a Fillet or a
Cut beside it, another edge, ThroughAll, an arbitrary plane, in-place Save and
live preview. No automatic moving of a neighbouring vertex: a half-typed draft is
refused honestly until the whole candidate is a plate. The local PlaneGCS is not
the pinned one. No window ran here, and the headless app tests are not a window
check. The memory exhaustion seen in earlier macOS runs is still unexplained.
Milestone 5C is not complete.

## macOS fixture and window scenario (for Codex on the Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised with CLI calls
standing in for the window, which says nothing about the window:
`FCAD_29C_GUI_COMPARE_OK cells=512 triangles=16`. Controls rejected: a size of
41.5 instead of 41.0 (the corner did not follow its vertex), a moved plate 10.5
deep instead of 10.25 (the plate as typed), a changed `objects.name` cell in a
window copy (`objects.name moved`), a `gui-refused.fcad`, a changed source
byte, a missing set of window files (`FCAD_29C_GUI_COMPARE_MISSING …`, nothing
created), and — on the independent STL reader alone, with the SQL and
byte-equality checks bypassed — the mesh of a Chamfer at another vertex of the
same plate, of the right vertex of a plate 0.25 mm deeper, and of the right
vertex of the old plate position.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon Mac, with a fresh arm64 bundle, and do not set
`FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `chamfer.fcad` — a free plate, 37.5 × 12.25 × 6.75 mm, drawn clockwise
from the upper right, offset and fractional, vertices V0 (33, 15.5), V1 (33,
3.25), V2 (−4.5, 3.25), V3 (−4.5, 15.5), with one Chamfer at V1, d 2.375 mm —
and `facts.json`.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-chamfer-base-sketch-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_29C_GUI_FIXTURE\n", "ferrite-29c-fixture.py"),
                   ("# FCAD_29C_GUI_COMPARE\n", "ferrite-29c-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_29C_DIR="$PWD/chamfer-sketch-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-29c-fixture.py "$FCAD_29C_DIR"
```

```python
# FCAD_29C_GUI_FIXTURE
import hashlib, json, math, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "chamfer-sketch-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# A free plate, clockwise from the upper right, offset and fractional, no
# constraint: V0 (33, 15.5), V1 (33, 3.25), V2 (-4.5, 3.25), V3 (-4.5, 15.5);
# 37.5 mm wide, 12.25 mm deep, 6.75 mm tall. One equal-distance Chamfer at V1,
# d 2.375 mm. The window resizes the plate, moves it, then changes its height and
# the Chamfer's distance; the Chamfer stays on the same vertex throughout.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
H0, H1, D1, D2 = 6.75, 9.5, 2.375, 4.5
RECTS = {"size": [-4.5, 3.25, 41.0, 10.25], "moved": [8.5, -6.75, 41.0, 10.25]}
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H0, "points_mm": PLATE}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
catalog = run("inspect", plate, "--json")["result"]
candidate = next(c for c in catalog["bodies"][0]["chamfer_edge"]["target"]["candidates"]
                 if c["corner_mm"] == PLATE[1])
request.write_text(json.dumps({"request_version": 1, "edge": candidate["edge"], "distance_mm": D1}))
source = out / "chamfer.fcad"
run("chamfer-edge-copy", plate, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", source, "--json")
request.unlink()
plate.unlink()
c = run("inspect", source, "--json")["result"]
(base,) = c["features"]
(chamfer,) = c["chamfers"]
(sketch,) = c["sketches"]
assert sketch["editable"] is True and sketch["chamfer_base"]["chamfer_feature_id"] == chamfer["feature_id"]
assert [v["start_mm"] for v in sketch["vertices"]] == PLATE
facts = {
    "body_id": c["bodies"][0]["body_id"], "base_feature_id": base["feature_id"],
    "chamfer_id": chamfer["feature_id"], "edge": chamfer["edge"], "sketch_id": sketch["sketch_id"],
    "vertices": [{"curve_id": v["curve_id"], "start_mm": v["start_mm"]} for v in sketch["vertices"]],
    "plate_mm": [-4.5, 3.25, 37.5, 12.25], "corner_mm": PLATE[1],
    "rects_mm": RECTS, "height_mm": {"source": H0, "final": H1},
    "distances_mm": {"source": D1, "final": D2},
    # What the window is asked to type and which refusal it must show.
    "refused_depth_typed_mm": 5.5, "refused_needs_side_mm": D1 + 0.01,
    "content_version": c["content_version"],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_29C_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_29C_DIR/../watch-29c.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

If system memory pressure aborts or stops the run, it does not count; start
again from a fresh fixture directory. **Do not address the viewer after Quit
through the automation**: it may relaunch the app outside the watchdog. If it was
relaunched, quit that instance too and read only the original PID's exit and
watchdog record. Type numbers exactly as written; **Undo draft**, **Redo draft**
and **Restore saved vertices** are the existing buttons.

1. **Open** `$FCAD_29C_DIR/chamfer.fcad` (asynchronously). The feature list shows
   Extrude → Chamfer.
2. **Form.** **Edit Sketch Profile — <UUID>…** is enabled (before §29C a
   chamfered plate refused it). The form shows the four vertices above and
   "Chamfered by Chamfer <UUID> at the corner of Lines <UUID> | <UUID>, d 2.375
   mm. The Chamfer keeps its corner and distance: every Line keeps its side, and
   the shorter adjacent side may not be shorter than 2.385 mm."
3. **A half-typed plate.** Change only the first `15.5` Y field to `9`: the
   drawing is no longer a rectangle; the form says so, **Save edited copy…** is
   unavailable, and **Undo draft**, **Redo draft** and **Restore saved
   vertices** stay available. No other vertex moved.
4. **Bound refusal.** Change the second `15.5` Y field to `9` too (a plate 5.75
   mm deep: valid), then both upper Y fields from `9` to `5.5`: the refusal
   names "Chamfer <UUID> of 2.375 mm does not fit the new plate" and the bound;
   Save stays unavailable and the typed values stay in the draft.
5. **Undo/Redo/Restore.** **Undo draft** twice, **Redo draft** once, then
   **Restore saved vertices** (one step): the four saved vertices are back.
6. **Resize.** Type the plate 41 × 10.25 mm, keeping its lower left corner:
   V0 (36.5, 13.5), V1 (36.5, 3.25), V2 (−4.5, 3.25) unchanged, V3 (−4.5, 13.5).
   (Optionally drag one vertex on the canvas: the draft is no longer a plate and
   is refused; **Undo draft** removes the drag in one step.)
7. **Save Cancel.** **Save edited copy…** → **Cancel** in the file dialog:
   nothing is created (`gui-cancelled.fcad` never exists), the draft stays.
8. **Publication one.** **Save edited copy…** → `$FCAD_29C_DIR/gui-size.fcad`;
   it opens asynchronously. The Chamfer is still at the lower right vertex, now
   (36.5, 3.25), d 2.375 mm.
9. **Translation.** In `gui-size.fcad`, **Edit Sketch…** and type V0 (49.5, 3.5),
   V1 (49.5, −6.75), V2 (8.5, −6.75), V3 (8.5, 3.5) → **Save edited copy…** →
   `$FCAD_29C_DIR/gui-moved.fcad`; it opens. The Chamfer is at (49.5, −6.75).
10. **Height.** **Edit extrusion…**, the base Extrude, type `9.5` → **Save new
    file…** → `$FCAD_29C_DIR/gui-tall.fcad`; it opens.
11. **Distance.** **Edit Chamfer distance…**, **New distance** `4.5` → **Apply
    distance** → **Save distance copy…** → `$FCAD_29C_DIR/gui-distance.fcad`; it
    opens.
12. **Exports.** Export `gui-distance.stl` and `gui-distance.fbx` of
    `gui-distance.fcad` into `$FCAD_29C_DIR`, at the default tessellation.
13. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-29c-compare.py "$FCAD_29C_DIR"
```

It first requires `gui-size.fcad`, `gui-moved.fcad`, `gui-tall.fcad`,
`gui-distance.fcad`, `gui-distance.stl` and `gui-distance.fbx` (and never
creates them), refuses a `gui-refused.fcad` or `gui-cancelled.fcad`, checks the
source's SHA-256, makes the CLI peers, checks each window edit's allowlist (one
object row's payload and hash and the stamp; nothing minted), the same Chamfer
UUID and edge in every document with its corner at the vertex it was made at on
the plate as typed, the Line UUIDs and order, the saved names under the same
UUIDs and resolving after a cold rebuild, every SQL cell equal to the peer's
(nothing is mapped), byte-equal STL/FBX, and then reads the STL itself — closed,
one winding, the plate's bounds, the volume `(W·D − d²/2)·h`, the three whole
corners, the cut at the chosen vertex and its one flat (plane, outward normal,
area `d·√2·h`) — for the window's export and, through the CLI, for each saved
step. It prints `FCAD_29C_GUI_COMPARE_OK cells=N triangles=M`.

```python
# FCAD_29C_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-size.fcad", "gui-moved.fcad", "gui-tall.fcad", "gui-distance.fcad",
       "gui-distance.stl", "gui-distance.fbx")
missing = [n for n in GUI if not (out / n).is_file()]
if missing:
    sys.exit(f"FCAD_29C_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first")
for n in ("gui-refused.fcad", "gui-cancelled.fcad"):
    assert not (out / n).exists(), f"a refused or cancelled Save published {n}"
source = out / "chamfer.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
BASE, CHAMFER, BODY, SKETCH = facts["base_feature_id"], facts["chamfer_id"], facts["body_id"], facts["sketch_id"]
RECTS, H, D = facts["rects_mm"], facts["height_mm"], facts["distances_mm"]
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
def allowlist_row(before, after, row):
    """Exactly one object row's payload and hash and the stamp moved; nothing
    was minted, so no table gained or lost a row."""
    rid = bytes.fromhex(row.replace("-", ""))
    a, b = tables(before), tables(after)
    assert a.keys() == b.keys()
    moved = 0
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        assert len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u != v:
                    moved += 1
                assert u == v or (t == "objects" and c in ("payload", "payload_hash") and x[ac.index("id")] == rid) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c} moved"
    assert moved >= 2, "the selected row did not change"
def vertices_of(rect):
    """Clockwise from the upper right: V0 upper right, V1 lower right (the
    Chamfer's vertex), V2 lower left, V3 upper left."""
    x0, y0, w, d = rect
    return [[x0 + w, y0 + d], [x0 + w, y0], [x0, y0], [x0, y0 + d]]
def corner_of(rect):
    return vertices_of(rect)[1]
def stl_of(path, scratch):
    target = scratch / (path.stem + ".stl")
    run("export-stl", path, "-o", target, "--json")
    return target
def mesh_checks(stl_path, rect, height, distance):
    """The mesh is read here, independently of the B-Rep: closed, one winding,
    the plate's bounds, the volume, the three whole corners, the chosen corner
    (the vertex the Chamfer's Lines meet at, on the NEW plate) cut, and the one
    flat facing out of the plate with area d*sqrt(2)*h."""
    x0, y0, w, dep = rect
    corner = corner_of(rect)
    data = stl_path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    tri = [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)] for i in range(count)]
    pts = [q for t in tri for q in t]
    key = lambda v: tuple(round(x * 1e4) for x in v)
    directed = {}
    for a, b, c in tri:
        for u, v in ((a, b), (b, c), (c, a)):
            directed[(key(u), key(v))] = directed.get((key(u), key(v)), 0) + 1
    assert all(n == 1 for n in directed.values()), "not one oriented surface"
    assert all((v, u) in directed for u, v in directed), "the mesh is open"
    six = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2])
              + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in tri)
    xs, ys, zs = ([pt[k] for pt in pts] for k in range(3))
    for got, want in ((min(xs), x0), (max(xs), x0 + w), (min(ys), y0), (max(ys), y0 + dep), (min(zs), 0.0), (max(zs), height)):
        assert abs(got - want) < 1e-4, ("bounds", got, want)
    exact = (w * dep - distance * distance / 2) * height
    assert abs(six / 6 - exact) < 1e-3, ("volume", six / 6, exact)
    near = lambda c, z: any(abs(pt[0] - c[0]) < 1e-4 and abs(pt[1] - c[1]) < 1e-4 and abs(pt[2] - z) < 1e-4 for pt in pts)
    for c in vertices_of(rect):
        for z in (0.0, height):
            assert near(c, z) == (c != corner), ("corner", c, z)
    ix = -1.0 if abs(corner[0] - (x0 + w)) < 1e-9 else 1.0
    iy = -1.0 if abs(corner[1] - (y0 + dep)) < 1e-9 else 1.0
    for z in (0.0, height):
        assert near([corner[0] + ix * distance, corner[1]], z) and near([corner[0], corner[1] + iy * distance], z), z
    flat = 0.0
    for a, b, c in tri:
        if all(abs(ix * (v[0] - corner[0]) + iy * (v[1] - corner[1]) - distance) < 1e-4 for v in (a, b, c)):
            u = [b[i] - a[i] for i in range(3)]
            v_ = [c[i] - a[i] for i in range(3)]
            n = [u[1] * v_[2] - u[2] * v_[1], u[2] * v_[0] - u[0] * v_[2], u[0] * v_[1] - u[1] * v_[0]]
            length = math.sqrt(sum(x * x for x in n))
            assert (-ix * n[0] - iy * n[1]) / length / math.sqrt(2) > 1 - 1e-4, "the flat faces the plate"
            flat += length / 2
    assert abs(flat - distance * math.sqrt(2) * height) < 1e-3, ("flat", flat)
    return count
def version(path):
    return run("inspect", path, "--json")["result"]["content_version"]
def peer_sketch(src, rect, dest):
    dest.unlink(missing_ok=True)
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "vertices": [
        {"curve_id": v["curve_id"], "start_mm": p} for v, p in zip(facts["vertices"], vertices_of(rect))]}))
    run("edit-sketch-copy", src, "--sketch", SKETCH, "--expect-version", version(src),
        "--request", request, "-o", dest, "--json")
    request.unlink()
def peer_height(src, height, dest):
    dest.unlink(missing_ok=True)
    run("edit-extrude", src, "--feature", BASE, "--distance-mm", str(height), "--expect-version",
        version(src), "-o", dest, "--json")
def peer_distance(src, distance, dest):
    dest.unlink(missing_ok=True)
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "distance_mm": distance}))
    run("edit-chamfer-distance", src, "--feature", CHAMFER, "--expect-version", version(src),
        "--request", request, "-o", dest, "--json")
    request.unlink()
g = {n: out / f"gui-{n}.fcad" for n in ("size", "moved", "tall", "distance")}
p = {n: out / f"peer-{n}.fcad" for n in g}
peer_sketch(source, RECTS["size"], p["size"])
peer_sketch(p["size"], RECTS["moved"], p["moved"])
peer_height(p["moved"], H["final"], p["tall"])
peer_distance(p["tall"], D["final"], p["distance"])
# Allowlists on the window's own chain: each step moves exactly what it may.
allowlist_row(source, g["size"], SKETCH)
allowlist_row(g["size"], g["moved"], SKETCH)
allowlist_row(g["moved"], g["tall"], BASE)
allowlist_row(g["tall"], g["distance"], CHAMFER)
# What each window document says: the same Chamfer at the vertex it was made
# at, on the plate as typed, every saved name resolving after a cold rebuild.
def chamfer_of(path, rect, height, distance):
    result = run("inspect", path, "--json")["result"]
    (row,) = result["chamfers"]
    assert row["feature_id"] == CHAMFER and row["distance_mm"] == distance, row
    assert row["edge"] == facts["edge"], "the Chamfer's two Lines changed"
    assert row["corner_mm"] == corner_of(rect), ("the corner did not follow its vertex", row["corner_mm"])
    (feature,) = result["features"]
    assert feature["feature_id"] == BASE and feature["distance_mm"] == height, feature
    (sk,) = result["sketches"]
    assert sk["sketch_id"] == SKETCH and [v["start_mm"] for v in sk["vertices"]] == vertices_of(rect), "the plate as typed"
    assert [v["curve_id"] for v in sk["vertices"]] == [v["curve_id"] for v in facts["vertices"]], "a Line UUID changed"
    for base in (feature["chamfer_base"], sk["chamfer_base"]):
        assert base["chamfer_feature_id"] == CHAMFER and base["corner_mm"] == corner_of(rect) and base["distance_mm"] == distance
    assert result["bodies"][0]["body_id"] == BODY
    assert result["bodies"][0]["chamfer_edge"]["available"] is False, "a second Chamfer is offered"
    assert run("validate", path, "--json")["result"]["valid"] is True
    n = len(tables(path)["topology_refs"][1])
    text = run("rebuild", path, "--cold")
    assert "tip Chamfer" in text and f"{n} of {n} stored references resolved" in text, text
chamfer_of(g["size"], RECTS["size"], H["source"], D["source"])
chamfer_of(g["moved"], RECTS["moved"], H["source"], D["source"])
chamfer_of(g["tall"], RECTS["moved"], H["final"], D["source"])
chamfer_of(g["distance"], RECTS["moved"], H["final"], D["final"])
# The saved names are the source's, under the same UUIDs, at every step.
refs = [r[0] for r in tables(source)["topology_refs"][1]]
for n in g:
    assert [r[0] for r in tables(g[n])["topology_refs"][1]] == refs, f"{n}: a name changed UUID"
# Every cell of every window document equals the shipped CLI's, nothing mapped:
# no operation here mints a UUID. Only each copy's own stamp is left out.
def same(gui, peer):
    left, right = tables(gui), tables(peer)
    assert left.keys() == right.keys()
    cells = 0
    for t in left:
        (cols, lrows), (rc, rrows) = left[t], right[t]
        assert cols == rc and len(lrows) == len(rrows), t
        for x, y in zip(lrows, rrows):
            for k, c in enumerate(cols):
                if t == "meta" and c == "modified_at":
                    continue
                assert x[k] == y[k], f"{t}.{c}"
                cells += 1
    return cells
cells = sum(same(g[n], p[n]) for n in g)
for fmt in ("stl", "fbx"):
    target = out / f"peer-distance.{fmt}"
    target.unlink(missing_ok=True)
    run(f"export-{fmt}", p["distance"], "-o", target, "--json")
    assert (out / f"gui-distance.{fmt}").read_bytes() == target.read_bytes(), fmt
# The geometry, by an independent reader of binary STL: the window's own export
# at the final plate, height and distance, and each saved step at its own plate.
count = mesh_checks(out / "gui-distance.stl", RECTS["moved"], H["final"], D["final"])
with tempfile.TemporaryDirectory(prefix="ferrite-29c-compare-") as scratch:
    scratch = pathlib.Path(scratch)
    mesh_checks(stl_of(g["size"], scratch), RECTS["size"], H["source"], D["source"])
    mesh_checks(stl_of(g["moved"], scratch), RECTS["moved"], H["source"], D["source"])
    mesh_checks(stl_of(g["tall"], scratch), RECTS["moved"], H["final"], D["source"])
print("FCAD_29C_GUI_COMPARE_OK", f"cells={cells}", f"triangles={count}")
```

## Independent review on macOS arm64 — 2026-10-02

Reviewed `a5ba39a30716a98cd28b444ca26608caeb8bb48c`, whose code is unchanged
from `4dbadc936f49c5ee036f11572c72d03e87ef67b2`. No blocking code defect was
found. Clarified that the seven-name restriction concerns Chamfer-owned names,
not additional resolving base references, and corrected the fixture's UI name.
The shared reader, candidate validator, transactional writer re-derivation and
strict before/after reference check were inspected together; the §29B reference
regression remains in the native campaign.

Rebuilt the release CLI and viewer with the existing pinned OCCT 8.0.1 and
PlaneGCS, without rebuilding either dependency. Sequential affected tests:
90 CLI fillet tests, 50 app sketch tests, 16 app edit tests and 499
document/jobs/eval tests: **655 executed, zero failures**. Four explicit
no-solver/mixed-only N/A cases were excluded from that number; one old manual
benchmark remained ignored. Fmt, workspace clippy with all targets/features and
`-D warnings`, actionlint, licence headers (401 files), export boundary and
whitespace checks passed. The Markdown recipe ran against the fresh bundled CLI:
`FCAD_29C_RECIPE_OK d=2.375 big=7211.056641/7211.056641`.

The full runtime log of run 36995480290 was downloaded and checked, rather than
inferring execution from workflow definitions. Each of Linux, macOS and Windows
executed all 48 named Chamfer gates once, with positive test results and no skips,
plus two §29C recipe markers (native and OCCT without solver), nine strict ufbx
reads with six checks and zero failures each, and nine oriented 16-triangle
STL/FBX joins. The planegcs jobs of run 36995480272 and all seven ordinary CI jobs
of the reviewed docs head, run 37003696268, were successful. This is PR evidence;
no future post-merge CI is claimed here. Stub/mixed configurations were audited
remotely and not rebuilt again on this Mac.

A freshly staged arm64 bundle passed its dependency-closure checks, deep strict
code-signature verification and `--solver-info`. Viewer UUID:
`AF403A80-C432-37C8-8A52-4AA17E5A0F42`. One viewer, PID 7178, ran under the
1536 MiB watchdog, using only the temporary fixture directory.

The actual window completed the scenario above: native Open; non-rectangle
refusal with only one vertex changed; too-short-side refusal naming the Chamfer
and bound; Undo/Redo/Restore; resize; native Save Cancel with the draft retained;
four publications and async Opens (size, translation, height, Chamfer distance);
and both STL and FBX exports through their real Save dialogs. The final model
was 41 × 10.25 × 9.5 mm, translated to lower-left (8.5, −6.75), with distance
4.5 mm at the same lower-right Line-UUID corner. The optional mouse drag was
not executed in this window run: after native Open, CUA coordinate clicks
returned `noWindowsAvailable` despite a live observable window. Keyboard
navigation and native accessibility actions completed the scenario in the same
PID. The headless drag regression passed separately. Numeric typing can create
intermediate text-history entries; the observed Undo/Redo restored those exact
strings before Restore returned all saved vertices.

The comparator first consumed the six real window-produced files, then created
CLI peers: `FCAD_29C_GUI_COMPARE_OK cells=512 triangles=16`. All four SQL
allowlists, the source hash, UUID/reference preservation, cold rebuilds and
independent mesh checks passed. The final UI/CLI STL (884 bytes) and FBX
(5418 bytes) were byte-identical. Pinned ufbx 0.23.0 read the GUI FBX with
6 checks / 0 failures; the oriented join matched 16 triangles with worst
coordinate error `4.34e-19 m`. Private negative controls failed as intended for
a missing GUI file, an unrelated `objects.name` change and a 41.5 mm plate
instead of the typed 41 mm plate.

Before launch, system pressure had briefly been warning; launch waited until it
returned to normal. During all 2299 watchdog samples, pressure was normal and
swap stayed at 715784192 bytes. Peak process footprint was **216.095 MiB**,
minimum free disk 139.40 GiB. Cmd-Q gave exit 0 after 1256 seconds, without a
watchdog abort. Only the PID/watchdog was inspected after Quit; the viewer was
not addressed or relaunched. The older OOM remains unexplained.

Evidence: `/private/tmp/ferrite-pr79-review/` (logs, runtime audit, screenshots,
models, comparator controls and `memory-summary.json`), also retained in the
local review-artifact directory. No Linux/Windows window test, heavy local
STEP rerun or general GPU campaign is implied by this focused macOS run.
