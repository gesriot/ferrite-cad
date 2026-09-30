# §28H — the radius of either of two sequential Fillets, in a new copy

[Executed verification and limitations](edit-sequential-fillet-radii-verification.md).
[Two sequential Fillets](sequential-edge-fillets.md);
[the one-Fillet radius edit](edit-fillet-radius.md).

A person opens a plate that §28G rounded twice — Extrude → Fillet 1 →
Fillet 2 → Body tip — and changes the radius of **either** Fillet, named by
its exact UUID, into a new copy. The existing `edit-fillet-radius` command and
**Edit Fillet radius** form are the only entry points: same request v1
(`{"request_version":1,"radius_mm":R}`), envelope, operation, exit codes and
publication guarantees, same shared copy job, writer and evaluator. No
command, request version, copier, solver or geometric route is added.

## Contract recorded before implementation

### The supported source

Exactly the §28G class: one untransformed XY datum, one Sketch whose stored
Lines are an axis-aligned rectangle (unconstrained, or carrying the constraint
editor's managed Line family), one forward literal Blind `Extrude`/`NewBody`,
and exactly two Fillets:

* **Fillet 1**: `previous` = `edge.feature` = the base Extrude (payload v1);
* **Fillet 2**: `previous` = Fillet 1, `edge.feature` = the base Extrude
  (payload v2), at another corner; the Body's tip.

No Cut, no other object; exactly the plane, profile, two predecessor and one
body-tip dependencies; Fillet 1's seven §28A names and Fillet 2's eight §28G
names, by meaning, and no other name owned by either. Anything else is
refused by name, as before.

The one-Fillet path is unchanged: a document with one Fillet goes through the
§28B frame exactly as it did, with the same wire. The guard that the base
height, Sketch and constraint editors apply (exactly one Fillet) is **not**
lifted: with two Fillets they still refuse, now saying that only the radius
edit is offered. (Since [§28I](edit-two-fillet-base-height.md) the base
height, and since [§28J](edit-two-fillet-base-sketch.md) the base Sketch's
coordinates, are edited through the two-Fillet reader; the constraint guard
still holds.)

### What an edit is

Only `radius_mm` of the selected Fillet's payload. Both Fillets keep their
UUIDs, `previous`, edge meaning (base Extrude + two Line UUIDs), payload
version and names; the Body keeps its tip; nothing is minted, squashed,
retargeted or re-created.

### SQL allowlist

* `objects`: the selected Fillet row's `payload` and `payload_hash`;
* `meta.modified_at`.

Nothing else: not the Sketch, not the other Fillet, not `schema_version`,
dependencies, names or capabilities. The writer re-derives the whole
preparation inside its transaction against the document version it was
prepared from; a forged payload (another predecessor, corner, the other
Fillet's radius, another row) is refused.

### One validator; stored and solved

The shared rule is §28A's per-corner bound plus §28G's pair policy, applied
in **history order**: `check_pair(Fillet 1's corner, r1, Fillet 2's corner,
r2)`, i.e. `r2 ≤ L_shared − r1 − 0.01 mm` for adjacent corners, nothing for
opposite ones. An edit of either radius is judged by that same predicate with
the other radius as saved, so raising Fillet 1 is refused exactly when the
rebuild's check of Fillet 2 would refuse it.

* **Unconstrained plate:** preparation applies the whole rule to the stored
  Lines (which are the part). Discovery reports the selected Fillet's exact
  bound: its §28A bound and, beside an adjacent Fillet, the pair bound — for
  Fillet 2 `L − r1 − 0.01` (the check's own expression); for Fillet 1 the
  largest `r1` the same predicate accepts with the saved `r2`, found by
  bounded bisection of ordered nonnegative float bit patterns, so the offered maximum is
  accepted and the next float refused.
* **Constrained plate:** the stored Lines are the solver's starting guess.
  Preparation checks only the value part (finite, ≥ 0.01 mm); discovery shows
  stored numbers labelled as stored and `null` for every bound; the copy's
  rebuild — the one solve it already runs — judges both corners and the pair
  on the solved Lines before anything is published.

The rebuild checks the whole suffix: changing Fillet 1 is judged again at
Fillet 1 and at Fillet 2.

### Cache

Fillet 2's key already includes Fillet 1's. Editing Fillet 2: Extrude and
Fillet 1 Hit, Fillet 2 Miss (built on the restored Fillet 1). Editing Fillet
1: Extrude Hit, both Fillets Miss. No archive format change.

### Discovery and UI (JSON v1, additive)

`fillets[]` rows gain, for a two-Fillet history, `history_index` (1 or 2) and
`radius_edit.neighbour`: the other Fillet's UUID, index, corner, radius and,
when adjacent, the shared Line and its stored length. `radius_edit.available`
becomes `true` for both; `max_radius_mm` is the exact bound above for an
unconstrained plate and `null` for a constrained one. The result of
`edit-fillet-radius` adds `previous_feature_id` and names the selected Fillet.
The form states which Fillet it edits (order, UUID, corner, radius), the other
Fillet's radius and the shared-side rule, keeps its draft on Cancel and
refusal, and shows a worker refusal inside the form as well as in the status
line.

### Out of scope

A third Fillet, editing the base height (since
[§28I](edit-two-fillet-base-height.md) it can be), the Sketch's coordinates
(since [§28J](edit-two-fillet-base-sketch.md)) or constraints of a
two-Fillet history, retargeting a corner, Cut with Fillet, Chamfer, picking, preview,
in-place Save.

## Recipe: inspect → exact UUID → edit Fillet 1 → edit Fillet 2 → cold rebuild, export

For a caller driving the CLI with JSON v1. Extract it from this file and run
it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-sequential-fillet-radii.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28H_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28h-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28h-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_28H_RECIPE_NO_KERNEL`. Any build with Open CASCADE — the plate is
unconstrained, so no solver is asked — prints `FCAD_28H_RECIPE_OK` with the
measured mesh and exact volumes. The Fillets are picked by `history_index`
and then addressed only by their exact UUIDs; the pair bound is checked in
Python with the same float expression, not trusted from the report.

```python
# FCAD_28H_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28h-"))
OP = "edit-fillet-radius"

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
            print("FCAD_28H_RECIPE_NO_KERNEL", json.dumps(error))
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
    """Only the selected Fillet's payload/payload_hash and meta.modified_at
    may differ; every table keeps its rows and every other cell."""
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

def measured(copy, rounded):
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

# 1. A plate rounded twice (§28G): Extrude → Fillet 1 → Fillet 2 → Body tip.
create = root / "create.json"
create.write_text(json.dumps({"request_version": 1, "points_mm": CORNERS, "height_mm": H}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", create, "-o", plate, "--json"], plate)
R1, R2 = 2.375, 3.0625
done1 = fillet(plate, FIRST, R1, root / "first.fcad")
done2 = fillet(root / "first.fcad", SECOND, R2, root / "twice.fcad")
twice = root / "twice.fcad"

# 2. Discovery: both rows editable, in history order, each naming the other.
def rows(path):
    catalog = inspect(path)
    by = {f["history_index"]: f for f in catalog["fillets"]}
    assert sorted(by) == [1, 2], catalog["fillets"]
    return catalog, by
catalog, by = rows(twice)
F1, F2 = by[1]["feature_id"], by[2]["feature_id"]
assert (F1, F2) == (done1["feature_id"], done2["feature_id"])
for me, other, r in ((1, 2, R2), (2, 1, R1)):
    edit = by[me]["radius_edit"]
    assert edit["available"] is True and edit["min_radius_mm"] == 0.01, edit
    n = edit["neighbour"]
    assert n["feature_id"] == by[other]["feature_id"] and n["history_index"] == other, n
    assert n["radius_mm"] == r and n["stored_shared_length_mm"] == D, n
    assert n["shared_line_id"] is not None, n
assert by[1]["radius_edit"]["max_radius_mm"] == D / 2   # §28A's bound; the pair is looser
assert by[2]["radius_edit"]["max_radius_mm"] == D / 2
assert catalog["bodies"][0]["fillet_edge"]["available"] is False, "a third Fillet"
refs = tables(twice)["topology_refs"]
request = root / "radius.json"

def edited(source, feature, r, name, previous, index):
    version = inspect(source)["content_version"]
    request.write_text(json.dumps({"request_version": 1, "radius_mm": r}))
    out = root / name
    before = source.read_bytes()
    result = run([OP, source, "--feature", feature, "--expect-version", version,
                  "--request", request, "-o", out, "--json"])["result"]
    assert result["feature_id"] == feature and result["history_index"] == index, result
    assert result["previous_feature_id"] == (F1 if index == 2 else done1["previous_feature_id"]), result
    assert result["previous_radius_mm"] == previous and result["radius_mm"] == r, result
    assert source.read_bytes() == before, "the source is untouched"
    moved = allowlist(source, out, feature)
    assert tables(out)["topology_refs"] == refs, "a name moved"
    return out, moved

def refused(source, feature, r, words, version=None):
    before = sorted(p.name for p in root.iterdir())
    request.write_text(json.dumps({"request_version": 1, "radius_mm": r}))
    never = root / "never.fcad"
    error = run([OP, source, "--feature", feature, "--expect-version",
                 version or inspect(source)["content_version"], "--request", request,
                 "-o", never, "--json"], 2)["error"]
    assert error["kind"] == "input" and words in error["message"], error
    assert not never.exists() and sorted(p.name for p in root.iterdir()) == before

# 3. Fillet 2 up to 6.12 mm: Fillet 1's row and every name stay.
a, moved = edited(twice, F2, 6.12, "f2-up.fcad", R2, 2)
assert ("objects", "payload") in moved, moved
va, ea = measured(a, [(FIRST, R1), (SECOND, 6.12)])

# 4. Now the pair binds Fillet 1: the largest r1 with 6.12 <= 12.25 - r1 - 0.01.
catalog, by = rows(a)
m = by[1]["radius_edit"]["max_radius_mm"]
assert m < D / 2, m
assert 6.12 <= D - m - 0.01 and not 6.12 <= D - math.nextafter(m, math.inf) - 0.01, m
refused(a, F1, math.nextafter(m, math.inf), "flat")
b, _ = edited(a, F1, m, "f1-bound.fcad", R1, 1)
vb, eb = measured(b, [(FIRST, m), (SECOND, 6.12)])

# 5. Fillet 1 down, then Fillet 2 down: each edit changes its own radius only.
c, _ = edited(b, F1, 1.1875, "f1-down.fcad", m, 1)
d, _ = edited(c, F2, 4.8125, "f2-down.fcad", 6.12, 2)
vd, ed = measured(d, [(FIRST, 1.1875), (SECOND, 4.8125)])
_, by = rows(d)
assert (by[1]["radius_mm"], by[2]["radius_mm"]) == (1.1875, 4.8125)

# 6. Refusals write nothing: past the corner, a stale version, the Body's UUID.
refused(d, F2, D / 2 + 0.25, "")
refused(d, F1, 2.0, "", version=inspect(c)["content_version"])
refused(d, inspect(d)["bodies"][0]["body_id"], 2.0, "")
print("FCAD_28H_RECIPE_OK", f"f2_up={va:.6f}/{ea:.6f}", f"f1_bound={m!r}:{vb:.6f}/{eb:.6f}",
      f"both_down={vd:.6f}/{ed:.6f}", f"fillets={F1},{F2}")
```
