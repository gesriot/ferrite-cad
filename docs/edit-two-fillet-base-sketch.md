# §28J — move or resize the rectangle under two Fillets, in a new copy

[Executed verification and limitations](edit-two-fillet-base-sketch-verification.md).
[The same edit under one Fillet](edit-fillet-base-sketch.md);
[two sequential Fillets](sequential-edge-fillets.md);
[their radii](edit-sequential-fillet-radii.md);
[the plate's height under them](edit-two-fillet-base-height.md).

A person opens a plate that §28G rounded twice — Extrude → Fillet 1 →
Fillet 2 → Body tip — moves or resizes its base rectangle in the existing
**Edit Sketch** form (typed coordinates, dragging vertices, Undo/Redo,
Restore saved vertices), and saves a new `.fcad`. Both roundings stay on the
same named corners with the same radii, at the rectangle's new corners. An
agent does the same with the existing `edit-sketch-copy`. There is no new
command, request format, copy pipeline, solver or geometric route: this is
§28D's edit on §28H's reading of the history.

## Contract recorded before implementation

### The supported source

Exactly the §28G class, read again from the saved history on every
discovery, preparation and write by the reader the radius and height edits
use for two Fillets (`fillet_radius::fillets_over_plate` →
`saved_sequential_fillet`, over `cut_edit::saved_history_under_fillets`):
one untransformed XY datum; one Sketch of four Lines forming an axis-aligned
rectangle; one forward literal Blind `Extrude`/`NewBody`; **Fillet 1** with
`previous` = `edge.feature` = that Extrude; **Fillet 2** with `previous` =
Fillet 1, `edge.feature` = that Extrude, another corner, the Body's tip; no
Cut and no other object; exactly the plane, profile, two predecessor and one
body-tip dependencies; Fillet 1's seven §28A names and Fillet 2's eight §28G
names, by meaning.

The Sketch is free: no constraint, or only Coincident closure links at
adjacent Line joints (what the constraint editor leaves when its last user
constraint is removed, §28E). Such links reference curve UUIDs and endpoint
selectors, not coordinates, and the coordinate editor keeps the loop exactly
closed, so they stay satisfied and are kept byte for byte. A Sketch with any
other constraint (length, H/V, equal, Parallel/Perpendicular, Fixed) keeps
refusing coordinate editing; such constraints are edited with the constraint
editor ([§28K](edit-two-fillet-base-constraints.md)).
The selected Sketch must be the plate's base Sketch.

The one-Fillet frame (§28D) is unchanged, including its refusal of every
constrained Sketch. The add-Fillet editor keeps reading `fillet_over_plate`,
which admits exactly one Fillet, and keeps refusing a two-Fillet history (the
constraint editor reads it since §28K).

### Typed context

`SketchChoice.fillet` stays the Fillet on the base (Fillet 1); a new
`SketchChoice.second_fillet` carries Fillet 2 (`history_index` 2, `previous`
= Fillet 1). Both are the `SavedFillet`s `fillets_over_plate` returns; the
history is not re-checked by a copy of that reader. Both joints name corners
swept by the base Extrude.

### What a candidate rectangle must be

The existing coordinate rules (every saved Line UUID once in saved order,
exactly closed, same winding, the Extrude's polygon policy), then, jointly,
on the candidate Lines:

* an axis-aligned rectangle of the same four Lines (`rectangle_corners`);
* **every Line keeps its side** (`keeps_every_side`, as §28D);
* Fillet 1's saved joint is still a corner, found by its two Line UUIDs,
  and its saved radius fits there (`SavedFillet::corner_on`);
* the same for Fillet 2, its own joint and radius;
* the **pair rule** in history order on the candidate corners
  (`check_pair(c1, r1, c2, r2)`): on adjacent corners the shared Line keeps
  at least `MIN_RADIUS_MM` flat between the arcs; opposite corners share no
  Line. The same predicate the rebuild and §28H apply; its bound for Fillet 1
  (`pair_bound_of_first`) keeps the bounded bisection.

Nothing is clamped: no radius is changed, no corner reselected, no epsilon
added. A Line that changes side, a rectangle too small for either radius or
too short for the pair are `input`; a non-rectangle is `unsupported`.

### SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Sketch row's `payload` and `payload_hash` |
| `meta` | `modified_at` (the coordinate writer does not stamp it; measured) |
| every other table and column | nothing |

Only the Lines' `geometry` inside the payload changes: curve UUIDs, their
order, `construction`, the plane and every constraint stay. Preserved byte for
byte: the document id, the base Extrude (height), both Fillet rows (UUIDs,
`previous`, edges, radii), the Body tip, every dependency, topology reference
and capability. Nothing is minted; no capability, schema or payload version.
Stored coordinates are never replaced by solver output.

### Writer, version, rebuild and cache

`Document::write_sketch_geometry` unchanged: inside its transaction it
re-derives the edit from the current document through the same choice (both
Fillets read again) and compares the whole prepared payload; version, alias,
no-clobber, cancellation, cleanup and strict references are the copy job's.
Every name must resolve after the cold rebuild, both cylinders under their
own names. The base Extrude's key covers the coordinates and each Fillet is
keyed by its predecessor, so a moved rectangle misses all three; a repeat
hits.

### Clients

* `inspect --json`: the base Sketch's `sketches[]` row becomes editable with
  `fillet_base` (Fillet 1) and its additive `second_fillet` (Fillet 2),
  previously `null` on `sketches[]` rows. Other fields keep shape and type.
* `edit-sketch-copy`: unchanged request, result, envelope, exit codes.
* UI: the existing Edit Sketch form, with one context line naming both
  Fillets in history order and the smallest sides they allow.

### Out of scope

A third Fillet (§28L now supports a third and a fourth), arbitrary edges, Cut with Fillet, Chamfer, reselecting a corner, in-place
Save.

## Recipe: inspect -> exact UUIDs -> move and resize -> the bound -> height and radius after -> refusals

For a caller driving the CLI with JSON v1. Extract it from this file and run
it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-two-fillet-base-sketch.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28J_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28j-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28j-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_28J_RECIPE_NO_KERNEL`. Any build with Open CASCADE — the plate is
unconstrained, so no solver is asked — prints `FCAD_28J_RECIPE_OK` with the
measured mesh and exact volumes at each step.

```python
# FCAD_28J_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28j-"))
OP = "edit-sketch-copy"

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def geometry(args, out):
    """A step that needs the kernel: a build without it refuses typed."""
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 2 and not out.exists():
        error = json.loads(p.stdout)["error"]
        if error["kind"] == "unsupported" and "Open CASCADE" in error["message"]:
            print("FCAD_28J_RECIPE_NO_KERNEL", json.dumps(error))
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

def allowlist(source, copy, row_id):
    """Only the selected row's payload/payload_hash and meta.modified_at may
    differ; every table keeps its rows and every other cell."""
    rid = bytes.fromhex(row_id.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    moved = set()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc and len(arows) == len(brows), t
        if t == "objects":
            key = ac.index("id")
            arows = sorted(arows, key=lambda r: r[key])
            brows = sorted(brows, key=lambda r: r[key])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u == v:
                    continue
                ok = (t == "objects" and c in ("payload", "payload_hash") and x[ac.index("id")] == rid) \
                    or (t == "meta" and c == "modified_at")
                assert ok, f"{t}.{c} moved"
                moved.add((t, c))
    return moved

def stl(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    six, points = 0.0, []
    for i in range(count):
        a, b, c = (struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3))
        six += (a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
        points += [a, b, c]
    return six / 6, points

X0, Y0, W, D, H = -4.5, 3.25, 37.5, 12.25, 6.75
TEMPLATE = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]
FIRST, SECOND = [X0 + W, Y0], [X0 + W, Y0 + D]   # adjacent: they share the Line x = 33

def at(rect, p):
    """The corner of `rect` (x0, y0, width, depth) that `p` is of the saved plate."""
    x0, y0, w, d = rect
    return [x0 if p[0] == X0 else x0 + w, y0 if p[1] == Y0 else y0 + d]

def measured(copy, rect, rounded, height):
    """After reopening: valid, a cold rebuild resolves every name, and the
    independently read mesh is `rect` extruded `height` with exactly `rounded`
    (corner, radius) pairs rounded; the exact volume is the B-Rep's analytic one."""
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    n = len(tables(copy)["topology_refs"][1])
    text = run(["rebuild", copy, "--cold"])
    assert "tip Fillet" in text and f"{n} of {n} stored references resolved" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--linear-deflection", "0.01", "--json"])
    volume, points = stl(out)
    x0, y0, w, d = rect
    corners = [at(rect, c) for c in TEMPLATE]
    exact = (w * d - (1 - math.pi / 4) * sum(r * r for _, r in rounded)) * height
    slack = sum(math.pi / 2 * r * 0.01 * height for _, r in rounded)
    assert exact - slack - 1e-3 <= volume <= exact + 1e-3, (volume, exact)
    for c in corners:
        for z in (0.0, height):
            near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in points)
            assert near == all(c != cut for cut, _ in rounded), (c, z)
    for corner, r in rounded:
        # Inside the corner's r x r square the mesh is the arc: every vertex
        # lies at r from its centre, so each radius is measured on its own.
        sx = 1 if corner[0] == x0 else -1
        sy = 1 if corner[1] == y0 else -1
        centre = (corner[0] + sx * r, corner[1] + sy * r)
        arc = [p for p in points if 1e-6 < sx * (p[0] - corner[0]) < r - 1e-6
               and 1e-6 < sy * (p[1] - corner[1]) < r - 1e-6]
        assert len(arc) >= 6, (corner, r, len(arc))
        assert all(abs(math.hypot(p[0] - centre[0], p[1] - centre[1]) - r) < 1e-3 for p in arc), (corner, r)
    fbx = run(["export-fbx", copy, "-o", copy.with_suffix(".fbx"), "--json"])["result"]
    assert fbx["complete"] is True, fbx
    return volume, exact

def fillet(source, corner, radius, out):
    catalog = inspect(source)
    body = catalog["bodies"][0]
    chosen = next(c for c in body["fillet_edge"]["target"]["candidates"]
                  if c["stored_corner_mm"] == corner)
    request = root / "fillet.json"
    request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"],
                                   "radius_mm": radius}))
    return geometry(["fillet-edge-copy", source, "--body", body["body_id"], "--expect-version",
                     catalog["content_version"], "--request", request, "-o", out, "--json"],
                    out)["result"]

