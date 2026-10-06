# §29B — verification record

[The contract and recipe](edit-chamfer-base-height.md).

## Base and scope

Base `main` = `06d1e60bc9f37a0daa661f1286e5ba63af0b1751`, tree
`47bc814d006344381b90c8d25ef5cd63b0e11e9c` (PR #77, §29A merged). The branch is
`edit-chamfer-base-height`. §29A was not redone and `main` was not rolled back.
The post-merge runs of #77 on `06d1e60` were still running when this work began
(ordinary CI and the combined runtime layout were `in_progress`; the planegcs pin
and both SBOM workflows were `success`); they are not counted here, and this PR's
runs are told apart from them below. Milestone 5C is **not** declared complete.

## Where and how this was run

Local, in the cloud container: Linux, Open CASCADE 8.0.1 and PlaneGCS built from
local inputs — **the local PlaneGCS is not the pinned one**; the pinned
`planegcs pin` workflow and the three-platform runtime layout are
authoritative. macOS is Apple Silicon (arm64) only. No window, GPU or Unity ran
here. One heavy process at a time, 2–3 jobs; OCCT, Boost and PlaneGCS were not
rebuilt. The disk had 5.5 GiB free before heavy work (`df`, one measurement).

## What changed

The existing `edit-extrude` and the **Edit extrusion** form now accept the base
Extrude under the one §29A Chamfer. The class is read by the distance edit's own
reader (`saved_chamfer`); the prepared edit carries it, the writer re-derives it
and updates one row. There is no new command, copier, request format, payload,
capability or archive/SQLite version. The only wire addition is
`features[].chamfer_base`. See the contract for the measured height policy.

## New tests

- Document (`chamfer::tests`, 2 new, 2 flipped): the height changes one row,
  refs/dependencies/payloads of everything else unchanged, the valid distance
  edit after it, stale and forged prepared edits (another row, the Chamfer
  removed, the Chamfer's distance or joint changed) written nowhere; a
  dimension, a Fillet, a Cut, a second Chamfer and a second Extrude each refused
  with the guilty UUID. The earlier "every other editor refuses" and "height
  refused" assertions are flipped, not weakened: the height is the one edit.
- CLI (`tests/fillet/base_height.rs`, 5): every corner in three drawing orders
  taller; shorter and the cache (old copy fills it, the new height misses for the
  plate and the Chamfer, a second run hits) and the following distance edit;
  the chosen corner against another with the same volume (the other corner's
  mesh test must fail); refusals, races and guards (stale version, no clobber,
  alias, closed pipes → exit 7, the kernel's own limit at 1e-6 mm typed
  `kernel`); a Sketch edit still names the Chamfer after a height edit;
  discovery and the protocol without a kernel.
- App (2): the form shows the Chamfer, refuses 0/negative/text, keeps the draft
  through Save Cancel, a stale reply and a worker refusal; the worker's copy and
  the CLI's are SQL-cell-equal with byte-equal STL/FBX. The height form has no
  Undo/Redo and none is claimed.

## Local results

Full regression (document, jobs, eval, CLI test targets, app, solver info;
planegcs, one run): 1173 passed, 1 old ignored benchmark, 2 failed — the same two
tests that need a read-only file to be unreadable and fail as root in this
container (`read_only_permissions_still_dump_when_the_file_can_be_read`,
`validation_really_read_only_permissions`; they fail on `main` here too). The
two mutations above were each restored and this run is after the restore.
`cargo fmt --check`, workspace clippy (`--all-targets --features planegcs -- -D
warnings`), the licence-header and export-boundary scripts and `git diff --check`
are clean. `actionlint` is not installed here; both edited workflows were
parsed as YAML, and no `run:` step is over GitHub's 21 000-character limit
(largest 20 431 bytes, unchanged; the Chamfer step is far smaller).

Packed argv, executed: the stub step of `ci.yml` was extracted and run against a
real no-kernel build (`CARGO_TARGET_DIR=/home/user/stub-target`): all gates `ok`
and `FCAD_29A_RECIPE_NO_KERNEL`, `FCAD_29B_RECIPE_NO_KERNEL`. The Chamfer step
of `runtime-layout.yml` was extracted and run on the native debug build (the
flags `--release`, library paths and artifact directory adapted; nothing else): every
named gate `ok`, 54 FBX/STL artifacts written. OCCT without the solver
(release, `--no-default-features`, `FERRITECAD_REQUIRE_PLANEGCS=0`,
`FCAD_PLANEGCS_DIR` unset): the kernel-free gate and two native gates `ok`, recipe
`FCAD_29B_RECIPE_OK`. That run found a stale line in the §29A recipe (its
refusal text now names the UUID); fixed. The twelve recipes 28A–28L and both
29A and 29B ran against this build: all `…_RECIPE_OK`. Six new FBX/STL pairs were
read by pinned ufbx (`FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0`) and
joined with their STL (`FCAD_STL_FBX_MATCH triangles=16`).


### Mutations — local, executed, restored

Each compiled, failed on an executed assertion and was restored byte for byte
(`git status` clean), after which the positive gates passed again:

1. The Chamfer's cache key ignores the key of the plate under it (the lost
   height downstream): the cached run after a height edit reports `Hit` for the
   Chamfer where `Miss` is required
   (`the new height must not hit the old plate`).
2. The writer's re-derivation no longer compares the prepared edit with the
   document: `write_extrude_height` accepts an edit whose Chamfer was removed
   (`assertion failed: d.write_extrude_height(&without).is_err()`).

### Compatibility with the reader on `main`

`main` (`06d1e60`) built with the same solver, against a copy this build
made: `inspect` works (without `chamfer_base`, which it does not know),
`validate` is valid, `rebuild --cold` resolves every name, `export-stl` and
`export-fbx` are byte-identical to this build's, its `edit-extrude` still
refuses the plate naming the Chamfer, its `edit-chamfer-distance` works on the
copy, and the copy's bytes are unchanged (`FCAD_29B_COMPAT_OK`). A document
`main` made was edited by this build.

## CI

Pull request [#78](https://github.com/gesriot/ferrite-cad/pull/78), **code head**
`f05584e4ca42184a6a6b6afee60e38bf580ec3fe` (later commits change only this
record). These are the PR's runs, not post-merge runs of `main`.

- Ordinary CI [36946074908](https://github.com/gesriot/ferrite-cad/actions/runs/36946074908): success (includes the stub step with the §29B discovery/protocol, widget and document gates and `FCAD_29B_RECIPE_NO_KERNEL`).
- Combined runtime layout [36946056568](https://github.com/gesriot/ferrite-cad/actions/runs/36946056568): success on Linux, macOS and Windows plus the three-platform comparison (the OCCT-without-solver `FCAD_29B_RECIPE_OK` step, the native Chamfer step with the five `base_height` CLI gates, two document gates, two app gates and the recipe, and the pinned-ufbx step with `FCAD_CHAMFER_HEIGHT_UFBX_EXECUTED`).
- planegcs pin [36946056443](https://github.com/gesriot/ferrite-cad/actions/runs/36946056443): success.
- The post-merge runs of `main` at the base `06d1e60` (ordinary CI [36942146644](https://github.com/gesriot/ferrite-cad/actions/runs/36942146644), runtime layout [36942146856](https://github.com/gesriot/ferrite-cad/actions/runs/36942146856), planegcs pin, notices and both SBOMs) were `in_progress` when this work began and have since all completed `success`; they are the base's, not this PR's.

Only step conclusions and job metadata were read; the full job logs were not
downloaded, so per-step test counts are an inference from the gates (each fails its
job unless its exact test passed and each marker was grepped). **Final docs
head:** the CI of the commit that carries this section is reported in the PR
and not here, since a document cannot name the run of its own commit.

## Limits

Out of scope and refused: Sketch or constraint edits under a Chamfer, a second
Chamfer, a Fillet or a Cut beside it, another edge, ThroughAll, an arbitrary
plane, chains, in-place Save and live preview. The height policy is the shared
rule plus the kernel's own measured limit (1e-5 mm and below fails on OCCT
8.0.1; 1.2e-5 builds); that limit is OCCT's, reported as `kernel`, not a domain
bound. The local PlaneGCS is not the pinned one. No window ran here. The
memory exhaustion seen in earlier macOS runs is still unexplained.

## macOS fixture and window scenario (for the user's Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised with CLI calls
standing in for the window, which says nothing about the window:
`FCAD_29B_GUI_COMPARE_OK cells=384 triangles=16`. Controls rejected: a first
height of 3.5 instead of 3.25, a second of 9.25 instead of 9.5, a missing file
(`FCAD_29B_GUI_COMPARE_MISSING …`, nothing created), a `gui-refused.fcad`, the
files of a plate chamfered at another corner, a changed byte in a refs row, and
a vertex moved in the STL.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon Mac, with a fresh arm64 bundle, and do not set
`FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `chamfer.fcad` — a free plate, 37.5 × 12.25 × 6.75 mm, drawn clockwise
from the upper right, offset and fractional, with one Chamfer at (33, 3.25),
d 2.375 mm — and `facts.json`.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-chamfer-base-height-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_29B_GUI_FIXTURE\n", "ferrite-29b-fixture.py"),
                   ("# FCAD_29B_GUI_COMPARE\n", "ferrite-29b-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_29B_DIR="$PWD/chamfer-height-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-29b-fixture.py "$FCAD_29B_DIR"
```

```python
# FCAD_29B_GUI_FIXTURE
import hashlib, json, math, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "chamfer-height-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# A free plate, clockwise from the upper right, offset and fractional, no
# constraint: corners C0 (33, 15.5), C1 (33, 3.25), C2 (-4.5, 3.25), C3 (-4.5,
# 15.5); sides 12.25 mm (C0-C1, C2-C3) and 37.5 mm. One equal-distance Chamfer
# at C1, d 2.375 mm; the window changes the plate's height, then the distance.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
H0, H1, H2 = 6.75, 3.25, 9.5
CORNER, D1, D2 = [33.0, 3.25], 2.375, 4.5
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H0, "points_mm": PLATE}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
catalog = run("inspect", plate, "--json")["result"]
candidate = next(c for c in catalog["bodies"][0]["chamfer_edge"]["target"]["candidates"]
                 if c["corner_mm"] == CORNER)
request.write_text(json.dumps({"request_version": 1, "edge": candidate["edge"], "distance_mm": D1}))
source = out / "chamfer.fcad"
run("chamfer-edge-copy", plate, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", source, "--json")
request.unlink()
plate.unlink()
c = run("inspect", source, "--json")["result"]
(base,) = c["features"]
(chamfer,) = c["chamfers"]
assert base["chamfer_base"]["chamfer_feature_id"] == chamfer["feature_id"] and c["edit_extrude"]["available"] is True
(sketch,) = c["sketches"]
facts = {
    "body_id": c["bodies"][0]["body_id"], "base_feature_id": base["feature_id"],
    "chamfer_id": chamfer["feature_id"], "edge": chamfer["edge"], "sketch_id": sketch["sketch_id"],
    "plate_mm": [-4.5, 3.25, 37.5, 12.25], "corner_mm": CORNER,
    "heights_mm": {"source": H0, "short": H1, "tall": H2},
    "distances_mm": {"source": D1, "final": D2},
    # What the window is asked to type, and which refusals it must show.
    "refused_validation": "0", "refused_kernel": "0.000001",
    "content_version": c["content_version"],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_29B_GUI_FIXTURE_OK", out)
```

### Window scenario

> **Current viewer (§30H):** in the distance form, the button these historical steps
> call **Apply distance** is now **Confirm draft number**; it is what readies
> **Save distance copy…**. The new **Apply distance** changes the open document
> through the session instead ([contract](chamfer-distance-session-apply.md)).

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_29B_DIR/../watch-29b.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

If system memory pressure aborts or stops the run, it does not count; start
again from a fresh fixture directory. **Do not address the viewer after Quit
through the automation**: it may relaunch the app outside the watchdog. If it was
relaunched, quit that instance too and read only the original PID's exit and
watchdog record. Type numbers exactly as written.

1. **Open** `$FCAD_29B_DIR/chamfer.fcad` (asynchronously). The feature list
   shows Extrude → Chamfer; **Edit extrusion…** is enabled and **Chamfer edge of …**
   is not (a second Chamfer is refused).
2. **Refusals.** **Edit extrusion…**, choose the base Extrude (it lists 6.75 mm
   and the context line "Chamfered by Chamfer … at (33, 3.25), d 2.375 mm. The
   Chamfer keeps its edge and distance; only the plate's height changes."). Type
   `0` → refused (positive distance), **Save new file…** is unavailable. Type
   `0.000001` (1e-6 mm, below what OCCT can chamfer) → the form accepts the
   number; **Save new file…** → `$FCAD_29B_DIR/gui-refused.fcad`: the job runs and
   is refused by the kernel, the draft stays, **no file is published**.
3. **Save Cancel.** Type `3.25`; **Save new file…** → **Cancel** in the file
   dialog (`gui-cancelled.fcad` is never created): nothing starts, the draft stays.
4. **Shorter.** **Save new file…** → `$FCAD_29B_DIR/gui-short.fcad`; it opens
   asynchronously. The Chamfer and its corner are unchanged; the plate is 3.25 mm.
5. **Taller.** In `gui-short.fcad`, **Edit extrusion…**, base Extrude, type `9.5`
   → **Save new file…** → `$FCAD_29B_DIR/gui-tall.fcad`; it opens.
6. **Distance.** **Edit Chamfer distance…** on the Chamfer, **New distance** `4.5`
   → **Apply distance** → **Save distance copy…** →
   `$FCAD_29B_DIR/gui-distance.fcad`; it opens.
7. **Exports.** Export `gui-distance.stl` and `gui-distance.fbx` of
   `gui-distance.fcad` into `$FCAD_29B_DIR`, at the default tessellation.
8. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-29b-compare.py "$FCAD_29B_DIR"
```

It first requires `gui-short.fcad`, `gui-tall.fcad`, `gui-distance.fcad`,
`gui-distance.stl` and `gui-distance.fbx` (and never creates them), refuses a
`gui-refused.fcad` or `gui-cancelled.fcad`, checks the source's SHA-256, makes the
CLI peers (shorter, taller, distance), checks each window edit's allowlist (one
object row's payload and hash and the stamp; nothing minted), the same Chamfer
UUID, corner, edge and size in every document, the saved names under the same
UUIDs and resolving after a cold rebuild, every SQL cell equal to the peer's
(nothing is mapped: no UUID is minted), byte-equal STL/FBX, and then reads the
STL itself — closed, one winding, the plate's bounds, the volume
`(W·D − d²/2)·h`, the three whole corners, the cut at the chosen corner and its
one flat (plane, outward normal, area `d·√2·h`) — for the window's export and,
through the CLI, for each saved step at its own height.
It prints `FCAD_29B_GUI_COMPARE_OK cells=N triangles=M`.

```python
# FCAD_29B_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-short.fcad", "gui-tall.fcad", "gui-distance.fcad", "gui-distance.stl", "gui-distance.fbx")
missing = [n for n in GUI if not (out / n).is_file()]
if missing:
    sys.exit(f"FCAD_29B_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first")
for n in ("gui-refused.fcad", "gui-cancelled.fcad"):
    assert not (out / n).exists(), f"a refused or cancelled Save published {n}"
source = out / "chamfer.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
BASE, CHAMFER, BODY = facts["base_feature_id"], facts["chamfer_id"], facts["body_id"]
H = facts["heights_mm"]
D = facts["distances_mm"]
X0, Y0, W, DEPTH = facts["plate_mm"]
CORNER = facts["corner_mm"]
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
def uuid_bytes(text):
    return bytes.fromhex(text.replace("-", ""))
def allowlist_row(before, after, row):
    """Exactly one object row's payload and hash and the stamp moved; nothing
    was minted, so no table gained or lost a row."""
    rid = uuid_bytes(row)
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
def stl_of(path, scratch):
    target = scratch / (path.stem + ".stl")
    run("export-stl", path, "-o", target, "--json")
    return target
def mesh_checks(stl_path, height, distance):
    """The mesh is read here, independently of the B-Rep: closed, one winding,
    the plate's bounds, the volume, the three whole corners, the chosen corner
    cut and the one flat facing out of the plate with area d*sqrt(2)*h."""
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
    for got, want in ((min(xs), X0), (max(xs), X0 + W), (min(ys), Y0), (max(ys), Y0 + DEPTH), (min(zs), 0.0), (max(zs), height)):
        assert abs(got - want) < 1e-4, ("bounds", got, want)
    exact = (W * DEPTH - distance * distance / 2) * height
    assert abs(six / 6 - exact) < 1e-3, ("volume", six / 6, exact)
    corners = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + DEPTH], [X0, Y0 + DEPTH]]
    near = lambda c, z: any(abs(pt[0] - c[0]) < 1e-4 and abs(pt[1] - c[1]) < 1e-4 and abs(pt[2] - z) < 1e-4 for pt in pts)
    for c in corners:
        for z in (0.0, height):
            assert near(c, z) == (c != CORNER), ("corner", c, z)
    ix = -1.0 if abs(CORNER[0] - (X0 + W)) < 1e-9 else 1.0
    iy = -1.0 if abs(CORNER[1] - (Y0 + DEPTH)) < 1e-9 else 1.0
    for z in (0.0, height):
        assert near([CORNER[0] + ix * distance, CORNER[1]], z) and near([CORNER[0], CORNER[1] + iy * distance], z), z
    flat = 0.0
    for a, b, c in tri:
        if all(abs(ix * (v[0] - CORNER[0]) + iy * (v[1] - CORNER[1]) - distance) < 1e-4 for v in (a, b, c)):
            u = [b[i] - a[i] for i in range(3)]
            w = [c[i] - a[i] for i in range(3)]
            n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]]
            length = math.sqrt(sum(x * x for x in n))
            assert (-ix * n[0] - iy * n[1]) / length / math.sqrt(2) > 1 - 1e-4, "the flat faces the plate"
            flat += length / 2
    assert abs(flat - distance * math.sqrt(2) * height) < 1e-3, ("flat", flat)
    return count
def peer_height(src, height, dest):
    dest.unlink(missing_ok=True)
    run("edit-extrude", src, "--feature", BASE, "--distance-mm", str(height), "--expect-version",
        run("inspect", src, "--json")["result"]["content_version"], "-o", dest, "--json")
def peer_distance(src, distance, dest):
    dest.unlink(missing_ok=True)
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "distance_mm": distance}))
    run("edit-chamfer-distance", src, "--feature", CHAMFER, "--expect-version",
        run("inspect", src, "--json")["result"]["content_version"], "--request", request, "-o", dest, "--json")
    request.unlink()
