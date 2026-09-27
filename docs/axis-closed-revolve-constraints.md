# §27H — dimensional constraints on a saved Revolve profile closed on the axis

[Executed verification and limitations](axis-closed-revolve-constraints-verification.md).

A person or an agent opens a saved **solid** Revolve (§27C) or a solid
sector (§27D): a cylinder, a cone, a stepped shaft. It is closed on the sketch
Y axis along one stated Line (`axis_segment`). The profile's Lines can be
dimensioned through the existing **Edit constraints** window or the existing
`edit-sketch-constraints-copy`, with:

* H/V and a Line length;
* one Fixed endpoint;
* EqualLength, Parallel and Perpendicular.

A dimension can be changed later by exact UUID. The new copy's Body turns the
**solved** profile. It keeps:

* the stored axis Line, which still raises no face;
* the turn angle and every other Line's face name;
* on a sector, both `RevolveCap` names.

This widens the class the §27G constraint editor accepts. The bored-profile
behaviour of §27G is unchanged. There is no new command, request field, rule
family, solver, copier, capability or editor.

## Contract recorded before implementation

### Which documents are editable

The §27G frame is used unchanged: four root objects, namely the untransformed
XY datum, the Sketch, one SketchY/NewBody Revolve of that Sketch and its Body,
with exactly the plane, profile and body-tip dependencies. §27G refused a
Revolve that states an `axis_segment`. §27H accepts it: full turn (payload
v2) and partial (payload v4).

The **stored** Lines must already satisfy the §27C class policy for the
stated axis Line (`stated_revolution`). Concretely:

* both ends of the stated Line have x = 0 exactly;
* every other vertex is beyond the positive clearance;
* no other Line lies on the axis.

The stored constraints must be the families the editor already manages.

Still refused, each with its own reason: Circles and Arcs, other axes and
extents, booleans, multibody and other document shapes. A Revolve stating an
`axis_segment` that is not one of the Sketch's Lines is refused as before.

### The numerical contract at the axis

Measured before widening the refusal. A probe calls the shim's own session
API (`fc_gcs_session_*`) the way the evaluator does: two points per Line,
closure Coincidents first, then the additions. It is compiled from the same
FreeCAD 1.0.1 `planegcs` sources and Eigen 3.4.0, with Boost 1.83 because the
pinned 1.91 archive is unreachable from this container. The probe is recorded
in the verification. What the pinned library does on all three platforms is
established again by the CI gates.

* **Aliasing.** planegcs turns H, V and Coincident into parameter-to-parameter
  equalities and *reduces* them in `initSolution`: the two x of a vertical
  Line, and the two coordinates of a closure joint, become one parameter. A V
  on the stated axis Line therefore makes its two ends bit-identical in x.
* **Fixed.** A Fixed endpoint becomes `CoordinateX`/`CoordinateY` equalities to
  a constant. These are **not** reduced; they are solved.
* **Measured results, with a pin at x = 0 on the axis Line (or on the corner
  it shares with its neighbour) and V on the axis Line.** The solved axis ends
  came back exactly `0x0p+0` for:
  * a rigid set equal to the stored profile;
  * a wider and a taller cylinder;
  * fractional offsets;
  * a cone;
  * a stepped shaft.
* **Without any constraint holding the axis Line on x = 0.** Changing a radius
  moved the axis Line to x = −1.58 (no pin), or to −2.5 (only the outer corner
  pinned).

The contract is therefore:

1. **Every evaluation checks the solved profile.** The solved Lines of every
   cold or cached evaluation pass through the existing `stated_revolution`
   with the Revolve's own `axis_segment`, and so does the copy job's
   solved-profile check before publication. That check requires:
   * both ends of that **same** Line at x = 0.0 **exactly**;
   * every other vertex beyond the clearance;
   * no second axis contact.
2. **Nothing is snapped.** Near-axis values are not snapped, the axis Line is
   not substituted, no seam or face is invented, and no Fixed is hidden or
   added. A solution whose axis ends are not exactly 0.0 is outside the class
   and refuses publication, even if the solver residual is tiny. The refusal
   reads, for example, "vertex … is not strictly on the positive radial side
   and not on the axis" or "no longer touches the axis".
3. **Pin the axis explicitly.** To change a size, the user states the axis
   with the existing rules: a Fixed endpoint on the axis Line (x = 0) and V on
   that Line. The window's owner line and the discovery kinds say so. The
   editor does not add these for the user.
