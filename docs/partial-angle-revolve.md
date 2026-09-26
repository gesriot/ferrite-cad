# §27D — partial Revolve: a sector of a turned part

[Executed verification and limitations](partial-angle-revolve-verification.md).

A person or an agent turns the same simple closed Line profile as §27A/§27C
(a part with a bore, or a solid part closed on the axis) through a stated
angle instead of a full turn, and gets a new `.fcad` whose Body is that
sector: a quarter of a bushing at 90°, half a cylinder at 180°, three
quarters of a cone at 270°. The sector is real OCCT geometry with two
planar end faces, each named. It is not a full body cut or hidden after the
fact.

Full-turn documents, their payloads, JSON, cache keys and editing are
unchanged.

## Contract recorded before implementation

### Measured first (OCCT 8.0.1, pinned)

A C++ probe against the installed kernel turned three profiles about the Y
axis through the origin with `BRepPrimAPI_MakeRevol(face, axis, θ)`:
the cylinder [(0,0),(10,0),(10,15),(0,15)] (solid, axis-closed), the bushing
[(4,0),(10,0),(10,15),(4,15)] (with a bore) and the cone
[(0,0),(10,0),(0,15)] (solid, axis-closed), at θ = 90°, 180°, 270° and
137.5°, both windings. Every result is one valid solid whose volume equals
the full-turn volume × θ/360 to a relative 4e-16.

| Profile | Faces of the sector | From the Lines | Caps |
|---|---|---|---|
| Cylinder | 5: Plane, Cylinder, Plane, Plane, Plane | 3 (the axis Line raises nothing) | 2 |
| Bushing | 6: Plane, Cylinder, Plane, Cylinder, Plane, Plane | 4 | 2 |
| Cone | 4: Plane, Cone, Plane, Plane | 2 (the axis Line raises nothing) | 2 |

What the history reported:
* **Every non-axis Line raises exactly one face.** For a partial angle
  `MakeRevol::Generated(edge)` and `BRepSweep_Revol::Shape(edge)` agree on
  it, radial Lines included. (For a full turn `Generated` is empty for
  radial Lines; see §27A. The bridge keeps reading `Shape(edge)` in both
  cases.)
* **The axis Line still raises nothing.** `Shape(edge)` is null,
  `Generated` is empty and `IsDeleted` is false, as for a full turn.
* **Two caps with their own history.** `BRepSweep_Revol::FirstShape(face)`
  and `LastShape(face)` of the profile face are each one face of the
  finished solid. They are distinct from each other and from every Line's
  face, and together with the Line faces they cover the solid exactly. The
  first cap is not the profile face object itself (a copy, `IsSame` false),
  so it is taken from the sweep's history, never matched by position.
* **Orientation is read from the solid.** The cap faces in the solid carry
  their own orientation: the end cap's instance inside the solid is REVERSED
  relative to `LastShape`. Outward normals are measured on the solid's
  instances.
* **A full turn is different topology.** At exactly 360° `FirstShape` and
  `LastShape` are the same face and are not faces of the solid. The full
  turn keeps its own extent; a partial angle is never widened to it.

### Angle, unit, direction

* **Unit and form.** The angle is in **degrees**, a finite number, stored
  exactly as given. Nothing is rounded, reduced modulo 360 or converted to
  another operation. The kernel converts it once, at the bridge, as
  `θ_rad = degrees × (π / 180)`.
* **Direction.** A right-handed turn about the sketch's +Y axis through its
  origin, starting at the profile itself. With the sketch on the untransformed
  XY datum, a profile point (x, y) sweeps through
  (x·cos φ, y, −x·sin φ) for φ from 0 to θ. A 90° sector of a profile with
  x > 0 therefore occupies x ≥ 0, z ≤ 0.
* **Caps.**
  - **Start cap:** the profile's own region at φ = 0, on the sketch plane
    z = 0, outward normal (0, 0, +1).
  - **End cap:** the profile's region turned to φ = θ, outward normal
    (−sin θ, 0, −cos θ).
* **Accepted range.** 0.01 ≤ θ ≤ 359.99 degrees, both ends inclusive.
* **Refused, each with a message naming the value:**
  - 0, −0 and any negative angle (no mirrored sweep);
  - 0 < θ < 0.01 (too thin to be a sector this build keeps);
  - 359.99 < θ < 360 (within 0.01° of a full turn);
  - exactly 360: refused with "state a full turn instead". It is not
    rewritten to the full-turn extent: the two have different faces;
  - θ > 360: several turns are not a solid;
  - NaN and ±∞.

