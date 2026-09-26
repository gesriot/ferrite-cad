# §27C — solid full-turn Revolve: a profile closed on the axis

[Executed verification and limitations](axis-closed-revolve-verification.md).

A person creates an ordinary solid cylinder, cone or stepped shaft in the
same sketch window, or through the existing `create-sketch-revolve`. The
profile's saved coordinates are then edited through the existing Edit Sketch
/ `edit-sketch-copy`, with the same guards.

Until now (§27A/B) every vertex needed X > 1e-6 mm, so only parts with a
bore could be made. This slice widens exactly that limit. It adds no new
family of primitives, no builder of its own, and no fictitious small bore.

## Contract recorded before implementation

### Measured first (OCCT 8.0.1, pinned)

A C++ probe against the installed kernel revolved three profiles once about
the Y axis through the origin: the cylinder [(0,0),(10,0),(10,15),(0,15)],
the cone [(0,0),(10,0),(0,15)] and the stepped shaft
[(0,0),(10,0),(10,5),(6,5),(6,15),(0,15)]. Each came out as one valid solid:

| Profile | Volume | Faces |
|---|---|---|
| Cylinder | exactly 1500π | Plane, Cylinder, Plane |
| Cone | exactly 500π | Plane, Cone (no face at the apex) |
| Stepped shaft | exactly 860π | Plane, Cylinder, Plane, Cylinder, Plane |

What the history reported:
* **The Line on the axis raises nothing.** `MakeRevol::Generated(edge)` is
  empty, `IsDeleted` is false, and `BRepSweep_Revol::Shape(edge)` is null.
* **Every other Line raises exactly one face of the solid**, radial Lines
  included (unlike the hollow case, where `Generated` is empty for them).
* **Full coverage.** The faces of the non-axis Lines cover the solid exactly.

### The accepted classes

`FullTurnRevolution` keeps one policy with two named classes. The shared
simple-polygon rules — finite values, |coordinate| ≤ 1e6 mm, 3..256
vertices, closure, no self-touching, nonzero area — apply to both.

* **`RadialClear`, unchanged from §27A.** Every vertex has
  X > `AXIS_CLEARANCE_MM` (1e-6 mm). This is a part with a bore.
* **`AxisClosed`, new.** Every vertex has X ≥ 0, and exactly two vertices
  have X exactly 0. Those two must be the ends of one Line: the **axis
  Line**. Every other vertex has X > `AXIS_CLEARANCE_MM`.
  - The axis Line has nonzero length, because the shared rules forbid
    repeated vertices.
  - Any other Line touches the axis at most at one of its own ends.
  - Either winding, fractional sizes, any Y translation, and any position of
    the axis Line in saved order are accepted.

**Exactly zero.** "On the axis" means the coordinate is exactly 0. IEEE
−0.0 is the same number: it is accepted and stored as +0.0, so a request
saying −0 and one saying 0 make the same document. There is no snapping,
no `abs`, and no tolerance that turns a small positive X into 0. A vertex
with 0 < X ≤ 1e-6 mm is refused as near the axis but not on it.

**Refused, with a message naming the vertex or Line:**
* X < 0 (crossing the axis);
* one isolated vertex on the axis;
* two vertices on the axis that are not the ends of one Line (two touches);
* more than two vertices on the axis (several axis Lines or intervals);
* an off-axis vertex within the clearance.

### Mesh precision

The mesh stores single-precision positions. Triangles with zero area at
that precision are omitted, including the collapsed triangles at a cone's
apex. If a whole face loses its triangles, mesh validation refuses export;
a valid B-Rep alone does not guarantee an exportable mesh. For example, a
0.002 mm thick hollow revolution at Y = 100000 mm has coincident Y levels
after float conversion. STL and FBX both refuse atomically, without an
output file. Earlier builds could publish a degenerate FBX for this case.

### Storage and capability

