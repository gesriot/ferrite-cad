# §26H — explicit ThroughAll for circular Cuts

[Executed verification and limitations](circular-cut-through-all-verification.md).

## Policy recorded before implementation

Until now a "through" Cut was a Blind Cut whose depth happened to equal the
plate's current thickness. After §26G a thicker base turns it into a pocket,
which is correct for Blind and is exactly why a person who meant *through*
needs a way to say so. §26H adds that intent. It does not widen the supported
class: one unconstrained, axis-aligned XY rectangular NewBody plate on the
untransformed XY datum, forward Blind base, and 1–16 pairwise separated
circular Cut links on that datum along +Z. The 16-link limit, the strict
`WALL_CLEARANCE_MM` wall and disk clearances, the DAG (`Feature.previous` and
`Body.tip`, never feature → Body) and every fork/shared-tip/cycle/lossless
payload check stay exactly as they are. Arbitrary planes, reversed/symmetric
cuts, several Bodies, overlapping tools, live preview, Add/Intersect, feature
deletion and in-place Save remain out of scope.

### Four facts, four owners

| Fact | Owner | Where it lives |
| --- | --- | --- |
| **Stored intent** — Blind with a literal depth, or ThroughAll | the Cut feature's payload | `Extrude.end_condition`: `Blind { distance }` or the existing `EndCondition::ThroughAll` |
| **Computed tool length** of a ThroughAll Cut | the evaluator, at every rebuild | derived from the current base extrusion; never stored |
| **Mode transitions and floor names** | `ferritecad-document` Cut policy | one `floor_transition` rule shared by add, edit, height and base-sketch edits |
| **Compatibility** — who may rewrite such a payload | the payload layout and its capability | Extrude payload v3 + `feature.through-all.v1` |

The document never stores a Blind depth equal to the current height, a large
"magic" depth, a formula, NaN, a side table or a copy of the history in place
of ThroughAll. The typed domain value is
`CutExtent::{Blind { depth_mm }, ThroughAll}`; `CircularCut`, `CircularCutEdit`,
`SavedCutTool` and `SavedCircularCut` carry it, and catalogue, preparation,
writer re-derivation, UI and CLI all read the same value. A stated Blind depth
and the computed ThroughAll reach are different numbers with different names:
`CutExtent::blind_depth_mm()` is `None` for ThroughAll, and
`CutExtent::reach_mm(height)` is what a tool reaches in a plate of that height.

### Evaluation: where the length comes from

The evaluator records, for every NewBody extrusion that runs forward to a Blind
distance, a *reach*: the datum its profile is drawn on and that distance. A Cut
result inherits its predecessor's reach unchanged, because a boolean cut only
removes material: the solid at any link is a subset of the root prism, which
spans exactly `[0, h]` along the shared datum normal. A ThroughAll tool is
therefore an ordinary forward extrusion of its own circle for exactly `h`, the
**current** base height read in this rebuild. It is evaluated only when its
sketch is on the datum that recorded the reach; any other plane, a missing
reach (Symmetric, reversed or ThroughAll root) or a non-positive reach is an
explicit refusal, not an approximation.

Direction and tolerance have one contract: the tool starts on the base XY datum
and runs along its +Z normal, exactly like a Blind tool; its length is `h` with
no epsilon added. A Blind tool whose depth equals the height is the existing,
measured "through" case (a coplanar end cap that OCCT removes and the history
reports as not surviving). A ThroughAll tool is geometrically the same tool at
every height, which is why Blind-through ↔ ThroughAll keeps every name.

### Cache

The tool key is the extrusion key of the tool *request*, which holds the
computed length, so a height change moves it. The Cut key already folds in the
predecessor's key, which moves with the base. In addition the named Cut key
feeds the field `through_all` only for a ThroughAll Cut, so a Blind Cut's key is
unchanged from §26G and a ThroughAll archive can never be served for a Blind
request of the same numbers or vice versa. Cold, Miss and Hit must give the same
geometry and name resolution, including after a height change of the file at
the same path and for an early link of a long history.

### Floors and transitions

One rule decides names for add, edit, height and base-sketch edits:
`leaves_a_floor = Blind && depth < height`; ThroughAll never has a floor at
any height.

* ThroughAll has no floor and never gains one when the base grows.
* Blind-through → ThroughAll and ThroughAll → Blind-through (depth equal to the
  height) keep every saved name: no reference is added or removed; the real
  resolver must resolve all of them after a cold rebuild.
