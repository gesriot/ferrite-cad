# §29D — constraints of a chamfered plate: verification

[The contract and executable recipe](edit-chamfer-base-constraints.md). This is the
record of what was run, where, and what was not. It states the result of one head;
the CI of the PR that carries it is recorded in the PR, not here, since a document
cannot name the run of its own commit.

## What changed

The existing constraint editor (`edit-sketch-constraints-copy`, **Edit constraints**)
accepts the base Sketch of the §29A–C plate: `constraint_frame` reads the saved
Chamfer (`saved_chamfer`, now carrying `constrained`), `PreparedSketchConstraints`
carries it, and the writer re-derives the whole prepared value inside its
transaction. The evaluator (`evaluable_chamfer`), which runs at every cold and
cached rebuild on the **solved** presentation Lines, now judges the rectangle, the
Line UUIDs and order, every Line's axis and direction, the corner (found by its two
UUIDs) and the distance bound `0.001 ≤ d ≤ min(adjacent solved sides) − 0.01`
exactly. The stored coordinates are never overwritten by a solve. The §29C
coordinate editor refuses while user constraints exist. Additive JSON only:
`profile_constrained` (in `chamfers[]` and both `chamfer_base` objects),
`sketches[].constraint_edit.chamfer_base`, and `distance_edit.max_distance_mm` is
`null` for a constrained plate.

## Local results

Everything below ran in the Linux cloud container (x86-64, native Open CASCADE 7.9
and a **local** PlaneGCS that is not the CI-pinned one; macOS and Windows are CI).

- **Native gates** (`runtime-layout.yml` step "Chamfer one vertical edge of a saved
  plate into a named flat", extracted and run on the debug build with only the
  flags, library path and artifact directory adapted): exit 0 after the recipe
  fix below; seven new CLI gates (six native, and the discovery/protocol gate that also
  runs in the stub step), three new document gates, two new app gates, all executed by exact
  name and none skipped; recipes `FCAD_29A/B/C_RECIPE_OK`; `FCAD_29D_RECIPE_OK
  dimensioned=4318.365234/4318.365234 replaced=3171.392578/3171.392578
  taller=4580.900391/4580.900391 distance14.5=3583.429688/3583.429688
  moved=10399.593750/10399.593750` (measured/exact volumes).
  The first run of this step stopped at the §29B recipe: it compared `chamfer_base`
  for exact equality and the additive `profile_constrained` field broke that; the
  §29B and §29C recipes now expect `"profile_constrained": False` (a separate
  commit). The recipes were then run again; the 28A–28L and 29A–29D recipes all
  print their `…_RECIPE_OK`.
- **Stub (no Open CASCADE)**: the "Discover and refuse one Chamfer without native
  geometry" step, extracted and run on a real no-kernel build
  (`/home/user/stub-target`, no `OpenCASCADE_DIR`): every gate `ok`, and
  `FCAD_29A/B/C/D_RECIPE_NO_KERNEL`. A stub's "unsupported" is not claimed as
  proof of any geometry.
- **OCCT without the solver** (debug, `--no-default-features`,
  `FERRITECAD_REQUIRE_PLANEGCS=0`, no `FCAD_PLANEGCS_DIR`): the "Chamfer a plate
  with Open CASCADE and no solver" step ran: the two new gates pass (constrained
  publication refuses and writes nothing; discovery and protocol), the free and
  closure-only routes work, `FCAD_29A/B/C_RECIPE_OK` and
  `FCAD_29D_RECIPE_NO_SOLVER`. A build without the solver builds the free plate and
  refuses the constraint copy; it never produces geometry for a constrained plate.
