# §27A — full-turn Revolve / NewBody from a simple Line profile

[Executed verification and limitations](full-turn-revolve-verification.md).

The first vertical slice of Revolve from wave 5A: a person draws, or an agent
states, one closed Line profile and gets a new `.fcad` whose Body is that
profile turned once around an axis. The document stores the **intent** —
profile, axis, full turn — never a mesh, an import or an equivalent Extrude
or Boolean chain. Later editing of the Revolve, constraints, booleans on it,
face attachment, in-place Save, preview, fillets and new selection kinds are
out of scope. This slice does not complete Revolve or wave 5A.

## Contract recorded before implementation

### Coordinates, axis, units

* One untransformed XY datum, one Sketch on it. Millimetres throughout.
* On the sketch canvas **X is the radial distance** from the axis and **Y is
  the axial coordinate**.
* The axis is the sketch's **local Y axis through the datum origin**:
  `RevolveAxis::SketchY`. The angle is **exactly one full turn**, 2π:
  `RevolveExtent::FullTurn`. Both are named, typed values in the document and
  in the kernel request; no adapter supplies them as constants of its own.

### Supported profiles

A profile is accepted iff all of these hold:

* **Simple line polygon.** 3..256 vertices of one unconstrained closed Line
  polygon, validated by the same simplicity rules `PolygonExtrusion` uses:
  - no repeated vertex and no zero-length edge;
  - no collinear or backtracking vertex within 1e-6 mm;
  - no crossing or touching edges;
  - nonzero area;
  - |coordinate| ≤ 1 000 000 mm.

  Those rules are extracted into one shared function. The extrusion keeps its
  own order of checks and its messages.
* **Either winding.** Sloped walls, steps, and any translation along Y are
  accepted.
* **Strictly on the positive radial side.** Every vertex has
  `x > AXIS_CLEARANCE_MM` = 1e-6 mm. Because every edge is a straight segment
  between two such vertices, the profile then neither touches nor crosses the
  axis.
* **Refused:** zero or negative radius, touching or crossing the axis, a
  solid shaft that closes on the axis, partial angles, another axis or plane,
  several loops, construction or non-Line geometry, and constraints.

The same domain value, `FullTurnRevolution`, is checked in several places:
* the UI draft;
* the CLI request;
* the create job;
* the evaluator, again from the saved Sketch before any kernel call.

A profile the domain refuses is never published, even if the kernel could
build it.

### Persistence and capability

* **New object kind** `feature.revolve`, payload schema v1:
  `{profile, axis:"sketch_y", extent:"full_turn", operation:"new_body"}`.
* **New capability** `feature.revolve.v1`. It is required by:
  - that payload;
  - every stored `RevolveFace` reference;
  - the document's capability index.

  The Extrude capabilities are not widened.
* **Dependency edges:**
  - `revolve → sketch` (`Profile`);
  - `sketch → plane` (`Plane`);
  - `body → revolve` (`BodyTip`).

  The DAG and BodyTip rules are the ones Extrude uses; a Revolve is a feature
  for `Body.tip_feature`.
* **Old builds** (9d1a5f5 and earlier) do not know the object kind or the
  capability. They preserve the object verbatim and open the document
  read-only: a write or rebuild is refused rather than corrupting it. No SQL
  schema change.

### Topology outputs

* **Faces only.** Each profile Line raises exactly one face of revolution,
  read from the sweep run by `BRepPrimAPI_MakeRevol`
  (`revol.Revol().Shape(edge)`; see the measured reason below):
  - a Line parallel to the axis gives a cylinder;
  - a Line perpendicular to it gives an annular plane;
  - any other Line gives a cone.
* **Role.** Stored as `SemanticRole::RevolveFace { profile_segment }`, where
  `profile_segment` is the Line's saved UUID. The selection is
  `AllDerivedFrom { ancestor: segment }` or `Exact`.
* **What is not stored.**
  - There are no start/end caps: a full turn has none, and the annular end
    faces are the rotations of the radial Lines.
  - No face index, traversal order, seam vertex or edge, and no
    `ProfileJoint` name.
* **History check.** Every Line must raise exactly one face of the finished
  solid, and no face may be claimed by two Lines. Anything else refuses the
  build.
* **Reference check.** A `RevolveFace` reference resolves only against a
  Revolve's names, and an `ExtrudeSide` reference never against them. There
  is no fallback to a similar face. Creation writes one `RevolveFace`
  reference per Line, and publication requires every one to resolve after the
  cold build.

### Kernel, evaluator and cache

