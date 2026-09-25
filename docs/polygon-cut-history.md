# §26I — circular Cut history on a simple Line polygon

[Executed verification and limitations](polygon-cut-history-verification.md).

## Contract recorded before implementation

§26D–§26H edit a bounded history of 1–16 pairwise separated circular Cuts, but
only in an axis-aligned rectangular plate: the reader `rectangle()` demanded
four axis-parallel Lines, and every rule downstream measured a tool against
`[[min_x, min_y], [max_x, max_y]]`. §26I replaces that rectangle with the
part's real outer boundary. Nothing else about the class widens.

### Supported class

* One untransformed XY datum, one **unconstrained** base Sketch on it, one
  forward literal Blind `Extrude`/`NewBody`, its single `Body`, and 0–16
  circular Cut links on that datum along +Z (Blind or explicit ThroughAll),
  with exactly the plane/profile/predecessor/body-tip edges. Unchanged.
* The base Sketch is a **simple closed polygon of 3..256 Lines**, stored end to
  end in order, accepted by the one shared simplicity validator
  `PolygonExtrusion::new` (no repeated vertex, no collinear or backtracking
  vertex, no crossing or touching edges, nonzero area, |coordinate| and height
  ≤ 1 000 000 mm). Either winding. Sloped sides and concave vertices are
  supported. A rectangle is the four-Line case of this class and keeps every
  answer it had.
* Out of scope and refused with a reason: construction geometry, arcs,
  circles or other curved outer walls, profiles with holes, base constraints,
  arbitrary planes or face attachment, intersecting tools, new booleans,
  fillets, changes to the vertex count, live preview and in-place Save.

### One boundary, one owner

`ferritecad-document` reads the base Sketch once into a `CutBoundary`: the
ordered Line segments, each with its **saved curve UUID** and its stored
start/end, plus the winding the person drew. `CutHistory` carries it, and so
do `SavedCutTarget`, `SavedCircularCut`, `BaseHeightContext` and
`SketchCutHistory`. There is no second reader in the UI or in the CLI; both
present this value.

The axis-aligned bounding box is still available as `CutBoundary::bounds_mm()`
and is named *bounds* everywhere it appears. It is a range for a form to offer
numbers in, **never** evidence that a disk lies inside the part. Whether the
part is literally an axis-aligned rectangle is a separate, explicit question,
`CutBoundary::rectangle_mm()`, answered by exactly the test `rectangle()` used
to apply; it exists only so the pre-§26I JSON blocks can keep describing
rectangles and nothing else.

### The containment rule

A tool disk (centre `c`, radius `r`) is inside the part iff **both**:

1. `c` is inside the polygon (crossing test over the finite saved segments);
2. for **every** finite segment — which includes its two end vertices — the
   Euclidean distance from `c` to the closest point of that segment exceeds
   `r + WALL_CLEARANCE_MM` (`WALL_CLEARANCE_MM` is the kernel's own linear
   tolerance, as before).

(1) and (2) together imply the closed disk lies in the open interior: the disk
is connected, contains an interior point, and meets no segment. Not
allowed, and each is a refusal rather than an approximation: bounding-box-only
tests, centre-only tests, distances to infinite supporting lines (which would
let a disk poke through a reflex vertex of an L), and any epsilon shift of the
geometry. The rule is winding- and translation-invariant: it uses only
relative vectors, and orientation enters nowhere. Equality with the clearance
is refused (strictly more is required), as it always was.

A refusal names what failed: the centre outside the part, or the closest
approach and the saved curve UUID of the segment it is measured to; callers
that check a set of tools prefix the Cut UUID.

### Where the rule runs

