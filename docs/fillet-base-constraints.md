# §28E — dimension the rectangle under a Fillet, in a new copy

[Executed verification and limitations](fillet-base-constraints-verification.md).
[The Fillet this keeps](single-edge-fillet.md); [its radius](edit-fillet-radius.md),
[its plate's height](edit-fillet-base-height.md) and
[its rectangle's coordinates](edit-fillet-base-sketch.md).
[The Line constraints themselves](sketch-constraints-copy.md).

A person opens a saved rounded plate — the §28A Fillet on one vertical edge of
a rectangular plate — and, in the existing **Edit constraints** form, adds,
changes or removes the Line constraints the constraint editor already knows
(Horizontal/Vertical, length and Replace length, one Fixed endpoint, equal
length, Parallel, Perpendicular) on the plate's base Sketch, then saves a new
`.fcad`. An agent does the same with `inspect --json` and
`edit-sketch-constraints-copy`. The rounding stays on the same corner, named by
the same two Line UUIDs, with the same radius. The stored coordinates stay the
solver's starting approximation; the Body is built from the PlaneGCS solution.
No command, request format, copy pipeline, constraint family, capability,
payload version or schema is added.

## Contract recorded before implementation

### The supported source

The frame §28B–§28D read, by the same reader
(`fillet_radius::fillet_over_plate` over
`cut_edit::saved_history_under_fillet`): one untransformed XY datum; one Sketch
of four Lines whose **stored** coordinates are an axis-aligned rectangle; one
forward literal Blind `Extrude`/`NewBody`; one Fillet whose `previous` and
`edge.feature` are that Extrude and whose joint is a corner of the stored
rectangle; one Body whose tip is the Fillet; no Cut; exactly the plane,
profile, predecessor and body-tip dependencies; exactly the seven §28A names.

What is new: the base Sketch may carry constraints, and only the constraint
editor's own managed Line family (read by its own `managed` check): Coincident
closure links at the adjacent joints, and at most one each of the existing
H/V, length, Fixed endpoint, equal length and Parallel/Perpendicular slots.
Anything else on it is refused, naming the Fillet.

Unchanged: creating a Fillet (§28A) still requires an unconstrained profile;
a second Fillet, a Cut with a Fillet, Chamfer, another plane and any other
profile are still refused.

### Stored and solved geometry are separate facts

* **Structure** (document, kernel-free, every discovery, preparation and
  write): the frame above, the stored rectangle, the joint found in the stored
  Lines by its two UUIDs, the managed constraint family.
* **Solved geometry** (the evaluator, every cold *and* cached rebuild): the
  Fillet's policy is asked of the Lines its predecessor Extrude was actually
  built from — the presentation of the profile the rebuild already solved,
  once, for that Extrude. No second solve. For an unconstrained Sketch those are
  the stored Lines, so §28A–§28D behave exactly as before. The policy:
  * the same curve UUIDs, in stored order, all Lines, none construction;
  * an axis-aligned rectangle by the shared rectangle reader, at its unchanged
    tolerance (`Tolerance::DEFAULT_LINEAR`, 1e-7 mm); nothing is snapped and
    no wider tolerance is introduced;
  * every Line keeps its side — the same axis and direction as stored, by its
    dominant component, the rule §28D uses — so the rounded corner is the same
    corner of the part;
  * the saved joint is a corner of that solved rectangle, found by its two
    Line UUIDs (`corner_for`), never by position or nearest coordinates;
  * the saved radius fits: `r ≤ ½ · min(adjacent solved sides)` (§28A's
    unchanged policy, `check_radius`). Nothing is clamped and the radius,
    height and Fillet row are never changed to fit.
* **Stored dimensions are not evidence** that the radius fits a constrained
  part. For a constrained base, discovery and preparation do not apply the
  radius bound to stored Lengths in either direction; the rebuild applies it
  to the solved ones.

### Identity — the exact SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Sketch row's `schema_version`, `payload`, `payload_hash` |
| `capabilities` | the `sketch.constraints.v1` row, upserted with `required = 1` by the existing policy when the saved Sketch has constraints (already present when the source had them) |
| `meta` | `modified_at` only if the existing writer stamps it (measured and reported) |
| every other table and column, including `deps` and `topology_refs` | nothing |

Row counts are equal except for the one possible new `capabilities` row.
Preserved: every object, curve, Fillet, joint and topology-reference UUID; the
Lines' order, winding and sides; every dependency; the stored coordinates.
Constraint UUIDs change only by the existing atomic remove-then-add: the
removed UUIDs disappear, new ones are minted for additions and for missing
closure Coincidents.

### Removing constraints

The existing rule stays: closure Coincidents are never removable, and are added
with the first addition. Removing the last user constraint leaves the four
closure links, so the Sketch remains constrained (solved = stored, since the
stored loop already closes). The coordinate editor (§28D) then honestly
refuses, as it refuses every constrained profile since §25E; nothing is dropped
silently.

### Radius and height after constraints

`edit-fillet-radius` (§28B) and `edit-extrude` (§28C) accept the constrained
plate and keep its constraints byte for byte. The radius bound for a
constrained plate is checked by the copy's rebuild on the solved predecessor;
preparation only checks the radius is finite and at least the minimum. The
height needs no Fillet bound (§28C).

### Writer, version, references

`Document::write_sketch_constraints` re-derives inside its transaction instead
of trusting the prepared payload: the Sketch row is the one prepared; the frame
and managed family are read again from the current document (so the Fillet
frame too); the prepared constraint list must equal the stored list without
the removed UUIDs followed by the added constraints; curves and plane are
unchanged; the result is inside the managed family. The existing job keeps the
content-version guard (it covers the Fillet row and every name), `copy_access`,
alias/no-clobber, cancellation, strict resolved references after the cold
rebuild, SQLite close, atomic publication and exit 7. A solver conflict stays
the typed `constraint` refusal naming UUIDs; a solved plate outside the Fillet
policy is a typed domain refusal; the source, destination and draft are left
as they were.

### Discovery (`inspect --json`), additive

* `sketches[].constraint_edit` of the base Sketch becomes `available`, with
  its stored curves and constraints as for any managed profile, and gains
  `fillet_base`: `fillet_feature_id`, `body_id`, `edge`, `radius_mm` and
  `stored_corner_mm` — the corner in the **stored** coordinates, named as such.
  `null` everywhere else.
* `features[].fillet_base`, `sketches[].fillet_base` and `fillets[]` gain
  `profile_constrained`. When it is `true`, their `corner_mm` is the stored
  approximation, and `fillets[].radius_edit.max_radius_mm` is `null`: the
  bound is judged on the solved plate when a copy is published.
* Kernel-free, one pinned snapshot, old fields, types, operation names and exit
  codes unchanged; the request stays strict.

### UI

The existing Edit constraints form and worker: bounded Undo/Redo, Replace
length, Save/Cancel, refusal and stale-reply preservation. One context line
names the Fillet, the two Lines of its corner and its radius, says the
coordinates shown are the stored ones, and says the new copy is published only
if the solved plate is still this rectangle with room for the radius.

### Compatibility

No new capability, payload version or SQLite schema: the saved Sketch uses the
existing constraint payload (Sketch v2) and capability `sketch.constraints.v1`. A build before this one
refuses to rebuild the rounded plate once its base carries constraints (its
Fillet reader requires an unconstrained profile) — a typed refusal, never a
part of the wrong shape. Checked with the real CLI of the base commit.

### Out of scope

Creating a Fillet on a constrained source, a second Fillet, Cut with Fillet,
Chamfer, an arbitrary quadrilateral, a rotated plane, new constraint families,
live preview, picking edges, in-place Save. The Fillet/Chamfer milestone stays
open.

## Agent recipe

The whole agent route, with no prior knowledge of the document:

* create an asymmetric, translated plate with fractional sizes and round one
  corner with `fillet-edge-copy`;
* find the base Sketch's `constraint_edit` in `inspect --json`, available,
  with its `fillet_base` context and the stored Lines;
* dimension it completely with `edit-sketch-constraints-copy` — H/V on every
  Line, the first Line's start pinned elsewhere, a new width and depth — and
  check the SQL allowlist cell by cell, the stored coordinates, the names, the
  Fillet row and the DOF;
* `validate`, cold `rebuild`, and read the STL independently: extents of the
  solved plate, the analytic volume within the chord bound, which corner is
  rounded; check the FBX export;
* Replace the width (its constraint UUID out, a new length in);
* change the radius to one the stored rectangle would not allow and the solved
  one does, then the height, and check the constraints are kept;
* check that a solved plate too narrow for the radius, a real solver conflict
  and a stale version are refused and write nothing.

Extract it from this file and run it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/fillet-base-constraints.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28E_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28e-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28e-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_28E_RECIPE_NO_KERNEL`; one with Open CASCADE and no PlaneGCS stops at the
first constraint copy and prints `FCAD_28E_RECIPE_NO_SOLVER`; a build with both
prints `FCAD_28E_RECIPE_OK` with the measured and exact volumes.

```python
# FCAD_28E_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28e-"))
OP = "edit-sketch-constraints-copy"

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def geometry(args, out, marker="FCAD_28E_RECIPE_NO_KERNEL", missing="Open CASCADE"):
    """A step that needs the kernel (or the solver): a build without it
    refuses typed, and the recipe says which."""
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 2 and not out.exists():
        error = json.loads(p.stdout)["error"]
        if error["kind"] == "unsupported" and missing in error["message"]:
            print(marker, json.dumps(error))
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

def allowlist(source, copy, sketch):
    """Only the base Sketch row's schema_version/payload/payload_hash,
    meta.modified_at and one new `sketch.constraints.v1` capability row may
    differ; every other table keeps its rows and every other cell."""
    sid = bytes.fromhex(sketch.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    moved = set()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        if t == "capabilities":
            assert set(arows) <= set(brows), "a capability changed"
            new = set(brows) - set(arows)
            assert all("sketch.constraints.v1" in repr(r) for r in new), new
            continue
        assert len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u == v:
                    continue
                ok = (t == "objects" and c in ("schema_version", "payload", "payload_hash")
                      and x[ac.index("id")] == sid) or (t == "meta" and c == "modified_at")
                assert ok, f"{t}.{c} moved"
                moved.add((t, c))
    names = [r[0] for r in tables(copy)["capabilities"][1]]
    assert "sketch.constraints.v1" in names, names
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
CORNERS = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]
corner, r = [X0 + W, Y0], 2.375

def measured(copy, rect, at, radius, height):
    """A cold rebuild resolves every name; the independently read mesh is the
    solved plate `rect` of `height`, only the corner `at` rounded."""
    x0, y0, w, d = rect
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    text = run(["rebuild", copy, "--cold"])
    assert "tip Fillet" in text and "10 of 10 stored references resolved" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--linear-deflection", "0.01", "--json"])
    volume, points = stl(out)
    exact = (w * d - (1 - math.pi / 4) * radius * radius) * height
    assert exact - math.pi / 2 * radius * 0.01 * height - 1e-3 <= volume <= exact + 1e-3, (volume, exact)
    xs, ys, zs = ([p[k] for p in points] for k in range(3))
    assert abs(min(xs) - x0) < 1e-4 and abs(max(xs) - x0 - w) < 1e-4, (min(xs), max(xs))
    assert abs(min(ys) - y0) < 1e-4 and abs(max(ys) - y0 - d) < 1e-4, (min(ys), max(ys))
    assert abs(min(zs)) < 1e-4 and abs(max(zs) - height) < 1e-4, (min(zs), max(zs))
    for c in ([x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]):
        for z in (0.0, height):
            near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in points)
            assert near == (c != at), (c, z)
    fbx = run(["export-fbx", copy, "-o", copy.with_suffix(".fbx"), "--json"])["result"]
    assert fbx["complete"] is True, fbx
    return volume, exact

# 1. A plate, and one saved Fillet at its lower-right corner.
create = root / "create.json"
create.write_text(json.dumps({"request_version": 1, "points_mm": CORNERS, "height_mm": H}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", create, "-o", plate, "--json"], plate)
catalog = inspect(plate)
body = catalog["bodies"][0]
chosen = next(c for c in body["fillet_edge"]["target"]["candidates"] if c["corner_mm"] == corner)
request = root / "fillet.json"
request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": r}))
rounded = root / "rounded.fcad"
geometry(["fillet-edge-copy", plate, "--body", body["body_id"], "--expect-version",
          catalog["content_version"], "--request", request, "-o", rounded, "--json"], rounded)

# 2. Discovery: the base Sketch's constraint editor is available, with the
#    Fillet as context and the corner named as stored.
catalog = inspect(rounded)
(fillet,) = catalog["fillets"]
(sketch,) = catalog["sketches"]
editor = sketch["constraint_edit"]
assert editor["available"] is True, editor
context = editor["fillet_base"]
assert context["fillet_feature_id"] == fillet["feature_id"], context
assert context["edge"] == chosen["edge"] and context["radius_mm"] == r, context
assert context["stored_corner_mm"] == corner, context
assert fillet["profile_constrained"] is False, fillet
lines = [c["curve_id"] for c in editor["curves"]]
starts = [c["start_mm"] for c in editor["curves"]]
across = [starts[i][1] == starts[(i + 1) % 4][1] for i in range(4)]
refs = tables(rounded)["topology_refs"]

def ask(remove, add):
    path = root / "constraints.json"
    path.write_text(json.dumps({"request_version": 1, "remove": remove, "add": add}))
    return path

def constrain(source, version, remove, add, out, code=0):
    args = [OP, source, "--sketch", sketch["sketch_id"], "--expect-version", version,
            "--request", ask(remove, add), "-o", out, "--json"]
    if code:
        return run(args, code)["error"]
    return geometry(args, out, "FCAD_28E_RECIPE_NO_SOLVER", "constraint")["result"]

def dimensioned(at, width, depth):
    """H/V on every Line, the first Line's start pinned at `at`, the width
    and the depth: nothing left for the solver to choose."""
    add = [{"curve_id": lines[i], "rule": "horizontal" if across[i] else "vertical"}
           for i in range(4)]
    add.append({"curve_id": lines[0], "rule": "fixed", "at": "start", "x_mm": at[0], "y_mm": at[1]})
    add.append({"curve_id": lines[across.index(True)], "rule": "distance", "distance_mm": width})
    add.append({"curve_id": lines[across.index(False)], "rule": "distance", "distance_mm": depth})
    return add

# 3. Dimension it: translated, both sides changed, fully constrained.
AT, WIDTH, DEPTH = [-9.25, -2.5], 41.125, 15.625
first = root / "dimensioned.fcad"
before = rounded.read_bytes()
result = constrain(rounded, catalog["content_version"], [], dimensioned(AT, WIDTH, DEPTH), first)
assert result["solve"]["degrees_of_freedom"] == 0, result
assert rounded.read_bytes() == before
allowlist(rounded, first, sketch["sketch_id"])
assert tables(first)["topology_refs"] == refs, "a name moved"
after = inspect(first)
assert [c["start_mm"] for c in after["sketches"][0]["constraint_edit"]["curves"]] == starts
(again,) = after["fillets"]
assert again["feature_id"] == fillet["feature_id"] and again["edge"] == fillet["edge"], again
assert again["radius_mm"] == r and again["profile_constrained"] is True, again
assert again["radius_edit"]["max_radius_mm"] is None, again
rect = [AT[0], AT[1], WIDTH, DEPTH]
solved_corner = [AT[0] + WIDTH, AT[1]]
v1, e1 = measured(first, rect, solved_corner, r, H)

# 4. Replace the width: its constraint UUID out, a new length in.
width_id = next(c["constraint_id"] for c in after["sketches"][0]["constraint_edit"]["constraints"]
                if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == WIDTH)
replaced = root / "replaced.fcad"
constrain(first, after["content_version"], [width_id],
          [{"curve_id": lines[across.index(True)], "rule": "distance", "distance_mm": 30.25}],
          replaced)
allowlist(first, replaced, sketch["sketch_id"])
rect2 = [AT[0], AT[1], 30.25, DEPTH]
v2, e2 = measured(replaced, rect2, [AT[0] + 30.25, AT[1]], r, H)

# 5. The radius answers to the solved plate: 7 mm is beyond the stored
#    rectangle's bound (6.125 mm) and within the solved one's (7.8125 mm).
catalog2 = inspect(replaced)
radius_req = root / "radius.json"
radius_req.write_text(json.dumps({"request_version": 1, "radius_mm": 7.0}))
rounder = root / "rounder.fcad"
run(["edit-fillet-radius", replaced, "--feature", fillet["feature_id"], "--expect-version",
     catalog2["content_version"], "--request", radius_req, "-o", rounder, "--json"])
v3, e3 = measured(rounder, rect2, [AT[0] + 30.25, AT[1]], 7.0, H)
taller = root / "taller.fcad"
catalog3 = inspect(rounder)
run(["edit-extrude", rounder, "--feature", catalog3["features"][0]["feature_id"],
     "--distance-mm", 9.75, "--expect-version", catalog3["content_version"], "-o", taller, "--json"])
sketch_row = lambda p: next(r for r in tables(p)["objects"][1]
                            if r[0] == bytes.fromhex(sketch["sketch_id"].replace("-", "")))
assert sketch_row(taller) == sketch_row(rounder), "the constraints are kept"
v4, e4 = measured(taller, rect2, [AT[0] + 30.25, AT[1]], 7.0, 9.75)

# 6. Refusals write nothing.
names = sorted(p.name for p in root.iterdir())
never = root / "never.fcad"
error = constrain(rounded, catalog["content_version"], [], dimensioned([0, 0], 30, 4.5), never, 2)
assert error["kind"] == "input" and "too short" in error["message"], error
conflict = dimensioned([0, 0], 30, 10) + [
    {"curve_id": lines[(across.index(True) + 2) % 4], "rule": "distance", "distance_mm": 20}]
error = constrain(rounded, catalog["content_version"], [], conflict, never, 2)
assert error["kind"] == "constraint" and error["constraint_conflict"]["constraints"], error
error = constrain(rounded, after["content_version"], [], dimensioned([0, 0], 30, 10), never, 2)
assert error["kind"] == "input", error
assert not never.exists()
assert sorted(p.name for p in root.iterdir()) == names
print("FCAD_28E_RECIPE_OK", f"dimensioned={v1:.6f}/{e1:.6f}", f"replaced={v2:.6f}/{e2:.6f}",
      f"radius7={v3:.6f}/{e3:.6f}", f"taller={v4:.6f}/{e4:.6f}", f"sketch={sketch['sketch_id']}")
```
