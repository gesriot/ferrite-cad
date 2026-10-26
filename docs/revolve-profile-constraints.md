# Constrained Revolve profiles

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
# FCAD_27G_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
sys.path.insert(0, str(pathlib.Path.cwd() / "tools"))
from edit_requests import constraint_request, line_rule, distance, fixed as pin_rule, relation
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27g-"))
OP = "edit"

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
    """Only the Sketch row's payload/payload_hash may differ."""
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
edit = row



sketch = row["sketch_id"]
source_bytes = source.read_bytes()

# 2. Strict request parsing rejects an array and an escaped duplicate key.
first = lines(catalog)[0]["curve_id"]
for body in ('[]', '{"request_version":1,"changes":[],"chan\\u0067es":[]}'):
    refused = constrain(source, body, root / "never.fcad", 2)
    assert refused["error"]["kind"] == "input", refused
assert not (root / "never.fcad").exists() and source.read_bytes() == source_bytes

# 3. Rigid: H/V on all four Lines, the inner bottom corner pinned where it
#    is stored, both sizes as stored.
rigid_body = constraint_request(catalog["sketches"][0], [], [
    line_rule(lines(catalog)[0]["curve_id"], "horizontal"), line_rule(lines(catalog)[1]["curve_id"], "vertical"),
    line_rule(lines(catalog)[2]["curve_id"], "horizontal"), line_rule(lines(catalog)[3]["curve_id"], "vertical"),
    pin_rule(lines(catalog)[0]["curve_id"], "start", 4.25, -1.5),
    distance(lines(catalog)[0]["curve_id"], 6.25),
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
    print("FCAD_27G_RECIPE_NO_SOLVER", json.dumps(reply["error"]["message"]))
    sys.exit(1 if os.environ.get("FCAD_EXPECT_SOLVER") == "1" else 0)
assert p.returncode == 0, (p.stdout, p.stderr)
published = reply["result"]


allowlist(source, rigid, sketch)
assert lines(inspect(rigid)) == lines(catalog), "stored inputs never move"
rigid_volume, rigid_r = exported(rigid, BUSHING)

# 4. A changed dimension: the exact old length removed, a new one added.
catalog = inspect(rigid)
old = stored_constraint(catalog, "distance", 0)
wide = root / "wide.fcad"
changed = constrain(rigid, constraint_request(catalog["sketches"][0], [old], [distance(lines(catalog)[0]["curve_id"], 7.5)]), wide)["result"]


allowlist(rigid, wide, sketch)
WIDE = [[4.25, -1.5], [11.75, -1.5], [11.75, 13.75], [4.25, 13.75]]
wide_volume, wide_r = exported(wide, WIDE)
assert wide_volume > rigid_volume and abs(wide_r - 11.75) < 1e-4

# 5. Typed errors, nothing written: a real conflict and a solved profile
#    that would cross the axis.
catalog = inspect(wide)
conflict = constrain(wide, constraint_request(catalog["sketches"][0], [], [
    relation(lines(catalog)[0]["curve_id"], lines(catalog)[1]["curve_id"], "equal_length")]), root / "conflict.fcad", 2)["error"]
assert conflict["kind"] == "constraint", conflict
assert conflict["constraint_conflict"]["constraints"], conflict
pin = stored_constraint(catalog, "fixed", 0)
crossing = constrain(wide, constraint_request(catalog["sketches"][0], [pin], [
    pin_rule(lines(catalog)[0]["curve_id"], "start", -1.0, -1.5)]), root / "crossing.fcad", 2)["error"]
assert "axis" in crossing["message"], crossing
assert not (root / "conflict.fcad").exists() and not (root / "crossing.fcad").exists()

# 6. Every user constraint removed by exact UUID: closure stays, the
#    stored inputs rebuild again, coordinate edits stay refused.
user = [c["constraint_id"] for c in catalog["sketches"][0]["constraints"]
        if c["rule"]["kind"] != "coincident"]
bare = root / "bare.fcad"
removed = constrain(wide, constraint_request(catalog["sketches"][0], user, []), bare)["result"]

after = inspect(bare)["sketches"][0]
assert [c["rule"]["kind"] for c in after["constraints"]] == ["coincident"] * 4

exported(bare, BUSHING)

# 7. A 137.5° sector: dimensioned, its outer wall made taller; the angle is
#    kept and later edited with the constraints kept.
SECTOR = [[2.75, -3.25], [7.5, -3.25], [7.5, 2.125], [4.25, 9.5], [2.75, 9.5]]
sector = create("sector", SECTOR, {"kind": "angle", "degrees": 137.5})
catalog = inspect(sector)

sector_rigid = root / "sector-rigid.fcad"
constrain(sector, constraint_request(catalog["sketches"][0], [], [
    line_rule(lines(catalog)[0]["curve_id"], "horizontal"), line_rule(lines(catalog)[1]["curve_id"], "vertical"),
    line_rule(lines(catalog)[3]["curve_id"], "horizontal"), line_rule(lines(catalog)[4]["curve_id"], "vertical"),
    pin_rule(lines(catalog)[0]["curve_id"], "start", 2.75, -3.25),
    distance(lines(catalog)[0]["curve_id"], 4.75),
    distance(lines(catalog)[1]["curve_id"], 5.375),
    distance(lines(catalog)[3]["curve_id"], 1.5),
    distance(lines(catalog)[4]["curve_id"], 12.75)]), sector_rigid)
catalog = inspect(sector_rigid)
tall = root / "sector-tall.fcad"
constrain(sector_rigid, constraint_request(catalog["sketches"][0], [stored_constraint(catalog, "distance", 1)], [distance(lines(catalog)[1]["curve_id"], 7.0)]), tall)
TALL = [[2.75, -3.25], [7.5, -3.25], [7.5, 3.75], [4.25, 9.5], [2.75, 9.5]]
sector_volume, _ = exported(tall, TALL, 137.5)
revolve = inspect(tall)["revolves"][0]
angle_request = root / "angle.json"
angle_request.write_text(json.dumps({"request_version": 1, "changes": [{"kind": "set_expression", "object": revolve["feature_id"], "slot": {"kind": "revolve_angle"}, "value": 212.25, "unit": "deg"}]}))
turned = root / "sector-turned.fcad"
run(["edit", tall, "--expect-version", inspect(tall)["content_version"], "--request", angle_request, "-o", turned, "--json"])
assert inspect(turned)["sketches"][0]["constraints"] == \
    inspect(tall)["sketches"][0]["constraints"]
turned_volume, _ = exported(turned, TALL, 212.25)
assert source.read_bytes() == source_bytes
print("FCAD_27G_RECIPE_OK", json.dumps({
    "bushing_rigid_mm3": round(rigid_volume, 3), "bushing_wide_mm3": round(wide_volume, 3),
    "bushing_wide_outer_r_mm": round(wide_r, 5), "sector_tall_mm3": round(sector_volume, 3),
    "sector_turned_mm3": round(turned_volume, 3)}))
```
