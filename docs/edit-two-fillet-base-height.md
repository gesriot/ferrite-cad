# §28I — change the height of a plate rounded twice, in a new copy

[Executed verification and limitations](edit-two-fillet-base-height-verification.md).
[The height edit under one Fillet](edit-fillet-base-height.md);
[two sequential Fillets](sequential-edge-fillets.md);
[their radii](edit-sequential-fillet-radii.md).

A person opens a plate that §28G rounded twice — Extrude → Fillet 1 →
Fillet 2 → Body tip — changes the plate's Blind height in the existing **Edit
extrusion** form, and saves a new `.fcad`. Both rounded edges keep their
Fillets, radii, corners and names; only the height changed. An agent does the
same with the existing `edit-extrude`. There is no new command, request
format, copy pipeline, solver or geometric route: this is §28C's edit on
§28H's reading of the history.

## Contract recorded before implementation

### The supported source

Exactly the §28G class, read again from the saved history on every
discovery, preparation and write by the reader the radius edit uses for two
Fillets (`fillet_radius::saved_fillet_for_radius` →
`saved_sequential_fillet`, over `cut_edit::saved_history_under_fillets`):

* one untransformed XY datum and one Sketch whose stored Lines are an
  axis-aligned rectangle — unconstrained, or carrying the constraint editor's
  managed Line family;
* one forward literal Blind `Extrude`/`NewBody` whose distance is not a
  formula or parameter (`editable_extrude`, unchanged);
* **Fillet 1**: `previous` = `edge.feature` = that Extrude;
* **Fillet 2**: `previous` = Fillet 1, `edge.feature` = that Extrude, another
  corner; the Body's tip;
* no Cut, no other object; exactly the plane, profile, two predecessor and
  one body-tip dependencies; Fillet 1's seven §28A names and Fillet 2's
  eight §28G names, by meaning, and no other name owned by either.

The selected feature must be the base Extrude (Fillet 1's `previous`). Both
saved Fillets are read and carried as they are; their radii, and the pair
rule between them, are the ones §28H checks. A plate with one Fillet takes
§28C's path unchanged (`saved_fillet`, the same frame as before).

The guard that the Sketch-coordinate, constraint and add-Fillet editors apply
(`fillet_over_plate`, exactly one Fillet) is **not** widened: with two
Fillets they keep refusing, now saying that the radii and the height can be
edited.

### Typed context

The height preparation and the extrusion catalogue carry the Fillets over the
plate as they stand in the history: `fillet` is the Fillet whose `previous`
is the base (Fillet 1, as in §28C), and a new `second_fillet` is the Fillet
whose `previous` is Fillet 1 (`history_index` 2). Fillet 2 is never described
as a direct child of the base. JSON `features[].fillet_base` keeps its shape
and still describes the Fillet on the base; a new additive
`fillet_base.second_fillet` describes Fillet 2 (`fillet_feature_id`,
`previous_feature_id`, `history_index`, `edge`, `corner_mm`, `radius_mm`),
and is `null` for a plate with one Fillet.

### Height policy

§28C's, unchanged: a finite, positive literal distance; the domain adds no
bound of its own. A height the kernel cannot round is refused by the copy's
strict rebuild (`kernel`), nothing published. For a constrained plate the
rebuild solves the Sketch as it always does and judges both corners and the
pair on the solved Lines; the height does not enter those checks.

### SQL allowlist

* `objects`: the base Extrude row's `payload` and `payload_hash`;
* `meta.modified_at`.

Nothing else: not the Sketch (its stored coordinates stay the solver's
starting guess, never replaced by solved ones), not either Fillet row, not
dependencies, names, capabilities, `schema_version`, the Body tip or the
document id. Nothing is minted, squashed, retargeted or re-created. The
writer re-derives the whole preparation inside its transaction — both Fillets
included — against the document version it was prepared from; a forged
payload, a stale version or a changed Fillet is refused.

### Rebuild, names and cache

The copy's strict cold rebuild must resolve every stored name, including
Fillet 1's cylinder as carried into the final Body (`origin_fillet_face`) and
Fillet 2's own. The cache keys chain through the predecessor, so a new
height misses the Extrude and both Fillets; a second rebuild hits all three.

### Refused, nothing written

* the Sketch, either Fillet or any other feature as `--feature`;
* a two-Fillet history outside the class above (a Cut, a third Fillet,
  forged names, a same-corner pair, a formula height);
* a zero, negative or non-finite height (`input`);
* a height the kernel cannot round (`kernel`, at the rebuild).

### Out of scope

A third Fillet, arbitrary edges, Cut with Fillet, Chamfer, editing the
constraints of a two-Fillet history, in-place Save. (The Sketch's coordinates
are [§28J](edit-two-fillet-base-sketch.md).)

## Recipe: inspect -> exact UUIDs -> height up and down -> a radius after -> cold rebuild, export

For a caller driving the CLI with JSON v1. Extract it from this file and run
it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-two-fillet-base-height.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28I_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28i-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28i-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_28I_RECIPE_NO_KERNEL`. Any build with Open CASCADE — the plate is
unconstrained, so no solver is asked — prints `FCAD_28I_RECIPE_OK` with the
measured mesh and exact volumes at each height.

```python
# FCAD_28I_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28i-"))
OP = "edit-extrude"
RADIUS = "edit-fillet-radius"

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
            print("FCAD_28I_RECIPE_NO_KERNEL", json.dumps(error))
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

def allowlist(source, copy, feature):
    """Only the selected row's payload/payload_hash and meta.modified_at may
    differ; every table keeps its rows and every other cell."""
    fid = bytes.fromhex(feature.replace("-", ""))
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
                ok = (t == "objects" and c in ("payload", "payload_hash") and x[ac.index("id")] == fid) \
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
CORNERS = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]
FIRST, SECOND = [X0 + W, Y0], [X0 + W, Y0 + D]   # adjacent: they share the Line x = 33

