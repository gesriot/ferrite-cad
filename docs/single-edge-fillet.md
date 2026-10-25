# §28A — one named Fillet on one vertical edge, in a new copy

[Executed verification and limitations](single-edge-fillet-verification.md).
[History decision](decisions/0004-feature-predecessor.md).

A person or an agent opens a saved rectangular plate. They pick one of its
four vertical edges from a list, enter a radius, and publish a new `.fcad`.
The Body of the copy ends in a real, named Fillet feature. The copy reopens
and rebuilds cold or from the cache to the same rounded solid, and it exports
to STL and FBX.

This is a deliberately bounded first slice, not a general fillet system.

* **UI:** `Fillet edge of <Body> — <UUID>…` opens a form with a list of the
  four corners and a radius field.
* **CLI:**

  ```text
  ferritecad fillet-edge-copy <source.fcad> --body UUID --expect-version HASH \
      --request <request.json> -o <copy.fcad> [--json]
  ```

  The request is `{"request_version":1,"edge":{"feature_id":"…",
  "joint":["<curve UUID>","<curve UUID>"]},"radius_mm":2.5}`.

## Contract recorded before implementation

### The supported source

The class is exactly the §25K/§26A frame, with no Cuts:

* one untransformed XY datum;
* one unconstrained Sketch of four Lines forming an **axis-aligned
  rectangle** (`CutBoundary::rectangle_mm`);
* one forward literal Blind `Extrude`/`NewBody`;
* its one Body;
* exactly the plane, profile and body-tip dependencies.

The reader is the shared `saved_history` reader of the Cut editors, with zero
Cut links. Fractional sizes, translated coordinates, either winding and any
starting segment are accepted, because nothing below depends on them.

Each of these is refused with its reason:

* imported STEP;
* several Bodies;
* a placement;
* a formula height;
* a constrained profile;
* a non-rectangle, a Circle or an annulus;
* a Revolve;
* a Cut history;
* a Body that already ends in a Fillet (a second fillet is a follow-up).

### What is rounded

One **convex vertical edge**: the edge the base Extrude swept along +Z at one
corner of the rectangle. It is named by that producer and the existing
semantic `ExtrudeSweepEdge { joint }` — the unordered pair of the two
adjacent Line UUIDs. The radius is one constant, finite, positive number.

Nothing else is offered:

* no chains, no cap edges, no fillet-all;
* no chamfer, no shell;
* no preview, no mouse picking, no in-place Save;
* no editing of the new Fillet (a follow-up).

### The selection contract

The edge is chosen by its meaning, never by an index, a coordinate, the
nearest geometry, an invented curve UUID or a reference UUID that was never
recorded.

* **Discovery.** Discovery lists the four candidates of the saved plate as
  `{ feature_id, joint: [a, b] }`. The pair is the two real adjacent curve
  UUIDs, in the canonical order `ProfileJoint` stores. Each candidate also
  carries the corner's coordinates, its two adjacent Line lengths and its
  radius limit, as labels. They are not identity.
* **Request.** A request may give the pair in either order; `ProfileJoint`
  makes both one meaning.
* **Refusals.** A pair that is not two adjacent Lines of this profile is
  refused with its reason. So is a feature other than the base Extrude, and a
  meaning that names no single edge.
* **What is persisted.** The Fillet payload stores the typed meaning:
  `edge: { feature, joint }`. Output names are derived during rebuild;
  no output references are persisted.
* **Evaluation.** The evaluator resolves exactly that meaning against the
  predecessor's output. It requires exactly one sweep edge under that joint;
  none or several is a refusal.

### Document model

`feature.fillet` is a new object kind, stored at payload v1:

```text
Fillet {
    previous: ObjectId,        // the feature whose result this rounds
    edge: { feature: ObjectId, joint: ProfileJoint },
    radius_mm: f64,            // finite, > 0
}
```

* **Capabilities.** It requires `core.part.v1`, `feature.predecessor.v1` and
  the new `feature.fillet.v1`.
* **Why a new capability.** A prior-main build does not know
  `feature.fillet`. It preserves the object verbatim and opens the document
  read-only, and its evaluator cannot build the Body's tip. That is a real
  prior-reader incompatibility, and it is why the capability exists; it is
  tested against a real prior-main binary.

History follows ADR 0004, unchanged:

* the Fillet names the feature it consumes (`previous`, with a `Predecessor`
  edge);
* the Body names its tip (`BodyTip` moves from the Extrude to the Fillet);
* ownership stays derived. There is no feature-to-Body edge, no second
  ownership table, and no copier of its own.

The validator's `feature.forked-history`, `body.shared-tip` and
`body.shared-history` rules, and its missing-edge and kind checks, cover the
Fillet's `previous` exactly as they cover an Extrude's. The SQLite schema does
not move.