### Why these numeric limits

They are chosen by kernel and precision checks, separately from the full
turn:
* **Kernel.** OCCT built one valid solid with the exact volume at every angle
  probed, down to 1e-6° and up to 359.999999°, including a ring at a radius
  of 1e6 mm. The kernel is not the limit.
* **Mesh precision.** The mesh stores single-precision positions (§27C).
  At 1e-6° a 10 mm bushing loses three whole faces to float collapse, and a
  cone loses one; export would then refuse. At 0.01° every face of the
  10 mm bushing and cone, and of a 1e6 mm ring, keeps its triangles, and so
  does the full-size gap left by 359.99°.
* **Angular precision.** 0.01° is 1.745e-4 rad, eight orders of magnitude
  above OCCT's `Precision::Angular()` (1e-12), at both ends of the range.
* **Still guarded.** A sector can still be too thin to mesh near the axis
  clearance: a profile at x = 2e-6 mm turned 0.01° kept a valid B-Rep but
  lost one face's triangles. Such an export is refused atomically by the
  §27C rule ("a whole face loses its triangles"). A valid B-Rep alone still
  does not guarantee an exportable mesh.

### Profiles

Exactly the §27A/§27C policy (`FullTurnRevolution`), shared unchanged:
* a part with a bore, every vertex beyond the clearance; or
* a solid part closed on the axis along one whole Line.

Either winding, fractional sizes, any Y translation and any position of the
axis Line in saved order are accepted. The profile policy and the angle
policy are separate values; a request needs both.

### Storage and capability

* **Extent.** `RevolveExtent` gains `Partial { degrees }`. The degrees are
  a validated `RevolveAngle`, checked again when a payload is decoded.
* **The layout moves with the meaning**, one payload version per
  combination, decided by what the Revolve holds:

  | Payload | Extent | Axis Line | Required capabilities |
  |---|---|---|---|
  | v1 | full turn | none | core, `feature.revolve.v1` |
  | v2 | full turn | stated | + `feature.revolve.axis-closed.v1` |
  | v3 | partial | none | core, `feature.revolve.v1`, **`feature.revolve.partial.v1`** |
  | v4 | partial | stated | core, `feature.revolve.v1`, `feature.revolve.axis-closed.v1`, **`feature.revolve.partial.v1`** |

* **Full turns unchanged.** v1/v2 payloads stay byte-identical, with the
  same capabilities, references and cache keys.
* **Old builds.** A §27C build reads Revolve payloads v1–v2 only. It keeps a
  v3/v4 object verbatim and opens the document read-only, because it does
  not implement the new capability. It cannot read a sector as a full turn.
  This is checked with the real §27C binary, not emulated.
* **No SQL schema change.**
* **Payload and header must agree.** A header version that disagrees with
  what the payload holds is refused, as for v1/v2.

### Topology outputs

* **Faces of revolution.** One `RevolveFace { profile_segment }` per
  non-axis Line, unchanged.
* **Caps.** New role `RevolveCap { side: start | end }`.
  - **Owner:** the Revolve, as producer and owner of the reference.
  - **Provenance:** the sweep's `FirstShape`/`LastShape` of the profile
    face, checked in the bridge. Each cap must be one face of the finished
    solid, the two must differ, and neither may also be a Line's face.
    Every face of the solid must be claimed once.
  - It is not `ExtrudeCap`: an Extrude cap reference never resolves against
    a Revolve, and a Revolve cap reference never against an Extrude.
  - It requires `feature.revolve.v1` and `feature.revolve.partial.v1`.
* **What is not named.** The axis Line still raises nothing and gets no
  name. There is no seam, axis face, apex, cap edge or vertex name, no face
  index, and no fallback to the first face of any kind.
* **A full turn names no caps.** A cap reported for a full turn is refused.
* **Creation** writes one `RevolveFace` reference per non-axis Line and the
  two `RevolveCap` references. Publication requires every one to resolve
  after the cold build.
* **Cache.** The archive stores the caps under new `RevolvedCap` names (new
  tags; an older cache reader refuses the unknown tag and rebuilds). The
  evaluator's key adds the angle only for a partial turn, so full-turn keys
  do not move.

### Kernel