def measured(copy, rounded, H=H):
    """After reopening: valid, a cold rebuild resolves every name, and the
    independently read mesh is the plate with exactly `rounded` (corner,
    radius) pairs rounded; the exact volume is the B-Rep's analytic one."""
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    n = len(tables(copy)["topology_refs"][1])
    text = run(["rebuild", copy, "--cold"])
    assert "tip Fillet" in text and f"{n} of {n} stored references resolved" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--linear-deflection", "0.01", "--json"])
    volume, points = stl(out)
    exact = (W * D - (1 - math.pi / 4) * sum(r * r for _, r in rounded)) * H
    slack = sum(math.pi / 2 * r * 0.01 * H for _, r in rounded)
    assert exact - slack - 1e-3 <= volume <= exact + 1e-3, (volume, exact)
    for c in CORNERS:
        for z in (0.0, H):
            near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in points)
            assert near == all(c != at for at, _ in rounded), (c, z)
    for at, r in rounded:
        # Inside the corner's r x r square the mesh is the arc: every vertex
        # lies at r from its centre, so each radius is measured on its own.
        sx = 1 if at[0] == X0 else -1
        sy = 1 if at[1] == Y0 else -1
        centre = (at[0] + sx * r, at[1] + sy * r)
        arc = [p for p in points if 1e-6 < sx * (p[0] - at[0]) < r - 1e-6
               and 1e-6 < sy * (p[1] - at[1]) < r - 1e-6]
        assert len(arc) >= 6, (at, r, len(arc))
        assert all(abs(math.hypot(p[0] - centre[0], p[1] - centre[1]) - r) < 1e-3 for p in arc), (at, r)
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
create.write_text(json.dumps({"request_version": 1, "points_mm": CORNERS, "height_mm": H}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", create, "-o", plate, "--json"], plate)
R1, R2 = 2.375, 3.0625
done1 = fillet(plate, FIRST, R1, root / "first.fcad")
done2 = fillet(root / "first.fcad", SECOND, R2, root / "twice.fcad")
twice = root / "twice.fcad"

# 2. Discovery: the base row is editable and names both Fillets in history
#    order; Fillet 2 rounds Fillet 1's result, not the base.
catalog = inspect(twice)
assert catalog["edit_extrude"]["available"] is True, catalog["edit_extrude"]
(base,) = catalog["features"]
BASE = base["feature_id"]
assert base["editable"] is True and base["refusal"] is None and base["distance_mm"] == H, base
ctx = base["fillet_base"]
assert ctx["fillet_feature_id"] == done1["feature_id"] and ctx["radius_mm"] == R1, ctx
two = ctx["second_fillet"]
assert two["fillet_feature_id"] == done2["feature_id"] and two["radius_mm"] == R2, two
assert two["previous_feature_id"] == done1["feature_id"] and two["history_index"] == 2, two
assert catalog["sketches"][0]["editable"] is True, "§28J: the base Sketch reads both Fillets"
refs = tables(twice)["topology_refs"]

def raised(source, h, name):
    version = inspect(source)["content_version"]
    out = root / name
    before = source.read_bytes()
    result = run([OP, source, "--feature", BASE, "--distance-mm", h,
                  "--expect-version", version, "-o", out, "--json"])["result"]
    assert result["feature_id"] == BASE, result
    assert source.read_bytes() == before, "the source is untouched"
    allowlist(source, out, BASE)
    assert tables(out)["topology_refs"] == refs, "a name moved"
    (row,) = inspect(out)["features"]
    assert row["distance_mm"] == h and row["fillet_base"] == ctx, row
    return out

# 3. Up, then down: only the base row moves; both Fillets stay, measured.
up = raised(twice, 11.5, "up.fcad")
vu, eu = measured(up, [(FIRST, R1), (SECOND, R2)], 11.5)
down = raised(up, 2.25, "down.fcad")
vd, ed = measured(down, [(FIRST, R1), (SECOND, R2)], 2.25)

# 4. Each radius stays editable on the lowered plate.
request = root / "radius.json"
request.write_text(json.dumps({"request_version": 1, "radius_mm": 4.25}))
f1 = root / "down-f1.fcad"
run([RADIUS, down, "--feature", done1["feature_id"], "--expect-version",
     inspect(down)["content_version"], "--request", request, "-o", f1, "--json"])
v1, e1 = measured(f1, [(FIRST, 4.25), (SECOND, R2)], 2.25)

# 5. Refusals write nothing: Fillet 2 as the feature, a zero height, a stale
#    version, a height the kernel cannot round.
names = sorted(p.name for p in root.iterdir())
never = root / "never.fcad"
for feature, h, version, kind in (
        (done2["feature_id"], 9.0, inspect(down)["content_version"], None),
        (BASE, 0, inspect(down)["content_version"], "input"),
        (BASE, 9.0, catalog["content_version"], "input"),
        (BASE, 1e-6, inspect(down)["content_version"], "kernel")):
    error = run([OP, down, "--feature", feature, "--distance-mm", h, "--expect-version",
                 version, "-o", never, "--json"], 2)["error"]
    assert kind is None or error["kind"] == kind, error
assert not never.exists() and sorted(p.name for p in root.iterdir()) == names
print("FCAD_28I_RECIPE_OK", f"up={vu:.6f}/{eu:.6f}", f"down={vd:.6f}/{ed:.6f}",
      f"f1_after={v1:.6f}/{e1:.6f}", f"fillets={done1['feature_id']},{done2['feature_id']}")
```
