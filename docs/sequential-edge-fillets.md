# §28G — a second Fillet on another vertical corner, in a new copy

[Executed verification and limitations](sequential-edge-fillets-verification.md).
[The first Fillet](single-edge-fillet.md);
[on a dimensioned plate](fillet-constrained-plate.md) and
[dimensioned afterwards](fillet-base-constraints.md).

A person opens a plate that already carries one Fillet (§28A, on an
unconstrained plate or one dimensioned with the constraint editor's Line
constraints) and rounds a **different** vertical corner with its own radius,
into a new copy. The history becomes Extrude → Fillet 1 → Fillet 2 → Body
tip: two real features, each built by its own kernel operation on its own
predecessor's result, never one hidden operation that rounds both. The
existing **Fillet edge of …** form and `fillet-edge-copy` are the only
entry points; no command, request version or copy pipeline is added.

## Contract recorded before implementation

### The supported source

The §28B frame (`fillet_radius::fillet_over_plate`): one untransformed XY
datum; one Sketch of four Lines whose stored coordinates are an axis-aligned
rectangle, unconstrained or carrying the constraint editor's managed Line
family; one forward literal Blind `Extrude`/`NewBody`; exactly one Fillet,
whose `previous` and `edge.feature` are that Extrude, which is the Body's tip;
no Cut; exactly the plane, profile, predecessor and body-tip dependencies and
the seven §28A names.

The new Fillet rounds one of the **three other** corners of the same plate,
named as before by the base Extrude and the unordered pair of Line UUIDs that
meet there. Refused, typed, before anything is written: the corner the first
Fillet rounded (`input`), a document that already holds two Fillets
(`unsupported`: a third Fillet is out of scope), and everything §28A refuses.

### The stored model

* **Fillet 2** is a Fillet row with `previous` = Fillet 1 (the result it
  actually rounds) and `edge` = { `feature`: the base Extrude, `joint`: the
  Line pair } — the stable meaning of the corner. The producer is not
  rewritten to Fillet 1 to keep the old `edge.feature == previous` equality.
* That is new stored meaning, so it moves the layout: a Fillet whose
  `edge.feature` is not its `previous` — decided from the payload alone,
  which in this class means one that rounds another Fillet's result — is
  **payload v2** and requires the new capability
  **`feature.fillet.sequential.v1`**; the header must say so, both ways. A
  §28A Fillet stays v1. A
  build that predates this one does not read Fillet v2, keeps the row verbatim
  and opens the document read-only; its rebuild refuses the object rather than
  building part of the history. No SQLite schema change.
* The validator adds one rule: a Fillet's `edge.feature` must be its
  `previous` or an ancestor of it through the predecessor chain
  (`fillet.edge-outside-history`).

### Names