4. **This is a numeric limitation, stated rather than hidden.** It holds for
   the measured library. A platform whose solver returned a non-zero x for a
   pinned axis end would refuse publication, not publish a wrong part.

### What a request may say, and what stays

The §27G contract applies unchanged:

* **Request.** The same request v1 with its seven Line forms and closure
  Coincidents, exact-UUID removal, atomic Replace, the 128-step history, and
  the strict request decode.
* **SQL allowlist.** Only the Sketch row's `payload`, `payload_hash` and
  `schema_version` change, plus the `sketch.constraints.v1` capability row.
  The Revolve row stays byte-identical: extent, angle, axis and
  `axis_segment`.
* **Identities.** Every object and Line UUID, every face reference and both
  caps stay the same.
* **Axis Line.** It still produces no face, and no reference changes producer.

Removing the last user constraint keeps the closure. The Sketch then still
requires a solver, and coordinate editing (`editable`) keeps refusing it. The
angle edit of a constrained solid sector keeps its constraints.

### Discovery (additive)

For these rows `sketches[].constraint_edit` becomes available. Its
`profile_feature` uses the existing kinds `full_turn_revolve_axis_closed` and
`partial_turn_revolve_axis_closed`, with `axis_curve_id`, so an agent knows
which Line to pin. JSON v1 field names and types are unchanged.

### Compatibility

There is no new capability. A constrained solid Revolve requires
`sketch.constraints.v1`, which every build since §25E knows. The evaluator
already solves the Sketch and applies `stated_revolution` with the stated
axis Line; neither changes here. A prior-main reader therefore reads and
validates these documents, and rebuilds them with a solver. Its constraint
editor refuses axis-closed profiles by name, which is the honest read-only
boundary for that edit.

### UI

The existing window's owner line names the axis Line and says that it stays
on the axis. The worker, draft, history, Replace length and Save/Cancel are
unchanged. The solver never runs on redraw.

## Agent recipe

An agent needs only `inspect --json`, the UUIDs it reports and the existing
request. The recipe below runs the public contract end to end:

* discovery of the stated axis Line;
* a rigid dimensioning that pins that Line on the axis;
* a changed dimension, by exact remove + add;
* the refusals when a solution leaves the axis or the class, and a typed
  conflict;
* an exact removal down to the closure;
* a solid sector's angle edit that keeps the constraints;
* cold rebuilds, and STL and FBX exports measured independently of the
  product.