* ThroughAll → Blind pocket adds exactly one own `ExtrudeCap(End)` and one
  `OriginCap(this Cut, End)` at each later producer, through the existing
  `added_floor_references`.
* Blind pocket with saved floor names → ThroughAll is refused **during
  preparation** with the Cut UUID and every protected reference UUID, before
  any identifier is minted. The strict copy-job reference check remains a
  second guard, not the first.
* A height edit keeps ThroughAll through and Blind depths absolute. Every
  remaining Blind tool keeps the §26G depth and floor checks; one invalid tool
  refuses the whole publication.

### Writer, SQL allowlist and compatibility

Add writes the existing two objects, four edges, Body tip move and the names
the numbers give (no floor name for ThroughAll). Edit rewrites only the tool
Sketch and Cut payload/hash (and, for a mode change, the Cut's `schema_version`
column), permitted new refs, `meta.modified_at`, and — only for a transition to
ThroughAll — one `capabilities` row `feature.through-all.v1` if absent. The
writer re-reads the current history inside its transaction and re-derives the
payload, schema/capabilities and added refs from the prepared payload's own
numbers and mode; forged payloads, stale history, duplicate or cross-domain
UUIDs are refused. The existing copy job keeps the snapshot/version/alias
guards, baseline cold rebuild, strict refs (old and new), cancellation,
cleanup, SQLite close, atomic Keep publication and exit 7 semantics (a lost
report does not unpublish a copy).

A Cut storing ThroughAll is Extrude payload **v3** requiring `core.part.v1`,
`feature.predecessor.v1` and `feature.through-all.v1`. Blind Cuts stay v2 and
NewBody extrusions stay v1, byte for byte. Builds up to §26G read Extrude
layouts `[2, 1]` only: they keep a v3 object verbatim and, because its envelope
declares a capability they do not implement, open the document read-only. That
is the refusal that protects the intent; the capability makes the reason
legible. The envelope contract is checked both ways, so a v2 header over a
ThroughAll payload (or v3 over Blind) is refused. A ThroughAll → Blind edit
returns the Cut to v2; the `capabilities` table is an index and keeps its row,
exactly as the constraint editors keep theirs, while access is always decided
from the envelopes. The SQLite schema is not raised.

### CLI request, discovery and response

Request v1 is unchanged for both `cut-circular-copy` and `edit-circular-cut`:
`depth_mm` is required and always means Blind. JSON v1 is not bumped. Request
**v2** is a narrow, versioned addition for these two commands only:

```json
{"request_version":2,"center_mm":[x,y],"radius_mm":r,"extent":{"kind":"blind","depth_mm":d}}
{"request_version":2,"center_mm":[x,y],"radius_mm":r,"extent":{"kind":"through_all"}}
```

`edit-circular-cut` adds `"tool_curve_id"` as in v1. Both versions are strict
(`deny_unknown_fields`, 65536 bytes). A v1 edit request aimed at a saved
ThroughAll Cut is refused during preparation: v1 can only state a Blind depth,
and applying it would silently destroy the intent (`CircularCutEdit.vocabulary
= BlindOnly`). Adding with v1 creates Blind as before.

Discovery keeps every JSON v1 block **exactly** as it was and adds explicitly
versioned blocks beside them. The v1 blocks — `bodies[].cut_edit`,
`features[].circular_cut_edit`, `features[].base_height_edit` and
`sketches[].cut_history` — spell a tool's end as the required number
`depth_mm`, and that type is their contract. A ThroughAll Cut has no Blind
depth, so a v1 block that would have to list one cannot describe the history:
it reports that in the way it was already allowed to. `cut_edit.target` and
`circular_cut_edit.saved` become `null` with `available:false` and a `refusal`
naming the `_v2` block; `base_height_edit` and `cut_history` (already nullable
objects without a reason slot) become `null`. The computed height is never
reported as a Blind depth, and no v1 field changes type. On a document without
ThroughAll every v1 block is byte-for-byte what §26G printed.

The additive blocks `bodies[].cut_edit_v2`, `features[].circular_cut_edit_v2`,
`features[].base_height_edit_v2` and `sketches[].cut_history_v2` have the same
shape as their v1 counterparts except that each tool (`tools`, `existing_cut`,
`neighboring_tool`) and `saved` carry `extent`
(`{"kind":"blind","depth_mm":d}` or `{"kind":"through_all"}`) in place of
`depth_mm`, and `target`/`saved` carry `request_versions` (`[1,2]` for add and
for a Blind Cut, `[2]` for a ThroughAll Cut). They are present for every
document. Responses of both commands gain `extent`; existing fields and exits
0/2/7 are unchanged. The CLI constructs the kernel before the job, as before:
a stub CLI refuses on the kernel after request parsing and before any
document-domain check.

