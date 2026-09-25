# §27B — edit a saved full-turn Revolve profile in a new copy

[Executed verification and limitations](edit-revolve-profile-verification.md).

A person opens a part created by [§27A](full-turn-revolve.md) — a bushing, a
stepped or a sloped (conical) part — changes the coordinates of its saved
profile in the ordinary **Edit Sketch** window and saves a new `.fcad`. An
agent gets exactly the same result through the existing `edit-sketch-copy`.
Changing coordinates creates no new Line, no new Revolve and no new face
name. This slice is not Revolve editing in general and does not complete
Revolve or wave 5A.

## Contract recorded before implementation

### The document class

Exactly one standalone §27A document:

* four root objects, none with a parent:
  - an untransformed XY `DatumPlane`;
  - one unconstrained, closed Line `Sketch` on it;
  - one `Revolve` of that Sketch with `axis: sketch_y`, `extent: full_turn`,
    `operation: new_body`;
  - one `Body` whose tip is that Revolve;
* exactly three dependencies: `sketch → plane (Plane)`,
  `revolve → sketch (Profile)`, `body → revolve (BodyTip)`;
* a profile of 3..256 Lines, as accepted by `FullTurnRevolution`: a simple
  polygon, every vertex at X > `AXIS_CLEARANCE_MM` (1e-6 mm). Either
  winding, steps, sloped walls and any Y translation are accepted, as at
  creation.

Anything else refuses. That includes:
* constraints, construction geometry, Circles or Arcs;
* a transformed plane;
* extra objects, a second Body, or a Cut/Boolean on the Revolve;
* a Revolve with another axis or angle (not representable today, and
  refused if it ever is);
* unknown or future payloads, and documents that `copy_access` forbids.

The Extrude-based classes — §25B polygons, the §26F/§26I Cut-history base,
and the circle, annulus, constraint and height editors — keep their exact
boundaries. Their structure check (`frame`) is not changed.

### What may change

Only the coordinates of the saved Lines. The following stay identical:
* the curve UUIDs, their number and order, and the closure;
* the Sketch, Revolve, Body, plane and document IDs;
* the Revolve payload (axis, angle, operation), units and capabilities;
* every other SQL cell, apart from the selected Sketch's `payload`/
  `payload_hash`. As before, `meta.modified_at` is preserved too.

The start of Line *i* is the end of Line *i−1*. Each vertex is set once, so
both Lines move together. As for every coordinate edit, the winding may not
change. This refuses a mirror image of the profile rather than accepting a
different drawing under the same names.

### Face names under an edit

Each stored `RevolveFace { profile_segment }` means the face raised by that
Line. It does not mean a surface type that stays fixed. Moving a Line's ends
may change the analytic kind of its face, and that is allowed and measured:
* an axial Line whose ends get different radii turns from a cylinder into a
  cone;
* a Line that becomes perpendicular to the axis turns into an annular plane.

The reference must still resolve, and exactly to the face that Line raises
now. A reference that is lost or swapped is refused; it is never the price
of an edit.

### One owner of the rules

* `sketch_edit::coordinate_choice` stays the single place that decides
  whether a Sketch's coordinates are editable. Three callers use it:
  - discovery (`sketch_choices`);
  - preparation (`replace_sketch_coordinates`);
  - the writer's in-transaction re-derivation (`write_sketch_geometry`).

  It gains a second structure check, `revolve_frame`, beside the unchanged
  Extrude `frame`. Which one applies follows from the document: a Revolve
  whose profile is this Sketch selects the Revolve frame. There is no
  fallback between them.
* **Explicit profile kind.** `SketchChoice` states which feature uses the
  profile, as an explicit `SketchProfileUse`:
  - `BlindExtrude { feature, height_mm }`;
  - `FullTurnRevolve { feature, body }`.

  It replaces the bare `height_mm: Option<f64>`. A Revolve is never given a
  height, a fictitious Extrude, or an implied UUID.
