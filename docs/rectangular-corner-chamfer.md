# §29A — one equal-distance Chamfer on one vertical edge of a rectangular plate, in a new copy

[Executed verification and limitations](rectangular-corner-chamfer-verification.md).
[History decision](decisions/0004-feature-predecessor.md); the stack it parallels is
[§28A](single-edge-fillet.md) (create) and [§28B](edit-fillet-radius.md) (edit the one number).

A person or an agent opens a saved rectangular plate, picks one of its four
vertical edges and a distance, and publishes a new `.fcad` whose Body ends in a
real `feature.chamfer`. The copy reopens, rebuilds cold or from the cache,
exports to STL/FBX, and the distance is edited in another new copy. This is one
feature and one number, not a chamfer system.

## Commands

Existing names are `fillet-edge-copy` (create) and `edit-fillet-radius`,
`edit-revolve-angle`, `edit-extrude` (edit one named parameter of a feature).
The Chamfer follows them:

```text
ferritecad chamfer-edge-copy <source.fcad> --body UUID --expect-version HASH \
    --request <request.json> -o <copy.fcad> [--json]
ferritecad edit-chamfer-distance <source.fcad> --feature UUID --expect-version HASH \
    --request <request.json> -o <copy.fcad> [--json]
```

`edit-chamfer-distance` (not `edit-chamfer-copy`) because the existing edit
commands are named for the parameter they change, and a Chamfer has exactly
one. Requests (strict: unknown fields, arrays, extra keys, non-finite, zero or
negative numbers, malformed UUIDs are refused; nothing is parsed from prose):

```json
{"request_version":1,"edge":{"feature_id":"…","joint":["<Line UUID>","<Line UUID>"]},"distance_mm":2.5}
{"request_version":1,"distance_mm":3.0}
```

Unit: **millimetres**. `distance_mm` is the distance from the edge **measured
along each of the two adjacent faces** (equal on both). It is *not* the width
of the slanted flat, which is `distance_mm * sqrt(2)`.

Exit codes and the `{ok,…}`/`{ok:false,error}` envelope are JSON v1 as for every
copy command: 0 published, 2 refused (nothing written), 7 published but the
report was lost.

## The class

* one untransformed XY datum; one Sketch of four Lines forming an
  axis-aligned rectangle; **free or closure-only** (no constraint, or only the
  four Coincident closure links); a Sketch with any other constraint is refused
  with the constraint's UUID and the kind;
* one forward literal Blind `Extrude`/`NewBody`, one Body, the plane/profile/tip
  dependencies and nothing else;
* **no earlier Cut, Fillet or Chamfer**. Exactly one Chamfer, on one of the four
  vertical edges. A second Chamfer, a Chamfer on a rounded Body, a Fillet on a
  chamfered Body, a Cut on it, a cap/arbitrary edge, unequal distances, an angle,
  in-place Save and live preview are refused by name with their reason.

Fractional sizes, translated (offset) rectangles, either winding and any
starting Line are accepted; nothing below depends on them.

## Architecture (ADR 0004, unchanged)

`Chamfer.previous` names the Extrude whose result it modifies
(`Predecessor` edge Chamfer → Extrude); the Body names its tip (`BodyTip` moves
Extrude → Chamfer); ownership is derived. No feature→Body edge, no second
table, no cycle. The validator's `forked-history`, `shared-tip`, `shared-history`
rules cover it as they cover a Fillet.

The edge is the base Extrude's vertical sweep edge at one corner, named by its
producer and the unordered `ProfileJoint` of the two adjacent Line UUIDs — the
same meaning `ExtrudeSweepEdge` carries. No OCCT index, nearest edge, or
coordinate selects it.

### Document model

New kind `feature.chamfer`, payload **v1** (it did not exist before, so v1 is its
first layout):

```text
Chamfer { previous: ObjectId, edge: SweptEdge { feature, joint }, distance_mm: f64 }
```

`SweptEdge` is the neutral name of the struct that was `FilletEdge`; its fields,
`deny_unknown_fields` and therefore the persisted and wire form of a Fillet are
unchanged (`FilletEdge` stays as an alias). `edge.feature == previous` is
required: this slice chamfers the Extrude's own edge.

