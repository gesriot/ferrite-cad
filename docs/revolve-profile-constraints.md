# §27G — dimensional constraints on a saved Revolve profile with a bore

[Executed verification and limitations](revolve-profile-constraints-verification.md).

A person or an agent opens a saved Revolve whose profile has a bore — a full
turn (§27A) or a sector (§27D) — and uses the existing **Edit constraints**
editor or the existing `edit-sketch-constraints-copy` command. The Lines of
the profile can gain or lose:

* H/V and a Line length (Distance);
* one Fixed endpoint;
* EqualLength, Parallel and Perpendicular.

The result is a new `.fcad` whose Body is the Revolve of the **solved**
profile. It keeps the stated angle, axis and direction of the turn, every
object and Line UUID, every face name, and on a sector both `RevolveCap`
names. A later copy changes or removes exactly those constraints by UUID.
The source is never written.

This widens the class the existing constraint editor accepts. There is no new
command, request field, rule family, solver, copier or special Revolve editor.

## Contract recorded before implementation

### Which documents are editable

These are the documents the §27B/§27F profile edit already accepts, through
the same shared `revolve_document` frame:

* **Document.** Four root objects: the untransformed XY datum, the Sketch,
  one Revolve of that Sketch, and its Body.
* **Revolve.** It turns about the sketch Y axis, is NewBody, and its extent
  is a full turn (payload v1) or partial (payload v3).
* **Dependencies.** Exactly Sketch→plane, Revolve→Sketch (Profile) and
  Body→Revolve (BodyTip).
* **Profile.** 3–256 non-construction Lines in an exactly closed sequence,
  whose payload round-trips losslessly. Its **stored** coordinates satisfy
  the same `stated_revolution` policy as an unconstrained profile. Any stored
  constraints are only the families this editor already manages, and are
  checked by the same `managed` rule an extruded polygon uses.

This slice accepts **radial-clear** profiles only: the Revolve states no
`axis_segment`. A profile closed on the axis, whether full turn (v2) or
sector (v4), is refused explicitly with the reason. The slice does not snap
anything to the axis and adds no seam points.

Also refused, each with its own reason: Circles and Arcs, booleans, other
axes and extents, multibody and other document shapes. A Sketch that a
Revolve turns is judged only by the Revolve frame: the Extrude frame is never
tried as a fallback, and the reverse is never tried either.

### What a request may say

It is the existing `edit-sketch-constraints-copy` request v1, unchanged:

* the same seven Line forms: H/V, `distance`, `fixed`, `equal_length`,
  `parallel`, `perpendicular`;
* the same slots and limits, and the same atomic remove-then-add;
* the same explicit closure Coincidents, added once on the first addition;
* the same exact-UUID removal;
* the same strict decode of the original bytes, which refuses duplicate
  keys, escaped duplicate keys, arrays where objects belong and unknown
  fields.

Circle forms are refused on a Line profile, exactly as today. Replacing a
length is the existing atomic remove + add: the operation mints the new UUID
once, and the UI's **Replace length** is the existing adapter for it.

### What the solved profile must be

The stored coordinates are **not** rewritten: they stay the solver's starting
geometry. There is one solve per Sketch per evaluation, on the existing
generic Sketch path. The Revolve request is built from that solved profile,
so a cold rebuild and a cached rebuild use the same solved geometry.
`stated_revolution` runs on the solved Lines in the evaluator, as it already
does. The copy job checks the same policy once more against the solved
presentation, through the owning feature's profile policy
(`SketchProfileUse`), before publication.

Publication is refused, and the source and destination are left untouched,
when the solved profile:

* crosses or touches the axis, or comes within the clearance of it;
* reaches the axis and so becomes axis-closed, changing the allowed class;
* self-intersects;
* degenerates to zero area or a collapsed Line;
* loses a joint.

A structurally valid set the solver cannot satisfy stays a typed
`constraint_conflict` with the actual constraint UUIDs. Redundancy is what
the solver reports in `redundant_constraint_ids`.

### What stays the same, and what may change

Every SQL cell is compared against this allowlist.

**May change:**

* In the selected Sketch row: `payload`, `payload_hash` and `schema_version`.
  The Sketch payload becomes v2 once it carries a constraint.
* In `capabilities`: the `sketch.constraints.v1` row may be added with
  `required=1`, or an optional one promoted to required with its rowid kept.

**Stays the same:**

* The Revolve row, including extent, angle, axis and `axis_segment`.
* The Body, the plane, the dependencies and the topology refs: every
  `RevolveFace` and both `RevolveCap` names, under the same UUIDs.
