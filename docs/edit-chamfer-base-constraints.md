# §29D — dimension the rectangle under a Chamfer, in a new copy

[Executed verification and limitations](edit-chamfer-base-constraints-verification.md).
[The Chamfer this keeps](rectangular-corner-chamfer.md); [its height](edit-chamfer-base-height.md)
and [its rectangle's coordinates](edit-chamfer-base-sketch.md).
[The Line constraints themselves](sketch-constraints-copy.md); [the same slice under a
Fillet](fillet-base-constraints.md).

A person opens a saved plate with the one §29A Chamfer and, in the existing
**Edit constraints** form, adds, replaces or removes the Line constraints the
constraint editor already knows (Horizontal/Vertical, length and Replace length,
one Fixed endpoint, equal length, Parallel, Perpendicular, with the Coincident
closure links) on the plate's base Sketch, then saves a new `.fcad`. An agent does
the same with `inspect --json` and `edit-sketch-constraints-copy`. PlaneGCS
decides the rectangle's size and position; the Chamfer stays on the **same
corner**, named by the same two Line UUIDs, with the same distance. No command,
request format, copy pipeline, solver, constraint family, capability, payload
version or schema is added. Milestone 5C is not complete.

## Contract recorded before implementation

### The supported source

The class of §29A–§29C, read by the one reader the distance, height and
coordinate edits use (`chamfer::saved_chamfer` over
`cut_edit::saved_history_under_chamfer`): one untransformed XY datum; one Sketch
of four Lines whose **stored** coordinates are an axis-aligned rectangle; one
forward literal Blind `Extrude`/`NewBody`; one terminal Chamfer on a corner of
that rectangle; one Body whose tip is the Chamfer; the Chamfer's seven owned
names. Additional base-owned references that resolve are allowed and are never
forbidden here (§29B review: **every saved reference must resolve before and
after** the edit; unchanged).

What is new: the base Sketch may carry constraints, and only the constraint
editor's managed Line family (its own `managed` check): the Coincident closure
links and at most one each of the existing H/V, length, Fixed endpoint, equal
length and Parallel/Perpendicular slots. Anything else on it is refused, naming
the guilty constraint. Unchanged and still refused: creating a Chamfer on a
constrained source (§29A creation requires an unconstrained profile), a second
Chamfer, a Fillet or Cut beside it, another plane, another corner, ThroughAll,
new constraint families and circles, in-place Save, live preview.

### Stored and solved geometry are separate facts

* **Structure** (document, kernel-free, every discovery, preparation and write):
  the frame above, the stored rectangle, the corner found in the stored Lines by
  its two UUIDs, the managed constraint family.
* **Solved geometry** (the evaluator, at every cold *and* cached rebuild): the
  Chamfer's policy is asked of the Lines its predecessor Extrude was built from —
  the presentation the rebuild already solved, once, for that Extrude. No second
  solve and no parsing of prose. For an unconstrained Sketch those are the stored
  Lines, so §29A–§29C behave exactly as before. The policy:
  * the same curve UUIDs, in stored order, all Lines;
  * an axis-aligned rectangle by the shared rectangle reader at its unchanged
    tolerance; nothing is snapped and no wider tolerance is introduced;
  * every Line keeps its side (same axis and direction as stored, the rule §29C
    uses), so the cut corner is the same corner of the part;
  * the saved joint is a corner of that solved rectangle, found by its two Line
    UUIDs, never by position;
  * `0.001 mm ≤ d ≤ min(adjacent solved sides) − 0.01 mm`, the one §29A
    expression, exact at the bound. Nothing is clamped; the distance, the height
    and the Chamfer row are never changed to fit.
* **Stored dimensions are not evidence.** For a constrained base, discovery and
  preparation never apply the upper bound to stored coordinates — neither to
  refuse a distance the solved plate allows nor to accept one it does not. The
  rebuild applies it to the solved sides.

### Error policy

A real solver conflict is the typed `constraint` refusal naming its constraint
UUIDs (unchanged). A consistent redundant constraint may publish successfully;
its UUID is reported in `solve.redundant_constraint_ids`, rather than turned into
a refusal. A solved plate that is not the rectangle, moves a
Line off its side, loses the corner or leaves no room for the saved distance is a
separate domain refusal naming the Chamfer UUID and the violated bound. Neither
publishes anything: the source, the destination and the draft are left as they were.

### Identity — the exact SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Sketch row's `schema_version`, `payload`, `payload_hash` |
| `capabilities` | the `sketch.constraints.v1` row, upserted with `required = 1` by the existing policy when the saved Sketch has constraints (already present when the source had them) |
| `meta` | `modified_at` |
| every other table and column, including `deps` and `topology_refs` | nothing |

Preserved: every object, curve, the Chamfer (UUID, `previous`, edge, joint,
distance), the Extrude (height), the Body tip, every dependency and topology
reference, the stored coordinates (the solver's starting approximation — solved
numbers are never written back). Constraint UUIDs change only by the existing
atomic remove-then-add; closure Coincidents are never removable and are added with
the first addition.

### The lifecycle after constraints

* `edit-chamfer-distance` accepts the constrained plate and keeps its constraints
  byte for byte. Preparation checks only that the distance is finite and at least
  0.001 mm; the true maximum is the rebuild's, on the solved predecessor.
* `edit-extrude` accepts it too and keeps the constraints; the Chamfer adds no
  bound on the height.
* The coordinate editor (§29C) refuses while a **user** constraint exists, naming
  the Chamfer and pointing at the constraint editor. Removing every user
  constraint leaves the closure Coincidents, and the closure-only coordinate edit
  works again. Nothing is silently dropped and no solved coordinate is baked in.
* The strict reference rule of the shared job covers all of it.

### Writer, version, references

`Document::write_sketch_constraints` re-derives inside its transaction: the Sketch
row is the one prepared; the frame, the managed family and the Chamfer context are
read again from the current document; the prepared constraint list must equal the
stored list without the removed UUIDs followed by the added ones; curves and plane
are unchanged. A forged or stale prepared value is refused and nothing is written.
The existing job keeps the content-version guard, `copy_access`, alias/no-clobber,
cancellation, strict resolved references after the cold rebuild, atomic
publication and exit 7. No new payload, capability or SQLite version is needed.

### Discovery (`inspect --json`), additive

* `sketches[].constraint_edit` of the base Sketch becomes `available` and gains
  `chamfer_base` (the same object as `features[].chamfer_base`) — `null`
  everywhere else.
* `features[].chamfer_base`, `sketches[].chamfer_base` and `chamfers[]` gain
  `profile_constrained`. When it is `true`, their `corner_mm` is the **stored**
  approximation, `chamfers[].distance_edit.max_distance_mm` is `null` (the bound is
  judged on the solved plate when a copy is published), and `sketches[].editable`
  is `false` for the coordinate editor with a refusal that names the Chamfer.
* Kernel-free, one pinned snapshot; every old field, type, operation name and exit
  code is unchanged; inspection proves no geometry.

### UI

The existing Edit constraints form and worker (Undo/Redo, Replace length,
Save/Cancel, refusal and stale-reply preservation). One context line names the
Chamfer, the two Lines of its corner and its distance, says the coordinates shown
are the stored ones, and says a new copy is published only if the solved plate is
still this rectangle with room for the distance. The coordinate form refuses a
constrained plate with that reason. The form holds no copy of the geometry rule.

### Compatibility

The saved Sketch uses the existing constraint payload (Sketch v2) and capability
`sketch.constraints.v1`. A build before this one refuses to rebuild the chamfered
plate once its base carries constraints (its Chamfer reader requires a free or
closure-only profile) — a typed refusal, never a part of the wrong shape; checked
with the real CLI of the base commit.

### Out of scope

Creating a Chamfer on a constrained source, a second Chamfer, a Fillet or Cut
beside it, an arbitrary quadrilateral, a rotated plane, new constraint families,
live preview, picking edges, in-place Save.

## Agent recipe

The whole agent route, with no prior knowledge of the document:

* create an asymmetric, translated plate with fractional sizes and chamfer one
  corner with `chamfer-edge-copy`;
* find the base Sketch's `constraint_edit` in `inspect --json`, available, with its
  `chamfer_base` context (stored corner, `profile_constrained: false`);
* dimension it completely with `edit-sketch-constraints-copy` — H/V on every Line,
  the first Line's start pinned elsewhere, a new width and depth — and check the SQL
  allowlist cell by cell, the stored coordinates, the names, the Chamfer row, the
  discovery (`profile_constrained: true`, `max_distance_mm: null`, the coordinate
  editor refusing with the Chamfer named) and the DOF;
* `validate`, cold `rebuild`, and read the STL independently: the solved plate's
  extents, the analytic volume, which corner is cut, the flat's normal and area;
* Replace the width (its constraint UUID out, a new length in);
* edit the height, then the Chamfer's distance (to one the stored rectangle would
  not allow and the solved one does), and check the constraints are kept byte for
  byte;