* **The axis Line is stated.** An axis-closed Revolve stores it by curve
  UUID in its own payload, as `axis_segment`. The layout moves with the
  meaning, exactly as for `previous` and ThroughAll:
  - a Revolve that names one is **payload v2**;
  - it requires the new capability **`feature.revolve.axis-closed.v1`** as
    well as `feature.revolve.v1`.
* **Hollow Revolves unchanged.** They stay payload v1, with byte-identical
  payloads, the same capabilities, refs and cache keys.
* **Old builds.** A §27B build does not list Revolve v2 as readable. It
  keeps the object verbatim and opens the document read-only. It cannot
  mistake the axis Line for a Line that failed to raise a face. This is
  checked with the real §27B binary, not emulated.
* **Two checks, both must pass.** The stored UUID is checked against the
  class the policy derives from the saved coordinates, every time the
  document is built, discovered, prepared or written. A payload naming a
  Line that is not the derived axis Line, or a hollow payload whose profile
  touches the axis, is refused. Neither the data nor the stored claim is
  trusted alone.

### The axis Line raises no face

The class is decided once, in the document policy. Every later layer checks
it and never infers it:
* **Kernel request.** `RevolveRequest` carries `axis_segment:
  Option<label>`, taken from the checked class.
* **Bridge.** `fc_occt_revolve` gains an explicit `axis_segment` index
  (`FC_OCCT_NO_AXIS_SEGMENT` for none). It requires exactly that Line's two
  vertices to lie on the axis and every other vertex to be off it. It
  requires the sweep to make no face of that Line and exactly one face of
  every other Line, with every face of the solid claimed once.
* **Topology.** `record_revolve` takes the same `Option<label>`. The named
  axis Line must raise nothing. Any other Line that raises nothing is still
  a refusal, exactly as before; the empty-history check is not weakened for
  anyone else.
* **Names.** Creation writes one `RevolveFace` reference per non-axis Line.
  The axis Line gets no reference, no seam or vertex name, and no
  placeholder. A cone's apex is not a face and gets no name.
* **Cache.** The archive stores the same `RevolvedFace` names, so a
  restored solid resolves them identically. The cache key includes the axis
  Line (fed only when present, so hollow keys stay the same).

### Editing coordinates

The same `edit-sketch-copy` and Edit Sketch, request v1 unchanged, and the
same job, writer and SQL copier.

* **Axis-closed profiles.** They stay axis-closed with the **same** axis
  Line. It must stay exactly on X = 0 and keep a nonzero length. Radii,
  lengths and Y position may change, and a cylinder may become a cone where
  the same Lines and names remain.
* **Hollow profiles** keep exactly the §27B rules.
* **No hollow ↔ solid transitions.** A transition would destroy or add
  faces, and needs its own reference policy. It is refused before copying,
  atomically, and the message says the part's axis closure cannot change.
  The same refusal is re-derived by the writer inside its transaction.

### CLI and JSON

* **Unchanged:** `create-sketch-revolve` and `edit-sketch-copy` keep their
  requests, envelopes, operations and exit codes. What changes is the
  geometry policy: requests with an axis Line that §27A/B refused are now
  accepted.
* **`sketches[].profile_feature`** gains a third kind for this class:

  ```json
  {"kind": "full_turn_revolve_axis_closed", "feature_id": "…", "body_id": "…",
   "axis": "sketch_y", "extent": "full_turn", "axis_curve_id": "…",
   "off_axis_clearance_mm": 1e-6}
  ```

  `full_turn_revolve` keeps meaning a profile strictly off the axis, with
  its `axis_clearance_mm`. A client that knows only the old kinds stops at
  the new one; it never reads it as the old contract.
* **`revolves[]`** gains two additive fields: `closure`
  (`"radial_clear"` or `"axis_closed"`) and `axis_curve_id` (null or the
  UUID).

### UI

The same Line editor, with precise coordinates, drag, Snap and Undo/Redo.
* **What the window says.** The canvas labels the axis, and the text
  explains both options: keep every point off the axis for a part with a
  bore, or put exactly one whole edge on X = 0 for a solid part. It no
  longer says unconditionally that every point needs X > 0.