* Every other row and table, and `meta`. The writer does not stamp
  `modified_at`.

After a successful edit, every reference that resolved before must resolve
on the rebuilt Body.

### Removing the last user constraint

Removing the last user constraint follows the existing policy. The
Coincident closure links **remain**, so the Sketch is still constrained and
still requires `sketch.constraints.v1`. The stored coordinates are unchanged
and the model still evaluates through the solver.

Consequences:

* **Coordinate edits.** `edit-sketch-copy` (`sketches[].editable`) keeps
  refusing a constrained profile ("unconstrained Line segments"). That is the
  shared, deliberate policy of every coordinate editor. It is not relaxed
  here, and closure is never silently dropped to make the profile look
  unconstrained.
* **Without a solver.** A build without PlaneGCS reads, inspects and
  validates the document. Rebuilding, exporting or editing the Revolve then
  fails with the typed solver-unavailable error. A constraint copy fails the
  same way before publication, leaving nothing behind.

### The sector angle edit

A constrained sector keeps its constraints through `edit-revolve-angle`.
The angle edit writes only the Revolve row, so the Sketch row stays
byte-identical, constraints included. Its cold rebuilds solve the same
Sketch exactly once each.

The only change is in Revolve discovery. `revolves[].profile` no longer
refuses a profile merely because it carries constraints. Its stored Lines
are checked by the same `stated_revolution` policy, and the solved profile
is checked by the evaluator on every rebuild. `revolves[].profile.segments`
remain the **stored** Lines, as documented; a client that wants the solved
drawing rebuilds or exports.

### Discovery (JSON v1, additive)

All existing field names and types are kept. `sketches[].constraint_edit`
gains `profile_feature`. It is `null` exactly when `curves` is. Otherwise it
names the feature that turns the profile into a solid, with the kinds
`sketches[].profile_feature` already uses:

* `blind_extrude` for an extruded polygon, with its `height_mm`;
* `full_turn_revolve` or `partial_turn_revolve` for the class added here,
  with the axis, the extent, the saved `angle_deg` for a sector and the axis
  clearance.

No height is invented for a Revolve. `sketches[].profile_feature` keeps its
meaning: the owner of a **coordinate**-editable profile. It is therefore
`null` for a constrained one.

### Compatibility

There is no new capability, storage migration, dependency or ABI.

A constrained Revolve profile requires `sketch.constraints.v1`, which every
build since §25E knows. The evaluator is not changed by this slice. A
prior-main reader (5caaa1c) therefore solves the same stored constraints on
the same generic path and turns the same solved profile. Its editors refuse
the class honestly:

* its constraint editor knows only the Extrude frame;
* its coordinate editor needs an unconstrained profile;
* its angle editor refuses a profile that carries constraints.

The verification records what was actually executed with that reader.

### UI

Headless tests run the real egui widgets and the real worker. The existing
**Edit constraints** window gains one line naming the owning Revolve: full
turn or its saved angle, both kept. It uses the same draft, the bounded
128-step Undo/Redo, Replace length, Save/Cancel and the worker.

* **Solver.** There is no solver on redraw.
* **After publication.** The ordinary async Open shows the solved Body.
* **Save Cancel, refusal and a stale reply.** Each keeps the draft and its
  history.

### Request shape (§27G hardening of this operation)

The request is decoded once, strictly, from its original bytes: duplicate
keys — escaped spellings included — and unknown fields are refused at every
level, as before. New in this slice: a request that is a JSON array, or an
addition written as an array led by its rule, is refused with
`invalid constraint request JSON: the request and each addition must be
objects` (input, exit 2). The serde derives would otherwise read both in
field order. Nothing else about the parser changes, and no other command's
parser is touched.

## Agent recipe