* remove the exact width UUID, then every user constraint (the closure links stay;
  the stored rectangle is then the plate again, and a distance it cannot hold is
  refused) and edit the coordinates again;
* check that a solved plate too narrow for the distance, a real solver conflict and
  a stale version are refused and write nothing.

Extract it from this file and run it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-chamfer-base-constraints.md").read_text(encoding="utf-8")
code = text.split("# FCAD_29D_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-29d-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-29d-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_29D_RECIPE_NO_KERNEL`; one with Open CASCADE and no PlaneGCS stops at the
first constraint copy and prints `FCAD_29D_RECIPE_NO_SOLVER`; a build with both
prints `FCAD_29D_RECIPE_OK` with the measured and exact volumes.

```python
# FCAD_29D_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-29d-"))
OP = "edit-sketch-constraints-copy"

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def geometry(args, out, marker="FCAD_29D_RECIPE_NO_KERNEL", missing="Open CASCADE"):
    """A step that needs the kernel (or the solver): a build without it refuses
    typed, and the recipe says which."""
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
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        if t == "capabilities":
            assert set(arows) <= set(brows), "a capability changed"
            assert all("sketch.constraints.v1" in repr(r) for r in set(brows) - set(arows))
            continue
        assert len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u != v:
                    assert (t == "objects" and c in ("schema_version", "payload", "payload_hash")
                            and x[ac.index("id")] == sid) or (t == "meta" and c == "modified_at"), \
                        f"{t}.{c} moved"
    assert "sketch.constraints.v1" in [r[0] for r in tables(copy)["capabilities"][1]]

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
CORNER = [X0 + W, Y0]
d1 = 2.375

def measured(copy, rect, corner, distance, height):
    """After reopening: valid, a cold rebuild resolves every name, the exact
    analytic volume of the SOLVED plate (w*d - dist*dist/2)*h, and the
    independently read mesh is closed at the chosen corner of that plate: the
    other three corners are whole, the cut's two new vertex columns are
    `distance` along each adjacent side, and the flat adds up to
    distance*sqrt(2)*h and faces out of the plate."""
    x0, y0, w, d = rect
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
    assert abs(min(p[0] for p in pts) - x0) < 1e-4 and abs(max(p[0] for p in pts) - (x0 + w)) < 1e-4
    assert abs(max(p[2] for p in pts) - height) < 1e-4
    out.unlink()
    return volume, exact

# 1. A plate, drawn offset and fractional, and the one Chamfer of the 29A recipe.
request = root / "plate.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H,
                               "points_mm": [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", request, "-o", plate, "--json"], plate)
catalog = inspect(plate)
candidate = next(c for c in catalog["bodies"][0]["chamfer_edge"]["target"]["candidates"]
                 if c["corner_mm"] == CORNER)
request.write_text(json.dumps({"request_version": 1, "distance_mm": d1, "edge": candidate["edge"]}))
chamfered = root / "chamfered.fcad"
geometry(["chamfer-edge-copy", plate, "--body", catalog["bodies"][0]["body_id"],
          "--expect-version", catalog["content_version"], "--request", request,
          "-o", chamfered, "--json"], chamfered)

# 2. Discovery: the base Sketch's constraint editor is available, with the
#    Chamfer as context and its corner named as stored.
catalog = inspect(chamfered)
(chamfer,) = catalog["chamfers"]
(sketch,) = catalog["sketches"]
editor = sketch["constraint_edit"]
assert editor["available"] is True, editor
context = editor["chamfer_base"]
assert context == catalog["features"][0]["chamfer_base"], context
assert context["chamfer_feature_id"] == chamfer["feature_id"] and context["edge"] == chamfer["edge"]
assert context["corner_mm"] == CORNER and context["distance_mm"] == d1, context
assert context["profile_constrained"] is False and chamfer["profile_constrained"] is False
lines = [c["curve_id"] for c in editor["curves"]]
starts = [c["start_mm"] for c in editor["curves"]]
across = [starts[i][1] == starts[(i + 1) % 4][1] for i in range(4)]
refs = tables(chamfered)["topology_refs"]

def ask(remove, add):
    path = root / "constraints.json"
    path.write_text(json.dumps({"request_version": 1, "remove": remove, "add": add}))
    return path

def constrain(source, version, remove, add, out, code=0):
    args = [OP, source, "--sketch", sketch["sketch_id"], "--expect-version", version,
            "--request", ask(remove, add), "-o", out, "--json"]
    if code:
        return run(args, code)["error"]
    return geometry(args, out, "FCAD_29D_RECIPE_NO_SOLVER", "constraint")["result"]

def dimensioned(at, width, depth):
    """H/V on every Line, the first Line's start pinned at `at`, the width and
    the depth: nothing left for the solver to choose."""
    add = [{"curve_id": lines[i], "rule": "horizontal" if across[i] else "vertical"}
           for i in range(4)]
    add.append({"curve_id": lines[0], "rule": "fixed", "at": "start", "x_mm": at[0], "y_mm": at[1]})
    add.append({"curve_id": lines[across.index(True)], "rule": "distance", "distance_mm": width})
    add.append({"curve_id": lines[across.index(False)], "rule": "distance", "distance_mm": depth})
    return add

def sketch_row(path):
    return next(r for r in tables(path)["objects"][1]
                if r[0] == bytes.fromhex(sketch["sketch_id"].replace("-", "")))

# 3. Dimension it: translated, both sides changed, fully constrained.
AT, WIDTH, DEPTH = [-9.25, -2.5], 41.125, 15.625
first = root / "dimensioned.fcad"
before = chamfered.read_bytes()
result = constrain(chamfered, catalog["content_version"], [], dimensioned(AT, WIDTH, DEPTH), first)
assert result["solve"]["degrees_of_freedom"] == 0, result
assert chamfered.read_bytes() == before
allowlist(chamfered, first, sketch["sketch_id"])
assert tables(first)["topology_refs"] == refs, "a name moved"
after = inspect(first)
assert [c["start_mm"] for c in after["sketches"][0]["constraint_edit"]["curves"]] == starts
(again,) = after["chamfers"]
assert again["feature_id"] == chamfer["feature_id"] and again["edge"] == chamfer["edge"], again
assert again["distance_mm"] == d1 and again["profile_constrained"] is True, again
assert again["distance_edit"]["max_distance_mm"] is None, again
assert after["sketches"][0]["editable"] is False
assert chamfer["feature_id"] in after["sketches"][0]["refusal"], after["sketches"][0]
rect = [AT[0], AT[1], WIDTH, DEPTH]
solved_corner = [AT[0] + WIDTH, AT[1]]
v1, e1 = measured(first, rect, solved_corner, d1, H)

# 4. Replace the width: its constraint UUID out, a new length in.
width_id = next(c["constraint_id"] for c in after["sketches"][0]["constraint_edit"]["constraints"]
                if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == WIDTH)
replaced = root / "replaced.fcad"
constrain(first, after["content_version"], [width_id],
          [{"curve_id": lines[across.index(True)], "rule": "distance", "distance_mm": 30.25}],
          replaced)
allowlist(first, replaced, sketch["sketch_id"])
rect2 = [AT[0], AT[1], 30.25, DEPTH]
corner2 = [AT[0] + 30.25, AT[1]]
v2, e2 = measured(replaced, rect2, corner2, d1, H)

# 5. The height, then the distance: 14.5 mm is beyond the stored rectangle's
#    bound (12.24 mm) and within the solved plate's (15.615 mm). The
#    constraints are kept byte for byte.
catalog2 = inspect(replaced)
taller = root / "taller.fcad"
run(["edit-extrude", replaced, "--feature", catalog2["features"][0]["feature_id"],
     "--distance-mm", 9.75, "--expect-version", catalog2["content_version"], "-o", taller, "--json"])
assert sketch_row(taller) == sketch_row(replaced), "the constraints are kept"
v3, e3 = measured(taller, rect2, corner2, d1, 9.75)
catalog3 = inspect(taller)
distance_req = root / "distance.json"
distance_req.write_text(json.dumps({"request_version": 1, "distance_mm": 14.5}))
wider = root / "wider.fcad"
run(["edit-chamfer-distance", taller, "--feature", chamfer["feature_id"], "--expect-version",
     catalog3["content_version"], "--request", distance_req, "-o", wider, "--json"])
assert sketch_row(wider) == sketch_row(taller), "the constraints are kept"
v4, e4 = measured(wider, rect2, corner2, 14.5, 9.75)

# 6. Remove the exact width UUID, then every user constraint: the closure links
#    stay, nothing was baked into the stored coordinates, and the coordinate
#    editor accepts the plate again.
catalog4 = inspect(wider)
width2 = next(c["constraint_id"] for c in catalog4["sketches"][0]["constraint_edit"]["constraints"]
              if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == 30.25)
fewer = root / "fewer.fcad"
constrain(wider, catalog4["content_version"], [width2], [], fewer)
catalog5 = inspect(fewer)
user = [c["constraint_id"] for c in catalog5["sketches"][0]["constraint_edit"]["constraints"]
        if c["rule"]["kind"] != "coincident"]
# With no user constraint the stored rectangle is the plate again, and a
# distance of 14.5 mm does not fit its 12.25 mm side: refused, nothing written.
never = root / "never.fcad"
error = constrain(fewer, catalog5["content_version"], user, [], never, 2)
assert error["kind"] == "input" and chamfer["feature_id"] in error["message"], error
assert "does not fit the solved plate" in error["message"] and not never.exists(), error
distance_req.write_text(json.dumps({"request_version": 1, "distance_mm": 3.0}))
lower = root / "lower.fcad"
run(["edit-chamfer-distance", fewer, "--feature", chamfer["feature_id"], "--expect-version",
     catalog5["content_version"], "--request", distance_req, "-o", lower, "--json"])
catalog5 = inspect(lower)
bare = root / "bare.fcad"
constrain(lower, catalog5["content_version"], user, [], bare)
catalog6 = inspect(bare)
left = catalog6["sketches"][0]["constraint_edit"]["constraints"]
assert len(left) == 4 and all(c["rule"]["kind"] == "coincident" for c in left), left
assert [c["start_mm"] for c in catalog6["sketches"][0]["constraint_edit"]["curves"]] == starts
assert catalog6["chamfers"][0]["profile_constrained"] is False
assert catalog6["sketches"][0]["editable"] is True
assert catalog6["chamfers"][0]["distance_edit"]["max_distance_mm"] is not None
rect3 = [-10.125, 7.5, 52.25, 20.5]
x0, y0, w3, d3 = rect3
new = [[x0 + w3, y0 + d3], [x0 + w3, y0], [x0, y0], [x0, y0 + d3]]
coords = [[x0, y0], [x0 + w3, y0], [x0 + w3, y0 + d3], [x0, y0 + d3]]
request.write_text(json.dumps({"request_version": 1, "vertices": [
    {"curve_id": c["curve_id"], "start_mm": coords[starts.index(c["start_mm"])]}
    for c in catalog6["sketches"][0]["constraint_edit"]["curves"]]}))
moved = root / "moved.fcad"
run(["edit-sketch-copy", bare, "--sketch", sketch["sketch_id"], "--expect-version",
     catalog6["content_version"], "--request", request, "-o", moved, "--json"])
v5, e5 = measured(moved, rect3, [x0 + w3, y0], 3.0, 9.75)

# 7. Refusals write nothing: a solved plate too narrow for the distance (the
#    stored one is long enough), a real solver conflict, a stale version.
names = sorted(p.name for p in root.iterdir())
error = constrain(chamfered, catalog["content_version"], [], dimensioned([0, 0], 30, 2.0), never, 2)
assert error["kind"] == "input" and chamfer["feature_id"] in error["message"], error
assert "does not fit the solved plate" in error["message"], error
conflict = dimensioned([0, 0], 30, 10) + [
    {"curve_id": lines[(across.index(True) + 2) % 4], "rule": "distance", "distance_mm": 20}]
error = constrain(chamfered, catalog["content_version"], [], conflict, never, 2)
assert error["kind"] == "constraint" and error["constraint_conflict"]["constraints"], error
error = constrain(chamfered, after["content_version"], [], dimensioned([0, 0], 30, 10), never, 2)
assert error["kind"] == "input", error
assert not never.exists()
assert sorted(p.name for p in root.iterdir()) == names
print("FCAD_29D_RECIPE_OK", f"dimensioned={v1:.6f}/{e1:.6f}", f"replaced={v2:.6f}/{e2:.6f}",
      f"taller={v3:.6f}/{e3:.6f}", f"distance14.5={v4:.6f}/{e4:.6f}", f"moved={v5:.6f}/{e5:.6f}",
      f"sketch={sketch['sketch_id']}")
```
