# §28K — edit the constraints of a plate rounded twice, in a new copy

[Executed verification and limitations](edit-two-fillet-base-constraints-verification.md).
[The same edit under one Fillet](fillet-base-constraints.md);
[two sequential Fillets](sequential-edge-fillets.md);
[their radii](edit-sequential-fillet-radii.md),
[height](edit-two-fillet-base-height.md) and
[Sketch coordinates](edit-two-fillet-base-sketch.md).

A person opens a plate that §28G rounded twice — Extrude → Fillet 1 →
Fillet 2 → Body tip — adds, replaces or removes Line constraints of its base
Sketch in the existing **Edit constraints** form, and saves a new `.fcad`. The
solved rectangle is the new plate; both roundings stay on the same named
corners with the same radii. An agent does the same with the existing
`edit-sketch-constraints-copy`. There is no new command, request format, copy
pipeline, solver or geometric route: this is §28E's edit on §28H–§28J's
reading of the history.

## Contract recorded before implementation

### The supported source

Exactly the §28G class, read again from the saved history on every
discovery, preparation and write by the reader the radius, height and
coordinate edits use for two Fillets (`fillet_radius::fillets_over_plate` →
`saved_sequential_fillet`, over `cut_edit::saved_history_under_fillets`): one
untransformed XY datum; one Sketch of four Lines whose **stored** Lines are an
axis-aligned rectangle, carrying no constraint or only the constraint
editor's managed Line family (§25E: H/V, Start/End length, one Fixed endpoint,
equal length, Parallel/Perpendicular, and the Coincident closure links); one
forward literal Blind `Extrude`/`NewBody`; **Fillet 1** with `previous` =
`edge.feature` = that Extrude; **Fillet 2** with `previous` = Fillet 1,
`edge.feature` = that Extrude, another corner, the Body's tip; no Cut and no
other object; exactly the plane, profile, two predecessor and one body-tip
dependencies; Fillet 1's seven §28A names and Fillet 2's eight §28G names, by
meaning. The selected Sketch must be that plate's base Sketch.

The add-Fillet editor keeps reading `fillet_over_plate` (exactly one Fillet)
and keeps refusing; so does any history outside the class above. Nothing is
admitted by an object count.

### Typed context

`ConstraintSketchChoice.fillet` stays the Fillet on the base (Fillet 1);
`second_fillet` carries Fillet 2 (`history_index` 2, `previous` = Fillet 1,
`base_feature` = the Extrude). Both are the `SavedFillet`s
`fillets_over_plate` returns, carried into `PreparedSketchConstraints` and
compared whole by the writer's re-derivation. Fillet 2 is never described as
a direct child of the base.

### What the solved plate must be

The stored coordinates stay the solver's **starting guess** and are never
replaced by solved ones. Their lengths and bounds prove nothing about the
part (for a Sketch that carries constraints, the reader asks only that each
radius is a finite number of at least the minimum), and the stored corners
are not judged. At every rebuild — the copy's strict cold rebuild before
publication, and every later one — the evaluator asks, of the **solved** Lines
of the same four UUIDs, what it already asks for two Fillets
(`evaluable_fillet`, unchanged): the same four Lines in stored order, an
axis-aligned rectangle, every Line on its saved side, each saved joint a
corner, each saved radius within ½ × the shorter solved side at its corner
and, on adjacent corners, the pair rule in history order (`check_pair`: the
Line they share keeps at least `MIN_RADIUS_MM` flat between the arcs;
opposite corners share no Line). Nothing is clamped, reselected or
approximated, no epsilon is added, and no second check is written. A plate the
solved Lines make too small for either radius, for the flat, or that loses a
side is `input`, naming the Fillet's joint UUIDs, and publishes nothing; a
solver conflict is its own `constraint` refusal with the real constraint
UUIDs. A redundant but consistent system may publish; the result reports the
real redundant UUIDs in `solve.redundant_constraint_ids`.

### SQL allowlist

Exactly §28E's, unchanged: `objects`, the base Sketch row's `schema_version`,
`payload`, `payload_hash`; `capabilities`, the `sketch.constraints.v1` row,
upserted with `required = 1` by the existing constraints contract when the
saved Sketch has constraints (already present when the source had them);
`meta.modified_at` only if the existing writer stamps it. Every other table
and column, including `deps` and `topology_refs`, nothing; row counts are
equal except for that one possible capability row. The curves, their order and
plane, both Fillet rows (UUIDs, `previous`, edges, radii), the Extrude and its
height, the Body tip, `document_id` and every ref are kept. Constraint UUIDs
follow §28E: every untouched constraint and every closure Coincident keeps its
UUID; only new rules and newly needed closure links mint one; removals
disappear. Payload/capability versions change only as the constraints
contract already says — by what the Sketch holds, not by the presence of two
Fillets.