* **Kernel.** `GeometryKernel::revolve(RevolveRequest) → RevolveResult`
  (shape plus segment → face history). The OCCT bridge adds a new entry point,
  `fc_occt_revolve`, with its own ABI:
  - the segments (`FC_OCCT_SEGMENT_LINE` only);
  - explicit model-space axis origin and direction;
  - an explicit full-turn flag.

  No existing argument is reinterpreted. The bridge re-checks everything it
  is given: finite inputs, the axis lying in the plane, the full turn, a
  valid solid with positive volume, and a face for every edge. It also
  handles cancellation, cleanup and handle ownership. The mock and the
  unavailable kernel implement the same contract.
* **Evaluator.** Cold and cached rebuilds share one path. The cache key hashes
  all of these:
  - the kernel identity;
  - the tolerance;
  - the profile (plane, labels, coordinates);
  - the axis;
  - the extent.

  The archive stores the faces under `RevolvedFace` names keyed by segment
  UUID, never by traversal order. A restored B-Rep still has every stored
  reference checked. Nothing solved or native is written to the document.

### CLI

`ferritecad create-sketch-revolve <request.json> -o <new.fcad> [--json]`,
strict request v1 (`deny_unknown_fields`, ≤ 65536 bytes):

```json
{"request_version":1,"points_mm":[[4,0],[10,0],[10,15],[4,15]],"axis":"sketch_y","angle":"full_turn"}
```

* **Required fields.** `axis` and `angle` are required and have exactly one
  accepted value each, so the request says what it means.
* **JSON output.** `--json` uses the shared envelope with
  `operation:"create-sketch-revolve"` and result `{destination, document_id}`.
* **Exit codes.** 0 published, 2 refused, 7 report delivery failed after
  publication.
* **Publication.** It goes through the shared create job, the same way
  `create-sketch-extrude` does: checked cold build and reference check, closed
  SQLite, atomic Keep publish, cleanup and cancellation.
* **Stub builds.** A build without a kernel refuses and publishes nothing.

`inspect --json` adds a top-level `revolves` array, read from the same pinned
snapshot. Each entry is:

```json
{"feature_id", "name", "body_id", "profile_sketch_id", "plane_id",
 "axis": "sketch_y", "extent": "full_turn", "operation": "new_body",
 "profile": {"available", "refusal", "segments": [{"curve_id","start_mm","end_mm"}]}}
```

`features`, `bodies` and `sketches` keep their fields and types:
* a Revolve is not listed as a feature, so an old consumer cannot take it for
  an Extrude;
* its Body's `cut_edit` blocks refuse;
* its Sketch's editors refuse, except the coordinate editor since
  [§27B](edit-revolve-profile.md): `sketches[].editable` and `vertices` then
  describe the profile, and `profile_feature` names the Revolve.

### UI

The existing Line canvas and numeric editor gain an explicit **Revolve 360°**
action beside Extrude:
* the local Y axis is drawn and labelled "axis (Y)", and the positive radial
  side is labelled;
* the draft is checked by the same domain value.

Undo/Redo, the draft, Save Cancel, invalid input, a worker refusal and a
failed async Open behave as they do for creating an Extrude. Publication goes
through the same worker, job and Open path.

## Implementation notes

* **Where the faces come from.** On OCCT 8.0.1,
  `BRepPrimAPI_MakeRevol::Generated(edge)` returns nothing for a Line
  perpendicular to the axis on a full turn: it reports such an edge as
  deleted. So the bridge reads each Line's face from the sweep that
  `MakeRevol` itself ran (`revol.Revol().Shape(edge)`). That is the same
  history, one level down, and it is still checked:
  * exactly one face per Line;
  * that face is a face of the finished solid;
  * no face is claimed by two Lines;
  * every face of the solid is claimed.

  The contract above is otherwise unchanged.
* **Surfaces.** The bridge reports a cone surface as `FaceSurface::Cone` and
  gives the axis of a cylinder or cone (`fc_occt_surface_axis`), so tests
  can check each named face against its own Line.
* **Old editors.** `print-topology` names each reference as
  `revolve face from segment <UUID>`. The Extrude-shaped editors refuse:
  `edit_extrude`, `cut_edit*` and the constraint, circle and annulus sketch
  blocks each say they require a forward Blind Extrude/NewBody. The Line
  coordinate editor accepts this class since [§27B](edit-revolve-profile.md).

## Stepped fixture for the bundled CLI

The window scenario below starts from these files. With `FERRITECAD` set to
the bundled CLI (`FerriteCAD.app/Contents/MacOS/ferritecad` on macOS):

```sh
cat > stepped.json <<'JSON'
{"request_version":1,"points_mm":[[4,0],[10,0],[10,5],[7,5],[7,15],[4,15]],"axis":"sketch_y","angle":"full_turn"}
JSON
"$FERRITECAD" create-sketch-revolve stepped.json -o stepped-cli.fcad --json
"$FERRITECAD" export-stl stepped-cli.fcad -o stepped-cli.stl --json
"$FERRITECAD" export-fbx stepped-cli.fcad -o stepped-cli.fbx --json
```