* **Request.** `RevolveTurn` gains `Partial { degrees }`. `RevolveResult`
  gains `start_cap` and `end_cap`, which must be empty for a full turn and
  exactly one face each for a partial turn.
* **Bridge.**
  - `fc_occt_revolve` is unchanged, including its `full_turn` flag.
  - A new entry point, `fc_occt_revolve_partial`, takes `double
    angle_degrees` in its own place. It refuses a non-finite value or one
    outside the open interval (0, 360); the 0.01° policy is the caller's.
  - `fc_occt_revolve_caps(shape, side)` answers the caps with the same
    two-call protocol as the face query. It is refused for a full turn or a
    decoded shape.
* **Doubles.** The mock kernel still does not build revolutions, and the
  unavailable kernel still refuses them.

### CLI

`create-sketch-revolve` accepts two request versions:
* **Request v1** keeps exactly its bytes and meaning: `"angle":"full_turn"`
  is required and is the only value.
* **Request v2**, strict (`deny_unknown_fields` at every level):

  ```json
  {"request_version":2,"points_mm":[[4,0],[10,0],[10,15],[4,15]],
   "axis":"sketch_y","extent":{"kind":"angle","degrees":90}}
  {"request_version":2,"points_mm":[[0,0],[10,0],[10,15],[0,15]],
   "axis":"sketch_y","extent":{"kind":"full_turn"}}
  ```

  - A v2 full turn makes the same document class a v1 request does.
  - v2 has no `angle` field. A v2 request carrying v1's
    `"angle":"full_turn"`, or no `extent`, is refused.
  - Any other version is refused as unsupported.

Envelopes, operation names, the result object and exit codes (0 published,
2 refused, 7 report delivery failed) are unchanged.

### JSON discovery (additive)

* **`revolves[]`**
  - `extent` keeps its type (a string). It is `"partial_turn"` for a sector;
    a client that knows only `"full_turn"` stops at the new value.
  - **`angle_deg`**, new: the stored degrees for a sector, null for a full
    turn.
* **`sketches[]`** of a sector's profile:
  - In §27D, `editable` was false, and `refusal` said coordinate editing
    of a partial Revolve was not supported. `vertices` and
    `profile_feature` were null.
  - §27F replaces this: the row is editable, with its `vertices` and the
    new kinds `partial_turn_revolve`/`partial_turn_revolve_axis_closed`,
    which state `angle_deg`
    ([contract](edit-partial-revolve-profile.md)).
  - No field of the full-turn kinds changes.
* **`print-topology`** names the caps as `revolve start cap` and
  `revolve end cap`.

### Editing