### Kernel operation

`GeometryKernel::fillet_edge(request { target, edge, radius }, track, ctx)` is
a selected-edge operation, and not a wrapper of the experimental
`OcctKernel::fillet_all` corpus probe. The OCCT bridge gains
`fc_occt_fillet_edge` and `fc_occt_fillet_faces`:

* `BRepFilletAPI_MakeFillet` with exactly the one edge;
* session and shape-ownership checks, and a check that the edge is an EDGE
  sub-shape of the target;
* cancellation before and during the build;
* success requires all of:
  * `IsDone`;
  * exactly one solid;
  * `BRepCheck_Analyzer` valid;
  * a finite, positive removed volume;
  * at least one face `Generated` from the edge, lying in the result.
* the answers for every registered sub-shape of the target (kept, modified,
  deleted) are stored while the algorithm is alive, as for a Cut. They are
  read back through the existing `fc_occt_cut_carried`.

The Rust side validates the result again. The history must speak only about
the target; outputs must belong to the result; the rounded edge must be
reported deleted; exactly one fillet face must be generated from it; and the
removal must be positive. The C ABI change is covered by argument and layout
tests.

### Names

Output names are derived at rebuild time from the feature, operation, saved
profile curves and cap roots, together with complete kernel history and
incidence. Each feature exposes face, edge and vertex names. A name binds a
selection only when it identifies exactly one entity in that feature; an
ambiguous name never selects a neighbour. Names of historical results stay at
their producing feature. No output-name rows are stored in the document.

The CLI reports the kind, digest, entity count and complete derivation for
each name. The window displays the same derivation and digest. See
[ADR 0007](decisions/0007-generative-topological-naming.md) and the
[current file format](file-format.md). Fillet and Chamfer still consume
`SweptEdge { feature, joint }`; their corner policies and refusal messages
are unchanged.

### Cache

The key is `eval.fillet.named`. It includes:

* the predecessor's identity and cache key (content);
* the edge producer and the canonical joint;
* the radius bits;
* the kernel and tolerance.

An upstream change changes the predecessor key and invalidates the fillet.

The archive gains one tag (`TAG_EDGE_FILLET_FACE`), the vocabulary-only route
earlier tags took. An older build refuses the entry as malformed and rebuilds.
The archive format version stays 3, because the layout is unchanged; older
v1/v2 entries stay explicitly invalidated. Cold, Miss and Hit give the same
Body, names and refusals.

### Radius policy (measured, then chosen)

Measured on OCCT 8.0.1 with a 37.5 × 12.25 × 6.75 mm plate at (−4.5, 3.25).
The corner's shorter adjacent side is D = 12.25.

| r | OCCT |
| --- | --- |
| 12.25 (= D) and larger | `IsDone` false |
| 12.2 (0.996 D) | valid; removed volume = (1 − π/4)·r²·H to 3e-15 |
| 0.01 … 12.2 | valid; relative volume error ≤ 5e-10 |
| 0.001 | error 1.3e-7 |
| 1e-5 | error 1.4e-3 |
| 1e-7 | "valid", removes **0** mm³ |

The chosen class leaves wide margins on both sides:

    0.01 mm ≤ r ≤ min(len_a, len_b) / 2

`len_a` and `len_b` are the corner's two adjacent Lines.

* The upper bound keeps at least half of each adjacent side flat, far from the
  degenerate limit. It was not widened to make a bad example pass.
* The lower bound is 10⁵ × the kernel linear tolerance, and keeps volumes
  measurable.

Nothing is clamped. A radius outside the class is refused with the numbers, in
discovery validation, in preparation, in the writer's re-derivation and in the
evaluator. The fillet never changes another corner, and never returns the
unmodified block.

### Measured expectations

Removing one convex corner of a W × D × H block gives:

* volume `W·D·H − (1 − π/4)·r²·H`;
* 7 faces: two trimmed caps, four side planes and one cylindrical fillet face;
* a cylinder of radius r whose axis is parallel to Z and passes through
  `(cx ∓ r, cy ∓ r)`, inward from the chosen corner.

These are measured on the B-Rep (surface type, radius and axis location),
not only on a tessellation.

### Copy operation

One shared jobs route (`fillet_edge_copy`), through the existing
`edit_object_copy`, for UI and CLI alike:

* one snapshot and a version check;
* the source is read-only;
* a no-clobber output, and a refusal of aliasing;
* a baseline cold rebuild;
* the write;
* a rebuild that leaves no newly failing feature;
* the version checked again;
* SQLite closed;
* atomic publication;
* cancellation boundaries throughout.

The writer re-derives the whole prepared value inside its transaction.

The SQL allowlist:

* **objects:** one new Fillet row; the Body row's payload and payload hash
  (the tip). Nothing else.
* **deps:** + `Fillet → base` (Predecessor), + `Body → Fillet` (BodyTip),
  − `Body → base` (BodyTip).
* **capabilities:** `feature.fillet.v1` and `topology.origin-face.v1` (and
  `feature.predecessor.v1`), added only if missing.
* **meta.modified_at.**

Every other cell is byte-identical.

### Clients

* **`inspect --json`**: an additive `bodies[].fillet_edge` per Body, with
  `available`, `refusal`, `document_refusal` and `target`. The target has
  `body_id`, `base_feature_id`, `profile_sketch_id`, `height_mm`,
  `request_versions`, `min_radius_mm`, `max_radius_fraction` and
  `candidates[]`: each with `edge` (`feature_id`, `joint`), `label`,
  `corner_mm`, `adjacent_lengths_mm` and `max_radius_mm`. It is structural
  only: it promises neither native libraries nor successful geometry. It is
  read from the existing pinned snapshot.
* **`fillet-edge-copy`**: JSON v1, strict. It refuses unknown, duplicate and
  escape-duplicate keys, and arrays where objects belong, with `input`.
  Exit 7 means published with the report lost; the file is never retried or
  rolled back.
* **UI**: the same typed candidates and request/job.
  * The form shows the chosen corner (both Line UUIDs, coordinates) and the
    radius.
  * The draft survives a cancelled Save, a worker refusal and a stale reply.
  * Publication goes through the existing async Open.
  * Work runs off the window thread; there is no perpetual redraw.

Existing editors refuse a Body that ends in a Fillet with a reason naming it:
extrude height, sketch coordinates and constraints, Cut add and edit, and the
Revolve editors.

### Compatibility

* Documents without a Fillet are unchanged, byte for byte.
* A prior-main reader opens a filleted copy read-only and refuses writes; its
  source file is untouched.
* Stub builds discover and refuse geometry with a typed reason. An OCCT build
  without PlaneGCS fillets normally: an unconstrained fillet has no solver
  dependency.

## Agent recipe

The recipe below is the whole agent route, with no prior knowledge of the
document:

* create an asymmetric, translated plate with fractional sizes;
* read the four candidates from `inspect --json` and choose one by its label;
* send the pair in the other order, which is the same edge;
* check the SQL allowlist, `validate`, a cold `rebuild`, and the STL and FBX
  exports, measured independently of the product;
* check that the chosen corner, and only it, is rounded;
* check that an excessive radius and a second Fillet are refused and leave
  every file as it was.

Extract it from this file and run it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/single-edge-fillet.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28A_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28a-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28a-recipe.py
```

A build without Open CASCADE stops at the first geometry and prints
`FCAD_28A_RECIPE_NO_KERNEL` with the typed `unsupported` error. A complete run
prints `FCAD_28A_RECIPE_OK` with the measured volume. No sketch solver is
involved.

```python
# FCAD_28A_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile, uuid, time
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28a-"))
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


def new_id():
    raw = bytearray(int(time.time() * 1000).to_bytes(6, "big") + os.urandom(10))
    raw[6] = (raw[6] & 15) | 112
    raw[8] = (raw[8] & 63) | 128
    return str(uuid.UUID(bytes=bytes(raw)))

def edge_at(catalog, corner):
    body = catalog["bodies"][0]
    base = next(f for f in catalog["features"] if f["feature_id"] == body["history"][0])
    sketch = next(s for s in catalog["sketches"] if s["sketch_id"] == base["profile_sketch_id"])
    joint = sorted(c["curve_id"] for c in sketch["curves"] if not c["construction"]
                   and c["geometry"]["kind"] == "line"
                   and corner in [c["geometry"]["start_mm"], c["geometry"]["end_mm"]])
    assert len(joint) == 2, (corner, joint)
    return {"edge": {"feature_id": base["feature_id"], "joint": joint}}

def addition(catalog, kind, edge, value):
    field = "radius" if kind == "fillet" else "distance"
    return {"request_version": 1, "changes": [{"kind": "add_feature", "object": new_id(),
        "body": catalog["bodies"][0]["body_id"], "name": kind.title(),
        "feature": {"kind": kind, "edge": {"feature": edge["feature_id"], "joint": edge["joint"]},
                    field: {"value": value, "unit": "mm"}}}]}

def tip(catalog, kind="fillets"):
    return next(f for f in catalog[kind] if f["feature_id"] == catalog["bodies"][0]["history"][-1])

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def geometry(args, out):
    """A step that needs the kernel: a build without one refuses it typed."""
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 2 and not out.exists():
        error = json.loads(p.stdout)["error"]
        if error["kind"] == "unsupported" and "Open CASCADE" in error["message"]:
            print("FCAD_28A_RECIPE_NO_KERNEL", json.dumps(error))
            sys.exit(0)
    assert p.returncode == 0, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout)