Fillet 2 persists eight references (all `Exact` except the four sides, which
keep §28A's `AllDerivedFrom`):

* `EdgeFilletFace { edge_feature: base, joint: j2 }` — its own cylinder;
* `OriginCap { base, Start|End }`, `OriginSide { base, segment }` × 4 — the
  plate's faces as the final Body has them;
* **new** `OriginFilletFace { origin_feature: Fillet 1, edge_feature: base,
  joint: j1 }` — Fillet 1's cylinder as the final Body has it. Its own role:
  Fillet 1's own `EdgeFilletFace` reference keeps naming Fillet 1's result,
  and this one names the same face in the Body. It requires
  `feature.fillet.sequential.v1`.

Fillet 1's seven names are untouched and keep resolving on Fillet 1's result.

### Topology

`TopologyMap::record_fillet` carries, from the predecessor's names, through
the fillet's own history:

* every face it carried before (caps, sides and their origins), as now;
* **every vertical sweep edge** of the base, as an origin name
  `(base, SweepEdge(joint))`; the rounded one is recorded as removed;
* **the predecessor's own fillet face**, as `(Fillet 1, FilletFace(base, j1))`.

Fillet 2's kernel edge is the edge `(base, SweepEdge(j2))` names on Fillet
1's result — exactly one, found by history. Missing, removed (already rounded),
ambiguous or belonging to another shape is a typed `topology` refusal before
anything is published. No OCCT index, nearest edge or coordinate match is
used, and a handle of the source shape is never taken for one of the result.

Measured on Open CASCADE 8.0.1: `BRepFilletAPI_MakeFillet::IsDeleted` answers
`true` for edges and vertices the result still contains unchanged (7 of 8
edges and 6 of 8 vertices of a box after rounding one vertical edge), and
`false` for kept faces. The bridge's history therefore classifies by
`Modified` first, then membership of the result, and only then as deleted;
the answer for every face is unchanged. It also refuses a reported output
whose shape type differs from the input's.

### Cache

The named archive gains `OriginSweepEdge { origin_feature, joint }` (an edge)
and `OriginFilletFace { origin_feature, edge_feature, joint }` (a face), so a
Fillet restored from the cache carries the same edges and faces a cold build
does, and Fillet 2 built on a restored Fillet 1 picks the same edge. The
archive format moves from v3 to **v4**; a v3 entry is refused and rebuilt, not
read with the new meaning. Fillet 2's key already includes Fillet 1's, which
includes the Extrude's and the Sketch's: changing the Sketch or the first
radius invalidates the suffix. No extra solve or rebuild is added.

### One validator for structure and solved geometry

`evaluable_fillet` checks Fillet 2 structurally (previous is the one Fillet
over the frame above, the same base, a different corner) and geometrically on
the Lines the base was **built** from — the one solve of the rebuild, cold or
cached (§28E/F): the same Line UUIDs in stored order, an axis-aligned
rectangle at the unchanged 1e-7 mm, every Line on its side, both joints
corners, each radius within §28A's bound (`0.01 mm ≤ r ≤ ½` the shorter side
at its corner), and, for **adjacent** corners, the new pair policy:

> the flat left on the Line the two corners share must be at least
> `MIN_RADIUS_MM` (0.01 mm), stated as a bound on the second radius:
> `r2 ≤ L_shared − r1 − 0.01 mm` — the very expression discovery reports, so
> the largest radius it offers is accepted.

Measured on OCCT 8.0.1 on the 37.5 × 12.25 × 6.75 mm plate, second corner
adjacent across the 12.25 mm side: touching (`r1 + r2 = L`, flat 0) is not
built at all; a flat of 1e-7, 1e-5, 1e-4, 1e-3, 5e-3, 0.01 and 0.025 mm builds
a valid 8-face solid whose volume matches `(W·D − (1 − π/4)(r1² + r2²))·h` to
2e-16 relative, with Fillet 1's face kept. The chosen minimum is the smallest
feature this build rounds: a strip narrower than the smallest radius is
treated as touching. Nothing is clamped or snapped. Opposite corners share no
Line and need no further condition. Stored lengths of a constrained plate are
not solved bounds.

### Writer and allowlist

Unchanged pipeline (snapshot, version guard, read-only source, baseline
rebuild, every baseline and minted name resolving, cancellation, no-clobber
atomic publication, exit 7). The writer re-derives the whole preparation
inside its transaction. The SQL allowlist:

* **objects:** one new Fillet row (schema v2); the Body row's `payload` and
  `payload_hash` (the tip). The Sketch (constraints included), the Extrude and
  Fillet 1 rows are byte-identical.
* **deps:** + `Fillet 2 → Fillet 1` (Predecessor), + `Body → Fillet 2`
  (BodyTip), − `Body → Fillet 1` (BodyTip).
* **topology_refs:** Fillet 2's eight new rows; every existing row
  byte-identical.
* **capabilities:** + `feature.fillet.sequential.v1`; the others as they were.
* **meta.modified_at.**

### Discovery and UI (JSON v1, additive)

* `bodies[].fillet_edge` of a Body whose tip is one supported Fillet becomes
  `available`, structurally: `target.previous_feature_id` (the Fillet the new
  one rounds; for a plain plate the base, as before) and `target.fillets`
  (the existing Fillet: feature, edge, radius); `candidates` are the three
  other corners, each with `adjacent_fillet_feature_id` (null for the opposite
  corner). For an unconstrained plate `max_radius_mm` also answers the pair
  policy; for a constrained one it stays `null`. The rebuild decides finally.