# 1. A plate rounded twice (§28G): Extrude -> Fillet 1 -> Fillet 2 -> Body tip.
create = root / "create.json"
create.write_text(json.dumps({"request_version": 1, "points_mm": TEMPLATE, "height_mm": H}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", create, "-o", plate, "--json"], plate)
R1, R2 = 2.375, 3.0625
done1 = fillet(plate, FIRST, R1, root / "first.fcad")
done2 = fillet(root / "first.fcad", SECOND, R2, root / "twice.fcad")
twice = root / "twice.fcad"

# 2. Discovery: the base Sketch is editable and names both Fillets in history
#    order; Fillet 2 rounds Fillet 1's result, not the base.
catalog = inspect(twice)
row = catalog["sketches"][0]
SKETCH = row["sketch_id"]
assert row["editable"] is True and row["refusal"] is None, row
ctx = row["fillet_base"]
assert ctx["fillet_feature_id"] == done1["feature_id"] and ctx["radius_mm"] == R1, ctx
two = ctx["second_fillet"]
assert two["fillet_feature_id"] == done2["feature_id"] and two["radius_mm"] == R2, two
assert two["previous_feature_id"] == done1["feature_id"] and two["history_index"] == 2, two
assert row["constraint_edit"]["available"] is True, "§28K: the constraint editor reads both Fillets"
refs = tables(twice)["topology_refs"]

def redraw(source, rect, name, code=0, starts=None):
    """One edit: every saved Line UUID, in saved order, at the new corners."""
    current = inspect(source)["sketches"][0]
    vertices = [{"curve_id": v["curve_id"], "start_mm": p}
                for v, p in zip(current["vertices"], starts or [at(rect, c) for c in TEMPLATE])]
    request = root / "redraw.json"
    request.write_text(json.dumps({"request_version": 1, "vertices": vertices}))
    out = root / name
    before = source.read_bytes()
    reply = run([OP, source, "--sketch", SKETCH, "--expect-version",
                 inspect(source)["content_version"], "--request", request, "-o", out, "--json"], code)
    assert source.read_bytes() == before, "the source is untouched"
    if code:
        return reply["error"], out
    assert reply["result"]["sketch_id"] == SKETCH, reply
    allowlist(source, out, SKETCH)
    assert tables(out)["topology_refs"] == refs, "a name moved"
    return out, reply

# 3. Move and resize: the Fillets stay on the same named corners, which move.
rect_a = (1.5, -2.0, 30.75, 9.5)
moved, _ = redraw(twice, rect_a, "moved.fcad")
after = inspect(moved)["sketches"][0]["fillet_base"]
assert after["corner_mm"] == at(rect_a, FIRST) and after["second_fillet"]["corner_mm"] == at(rect_a, SECOND), after
assert after["fillet_feature_id"] == ctx["fillet_feature_id"] and after["edge"] == ctx["edge"]
va, ea = measured(moved, rect_a, [(at(rect_a, FIRST), R1), (at(rect_a, SECOND), R2)], H)

# 4. Shrink to the narrowest side both radii allow: 2 * R2 = 6.125 mm.
rect_b = (2.0, -3.5, 20.25, 2 * R2)
narrow, _ = redraw(moved, rect_b, "narrow.fcad")
vb, eb = measured(narrow, rect_b, [(at(rect_b, FIRST), R1), (at(rect_b, SECOND), R2)], H)

# 5. Height, then a radius, still edit on the moved plate.
(base,) = inspect(narrow)["features"]
version = inspect(narrow)["content_version"]
tall = root / "tall.fcad"
run(["edit-extrude", narrow, "--feature", base["feature_id"], "--distance-mm", 9.5,
     "--expect-version", version, "-o", tall, "--json"])
request = root / "radius.json"
request.write_text(json.dumps({"request_version": 1, "radius_mm": 1.5}))
small = root / "tall-f2.fcad"
run(["edit-fillet-radius", tall, "--feature", done2["feature_id"], "--expect-version",
     inspect(tall)["content_version"], "--request", request, "-o", small, "--json"])
vc, ec = measured(small, rect_b, [(at(rect_b, FIRST), R1), (at(rect_b, SECOND), 1.5)], 9.5)

# 6. Refusals write nothing: Lines that swap their sides, a plate under 2 * R2
#    deep, a shape that is no longer a rectangle and a stale version.
names = sorted(p.name for p in root.iterdir())
swapped = [at(rect_a, c) for c in TEMPLATE]
swapped = swapped[1:] + swapped[:1]
shallow = (2.0, -3.5, 20.25, 6.0)
skew = [at(rect_a, c) for c in TEMPLATE]
skew[3] = [skew[3][0] + 1.0, skew[3][1]]
for rect, starts, kind in ((rect_a, swapped, "input"), (shallow, None, "input"),
                           (rect_a, skew, "unsupported")):
    error, out = redraw(twice, rect, "never.fcad", 2, starts)
    assert error["kind"] == kind and not out.exists(), error
request = root / "redraw.json"
stale = run([OP, twice, "--sketch", SKETCH, "--expect-version", inspect(narrow)["content_version"],
             "--request", request, "-o", root / "never.fcad", "--json"], 2)["error"]
assert stale["kind"] == "input", stale
assert not (root / "never.fcad").exists() and sorted(p.name for p in root.iterdir()) == names
print("FCAD_28J_RECIPE_OK", f"moved={va:.6f}/{ea:.6f}", f"narrow={vb:.6f}/{eb:.6f}",
      f"tall_f2={vc:.6f}/{ec:.6f}", f"fillets={done1['feature_id']},{done2['feature_id']}")
```