* **Editing.** There is no feature, axis or angle switch. Moving an
  axis-Line vertex off the axis is refused with the reason, and the draft is
  kept.

### Not in this slice

* hollow ↔ solid transitions;
* partial turns and angle editing;
* other axes;
* Circle/Arc, constraints, several profiles or axis intervals;
* Boolean/Cut on the Revolve;
* in-place Save, live preview and new primitive builders.

## Executable agent recipe

Set `FERRITECAD` to a fresh native CLI. Optionally set `FCAD_UFBX_READER` to
the pinned `read_production` reader. The marked block uses only the public
CLI and its JSON, plus its own STL parser and its own Pappus volume; it never
parses prose. It chooses what to edit from `inspect --json`: the Sketch whose
`profile_feature.kind` is `full_turn_revolve_axis_closed`.

* **Profiles.** The cylinder, the cone, the stepped shaft, and a fractional
  shaft shifted along Y. Each is drawn in both windings, and once more with
  the axis Line moved to another place in saved order.
* **Create.** Discovery names the axis Line: the one Line both of whose ends
  have X = 0. `revolves[].closure` is `axis_closed`. `print-topology` names
  every other Line once and the axis Line not at all.
* **Mesh check.** The STL is closed and consistently oriented, within its
  radial and axial bounds, and within the chord band of the recipe's own
  Pappus volume. Every disc that ends on the axis has the area of a full
  disc, so it has no hole; a Line sloping onto the axis ends in an apex
  vertex on the axis.
* **Edit.** Every off-axis vertex moves 20 % in towards the axis and the
  whole profile 2.5 mm along it. The copy keeps every UUID, the axis Line and
  the Revolve entry, and is checked the same way.
* **Refusals, each exit 2 with nothing written.**
  - Create: a single touch, a crossing, two axis Lines, a vertex within
    1e-6 mm of the axis.
  - Edit: an axis end moved off the axis, the whole profile moved off it (a
    bored part), a stale version, reordered UUIDs and an occupied output.
* **−0.** A request saying −0 stores +0.
* **Source untouched.** The source bytes never change.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/axis-closed-revolve.md").read_text()
code = text.split("# FCAD_27C_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27c-recipe.py").write_text(code)
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-27c-recipe.py
```

```python
# FCAD_27C_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ.get("FCAD_UFBX_READER")
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27c-"))
LINEAR, ANGULAR = 0.05, 0.1

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def pappus(points):
    """The recipe's own volume of a full turn about Y: 2π ∫∫ x dA."""
    n = len(points)
    moment = sum((points[(i + 1) % n][1] - points[i][1])
                 * (points[i][0] ** 2 + points[i][0] * points[(i + 1) % n][0]
                    + points[(i + 1) % n][0] ** 2) / 6 for i in range(n))
    return 2 * math.pi * abs(moment)

def create(name, points, code=0):
    request = root / f"{name}.json"
    request.write_text(json.dumps({"request_version": 1, "points_mm": points,
                                   "axis": "sketch_y", "angle": "full_turn"}))
    out = root / f"{name}.fcad"
    reply = run(["create-sketch-revolve", request, "-o", out, "--json"], code)
    assert out.exists() == (code == 0), name
    return out, reply

def solid(catalog):
    [row] = [s for s in catalog["sketches"] if s["editable"]
             and (s["profile_feature"] or {}).get("kind") == "full_turn_revolve_axis_closed"]
    return row

def axis_line(vertices):
    n = len(vertices)
    on = [i for i in range(n) if vertices[i]["start_mm"][0] == 0]
    [line] = [vertices[i]["curve_id"] for i in on if (i + 1) % n in on]
    return line