The same function is called by: adding a Cut (catalogue draft check,
preparation, and the writer's re-derivation inside its transaction), editing
any Cut, editing the base height, editing the base Sketch's coordinates, and
reading a saved history (a saved tool that violates it makes the whole history
unsupported). A base edit checks **every** tool, including ones far from the
moved vertex, and the refusal names the violating Cut's UUID. Tools stay in
absolute XY: changing the base never moves them. The 16-tool limit, disk
separation, source/version/no-clobber/late checks, cancellation, cleanup and
atomicity are unchanged.

### Names

Side names were already per saved curve UUID (`CarriedSide`/`OriginSide`
with `profile_segment` = the Line's UUID); §26I only removes the reader that
refused more than four of them. No identity is derived from a segment's index,
and no UUID is invented. The strict floor-reference policy, reopen, cold
rebuild and cache behave as for a rectangle.

### Base Sketch edits

`Edit sketch` of the history's base keeps the count, order and UUIDs of the
Lines and the winding; only coordinates change, through the same
frame/catalogue/job. The standalone (no-history) editor keeps its own
restrictions. ThroughAll Cuts follow the base height; Blind depths stay
absolute.

### JSON

No global schema change; `schema_version` stays 1 and request v1/v2,
operations and exit codes are unchanged.

* For a rectangular part every existing block (`cut_edit`, `cut_edit_v2`,
  `circular_cut_edit`, `circular_cut_edit_v2`, `base_height_edit`,
  `base_height_edit_v2`, `cut_history`, `cut_history_v2`) is byte-for-byte
  what it was, including `extents_mm`.
* For any other polygon those blocks cannot state the part without lying — a
  bounding box is not a rectangle — so they report themselves unavailable in
  the ways they already could: `target`/`saved` `null` with a `refusal` naming
  the `_v3` block, and `base_height_edit`/`cut_history` `null`. An old consumer
  therefore never receives polygon data under a field that meant "rectangle".
* Additive `_v3` blocks (`cut_edit_v3`, `circular_cut_edit_v3`,
  `base_height_edit_v3`, `cut_history_v3`) mirror `_v2` (explicit `extent`,
  `request_versions`) and replace `extents_mm` with `bounds_mm` (AABB only)
  and a typed `boundary`:
  `{"kind":"line_polygon","orientation":"counter_clockwise"|"clockwise",
  "segments":[{"curve_id","start_mm","end_mm"},…]}`. They are present for
  rectangles too.

### UI

The existing `Cut circle into…`, `Edit cut…`, `Edit Sketch …` and `Edit
extrusion` forms are unchanged in shape. The Cut form states the part from
the catalogue's `CutBoundary`: a rectangle by its size (as before), any other
outline as "outline of N Lines, A mm², within bounds (…)–(…)", and says that
the tool must clear the real outline, not the box around it. Refusals are the
document's own messages. No geometry is recomputed in the UI.

## L-profile fixture for the bundled CLI

The GUI scenario below starts from this file. With `FERRITECAD` set to the
bundled CLI (`FerriteCAD.app/Contents/MacOS/ferritecad` on macOS):

```sh
cat > l-profile.json <<'JSON'
{"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}
JSON
"$FERRITECAD" create-sketch-extrude l-profile.json -o l-profile.fcad --json
```

## macOS GUI scenario (not run in the cloud)

1. Open `l-profile.fcad`. `Cut circle into…`: the form says "outline of 6
   Lines, 1600 mm²".
2. Centre (40, 30), r 3 → Apply is refused ("…centre (40, 30) mm is outside the
   part"), although (40, 30) is inside the 60 × 40 bounding box. Centre
   (17, 17), r 4.3 → refused (reflex vertex). (40, 17), r 4 → refused (concave
   edge).
3. Centre (10.125, 18.625), r 2.25, **Through all** → Apply; then (45, 10), r 5
   → Apply. Undo returns to the first; Redo to the second; Undo again.
4. `Save cut copy…` → Cancel in the dialog: nothing is written. Save again to a
   new path; Open it: one Cut, through.
5. `Edit cut…` on it: move to (10.5, 18.5) → publish; move into (40, 30) →
   refused.
6. `Edit Sketch …` of the base: move the vertex pair (20, 20)/(20, 40) to
   x = 12 → refused, naming the Cut (the far tool at x 10.125 + 2.25 is no
   longer clear); x = 21 → published, the tool stays at its absolute XY.
7. `Edit extrusion` of the base to 13 mm → published; the ThroughAll hole is
   still through.

## Executable agent recipe

Set `FERRITECAD` to a fresh native CLI; optionally `FCAD_UFBX_READER` to the
pinned `read_production` reader. The marked block builds the L with 1, 2 and
16 Cuts, reads every UUID, version and the boundary from `_v3` JSON only, and
chooses its centres with its **own** containment test over the reported
segments. It checks the notch/reflex/concave refusals, first/middle/last
edits with an SQL allowlist, base height and base coordinates (including the
far-tool refusal naming the right Cut), an independent STL parse (closed,
oriented, bores, caps only over the real outline, volume within the
tessellation bound of area·h − Σπr²·reach), and sloped triangles/quads in
both windings far from the origin. Old `extents_mm` blocks are checked to be
absent for every non-rectangle.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/polygon-cut-history.md").read_text()
code = text.split("# FCAD_26I_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-26i-recipe.py").write_text(code)
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-26i-recipe.py
```

```python
# FCAD_26I_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, tempfile, uuid
cli = os.environ["FERRITECAD"]
reader = os.environ.get("FCAD_UFBX_READER")
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-26i-"))
L = [[0, 0], [60, 0], [60, 20], [20, 20], [20, 40], [0, 40]]
H = 10.0
DEFLECTION, ANGULAR, CLEAR = 0.05, 0.1, 1e-7
THROUGH = {"kind": "through_all"}
def blind(d): return {"kind": "blind", "depth_mm": d}

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def catalog(path): return run(["inspect", path, "--json"])["result"]

def create(points, name):
    req, out = root / f"{name}.json", root / f"{name}.fcad"
    req.write_text(json.dumps({"request_version": 1, "points_mm": points, "height_mm": H}))
    run(["create-sketch-extrude", req, "-o", out, "--json"])
    return out

def boundary(c):
    """The part as `_v3` states it; never `extents_mm`."""
    base = [f["base_height_edit_v3"] for f in c["features"] if f["base_height_edit_v3"]]
    block = base[0] if base else c["bodies"][0]["cut_edit_v3"]["target"]
    return block["boundary"]

def segments(b): return [(s["start_mm"], s["end_mm"]) for s in b["segments"]]

def clear(b, center, r):
    """Own containment test: centre inside, every finite segment > r + CLEAR."""
    x, y = center
    inside, nearest = False, math.inf
    for (ax, ay), (bx, by) in segments(b):
        if (ay > y) != (by > y) and x < ax + (y - ay) * (bx - ax) / (by - ay):
            inside = not inside
        dx, dy = bx - ax, by - ay
        t = max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
        nearest = min(nearest, math.hypot(x - ax - t * dx, y - ay - t * dy))
    return inside and nearest - r > CLEAR

def area(points):
    return abs(sum(points[i][0] * points[(i + 1) % len(points)][1]
                   - points[(i + 1) % len(points)][0] * points[i][1]
                   for i in range(len(points)))) / 2

def add(source, dest, center, radius, extent, code=0):
    c = catalog(source)
    req = root / "add.json"
    req.write_text(json.dumps({"request_version": 2, "center_mm": center,
                               "radius_mm": radius, "extent": extent}))
    return run(["cut-circular-copy", source, "--body", c["bodies"][0]["body_id"],
                "--expect-version", c["content_version"], "--request", req,
                "-o", dest, "--json"], code)

def saved_cuts(path):
    c = catalog(path)
    saved = [f["circular_cut_edit_v3"]["saved"] for f in c["features"]
             if f["circular_cut_edit_v3"]["saved"]]
    return c, (saved[0]["tools"] if saved else []), {s["feature_id"]: s for s in saved}

def edit(source, dest, feature, center, radius, extent, code=0):
    c, _, saved = saved_cuts(source)
    req = root / "edit.json"
    req.write_text(json.dumps({"request_version": 2, "tool_curve_id": saved[feature]["tool_curve_id"],
                               "center_mm": center, "radius_mm": radius, "extent": extent}))
    return run(["edit-circular-cut", source, "--feature", feature, "--expect-version",
                c["content_version"], "--request", req, "-o", dest, "--json"], code)

def height(source, dest, h, code=0):
    c = catalog(source)
    base = [f for f in c["features"] if f["base_height_edit_v3"]][0]
    return run(["edit-extrude", source, "--feature", base["feature_id"], "--expect-version",
                c["content_version"], "--distance-mm", h, "-o", dest, "--json"], code)

def sketch(source, dest, points, code=0):
    c = catalog(source)
    s = [s for s in c["sketches"] if s["cut_history_v3"]][0]
    ids = [seg["curve_id"] for seg in s["cut_history_v3"]["boundary"]["segments"]]
    req = root / "sketch.json"
    req.write_text(json.dumps({"request_version": 1, "vertices": [
        {"curve_id": i, "start_mm": p} for i, p in zip(ids, points)]}))
    return run(["edit-sketch-copy", source, "--sketch", s["sketch_id"], "--expect-version",
                c["content_version"], "--request", req, "-o", dest, "--json"], code)

def sql(path):
    with sqlite3.connect(path) as con:
        out = {}
        for (name,) in con.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name"):
            q = '"' + name.replace('"', '""') + '"'
            try: cur = con.execute(f"SELECT rowid,* FROM {q} ORDER BY rowid")
            except sqlite3.OperationalError: cur = con.execute(f"SELECT * FROM {q} ORDER BY 1,2")
            out[name] = ([x[0] for x in cur.description], cur.fetchall())
        return out

def preserved(a_path, b_path, changed_ids):
    a, b = sql(a_path), sql(b_path)
    ids = [uuid.UUID(i).bytes for i in changed_ids]
    assert a.keys() == b.keys()
    for name, (cols, rows) in a.items():
        now = b[name][1]
        assert cols == b[name][0] and len(rows) == len(now), name
        for old, new in zip(rows, now):
            for i, (x, y) in enumerate(zip(old, new)):
                allowed = (name == "meta" and cols[i] == "modified_at") or (
                    name == "objects" and old[cols.index("id")] in ids
                    and cols[i] in ("payload", "payload_hash", "schema_version"))
                assert x == y or allowed, (name, cols[i])

def inside_polygon(points, x, y):
    inside = False
    for i in range(len(points)):
        (ax, ay), (bx, by) = points[i], points[(i + 1) % len(points)]
        if (ay > y) != (by > y) and x < ax + (y - ay) * (bx - ax) / (by - ay):
            inside = not inside
    return inside

def measure(path, points, tools, h):
    stl = path.with_suffix(".stl")
    rep = run(["export-stl", path, "-o", stl, "--linear-deflection", DEFLECTION,
               "--angular-deflection", ANGULAR, "--json"])
    raw = stl.read_bytes()
    n = struct.unpack_from("<I", raw, 80)[0]
    assert len(raw) == 84 + 50 * n and rep["result"]["triangles"] == n
    tris, six = [], 0.0
    for i in range(n):
        t = [list(struct.unpack_from("<fff", raw, 84 + 50 * i + 12 + 12 * k)) for k in range(3)]
        tris.append(t)
        a, b, c = t
        six += (a[0]*(b[1]*c[2]-b[2]*c[1]) + a[1]*(b[2]*c[0]-b[0]*c[2]) + a[2]*(b[0]*c[1]-b[1]*c[0]))
    q = lambda v: tuple(round(x, 4) for x in v)
    directed = {}
    for t in tris:
        p = [q(v) for v in t]
        for k in range(3):
            e = (p[k], p[(k + 1) % 3]); directed[e] = directed.get(e, 0) + 1
    assert all(v == 1 for v in directed.values()), "not one oriented surface"
    assert all((b, a) in directed for (a, b) in directed), "open mesh"
    for t in tris:  # caps lie over the real outline, never over a notch
        if all(abs(v[2] - t[0][2]) < 1e-4 for v in t) and (abs(t[0][2]) < 1e-4 or abs(t[0][2] - h) < 1e-4):
            assert inside_polygon(points, sum(v[0] for v in t) / 3, sum(v[1] for v in t) / 3)
    removed_exact = removed_inscribed = 0.0
    for (cx, cy), r, extent in tools:
        reach = h if extent["kind"] == "through_all" else extent["depth_mm"]
        wall = [t for t in tris if all(abs(math.hypot(v[0]-cx, v[1]-cy) - r) < DEFLECTION + 1e-4 for v in t)
                and max(v[2] for v in t) - min(v[2] for v in t) > 1e-4]
        assert len(wall) >= 12, "no bore wall"
        zs = sorted(v[2] for t in wall for v in t)
        assert abs(zs[0]) < 1e-4 and abs(zs[-1] - reach) < 1e-4, (zs[0], zs[-1], reach)
        removed_exact += math.pi * r * r * reach
        removed_inscribed += math.pi * (r - DEFLECTION) ** 2 * reach
    volume = six / 6.0
    full = area(points) * h
    assert full - removed_exact - 1e-3 <= volume <= full - removed_inscribed + 1e-3, (volume, full)

plate = create(L, "L")
b = boundary(catalog(plate))
assert len(b["segments"]) == 6 and abs(b["area_mm2"] - 1600) < 1e-9
assert b["orientation"] == "counter_clockwise"
# Inside the bounding box, outside the part; over the reflex vertex; across y=20.
assert not clear(b, [40, 30], 3) and not clear(b, [17, 17], 4.3) and not clear(b, [40, 17], 4)
assert clear(b, [17, 17], 4.2)

slots = [[26.125 + 7 * (k % 5), 5.625 + 8 * (k // 5)] for k in range(10)] + \
        [[6.125, 25.625], [14.125, 25.625], [6.125, 33.625], [14.125, 33.625],
         [10.125, 9.625], [10.125, 18.625]]
def tool_at(i):
    extent = THROUGH if i % 3 == 0 else (blind(H) if i % 3 == 1 else blind(3.5 + i % 7))
    return slots[(i * 11) % 16], 1.5 + i % 4 * 0.25, extent

for count in (1, 2, 16):
    src, tools = plate, []
    for i in range(count):
        center, radius, extent = tool_at(i)
        c = catalog(src)
        assert clear(boundary(c), center, radius)
        # The old rectangle blocks never describe an L.
        assert c["bodies"][0]["cut_edit"]["target"] is None
        assert c["bodies"][0]["cut_edit_v2"]["target"] is None
        dest = root / f"l{count}-{i + 1}.fcad"
        add(src, dest, center, radius, extent)
        tools.append((center, radius, extent)); src = dest
    c, ordered, saved = saved_cuts(src)
    assert all(f["base_height_edit"] is None and f["base_height_edit_v2"] is None for f in c["features"])
    assert all(s["cut_history"] is None and s["cut_history_v2"] is None for s in c["sketches"])
    measure(src, L, tools, H)
    if count < 16:
        for center, r in (([40, 30], 3), ([17, 17], 4.3), ([40, 17], 4)):
            never = root / "never.fcad"
            add(src, never, center, r, THROUGH, code=2)
            assert not never.exists()
    for index in sorted({0, count // 2, count - 1}):
        feature = ordered[index]["feature_id"]
        center, radius, extent = tools[index]
        moved = [center[0] - 0.25, center[1] + 0.125]
        dest = root / f"l{count}-edit-{index}.fcad"
        edit(src, dest, feature, moved, radius - 0.125, extent)
        preserved(src, dest, [feature, ordered[index]["tool_sketch_id"]])
        changed = list(tools); changed[index] = (moved, radius - 0.125, extent)
        measure(dest, L, changed, H)
    grown = root / f"l{count}-grown.fcad"
    height(src, grown, 13.0)
    measure(grown, L, tools, 13.0)
    wider = [[-2, -1.5], [61.25, 0], [61.25, 21], [20.5, 21], [20.5, 41], [0, 40]]
    moved = root / f"l{count}-moved.fcad"
    sketch(grown, moved, wider)
    measure(moved, wider, tools, 13.0)
    if count == 16:
        near = [list(p) for p in L]; near[1][0] = near[2][0] = 56
        err = sketch(src, root / "never.fcad", near, code=2)["error"]["message"]
        bad = [t["feature_id"] for t, (center, r, _) in zip(ordered, tools)
               if not clear({"segments": [{"start_mm": near[i], "end_mm": near[(i + 1) % 6]}
                                           for i in range(6)]}, center, r)]
        assert len(bad) == 1 and bad[0] in err, (bad, err)
    if reader and count == 2:
        fbx = moved.with_suffix(".fbx")
        run(["export-fbx", moved, "-o", fbx, "--json"])
        p = subprocess.run([reader, "--identity", str(fbx)], capture_output=True, text=True)
        assert p.returncode == 0 and "failures=0" in p.stdout, (p.stdout, p.stderr)
    print("FCAD_26I_COUNT_OK", count)

shift = [1000.25, -250.5]
for name, shape, inside in (("tri", [[0, 0], [50, 5], [10, 40]], [[20, 15], [15, 28]]),
                            ("quad", [[0, 0], [50, 10], [45, 45], [-5, 35]], [[15, 12], [30, 30]])):
    for reverse in (False, True):
        pts = [[x + shift[0], y + shift[1]] for x, y in shape]
        if reverse: pts.reverse()
        src = create(pts, f"{name}-{int(reverse)}")
        b = catalog(src)["bodies"][0]["cut_edit_v3"]["target"]["boundary"]
        assert b["orientation"] == ("clockwise" if reverse else "counter_clockwise")
        tools = []
        for k, (x, y) in enumerate(inside):
            center = [x + shift[0], y + shift[1]]
            assert clear(b, center, 3)
            dest = root / f"{name}-{int(reverse)}-{k}.fcad"
            add(src, dest, center, 3, THROUGH if k == 0 else blind(4.5))
            tools.append((center, 3, THROUGH if k == 0 else blind(4.5))); src = dest
        measure(src, pts, tools, H)
print("FCAD_26I_RECIPE_OK", root)
```