### UI

The existing Cut editor gets an explicit End choice, `Blind depth` or `Through
all`. With Through all the depth field is disabled and its text kept; the form
states that the length follows the part's current height. The choice is part of
the draft numbers, so one Apply is one history step over centre, radius and
end, Undo/Redo restore it exactly, and Save Cancel, worker refusal, stale reply
and failed async Open keep the exact draft through the existing mechanisms.
The tool list and the saved-state line say "through all" instead of a depth.

## Executable agent recipe

Set `FERRITECAD` to a fresh native CLI. Optionally set `FCAD_UFBX_READER` to the
pinned `read_production` reader to also read three small FBX files. The marked
block creates 1-, 2-, 4- and 16-Cut plates with mixed Blind pockets, Blind
through holes and ThroughAll Cuts, reads every UUID/version from JSON, changes
the base height 12 → 14.25 → 13, and edits first/middle/last links through every
mode transition. It checks SQL allowlists, the absence of a floor at the chosen
ThroughAll Cut, an independent STL parse (closed oriented mesh, open holes,
floors, z-range, volume bounds), the protected-floor refusal and the v1
refusal. No human-readable output is parsed for identity or control flow.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/circular-cut-through-all.md").read_text()
code = text.split("# FCAD_26H_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-26h-recipe.py").write_text(code)
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-26h-recipe.py
```

```python
# FCAD_26H_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, tempfile, uuid
cli = os.environ["FERRITECAD"]
reader = os.environ.get("FCAD_UFBX_READER")
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-26h-"))
W, D, H0 = 80.0, 50.0, 12.0
DEFLECTION, ANGULAR = 0.05, 0.1

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def catalog(path):
    return run(["inspect", path, "--json"])["result"]

def blind(d): return {"kind": "blind", "depth_mm": d}
THROUGH = {"kind": "through_all"}

def add(source, dest, center, radius, extent, code=0):
    c = catalog(source)
    req = root / "add.json"
    req.write_text(json.dumps({"request_version": 2, "center_mm": center,
                               "radius_mm": radius, "extent": extent}))
    return run(["cut-circular-copy", source, "--body", c["bodies"][0]["body_id"],
                "--expect-version", c["content_version"], "--request", req,
                "-o", dest, "--json"], code)

def saved_cuts(path):
    c = catalog(path)
    tip = [f["circular_cut_edit_v2"]["saved"] for f in c["features"]
           if f["circular_cut_edit_v2"]["saved"]]
    return c, tip[0]["tools"] if tip else [], {s["feature_id"]: s for s in tip}

def edit(source, dest, feature, center, radius, extent, code=0, version=2):
    c, _, saved = saved_cuts(source)
    s = saved[feature]
    req = root / "edit.json"
    body = {"request_version": version, "tool_curve_id": s["tool_curve_id"],
            "center_mm": center, "radius_mm": radius}
    if version == 1: body["depth_mm"] = extent["depth_mm"]
    else: body["extent"] = extent
    req.write_text(json.dumps(body))
    return run(["edit-circular-cut", source, "--feature", feature, "--expect-version",
                c["content_version"], "--request", req, "-o", dest, "--json"], code)

def height(source, dest, h, code=0):
    c = catalog(source)
    base = [f for f in c["features"] if f["base_height_edit_v2"] is not None][0]
    return run(["edit-extrude", source, "--feature", base["feature_id"],
                "--expect-version", c["content_version"], "--distance-mm", h,
                "-o", dest, "--json"], code)

def sql(path):
    with sqlite3.connect(path) as con:
        out = {}
        for (name,) in con.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name"):
            q = '"' + name.replace('"', '""') + '"'
            try: cur = con.execute(f"SELECT rowid,* FROM {q} ORDER BY rowid")
            except sqlite3.OperationalError: cur = con.execute(f"SELECT * FROM {q} ORDER BY 1,2")
            out[name] = ([x[0] for x in cur.description], cur.fetchall())
        return out