def edit(source, name, move, code=0, version=None, reorder=False, out=None):
    catalog = run(["inspect", source, "--json"])["result"]
    row = solid(catalog)
    vertices = [{"curve_id": v["curve_id"], "start_mm": move(v["start_mm"])}
                for v in row["vertices"]]
    if reorder:
        vertices[0], vertices[1] = vertices[1], vertices[0]
    request = root / f"{name}-edit.json"
    request.write_text(json.dumps({"request_version": 1, "vertices": vertices}))
    out = out or root / f"{name}.fcad"
    reply = run(["edit-sketch-copy", source, "--sketch", row["sketch_id"],
                 "--expect-version", version or catalog["content_version"],
                 "--request", request, "-o", out, "--json"], code)
    return catalog, row, out, reply, [v["start_mm"] for v in vertices]

def stl(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    return [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)]
            for i in range(count)]

def check_mesh(triangles, points):
    key = lambda v: tuple(round(c, 4) for c in v)
    edges = {}
    for t in triangles:
        for a, b in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
            edges[(key(a), key(b))] = edges.get((key(a), key(b)), 0) + 1
    assert all(n == 1 and edges.get((b, a)) == 1 for (a, b), n in edges.items()), "not closed"
    volume = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                 + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in triangles) / 6
    n = len(points)
    band = sum(2 * math.pi * max(points[i][0], points[(i + 1) % n][0]) * LINEAR
               * abs(points[(i + 1) % n][1] - points[i][1]) for i in range(n))
    exact = pappus(points)
    assert abs(volume - exact) <= band, (volume, exact, band)
    xs, ys = [p[0] for p in points], [p[1] for p in points]
    quadrants, discs, near = set(), {}, []
    for t in triangles:
        for x, y, z in t:
            assert math.hypot(x, z) < max(xs) + 1e-4
            assert min(ys) - 1e-4 < y < max(ys) + 1e-4
            if abs(x) > 1e-3 and abs(z) > 1e-3:
                quadrants.add((x > 0, z > 0))
            near.append((math.hypot(x, z), y))
        u = [t[1][k] - t[0][k] for k in range(3)]
        v = [t[2][k] - t[0][k] for k in range(3)]
        c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
        area = math.sqrt(sum(k * k for k in c)) / 2
        if area > 0 and abs(c[1]) / (2 * area) > 1 - 1e-9:
            y = round(t[0][1], 4)
            discs[y] = discs.get(y, 0) + area
    assert len(quadrants) == 4
    for i in range(n):
        (x0, y0), (x1, y1) = points[i], points[(i + 1) % n]
        if y0 == y1 and 0 in (x0, x1):
            # A radial Line ending on the axis is a full disc: no hole.
            r = max(x0, x1)
            assert abs(discs[round(y0, 4)] - math.pi * r * r) <= 2 * math.pi * r * LINEAR, \
                (y0, discs, r)
        elif y0 != y1 and (x0 == 0) != (x1 == 0):
            # A sloped Line onto the axis ends in an apex on the axis.
            apex = y0 if x0 == 0 else y1
            assert any(rho < 1e-6 and abs(y - apex) < 1e-4 for rho, y in near), apex

def verify(row, out, points, catalog=None):
    after = run(["inspect", out, "--json"])["result"]
    new = solid(after)
    ids = [v["curve_id"] for v in new["vertices"]]
    assert [v["start_mm"] for v in new["vertices"]] == points
    assert all(math.copysign(1, v["start_mm"][0]) == 1 for v in new["vertices"]), "−0 stored"
    axis = axis_line(new["vertices"])
    assert new["profile_feature"]["axis_curve_id"] == axis
    [revolve] = after["revolves"]
    assert revolve["closure"] == "axis_closed" and revolve["axis_curve_id"] == axis
    if catalog is not None:
        assert ids == [v["curve_id"] for v in row["vertices"]]
        assert new["profile_feature"] == row["profile_feature"]
        assert after["document_id"] == catalog["document_id"]
        assert after["bodies"] == catalog["bodies"]
    run(["validate", out])
    assert "1 shape built" in run(["rebuild", "--cold", out])
    topology = run(["print-topology", out])
    faces = [i for i in ids if i != axis]
    for i in faces:
        assert f"revolve face from segment {i}" in topology
    assert f"segment {axis}" not in topology
    assert f"{len(faces)} of {len(faces)} references resolved" in topology
    mesh = out.with_suffix(".stl")
    run(["export-stl", out, "-o", mesh, "--linear-deflection", LINEAR,
         "--angular-deflection", ANGULAR, "--json"])
    check_mesh(stl(mesh), points)
    if reader:
        fbx = out.with_suffix(".fbx")
        assert run(["export-fbx", out, "-o", fbx, "--json"])["result"]["complete"]
        text = subprocess.run([reader, "--identity", fbx], capture_output=True,
                              text=True, check=True).stdout
        assert "FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0" in text
    return new