* **Numeric policy.** `SketchChoice::validate_coordinates` checks a draft
  with the policy of that kind: `PolygonExtrusion` for an Extrude and
  `FullTurnRevolution` for a Revolve. UI and CLI copy no arithmetic.
* **Unchanged route.** `EditSketchRequest`, `edit_sketch_copy`,
  `edit_object_copy`, `Document::write_sketch_geometry`, the snapshot
  copier, the cold rebuild before publication, the check that every saved
  reference resolves, and the source/version/alias/no-clobber guards,
  cancellation, cleanup, SQLite close and atomic Keep publish are all
  reused unchanged. There is no second job and no second CLI command.

### CLI and JSON

* **Command.** `edit-sketch-copy` with request v1 is unchanged: all ordered
  `curve_id`/`start_mm`, `deny_unknown_fields`, ≤ 65536 bytes. The response
  operation, schema and exit codes 0/2/7 are unchanged.
* **Discovery.** `inspect --json` reads the same pinned snapshot and
  `content_version`. For a §27A document:
  - `sketches[].vertices` is now filled, and `editable` is true unless the
    document or structure refuses;
  - on every older model, every existing field keeps its value and type;
  - the Revolve stays out of `features`;
  - `revolves[].profile.available` keeps its meaning: the profile can be
    read, not written.
* **New field.** `sketches[].profile_feature` is additive. It is null when
  `vertices` is null, and otherwise one of:

  ```json
  {"kind": "blind_extrude", "feature_id": "…", "height_mm": 10.0}
  {"kind": "full_turn_revolve", "feature_id": "…", "body_id": "…",
   "axis": "sketch_y", "extent": "full_turn", "axis_clearance_mm": 1e-6}
  ```

  A client that knows the new field can tell which policy the job will
  apply. An old client ignores it.
* **Without a kernel.** Coordinates and IDs are available without OCCT or
  PlaneGCS. The edit itself needs OCCT.

### UI

**Edit Sketch** on a Revolve part opens the existing coordinate editor:
* selection, drag, exact coordinates, Snap and Undo/Redo work as for an
  Extrude;
* the header states the saved full turn about the sketch Y axis, and that
  X is the radius;
* the canvas draws the labelled axis;
* there is no Blind height field and no Extrude/Revolve switch.

Save Cancel, a worker refusal and a failed async Open keep the draft being
edited. Publication goes through the existing worker and Open path.

### Not in this slice

* adding, removing or reordering Lines;
* constraints and Circle/Arc profiles;
* editing the axis or the angle, and partial turns;
* touching the axis;
* Boolean/Cut on the Revolve, and face attachment;
* in-place Save and live preview.

## Executable agent recipe

Set `FERRITECAD` to a fresh native CLI. Optionally set `FCAD_UFBX_READER` to
the pinned `read_production` reader. The marked block uses only the public
CLI and its JSON, plus its own STL parser. It chooses what to edit from
`inspect --json`: the Sketch whose `profile_feature.kind` is
`full_turn_revolve` and which is `editable`. It never parses prose. It does
the following, for a bushing and a sloped part drawn in both windings:

* **Edit.** Moves the whole profile 1 mm towards the axis and 2.5 mm along
  it.
* **Identities.** The copy keeps every UUID and the Revolve entry. Every
  Line is still named once by `print-topology`.
* **Mesh check.** The copy's STL is closed, oriented, full-turn, within its
  radial and axial bounds, and within the chord band of the recipe's own
  Pappus volume.
* **Surface change.** Turns the bushing's outer cylinder into a cone and
  checks the same way.
* **Refusals.** Axis contact, a stale version, reordered UUIDs and an
  occupied output each exit 2 with nothing written.
* **Source untouched.** The source bytes never change.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-revolve-profile.md").read_text()
code = text.split("# FCAD_27B_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27b-recipe.py").write_text(code)
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-27b-recipe.py
```

```python
# FCAD_27B_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ.get("FCAD_UFBX_READER")
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27b-"))
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