def cells(path):
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    out = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        names = [d[0] for d in cur.description]
        out[t] = [dict(zip(names, row)) for row in cur.fetchall()]
    db.close()
    return out

def allowlist(source, copy, body):
    """Every source cell survives, except the Body's payload/payload_hash and
    its old tip edge; the copy adds one object, two edges and seven names, and
    stamps modified_at. Nothing else moves."""
    bid = bytes.fromhex(body.replace("-", ""))
    a, b = cells(source), cells(copy)
    assert a.keys() == b.keys()
    for t in a:
        def key(row):
            d = dict(row)
            if t == "objects" and d["id"] == bid:
                d.pop("payload"); d.pop("payload_hash")
            if t == "meta":
                d.pop("modified_at", None)
            return repr(sorted(d.items()))
        mine = {key(r) for r in b[t]}
        for row in a[t]:
            old_tip = t == "deps" and bid in row.values()
            assert key(row) in mine or old_tip, f"{t}: a source cell moved"
        added = len(b[t]) - len(a[t])
        expected = {"objects": 1, "deps": 1}.get(t)
        if expected is not None:
            assert added == expected, (t, added)
        else:
            assert added == 0, (t, added)

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

# 1. An asymmetric plate, translated off the origin, with fractional sizes.
X0, Y0, W, D, H = -4.5, 3.25, 37.5, 12.25, 6.75
CORNERS = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]
create = root / "create.json"
create.write_text(json.dumps({"request_version": 1, "points_mm": CORNERS, "height_mm": H}))
source = root / "plate.fcad"
geometry(["create-sketch-extrude", create, "-o", source, "--json"], source)
catalog = inspect(source)
body = catalog["bodies"][0]
corner = [X0 + W, Y0 + D]
chosen = edge_at(catalog, corner)
r = 3.0625
assert r <= D / 2
a, b = chosen["edge"]["joint"]
request = root / "fillet.json"
request.write_text(json.dumps(addition(catalog, "fillet", {"feature_id": chosen["edge"]["feature_id"],
                                        "joint": [b, a]}, r)))
source_bytes = source.read_bytes()
copy = root / "rounded.fcad"
done = geometry([OP, source, "--expect-version",
                 catalog["content_version"], "--request", request, "-o", copy, "--json"], copy)
result = tip(inspect(copy))
assert result["edge"] == chosen["edge"], result
assert result["expression"]["value"] == r
assert source.read_bytes() == source_bytes
allowlist(source, copy, body["body_id"])
assert run(["validate", copy, "--json"])["result"]["valid"] is True
text = run(["rebuild", copy, "--cold"])
assert "tip Fillet" in text and sum(line.lstrip().startswith("face ") for line in text.splitlines()) == 13, text
# 3. Independent measurements: volume, and the rounded corner alone.
out = copy.with_suffix(".stl")
run(["export-stl", copy, "-o", out, "--linear-deflection", "0.01", "--json"])
volume, points = stl(out)
exact = W * D * H - (1 - math.pi / 4) * r * r * H
assert exact - math.pi / 2 * r * 0.01 * H - 1e-3 <= volume <= exact + 1e-3, (volume, exact)
def near(p, q):
    return abs(p[0] - q[0]) < 1e-4 and abs(p[1] - q[1]) < 1e-4
for c in CORNERS:
    assert any(near(p, c) for p in points) == (c != corner), c
fbx = run(["export-fbx", copy, "-o", copy.with_suffix(".fbx"), "--json"])["result"]
assert fbx["complete"] is True, fbx
# 4. Refusals leave every file as it was.
before = sorted(p.name for p in root.iterdir())
never = root / "never.fcad"
request.write_text(json.dumps(addition(catalog, "fillet", chosen["edge"], D / 2 + 0.5)))
refused = run([OP, source, "--expect-version",
               catalog["content_version"], "--request", request, "-o", never, "--json"], 2)
assert refused["error"]["kind"] == "input", refused
assert inspect(copy)["bodies"][0]["history"][-1] == result["feature_id"]
request.write_text(json.dumps(addition(catalog, "fillet", chosen["edge"], 1)))
twice = run([OP, copy, "--expect-version",
             inspect(copy)["content_version"], "--request", request, "-o", never, "--json"], 2)
assert twice["error"]["kind"] == "input" and "already rounded" in twice["error"]["message"], twice
assert sorted(p.name for p in root.iterdir()) == before and not never.exists()
print("FCAD_28A_RECIPE_OK", f"volume={volume:.6f}", f"exact={exact:.6f}", f"r={r}")
```