def preserved(a_path, b_path, changed_ids, added_refs, capability=False, schema=False):
    a, b = sql(a_path), sql(b_path)
    assert a.keys() == b.keys()
    ids = [uuid.UUID(i).bytes for i in changed_ids]
    for name, (cols, rows) in a.items():
        now = b[name][1]
        assert cols == b[name][0]
        if name == "topology_refs":
            assert len(now) == len(rows) + added_refs and all(r in now for r in rows)
            continue
        if name == "capabilities" and capability:
            assert all(r in now for r in rows) and len(now) <= len(rows) + 1
            continue
        assert len(rows) == len(now), name
        for old, new in zip(rows, now):
            for i, (x, y) in enumerate(zip(old, new)):
                allowed = (name == "meta" and cols[i] == "modified_at") or (
                    name == "objects" and old[cols.index("id")] in ids
                    and (cols[i] in ("payload", "payload_hash")
                         or (schema and cols[i] == "schema_version")))
                assert x == y or allowed, (name, cols[i])

def measure(path, tools, h):
    stl = path.with_suffix(".stl")
    rep = run(["export-stl", path, "-o", stl, "--linear-deflection", DEFLECTION,
               "--angular-deflection", ANGULAR, "--json"])
    raw = stl.read_bytes()
    n = struct.unpack_from("<I", raw, 80)[0]
    assert len(raw) == 84 + 50 * n and rep["result"]["triangles"] == n
    tris, six = [], 0.0
    for i in range(n):
        t = [list(struct.unpack_from("<fff", raw, 84 + 50 * i + 12 + 12 * k)) for k in range(3)]
        tris.append(t)
        a, b, c = t
        six += (a[0]*(b[1]*c[2]-b[2]*c[1]) + a[1]*(b[2]*c[0]-b[0]*c[2]) + a[2]*(b[0]*c[1]-b[1]*c[0]))
    q = lambda v: tuple(round(x, 4) for x in v)
    directed = {}
    for t in tris:
        p = [q(v) for v in t]
        for k in range(3):
            e = (p[k], p[(k + 1) % 3]); directed[e] = directed.get(e, 0) + 1
    assert all(v == 1 for v in directed.values()), "not one oriented surface"
    assert all((b, a) in directed for (a, b) in directed), "open mesh"
    def covers(t, z, cx, cy):
        if any(abs(v[2] - z) > 1e-4 for v in t): return False
        (x1, y1), (x2, y2), (x3, y3) = [(v[0], v[1]) for v in t]
        d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
        if abs(d) < 1e-12: return False
        a = ((y2 - y3) * (cx - x3) + (x3 - x2) * (cy - y3)) / d
        b = ((y3 - y1) * (cx - x3) + (x1 - x3) * (cy - y3)) / d
        return a >= -1e-9 and b >= -1e-9 and 1 - a - b >= -1e-9
    removed_exact = removed_inscribed = 0.0
    for (cx, cy), r, extent in tools:
        reach = h if extent["kind"] == "through_all" else extent["depth_mm"]
        wall = [t for t in tris if all(abs(math.hypot(v[0]-cx, v[1]-cy) - r) < DEFLECTION + 1e-4 for v in t)
                and max(v[2] for v in t) - min(v[2] for v in t) > 1e-4]
        assert len(wall) >= 12, "no bore wall"
        zs = sorted(v[2] for t in wall for v in t)
        assert abs(zs[0]) < 1e-4 and abs(zs[-1] - reach) < 1e-4, (zs[0], zs[-1], reach)
        if reach < h:
            assert any(covers(t, reach, cx, cy) for t in tris), "pocket floor"
        else:
            assert not any(covers(t, h, cx, cy) for t in tris), "hole is open"
        removed_exact += math.pi * r * r * reach
        removed_inscribed += math.pi * (r - DEFLECTION) ** 2 * reach
    lo = [round(min(v[j] for t in tris for v in t), 4) for j in range(3)]
    hi = [round(max(v[j] for t in tris for v in t), 4) for j in range(3)]
    assert lo == [0, 0, 0] and hi == [W, D, round(h, 4)], (lo, hi)
    volume = six / 6.0
    assert W*D*h - removed_exact - 1e-3 <= volume <= W*D*h - removed_inscribed + 1e-3, volume

create = root / "plate.json"
create.write_text(json.dumps({"request_version": 1, "points_mm": [[0, 0], [W, 0], [W, D], [0, D]],
                              "height_mm": H0}))
plate = root / "plate.fcad"
run(["create-sketch-extrude", create, "-o", plate, "--json"])