## macOS window scenario (not run in the cloud)

1. **Start the draft.** Click `Create sketch + Extrude…`, then choose
   **Feature: Revolve 360°**.
   * The header reads "XY · mm · Line polygon · Revolve 360° about the
     sketch Y axis · NewBody" and explains that X is the radius.
   * The canvas shows an orange vertical line at X = 0 labelled
     "axis (Y) · radius X > 0 →".
   * The height field is replaced by "Revolve: one full turn (360°) about
     the sketch Y axis, through X = 0."
2. **Enter the profile.** Enter the stepped points (4,0) (10,0) (10,5) (7,5)
   (7,15) (4,15) in the numeric editor. `Create in new file…` appears.
3. **Refuse the axis.** Change (4,0) to (0,0). In place of the button, a red
   refusal says the point is "not strictly on the positive radial side".
   * Undo restores (4,0) and the button.
   * Redo brings the refusal back; Undo once more.
4. **Switch features.** Switch to **Extrude**, where the height field
   returns, and back to **Revolve 360°**. Undo/Redo step through the switch
   too, and the points are kept.
5. **Cancel the save.** `Create in new file…` → **Cancel** in the Save
   dialog. Nothing is written and the draft stays.
6. **Publish.** `Create in new file…` → `stepped-gui.fcad`. It opens with one
   Body. Choose the private test directory with the system panel's Go to
   Folder action and enter only the basename in Save As; an absolute path
   entered as a filename can become a colon-containing filename on macOS.
7. **Check the old editors.** The extrusion editor, the Cut form and
   `Edit Sketch …` are unavailable for it. Each gives the refusal
   `inspect --json` reports, for example "sketch edit requires one forward
   Blind Extrude/NewBody".
8. **Compare with the CLI fixture:**
   * `"$FERRITECAD" export-stl stepped-gui.fcad -o stepped-gui.stl` gives
     the same bytes as `stepped-cli.stl`;
   * `export-fbx` gives the same geometry as `stepped-cli.fbx`. The two new
     documents have different Body UUIDs: map only that UUID in DefinitionKey,
     DefinitionId and OccurrenceId, then compare the complete bytes. Equal
     lengths alone do not demonstrate equal geometry;
   * the `revolves[0].profile.segments` coordinates are equal in the two
     `inspect --json` outputs;
   * export the saved GUI document through both GUI and CLI: these two STL/FBX
     pairs must be byte-identical without any identity mapping.

The cloud check of the same behaviour is the test
`sketch::tests::native_revolve_draft_worker_and_cli_publish_equivalent_models`,
which drives the real widgets and worker.

## Executable agent recipe

Set `FERRITECAD` to a fresh native CLI. Optionally set `FCAD_UFBX_READER` to
the pinned `read_production` reader. The marked block uses only the public
CLI and its JSON, plus its own STL parser. It does the following:

* **Positives.** Creates the bushing, stepped and sloped profiles in both
  windings, and shifted by a fraction of a millimetre along Y.
* **Discovery.** Reads each document's `revolves` entry and checks the
  following:
  - the stated Line endpoints are exactly the request's edges;
  - `features` is empty;
  - every older editor refuses.
* **Rebuild.** `validate`, `rebuild --cold`, and `print-topology` naming one
  resolved face per Line UUID.
* **STL, parsed independently.** The mesh must be:
  - closed and consistently oriented;
  - within the profile's radial and axial band;
  - turned all the way round;
  - of a volume within the chord-deviation band of Pappus.
* **FBX.** Read by ufbx when a reader is given.
* **Refusals, each exit 2 with nothing written:**
  - touching or crossing the axis;
  - a partial angle;
  - an unknown field;
  - request v2;
  - an occupied output, whose bytes are unchanged.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/full-turn-revolve.md").read_text()
code = text.split("# FCAD_27A_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27a-recipe.py").write_text(code)
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-27a-recipe.py
```

```python
# FCAD_27A_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ.get("FCAD_UFBX_READER")
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27a-"))
LINEAR, ANGULAR = 0.05, 0.1
PROFILES = {
    "bushing": ([[4, 0], [10, 0], [10, 15], [4, 15]], math.pi * (10**2 - 4**2) * 15),
    "stepped": ([[4, 0], [10, 0], [10, 5], [7, 5], [7, 15], [4, 15]], 750 * math.pi),
    "sloped": ([[4, 0], [10, 0], [7, 15], [4, 15]], 855 * math.pi),
}

