# §29C — move the corners of a chamfered plate, in a new copy

[Executed verification and limitations](edit-chamfer-base-sketch-verification.md).
[The Chamfer this keeps](rectangular-corner-chamfer.md);
[the height edit beside it](edit-chamfer-base-height.md);
[the same edit under a Fillet](edit-fillet-base-sketch.md).

A person opens a saved rectangular plate with the one §29A Chamfer, changes the
four stored vertices of its base Sketch in the existing **Edit saved Sketch**
form (drag, exact coordinates, Undo/Redo, Restore saved vertices) and saves a
new `.fcad`. An agent does the same with the existing `edit-sketch-copy`. The
plate may be made longer or shorter on either side and may be moved. The
Chamfer stays on the **same corner** — the corner its two Line UUIDs meet at —
with the same distance; the height, the Body tip, the history and every name
are kept. There is no new command, request, copier or history table.

## Contract recorded before implementation

### The supported source and candidate

The class is exactly §29A/§29B, read by the one reader the distance and height
edits use (`chamfer::saved_chamfer`, over `cut_edit::saved_history_under_chamfer`):
one untransformed XY datum; one Sketch of four Lines forming an axis-aligned
rectangle that is free or closure-only (the four Coincident links, kept
byte-for-byte); one forward literal Blind `Extrude`/`NewBody`; one terminal
Chamfer on a corner of that rectangle; one Body whose tip is the Chamfer;
exactly the Chamfer's seven owned names, and no other model objects.
Additional base-owned references are allowed when they resolve; every saved
reference must resolve both before and after the edit.

The request is the existing one: the four `curve_id`s once each in stored
order with new start points. A candidate is accepted if and only if **all**
of these hold; the first violated rule is the refusal, naming the Chamfer where
it is the Chamfer's:

1. **The existing coordinate policy**, unchanged and shared: the saved UUIDs in
   saved order, finite numbers, the `PolygonExtrusion` policy of the Extrude the
   Sketch feeds (a simple polygon of non-degenerate Lines), and the saved
   winding. Nothing is reordered, re-sorted or re-wound.
2. **Still an axis-aligned rectangle of four Lines** (the same
   `corners_of_lines` reader the creation uses), so the Chamfer's corner is read
   again on the candidate.
3. **Every Line keeps its side** (`keeps_every_side`): each Line runs along the
   same axis in the same direction as saved. This is what makes the corner
   *follow its vertex*: a candidate that mirrors the plate, or moves the
   Chamfer's two Lines to the opposite corner, is refused, because the saved
   joint would then be a different corner of the part. Sides may be made longer
   or shorter and the whole rectangle may be moved, in any combination, as long
   as no Line flips.
4. **The saved joint is still one of its corners**, found by the two Line UUIDs
   alone (`corner_for`), never by an index or an absolute coordinate.
5. **The saved distance still fits the NEW adjacent sides**, under the one §29A
   policy and its one expression: `0.001 mm ≤ d ≤ min(adjacent sides) − 0.01 mm`,
   exact at the bound (the largest accepted value is accepted, the next
   representable value above it is refused). `d` is never reduced, clamped or
   rewritten, and the Chamfer's row is not touched.

The refusal of rule 5 reads "Chamfer `<uuid>` of `<d>` mm does not fit the new
plate: … the shorter adjacent side would be … mm, so it may be at most … mm". A
degenerate or non-rectangular candidate is refused by rule 1 or 2 with the
reason the shared reader gives; the other Chamfer-class refusals (a Fillet or Cut
beside it, a second Chamfer, a dimension on the Sketch, an extra Chamfer-owned name) name the
guilty feature or constraint UUID as for the height edit.

Validation is one function, `SketchChoice::validate_coordinates`, used by the
form while typing, by `edit-sketch-copy` before the copy, and again by the
writer inside its transaction; the UI and the CLI hold no copy of it.

### Identity — what may change

The only model change is the base Sketch row's `payload` and `payload_hash` (the
Line coordinates), plus the established `meta.modified_at` stamp. The Extrude
(height), the Chamfer (UUID, `previous`, edge, joint, distance), every
dependency, every topology reference, every other payload, names, ordinals and
parents, the Body tip and every capability row are preserved byte for byte.
Nothing is minted, deleted or recreated, and no solved geometry is stored. The
Chamfer's seven names and any additional saved references resolve under the
same UUIDs on the new solid, with the Chamfer's
plane now at the new corner position.

### Mechanism (all shared)