g = {"short": out / "gui-short.fcad", "tall": out / "gui-tall.fcad", "distance": out / "gui-distance.fcad"}
p = {n: out / f"peer-{n}.fcad" for n in g}
peer_height(source, H["short"], p["short"])
peer_height(p["short"], H["tall"], p["tall"])
peer_distance(p["tall"], D["final"], p["distance"])
# Allowlists on the window's own chain: each step moves exactly what it may.
allowlist_row(source, g["short"], BASE)
allowlist_row(g["short"], g["tall"], BASE)
allowlist_row(g["tall"], g["distance"], CHAMFER)
# What each window document says: the same Chamfer at the chosen corner, the
# size as typed, every saved name resolving after a cold rebuild.
def chamfer_of(path, height, distance):
    result = run("inspect", path, "--json")["result"]
    (row,) = result["chamfers"]
    assert row["feature_id"] == CHAMFER and row["distance_mm"] == distance and row["corner_mm"] == CORNER, row
    assert row["edge"] == facts["edge"] or sorted(row["edge"]["joint"]) == sorted(facts["edge"]["joint"]), row
    (feature,) = result["features"]
    assert feature["feature_id"] == BASE and feature["distance_mm"] == height, feature
    assert feature["chamfer_base"]["chamfer_feature_id"] == CHAMFER and feature["chamfer_base"]["corner_mm"] == CORNER
    assert feature["chamfer_base"]["distance_mm"] == distance
    assert result["bodies"][0]["body_id"] == BODY
    assert result["bodies"][0]["chamfer_edge"]["available"] is False, "a second Chamfer is offered"
    assert run("validate", path, "--json")["result"]["valid"] is True
    n = len(tables(path)["topology_refs"][1])
    text = run("rebuild", path, "--cold")
    assert "tip Chamfer" in text and f"{n} of {n} stored references resolved" in text, text