def create(name, points):
    request = root / f"{name}.json"
    request.write_text(json.dumps({"request_version": 1, "points_mm": points,
                                   "axis": "sketch_y", "angle": "full_turn"}))
    out = root / f"{name}.fcad"
    run(["create-sketch-revolve", request, "-o", out, "--json"])
    return out

def editable(catalog):
    [row] = [s for s in catalog["sketches"] if s["editable"]
             and (s["profile_feature"] or {}).get("kind") == "full_turn_revolve"]
    return row

def edit(source, name, move, code=0, version=None, reorder=False, out=None):
    catalog = run(["inspect", source, "--json"])["result"]
    row = editable(catalog)
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
    quadrants = set()
    for t in triangles:
        for x, y, z in t:
            assert min(xs) - LINEAR - 1e-4 < math.hypot(x, z) < max(xs) + 1e-4
            assert min(ys) - 1e-4 < y < max(ys) + 1e-4
            if abs(x) > 1e-3 and abs(z) > 1e-3:
                quadrants.add((x > 0, z > 0))
    assert len(quadrants) == 4

def verify(source, catalog, row, out, reply, points):
    assert reply["ok"] and reply["result"]["sketch_id"] == row["sketch_id"]
    after = run(["inspect", out, "--json"])["result"]
    new = editable(after)
    assert [v["curve_id"] for v in new["vertices"]] == [v["curve_id"] for v in row["vertices"]]
    assert [v["start_mm"] for v in new["vertices"]] == points
    assert new["profile_feature"] == row["profile_feature"]
    assert after["document_id"] == catalog["document_id"]
    assert after["features"] == [] and after["bodies"] == catalog["bodies"]
    strip = lambda r: {k: v for k, v in r.items() if k != "profile"}
    assert [strip(r) for r in after["revolves"]] == [strip(r) for r in catalog["revolves"]]
    run(["validate", out])
    assert "1 shape built" in run(["rebuild", "--cold", out])
    topology = run(["print-topology", out])
    for v in new["vertices"]:
        assert f"revolve face from segment {v['curve_id']}" in topology
    assert f"{len(points)} of {len(points)} references resolved" in topology
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

checked = 0
for name, points in (("bushing", [[4, 0], [10, 0], [10, 15], [4, 15]]),
                     ("sloped", [[4, 0], [10, 0], [7, 15], [4, 15]])):
    for tag, drawn in (("ccw", points), ("cw", points[::-1])):
        source = create(f"{name}-{tag}", drawn)
        before = source.read_bytes()
        result = edit(source, f"{name}-{tag}-moved", lambda p: [p[0] - 1, p[1] + 2.5])
        verify(source, *result)
        checked += 1
        if name == "bushing":
            # The outer wall's top end moves in: the same Line, now a cone.
            top = max(y for _, y in drawn)
            result = edit(source, f"{name}-{tag}-cone",
                          lambda p: [7, p[1]] if p == [10, top] else p)
            verify(source, *result)
            checked += 1
        # Refusals, each leaving everything as it was.
        names = sorted(p.name for p in root.iterdir())
        edit(source, "axis", lambda p: [0, p[1]] if p[0] == 4 else p, code=2)
        edit(source, "stale", lambda p: p, code=2, version="0" * 64)
        edit(source, "reordered", lambda p: p, code=2, reorder=True)
        taken = root / "taken.fcad"
        taken.write_bytes(b"keep")
        edit(source, "taken", lambda p: [p[0] - 1, p[1]], code=2, out=taken)
        assert taken.read_bytes() == b"keep"
        taken.unlink()
        assert not any((root / f"{n}.fcad").exists() for n in ("axis", "stale", "reordered"))
        assert sorted(p.name for p in root.iterdir() if not p.name.endswith("-edit.json")) \
            == [n for n in names if not n.endswith("-edit.json")]
        assert source.read_bytes() == before
print("FCAD_27B_RECIPE_OK", checked, root)
```