* **Discovery** from the one pinned snapshot: `sketches[]` gains the additive
  `chamfer_base` (the same object as `features[].chamfer_base`, §29B) on the base
  Sketch of a chamfered plate and `null` elsewhere; the Sketch row is
  `editable` exactly when the shared reader accepts the frame. Every existing
  field and type is unchanged.
* **Preparation** `replace_sketch_coordinates` reads the Chamfer through the
  frame (`coordinate_choice`), validates, and rewrites only the Line starts and
  ends of the selected Sketch.
* **Write** `write_sketch_geometry` re-derives the prepared payload in its
  transaction from the current document through the same function; a forged or
  stale payload is refused and nothing is written.
* **Job** the existing `edit_object_copy`: pinned snapshot and version guard,
  source/alias/no-clobber, strict cold rebuild (the evaluator judges the Chamfer
  again on the Lines it built), every saved reference must resolve before and
  after (the rule `CopyWrite::Coordinates` already has and the §29B review
  required for the height; it is not weakened and is not replaced by a count of
  the Chamfer's own seven names), cancellation and cleanup, atomic publication,
  exit 7 after publication.
* **Cache** unchanged: the Extrude's key follows its profile, the Chamfer's key
  the plate's, so a moved plate misses for both and never returns the old
  contour or corner.

### UI

The existing coordinate form names the Chamfer to keep ("Chamfered by Chamfer …
at the corner of Lines a | b, d … mm. The Chamfer keeps its corner and
distance: every Line keeps its side, and the shorter adjacent side may not be
shorter than … mm"). Undo/Redo, **Restore saved vertices** and drag (one gesture,
one Undo) are unchanged. While typing, the draft may be briefly not a
rectangle; the refusal is shown, Undo/Redo/Restore stay available and Save is
unavailable until the whole candidate is valid. No neighbouring vertex is moved
automatically to make a gesture succeed. A refusal and **Save Cancel** keep the
draft; a published copy opens asynchronously.

### Out of scope, refused

A second Chamfer, a Fillet or Cut beside it, a dimension or any non-closure
constraint (the constraint editors still refuse a chamfered plate by its UUID), a
different edge, ThroughAll, an arbitrary plane, in-place Save and live preview.
No new payload, capability, archive or schema version is needed or added.
Milestone 5C is not complete.

## Agent recipe

Extract the code between the markers and run it with the real command line:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-chamfer-base-sketch.md").read_text(encoding="utf-8")
code = text.split("# FCAD_29C_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-29c-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-29c-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_29C_RECIPE_NO_KERNEL`; Open CASCADE alone (no solver) runs it to the end,
because the class is free or closure-only. It prints `FCAD_29C_RECIPE_OK` with
the exact and the measured volumes of the last edited plate.

```python
# FCAD_29C_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-29c-"))

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def geometry(args, out):
    """A step that needs the kernel: a build without one refuses typed."""
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 2 and not out.exists():
        error = json.loads(p.stdout)["error"]
        if error["kind"] == "unsupported" and "Open CASCADE" in error["message"]:
            print("FCAD_29C_RECIPE_NO_KERNEL", json.dumps(error))
            sys.exit(0)
    assert p.returncode == 0, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout)

def tables(path):
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    out = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        out[t] = ([d[0] for d in cur.description], sorted(cur.fetchall(), key=repr))
    db.close()
    return out

def only_row(source, copy, row_id):
    """One object row's payload and hash (and the stamp) may differ; nothing else."""
    rid = bytes.fromhex(row_id.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    moved = 0
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc and len(arows) == len(brows), t
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
    return moved

def stl(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    tri = [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)]
           for i in range(count)]
    six = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2])
              + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in tri)
    return six / 6, tri

X0, Y0, W, D, H = -4.5, 3.25, 37.5, 12.25, 6.75

def corner_of(rect):
    """The lower right corner of the counter-clockwise plate: the vertex the
    Chamfer's two Lines meet at, wherever the plate has been moved to."""
    return [rect[0] + rect[2], rect[1]]

def vertices_of(catalog, rect):
    """Every saved Line start sent to the same corner of `rect`, in saved order."""
    x0, y0, w, d = rect
    lx = min(v["start_mm"][0] for v in catalog["sketches"][0]["vertices"])
    ly = min(v["start_mm"][1] for v in catalog["sketches"][0]["vertices"])
    return [{"curve_id": v["curve_id"],
             "start_mm": [x0 if v["start_mm"][0] == lx else x0 + w,
                          y0 if v["start_mm"][1] == ly else y0 + d]}
            for v in catalog["sketches"][0]["vertices"]]

def measured(copy, rect, distance, height):
    """After reopening: valid, a cold rebuild resolves every name, the exact
    analytic volume (w*d - dist*dist/2)*h, and the independently read mesh is
    closed at the chosen corner of the NEW plate: the other three corners are
    whole, the cut's two new vertex columns are `distance` along each adjacent
    side from z=0 to z=h, and the flat adds up to distance*sqrt(2)*h and faces
    out of the plate."""
    x0, y0, w, d = rect
    corner = corner_of(rect)
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    n = len(tables(copy)["topology_refs"][1])
    text = run(["rebuild", copy, "--cold"])
    assert "tip Chamfer" in text and f"{n} of {n} stored references resolved" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--json"])
    volume, tri = stl(out)
    exact = (w * d - distance * distance / 2) * height
    assert abs(volume - exact) < 1e-3 * max(1.0, exact / 1000), (volume, exact)
    flat = 0.0
    for a, b, c in tri:
        if all(abs(-(p[0] - corner[0]) + (p[1] - corner[1]) - distance) < 1e-4 for p in (a, b, c)):
            u = [b[i] - a[i] for i in range(3)]
            v = [c[i] - a[i] for i in range(3)]
            n_ = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
            length = math.sqrt(sum(x * x for x in n_))
            assert (n_[0] - n_[1]) / length / math.sqrt(2) > 1 - 1e-4, "the flat faces the plate"
            flat += length / 2
    assert abs(flat - distance * math.sqrt(2) * height) < 1e-3, flat
    pts = [p for t in tri for p in t]
    for z in (0.0, height):
        for x, y in ([corner[0] - distance, corner[1]], [corner[0], corner[1] + distance]):
            assert any(abs(p[0] - x) < 1e-4 and abs(p[1] - y) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in pts), (x, y, z)
        assert not any(abs(p[0] - corner[0]) < 1e-4 and abs(p[1] - corner[1]) < 1e-4
                       and abs(p[2] - z) < 1e-4 for p in pts), "the cut corner still has a vertex"
        for c in ([x0, y0], [x0 + w, y0 + d], [x0, y0 + d]):
            assert any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in pts), ("another corner is gone", c, z)
    assert abs(max(p[2] for p in pts) - height) < 1e-4
    assert abs(min(p[0] for p in pts) - x0) < 1e-4 and abs(max(p[0] for p in pts) - (x0 + w)) < 1e-4
    out.unlink()
    return volume

# 1. A plate, drawn offset and fractional, and the one Chamfer of the 29A recipe.
request = root / "plate.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H,
                               "points_mm": [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", request, "-o", plate, "--json"], plate)
catalog = inspect(plate)
candidate = next(c for c in catalog["bodies"][0]["chamfer_edge"]["target"]["candidates"]
                 if c["corner_mm"] == corner_of([X0, Y0, W, D]))
d1 = 2.375
request.write_text(json.dumps({"request_version": 1, "distance_mm": d1, "edge": candidate["edge"]}))
one = root / "one.fcad"
geometry(["chamfer-edge-copy", plate, "--body", catalog["bodies"][0]["body_id"],
          "--expect-version", catalog["content_version"], "--request", request,
          "-o", one, "--json"], one)

# 2. Discovery: the base Sketch carries the Chamfer as context, the same object
#    the base Extrude carries; a plain plate has none; the older fields keep types.
saved = inspect(one)
chamfer = saved["chamfers"][0]
sketch = saved["sketches"][0]
assert sketch["editable"] is True and sketch["refusal"] is None
assert sketch["chamfer_base"] == saved["features"][0]["chamfer_base"]
assert sketch["chamfer_base"] == {"chamfer_feature_id": chamfer["feature_id"],
                                  "body_id": saved["bodies"][0]["body_id"], "edge": chamfer["edge"],
                                  "corner_mm": corner_of([X0, Y0, W, D]), "distance_mm": d1,
                                  "distance_unit": "mm"}
assert sketch["fillet_base"] is None and sketch["fillet_history"] is None
assert inspect(plate)["sketches"][0]["chamfer_base"] is None

def ask(catalog, rect):
    request.write_text(json.dumps({"request_version": 1, "vertices": vertices_of(catalog, rect)}))
    return request

def resketch(source, rect, out, code=0):
    before = inspect(source)
    ask(before, rect)
    return run(["edit-sketch-copy", source, "--sketch", before["sketches"][0]["sketch_id"],
                "--expect-version", before["content_version"], "--request", request,
                "-o", out, "--json"], code=code)

# 3. Larger and moved, smaller and moved, only moved: one row, one cell pair,
#    the Chamfer, the height and every name as they were, the chosen corner
#    following its vertex to the new plate, the exact volume and the flat.
refs_before = [r[0] for r in tables(one)["topology_refs"][1]]
rects = {"big": [-10.125, 7.5, 52.25, 20.5], "small": [30.0, -2.75, 9.5, 6.25],
         "moved": [X0 + 10.5, Y0 - 5.25, W, D]}
copies = {}
for name, rect in rects.items():
    out = root / f"{name}.fcad"
    geometry(["edit-sketch-copy", one, "--sketch", sketch["sketch_id"],
              "--expect-version", saved["content_version"], "--request", ask(saved, rect),
              "-o", out, "--json"], out)
    assert only_row(one, out, sketch["sketch_id"]) >= 2
    assert [r[0] for r in tables(out)["topology_refs"][1]] == refs_before, "every name keeps its UUID"
    after = inspect(out)
    assert after["features"][0]["distance_mm"] == H, "the height is kept"
    assert after["chamfers"][0]["feature_id"] == chamfer["feature_id"]
    assert after["chamfers"][0]["edge"] == chamfer["edge"], "the same two Lines meet"
    assert after["chamfers"][0]["distance_mm"] == d1, "the distance is kept, not reduced"
    assert after["sketches"][0]["chamfer_base"]["corner_mm"] == corner_of(rect)
    measured(out, rect, d1, H)
    copies[name] = out

# 4. The height and the Chamfer's own distance editors still work on the edited
#    plate, and the distance bound follows the NEW adjacent sides.
edited = inspect(copies["big"])
taller = root / "taller.fcad"
geometry(["edit-extrude", copies["big"], "--feature", edited["features"][0]["feature_id"],
          "--distance-mm", "9.5", "--expect-version", edited["content_version"], "-o", taller, "--json"], taller)
measured(taller, rects["big"], d1, 9.5)
assert edited["chamfers"][0]["distance_edit"]["max_distance_mm"] == min(rects["big"][2], rects["big"][3]) - 0.01
request.write_text(json.dumps({"request_version": 1, "distance_mm": rects["big"][3] - 0.01}))
far = root / "far.fcad"
geometry(["edit-chamfer-distance", copies["big"], "--feature", chamfer["feature_id"],
          "--expect-version", edited["content_version"], "--request", request, "-o", far, "--json"], far)
measured(far, rects["big"], rects["big"][3] - 0.01, H)

# 5. The bound is exact: the smallest side that still fits the saved distance
#    is accepted and the next float below it is refused, naming the Chamfer.
fits = lambda s: s - 0.01 >= d1
s = d1 + 0.01
while not fits(s):
    s = math.nextafter(s, math.inf)
while fits(math.nextafter(s, 0.0)):
    s = math.nextafter(s, 0.0)
exact_side = root / "exact.fcad"
geometry(["edit-sketch-copy", one, "--sketch", sketch["sketch_id"],
          "--expect-version", saved["content_version"], "--request", ask(saved, [0.0, 0.0, 40.0, s]),
          "-o", exact_side, "--json"], exact_side)
assert inspect(exact_side)["chamfers"][0]["distance_mm"] == d1
measured(exact_side, [0.0, 0.0, 40.0, s], d1, H)

# 6. Refusals write nothing: just below the bound, a plate turned half a turn
#    (the Chamfer would land on the opposite corner) and a mirrored plate.
never = root / "never.fcad"
for rect in ([0.0, 0.0, 40.0, math.nextafter(s, 0.0)], [X0 + W, Y0 + D, -W, -D], [X0 + W, Y0, -W, D]):
    p = resketch(one, rect, never, code=2)
    assert p["error"]["kind"] == "input" and not never.exists(), (rect, p)
    if rect[2] > 0:
        assert chamfer["feature_id"] in p["error"]["message"] and "does not fit" in p["error"]["message"], p
print("FCAD_29C_RECIPE_OK",
      f"d={d1} big={measured(copies['big'], rects['big'], d1, H):.6f}/"
      f"{(rects['big'][2] * rects['big'][3] - d1 * d1 / 2) * H:.6f}")
```