* With two Fillets, `fillet_edge.available` is `false` with the reason; the
  radius, height, Sketch-coordinate and constraint editors refused the
  two-Fillet history honestly (their rows said so) and kept working for none
  or one Fillet. The radius editor offers the radius of either Fillet since
  [§28H](edit-sequential-fillet-radii.md), the height editor since
  [§28I](edit-two-fillet-base-height.md), the Sketch-coordinate editor since
  [§28J](edit-two-fillet-base-sketch.md) and the constraint editor since
  [§28K](edit-two-fillet-base-constraints.md).
* The form shows the history (Extrude → Fillet 1 at its corner, r → new
  Fillet), lists only the other corners, marks the adjacent ones with the
  shared-side rule, and refuses at Save what the rebuild refuses, keeping the
  draft.

### Compatibility

The reader on `main` before this change is measured on a copy with two
Fillets: it must refuse the unknown semantics (payload v2 / capability)
without a partial rebuild and without rewriting the file. Every document this
build wrote before still reads and rebuilds.

### Out of scope

A third Fillet, the same corner twice, touching or merged arcs, cap edges,
edge chains, Cut with Fillet, Chamfer, editing a history with two Fillets
(since [§28H](edit-sequential-fillet-radii.md) its radii can be edited),
picking, live preview, in-place Save.

## Recipe: create → first Fillet → second → reopen, measure, export

For a caller driving the CLI with JSON v1. Extract it from this file and run
it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/sequential-edge-fillets.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28G_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28g-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28g-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_28G_RECIPE_NO_KERNEL`. Any build with Open CASCADE — the plate is
unconstrained, so no solver is asked — prints `FCAD_28G_RECIPE_OK` with the
measured mesh and exact volumes.

```python
# FCAD_28G_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28g-"))
SEQUENTIAL = "feature.fillet.sequential.v1"

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
            print("FCAD_28G_RECIPE_NO_KERNEL", json.dumps(error))
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

