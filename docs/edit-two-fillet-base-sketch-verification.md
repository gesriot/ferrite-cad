# §28J — verification record

[The contract and recipe](edit-two-fillet-base-sketch.md).

## Base and scope

Base `main` = `2b5aab645fc7b2db53a3cf1bd7e2fbe70111c7eb` (PR #73, §28I),
tree `4bdc5a6ec7a62126cc1823b1145b0bb8ad05f007`, the tree of the reviewed
final head `52cb944`. Its post-merge runs (CI, planegcs pin, runtime layout,
SBOM and notices) were still queued or running when this branch started; they
are runs of the same tree and are told apart from the completed runs of the
PR. The branch is `edit-two-fillet-base-sketch`; §28I was not reimplemented.

## Where and how this was run

Local, in the cloud container: Linux, Open CASCADE 8.0.1 and PlaneGCS built
from local inputs — **the local PlaneGCS is not pinned**; the pinned
`planegcs pin` workflow and the three-platform runtime layout are
authoritative. macOS is Apple Silicon (arm64) only; macOS Intel is not
supported. No window, GPU or Unity ran here. One heavy process at a time,
`-j2`–`-j3`; OCCT, Boost and PlaneGCS were not rebuilt.

## What changed

* `sketch_edit.rs`: `SketchChoice` gains `second_fillet`; the coordinate
  choice reads the base Sketch through `fillet_radius::fillets_over_plate`
  (the reader §28H/§28I use for two Fillets; one Fillet is §28D's frame
  exactly) and `SketchChoice::validate_coordinates` judges, on the candidate
  Lines, the rectangle, each Line's side, Fillet 1's corner and radius,
  Fillet 2's own corner and radius, and `check_pair` in history order.
  `fillet_over_plate` — the Sketch-constraint and add-Fillet readers — is
  untouched, so those keep refusing two Fillets.
* `sketch_constraints::closure_links_only`: a Sketch under two Fillets may
  carry only distinct Coincident closure links at adjacent joints.
* `SavedFillet::corner_on` measures the candidate against the **base
  Extrude** that swept the edge (`base_feature`), not `previous`, which for
  Fillet 2 is Fillet 1.
* JSON: `sketches[].fillet_base.second_fillet`, additive (same shape as
  `features[]`, §28I). No capability, schema, payload version, command or
  copier.
* UI: the existing Edit Sketch form names both Fillets in history order.
* Refusal texts (`refuse_filleted`) now say that the base Sketch's
  coordinates can be edited.

The writer, `write_sketch_geometry`, is unchanged: inside its transaction it
re-derives the edit through the same choice and compares the whole payload.

## New tests

* Domain (`fillet_radius::tests`, 4): the rectangle moves, grows and shrinks
  to the exact bound alone under adjacent and opposite Fillets in both
  windings (only the Sketch row's payload/hash and the stamp move; both
  Fillet rows, the Extrude, names and dependencies stay; each corner follows
  its own Lines; height and both radii interleave); refusals naming the
  failing Fillet, the flat between adjacent arcs at the least float that
  leaves it and one float below, and opposite corners owing no flat;
  the writer re-deriving both Fillets against a forged payload, a forged
  constraint and a stale Fillet 2 radius; closure links kept byte for byte and
  any other constraint refused.
* CLI (`sequential::sketch`, 6): discovery and the stub order of checks;
  both windings, adjacent and opposite corners, different radii, moved, grown
  and shrunk to the bound, every copy measured cold, through a real Miss and
  Hit over the whole chain, by an independent STL parser and as FBX; a warm
  cache missing the plate and both Fillets, then a radius and the height on
  the moved plate; the flat exact at the bound; atomic refusals, cancellation
  and a lost report; a dimensioned plate refused and a closure-links plate
  edited with the solver agreeing.
* App (2): the widgets show both Fillets and keep the draft through
  refusals, Undo/Redo, a cancelled Save, a stale reply, a worker refusal and
  Restore saved vertices; the worker and the CLI publish one document with
  every SQL cell equal and byte-identical STL/FBX.
* Updated expectations: the §28G/§28H/§28I tests that asserted the Sketch of
  a two-Fillet history was refused now assert it is editable and that the
  constraint editor still refuses.

## Local results

Solver build (Open CASCADE + local unpinned PlaneGCS), debug:

* `tests/fillet.rs`: **60 passed** — 58 executed plus 2 that are N/A here
  because they run only in the build without PlaneGCS (`skipped: the mixed
  gate needs a build without PlaneGCS`). The six `sequential::sketch` tests
  executed; none printed `skipped:`.
* Recipes §28A–§28J against the solver CLI extracted from Markdown: all ten
  print `FCAD_28x_RECIPE_OK`. §28J: `FCAD_28J_RECIPE_OK
  moved=1949.910066/1950.087022 narrow=815.277235/815.454209
  tall_f2=1162.038831/1162.210120` (independently read mesh / exact volume,
  mm³, after the move, after the shrink to 2 × 3.0625 = 6.125 mm, and after
  height 9.5 mm and Fillet 2 → 1.5 mm on the shrunk plate). The §28I recipe's
  one assertion that the Sketch is refused became "editable" and passes.
* FBX: `tools/check-fbx-complex.sh --features planegcs` as CI runs it, over
  the artifacts of the whole `tests/fillet.rs` run: every Fillet marker
  including the new `FCAD_FILLET_TWO_SKETCH_UFBX_EXECUTED`; the four files
  `sketch-ccw-adjacent-1`, `sketch-cw-adjacent-0`, `sketch-ccw-opposite-1`,
  `s-links-moved` read by pinned ufbx 0.23.0 (`checks=6 failures=0` each)
  and joined with their STL (120, 120, 136, 120 triangles, worst 3.47e-18
  m); 60 joins in all.
* fmt, workspace clippy (`--all-targets --all-features -D warnings`) and
  `git diff --check`: clean.

Regression of the affected crates, solver build, debug (`--no-fail-fast`):
`ferritecad-document`, `-jobs`, `-eval` (lib and integration tests), every
`ferritecad-cli` test target except the heavy ones listed below, the CLI binaries, and `ferritecad-app`: **1112 passed,
2 failed, 1 ignored** (the 1100 of §28I plus the 12 new tests). The two
failures are the root-only permission tests (`dump_graph`'s
`read_only_permissions_still_dump_when_the_file_can_be_read` and `validate`'s
`validation_really_read_only_permissions`): a directory made read-only does
not stop uid 0. Copied to a directory and run as `nobody`, both pass (8/8 and
4/4). The ignored test is the timing benchmark. The heavy targets
`complex_step_pixels export_scene_complex occurrence_identity_complex
imported_step_pixels fillet_shell_corpus export_fbx_identity shared_step_import
import_step export_fbx_complex` were not run: nothing in this slice touches
STEP import, scene export or pixels.

OCCT without a solver (release, `--no-default-features`,
`FERRITECAD_REQUIRE_PLANEGCS=0`): `tests/fillet.rs` 60 passed, 14 of them N/A
(`skipped: constrained geometry requires PlaneGCS`), so 46 executed,
including `sequential::sketch::sketch_discovery_and_protocol_without_native`
and the recipe (`FCAD_28J_RECIPE_OK` with the same volumes as the solver
build).

Stub (no Open CASCADE, `CMAKE_TOOLCHAIN_FILE` hiding the native prefix):
`tests/fillet.rs` 60 passed, 49 N/A (`skipped: this build has no Open
CASCADE`), 11 executed; the extracted `ci.yml` step passes — the CLI
discovery gate, the app widget gate, the four document gates and
`FCAD_28J_RECIPE_NO_KERNEL`.

The worker-versus-CLI equality
(`sketch::tests::native_two_fillet_base_sketch_worker_and_cli_publish_the_same_part`)
compares every SQL cell but `meta.modified_at` and requires byte-identical
STL and FBX; it and the widget test executed.

### Mutations — local, executed, restored

Each compiled, ran the 22 `fillet_radius::tests`, failed exactly where named,
and was restored byte for byte (`git diff` clean):

1. `check_pair` dropped from `validate_coordinates` → `a_rectangle_that_loses_a_corner_a_radius_or_the_shared_flat_is_refused`
   fails (the shared flat, adjacent corners).
2. Fillet 2 judged on its stored corner instead of the candidate →
   the same test and `the_sketch_writer_rederives_both_fillets_and_refuses_forgery` fail.
3. Writer re-derivation bypassed (`checked = prepared.clone()`) →
   `the_sketch_writer_rederives_both_fillets_and_refuses_forgery` and the
   §28D `the_sketch_writer_refuses_a_forged_or_stale_preparation` fail.

### Compatibility with the reader on `main`

`main` at `2b5aab6` built with the same solver: it reads, validates and
cold-rebuilds a §28J copy (18 of 18 references) with byte-identical STL and
FBX; it refuses to edit it (`unsupported`, "…after 2 Fillets in all (§28G)…")
and writes nothing; a document written by that build is moved by this one.
`FCAD_28J_COMPAT_OK`.

## CI

CI_PLACEHOLDER

## Limits

* The local PlaneGCS is unpinned; authoritative CI is Linux, macOS arm64 and
  Windows. macOS Intel is not supported.
* No window, GPU or Unity ran in the cloud. The headless widget tests are not
  a GUI run. The previous OOM is not considered fixed.
* Constraints (other than closure links) under two Fillets, a third Fillet,
  arbitrary edges, Cut with Fillet, Chamfer, in-place Save are out of scope.

## macOS fixture and window scenario (for the user's Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised here with a
second CLI invocation standing in for the window, which says nothing about
the window: `FCAD_28J_GUI_COMPARE_OK cells=378`; without the window's files
the comparator stopped with `FCAD_28J_GUI_COMPARE_MISSING …` and created
nothing; a renamed Fillet 2 in `gui-narrow.fcad` was caught
(`AssertionError: objects.name`), a stray `never.fcad`, a coordinate typed
0.001 mm off (`gui-narrow.fcad`'s vertices) and one flipped byte of the STL
were caught. The refusals of the scenario were probed through the CLI on the
generated fixture: 7.9 mm deep names Fillet 1, 8.0 mm Fillet 2, 8.012 mm the
flat, 8.02 mm publishes.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon (arm64) Mac, and do not set `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `twice.fcad` — a **free** plate (no constraint), 37.5 × 12.25 ×
6.75 mm, drawn clockwise from the upper right with offset, fractional
coordinates, rounded by `fillet-edge-copy` twice on adjacent corners: Fillet
1 at the stored corner (33, 3.25) with r 4 mm and Fillet 2 at (33, 15.5) with
r 4.005 mm — and `facts.json`. The two radii differ by less than the 0.01 mm
flat, so a single fixture shows all three refusals (the comments in the
script give the depths).

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-two-fillet-base-sketch-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28J_GUI_FIXTURE\n", "ferrite-28j-fixture.py"),
                   ("# FCAD_28J_GUI_COMPARE\n", "ferrite-28j-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_28J_DIR="$PWD/fillet-two-sketch-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28j-fixture.py "$FCAD_28J_DIR"
```

```python
# FCAD_28J_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-two-sketch-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# A free plate, clockwise from the upper right, offset and fractional: no
# constraint at all. Segment 0 runs down x = 33, so the stored corners
# (33, 3.25) and (33, 15.5) are adjacent and share that 12.25 mm Line.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
FIRST, SECOND, H = [33.0, 3.25], [33.0, 15.5], 6.75
# Fillet 1 r 4 mm, then Fillet 2 r 4.005 mm beside it. Their radii differ by
# less than the 0.01 mm flat the pair rule keeps, so on the shared Line
#   D < 8.0      : Fillet 1 refuses (2 r1 = 8),
#   8.0 <= D < 8.01  : Fillet 2 refuses (2 r2 = 8.01), Fillet 1 fits,
#   8.01 <= D < 8.015: each radius fits alone, the flat between them does not,
#   D >= 8.015   : both fit (4.005 <= D - 4 - 0.01).
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
ctx = sketch["fillet_base"]
two = ctx["second_fillet"]
assert sketch["editable"] is True and ctx["profile_constrained"] is False, sketch
assert two["previous_feature_id"] == ctx["fillet_feature_id"] and two["history_index"] == 2, ctx
assert ctx["corner_mm"] == FIRST and two["corner_mm"] == SECOND, ctx
(base,) = catalog["features"]
(out / "facts.json").write_text(json.dumps({
    "sketch_id": sketch["sketch_id"], "base_feature_id": base["feature_id"],
    "curve_ids": [v["curve_id"] for v in sketch["vertices"]],
    "content_version": catalog["content_version"],
    "first_fillet_id": ctx["fillet_feature_id"], "second_fillet_id": two["fillet_feature_id"],
    "saved_corners_mm": [FIRST, SECOND], "saved_plate": PLATE, "radii_mm": [R1, R2],
    "height_mm": H,
    "moved_rect_mm": [-1.25, 1.0, 40.0, 12.5], "narrow_rect_mm": [2.0, -3.5, 20.25, 8.02],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28J_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_28J_DIR/../watch-28j.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

If system memory pressure aborts or stops the run, it does not count; start
again from a fresh fixture directory. **Do not address the viewer after Quit
through the automation**: it may relaunch the app outside the watchdog. If it
was relaunched, quit that instance too and read only the original PID's exit
and watchdog record.

The saved plate's vertices, in saved order, are `0 (33, 15.5)`, `1 (33,
3.25)`, `2 (-4.5, 3.25)`, `3 (-4.5, 15.5)`. Type numbers exactly as written.

1. **Open** `$FCAD_28J_DIR/twice.fcad` (asynchronously). **Edit Sketch…**
   is enabled for the base Sketch; the constraint editor is refused (two
   Fillets); **Fillet edge of …** is refused ("third"). Open the base Sketch:
   the form reads "History: Extrude -> Fillet 1 <first_fillet_id> at the
   corner of Lines … | …, r 4 mm -> Fillet 2 <second_fillet_id> at the corner
   of Lines … | …, r 4.005 mm. Both Fillets keep their corners and radii …
   no side may be shorter than 8 mm (Fillet 1) or 8.01 mm (Fillet 2) …".
2. **Move and resize.** Vertices to 0 `(38.75, 13.5)`, 1 `(38.75, 1)`, 2
   `(-1.25, 1)`, 3 `(-1.25, 13.5)` (40 × 12.5 mm at (-1.25, 1)). Undo the
   last field edit, Redo it. Drag vertex 0 a little on the canvas, then
   **Undo** that drag (one step), so the typed values stand.
3. **Second radius refused.** Set the two upper vertices' Y to `9` (12.5 →
   8 mm deep). The form refuses with Fillet 2's numbers ("a fillet of 4.005
   mm at corner … is too large … 8 mm"); nothing starts; the draft stays.
4. **Fillet 1's bound.** Set the Y to `8.9` (7.9 mm): the refusal names
   Fillet 1 (4 mm).
5. **The flat between the arcs.** Set the Y to `9.012` (8.012 mm): each
   radius fits alone, the flat does not: "…would leave 0.007… mm of it flat…".
6. **Save Cancel.** Set the Y back to `13.5`, **Save edited copy…** →
   **Cancel** in the file dialog: nothing starts; the draft stays.
7. **Restore.** **Restore saved vertices** returns the four saved vertices;
   type step 2's values again.
8. **Worker refusal keeps the draft** is not reachable with a valid draft on
   this fixture; skip it (the widget tests cover it).
9. **Publish, async Open.** **Save edited copy…** →
   `$FCAD_28J_DIR/gui-moved.fcad`. The viewer opens the copy asynchronously;
   the same form reads Fillets 1 and 2 at (38.75, 1) and (38.75, 13.5).
10. **Shrink to the bound.** In the opened copy vertices to 0 `(22.25,
    4.52)`, 1 `(22.25, -3.5)`, 2 `(2, -3.5)`, 3 `(2, 4.52)` (20.25 × 8.02
    mm): accepted (the flat is 0.015 mm). **Save edited copy…** →
    `$FCAD_28J_DIR/gui-narrow.fcad`; it opens asynchronously.
11. **Exports.** From the window export `gui-narrow.stl` and `gui-narrow.fbx`
    of `gui-narrow.fcad` into `$FCAD_28J_DIR`, at the default tessellation.
12. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28j-compare.py "$FCAD_28J_DIR"
```

It first requires `gui-moved.fcad`, `gui-narrow.fcad`, `gui-narrow.stl` and
`gui-narrow.fbx` (and never creates them), refuses a `never.fcad`, checks
the source's SHA-256, the allowlist of each window edit (the Sketch row's
payload/hash and the stamp only), the vertices, curve UUIDs and order, both
Fillets (UUIDs, radii, edges, predecessors) and both corners as the
catalogue describes them, validates and cold-rebuilds `gui-narrow.fcad`,
makes the CLI peer (`peer-moved.fcad`, `peer-narrow.fcad` and its exports,
only if absent), compares every SQL cell but `meta.modified_at` — this edit
mints no UUID — and requires byte-equal STL/FBX, then reads the window's STL
itself: the plate's bounds and volume, exactly the two saved corners rounded,
each wall at its own radius about its own axis. It prints
`FCAD_28J_GUI_COMPARE_OK cells=N`.

```python
# FCAD_28J_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-moved.fcad", "gui-narrow.fcad", "gui-narrow.stl", "gui-narrow.fbx")
missing = [n for n in GUI if not (out / n).is_file()]
if missing:
    sys.exit(f"FCAD_28J_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first")
assert not (out / "never.fcad").exists(), "a refused Save published something"
source = out / "twice.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
SKETCH, F1, F2 = facts["sketch_id"], facts["first_fillet_id"], facts["second_fillet_id"]
r1, r2 = facts["radii_mm"]
H = facts["height_mm"]
SAVED = facts["saved_plate"]
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
def allowlist(before, after):
    """Only the Sketch row's payload/payload_hash and meta.modified_at moved."""
    sid = bytes.fromhex(SKETCH.replace("-", ""))
    a, b = tables(before), tables(after)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc and len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                assert u == v or (t == "objects" and c in ("payload", "payload_hash")
                                  and x[ac.index("id")] == sid) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c}"
def same(gui, peer):
    """Every SQL cell equal but the stamp: this edit mints nothing."""
    left, right = tables(gui), tables(peer)
    assert left.keys() == right.keys()
    cells = 0
    for t in left:
        (cols, lrows), (_, rrows) = left[t], right[t]
        keep = [k for k, c in enumerate(cols) if not (t == "meta" and c == "modified_at")]
        norm = lambda rows: sorted((tuple(r[k] for k in keep) for r in rows), key=repr)
        assert norm(lrows) == norm(rrows), t
        cells += len(keep) * len(lrows)
    return cells
def corner(rect, p):
    """The corner of `rect` (x0, y0, width, depth) that `p` is of the saved plate."""
    x0, y0, w, d = rect
    return [x0 if p[0] == min(q[0] for q in SAVED) else x0 + w,
            y0 if p[1] == min(q[1] for q in SAVED) else y0 + d]
def starts(rect):
    return [corner(rect, p) for p in SAVED]
def described(path):
    (sketch,) = run("inspect", path, "--json")["result"]["sketches"]
    ctx = sketch["fillet_base"]
    two = ctx["second_fillet"]
    assert sketch["editable"] is True and sketch["constraint_edit"]["available"] is False
    return ([v["start_mm"] for v in sketch["vertices"]], [v["curve_id"] for v in sketch["vertices"]],
            ctx["fillet_feature_id"], ctx["radius_mm"], ctx["corner_mm"], ctx["edge"],
            two["fillet_feature_id"], two["previous_feature_id"], two["history_index"],
            two["radius_mm"], two["corner_mm"], two["edge"])
moved, narrow = facts["moved_rect_mm"], facts["narrow_rect_mm"]
# The window's copies against their sources, and what they say.
allowlist(source, out / "gui-moved.fcad")
allowlist(out / "gui-moved.fcad", out / "gui-narrow.fcad")
base = described(source)
for path, rect in ((out / "gui-moved.fcad", moved), (out / "gui-narrow.fcad", narrow)):
    got = described(path)
    assert got[0] == starts(rect), (path.name, got[0])
    assert got[1] == facts["curve_ids"], "the curve UUIDs or their order changed"
    assert (got[2], got[3], got[5]) == (base[2], base[3], base[5]), "Fillet 1 changed"
    assert (got[6], got[7], got[8], got[9], got[11]) == (base[6], base[7], 2, base[9], base[11]), "Fillet 2 changed"
    assert got[4] == corner(rect, facts["saved_corners_mm"][0]), (path.name, "Fillet 1's corner", got[4])
    assert got[10] == corner(rect, facts["saved_corners_mm"][1]), (path.name, "Fillet 2's corner", got[10])
(one,) = run("inspect", out / "gui-narrow.fcad", "--json")["result"]["features"]
assert one["distance_mm"] == H and one["feature_id"] == facts["base_feature_id"], one
assert run("validate", out / "gui-narrow.fcad", "--json")["result"]["valid"] is True
n = len(tables(out / "gui-narrow.fcad")["topology_refs"][1])
assert f"{n} of {n} stored references resolved" in run("rebuild", out / "gui-narrow.fcad", "--cold")
# The same two edits through the shipped CLI, and its exports.
def peer(src, rect, dest):
    if not dest.exists():
        catalog = run("inspect", src, "--json")["result"]
        request = out / "peer-request.json"
        request.write_text(json.dumps({"request_version": 1, "vertices": [
            {"curve_id": c, "start_mm": p} for c, p in zip(facts["curve_ids"], starts(rect))]}))
        run("edit-sketch-copy", src, "--sketch", SKETCH, "--expect-version",
            catalog["content_version"], "--request", request, "-o", dest, "--json")
        request.unlink()
peer(source, moved, out / "peer-moved.fcad")
peer(out / "peer-moved.fcad", narrow, out / "peer-narrow.fcad")
for fmt in ("stl", "fbx"):
    if not (out / f"peer-narrow.{fmt}").exists():
        run(f"export-{fmt}", out / "peer-narrow.fcad", "-o", out / f"peer-narrow.{fmt}", "--json")
cells = same(out / "gui-moved.fcad", out / "peer-moved.fcad") \
    + same(out / "gui-narrow.fcad", out / "peer-narrow.fcad")
for fmt in ("stl", "fbx"):
    assert (out / f"gui-narrow.{fmt}").read_bytes() == (out / f"peer-narrow.{fmt}").read_bytes(), fmt
# The window's mesh, read here: the narrow plate with exactly its two saved
# corners rounded, each wall at its own radius about its own axis.
data = (out / "gui-narrow.stl").read_bytes()
(count,) = struct.unpack_from("<I", data, 80)
assert len(data) == 84 + 50 * count
tri = [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)] for i in range(count)]
pts = [p for t in tri for p in t]
six = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2])
          + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in tri)
x0, y0, w, d = narrow
xs, ys, zs = ([p[k] for p in pts] for k in range(3))
for got, want in ((min(xs), x0), (max(xs), x0 + w), (min(ys), y0), (max(ys), y0 + d),
                  (min(zs), 0.0), (max(zs), H)):
    assert abs(got - want) < 1e-4, (got, want)
exact = (w * d - (1 - math.pi / 4) * (r1 * r1 + r2 * r2)) * H
slack = sum(math.pi / 2 * r * 0.01 * H for r in (r1, r2))
assert exact - slack - 1e-3 <= six / 6 <= exact + 1e-3, (six / 6, exact)
rounded = [(corner(narrow, c), r) for c, r in zip(facts["saved_corners_mm"], (r1, r2))]
for c in ([x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]):
    near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 for p in pts)
    assert near == all(c != at for at, _ in rounded), c
for (cx, cy), r in rounded:
    ax = cx + (r if abs(cx - x0) < 1e-9 else -r)
    ay = cy + (r if abs(cy - y0) < 1e-9 else -r)
    wall = [p for p in pts if abs(p[0] - cx) < r - 1e-4 and abs(p[1] - cy) < r - 1e-4]
    assert len(wall) >= 4, (cx, cy)
    assert all(abs(math.hypot(p[0] - ax, p[1] - ay) - r) < 1e-3 for p in wall), (cx, cy, r)
print("FCAD_28J_GUI_COMPARE_OK", f"cells={cells}")
```
