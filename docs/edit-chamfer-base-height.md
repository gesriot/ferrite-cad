# §29B — change the height of a chamfered plate, in a new copy

[Executed verification and limitations](edit-chamfer-base-height-verification.md).
[The Chamfer this keeps](rectangular-corner-chamfer.md);
[the same edit under a Fillet](edit-fillet-base-height.md).

A person opens a saved rectangular plate with the one §29A Chamfer, changes the
plate's Blind height in the existing **Edit extrusion** form, and saves a new
`.fcad`. An agent does the same with the existing `edit-extrude`. The Chamfer
is the same edge at the same distance, the Sketch is the same Sketch; only the
height changed. There is no new command, request format, copier or history
table.

## Contract recorded before implementation

### The supported source

Exactly the §29A class, read again from the saved history on every discovery,
preparation and write by the one reader the distance edit uses
(`chamfer::saved_chamfer`, over `cut_edit::saved_history_under_chamfer`):

* one untransformed XY datum, one Sketch of four Lines forming an axis-aligned
  rectangle that is free or closure-only (the four Coincident links);
* one forward literal Blind `Extrude`/`NewBody` (a formula or parameter height
  is refused as today), no Cut and no Fillet;
* one Chamfer whose `previous` and `edge.feature` are that Extrude, whose joint
  is a corner of that rectangle and whose distance answers to the §29A policy;
* one Body whose tip is the Chamfer, and exactly the plane, profile,
  predecessor and body-tip dependencies;
* exactly the seven names §29A gives the Chamfer, by meaning, and no other
  name it owns.

The selected `--feature` must be that Chamfer's `previous`, the base Extrude;
the existing literal-Blind check (`editable_extrude`) still applies to it. Only
the height becomes editable here.

Everything else keeps refusing a chamfered part **by naming the Chamfer UUID**:
`refuse_chamfered` is not disabled for any other editor — the Sketch
constraints (the Sketch's coordinates became editable in
[§29C](edit-chamfer-base-sketch.md) and its constraints in
[§29D](edit-chamfer-base-constraints.md)), circle and annulus edits, Cut add and edit,
Revolve edits, a Fillet or a second Chamfer, and `chamfer-edge-copy` on a
chamfered plate. `edit-chamfer-distance` is unchanged and works on the copy
this edit publishes.

Refused through the existing typed routes, each with the guilty feature's UUID
and nothing written:

* the Chamfer, the Sketch or any feature other than the base Extrude;
* a document whose Chamfer is outside the class (two Chamfers, a Fillet or a Cut
  beside it, a non-rectangle, a dimension, a foreign or extra name, a formula
  height, ThroughAll/Symmetric/reversed);
* a zero, negative or non-finite height (`input`, as today).

### Height policy — measured first

Probed through the product's own `chamfer-edge-copy` on pinned OCCT 8.0.1: the
37.5 × 12.25 mm plate at (−4.5, 3.25), the corner (33, 3.25), distances 0.001,
2.375 and 12.24 mm (the §29A maximum), heights from 1e-7 to 1e5 mm; then each
built plate exported to STL.

| height (mm) | every distance |
| --- | --- |
| 1.2e-5 … 1e5 | builds; exports; STL volume within tessellation of `(W·D − d²/2)·h` |
| 1e-5 and below | OCCT refuses to chamfer the edge (`kernel`) |

The limit does not depend on `d`, and `d`'s own bound (shorter adjacent side −
0.01 mm) does not depend on `h`. So the domain adds **no height bound of its
own**: the policy stays the existing `edit-extrude` rule, a finite positive
distance, applied by the shared validator. A height OCCT cannot chamfer is
refused by the kernel during the copy's strict rebuild, typed `kernel`, and
nothing is published. Nothing is clamped, and the plate alone (no Chamfer) can
still be made that thin — the refusal is the Chamfer's, so it is reported as the
kernel's.

### Identity — what may change

The only model change is the base Extrude's Blind distance: that row's `payload`
and `payload_hash`, plus the established `meta.modified_at` stamp. Everything
else is preserved byte for byte: the document id, every other object row
(including the Chamfer's payload — UUID, `previous`, edge producer, joint,
distance), every dependency, every topology reference UUID and meaning, object
names/ordinals/parents, the Body's tip and every capability row. Nothing is
minted, deleted or recreated, and no solved geometry or reference is written.
The selected corner and the Chamfer distance are the same; the plane of the
cut, its outward normal and its area (`d·√2·h`) follow the new height.

### Mechanism (all shared)