chamfer_of(g["short"], H["short"], D["source"])
chamfer_of(g["tall"], H["tall"], D["source"])
chamfer_of(g["distance"], H["tall"], D["final"])
# The saved names are the source's, under the same UUIDs, at every step.
refs = [r[0] for r in tables(source)["topology_refs"][1]]
for n in g:
    assert [r[0] for r in tables(g[n])["topology_refs"][1]] == refs, f"{n}: a name changed UUID"
# Every cell of every window document equals the shipped CLI's, nothing mapped:
# no operation here mints a UUID. Payloads, hashes, schema versions, references
# and dependencies all take part; only each copy's own stamp is left out.
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
# The exports of the window's last document are the CLI's, byte for byte.
for fmt in ("stl", "fbx"):
    target = out / f"peer-distance.{fmt}"
    target.unlink(missing_ok=True)
    run(f"export-{fmt}", p["distance"], "-o", target, "--json")
    assert (out / f"gui-distance.{fmt}").read_bytes() == target.read_bytes(), fmt
# The geometry, by an independent reader of binary STL: the window's own export
# at the final height and distance, and each saved step at its own height.
count = mesh_checks(out / "gui-distance.stl", H["tall"], D["final"])
with tempfile.TemporaryDirectory(prefix="ferrite-29b-compare-") as scratch:
    scratch = pathlib.Path(scratch)
    mesh_checks(stl_of(g["short"], scratch), H["short"], D["source"])
    mesh_checks(stl_of(g["tall"], scratch), H["tall"], D["source"])
