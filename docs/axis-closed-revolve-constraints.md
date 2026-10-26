# Constrained profiles meeting the axis

The shared `edit` transaction modifies constraint UUIDs without changing stored
coordinate guesses. The evaluator solves the sketch, builds the sweep and checks
its dependents before publication. The kernel decides validity and axis contact;
there is no retained solid/bored profile class or prescribed axis boundary.
Explicit Fixed and H/V constraints express the intended geometry; no points are
snapped. See [generic sweep semantics](generic-sweeps.md).

This recipe exercises discovery, exact removals and additions, cold rebuilds,
independent STL measurements, FBX export, solver conflicts and atomic publication.
Extract the Python block with `tools/occt/check-cli-recipes.py`; its dependency guard
reports a missing kernel or solver honestly.

```python
# FCAD_27H_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
sys.path.insert(0, str(pathlib.Path.cwd() / "tools"))
from edit_requests import constraint_request, line_rule, distance, fixed as pin_rule, relation
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27h-"))
OP = "edit"

def output_names(path):
    text = run(["print-topology", path])
    feature, names = None, []
    for line in text.splitlines():
        fields = line.split()
        if len(fields) == 2 and fields[0] == "feature":
            feature = fields[1]
        elif len(fields) == 3 and fields[0] in ("face", "edge", "vertex"):
            assert feature and len(fields[1]) == 64, line
            assert fields[2] == "entities=1", line
            names.append((feature, fields[0], fields[1]))
    assert names, text
    return sorted(names)


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
    return run([OP, source, "--expect-version", catalog["content_version"], "--request", request,
                "-o", out, "--json"], code)

def lines(catalog):
    return catalog["sketches"][0]["curves"]



def stored_constraint(catalog, kind, i):
    curve = lines(catalog)[i]["curve_id"]
    for c in catalog["sketches"][0]["constraints"]:
        r = c["rule"]
        if r["kind"] == kind and curve in (r.get("a", {}).get("curve_id"),
                                           r.get("point", {}).get("curve_id")):
            return c["constraint_id"]
    raise KeyError((kind, i))

def axis_index(catalog):
    """Find the fixture boundary on the stated coordinate axis."""
    feature = catalog["revolves"][0]
    assert feature["axis"] == "sketch_y" and feature["axis_curve_id"] is None, feature
    candidates = [i for i, c in enumerate(lines(catalog))
                  if c["geometry"]["start_mm"][0] == 0 and c["geometry"]["end_mm"][0] == 0]
    assert len(candidates) == 1, candidates
    return candidates[0]

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
    """Only the Sketch row's payload/payload_hash may differ. The Revolve row, with
    its axis and extent, and every derived face name are compared exactly."""
    sid = bytes.fromhex(sketch.replace("-", ""))
    a, b = cells(source), cells(copy)
    assert a.keys() == b.keys()
    for t in a:
        def strip(rows):
            kept = []
            for r in rows:
                d = dict(r)
                if t == "objects" and d["id"] == sid:
                    for k in ("payload", "payload_hash"):
                        d.pop(k)
                if t == "meta":
                    d.pop("modified_at")
                kept.append(sorted(d.items()))
            return sorted(kept, key=repr)
        assert strip(a[t]) == strip(b[t]), f"{t}: a cell outside the allowlist"

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
edit = row


axis = axis_index(catalog)
assert axis == 3
sketch = row["sketch_id"]
source_bytes = source.read_bytes()
# One saved face name per Line but the axis Line.
assert len([n for n in output_names(source) if n[1] == "face"]) == 3

# 2. Strict request parsing rejects an array and an escaped duplicate key.
first = lines(catalog)[0]["curve_id"]
for body in ('[]', '{"request_version":1,"changes":[],"chan\\u0067es":[]}'):
    refused = constrain(source, body, root / "never.fcad", 2)
    assert refused["error"]["kind"] == "input", refused
assert not (root / "never.fcad").exists() and source.read_bytes() == source_bytes

# 3. Rigid: H/V on all four Lines, the axis Line's lower end pinned at X 0,
#    both sizes as stored. Nothing is snapped: the pin and the V are what
#    keep the solved axis Line on the axis.
rigid_body = constraint_request(catalog["sketches"][0], [], [
    line_rule(lines(catalog)[0]["curve_id"], "horizontal"), line_rule(lines(catalog)[1]["curve_id"], "vertical"),
    line_rule(lines(catalog)[2]["curve_id"], "horizontal"), line_rule(lines(catalog)[3]["curve_id"], "vertical"),
    pin_rule(lines(catalog)[3]["curve_id"], "end", 0.0, -1.5),
    distance(lines(catalog)[0]["curve_id"], 7.25),
    distance(lines(catalog)[1]["curve_id"], 15.25)])
rigid = root / "rigid.fcad"
rigid_request = root / "rigid.json"
rigid_request.write_text(json.dumps(rigid_body))
p = subprocess.run([cli, OP, source, "--expect-version",
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

allowlist(source, rigid, sketch)
assert lines(inspect(rigid)) == lines(catalog), "stored inputs never move"
rigid_volume, _ = exported(rigid, CYLINDER)
assert abs(rigid_volume - math.pi * 7.25 ** 2 * 15.25) < 0.02 * rigid_volume

# 4. A changed radius: the exact old length removed, a new one added.
catalog = inspect(rigid)
old = stored_constraint(catalog, "distance", 0)
wide = root / "wide.fcad"
changed = constrain(rigid, constraint_request(catalog["sketches"][0], [old], [distance(lines(catalog)[0]["curve_id"], 8.5)]), wide)["result"]


allowlist(rigid, wide, sketch)
WIDE = [[0, -1.5], [8.5, -1.5], [8.5, 13.75], [0, 13.75]]
wide_volume, wide_r = exported(wide, WIDE)
assert wide_volume > rigid_volume and abs(wide_r - 8.5) < 1e-4

# 5. Refused, nothing written: the radius changed without the pin (the
#    solver moves the axis Line off X 0), the axis Line pinned across the
#    axis, and a real conflict.
pin = stored_constraint(catalog, "fixed", 3)
for name, body in (
        ("unpinned", constraint_request(catalog["sketches"][0], [pin, old], [distance(lines(catalog)[0]["curve_id"], 8.5)])),
        ("crossing", constraint_request(catalog["sketches"][0], [pin], [pin_rule(lines(catalog)[3]["curve_id"], "end", -1.0, -1.5)]))):
    error = constrain(rigid, body, root / f"{name}.fcad", 2)["error"]
    assert "axis" in error["message"] and "constraint_conflict" not in error, error
    assert not (root / f"{name}.fcad").exists()
conflict = constrain(rigid, constraint_request(catalog["sketches"][0], [], [
    relation(lines(catalog)[0]["curve_id"], lines(catalog)[1]["curve_id"], "equal_length")]), root / "conflict.fcad", 2)["error"]
assert conflict["kind"] == "constraint", conflict
assert conflict["constraint_conflict"]["constraints"], conflict

# 6. Every user constraint removed by exact UUID: the closure stays, the
#    stored inputs rebuild again, coordinate edits stay refused.
catalog = inspect(wide)
user = [c["constraint_id"] for c in catalog["sketches"][0]["constraints"]
        if c["rule"]["kind"] != "coincident"]
bare = root / "bare.fcad"
removed = constrain(wide, constraint_request(catalog["sketches"][0], user, []), bare)["result"]

after = inspect(bare)["sketches"][0]
assert [c["rule"]["kind"] for c in after["constraints"]] == ["coincident"] * 4

exported(bare, CYLINDER)

# 7. A solid stepped shaft turned through 137.5°: dimensioned, its base
#    widened so the upper step follows; both end faces keep their names; the
#    angle is then edited with the constraints kept.
SHAFT = [[0, -2.25], [8, -2.25], [8, 3.5], [5.5, 3.5], [5.5, 12.75], [0, 12.75]]
shaft = create("shaft", SHAFT, {"kind": "angle", "degrees": 137.5})
catalog = inspect(shaft)

assert axis_index(catalog) == 5
shaft_sketch = catalog["sketches"][0]["sketch_id"]
shaft_rigid = root / "shaft-rigid.fcad"
constrain(shaft, constraint_request(catalog["sketches"][0], [], [
    *(line_rule(lines(catalog)[i]["curve_id"], "horizontal" if i % 2 == 0 else "vertical") for i in range(6)),
    pin_rule(lines(catalog)[0]["curve_id"], "start", 0.0, -2.25),
    distance(lines(catalog)[0]["curve_id"], 8.0),
    distance(lines(catalog)[1]["curve_id"], 5.75),
    distance(lines(catalog)[2]["curve_id"], 2.5),
    distance(lines(catalog)[3]["curve_id"], 9.25)]), shaft_rigid)
allowlist(shaft, shaft_rigid, shaft_sketch)
catalog = inspect(shaft_rigid)
shaft_wide = root / "shaft-wide.fcad"
constrain(shaft_rigid, constraint_request(catalog["sketches"][0], [stored_constraint(catalog, "distance", 0)], [distance(lines(catalog)[0]["curve_id"], 9.25)]), shaft_wide)
allowlist(shaft_rigid, shaft_wide, shaft_sketch)
assert len([n for n in output_names(shaft_wide) if n[1] == "face"]) == 5 + 2, "five faces and two caps"
SHAFT_WIDE = [[0, -2.25], [9.25, -2.25], [9.25, 3.5], [6.75, 3.5], [6.75, 12.75], [0, 12.75]]
shaft_volume, shaft_r = exported(shaft_wide, SHAFT_WIDE, 137.5)
revolve = inspect(shaft_wide)["revolves"][0]
angle_request = root / "angle.json"
angle_request.write_text(json.dumps({"request_version": 1, "changes": [{"kind": "set_expression", "object": revolve["feature_id"], "slot": {"kind": "revolve_angle"}, "value": 212.25, "unit": "deg"}]}))
turned = root / "shaft-turned.fcad"
run(["edit", shaft_wide, "--expect-version", inspect(shaft_wide)["content_version"], "--request", angle_request, "-o", turned, "--json"])
assert inspect(turned)["sketches"][0]["constraints"] == \
    inspect(shaft_wide)["sketches"][0]["constraints"]
turned_volume, _ = exported(turned, SHAFT_WIDE, 212.25)
assert source.read_bytes() == source_bytes
print("FCAD_27H_RECIPE_OK", json.dumps({
    "cylinder_rigid_mm3": round(rigid_volume, 3), "cylinder_wide_mm3": round(wide_volume, 3),
    "cylinder_wide_r_mm": round(wide_r, 5), "shaft_wide_mm3": round(shaft_volume, 3),
    "shaft_wide_r_mm": round(shaft_r, 5), "shaft_turned_mm3": round(turned_volume, 3)}))
```