An agent needs only `inspect --json`, explicit UUIDs from it and the
existing request. The recipe below is the public contract executed end to
end: discovery, a rigid dimensioning, a changed dimension through an exact
remove + add, an exact removal, a cold rebuild of the reopened copy, STL and
FBX exports measured independently of the product, and the typed errors.
Extract it from this file and run it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/revolve-profile-constraints.md").read_text(encoding="utf-8")
code = text.split("# FCAD_27G_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27g-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-27g-recipe.py
```

A build without PlaneGCS stops after discovery and the request refusals,
at the first publication. It prints `FCAD_27G_RECIPE_NO_SOLVER` with the
typed `unsupported` error it received. With `FCAD_EXPECT_SOLVER=1` that
outcome is a failure instead. A complete run prints `FCAD_27G_RECIPE_OK`
with the measured volumes and radii.

```python
# FCAD_27G_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27g-"))
OP = "edit-sketch-constraints-copy"

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def create(name, points, extent=None):
    request = root / f"{name}-create.json"
    body = {"request_version": 1, "points_mm": points, "axis": "sketch_y", "angle": "full_turn"}
    if extent is not None:
        body = {"request_version": 2, "points_mm": points, "axis": "sketch_y", "extent": extent}
    request.write_text(json.dumps(body))
    out = root / f"{name}.fcad"
    run(["create-sketch-revolve", request, "-o", out, "--json"])
    return out

def constrain(source, body, out, code=0):
    """One request against the snapshot `inspect` reports right now."""
    catalog = inspect(source)
    request = root / f"{out.stem}.json"
    request.write_text(body if isinstance(body, str) else json.dumps(body))
    return run([OP, source, "--sketch", catalog["sketches"][0]["sketch_id"],
                "--expect-version", catalog["content_version"], "--request", request,
                "-o", out, "--json"], code)

def lines(catalog):
    return catalog["sketches"][0]["constraint_edit"]["curves"]

def rule(catalog, i, kind, **extra):
    return {"curve_id": lines(catalog)[i]["curve_id"], "rule": kind, **extra}

def stored_constraint(catalog, kind, i):
    curve = lines(catalog)[i]["curve_id"]
    for c in catalog["sketches"][0]["constraint_edit"]["constraints"]:
        r = c["rule"]
        if r["kind"] == kind and curve in (r.get("a", {}).get("curve_id"),
                                           r.get("point", {}).get("curve_id")):
            return c["constraint_id"]
    raise KeyError((kind, i))

def cells(path):
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    out = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        names = [d[0] for d in cur.description]
        out[t] = sorted((sorted(zip(names, row)) for row in cur.fetchall()), key=repr)
    db.close()
    return out

def allowlist(source, copy, sketch):
    """Only the Sketch row's payload/payload_hash/schema_version and the
    sketch.constraints.v1 capability row may differ."""
    sid = bytes.fromhex(sketch.replace("-", ""))
    a, b = cells(source), cells(copy)
    assert a.keys() == b.keys()
    for t in a:
        def strip(rows):
            kept = []
            for r in rows:
                d = dict(r)
                if t == "objects" and d["id"] == sid:
                    for k in ("payload", "payload_hash", "schema_version"):
                        d.pop(k)
                if t == "capabilities" and d["name"] == "sketch.constraints.v1":
                    continue
                kept.append(sorted(d.items()))
            return sorted(kept, key=repr)
        assert strip(a[t]) == strip(b[t]), f"{t}: a cell outside the allowlist"
    required = [dict(r) for r in b["capabilities"] if dict(r)["name"] == "sketch.constraints.v1"]
    assert required and required[0]["required"] == 1, required

def stl_facts(path):
    """Independent of the product: signed volume, and the radius and height
    range of the vertices, which lie on the true surfaces."""
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    six, radii, heights = 0.0, [], []
    for i in range(count):
        a, b, c = (struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3))
        six += (a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
        for p in (a, b, c):
            radii.append(math.hypot(p[0], p[2]))
            heights.append(p[1])
    return six / 6, min(radii), max(radii), min(heights), max(heights)

def pappus(points):
    n = len(points)
    return 2 * math.pi * abs(sum(
        (points[(i + 1) % n][1] - points[i][1])
        * (points[i][0] ** 2 + points[i][0] * points[(i + 1) % n][0]
           + points[(i + 1) % n][0] ** 2) / 6 for i in range(n)))

def exported(copy, points, degrees=360.0):
    run(["validate", copy, "--json"])
    run(["rebuild", copy, "--cold"])
    stl = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", stl, "--linear-deflection", "0.05", "--json"])
    fbx = run(["export-fbx", copy, "-o", copy.with_suffix(".fbx"), "--json"])["result"]
    assert fbx["complete"] is True, fbx
    volume, r_min, r_max, y_min, y_max = stl_facts(stl)
    exact = pappus(points) * degrees / 360
    # A chord moves a face of revolution by at most the 0.05 mm deflection,
    # so each Line's swept band bounds what the mesh may gain or lose.
    n = len(points)
    band = sum(2 * math.pi * max(points[i][0], points[(i + 1) % n][0]) * 0.05
               * abs(points[(i + 1) % n][1] - points[i][1]) for i in range(n)) * degrees / 360
    assert volume > 0 and abs(exact - volume) <= band, (volume, exact, band)
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    assert abs(r_min - min(xs)) < 1e-4 and abs(r_max - max(xs)) < 1e-4, (r_min, r_max)
    assert abs(y_min - min(ys)) < 1e-4 and abs(y_max - max(ys)) < 1e-4, (y_min, y_max)
    return volume, r_max

# 1. A full-turn bushing with fractional radii, off the origin in Y.
BUSHING = [[4.25, -1.5], [10.5, -1.5], [10.5, 13.75], [4.25, 13.75]]
source = create("bushing", BUSHING)
catalog = inspect(source)
row = catalog["sketches"][0]
edit = row["constraint_edit"]
assert edit["available"] is True, edit
assert edit["profile_feature"]["kind"] == "full_turn_revolve", edit
assert "height_mm" not in edit["profile_feature"]
sketch = row["sketch_id"]
source_bytes = source.read_bytes()

# 2. Refused before anything is solved: an array request, an escaped
#    duplicate key, and an addition written as an array.
first = lines(catalog)[0]["curve_id"]
for body in (f'[1,[],[{{"curve_id":"{first}","rule":"horizontal"}}]]',
             f'{{"request_version":1,"remove":[],"add":[{{"curve_id":"{first}",'
             f'"curve_\\u0069d":"{first}","rule":"horizontal"}}]}}',
             f'{{"request_version":1,"remove":[],"add":[["horizontal","{first}"]]}}'):
    refused = constrain(source, body, root / "never.fcad", 2)
    assert refused["error"]["kind"] == "input", refused
assert not (root / "never.fcad").exists() and source.read_bytes() == source_bytes

# 3. Rigid: H/V on all four Lines, the inner bottom corner pinned where it
#    is stored, both sizes as stored.
rigid_body = {"request_version": 1, "remove": [], "add": [
    rule(catalog, 0, "horizontal"), rule(catalog, 1, "vertical"),
    rule(catalog, 2, "horizontal"), rule(catalog, 3, "vertical"),
    rule(catalog, 0, "fixed", at="start", x_mm=4.25, y_mm=-1.5),
    rule(catalog, 0, "distance", distance_mm=6.25),
    rule(catalog, 1, "distance", distance_mm=15.25)]}
rigid = root / "rigid.fcad"
rigid_request = root / "rigid.json"
rigid_request.write_text(json.dumps(rigid_body))
p = subprocess.run([cli, OP, source, "--sketch", sketch, "--expect-version",
                    catalog["content_version"], "--request", rigid_request,
                    "-o", rigid, "--json"], capture_output=True, encoding="utf-8")
reply = json.loads(p.stdout)
if p.returncode == 2 and "planegcs" in reply["error"]["message"]:
    assert reply["error"]["kind"] == "unsupported", reply
    assert not rigid.exists() and source.read_bytes() == source_bytes
    print("FCAD_27G_RECIPE_NO_SOLVER", json.dumps(reply["error"]["message"]))
    sys.exit(1 if os.environ.get("FCAD_EXPECT_SOLVER") == "1" else 0)
assert p.returncode == 0, (p.stdout, p.stderr)
published = reply["result"]
assert published["solve"] == {"degrees_of_freedom": 0, "redundant_constraint_ids": []}, published
assert len(published["added_constraints"]) == 4 + 7
allowlist(source, rigid, sketch)
assert lines(inspect(rigid)) == lines(catalog), "stored inputs never move"
rigid_volume, rigid_r = exported(rigid, BUSHING)

# 4. A changed dimension: the exact old length removed, a new one added.
catalog = inspect(rigid)
old = stored_constraint(catalog, "distance", 0)
wide = root / "wide.fcad"
changed = constrain(rigid, {"request_version": 1, "remove": [old],
                            "add": [rule(catalog, 0, "distance", distance_mm=7.5)]}, wide)["result"]
assert changed["removed_constraint_ids"] == [old], changed
assert changed["solve"]["degrees_of_freedom"] == 0, changed
allowlist(rigid, wide, sketch)
WIDE = [[4.25, -1.5], [11.75, -1.5], [11.75, 13.75], [4.25, 13.75]]
wide_volume, wide_r = exported(wide, WIDE)
assert wide_volume > rigid_volume and abs(wide_r - 11.75) < 1e-4

# 5. Typed errors, nothing written: a real conflict and a solved profile
#    that would cross the axis.
catalog = inspect(wide)
conflict = constrain(wide, {"request_version": 1, "remove": [], "add": [
    {"rule": "equal_length", "a_curve_id": lines(catalog)[0]["curve_id"],
     "b_curve_id": lines(catalog)[1]["curve_id"]}]}, root / "conflict.fcad", 2)["error"]
assert conflict["kind"] == "constraint", conflict
assert conflict["constraint_conflict"]["constraints"], conflict
pin = stored_constraint(catalog, "fixed", 0)
crossing = constrain(wide, {"request_version": 1, "remove": [pin], "add": [
    rule(catalog, 0, "fixed", at="start", x_mm=-1.0, y_mm=-1.5)]}, root / "crossing.fcad", 2)["error"]
assert "axis" in crossing["message"], crossing
assert not (root / "conflict.fcad").exists() and not (root / "crossing.fcad").exists()

# 6. Every user constraint removed by exact UUID: closure stays, the
#    stored inputs rebuild again, coordinate edits stay refused.
user = [c["constraint_id"] for c in catalog["sketches"][0]["constraint_edit"]["constraints"]
        if c["rule"]["kind"] != "coincident"]
bare = root / "bare.fcad"
removed = constrain(wide, {"request_version": 1, "remove": user, "add": []}, bare)["result"]
assert removed["solve"]["degrees_of_freedom"] == 8, removed
after = inspect(bare)["sketches"][0]
assert [c["rule"]["kind"] for c in after["constraint_edit"]["constraints"]] == ["coincident"] * 4
assert after["editable"] is False and "unconstrained" in after["refusal"]
exported(bare, BUSHING)

# 7. A 137.5° sector: dimensioned, its outer wall made taller; the angle is
#    kept and later edited with the constraints kept.
SECTOR = [[2.75, -3.25], [7.5, -3.25], [7.5, 2.125], [4.25, 9.5], [2.75, 9.5]]
sector = create("sector", SECTOR, {"kind": "angle", "degrees": 137.5})
catalog = inspect(sector)
assert catalog["sketches"][0]["constraint_edit"]["profile_feature"]["angle_deg"] == 137.5
sector_rigid = root / "sector-rigid.fcad"
constrain(sector, {"request_version": 1, "remove": [], "add": [
    rule(catalog, 0, "horizontal"), rule(catalog, 1, "vertical"),
    rule(catalog, 3, "horizontal"), rule(catalog, 4, "vertical"),
    rule(catalog, 0, "fixed", at="start", x_mm=2.75, y_mm=-3.25),
    rule(catalog, 0, "distance", distance_mm=4.75),
    rule(catalog, 1, "distance", distance_mm=5.375),
    rule(catalog, 3, "distance", distance_mm=1.5),
    rule(catalog, 4, "distance", distance_mm=12.75)]}, sector_rigid)
catalog = inspect(sector_rigid)
tall = root / "sector-tall.fcad"
constrain(sector_rigid, {"request_version": 1,
                         "remove": [stored_constraint(catalog, "distance", 1)],
                         "add": [rule(catalog, 1, "distance", distance_mm=7.0)]}, tall)
TALL = [[2.75, -3.25], [7.5, -3.25], [7.5, 3.75], [4.25, 9.5], [2.75, 9.5]]
sector_volume, _ = exported(tall, TALL, 137.5)
revolve = inspect(tall)["revolves"][0]
assert revolve["angle_edit"]["available"] is True, revolve
angle_request = root / "angle.json"
angle_request.write_text(json.dumps({"request_version": 1, "angle_deg": 212.25}))
turned = root / "sector-turned.fcad"
run(["edit-revolve-angle", tall, "--feature", revolve["feature_id"], "--expect-version",
     inspect(tall)["content_version"], "--request", angle_request, "-o", turned, "--json"])
assert inspect(turned)["sketches"][0]["constraint_edit"]["constraints"] == \
    inspect(tall)["sketches"][0]["constraint_edit"]["constraints"]
turned_volume, _ = exported(turned, TALL, 212.25)
assert source.read_bytes() == source_bytes
print("FCAD_27G_RECIPE_OK", json.dumps({
    "bushing_rigid_mm3": round(rigid_volume, 3), "bushing_wide_mm3": round(wide_volume, 3),
    "bushing_wide_outer_r_mm": round(wide_r, 5), "sector_tall_mm3": round(sector_volume, 3),
    "sector_turned_mm3": round(turned_volume, 3)}))
```