### Writer, version, rebuild and cache

`Document::write_sketch_constraints` is unchanged in shape: inside its
transaction it re-derives the edit from the current document (frame read
again, both Fillets compared whole, stored list minus removals plus additions,
curves and plane unchanged, result inside the managed family). Version,
alias, no-clobber, cancellation, cleanup and strict references are the copy
job's. Every saved name must resolve after the cold rebuild, both cylinders
under their own names on their own solved axes. The base Extrude's key covers
the constraints; each Fillet is keyed by its predecessor, so a changed
constraint misses the plate and both Fillets and a repeat hits; the height
and each radius remain editable afterwards. Removing every user constraint
leaves the closure links, so the solved plate is the stored one and the §28J
Sketch-coordinate edit is offered again.

### Clients

* `inspect --json`: `constraint_edit.fillet_base` of the base Sketch gains the
  additive `second_fillet` (Fillet 2 with its `previous_feature_id` = Fillet 1,
  `history_index` 2, edge, `stored_corner_mm`, `radius_mm`); `null` with one
  Fillet. Other fields keep their shape and type; `constraint_edit.available`
  becomes `true` for the class above.
* `edit-sketch-constraints-copy`: unchanged request, result, envelope, exit
  codes and stub order of checks.
* UI: the existing Edit constraints form names both Fillets in history order
  and what the copy must be; Undo/Redo over the whole request, Save Cancel and
  a worker refusal keep the draft, a published copy opens asynchronously.

### Out of scope

A third Fillet, arbitrary edges, Cut with Fillet, Chamfer, new constraint
kinds, automatic radius fitting, retargeting a corner, saving solved
coordinates, in-place Save, live preview.

## Recipe: inspect -> dimension -> replace -> the bound -> remove -> coordinates again -> refusals

For a caller driving the CLI with JSON v1. Extract it from this file and run
it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-two-fillet-base-constraints.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28K_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28k-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28k-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_28K_RECIPE_NO_KERNEL`. A build with Open CASCADE and without the solver
stops at the first constraint step and prints `FCAD_28K_RECIPE_NO_SOLVER`. A
build with both prints `FCAD_28K_RECIPE_OK` with the measured mesh and exact
volumes at each step.

```python
# FCAD_28K_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28k-"))
OP = "edit-sketch-constraints-copy"

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def geometry(args, out):
    """A step that needs the kernel (or, for constraints, the solver): a build
    without it refuses typed."""
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 2 and not out.exists():
        error = json.loads(p.stdout)["error"]
        if error["kind"] == "unsupported" and "Open CASCADE" in error["message"]:
            print("FCAD_28K_RECIPE_NO_KERNEL", json.dumps(error))
            sys.exit(0)
        if error["kind"] == "unsupported" and "sketch solver" in error["message"]:
            print("FCAD_28K_RECIPE_NO_SOLVER", json.dumps(error))
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
    """Only the Sketch row's schema version, payload and hash, the stamp and the
    one sketch.constraints capability row may differ; every other cell stays."""
    rid = bytes.fromhex(row_id.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        if t == "capabilities":
            assert all(r in brows for r in arows), "a capability changed"
            assert all("sketch.constraints.v1" in repr(r) for r in brows if r not in arows)
            continue
        assert len(arows) == len(brows), t
        if t == "objects":
            key = ac.index("id")
            arows = sorted(arows, key=lambda r: r[key])
            brows = sorted(brows, key=lambda r: r[key])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u == v:
                    continue
                ok = (t == "objects" and c in ("schema_version", "payload", "payload_hash")
                      and x[ac.index("id")] == rid) or (t == "meta" and c == "modified_at")
                assert ok, f"{t}.{c} moved"

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
R1, R2 = 2.375, 3.0625

def at(rect, p):
    """The corner of `rect` (x0, y0, width, depth) that `p` is of the saved plate."""
    x0, y0, w, d = rect
    return [x0 if p[0] == X0 else x0 + w, y0 if p[1] == Y0 else y0 + d]

def measured(copy, rect, radii, height=H):
    """After reopening: valid, a cold rebuild resolves every name, and the
    independently read mesh is `rect` extruded `height` with exactly the two
    saved corners rounded, each by its own radius; the exact volume is the
    B-Rep's analytic one."""
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    n = len(tables(copy)["topology_refs"][1])
    text = run(["rebuild", copy, "--cold"])
    assert "tip Fillet" in text and f"{n} of {n} stored references resolved" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--linear-deflection", "0.01", "--json"])
    volume, points = stl(out)
    x0, y0, w, d = rect
    rounded = [(at(rect, c), r) for c, r in zip((FIRST, SECOND), radii)]
    exact = (w * d - (1 - math.pi / 4) * sum(r * r for _, r in rounded)) * height
    slack = sum(math.pi / 2 * r * 0.01 * height for _, r in rounded)
    assert exact - slack - 1e-3 <= volume <= exact + 1e-3, (volume, exact)
    for c in [at(rect, t) for t in TEMPLATE]:
        for z in (0.0, height):
            near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in points)
            assert near == all(c != cut for cut, _ in rounded), (c, z)
    for corner, r in rounded:
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
done1 = fillet(plate, FIRST, R1, root / "first.fcad")
done2 = fillet(root / "first.fcad", SECOND, R2, root / "twice.fcad")
twice = root / "twice.fcad"

