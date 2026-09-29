# §28F — round a dimensioned plate, in a new copy

[Executed verification and limitations](fillet-constrained-plate-verification.md).
[The Fillet this creates](single-edge-fillet.md);
[the same Fillet dimensioned afterwards](fillet-base-constraints.md), whose
stored/solved policy this reuses; [its radius](edit-fillet-radius.md),
[its plate's height](edit-fillet-base-height.md).
[The Line constraints themselves](sketch-constraints-copy.md).

Until now a person had to round the plate first and dimension it afterwards
(§28A, then §28E). This slice allows the natural order: draw the rectangle,
give it H/V, a Fixed point and its sizes with the existing **Edit
constraints** form, then round one vertical edge with the existing **Fillet
edge of Body** form. An agent does the same with `edit-sketch-constraints-copy`
and then `fillet-edge-copy`. The Fillet is a real saved §28A Fillet over a
constrained base, the same stored shape §28E already writes and reads, so no
command, request, copy pipeline, solve, constraint family, capability, payload
version or schema is added.

## Contract recorded before implementation

### The supported source

The plate `fillet-edge-copy` already accepts (§28A, read by
`cut_edit::read_history` through `CutHistory::target_for_fillet`): one
untransformed XY datum; one Sketch of four Lines whose **stored** coordinates
are an axis-aligned rectangle; one forward literal Blind `Extrude`/`NewBody`;
one Body whose tip is that Extrude; no Cut; no Fillet; exactly the plane,
profile and body-tip dependencies.

What is new: the Sketch may carry constraints, and only the constraint
editor's own managed Line family, checked by the same
`sketch_constraints::managed_lines` §28E uses for the Sketch under a Fillet:
Coincident closure links at the adjacent joints and at most one each of the
existing H/V, length, Fixed endpoint, equal length and Parallel/Perpendicular
slots. Anything else is refused by name. The four stored Lines keep their
UUIDs and order; the stored rectangle is the solver's starting guess and still
has to be a rectangle, because it is where the candidate joints come from.

Unchanged: a Cut history stays unconstrained (the Cut editors' reader keeps
refusing a constrained plate); a second Fillet, Cut with Fillet, Chamfer,
another plane, a rotated plane and any other profile are still refused.

### One policy for stored structure and solved geometry

This is §28E's policy, used as it is:

* **Structure** (document, kernel-free: discovery, preparation, the writer's
  re-derivation): the source above, the stored rectangle, the joint found in
  the stored Lines by its two UUIDs, the managed constraint family, and the
  part of the radius policy that holds at every corner (finite, ≥
  `MIN_RADIUS_MM` = 0.01 mm).
* **Solved geometry** (`evaluable_fillet`, every cold *and* cached rebuild,
  unchanged): the Lines the predecessor Extrude was actually built from — the
  presentation of the profile the rebuild already solved, once. The same
  curve UUIDs in stored order; an axis-aligned rectangle by the shared reader
  at its unchanged 1e-7 mm tolerance, nothing snapped; every Line on its
  stored side; the saved joint a corner; `r ≤ MAX_RADIUS_FRACTION` (½) × the
  shorter solved side at that corner.

The copy job already rebuilds the written copy cold before publishing it
(`edit_object_copy` → `checked_rebuild`), so a Fillet the solved plate cannot
carry is refused there and nothing is published. The radius bound therefore
works in both directions: a radius beyond the stored guess's bound publishes
when the solved sides allow it, and a radius within the stored bound is
refused when the solved sides are too short. The stored coordinates are never
replaced by solved ones and no constraint is dropped to satisfy a reader.

For an unconstrained plate nothing changes: preparation still applies the
full §28A bound to the stored Lines, and no solver is involved.

### Identity — the exact SQL allowlist

Exactly §28A's:

* **objects:** one new Fillet row; the Body row's `payload` and `payload_hash`
  (the tip). The Sketch row — `schema_version` 2, `payload` with its
  constraints and their UUIDs, `payload_hash` — is byte-identical, as is every
  other row.
* **deps:** + `Fillet → base` (Predecessor), + `Body → Fillet` (BodyTip),
  − `Body → base` (BodyTip).
* **topology_refs:** the Fillet's seven new rows; every existing row
  byte-identical.
* **capabilities:** `feature.fillet.v1`, `topology.origin-face.v1` and
  `feature.predecessor.v1`, added only if missing; `sketch.constraints.v1`
  stays as it was.
* **meta.modified_at.**

Every other cell is byte-identical, measured after reopen.

### Writer, version, references

Unchanged pipeline: snapshot, version guard against the exact snapshot and
again before publication, read-only source, baseline rebuild, every baseline
and every minted name resolving after the write, cancellation, no-clobber
atomic publication, typed refusals, exit 7 on a lost report. The writer
re-derives the prepared Fillet inside its transaction (`fillet::rederive`,
which reruns preparation against the transaction's document), so a forged or
stale preparation is refused. The baseline rebuild of a constrained plate
needs the solver: a build without PlaneGCS refuses before writing anything,
while the unconstrained route still needs none.

### Order of operations

constraints → Fillet and Fillet → constraints (§28E) end in the same stored
model: the same Sketch payload (up to the constraint UUIDs each run mints),
the same Fillet payload, dependencies, names and capabilities (up to the
Fillet's and the names' minted UUIDs), and the same geometry. Radius (§28B),
height (§28C) and constraint (§28E) edits then work on the result as they do
on a §28E part; the coordinate editor keeps refusing a constrained Sketch.

### Discovery (`inspect --json`), additive

`bodies[].fillet_edge` keeps every field, type and meaning for an
unconstrained plate. For a constrained one:

* `available` is `true` when the structure is supported. As before it is
  structural only: it promises neither native libraries nor that the solved
  plate will carry the radius.
* `target.profile_constrained` (new, `bool`; `false` for every plate this
  reported before).
* each candidate names a structurally admissible joint: `edge` (the two Line
  UUIDs, the identity a request repeats) and a `label` that says the numbers
  are stored.
* `corner_mm`, `adjacent_lengths_mm` and `max_radius_mm` describe the part.
  They are unchanged numbers for an unconstrained plate and `null` for a
  constrained one, whose part only a solve knows. Inspect does no solve and no
  rebuild.
* `stored_corner_mm` and `stored_adjacent_lengths_mm` (new, always numbers):
  the stored drawing. For an unconstrained plate they equal `corner_mm` and
  `adjacent_lengths_mm`.
* `min_radius_mm` and `max_radius_fraction` keep their meaning: the policy
  judged on the solved plate when the copy is built.

`fillet-edge-copy` JSON result: `corner_mm` is the corner of the plate the
copy was built from (for an unconstrained plate the stored corner, as before).
New: `profile_constrained`, `stored_corner_mm`, and `adjacent_lengths_mm` of
the built plate.

### UI

The existing **Fillet edge of Body** form. For a constrained plate it says the
Sketch carries constraints, lists each corner by its two Lines with its
stored position and sides labelled as stored, states that the corner's
position and the `r ≤ ½ × shorter side` bound are the solved plate's and are
checked when the copy is saved, and applies only the value part of the radius
policy before Save. A refusal at Save arrives in the status line with the draft
kept, as for every copy job.

### Compatibility

No new stored semantics: the document is exactly what §28E writes (a Fillet
over a constrained plate), so the reader on `main` after #69 reads and
rebuilds it; that is measured with a real build of that commit, not inferred
from the schema version. That build's `fillet-edge-copy` still refuses a
constrained plate, typed.

### Out of scope

Creating or editing an arbitrary quadrilateral, a second Fillet, Cut with
Fillet, Chamfer, picking, live preview, new constraint families, another or a
rotated plane, in-place Save. Milestone 5C is not closed.

## Agent recipe

The whole agent route in the natural order, with no prior knowledge of the
document:

* create an asymmetric, translated plate with fractional sizes;
* dimension it completely with `edit-sketch-constraints-copy` — H/V on every
  Line, the first Line's start pinned elsewhere, a new width and depth;
* find `bodies[].fillet_edge` in `inspect --json`: available, marked
  `profile_constrained`, each candidate named by two Line UUIDs with its
  stored numbers and no solved ones;
* round one corner with `fillet-edge-copy` at a radius the stored rectangle
  could not carry and the solved one can; check the SQL allowlist cell by
  cell (the Sketch row byte for byte) and the reported solved corner;
* `validate`, cold `rebuild` after reopening, and read the STL independently:
  extents of the solved plate, the analytic volume within the chord bound,
  which corner is rounded; check the FBX export;
* Replace the width, then change the radius and the height, and check the
  constraints are kept;
* check that a solved plate too narrow for the radius and a stale version are
  refused and write nothing.

Extract it from this file and run it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/fillet-constrained-plate.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28F_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28f-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28f-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_28F_RECIPE_NO_KERNEL`; one with Open CASCADE and no PlaneGCS stops at
the first constraint copy and prints `FCAD_28F_RECIPE_NO_SOLVER`; a build with
both prints `FCAD_28F_RECIPE_OK` with the measured and exact volumes.

```python
# FCAD_28F_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28f-"))
CONSTRAIN = "edit-sketch-constraints-copy"

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def geometry(args, out, marker="FCAD_28F_RECIPE_NO_KERNEL", missing="Open CASCADE"):
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

FILLET_CAPABILITIES = {"feature.fillet.v1", "topology.origin-face.v1", "feature.predecessor.v1"}

def fillet_allowlist(source, copy, body):
    """§28A's allowlist: every source cell survives except the Body row's
    payload/payload_hash and its old tip edge; the copy adds one object, two
    edges, seven names, Fillet capabilities if missing, and stamps
    modified_at. The Sketch row, constraints and all, is the same bytes."""
    bid = bytes.fromhex(body.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        if t == "objects":
            k = ac.index("id")
            mine = {r[k]: r for r in brows}
            assert len(brows) == len(arows) + 1, "one new object"
            for row in arows:
                new = mine[row[k]]
                for c, u, v in zip(ac, row, new):
                    assert u == v or (row[k] == bid and c in ("payload", "payload_hash")), \
                        f"objects.{c} moved"
        elif t in ("deps", "topology_refs"):
            kept = [r for r in arows if r in brows]
            lost = [r for r in arows if r not in brows]
            assert all(t == "deps" and bid in r for r in lost), (t, lost)
            assert len(brows) - len(kept) == (2 if t == "deps" else 7), t
        elif t == "capabilities":
            assert set(arows) <= set(brows), "a capability changed"
            new = {r[0] for r in set(brows) - set(arows)}
            assert new <= FILLET_CAPABILITIES, new
        elif t == "meta":
            for x, y in zip(arows, brows):
                for c, u, v in zip(ac, x, y):
                    assert u == v or c == "modified_at", f"meta.{c} moved"
        else:
            assert arows == brows, t

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
STORED = [X0 + W, Y0]

def measured(copy, rect, at, radius, height):
    """After reopening: a cold rebuild resolves every name; the independently
    read mesh is the solved plate `rect` of `height`, only the corner `at`
    rounded."""
    x0, y0, w, d = rect
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    n = len(tables(copy)["topology_refs"][1])
    text = run(["rebuild", copy, "--cold"])
    assert "tip Fillet" in text and f"{n} of {n} stored references resolved" in text, text
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

# 1. A plate.
create = root / "create.json"
create.write_text(json.dumps({"request_version": 1, "points_mm": CORNERS, "height_mm": H}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", create, "-o", plate, "--json"], plate)
catalog = inspect(plate)
(sketch,) = catalog["sketches"]
editor = sketch["constraint_edit"]
assert editor["available"] is True, editor
assert catalog["bodies"][0]["fillet_edge"]["target"]["profile_constrained"] is False
lines = [c["curve_id"] for c in editor["curves"]]
starts = [c["start_mm"] for c in editor["curves"]]
across = [starts[i][1] == starts[(i + 1) % 4][1] for i in range(4)]

def ask(remove, add):
    path = root / "constraints.json"
    path.write_text(json.dumps({"request_version": 1, "remove": remove, "add": add}))
    return path

def constrain(source, version, remove, add, out):
    args = [CONSTRAIN, source, "--sketch", sketch["sketch_id"], "--expect-version", version,
            "--request", ask(remove, add), "-o", out, "--json"]
    return geometry(args, out, "FCAD_28F_RECIPE_NO_SOLVER", "constraint")["result"]

def dimensioned(at, width, depth):
    """H/V on every Line, the first Line's start pinned at `at`, the width
    and the depth: nothing left for the solver to choose."""
    add = [{"curve_id": lines[i], "rule": "horizontal" if across[i] else "vertical"}
           for i in range(4)]
    add.append({"curve_id": lines[0], "rule": "fixed", "at": "start", "x_mm": at[0], "y_mm": at[1]})
    add.append({"curve_id": lines[across.index(True)], "rule": "distance", "distance_mm": width})
    add.append({"curve_id": lines[across.index(False)], "rule": "distance", "distance_mm": depth})
    return add

# 2. Dimension it first: translated, wider and deeper than drawn.
AT, WIDTH, DEPTH = [-6.0, 2.0], 41.25, 15.5
dim = root / "dimensioned.fcad"
result = constrain(plate, catalog["content_version"], [], dimensioned(AT, WIDTH, DEPTH), dim)
assert result["solve"]["degrees_of_freedom"] == 0, result

# 3. Discovery: a structural target whose numbers are the stored drawing's.
catalog = inspect(dim)
body = catalog["bodies"][0]
edge = body["fillet_edge"]
assert edge["available"] is True, edge
target = edge["target"]
assert target["profile_constrained"] is True, target
for c in target["candidates"]:
    assert c["corner_mm"] is None and c["max_radius_mm"] is None, c
chosen = next(c for c in target["candidates"] if c["stored_corner_mm"] == STORED)
assert min(chosen["stored_adjacent_lengths_mm"]) / 2 == 6.125, chosen

# 4. Round it at 7 mm: beyond the stored rectangle's 6.125 mm, within the
#    solved plate's 7.75 mm.
r = 7.0
request = root / "fillet.json"
request.write_text(json.dumps({"request_version": 1, "edge": {
    "feature_id": chosen["edge"]["feature_id"],
    "joint": chosen["edge"]["joint"][::-1]}, "radius_mm": r}))
rounded = root / "rounded.fcad"
before = dim.read_bytes()
done = run(["fillet-edge-copy", dim, "--body", body["body_id"], "--expect-version",
            catalog["content_version"], "--request", request, "-o", rounded, "--json"])["result"]
assert dim.read_bytes() == before
solved = [AT[0] + WIDTH, AT[1]]
assert done["profile_constrained"] is True and done["stored_corner_mm"] == STORED, done
assert all(abs(u - v) < 1e-9 for u, v in zip(done["corner_mm"], solved)), done
assert done["edge"] == chosen["edge"] and done["radius_mm"] == r, done
fillet_allowlist(dim, rounded, body["body_id"])
after = inspect(rounded)
(fillet,) = after["fillets"]
assert fillet["profile_constrained"] is True and fillet["radius_edit"]["max_radius_mm"] is None
assert after["sketches"][0]["editable"] is False, "the coordinate editor refuses"
rect = [AT[0], AT[1], WIDTH, DEPTH]
v1, e1 = measured(rounded, rect, solved, r, H)

# 5. Replace the width, then the radius, then the height.
constraints = after["sketches"][0]["constraint_edit"]["constraints"]
width_id = next(c["constraint_id"] for c in constraints
                if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == WIDTH)
replaced = root / "replaced.fcad"
constrain(rounded, after["content_version"], [width_id],
          [{"curve_id": lines[across.index(True)], "rule": "distance", "distance_mm": 30.25}],
          replaced)
rect2 = [AT[0], AT[1], 30.25, DEPTH]
v2, e2 = measured(replaced, rect2, [AT[0] + 30.25, AT[1]], r, H)
catalog2 = inspect(replaced)
radius_req = root / "radius.json"
radius_req.write_text(json.dumps({"request_version": 1, "radius_mm": 5.0}))
smaller = root / "smaller.fcad"
run(["edit-fillet-radius", replaced, "--feature", fillet["feature_id"], "--expect-version",
     catalog2["content_version"], "--request", radius_req, "-o", smaller, "--json"])
v3, e3 = measured(smaller, rect2, [AT[0] + 30.25, AT[1]], 5.0, H)
catalog3 = inspect(smaller)
taller = root / "taller.fcad"
run(["edit-extrude", smaller, "--feature", catalog3["features"][0]["feature_id"],
     "--distance-mm", 9.75, "--expect-version", catalog3["content_version"], "-o", taller, "--json"])
sid = bytes.fromhex(sketch["sketch_id"].replace("-", ""))
row = lambda p: next(x for x in tables(p)["objects"][1] if sid in x)
assert row(taller) == row(smaller) == row(replaced), "the constraints are kept"
v4, e4 = measured(taller, rect2, [AT[0] + 30.25, AT[1]], 5.0, 9.75)

# 6. Refusals write nothing: a solved plate too narrow for 5 mm (8.5 mm deep,
#    where the stored drawing would allow 6.125 mm), and a stale version.
narrow = root / "narrow.fcad"
constrain(plate, inspect(plate)["content_version"], [], dimensioned([1.0, 1.0], 30.0, 8.5), narrow)
cat4 = inspect(narrow)
chosen4 = next(c for c in cat4["bodies"][0]["fillet_edge"]["target"]["candidates"]
               if c["stored_corner_mm"] == STORED)
request.write_text(json.dumps({"request_version": 1, "edge": chosen4["edge"], "radius_mm": 5.0}))
names = sorted(p.name for p in root.iterdir())
never = root / "never.fcad"
error = run(["fillet-edge-copy", narrow, "--body", cat4["bodies"][0]["body_id"], "--expect-version",
             cat4["content_version"], "--request", request, "-o", never, "--json"], 2)["error"]
assert error["kind"] == "input" and "too short" in error["message"], error
error = run(["fillet-edge-copy", narrow, "--body", cat4["bodies"][0]["body_id"], "--expect-version",
             catalog["content_version"], "--request", request, "-o", never, "--json"], 2)["error"]
assert error["kind"] == "input", error
assert not never.exists()
assert sorted(p.name for p in root.iterdir()) == names
print("FCAD_28F_RECIPE_OK", f"rounded={v1:.6f}/{e1:.6f}", f"replaced={v2:.6f}/{e2:.6f}",
      f"radius5={v3:.6f}/{e3:.6f}", f"taller={v4:.6f}/{e4:.6f}", f"sketch={sketch['sketch_id']}")
```