def second_allowlist(source, copy, body):
    """§28G's allowlist: every source cell survives except the Body row's
    payload/payload_hash and its old tip edge; the copy adds one object, two
    edges, eight names, at most the sequential capability, and stamps
    modified_at. Fillet 1's row is the same bytes."""
    bid = bytes.fromhex(body.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        if t == "objects":
            k = ac.index("id")
            mine = {r[k]: r for r in brows}
            assert len(brows) == len(arows) + 1, "one new object"
            for row in arows:
                for c, u, v in zip(ac, row, mine[row[k]]):
                    assert u == v or (row[k] == bid and c in ("payload", "payload_hash")), \
                        f"objects.{c} moved"
        elif t in ("deps", "topology_refs"):
            kept = [r for r in arows if r in brows]
            lost = [r for r in arows if r not in brows]
            assert all(t == "deps" and bid in r for r in lost), (t, lost)
            assert len(brows) - len(kept) == (2 if t == "deps" else 8), t
        elif t == "capabilities":
            assert set(arows) <= set(brows), "a capability changed"
            assert {r[0] for r in set(brows) - set(arows)} == {SEQUENTIAL}
        elif t == "meta":
            for x, y in zip(arows, brows):
                assert all(u == v or c == "modified_at" for c, u, v in zip(ac, x, y)), "meta"
        else:
            assert arows == brows, t

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

def measured(copy, rounded):
    """After reopening: valid, a cold rebuild resolves every name, and the
    independently read mesh is the plate with exactly `rounded` (corner,
    radius) pairs rounded. The exact volume is the B-Rep's analytic one; the
    mesh is bounded by the chord deflection."""
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
    fbx = run(["export-fbx", copy, "-o", copy.with_suffix(".fbx"), "--json"])["result"]
    assert fbx["complete"] is True, fbx
    return volume, exact

def ask(candidate, radius):
    path = root / "fillet.json"
    path.write_text(json.dumps({"request_version": 1, "edge": {
        "feature_id": candidate["edge"]["feature_id"],
        "joint": candidate["edge"]["joint"][::-1]}, "radius_mm": radius}))
    return path

def fillet(source, catalog, request, out, code=0):
    body = catalog["bodies"][0]["body_id"]
    return run(["fillet-edge-copy", source, "--body", body, "--expect-version",
                catalog["content_version"], "--request", request, "-o", out, "--json"], code)

# 1. A plate.
create = root / "create.json"
create.write_text(json.dumps({"request_version": 1, "points_mm": CORNERS, "height_mm": H}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", create, "-o", plate, "--json"], plate)

# 2. The first Fillet.
catalog = inspect(plate)
at = lambda cat, c: next(x for x in cat["bodies"][0]["fillet_edge"]["target"]["candidates"]
                         if x["stored_corner_mm"] == c)
FIRST, R1 = [X0 + W, Y0], 2.375
first = root / "first.fcad"
done1 = geometry(["fillet-edge-copy", plate, "--body", catalog["bodies"][0]["body_id"],
                  "--expect-version", catalog["content_version"], "--request",
                  ask(at(catalog, FIRST), R1), "-o", first, "--json"], first)["result"]
v1, e1 = measured(first, [(FIRST, R1)])

# 3. Discovery on the rounded plate: its history and the three other corners.
catalog = inspect(first)
edge = catalog["bodies"][0]["fillet_edge"]
assert edge["available"] is True, edge
target = edge["target"]
assert target["previous_feature_id"] == done1["feature_id"], target
(saved,) = target["fillets"]
assert saved["feature_id"] == done1["feature_id"] and saved["radius_mm"] == R1, saved
assert len(target["candidates"]) == 3
SECOND, R2 = [X0 + W, Y0 + D], 3.0625
chosen = at(catalog, SECOND)
assert chosen["adjacent_fillet_feature_id"] == done1["feature_id"], chosen
assert chosen["edge"]["feature_id"] == target["base_feature_id"], "named by the plate"
assert at(catalog, [X0, Y0 + D])["adjacent_fillet_feature_id"] is None

# 4. The second Fillet, into a new copy.
second = root / "second.fcad"
before = first.read_bytes()
done2 = fillet(first, catalog, ask(chosen, R2), second)["result"]
assert first.read_bytes() == before, "the source is untouched"
assert done2["previous_feature_id"] == done1["feature_id"], done2
assert done2["edge"] == chosen["edge"] and done2["corner_mm"] == SECOND, done2
assert sorted(r["role"] for r in done2["references"]).count("origin_fillet_face") == 1
second_allowlist(first, second, catalog["bodies"][0]["body_id"])
v2, e2 = measured(second, [(FIRST, R1), (SECOND, R2)])
after = inspect(second)
assert after["bodies"][0]["fillet_edge"]["available"] is False, "a third Fillet"
# §28H: either radius is editable; nothing else of this history is.
assert all(f["radius_edit"]["available"] is True for f in after["fillets"])

# 5. Refusals write nothing: the same corner, a third Fillet, a stale version.
names = sorted(p.name for p in root.iterdir())
never = root / "never.fcad"
error = fillet(first, catalog, ask({"edge": saved["edge"]}, 1.0), never, 2)["error"]
assert error["kind"] == "input" and "already rounded" in error["message"], error
request = ask(at(catalog, [X0, Y0 + D]), 1.0)
error = run(["fillet-edge-copy", second, "--body", after["bodies"][0]["body_id"],
             "--expect-version", after["content_version"], "--request", request,
             "-o", never, "--json"], 2)["error"]
assert error["kind"] == "unsupported" and "third" in error["message"], error
error = run(["fillet-edge-copy", second, "--body", after["bodies"][0]["body_id"],
             "--expect-version", catalog["content_version"], "--request", request,
             "-o", never, "--json"], 2)["error"]
assert error["kind"] == "input", error
assert not never.exists()
assert sorted(p.name for p in root.iterdir()) == names
print("FCAD_28G_RECIPE_OK", f"first={v1:.6f}/{e1:.6f}", f"second={v2:.6f}/{e2:.6f}",
      f"fillets={done1['feature_id']},{done2['feature_id']}")
```