* **Capabilities:** the object requires `core.part.v1`, `feature.predecessor.v1`
  and the new `feature.chamfer.v1`. The new role `EdgeChamferFace` requires
  `feature.chamfer.v1`; the origin roles keep `topology.origin-face.v1`.
* **Why a new capability and kind:** a `main` build does not know
  `feature.chamfer`. It preserves the object verbatim and opens the document
  **read-only**; it cannot build the Body whose tip is a Chamfer, so it never
  shows a partial plate. Run against a real `main` binary (see the verification
  record). The SQLite schema does not move; the archive format version stays 4
  (one appended tag, below).

## Kernel operation

`GeometryKernel::chamfer_edge(ChamferRequest{target, edge, distance}, track, ctx)
-> ChamferResult`, default `Unsupported` (a test double never answers with an
invented solid). The OCCT bridge gains `fc_occt_chamfer_edge` and
`fc_occt_chamfer_faces`: `BRepFilletAPI_MakeChamfer::Add(distance, edge)` — the
**symmetric** form, which takes no reference face, so equal-distance cannot
depend on any arbitrary face or walk order (measured identical to
`Add(d, d, edge, face)` for either face of the edge). Success requires `IsDone`,
exactly one solid, `BRepCheck_Analyzer` valid, a positive removed volume, and one
face `Generated` from the edge that lies in the result. The answers for every
registered sub-shape of the target (kept, modified, deleted) are read while the
builder is alive, exactly as for a Cut or a Fillet, and the Rust side validates
them again. The two operations share the shim's post-build code; the Fillet's
behaviour does not change (its tests are the proof).

## The distance policy (measured on OCCT 8.0.1, then chosen)

37.5 × 12.25 × 6.75 mm plate, edge at (33, 3.25) (shorter adjacent side
12.25 mm). Both forms (`Add(d,E)` and `Add(d,d,E,F)` for either face) agree.

| d (mm) | OCCT |
| --- | --- |
| 1e-9 | `IsDone` false |
| 1e-7 … 12.249 | valid, 7 faces, volume `(W·D − d²/2)·H` to 1.7e-16 relative, flat area `d·√2·H` |
| 12.25 (= the side) and larger | `IsDone` false |