def run(args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
    if p.returncode == 7:
        raise RuntimeError("report lost: inspect the destination; do not retry blindly")
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def request(name, body):
    path = root / f"{name}.json"
    path.write_text(json.dumps(body))
    return path

def revolve(name, points, code=0, **extra):
    body = {"request_version": 1, "points_mm": points, "axis": "sketch_y",
            "angle": "full_turn", **extra}
    out = root / f"{name}.fcad"
    reply = run(["create-sketch-revolve", request(name, body), "-o", out, "--json"], code)
    return out, reply

def stl(path):
    """Own binary STL reader: triangles as vertex triples."""
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    assert len(data) == 84 + 50 * count
    return [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)]
            for i in range(count)]

def check_mesh(triangles, points, exact):
    key = lambda v: tuple(round(c, 5) for c in v)
    edges = {}
    for t in triangles:
        for a, b in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
            edges[(key(a), key(b))] = edges.get((key(a), key(b)), 0) + 1
    assert all(n == 1 and edges.get((b, a)) == 1 for (a, b), n in edges.items()), "not closed/oriented"
    volume = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                 + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in triangles) / 6
    band = sum(2 * math.pi * max(points[i][0], points[(i + 1) % len(points)][0]) * LINEAR
               * abs(points[(i + 1) % len(points)][1] - points[i][1]) for i in range(len(points)))
    assert abs(volume - exact) <= band, (volume, exact, band)
    xs, ys = [p[0] for p in points], [p[1] for p in points]
    quadrants = set()
    for t in triangles:
        for x, y, z in t:
            r = math.hypot(x, z)
            assert min(xs) - LINEAR - 1e-4 < r < max(xs) + 1e-4, (x, y, z)
            assert min(ys) - 1e-4 < y < max(ys) + 1e-4, (x, y, z)
            if abs(x) > 1e-3 and abs(z) > 1e-3:
                quadrants.add((x > 0, z > 0))
    assert len(quadrants) == 4, "not a full turn"
    return volume

def edges(points):
    return {(tuple(map(float, points[i])), tuple(map(float, points[(i + 1) % len(points)])))
            for i in range(len(points))}

checked = 0
for name, (points, exact) in PROFILES.items():
    shifted = [[x, y + 0.375] for x, y in points]
    for tag, variant in (("ccw", points), ("cw", points[::-1]),
                         ("shift", shifted), ("shift-cw", shifted[::-1])):
        out, reply = revolve(f"{name}-{tag}", variant)
        assert reply["ok"] and reply["result"]["destination"] == str(out)
        c = run(["inspect", out, "--json"])["result"]
        assert c["features"] == [], "a Revolve is never listed as an Extrude"
        assert not c["edit_extrude"]["available"]
        assert not c["bodies"][0]["cut_edit_v3"]["available"]
        assert not c["sketches"][0]["editable"]
        [rev] = c["revolves"]
        assert (rev["axis"], rev["extent"], rev["operation"]) == ("sketch_y", "full_turn", "new_body")
        assert rev["body_id"] == c["bodies"][0]["body_id"]
        prof = rev["profile"]
        assert prof["available"] and prof["refusal"] is None
        assert {(tuple(s["start_mm"]), tuple(s["end_mm"])) for s in prof["segments"]} == edges(variant)
        run(["validate", out])
        assert "1 shape built" in run(["rebuild", "--cold", out])
        topology = run(["print-topology", out])
        for s in prof["segments"]:
            assert f"revolve face from segment {s['curve_id']}" in topology
        n = len(variant)
        assert f"{n} of {n} references resolved" in topology
        mesh = root / f"{name}-{tag}.stl"
        run(["export-stl", out, "-o", mesh, "--linear-deflection", LINEAR,
             "--angular-deflection", ANGULAR, "--json"])
        check_mesh(stl(mesh), variant, exact)
        if reader and tag == "ccw":
            fbx = root / f"{name}.fbx"
            assert run(["export-fbx", out, "-o", fbx, "--json"])["result"]["complete"]
            text = subprocess.run([reader, "--identity", fbx], capture_output=True,
                                  text=True, check=True).stdout
            assert "FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0" in text
        checked += 1

bushing = PROFILES["bushing"][0]
for name, points, extra in (
    ("touch", [[0, 0], [10, 0], [10, 15], [0, 15]], {}),
    ("cross", [[-2, 0], [10, 0], [10, 15], [-2, 15]], {}),
    ("half", bushing, {"angle": "half_turn"}),
    ("unknown", bushing, {"height_mm": 10}),
    ("v2", bushing, {"request_version": 2}),
):
    out, reply = revolve(name, points, 2, **extra)
    assert not reply["ok"] and not out.exists(), (name, reply)
taken = root / "bushing-ccw.fcad"
before = taken.read_bytes()
run(["create-sketch-revolve", root / "bushing-ccw.json", "-o", taken, "--json"], 2)
assert taken.read_bytes() == before
print("FCAD_27A_RECIPE_OK", checked, root)
```