# 2. Discovery: the constraint editor offers the base Sketch, naming both
#    Fillets in history order with their STORED corners; Fillet 2 rounds
#    Fillet 1's result, not the base.
catalog = inspect(twice)
edit = catalog["sketches"][0]["constraint_edit"]
SKETCH = catalog["sketches"][0]["sketch_id"]
assert edit["available"] is True and edit["refusal"] is None, edit
ctx = edit["fillet_base"]
assert ctx["fillet_feature_id"] == done1["feature_id"] and ctx["radius_mm"] == R1, ctx
assert ctx["stored_corner_mm"] == FIRST, ctx
two = ctx["second_fillet"]
assert two["fillet_feature_id"] == done2["feature_id"] and two["radius_mm"] == R2, two
assert two["previous_feature_id"] == done1["feature_id"] and two["history_index"] == 2, two
assert two["stored_corner_mm"] == SECOND, two
assert catalog["sketches"][0]["editable"] is True, "a free plate: coordinates too (§28J)"
refs = tables(twice)["topology_refs"]
LINES = [c["curve_id"] for c in edit["curves"]]
STORED = [c["start_mm"] for c in edit["curves"]]

def h_or_v(i):
    return {"curve_id": LINES[i], "rule": "horizontal" if i % 2 == 0 else "vertical"}

def length(i, mm):
    return {"curve_id": LINES[i], "rule": "distance", "distance_mm": mm}

def pin(x, y):
    return {"curve_id": LINES[0], "rule": "fixed", "at": "start", "x_mm": x, "y_mm": y}

def constrain(source, remove, add, name, code=0):
    """One constraint edit: the exact allowlist, every name kept, the stored
    Lines still the solver's starting guess."""
    catalog = inspect(source)
    request = root / "constraints.json"
    request.write_text(json.dumps({"request_version": 1, "remove": remove, "add": add}))
    out = root / name
    before = source.read_bytes()
    reply = geometry([OP, source, "--sketch", SKETCH, "--expect-version",
                      catalog["content_version"], "--request", request, "-o", out, "--json"], out) \
        if code == 0 else run([OP, source, "--sketch", SKETCH, "--expect-version",
                               catalog["content_version"], "--request", request, "-o", out,
                               "--json"], code)
    assert source.read_bytes() == before, "the source is untouched"
    if code:
        assert not out.exists(), "a refusal published something"
        return reply["error"], out
    allowlist(source, out, SKETCH)
    assert tables(out)["topology_refs"] == refs, "a name moved"
    after = inspect(out)["sketches"][0]["constraint_edit"]
    assert [c["start_mm"] for c in after["curves"]] == STORED, "the stored guess was replaced"
    fb = after["fillet_base"]
    assert fb["fillet_feature_id"] == done1["feature_id"], fb
    assert fb["second_fillet"]["fillet_feature_id"] == done2["feature_id"], fb
    assert fb["second_fillet"]["previous_feature_id"] == done1["feature_id"], fb
    return out, reply["result"]

# 3. Add a full dimensioning: the solved plate is the new plate.
rect_a = (0.5, 1.25, 30.75, 10.5)
dims = [h_or_v(i) for i in range(4)] + [pin(0.5, 1.25), length(0, 30.75), length(1, 10.5)]
dimensioned, result = constrain(twice, [], dims, "dimensioned.fcad")
assert result["solve"]["degrees_of_freedom"] == 0 and result["solve"]["redundant_constraint_ids"] == []
va, ea = measured(dimensioned, rect_a, (R1, R2))