* **Full turns.** Coordinate editing (§27B/§27C) is unchanged.
* **A saved sector was not editable in this slice.**
  - `edit-sketch-copy` and Edit Sketch refused atomically: exit 2, nothing
    written, the source unchanged.
  - §27F replaces this with the class contract of
    [editing a sector's profile](edit-partial-revolve-profile.md).
  - The writer's own re-derivation refuses too, so the refusal does not
    depend on the discovery layer alone.
  - Angle editing is a separate slice, since added by §27E:
    [edit a saved sector's angle](edit-revolve-angle.md).

### UI

The same sketch window and Line editor. The **Feature** choice gains a
third option beside **Extrude** and **Revolve 360°**: **Revolve angle**.
* **Revolve 360°** keeps exactly the §27A/§27C behaviour and wording.
* **Revolve angle** shows an **Angle °** field (90 by default) and says the
  sector turns right-handed about the sketch Y axis from the profile, within
  0.01°–359.99°, and that 360° is Revolve 360°. The header reads "XY · mm ·
  Line polygon · Revolve through an angle about the sketch Y axis · NewBody",
  and the canvas labels the axis as for a full turn.
* An invalid angle (empty, text, 0, negative, 360, within 0.01° of either
  end) shows the domain's refusal in place of `Create in new file…`; the
  draft is kept.

The choice and the typed angle are part of the draft: Undo/Redo step through
them, and switching feature keeps the typed angle. Publication goes through
the same worker, create job and Open path as every other creation.

(Recorded before implementation as a separate "Turn" choice under Revolve;
implemented as one more Feature option so the full-turn option keeps its
label, text and tests unchanged. The behaviour is the same.)

### Mesh of a sector (measured after implementation)

On OCCT 8.0.1 the mesh of a cone sector contains collinear slivers along
its first ruling from the apex. Those whose float area is exactly zero are
omitted, as §27C omits every zero-area triangle, because STL cannot write
them. What remains is geometrically closed, but some edges end at a vertex
lying on a neighbour's edge: a T-junction. The independent STL checks
therefore split an unmatched edge at the mesh vertices lying on it and then
require every piece to meet its reverse. A real hole has no vertex on its
edge and still fails. Full-turn meshes are unchanged.

### Not in this slice

Angle editing and coordinate editing of a saved sector; other axes; a
negative (mirrored) sweep; Arc/Circle profiles; booleans on a Revolve;
several bodies; live preview; in-place Save.

## Executable agent recipe

Set `FERRITECAD` to a fresh native CLI. Optionally set `FCAD_UFBX_READER` to
the pinned `read_production` reader built from
`tools/unity-fbx-smoke/scripts/read_production.c`. The marked block uses only
the public CLI and its JSON, its own STL parser, its own Pappus and shoelace,
and the reader's `--identity` and `--triangles` output. It never parses prose.

* **Sectors.** The bushing, cylinder, cone and a fractional profile shifted
  along Y, each at 90°, 180°, 270° and 137.5°, drawn counter-clockwise,
  clockwise and from a shifted start: 48 documents.
* **Discovery.** `revolves[0].extent` is `partial_turn` with the exact
  `angle_deg`. `closure` is `axis_closed` exactly when a Line lies on
  X = 0. Since §27F, the Sketch is editable as a
  `partial_turn_revolve*` kind that states the angle.
* **Rebuild.** `validate` and `rebuild --cold` pass. `print-topology` names
  both end faces and resolves every reference.
* **STL, parsed independently.**
  - closed and consistently oriented, T-junctions excepted as recorded
    above;
  - outward, with a signed volume within the chord band of
    Pappus × θ/360;
  - every vertex inside [0°, θ] and none in the space beyond it;
  - each end face on its plane, facing out, covering the profile's area.
* **FBX.** With a reader, the counter-clockwise sector of each profile and
  angle is exported as FBX and as an STL at the same default tessellation.
  The reader's identity channel must report `checks=6 failures=0`, and its
  world-space triangles must equal the STL's under (x, z, −y) · 0.001,
  winding included, within 1 nm.
* **Refusals, each exit 2 with nothing written.**
  - 0°, −90°, 0.005°, 359.995°, 360°, 400°;
  - an angle on a full turn, an unknown kind, v1's spelling;
  - an occupied output, whose bytes are unchanged;
  - since §27F, an `edit-sketch-copy` that would make a bored sector
    solid (`input`, "hollow and solid", source unchanged).

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/partial-angle-revolve.md").read_text()
code = text.split("# FCAD_27D_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27d-recipe.py").write_text(code)
EXTRACT
FERRITECAD=/path/to/ferritecad FCAD_UFBX_READER=/path/to/read_production python3 ferrite-27d-recipe.py
```

```python
# FCAD_27D_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ.get("FCAD_UFBX_READER")
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27d-"))
LINEAR, ANGULAR = 0.05, 0.1

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def pappus(points):
    """The recipe's own full-turn volume: 2π ∫∫ x dA."""
    n = len(points)
    moment = sum((points[(i + 1) % n][1] - points[i][1])
                 * (points[i][0] ** 2 + points[i][0] * points[(i + 1) % n][0]
                    + points[(i + 1) % n][0] ** 2) / 6 for i in range(n))
    return 2 * math.pi * abs(moment)

def area(points):
    n = len(points)
    return abs(sum(points[i][0] * points[(i + 1) % n][1] - points[(i + 1) % n][0] * points[i][1]
                   for i in range(n))) / 2

def create(name, points, extent, code=0):
    request = root / f"{name}.json"
    request.write_text(json.dumps({"request_version": 2, "points_mm": points,
                                   "axis": "sketch_y", "extent": extent}))
    out = root / f"{name}.fcad"
    before = sorted(p.name for p in root.iterdir())
    reply = run(["create-sketch-revolve", request, "-o", out, "--json"], code)
    if code:
        assert sorted(p.name for p in root.iterdir()) == before, name
    return out, reply

def stl(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    return [tuple(struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3))
            for i in range(count)]

def sub(a, b): return [a[k] - b[k] for k in range(3)]
def dot(a, b): return sum(a[k] * b[k] for k in range(3))
def cross(t):
    u, v = sub(t[1], t[0]), sub(t[2], t[0])
    return [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]

def closed(triangles):
    """Every directed edge once; an edge without its reverse only where mesh
    vertices lie on it (a T-junction), and then every piece has its reverse."""
    key = lambda v: tuple(round(c * 1e4) for c in v)
    at, edges = {}, {}
    for t in triangles:
        for v in t:
            at[key(v)] = v
        for a, b in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
            e = (key(a), key(b))
            edges[e] = edges.get(e, 0) + 1
    assert all(n == 1 for n in edges.values()), "a directed edge twice"
    pieces = {}
    for a, b in [e for e in edges if (e[1], e[0]) not in edges]:
        pa, pb = at[a], at[b]
        d = sub(pb, pa)
        on = []
        for k, p in at.items():
            if k in (a, b):
                continue
            w = sub(p, pa)
            t = dot(w, d) / dot(d, d)
            off = [w[i] - t * d[i] for i in range(3)]
            if 0 < t < 1 and math.sqrt(dot(off, off)) < 1e-4:
                on.append((t, k))
        chain = [a] + [k for _, k in sorted(on)] + [b]
        for x, y in zip(chain, chain[1:]):
            pieces[(x, y)] = pieces.get((x, y), 0) + 1
            pieces[(y, x)] = pieces.get((y, x), 0) - 1
    assert all(n == 0 for n in pieces.values()), "open mesh"

def phi(p):
    a = math.degrees(math.atan2(-p[2], p[0])) % 360
    return a - 360 if a > 360 - 1e-3 else a

def check_mesh(triangles, points, degrees):
    closed(triangles)
    volume = sum(dot(t[0], [t[1][1] * t[2][2] - t[1][2] * t[2][1],
                            t[1][2] * t[2][0] - t[1][0] * t[2][2],
                            t[1][0] * t[2][1] - t[1][1] * t[2][0]]) for t in triangles) / 6
    n = len(points)
    band = sum(2 * math.pi * max(points[i][0], points[(i + 1) % n][0]) * LINEAR
               * abs(points[(i + 1) % n][1] - points[i][1]) for i in range(n)) * degrees / 360
    exact = pappus(points) * degrees / 360
    assert volume > 0 and abs(volume - exact) <= band, (volume, exact, band)
    angles = [phi(v) for t in triangles for v in t if math.hypot(v[0], v[2]) > 1e-3]
    assert min(angles) > -1e-3 and max(angles) < degrees + 1e-3, (min(angles), max(angles))
    assert abs(min(angles)) < 1e-3 and abs(max(angles) - degrees) < 1e-3
    s, c = math.sin(math.radians(degrees)), math.cos(math.radians(degrees))
    for outward, radial in (([0, 0, 1], [1, 0, 0]), ([-s, 0, -c], [c, 0, -s])):
        covered = 0.0
        for t in triangles:
            n3 = cross(t)
            length = math.sqrt(dot(n3, n3))
            if (length > 0 and dot(n3, outward) / length > 1 - 1e-6
                    and all(abs(dot(v, outward)) < 1e-4 and dot(v, radial) > -1e-4 for v in t)):
                covered += length / 2
        assert abs(covered - area(points)) < 1e-4 * area(points), (outward, covered)

def fbx_matches(fbx, stl_path):
    """Pinned ufbx's world-space triangles against the STL, (x, z, -y) · 0.001."""
    text = subprocess.run([reader, "--triangles", fbx], capture_output=True, text=True,
                          check=True).stdout
    assert "failures=0" in text.splitlines()[-1], text[-200:]
    got = [tuple(tuple(v[3 * k:3 * k + 3]) for k in range(3))
           for v in ([float(x) for x in line.split()[1:]] for line in text.splitlines()
                     if line.startswith("FCAD_TRIANGLE "))]
    want = [tuple((x / 1000, z / 1000, -y / 1000) for x, y, z in t) for t in stl(stl_path)]
    canon = lambda t: min(t[i:] + t[:i] for i in range(3))
    got, want = sorted(map(canon, got)), sorted(map(canon, want))
    assert len(got) == len(want), (len(got), len(want))
    worst = max(abs(a - b) for g, w in zip(got, want) for p, q in zip(g, w) for a, b in zip(p, q))
    assert worst < 1e-9, worst

PROFILES = {
    "bushing": [[4, 0], [10, 0], [10, 15], [4, 15]],
    "cylinder": [[0, 0], [10, 0], [10, 15], [0, 15]],
    "cone": [[0, 0], [10, 0], [0, 15]],
    "fraction": [[2.75, -3.25], [7.5, -3.25], [7.5, 2.125], [4.25, 9.5], [2.75, 9.5]],
}
checked = 0
for name, points in PROFILES.items():
    for degrees in (90, 180, 270, 137.5):
        for tag, drawn in (("ccw", points), ("cw", points[::-1]), ("shifted", points[1:] + points[:1])):
            out, _ = create(f"{name}-{degrees}-{tag}", drawn, {"kind": "angle", "degrees": degrees})
            c = run(["inspect", out, "--json"])["result"]
            [r] = c["revolves"]
            assert r["extent"] == "partial_turn" and r["angle_deg"] == degrees
            solid = any(p[0] == 0 for p in drawn)
            assert r["closure"] == ("axis_closed" if solid else "radial_clear")
            row = c["sketches"][0]
            # §27F: the sector's profile is editable and states its angle.
            assert row["editable"] and row["refusal"] is None
            assert row["profile_feature"]["angle_deg"] == degrees
            assert row["profile_feature"]["kind"] == (
                "partial_turn_revolve_axis_closed" if solid else "partial_turn_revolve")
            run(["validate", out])
            assert "1 shape built" in run(["rebuild", "--cold", out])
            topology = run(["print-topology", out])
            faces = len(drawn) - (1 if solid else 0)
            assert "revolve start cap" in topology and "revolve end cap" in topology
            assert f"{faces + 2} of {faces + 2} references resolved" in topology
            mesh = out.with_suffix(".stl")
            run(["export-stl", out, "-o", mesh, "--linear-deflection", LINEAR,
                 "--angular-deflection", ANGULAR, "--json"])
            check_mesh(stl(mesh), drawn, degrees)
            if reader and tag == "ccw":
                fbx, plain = out.with_suffix(".fbx"), root / f"{out.stem}-default.stl"
                assert run(["export-fbx", out, "-o", fbx, "--json"])["result"]["complete"]
                run(["export-stl", out, "-o", plain, "--json"])
                ident = subprocess.run([reader, "--identity", fbx], capture_output=True,
                                       text=True, check=True).stdout
                assert "FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0" in ident
                fbx_matches(fbx, plain)
            checked += 1

source, _ = create("sector", PROFILES["bushing"], {"kind": "angle", "degrees": 90})
kept = source.read_bytes()
for i, extent in enumerate(({"kind": "angle", "degrees": 0}, {"kind": "angle", "degrees": -90},
                            {"kind": "angle", "degrees": 0.005}, {"kind": "angle", "degrees": 359.995},
                            {"kind": "angle", "degrees": 360}, {"kind": "angle", "degrees": 400},
                            {"kind": "full_turn", "degrees": 90}, {"kind": "half_turn"}, "full_turn")):
    out, reply = create(f"refused-{i}", PROFILES["bushing"], extent, 2)
    assert reply["error"]["kind"] == "input", reply
taken = root / "taken.fcad"
taken.write_bytes(b"keep")
request = root / "sector.json"
run(["create-sketch-revolve", request, "-o", taken, "--json"], 2)
assert taken.read_bytes() == b"keep"
c = run(["inspect", source, "--json"])["result"]
segments = c["revolves"][0]["profile"]["segments"]
edit = root / "edit.json"
# §27F: a sector's profile edits like a full turn's, inside its class; the
# bored sector pulled onto the axis would become solid, which is refused.
edit.write_text(json.dumps({"request_version": 1, "vertices": [
    {"curve_id": s["curve_id"], "start_mm": [0 if s["start_mm"][0] == 4 else s["start_mm"][0],
                                              s["start_mm"][1]]} for s in segments]}))
names = sorted(p.name for p in root.iterdir())
reply = run(["edit-sketch-copy", source, "--sketch", c["sketches"][0]["sketch_id"],
             "--expect-version", c["content_version"], "--request", edit,
             "-o", root / "edited.fcad", "--json"], 2)
assert reply["error"]["kind"] == "input" and "hollow and solid" in reply["error"]["message"]
assert sorted(p.name for p in root.iterdir()) == names and source.read_bytes() == kept
print("FCAD_27D_RECIPE_OK", checked, "fbx" if reader else "no-fbx-reader", root)
```