* **Discovery** comes from the one pinned snapshot `ExtrudeEditSource`; the
  base Extrude's row carries the saved Chamfer as context
  (`ExtrudeChoice::chamfer`), the other Extrudes refuse naming it. Additive
  JSON: `features[].chamfer_base` (`chamfer_feature_id`, `body_id`, `edge`,
  `corner_mm`, `distance_mm`, `distance_unit`), `null` elsewhere. Every
  existing field and type is unchanged.
* **Preparation** `prepare_extrude_height` reads the Chamfer first through
  `saved_chamfer`; the prepared edit carries it and the complete content
  version of the snapshot.
* **Write** `write_extrude_height` re-derives the prepared edit in its own
  transaction (a forged or stale edit fails), then updates one row and the stamp
  exactly as for a Cut history or a Fillet; the legacy standalone writer is not
  used.
* **Job** the existing `edit_object_copy`: pinned snapshot and version guard,
  source/alias/no-clobber, strict cold rebuild that resolves every saved name,
  cancellation and cleanup, atomic publication, exit 7 after publication.
* **Evaluator and cache** unchanged: the Chamfer judges the Lines the rebuild
  built, its cache key includes the predecessor's key, so a new height misses
  for the base and the Chamfer and never returns the old plate.

### UI

The existing **Edit extrusion…** form shows the Chamfer as context ("Chamfered
by Chamfer … at (x, y), d … mm; the Chamfer keeps its edge and distance; only
the plate's height changes."), validates the number through the same shared
rule, and saves through the same worker into a new file that opens
asynchronously. This form has **no** Undo/Redo and none is claimed. A refusal
and **Save Cancel** keep the draft.

### Out of scope, refused

Sketch or constraint edits under a Chamfer (both opened later: §29C, §29D), a second Chamfer, a Fillet or Cut
beside it, changing the Chamfer's edge, ThroughAll, an arbitrary plane, chains,
in-place Save and live preview. Milestone 5C is not complete.

## Agent recipe

Extract the code between the markers and run it with the real command line:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-chamfer-base-height.md").read_text(encoding="utf-8")
code = text.split("# FCAD_29B_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-29b-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-29b-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_29B_RECIPE_NO_KERNEL`; Open CASCADE alone (no solver) runs it to the end,
because the class is free or closure-only. It prints `FCAD_29B_RECIPE_OK` with
the exact and the measured volumes.

```python
# FCAD_29B_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-29b-"))

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def geometry(args, out):
    """A step that needs the kernel: a build without one refuses typed."""
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 2 and not out.exists():
        error = json.loads(p.stdout)["error"]
        if error["kind"] == "unsupported" and "Open CASCADE" in error["message"]:
            print("FCAD_29B_RECIPE_NO_KERNEL", json.dumps(error))
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

def only_row(source, copy, row_id):
    """One object row's payload and hash (and the stamp) may differ; nothing else."""
    rid = bytes.fromhex(row_id.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    moved = 0
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc and len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u != v:
                    moved += 1
                assert u == v or (t == "objects" and c in ("payload", "payload_hash")
                                  and x[ac.index("id")] == rid) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c} moved"
    return moved

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
CORNER = [X0 + W, Y0]                      # the lower right corner of a counter-clockwise plate

def measured(copy, distance, height):
    """After reopening: valid, a cold rebuild resolves every name, the exact
    analytic volume (W*D - d*d/2)*h, and the independently read mesh is closed
    at the chosen corner — the other three corners are whole, the cut's two new
    vertex columns are d along each adjacent side from z=0 to z=h, and the flat
    adds up to d*sqrt(2)*h and faces out of the plate."""
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    n = len(tables(copy)["topology_refs"][1])
    text = run(["rebuild", copy, "--cold"])
    assert "tip Chamfer" in text and f"{n} of {n} stored references resolved" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--json"])
    volume, tri = stl(out)
    exact = (W * D - distance * distance / 2) * height
    assert abs(volume - exact) < 1e-3, (volume, exact)
    flat = 0.0
    for a, b, c in tri:
        if all(abs(-(p[0] - CORNER[0]) + (p[1] - CORNER[1]) - distance) < 1e-4 for p in (a, b, c)):
            u = [b[i] - a[i] for i in range(3)]
            v = [c[i] - a[i] for i in range(3)]
            n_ = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
            length = math.sqrt(sum(x * x for x in n_))
            assert (n_[0] - n_[1]) / length / math.sqrt(2) > 1 - 1e-4, "the flat faces the plate"
            flat += length / 2
    assert abs(flat - distance * math.sqrt(2) * height) < 1e-3, flat
    pts = [p for t in tri for p in t]
    for z in (0.0, height):
        for x, y in ([CORNER[0] - distance, CORNER[1]], [CORNER[0], CORNER[1] + distance]):
            assert any(abs(p[0] - x) < 1e-4 and abs(p[1] - y) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in pts), (x, y, z)
        assert not any(abs(p[0] - CORNER[0]) < 1e-4 and abs(p[1] - CORNER[1]) < 1e-4
                       and abs(p[2] - z) < 1e-4 for p in pts), "the cut corner still has a vertex"
        for c in ([X0, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]):
            assert any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in pts), ("another corner is gone", c, z)
    assert abs(max(p[2] for p in pts) - height) < 1e-4
    return volume

# 1. A plate, drawn offset and fractional, and the one Chamfer of the 29A recipe.
request = root / "plate.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H,
                               "points_mm": [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", request, "-o", plate, "--json"], plate)
catalog = inspect(plate)
candidate = next(c for c in catalog["bodies"][0]["chamfer_edge"]["target"]["candidates"]
                 if c["corner_mm"] == CORNER)
d1 = 2.375
request.write_text(json.dumps({"request_version": 1, "distance_mm": d1, "edge": candidate["edge"]}))
one = root / "one.fcad"
geometry(["chamfer-edge-copy", plate, "--body", catalog["bodies"][0]["body_id"],
          "--expect-version", catalog["content_version"], "--request", request,
          "-o", one, "--json"], one)

# 2. Discovery: the base Extrude carries the Chamfer as context; every other row
#    is null; the older fields keep their types.
saved = inspect(one)
chamfer = saved["chamfers"][0]
base = saved["features"][0]
assert saved["edit_extrude"]["available"] is True
assert base["editable"] is True and base["refusal"] is None and base["distance_mm"] == H
assert base["chamfer_base"] == {"chamfer_feature_id": chamfer["feature_id"],
                                "body_id": saved["bodies"][0]["body_id"], "edge": chamfer["edge"],
                                "corner_mm": CORNER, "distance_mm": d1, "distance_unit": "mm",
                                "profile_constrained": False}
for key in ("base_height_edit", "fillet_base", "fillet_history"):
    assert base[key] is None, key
assert inspect(plate)["features"][0]["chamfer_base"] is None

# 3. Taller and shorter (below the distance too): one row, one cell pair,
#    the Chamfer and every name as they were, the plane at the chosen corner.
refs_before = [r[0] for r in tables(one)["topology_refs"][1]]
copies = {}
for h in (9.5, 0.4, 3.0):
    out = root / f"h{h}.fcad"
    done = geometry(["edit-extrude", one, "--feature", base["feature_id"], "--distance-mm", str(h),
                     "--expect-version", saved["content_version"], "-o", out, "--json"], out)["result"]
    assert done["feature_id"] == base["feature_id"]
    assert only_row(one, out, base["feature_id"]) >= 2
    assert [r[0] for r in tables(out)["topology_refs"][1]] == refs_before, "every name keeps its UUID"
    after = inspect(out)
    assert after["features"][0]["distance_mm"] == h
    assert after["features"][0]["chamfer_base"] == base["chamfer_base"]
    assert after["chamfers"][0]["feature_id"] == chamfer["feature_id"] and after["chamfers"][0]["distance_mm"] == d1
    exact = (W * D - d1 * d1 / 2) * h
    measured(out, d1, h)
    copies[h] = out

# 4. The Chamfer's own distance edit still works on the edited plate.
edited = inspect(copies[9.5])
request.write_text(json.dumps({"request_version": 1, "distance_mm": D - 0.01}))
far = root / "far.fcad"
geometry(["edit-chamfer-distance", copies[9.5], "--feature", chamfer["feature_id"],
          "--expect-version", edited["content_version"], "--request", request, "-o", far, "--json"], far)
measured(far, D - 0.01, 9.5)

# 5. Refusals write nothing: the Chamfer itself, a non-number, and the height
#    OCCT cannot chamfer (measured: 1e-5 mm and below), typed `kernel`.
never = root / "never.fcad"
for feature, height, kind in ((chamfer["feature_id"], "9", "unsupported"), (base["feature_id"], "0", "input"),
                              (base["feature_id"], "1e-6", "kernel")):
    p = run(["edit-extrude", one, "--feature", feature, "--distance-mm", height,
             "--expect-version", saved["content_version"], "-o", never, "--json"], code=2)
    assert p["error"]["kind"] == kind and not never.exists(), (height, p)
(copies[9.5].with_suffix(".stl")).unlink()
print("FCAD_29B_RECIPE_OK", f"d={d1} h=9.5 volume={measured(copies[9.5], d1, 9.5):.6f}/{(W * D - d1 * d1 / 2) * 9.5:.6f}")
```