def tool_at(i):
    slot = (i * 7) % 16
    center = [10.0 + slot % 4 * 19.0 + 0.125, 7.0 + slot // 4 * 12.0 - 0.375]
    radius = 1.5 + i % 5 * 0.25
    extent = THROUGH if i % 3 == 0 else (blind(H0) if i % 3 == 1 else blind(3.0 + i % 7 + 0.5))
    return center, radius, extent

for count in (1, 2, 4, 16):
    src, tools = plate, []
    for i in range(count):
        dest = root / f"h{count}-{i + 1}.fcad"
        center, radius, extent = tool_at(i)
        out = add(src, dest, center, radius, extent)
        assert out["result"]["extent"] == extent
        tools.append((center, radius, extent)); src = dest
    c, ordered, saved = saved_cuts(src)
    assert [t["extent"] for t in ordered] == [t[2] for t in tools]
    assert "depth_mm" not in ordered[0]
    # A v1 reader of the same catalogue is told, not misled.
    if any(t[2] == THROUGH for t in tools):
        v1 = catalog(src)
        assert v1["bodies"][0]["cut_edit"]["target"] is None
        assert "cut_edit_v2" in v1["bodies"][0]["cut_edit"]["refusal"]
    measure(src, tools, H0)
    # Height grows: ThroughAll stays through, Blind(12) becomes a pocket.
    grown = root / f"h{count}-grown.fcad"
    height(src, grown, 14.25)
    measure(grown, tools, 14.25)
    _, after, saved_after = saved_cuts(grown)
    for (center, radius, extent), t in zip(tools, after):
        s = saved_after[t["feature_id"]]
        assert s["leaves_a_floor"] == (extent["kind"] == "blind"), (extent, s["leaves_a_floor"])
        if extent["kind"] == "through_all":
            assert s["floor_reference_id"] is None and s["request_versions"] == [2]
    shrunk = root / f"h{count}-shrunk.fcad"
    height(grown, shrunk, 13.0)
    measure(shrunk, tools, 13.0)
    run(["validate", shrunk, "--json"])
    run(["rebuild", "--cold", shrunk])
    # Edit first/middle/last through the mode transitions, on the grown copy.
    for index in sorted({0, count // 2, count - 1}):
        feature = after[index]["feature_id"]
        center, radius, extent = tools[index]
        moved = [center[0] + 0.25, center[1] - 0.125]
        if extent["kind"] == "through_all":
            # ThroughAll -> Blind pocket adds one own floor + one origin per later link.
            dest = root / f"e{count}-{index}-pocket.fcad"
            edit(grown, dest, feature, moved, radius + 0.125, blind(2.75))
            preserved(grown, dest, [feature, after[index]["tool_sketch_id"]],
                      count - index, schema=True)
            changed = list(tools); changed[index] = (moved, radius + 0.125, blind(2.75))
            measure(dest, changed, 14.25)
            # v1 aimed at ThroughAll is refused: it cannot state the intent.
            never = root / "never.fcad"
            edit(grown, never, feature, center, radius, blind(4.0), code=2, version=1)
            assert not never.exists()
        else:
            dest = root / f"e{count}-{index}-through.fcad"
            if saved_after[feature]["protected_floor_reference_ids"]:
                never = root / "never.fcad"
                err = edit(grown, never, feature, center, radius, THROUGH, code=2)
                msg = err["error"]["message"]
                assert feature in msg and all(r in msg for r in saved_after[feature]["protected_floor_reference_ids"])
                assert not never.exists()
                continue
    # Blind-through (created at 12 mm, before growth) -> ThroughAll keeps every name.
    for index, (center, radius, extent) in enumerate(tools):
        if extent == blind(H0):
            feature = ordered[index]["feature_id"]
            dest = root / f"e{count}-{index}-intent.fcad"
            out = edit(src, dest, feature, center, radius, THROUGH)
            assert out["result"]["extent"] == THROUGH and out["result"]["leaves_a_floor"] is False
            preserved(src, dest, [feature, ordered[index]["tool_sketch_id"]], 0,
                      capability=True, schema=True)
            grown2 = root / f"e{count}-{index}-intent-grown.fcad"
            height(dest, grown2, 14.25)
            changed = list(tools); changed[index] = (center, radius, THROUGH)
            measure(grown2, changed, 14.25)
            back = root / f"e{count}-{index}-back.fcad"
            edit(dest, back, feature, center, radius, blind(H0))
            preserved(dest, back, [feature, ordered[index]["tool_sketch_id"]], 0, schema=True)
            if reader and count in (2, 4):
                fbx = grown2.with_suffix(".fbx")
                run(["export-fbx", grown2, "-o", fbx, "--json"])
                p = subprocess.run([reader, "--identity", str(fbx)], capture_output=True, text=True)
                assert p.returncode == 0 and "failures=0" in p.stdout, (p.stdout, p.stderr)
            break
    print("FCAD_26H_COUNT_OK", count)
print("FCAD_26H_RECIPE_OK", root)
```