- **Pinned ufbx**: the `check-fbx-complex.sh` loop over `cons-0-solved`,
  `cons-1-solved` and `cons-2-solved` (the nine CLI-written FBX/STL pairs of the
  three drawing orders' constrained copies) was run against the step's artifacts
  with a trimmed copy of the script: each file read twice (`checks=6 failures=0`),
  joined to its STL (`FCAD_STL_FBX_MATCH triangles=16 worst_m=0`) and
  `FCAD_CHAMFER_CONSTRAINTS_UFBX_EXECUTED`.
- **Affected regression** (document, jobs, eval, every CLI test target except the
  large STEP corpus targets, app; planegcs): 1197 passed, 1 ignored (an old
  benchmark), 2 failed — the same two tests that need a read-only file to be
  unreadable and that fail as root in this container
  (`read_only_permissions_still_dump_when_the_file_can_be_read`,
  `validation_really_read_only_permissions`; they fail on `main` here too).
- `cargo fmt --check`; workspace
  `cargo clippy --workspace --all-targets --features planegcs -- -D warnings`
  (it found an unused variable and a very complex closure type in the new tests,
  fixed in their own commit, after which the three affected gates were run
  again); licence headers (402 files), export boundary, `git diff --check`: clean.
  `actionlint` is not installed here: both edited workflows were parsed as YAML
  and no `run:` step is over 21 000 characters (the largest is 20 411); **that is a
  local audit, not a workflow check — the workflow check is CI's.**
- The heavy STEP corpus targets were not run locally.

### What the native gates measure

Measured on the solved Lines, not on stored coordinates or on the volume alone:
all four corners in three drawing orders (clockwise, counter-clockwise, shifted
start), offset and fractional; the real solver's degrees of freedom and solved
sizes; the Chamfer's plane, outward normal and area `d·√2·h` under the saved name;
the volume `(W·D − d²/2)·h` on the solved width and depth; an independent STL
reader (closed, one winding, bounds, the three whole corners, the cut at the chosen
corner); the oriented FBX through pinned ufbx. A Chamfer at another corner with
the same volume is rejected (`…_is_at_its_own_corner_not_another_with_the_same_volume`).
Bound: a stored plate too small and a solved one large publishes the larger
distance; a stored plate large and a solved one too small is refused whole, with no
output and no scratch; exact at the bound and at its nearest distinguishable
numbers (at the document layer and through an in-process `rebuild_cold`, because
the CLI's JSON float parse is not exact to the last ulp, and with a margin through
the CLI); a side change, a non-axis solved shape, a real conflicting and a real
redundant (reported, published) constraint with their UUIDs. Lifecycle: add →
Replace length → remove an exact UUID → remove every user rule (the closure stays)
→ a coordinate edit is available again, with a height and a distance edit between
steps; every SQL cell by the allowlist, the source hash, every saved reference,
the cache Miss then Hit, the old solved body never returned, cold reopen. Widgets →
the existing worker → the peer CLI: Undo/Redo, Replace, the pending request kept
after refusal, Cancel and a stale reply, async Open, and byte-equal STL/FBX; the
SQL agrees except the new constraint UUIDs. There is no domain check duplicated in
the form.

### Mutations — local, executed, restored

Each compiled, failed on an executed assertion (not a compile error and not zero
tests) and was restored byte for byte (`git status` clean), after which the
positive gates passed again:

1. **M29D-1, the bound from the stored geometry instead of the solved.** The
   evaluator's Chamfer branch measures the sides on the stored rectangle. Failed:
   `the_evaluator_judges_the_chamfer_on_the_built_lines` and
   `the_evaluator_judges_a_constrained_plate_on_its_solved_lines` (document), and
   `native_the_distance_bound_is_the_solved_plates_exactly` and
   `native_constraint_refusals_under_a_chamfer_are_atomic` (CLI, e.g. "Chamfer … of
   15 mm does not fit the solved plate" expected but a plate was published).
2. **M29D-2, the writer's re-derivation bypassed** (the prepared value is written
   as given: a forged payload, a dropped protected constraint or an unresolved
   reference would be accepted). Failed:
   `the_constraint_writer_rederives_under_a_chamfer_and_refuses_forgery`
   (`assertion failed` at the forged write).

No equivalent mutation was left standing: choosing the corner by index instead of
by its two Line UUIDs does not change the bound (all four corners of a rectangle
have the same adjacent sides) and is caught instead by the corner-versus-volume
gate, so it was not counted as a mutation of the bound.

### Compatibility with the reader on `main`

The exact base `e00d72f` was built with the same solver and given a copy this
build made (`FCAD_29D_COMPAT_OK`): `inspect` works (without the new keys),
`validate` is valid, and `rebuild --cold`, `export-stl`, `edit-extrude`,
`edit-chamfer-distance` and `edit-sketch-constraints-copy` all refuse with exit 2
and a typed `unsupported` naming the Chamfer and the constraint's UUID (the old
reader's rule: a chamfered plate is free or closure-only), and nothing is written
and the file's bytes are unchanged. On the unconstrained chamfered plate the old and
new `inspect` agree on every old key and type; the only changed values are the
`constraint_edit` availability (false → true) and refusal prose, and the added keys
are `profile_constrained` and `constraint_edit.chamfer_base`.

## macOS fixture and window scenario (for Codex on the Mac)

Nothing here was run in this container, which has no window system. The generator
and comparator use only the CLI. They were exercised with CLI calls standing in for
the window, which says nothing about the window: `FCAD_29D_GUI_COMPARE_OK
cells=868 triangles=16`. Controls rejected: a depth typed 10.5 instead of 10.25
(`constraints differ` against the peer), a coordinate rectangle 41.5 wide
(`stored starts were replaced by a solve`), a changed `objects.name` cell in a
window copy (`objects.name moved`), a `gui-refused.fcad`, a missing window file
(`FCAD_29D_GUI_COMPARE_MISSING gui-back.fcad …`, no peer or other file created) and
— on the independent STL reader alone — the mesh of a Chamfer at another vertex of
the same plate and volume, of the right vertex of a plate 0.25 mm deeper, and of
the right vertex of the stored plate in place of the solved one. The comparator
does not check a stored/solved swap inside a window file beyond the stored-starts
assertion; that assertion is the control for it.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon Mac with a fresh arm64 bundle, and do not set
`FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `chamfer.fcad` — a free plate 37.5 × 12.25 × 6.75 mm, drawn clockwise from
the upper right, offset and fractional: V0 (33, 15.5), V1 (33, 3.25), V2 (−4.5,
3.25), V3 (−4.5, 15.5), with one Chamfer at V1, d 2.375 mm; no constraint — and
`facts.json`.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-chamfer-base-constraints-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_29D_GUI_FIXTURE\n", "ferrite-29d-fixture.py"),
                   ("# FCAD_29D_GUI_COMPARE\n", "ferrite-29d-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_29D_DIR="$PWD/chamfer-constraints-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-29d-fixture.py "$FCAD_29D_DIR"
```

```python
# FCAD_29D_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "chamfer-constraints-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# A free plate, clockwise from the upper right, offset and fractional, no user
# constraint: V0 (33, 15.5), V1 (33, 3.25), V2 (-4.5, 3.25), V3 (-4.5, 15.5);
# 37.5 mm wide, 12.25 mm deep, 6.75 mm tall. One equal-distance Chamfer at V1,
# d 2.375 mm. Its Lines: L0 V0-V1 and L2 are vertical, L1 V1-V2 and L3 horizontal.
# The window dimensions it (width 41, depth 10.25, V0 pinned where it is), replaces
# the depth by 20.5 (the stored 12.25 mm is shorter), then changes the height and
# the distance (15.25 mm: beyond the stored bound 12.24, inside the solved one),
# lowers the distance, removes every user constraint, and edits the coordinates.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
H0, H1, D1, D2, D3 = 6.75, 9.5, 2.375, 15.25, 3.0
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
edit = sketch["constraint_edit"]
assert edit["available"] is True and edit["chamfer_base"]["chamfer_feature_id"] == chamfer["feature_id"]
assert [v["start_mm"] for v in edit["curves"]] == PLATE
assert edit["constraints"] == [], "the source is free: the closure links are minted by the first constraint edit"
facts = {
    "body_id": c["bodies"][0]["body_id"], "base_feature_id": base["feature_id"],
    "chamfer_id": chamfer["feature_id"], "edge": chamfer["edge"], "sketch_id": sketch["sketch_id"],
    "lines": [v["curve_id"] for v in edit["curves"]], "stored_mm": PLATE,
    "stored_corner_mm": PLATE[1],
    "pin_mm": PLATE[0], "width_mm": 41.0, "depths_mm": {"first": 10.25, "second": 20.5},
    "refused_depth_mm": 2.38, "height_mm": {"source": H0, "final": H1},
    "distances_mm": {"source": D1, "wide": D2, "back": D3},
    "coordinate_rect_mm": [-4.5, 3.25, 41.0, 10.25],
    "content_version": c["content_version"],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_29D_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_29D_DIR/../watch-29d.jsonl" --limit-mib 1536 --seconds 2400 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

If system memory pressure aborts or stops the run, it does not count; start again
from a fresh fixture directory. **Do not address the viewer after Quit through the
automation**: it may relaunch the app outside the watchdog. If it was relaunched,
quit that instance too and read only the original PID's exit and watchdog record.
Type numbers exactly as written. Lines: L0 V0→V1 and L2 are vertical, L1 V1→V2 and
L3 horizontal.

1. **Open** `$FCAD_29D_DIR/chamfer.fcad` (asynchronously). The feature list shows
   Extrude → Chamfer.
2. **Form.** **Edit constraints** is enabled (before §29D a chamfered plate refused
   it). Its context reads "Chamfered by Chamfer <UUID> at the corner of Lines <UUID>
   | <UUID>, d 2.375 mm (stored corner (33, 3.25)). The Chamfer keeps its corner and
   distance: the coordinates shown are the stored ones…" and the four vertices shown
   are the stored ones.
3. **Dimension.** Add Vertical on L0 and L2, Horizontal on L1 and L3, **Fixed** on
   L0's start at (33, 15.5), **Length** 41 on L1 and **Length** 10.25 on L0. Then
   **Undo draft** once and **Redo draft** once (the last rule goes and returns).
4. **Save Cancel.** **Save edited copy…** → **Cancel** in the file dialog: nothing is
   created (`gui-cancelled.fcad` never exists), the draft and its pending request stay.
5. **Publication one.** **Save edited copy…** → `$FCAD_29D_DIR/gui-dim.fcad`; it opens
   asynchronously. The plate is solved: 41 × 10.25 (x −8…33, y 5.25…15.5) with the
   Chamfer at (33, 5.25), d 2.375 mm — shallower than the stored 12.25 mm. **Edit
   saved Sketch** is unavailable and says why.
6. **Solved-bound refusal.** **Edit constraints**, **Replace length** on L0 with
   `2.38` (below d + 0.01): the refusal names "Chamfer <UUID> of 2.375 mm does not
   fit the solved plate" and the bound; **Save** publishes nothing
   (`gui-refused.fcad` never exists) and the typed value stays in the draft.
7. **Replace.** **Replace length** on L0 with `20.5` (deeper than the stored
   12.25 mm) → **Save edited copy…** → `$FCAD_29D_DIR/gui-replaced.fcad`; it opens.
   The plate is 41 × 20.5 (y −5…15.5), the Chamfer at (33, −5).
8. **Height.** **Edit extrusion…**, the base Extrude, type `9.5` → **Save new
   file…** → `$FCAD_29D_DIR/gui-tall.fcad`; it opens.
9. **Distance.** **Edit Chamfer distance…**: no maximum is shown (it is the solved
   plate's). **New distance** `15.25` — beyond the stored bound 12.24 mm, inside the
   solved one — → **Apply distance** → **Save distance copy…** →
   `$FCAD_29D_DIR/gui-distance.fcad`; it opens.
10. **Removal refused.** **Edit constraints** on `gui-distance.fcad`, remove every
    user constraint, **Save**: refused ("does not fit the solved plate", the stored
    side being 12.25 mm); nothing is published.
11. **Back.** **Edit Chamfer distance…** `3.0` → **Save distance copy…** →
    `$FCAD_29D_DIR/gui-back.fcad`; it opens.
12. **Remove all.** **Edit constraints**, remove every user constraint (the four
    closure links stay) → **Save edited copy…** → `$FCAD_29D_DIR/gui-free.fcad`;
    it opens. The plate is the stored one again (37.5 × 12.25), the Chamfer at
    (33, 3.25), and **Edit saved Sketch** is available again.
13. **Coordinates.** **Edit saved Sketch**: type the plate 41 × 10.25 keeping its
    lower left corner — V0 (36.5, 13.5), V1 (36.5, 3.25), V2 (−4.5, 3.25) unchanged,
    V3 (−4.5, 13.5) → **Save edited copy…** → `$FCAD_29D_DIR/gui-coords.fcad`;
    it opens.
14. **Exports.** Export `gui-coords.stl` and `gui-coords.fbx` of `gui-coords.fcad`
    into `$FCAD_29D_DIR`, at the default tessellation.
15. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-29d-compare.py "$FCAD_29D_DIR"
```

It first requires `gui-dim`, `gui-replaced`, `gui-tall`, `gui-distance`, `gui-back`,
`gui-free` and `gui-coords` (`.fcad`) and `gui-coords.stl`/`.fbx` (and never
creates them), refuses a `gui-refused.fcad`, `gui-cancelled.fcad` or `gui-never.fcad`,
checks the source's SHA-256, runs the same chain through the CLI (including both
refusals), checks each step's SQL allowlist (one object row's payload and hash and
the stamp, plus the one capability on the first), the stored starts never
overwritten by a solve, the same Chamfer UUID and edge and the same Line UUIDs and
order, `profile_constrained` and the null/number `max_distance_mm` per step, the
closure UUIDs kept and the untouched rules' UUIDs kept (a replaced rule's gone),
every saved name resolving after a cold rebuild, every SQL cell equal to the peer's
(only the new constraint UUIDs are mapped by their rule), byte-equal STL/FBX, and
then reads the STL itself — closed, one winding, the SOLVED plate's bounds, the
volume `(W·D − d²/2)·h`, the three whole corners, the cut at the chosen corner and
its one flat (plane, outward normal, area `d·√2·h`) — for the window's export and,
through the CLI, for each saved step. It prints `FCAD_29D_GUI_COMPARE_OK cells=N
triangles=M`.