# 4. Replace the width and the pin: both cylinders' axes move.
listed = inspect(dimensioned)["sketches"][0]["constraint_edit"]["constraints"]
width = next(c for c in listed if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == 30.75)
fixed = next(c for c in listed if c["rule"]["kind"] == "fixed")
rect_b = (-2.25, 3.5, 22.25, 10.5)
replaced, _ = constrain(dimensioned, [width["constraint_id"], fixed["constraint_id"]],
                        [length(0, 22.25), pin(-2.25, 3.5)], "replaced.fcad")
vb, eb = measured(replaced, rect_b, (R1, R2))

# 5. The depth at exactly 2 * R2 is accepted; 2 * R2 - 0.005 mm is refused
#    naming Fillet 2's own corner, and publishes nothing.
listed = inspect(replaced)["sketches"][0]["constraint_edit"]["constraints"]
depth = next(c for c in listed if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == 10.5)
narrow, _ = constrain(replaced, [depth["constraint_id"]], [length(1, 2 * R2)], "narrow.fcad")
rect_n = (-2.25, 3.5, 22.25, 2 * R2)
vn, en = measured(narrow, rect_n, (R1, R2))
names = sorted(p.name for p in root.iterdir())
listed = inspect(narrow)["sketches"][0]["constraint_edit"]["constraints"]
depth = next(c for c in listed if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == 2 * R2)
error, _ = constrain(narrow, [depth["constraint_id"]], [length(1, 2 * R2 - 0.005)], "never.fcad", 2)
assert error["kind"] == "input", error
assert all(u in error["message"] for u in two["edge"]["joint"]), error

# 6. Remove every user constraint: the four closure links remain, the plate is
#    the stored one again, and the Sketch's coordinates (§28J) are offered again.
listed = inspect(replaced)["sketches"][0]["constraint_edit"]["constraints"]
user = [c["constraint_id"] for c in listed if c["rule"]["kind"] != "coincident"]
closed, result = constrain(replaced, user, [], "closure-only.fcad")
vc, ec = measured(closed, (X0, Y0, W, D), (R1, R2))
after = inspect(closed)
left = after["sketches"][0]["constraint_edit"]["constraints"]
assert len(left) == 4 and all(c["rule"]["kind"] == "coincident" for c in left), left
assert after["sketches"][0]["editable"] is True, "§28J again"
vertices = [{"curve_id": v["curve_id"], "start_mm": at((1.5, -2.0, 30.75, 9.5), p)}
            for v, p in zip(after["sketches"][0]["vertices"], TEMPLATE)]
request = root / "redraw.json"
request.write_text(json.dumps({"request_version": 1, "vertices": vertices}))
moved = root / "moved.fcad"
run(["edit-sketch-copy", closed, "--sketch", SKETCH, "--expect-version",
     after["content_version"], "--request", request, "-o", moved, "--json"])
vm, em = measured(moved, (1.5, -2.0, 30.75, 9.5), (R1, R2))

# 7. More refusals publish nothing: a real solver conflict naming constraints the
#    document holds, a lone length that costs the rectangle, a stale version.
names = sorted(p.name for p in root.iterdir())
error, _ = constrain(dimensioned, [], [length(2, 20.0)], "never.fcad", 2)
assert error["kind"] == "constraint", error
held = {c["constraint_id"] for c in
        inspect(dimensioned)["sketches"][0]["constraint_edit"]["constraints"]}
named = [c["constraint_id"] for c in error["constraint_conflict"]["constraints"]]
assert named and any(c in held for c in named), error
error, _ = constrain(twice, [], [length(0, 30.0)], "never.fcad", 2)
assert error["kind"] == "unsupported" and "rectangle" in error["message"], error
request = root / "constraints.json"
stale = run([OP, twice, "--sketch", SKETCH, "--expect-version", inspect(dimensioned)["content_version"],
             "--request", request, "-o", root / "never.fcad", "--json"], 2)["error"]
assert stale["kind"] == "input", stale
assert not (root / "never.fcad").exists() and sorted(p.name for p in root.iterdir()) == names
print("FCAD_28K_RECIPE_OK", f"dimensioned={va:.6f}/{ea:.6f}", f"replaced={vb:.6f}/{eb:.6f}",
      f"narrow={vn:.6f}/{en:.6f}", f"closure={vc:.6f}/{ec:.6f}", f"moved={vm:.6f}/{em:.6f}",
      f"fillets={done1['feature_id']},{done2['feature_id']}")
```