print("FCAD_29B_GUI_COMPARE_OK", f"cells={cells}", f"triangles={count}")
```

## Independent review and macOS window run (2026-10-01)

Review found a publication defect outside the seven Chamfer-owned names:
`CopyWrite::Height` used the weaker legacy reference policy whenever Cut and
Fillet were absent, which also admitted a Chamfer. A document with one extra
base-owned `ExtrudeSide` referring to a nonexistent segment passed structural
validation and preparation; cold rebuild resolved all but that name, yet
`edit-extrude --json` published with exit 0. The new process regression first
failed on that actual exit (expected 2), not on compilation. A positive control
adds an extra, valid base reference and still publishes and preserves it.

Review fix `8ba7871` adds `p.chamfer().is_none()` to the legacy-only exception.
Every saved name must resolve before and after a Chamfer height edit. The gate
`chamfer::base_height::native_chamfer_height_requires_every_saved_reference_to_resolve`
is required by the existing three-platform Chamfer step (38 exact executions
now, previously 37). Bare-height compatibility is unchanged.

Local arm64 verification used the existing pinned OCCT 8.0.1 and PlaneGCS,
release target `/private/tmp/ferrite-24b-native-target`, two build jobs and one
heavy command at a time. The final affected matrix executed **602 tests**:
497 document/jobs/eval, 85 CLI Fillet/Chamfer, 16 app edit and four app Chamfer.
Four explicitly marked no-solver-only cases were N/A in this solver build;
one old timing benchmark was ignored. The six height tests and 11 shared
copy-job tests also passed separately after the failing-first reproduction.
Fmt, workspace clippy all targets/features with `-D warnings`, actionlint,
licence headers (400 files), export boundary and diff whitespace passed.
Native libraries were reused, not rebuilt; the large STEP corpus was left to CI.

A fresh arm64 app bundle was staged from the reviewed CLI/viewer, its closure
contained 50 OCCT libraries and PlaneGCS with no unexpected libraries, and
`codesign --verify --deep --strict` and bundled `--solver-info` passed with DYLD
variables unset. The Markdown recipe ran with that bundled CLI:
`FCAD_29B_RECIPE_OK d=2.375 h=9.5 volume=4337.269531/4337.269531`.

The actual window used the extracted fixture, with no CLI-generated substitutes
for its outputs. Observed in one guarded process:

- Open `chamfer.fcad`; Edit extrusion enabled and second Chamfer unavailable.
- Correct saved Chamfer UUID, corner `(33, 3.25)` and distance `2.375` shown.
- Height `0` refused by the form; `0.000001` refused by OCCT on Save, with the
  draft and original scene retained and no `gui-refused.fcad` published.
- Save Cancel at `3.25` retained the draft and created no cancelled output.
- Height `3.25`, then `9.5`, then Chamfer distance `4.5`: three real Save dialogs,
  three publications and asynchronous Open, followed by STL and FBX export.
- Normal Quit; no viewer CUA handle was accessed after Quit.

The unchanged Markdown comparator then checked all three window copies against
the bundled CLI: **384 SQL cells**, source hash, narrow per-step allowlists,
all saved identities/names, cold resolution, byte-identical STL and FBX, and
independently parsed geometry at the selected corner. Its result was
`FCAD_29B_GUI_COMPARE_OK cells=384 triangles=16`. The final STL is 884 bytes;
FBX is 5429 bytes. Pinned ufbx 0.23.0 reported six checks and zero failures;
the independent oriented join reported 16 triangles, worst error
`8.67e-19` metres. Review controls on private copies rejected a real 3.5-mm
publication where 3.25 was required, an unrelated `objects.name` change, and
missing GUI outputs. These controls did not modify the window evidence.

Watchdog PID 87463 exited 0, not aborted. Peak measured physical footprint was
**209.626 MiB**, pressure stayed 1 (normal), swap stayed 630521856 bytes, free
disk stayed above 136 GiB. Limit: 1536 MiB. The CUA guard twice paused because
the latest watchdog record was its periodic compression observation; it then
checked the latest fresh PID sample while still rejecting exit/abort records.
No viewer was killed or relaunched. One initial paste into Go To timed out;
setting the native path field completed the same dialog. Neither incident is
counted as a product defect. The earlier OOM remains unexplained.

Evidence (logs, screenshots, actual GUI/CLI files, negative controls and audits)
was retained under `/private/tmp/ferrite-pr78-review`, with a durable review
copy outside the repository. Full original runtime logs, rather than only
job conclusions, independently confirm 37 Chamfer executions, two §29B recipe
markers and six new ufbx reads/oriented joins on each OS. The review fix starts
fresh [ordinary CI](https://github.com/gesriot/ferrite-cad/actions/runs/36965672343)
and [native runtime CI](https://github.com/gesriot/ferrite-cad/actions/runs/36965668763).
Ordinary CI has completed all seven jobs successfully; completion and log audit
of the fixed-code native run remain mandatory before merge. This record does
not call a pending run successful.