Extract it from this file and run it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/axis-closed-revolve-constraints.md").read_text(encoding="utf-8")
code = text.split("# FCAD_27H_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27h-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-27h-recipe.py
```

A build without PlaneGCS stops at the first publication and prints
`FCAD_27H_RECIPE_NO_SOLVER` with the typed `unsupported` error; with
`FCAD_EXPECT_SOLVER=1` that is a failure. A complete run prints
`FCAD_27H_RECIPE_OK` with the measured volumes and radii.

```python
# FCAD_27H_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27h-"))
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

def axis_index(catalog):
    """The stated axis Line, as the constraint editor and the Revolve report it."""
    feature = catalog["sketches"][0]["constraint_edit"]["profile_feature"]
    assert feature["axis_curve_id"] == catalog["revolves"][0]["axis_curve_id"], feature
    ids = [c["curve_id"] for c in lines(catalog)]
    return ids.index(feature["axis_curve_id"])

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
    sketch.constraints.v1 capability row may differ. The Revolve row, with
    its axis_segment, and every saved face name are compared exactly."""
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
    n = len(points)
    band = sum(2 * math.pi * max(points[i][0], points[(i + 1) % n][0]) * 0.05
               * abs(points[(i + 1) % n][1] - points[i][1]) for i in range(n)) * degrees / 360
    assert volume > 0 and abs(exact - volume) <= band, (volume, exact, band)
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    # Closed on the axis: nothing is bored out, so the volume above is the
    # whole solid. A flat end face need not carry a vertex at its centre,
    # so the smallest vertex radius says nothing here; it is only >= 0.
    assert min(xs) == 0 and r_min >= 0, r_min
    assert abs(r_max - max(xs)) < 1e-4, r_max
    assert abs(y_min - min(ys)) < 1e-4 and abs(y_max - max(ys)) < 1e-4, (y_min, y_max)
    return volume, r_max

# 1. A solid cylinder with fractional sizes, off the origin in Y; its last
#    Line lies on the axis.
CYLINDER = [[0, -1.5], [7.25, -1.5], [7.25, 13.75], [0, 13.75]]
source = create("cylinder", CYLINDER)
catalog = inspect(source)
row = catalog["sketches"][0]
edit = row["constraint_edit"]
assert edit["available"] is True, edit
assert edit["profile_feature"]["kind"] == "full_turn_revolve_axis_closed", edit
axis = axis_index(catalog)
assert axis == 3
sketch = row["sketch_id"]
source_bytes = source.read_bytes()
# One saved face name per Line but the axis Line.
assert len(cells(source)["topology_refs"]) == 3

# 2. Refused before anything is solved: an escaped duplicate key.
first = lines(catalog)[axis]["curve_id"]
body = (f'{{"request_version":1,"remove":[],"add":[{{"curve_id":"{first}",'
        f'"curve_\\u0069d":"{first}","rule":"vertical"}}]}}')
assert "\\u0069" in body
refused = constrain(source, body, root / "never.fcad", 2)
assert refused["error"]["kind"] == "input", refused
assert not (root / "never.fcad").exists() and source.read_bytes() == source_bytes

# 3. Rigid: H/V on all four Lines, the axis Line's lower end pinned at X 0,
#    both sizes as stored. Nothing is snapped: the pin and the V are what
#    keep the solved axis Line on the axis.
rigid_body = {"request_version": 1, "remove": [], "add": [
    rule(catalog, 0, "horizontal"), rule(catalog, 1, "vertical"),
    rule(catalog, 2, "horizontal"), rule(catalog, 3, "vertical"),
    rule(catalog, 3, "fixed", at="end", x_mm=0.0, y_mm=-1.5),
    rule(catalog, 0, "distance", distance_mm=7.25),
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
    print("FCAD_27H_RECIPE_NO_SOLVER", json.dumps(reply["error"]["message"]))
    sys.exit(1 if os.environ.get("FCAD_EXPECT_SOLVER") == "1" else 0)
assert p.returncode == 0, (p.stdout, p.stderr)
published = reply["result"]
assert published["solve"] == {"degrees_of_freedom": 0, "redundant_constraint_ids": []}, published
allowlist(source, rigid, sketch)
assert lines(inspect(rigid)) == lines(catalog), "stored inputs never move"
rigid_volume, _ = exported(rigid, CYLINDER)
assert abs(rigid_volume - math.pi * 7.25 ** 2 * 15.25) < 0.02 * rigid_volume

# 4. A changed radius: the exact old length removed, a new one added.
catalog = inspect(rigid)
old = stored_constraint(catalog, "distance", 0)
wide = root / "wide.fcad"
changed = constrain(rigid, {"request_version": 1, "remove": [old],
                            "add": [rule(catalog, 0, "distance", distance_mm=8.5)]}, wide)["result"]
assert changed["removed_constraint_ids"] == [old], changed
assert changed["solve"]["degrees_of_freedom"] == 0, changed
allowlist(rigid, wide, sketch)
WIDE = [[0, -1.5], [8.5, -1.5], [8.5, 13.75], [0, 13.75]]
wide_volume, wide_r = exported(wide, WIDE)
assert wide_volume > rigid_volume and abs(wide_r - 8.5) < 1e-4

# 5. Refused, nothing written: the radius changed without the pin (the
#    solver moves the axis Line off X 0), the axis Line pinned across the
#    axis, and a real conflict.
pin = stored_constraint(catalog, "fixed", 3)
for name, body in (
        ("unpinned", {"request_version": 1, "remove": [pin, old],
                      "add": [rule(catalog, 0, "distance", distance_mm=8.5)]}),
        ("crossing", {"request_version": 1, "remove": [pin],
                      "add": [rule(catalog, 3, "fixed", at="end", x_mm=-1.0, y_mm=-1.5)]})):
    error = constrain(rigid, body, root / f"{name}.fcad", 2)["error"]
    assert "axis" in error["message"] and "constraint_conflict" not in error, error
    assert not (root / f"{name}.fcad").exists()
conflict = constrain(rigid, {"request_version": 1, "remove": [], "add": [
    {"rule": "equal_length", "a_curve_id": lines(catalog)[0]["curve_id"],
     "b_curve_id": lines(catalog)[1]["curve_id"]}]}, root / "conflict.fcad", 2)["error"]
assert conflict["kind"] == "constraint", conflict
assert conflict["constraint_conflict"]["constraints"], conflict

# 6. Every user constraint removed by exact UUID: the closure stays, the
#    stored inputs rebuild again, coordinate edits stay refused.
catalog = inspect(wide)
user = [c["constraint_id"] for c in catalog["sketches"][0]["constraint_edit"]["constraints"]
        if c["rule"]["kind"] != "coincident"]
bare = root / "bare.fcad"
removed = constrain(wide, {"request_version": 1, "remove": user, "add": []}, bare)["result"]
assert removed["solve"]["degrees_of_freedom"] == 8, removed
after = inspect(bare)["sketches"][0]
assert [c["rule"]["kind"] for c in after["constraint_edit"]["constraints"]] == ["coincident"] * 4
assert after["editable"] is False and "unconstrained" in after["refusal"]
exported(bare, CYLINDER)

# 7. A solid stepped shaft turned through 137.5°: dimensioned, its base
#    widened so the upper step follows; both end faces keep their names; the
#    angle is then edited with the constraints kept.
SHAFT = [[0, -2.25], [8, -2.25], [8, 3.5], [5.5, 3.5], [5.5, 12.75], [0, 12.75]]
shaft = create("shaft", SHAFT, {"kind": "angle", "degrees": 137.5})
catalog = inspect(shaft)
assert catalog["sketches"][0]["constraint_edit"]["profile_feature"]["kind"] == \
    "partial_turn_revolve_axis_closed"
assert axis_index(catalog) == 5
shaft_sketch = catalog["sketches"][0]["sketch_id"]
shaft_rigid = root / "shaft-rigid.fcad"
constrain(shaft, {"request_version": 1, "remove": [], "add": [
    *(rule(catalog, i, "horizontal" if i % 2 == 0 else "vertical") for i in range(6)),
    rule(catalog, 0, "fixed", at="start", x_mm=0.0, y_mm=-2.25),
    rule(catalog, 0, "distance", distance_mm=8.0),
    rule(catalog, 1, "distance", distance_mm=5.75),
    rule(catalog, 2, "distance", distance_mm=2.5),
    rule(catalog, 3, "distance", distance_mm=9.25)]}, shaft_rigid)
allowlist(shaft, shaft_rigid, shaft_sketch)
catalog = inspect(shaft_rigid)
shaft_wide = root / "shaft-wide.fcad"
constrain(shaft_rigid, {"request_version": 1,
                        "remove": [stored_constraint(catalog, "distance", 0)],
                        "add": [rule(catalog, 0, "distance", distance_mm=9.25)]}, shaft_wide)
allowlist(shaft_rigid, shaft_wide, shaft_sketch)
assert len(cells(shaft_wide)["topology_refs"]) == 5 + 2, "five faces and two caps"
SHAFT_WIDE = [[0, -2.25], [9.25, -2.25], [9.25, 3.5], [6.75, 3.5], [6.75, 12.75], [0, 12.75]]
shaft_volume, shaft_r = exported(shaft_wide, SHAFT_WIDE, 137.5)
revolve = inspect(shaft_wide)["revolves"][0]
assert revolve["angle_edit"]["available"] is True, revolve
angle_request = root / "angle.json"
angle_request.write_text(json.dumps({"request_version": 1, "angle_deg": 212.25}))
turned = root / "shaft-turned.fcad"
run(["edit-revolve-angle", shaft_wide, "--feature", revolve["feature_id"], "--expect-version",
     inspect(shaft_wide)["content_version"], "--request", angle_request, "-o", turned, "--json"])
assert inspect(turned)["sketches"][0]["constraint_edit"]["constraints"] == \
    inspect(shaft_wide)["sketches"][0]["constraint_edit"]["constraints"]
turned_volume, _ = exported(turned, SHAFT_WIDE, 212.25)
assert source.read_bytes() == source_bytes
print("FCAD_27H_RECIPE_OK", json.dumps({
    "cylinder_rigid_mm3": round(rigid_volume, 3), "cylinder_wide_mm3": round(wide_volume, 3),
    "cylinder_wide_r_mm": round(wide_r, 5), "shaft_wide_mm3": round(shaft_volume, 3),
    "shaft_wide_r_mm": round(shaft_r, 5), "shaft_turned_mm3": round(turned_volume, 3)}))
```