```python
# FCAD_29D_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
STEPS = ("dim", "replaced", "tall", "distance", "back", "free", "coords")
GUI = tuple(f"gui-{n}.fcad" for n in STEPS) + ("gui-coords.stl", "gui-coords.fbx")
missing = [n for n in GUI if not (out / n).is_file()]
if missing:
    sys.exit(f"FCAD_29D_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first")
for n in ("gui-refused.fcad", "gui-cancelled.fcad", "gui-never.fcad"):
    assert not (out / n).exists(), f"a refused or cancelled Save published {n}"
source = out / "chamfer.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
BASE, CHAMFER, BODY, SKETCH = facts["base_feature_id"], facts["chamfer_id"], facts["body_id"], facts["sketch_id"]
LINES, STORED = facts["lines"], facts["stored_mm"]
CLOSURE = set()
H, D, DEPTH = facts["height_mm"], facts["distances_mm"], facts["depths_mm"]
WIDTH = facts["width_mm"]
PIN = facts["pin_mm"]
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

def stl_of_doc(path, scratch):
    target = scratch / (path.stem + ".stl")
    run("export-stl", path, "-o", target, "--json")
    return target
def version(path):
    return run("inspect", path, "--json")["result"]["content_version"]
def allowlist(before, after, row, capability=False, schema=False):
    """Exactly one object row's payload and hash (and its schema version when
    the capability is minted) and the stamp moved; the one capability row may
    appear; no table gained or lost another row."""
    rid = bytes.fromhex(row.replace("-", ""))
    a, b = tables(before), tables(after)
    assert a.keys() == b.keys()
    moved = 0
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        if t == "capabilities":
            assert all(r in brows for r in arows), "a capability changed"
            assert all("sketch.constraints.v1" in repr(r) for r in brows if r not in arows)
            assert capability or len(brows) == len(arows), "an unexpected capability"
            continue
        assert len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u != v:
                    moved += 1
                assert u == v or (t == "objects" and c in ("schema_version", "payload", "payload_hash")
                                  and x[ac.index("id")] == rid) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c} moved"
    assert moved >= 2, "the selected row did not change"
def facts_of(path):
    result = run("inspect", path, "--json")["result"]
    (feature,) = result["features"]
    (chamfer,) = result["chamfers"]
    (sk,) = result["sketches"]
    return result, feature, chamfer, sk
def constraints_of(path):
    return facts_of(path)[3]["constraint_edit"]
def shape(c):
    return json.dumps(c["rule"], sort_keys=True)
# What each window document says, by the numbers: the stored starts are never
# overwritten by a solve; the same Chamfer, UUID, edge and Lines; the shown
# corner and the bound are the stored ones (or none); every saved name resolves.
def document(path, rect, height, distance, constrained, stored_rect):
    result, feature, chamfer, sk = facts_of(path)
    assert feature["feature_id"] == BASE and feature["distance_mm"] == height, feature
    assert chamfer["feature_id"] == CHAMFER and chamfer["distance_mm"] == distance, chamfer
    assert chamfer["edge"] == facts["edge"], "the Chamfer's two Lines changed"
    assert result["bodies"][0]["body_id"] == BODY
    assert result["bodies"][0]["chamfer_edge"]["available"] is False, "a second Chamfer is offered"
    assert sk["sketch_id"] == SKETCH
    edit = sk["constraint_edit"]
    assert edit["available"] is True, edit
    assert [c["curve_id"] for c in edit["curves"]] == LINES, "a Line UUID or order changed"
    stored = vertices_of(stored_rect)
    assert [c["start_mm"] for c in edit["curves"]] == stored, "stored starts were replaced by a solve"
    assert chamfer["profile_constrained"] is constrained, chamfer
    shown = vertices_of(stored_rect)[1]
    for base in (feature["chamfer_base"], edit["chamfer_base"]):
        assert base["chamfer_feature_id"] == CHAMFER and base["distance_mm"] == distance
        assert base["profile_constrained"] is constrained and base["corner_mm"] == shown, base
    if constrained:
        assert chamfer["distance_edit"]["max_distance_mm"] is None, chamfer["distance_edit"]
        assert sk["editable"] is False, "coordinate editing must refuse under user constraints"
        assert CHAMFER in sk["refusal"], sk["refusal"]
    else:
        side = min(stored_rect[2], stored_rect[3])
        assert abs(chamfer["distance_edit"]["max_distance_mm"] - (side - 0.01)) < 1e-9, chamfer["distance_edit"]
        assert sk["editable"] is True, sk
    kinds = sorted(c["rule"]["kind"] for c in edit["constraints"])
    assert kinds.count("coincident") == 4, kinds
    links = {c["constraint_id"] for c in edit["constraints"] if c["rule"]["kind"] == "coincident"}
    if not CLOSURE:
        CLOSURE.update(links)
    assert links == CLOSURE, "the closure links lost their UUIDs"
    assert run("validate", path, "--json")["result"]["valid"] is True
    n = len(tables(path)["topology_refs"][1])
    text = run("rebuild", path, "--cold")
    assert "tip Chamfer" in text and f"{n} of {n} stored references resolved" in text, text
    return edit["constraints"]
SOLVED = {"dim": [-8.0, 5.25, 41.0, 10.25], "replaced": [-8.0, -5.0, 41.0, 20.5]}
STORED_RECT = [-4.5, 3.25, 37.5, 12.25]
RECTS = {"dim": SOLVED["dim"], "replaced": SOLVED["replaced"], "tall": SOLVED["replaced"],
         "distance": SOLVED["replaced"], "back": SOLVED["replaced"], "free": STORED_RECT,
         "coords": facts["coordinate_rect_mm"]}
HEIGHTS = {n: H["final"] for n in STEPS}
HEIGHTS.update(dim=H["source"], replaced=H["source"])
DISTS = {n: D["source"] for n in STEPS}
DISTS.update(distance=D["wide"], back=D["back"], free=D["back"], coords=D["back"])
CONSTRAINED = {"dim": True, "replaced": True, "tall": True, "distance": True, "back": True,
               "free": False, "coords": False}
STORED_OF = {n: (facts["coordinate_rect_mm"] if n == "coords" else STORED_RECT) for n in STEPS}
g = {n: out / f"gui-{n}.fcad" for n in STEPS}
listed = {n: document(g[n], RECTS[n], HEIGHTS[n], DISTS[n], CONSTRAINED[n], STORED_OF[n]) for n in STEPS}
# The window's chain moves exactly what each step may.
allowlist(source, g["dim"], SKETCH, capability=True)
allowlist(g["dim"], g["replaced"], SKETCH)
allowlist(g["replaced"], g["tall"], BASE)
allowlist(g["tall"], g["distance"], CHAMFER)
allowlist(g["distance"], g["back"], CHAMFER)
allowlist(g["back"], g["free"], SKETCH)
allowlist(g["free"], g["coords"], SKETCH)
# Constraint UUIDs: untouched kept, exact removals gone, the rest new.
ids = lambda n, pred: {c["constraint_id"] for c in listed[n] if pred(c)}
hv = lambda c: c["rule"]["kind"] in ("horizontal", "vertical", "fixed")
length = lambda mm: (lambda c: c["rule"]["kind"] == "distance" and c["rule"]["distance"] == mm)
width_id = ids("dim", length(WIDTH))
assert len(width_id) == 1 and width_id == ids("replaced", length(WIDTH)), "the untouched width lost its UUID"
assert ids("dim", hv) == ids("replaced", hv), "the untouched H/V/Fixed rules lost their UUIDs"
assert not ids("dim", length(DEPTH["first"])) & ids("replaced", lambda c: True), "a replaced rule kept its UUID"
assert ids("replaced", length(DEPTH["second"])) and not ids("replaced", length(DEPTH["first"]))
for n in ("tall", "distance", "back"):
    assert {c["constraint_id"] for c in listed[n]} == {c["constraint_id"] for c in listed["replaced"]}, \
        f"{n}: the constraints changed with a height or distance edit"
assert ids("free", lambda c: True) == CLOSURE == ids("coords", lambda c: True), "closure not kept"
assert tables(g["tall"])["objects"][0] and True
# The same chain through the shipped CLI, which also gives each refusal.
def peer(src, remove, add, dest):
    dest.unlink(missing_ok=True)
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "remove": remove, "add": add}))
    run("edit-sketch-constraints-copy", src, "--sketch", SKETCH, "--expect-version", version(src),
        "--request", request, "-o", dest, "--json")
    request.unlink()
def refuse(src, remove, add, needle):
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "remove": remove, "add": add}))
    never = out / "peer-never.fcad"
    never.unlink(missing_ok=True)
    error = run("edit-sketch-constraints-copy", src, "--sketch", SKETCH, "--expect-version", version(src),
                "--request", request, "-o", never, "--json", code=2)["error"]
    request.unlink()
    assert error["kind"] == "input" and CHAMFER in error["message"] and needle in error["message"], error
    assert not never.exists(), "a refusal published"
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
rule = lambda i: {"curve_id": LINES[i], "rule": "vertical" if i in (0, 2) else "horizontal"}
dist = lambda i, mm: {"curve_id": LINES[i], "rule": "distance", "distance_mm": mm}
pin = {"curve_id": LINES[0], "rule": "fixed", "at": "start", "x_mm": PIN[0], "y_mm": PIN[1]}
p = {n: out / f"peer-{n}.fcad" for n in STEPS}
peer(source, [], [rule(i) for i in range(4)] + [pin, dist(1, WIDTH), dist(0, DEPTH["first"])], p["dim"])
first_depth = [c["constraint_id"] for c in constraints_of(p["dim"])["constraints"]
               if length(DEPTH["first"])(c)]
refuse(p["dim"], first_depth, [dist(0, facts["refused_depth_mm"])], "does not fit the solved plate")
peer(p["dim"], first_depth, [dist(0, DEPTH["second"])], p["replaced"])
peer_height(p["replaced"], H["final"], p["tall"])
peer_distance(p["tall"], D["wide"], p["distance"])
usr = lambda path: [c["constraint_id"] for c in constraints_of(path)["constraints"] if c["rule"]["kind"] != "coincident"]
refuse(p["distance"], usr(p["distance"]), [], "does not fit the solved plate")
peer_distance(p["distance"], D["back"], p["back"])
peer(p["back"], usr(p["back"]), [], p["free"])
x0, y0, w, d = facts["coordinate_rect_mm"]
request = out / "peer-request.json"
request.write_text(json.dumps({"request_version": 1, "vertices": [
    {"curve_id": l, "start_mm": q} for l, q in zip(LINES, vertices_of([x0, y0, w, d]))]}))
p["coords"].unlink(missing_ok=True)
run("edit-sketch-copy", p["free"], "--sketch", SKETCH, "--expect-version", version(p["free"]),
    "--request", request, "-o", p["coords"], "--json")
request.unlink()
def same(gui, peer_path):
    """Every SQL cell equal but the stamp and the Sketch row's payload, hash and
    schema version (which hold the newly minted constraint UUIDs); then the
    constraints, with only the new UUIDs mapped by their rule."""
    left, right = tables(gui), tables(peer_path)
    assert left.keys() == right.keys()
    cells = 0
    for t in left:
        (cols, lrows), (_, rrows) = left[t], right[t]
        skip = {"payload", "payload_hash", "schema_version"} if t == "objects" else set()
        keep = [k for k, c in enumerate(cols) if not (t == "meta" and c == "modified_at") and c not in skip]
        row_id = cols.index("id") if t == "objects" else None
        norm = lambda rows: sorted((tuple(r[k] for k in keep) for r in rows), key=repr)
        assert norm(lrows) == norm(rrows), t
        cells += len(keep) * len(lrows)
    a, b = facts_of(gui)[3]["constraint_edit"], facts_of(peer_path)[3]["constraint_edit"]
    assert a["curves"] == b["curves"] and len(a["constraints"]) == len(b["constraints"])
    old = set()
    tag = lambda c: (shape(c), c["constraint_id"] if c["constraint_id"] in old else "new")
    assert sorted(tag(c) for c in a["constraints"]) == sorted(tag(c) for c in b["constraints"]), "constraints differ"
    return cells + len(a["constraints"])
cells = sum(same(g[n], p[n]) for n in STEPS)
for fmt in ("stl", "fbx"):
    target = out / f"peer-coords.{fmt}"
    target.unlink(missing_ok=True)
    run(f"export-{fmt}", p["coords"], "-o", target, "--json")
    assert (out / f"gui-coords.{fmt}").read_bytes() == target.read_bytes(), fmt
# The geometry, by an independent reader of binary STL: the window's own export
# and each saved step, at the SOLVED plate where the Sketch is constrained.
count = mesh_checks(out / "gui-coords.stl", RECTS["coords"], HEIGHTS["coords"], DISTS["coords"])
with tempfile.TemporaryDirectory(prefix="ferrite-29d-compare-") as scratch:
    scratch = pathlib.Path(scratch)
    for n in STEPS[:-1]:
        mesh_checks(stl_of_doc(g[n], scratch), RECTS[n], HEIGHTS[n], DISTS[n])
print("FCAD_29D_GUI_COMPARE_OK", f"cells={cells}", f"triangles={count}")
```

## CI

To be filled in from the PR's own runs once they finish.

## Limits

Out of scope and refused: a second Chamfer, a Fillet or a Cut beside it, another
plane or corner, ThroughAll, circles in the base Sketch, in-place Save, live
preview, and creating a Chamfer on an already constrained source (the creation
reader still requires a free plate). Stored coordinates are never replaced by a
solve; a user who wants the solved numbers as coordinates must say so by removing
the constraints and typing them. The local PlaneGCS is not the pinned one. No
window ran here, and the headless app tests are not a window check. The memory
exhaustion seen in earlier macOS runs is still unexplained. Milestone 5C is not
complete.