Chosen constants (own, not the Fillet's `MIN_RADIUS_MM`/`MAX_RADIUS_FRACTION`):

    MIN_DISTANCE_MM = 0.001 ≤ d ≤ min(len_a, len_b) − MIN_FLAT_MM,   MIN_FLAT_MM = 0.01

* **Finite**, positive, and at least `MIN_DISTANCE_MM`: 10⁴ × the kernel linear
  tolerance, four orders above the smallest distance OCCT still builds
  (1e-7), a flat 1.4 µm wide, hundreds of f32 ulps in the STL.
* **At most the shorter adjacent side less `MIN_FLAT_MM`**: a chamfer of `d`
  leaves `len − d` of each adjacent face; OCCT refuses at `d = len`, and a face
  of a few µm would be a sliver a person did not intend. 0.01 mm is the smallest
  feature this build keeps (the Fillet policy's flat).
* The bound is **one expression**, `ChamferCorner::max_distance_mm`, used by
  discovery (the offered max), preparation, the writer's re-derivation and the
  evaluator. The offered max is accepted and `max.next_up()` is refused with the
  numbers. Nothing is clamped, no tolerance is widened.
* A corner whose bound is below `MIN_DISTANCE_MM` is listed with that fact and
  refused.

A distance outside the class never changes another corner and never returns the
unmodified block.

## What is measured (not only mesh)

Removing the corner prism of a W × D × H block gives:

* B-Rep volume `(W·D − d²/2)·H`;
* 7 faces: two trimmed caps, four sides (two trimmed, two kept) and **one planar
  slanted face** whose area is `d·√2·H`, whose outward normal is the diagonal
  `(±1, ±1, 0)/√2` of **the chosen corner** (not another), and which passes
  through the points `d` along each adjacent edge from the corner;
* the chamfer face is bound to exactly the two Line UUIDs of the joint.

A plane at another corner with the same volume is a failure, by the normal and
the points.

## Names

* Existing references keep their producer (the base Extrude's historical
  output); they are never repointed.
* The Chamfer's own references, all produced by the Chamfer:
  * `EdgeChamferFace { edge_feature: base, joint }` — **new role**, the one
    planar face made from that edge (`Face`, `Exact`). Not `EdgeFilletFace`:
    one is a cylinder, the other a plane, and the roles do not resolve
    against each other.
  * `OriginCap { base, Start|End }` ×2, `OriginSide { base, segment }` ×4 —
    through the Chamfer's own OCCT history: the two sides at the corner and both
    caps are trimmed (`Modified`), the other two sides are kept. A name the
    operation removed is recorded removed, never matched by geometry.
    Seven references in all.
* Edges and vertices of the base remain historical at the base producer; the
  Chamfer tip publishes no edge or vertex names in this slice.

## Cache and archive

Cache key `eval.chamfer.named`: the predecessor's identity and key (content),
the edge producer and canonical joint, the distance bits, the kernel and the
tolerance. Changing `d` invalidates the Chamfer; an upstream change changes the
predecessor key and cannot give a stale Hit. The topology archive gains one
appended tag (`TAG_EDGE_CHAMFER_FACE = 21`), in the same vocabulary-only way
`TAG_EDGE_FILLET_FACE` arrived: format version 4 is unchanged because the layout
is, and a build that does not know the tag treats the entry as malformed and
rebuilds. Cold, Miss and Hit give the same Body, names and refusals.

## SQL allowlists

**Create** (`chamfer-edge-copy`):

* `objects`: one new Chamfer row; the Body row's payload and payload hash (its tip);
* `deps`: + `Chamfer → Extrude` (Predecessor), + `Body → Chamfer` (BodyTip),
  − `Body → Extrude` (BodyTip);
* `topology_refs`: + 7 rows (the names above);
* `capabilities`: `feature.chamfer.v1`, `feature.predecessor.v1` and
  `topology.origin-face.v1` if not yet declared;
* `meta.modified_at`.

**Edit** (`edit-chamfer-distance`): the one Chamfer row's payload and payload
hash, and `meta.modified_at`. Its UUID, every reference UUID and every other
cell are unchanged.

Every other SQL cell and identity is byte-identical, and the source file is
read-only throughout.

## Copy operation

One jobs route for creation (`chamfer_edge_copy`) and one for the edit
(`edit_chamfer_distance`), both through the existing `edit_object_copy`: one
snapshot and a version guard; the source read-only; no-clobber and alias
refusal; a baseline cold rebuild; the write; a strict rebuild in which every
baseline reference and every minted one must resolve; the version checked again;
SQLite closed; atomic publication; cancellation boundaries; exit 7 after a
completed publication. The writer re-derives the whole prepared value inside
its transaction. There is no second copier, no second file read for the DTO and
no direct write from the UI.

## The other editors

A chamfered Body is outside every earlier frame. The shared refusal
(`refuse_filleted`, which every reader of the plate calls first) names the
Chamfer by UUID; so do the Sketch coordinate, constraint, Cut, Fillet,
Revolve and radius routes. A Chamfer is never silently dropped, ignored or
rebuilt on a changed plate by an editor that does not know it. Discovery reports
the saved Chamfer and which of these are unavailable, with the reason. (§29B
since made one exception: the plate's height is edited through `edit-extrude`,
with the Chamfer kept — [the contract](edit-chamfer-base-height.md); §29C then
made the base Sketch's coordinates a second one,
[the contract](edit-chamfer-base-sketch.md); every other editor still refuses.)

## Discovery (additive JSON)

`bodies[].chamfer_edge` — `{available, target:{previous_feature_id, height_mm,
min_distance_mm, min_flat_mm, constrained:false, candidates:[{edge:{feature_id,
joint:[a,b]}, corner_mm, adjacent_lengths_mm, max_distance_mm}]}, refusal}` — the
four corners, labels not identity — and top-level `chamfers[]` — `{feature_id,
body_id, previous_feature_id, edge, corner_mm, distance_mm, max_distance_mm,
editable, refusal, request_versions:[1]}`. Existing fields, types and codes are
unchanged; a `main` build that does not know them ignores them. Without a kernel
discovery is the same and creation refuses (`unsupported`, exit 2) and writes
nothing; Open CASCADE without the solver works for this class.

## UI

`Chamfer edge of <Body> — <UUID>…` and `Edit Chamfer distance <feature> — <name> (d… mm)…`
open bounded forms ("Chamfer one vertical edge — new copy", "Edit Chamfer distance —
new copy") with the same worker as the CLI: the four corners as buttons
(`Corner (x, y) — Lines a | b; sides L1 × L2 mm; d ≤ max mm`), a distance field
in mm, **Apply** / **Save…** / **Cancel … draft**. Each form keeps a whole-request
history (`Undo request` / `Redo request`, one entry per applied request, in the
form itself): this is implemented and tested here, and it is *not* inherited from
the Fillet forms, which have none. The draft survives a refusal and **Save
Cancel**; a published copy opens asynchronously. No write goes from the UI
directly to a file.

## Recipe

Extract the code between the markers and run it with the real command line:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/rectangular-corner-chamfer.md").read_text(encoding="utf-8")
code = text.split("# FCAD_29A_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-29a-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-29a-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_29A_RECIPE_NO_KERNEL`; Open CASCADE alone (no solver) runs it to the end,
because the class is free or closure-only. It prints `FCAD_29A_RECIPE_OK` with
the exact and the measured volumes.

```python
# FCAD_29A_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-29a-"))

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
            print("FCAD_29A_RECIPE_NO_KERNEL", json.dumps(error))
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
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc and len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                assert u == v or (t == "objects" and c in ("payload", "payload_hash")
                                  and x[ac.index("id")] == rid) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c} moved"

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

def measured(copy, distance):
    """After reopening: valid, a cold rebuild resolves every name, the exact
    analytic volume (W*D - d*d/2)*h, and the independently read mesh has the
    corner cut by one planar face facing out of it whose triangles add up to
    d*sqrt(2)*h, with the two new vertex columns d along each adjacent side."""
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    n = len(tables(copy)["topology_refs"][1])
    text = run(["rebuild", copy, "--cold"])
    assert "tip Chamfer" in text and f"{n} of {n} stored references resolved" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--json"])
    volume, tri = stl(out)
    exact = (W * D - distance * distance / 2) * H
    assert abs(volume - exact) < 1e-3, (volume, exact)
    flat = 0.0
    for a, b, c in tri:
        # The plane of the cut at the lower right corner: (x - cx) - (y - cy) = -d.
        if all(abs(-(p[0] - CORNER[0]) + (p[1] - CORNER[1]) - distance) < 1e-4 for p in (a, b, c)):
            u = [b[i] - a[i] for i in range(3)]
            v = [c[i] - a[i] for i in range(3)]
            n_ = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
            length = math.sqrt(sum(x * x for x in n_))
            assert (n_[0] - n_[1]) / length / math.sqrt(2) > 1 - 1e-4, "the flat faces the plate"
            flat += length / 2
    assert abs(flat - distance * math.sqrt(2) * H) < 1e-3, flat
    pts = [p for t in tri for p in t]
    for z in (0.0, H):
        for x, y in ([CORNER[0] - distance, CORNER[1]], [CORNER[0], CORNER[1] + distance]):
            assert any(abs(p[0] - x) < 1e-4 and abs(p[1] - y) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in pts), (x, y, z)
        assert not any(abs(p[0] - CORNER[0]) < 1e-4 and abs(p[1] - CORNER[1]) < 1e-4
                       and abs(p[2] - z) < 1e-4 for p in pts), "the cut corner still has a vertex"
    return volume

# 1. A plate, drawn offset and fractional.
request = root / "plate.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H,
                               "points_mm": [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", request, "-o", plate, "--json"], plate)

# 2. Discovery: four candidates, each by the two Line UUIDs of its corner, and the
#    bounds the policy states — never a guess.
catalog = inspect(plate)
row = catalog["bodies"][0]["chamfer_edge"]
assert row["available"] is True and catalog["chamfers"] == []
target = row["target"]
assert target["distance_unit"] == "mm" and target["min_distance_mm"] == 0.001
assert target["min_flat_mm"] == 0.01 and len(target["candidates"]) == 4
candidate = next(c for c in target["candidates"] if c["corner_mm"] == CORNER)
assert candidate["max_distance_mm"] == D - 0.01 and candidate["offerable"] is True

# 3. Create. The pair may be written in either order; it is one edge.
d1 = 2.375
edge = candidate["edge"]
request.write_text(json.dumps({"request_version": 1, "distance_mm": d1,
                               "edge": {"feature_id": edge["feature_id"],
                                        "joint": [edge["joint"][1], edge["joint"][0]]}}))
one = root / "one.fcad"
body, version = catalog["bodies"][0]["body_id"], catalog["content_version"]
done = geometry(["chamfer-edge-copy", plate, "--body", body, "--expect-version", version,
                 "--request", request, "-o", one, "--json"], one)["result"]
assert done["body_id"] == body and done["distance_mm"] == d1 and done["distance_unit"] == "mm"
assert done["edge"] == edge and done["corner_mm"] == CORNER
roles = sorted(r["role"] for r in done["references"])
assert roles == ["edge_chamfer_face"] + ["origin_cap"] * 2 + ["origin_side"] * 4, roles
exact_one = measured(one, d1)

# 4. The saved Chamfer, and every other editor's honest refusal.
saved = inspect(one)
chamfer = saved["chamfers"][0]
assert chamfer["feature_id"] == done["feature_id"] and chamfer["distance_mm"] == d1
assert chamfer["distance_edit"]["available"] is True
assert chamfer["distance_edit"]["max_distance_mm"] == D - 0.01
assert saved["bodies"][0]["chamfer_edge"]["available"] is False, "a second Chamfer"
# §29B: the plate's height is edited through the base Extrude, with the Chamfer as context.
assert saved["edit_extrude"]["available"] is True
assert saved["features"][0]["chamfer_base"]["chamfer_feature_id"] == chamfer["feature_id"]
assert saved["bodies"][0]["fillet_edge"]["available"] is False
request.write_text(json.dumps({"request_version": 1, "radius_mm": 1.0, "edge": edge}))
never = root / "never.fcad"
for args in (["edit-extrude", one, "--feature", chamfer["feature_id"], "--distance-mm", "9",
              "--expect-version", saved["content_version"], "-o", never, "--json"],):
    p = run(args, code=2)
    assert chamfer["feature_id"] in p["error"]["message"] and not never.exists()

# 5. The distance is edited in another copy: one row, one cell pair.
version = saved["content_version"]
for d2 in (4.5, D - 0.01):
    request.write_text(json.dumps({"request_version": 1, "distance_mm": d2}))
    two = root / f"two-{d2}.fcad"
    edited = geometry(["edit-chamfer-distance", one, "--feature", chamfer["feature_id"],
                       "--expect-version", version, "--request", request, "-o", two, "--json"], two)["result"]
    assert edited["previous_distance_mm"] == d1 and edited["distance_mm"] == d2
    assert edited["feature_id"] == chamfer["feature_id"]
    only_row(one, two, chamfer["feature_id"])
    assert [r[0] for r in tables(two)["topology_refs"][1]] == [r[0] for r in tables(one)["topology_refs"][1]], \
        "every name keeps its UUID"
    measured(two, d2)

# 6. The bound is exact: the largest distance was accepted above, the next
#    representable value is refused with nothing written.
request.write_text(json.dumps({"request_version": 1, "distance_mm": math.nextafter(D - 0.01, 1e9)}))
refusal = run(["edit-chamfer-distance", one, "--feature", chamfer["feature_id"],
               "--expect-version", version, "--request", request, "-o", never, "--json"], code=2)
assert refusal["error"]["kind"] == "input" and not never.exists()
print("FCAD_29A_RECIPE_OK", f"d={d1} volume={exact_one:.6f}/{(W * D - d1 * d1 / 2) * H:.6f}")
```