PROFILES = {
    "cylinder": ([[0, 0], [10, 0], [10, 15], [0, 15]], 1500 * math.pi),
    "cone": ([[0, 0], [10, 0], [0, 15]], 500 * math.pi),
    "shaft": ([[0, 0], [10, 0], [10, 5], [6, 5], [6, 15], [0, 15]], 860 * math.pi),
    "fraction": ([[0, -3.25], [7.5, -3.25], [7.5, 2.125], [2.75, 9.5], [0, 9.5]], None),
}
checked = 0
for name, (points, exact) in PROFILES.items():
    if exact is not None:
        assert abs(pappus(points) - exact) < 1e-9 * exact, name
    rotated = points[2:] + points[:2]
    for tag, drawn in (("ccw", points), ("cw", points[::-1]), ("rotated", rotated)):
        source, _ = create(f"{name}-{tag}", drawn)
        before = source.read_bytes()
        row = verify(None, source, drawn)
        checked += 1
        result = edit(source, f"{name}-{tag}-moved",
                      lambda p: [p[0] * 0.8, p[1] + 2.5])
        catalog, row, out, reply, moved = result
        assert reply["ok"] and reply["result"]["sketch_id"] == row["sketch_id"]
        assert verify(row, out, moved, catalog)["profile_feature"]["axis_curve_id"] \
            == row["profile_feature"]["axis_curve_id"]
        checked += 1
        names = sorted(p.name for p in root.iterdir() if not p.name.endswith("-edit.json"))
        axis_ends = [v["start_mm"] for v in row["vertices"] if v["start_mm"][0] == 0]
        edit(source, "unhinged", lambda p: [1, p[1]] if p == axis_ends[0] else p, code=2)
        edit(source, "bored", lambda p: [p[0] + 1, p[1]], code=2)
        edit(source, "stale", lambda p: p, code=2, version="0" * 64)
        edit(source, "reordered", lambda p: p, code=2, reorder=True)
        taken = root / "taken.fcad"
        taken.write_bytes(b"keep")
        edit(source, "taken", lambda p: [p[0] * 0.9, p[1]], code=2, out=taken)
        assert taken.read_bytes() == b"keep"
        taken.unlink()
        assert sorted(p.name for p in root.iterdir() if not p.name.endswith("-edit.json")) \
            == names
        assert source.read_bytes() == before

minus, _ = create("minus-zero", [[-0.0, 0], [10, 0], [10, 15], [-0.0, 15]])
verify(None, minus, [[0, 0], [10, 0], [10, 15], [0, 15]])
for name, points in (
    ("touch", [[0, 0], [10, 0], [10, 15], [4, 15]]),
    ("cross", [[-2, 0], [10, 0], [10, 15], [-2, 15]]),
    ("two-lines", [[0, 0], [10, 0], [10, 15], [0, 15], [0, 10], [5, 10], [5, 5], [0, 5]]),
    ("near", [[1e-7, 0], [10, 0], [10, 15], [0, 15]]),
):
    out, reply = create(name, points, 2)
    assert not reply["ok"], (name, reply)
print("FCAD_27C_RECIPE_OK", checked, root)
```
