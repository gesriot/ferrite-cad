# §28D — move or resize the rectangle under a Fillet, in a new copy

[Executed verification and limitations](edit-fillet-base-sketch-verification.md).
[The Fillet this keeps](single-edge-fillet.md);
[its radius edit](edit-fillet-radius.md); [its plate's height](edit-fillet-base-height.md).

A person opens a saved rounded plate — the §28A Fillet on one vertical edge —
changes the width, depth or position of its base rectangle in the existing
**Edit Sketch** (typed coordinates, or dragging vertices with Undo/Redo), and
saves a new `.fcad`. The rounding stays on the same named corner with the
same radius, at the rectangle's new corner. An agent gets the same copy with
the existing `edit-sketch-copy`, having found the Sketch in `inspect --json`.
There is no new command, request format or copy pipeline.

## Contract recorded before implementation

### The supported source

Exactly the frame §28B and §28C edit, read again from the saved history on
every discovery, preparation and write, by the same function
(`fillet_radius::fillet_over_plate`, over `cut_edit::saved_history_under_fillet`
and the §28A names): one untransformed XY datum; one unconstrained Sketch of
four Lines forming an axis-aligned rectangle; one forward literal Blind
`Extrude`/`NewBody`; one Fillet whose `previous` and `edge.feature` are that
Extrude and whose joint is a corner of that rectangle; one Body whose tip is
the Fillet; no Cut; exactly the plane, profile, predecessor and body-tip
dependencies; exactly the seven §28A names. The selected Sketch must be that
Fillet's profile.

The existing coordinate editor's rules still apply unchanged: the request
names every saved Line UUID once in saved order, the loop stays exactly
closed and in the same winding, and the Extrude's polygon policy holds.

### What a candidate rectangle must be

Judged by the functions the Fillet already uses, on the candidate Lines:

* it is an axis-aligned rectangle of the same four Lines
  (`rectangle_corners`);
* **every Line keeps its side**: the same axis and the same direction as
  saved (a Line running +X stays running +X). This forbids rotating or
  mirroring which Line is which side, even when the result would still be a
  rectangle of the same winding; the rounded corner is therefore the same
  corner of the part, not merely the same pair of UUIDs moved elsewhere;
* the saved joint is still a corner of it (`corner_for`), found by its two
  Line UUIDs — never by an index, a row or the nearest coordinates, so a CW
  loop or another starting Line changes nothing;
* the **saved radius** still fits that corner under §28A's unchanged policy
  (`check_radius`: `r ≤ ½ · min(adjacent Lines)`), with the numbers in the
  refusal. Nothing is clamped and the radius is never changed to fit.

Anything else is refused, typed, before any write: a non-rectangle, a Line
that changes side, a rectangle too small for the saved radius, constraints, a
reordered or reversed loop. A Line that changes side and a rectangle too small
for the radius are `input`; a non-rectangle is `unsupported`, as §28A refuses
one, and so is `--sketch` naming anything but the Fillet's base Sketch (the
Fillet itself included).

### Identity — what may change

Only the selected Sketch row's `payload` and `payload_hash`. The coordinate
writer does not stamp `meta.modified_at` (as for every coordinate edit since
§25B); the allowlist admits it, and the verification reports what actually
moves.

Preserved byte for byte: the document id; every other object row, including
the base Extrude (height), the Fillet (UUID, `previous`, edge producer and
joint, radius) and the Body; the curve UUIDs and their order; every
dependency, topology reference and capability row. Nothing is minted.

### The exact SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Sketch row's `payload` and `payload_hash` |
| `meta` | `modified_at` |
| every other table, including `deps`, `topology_refs`, `capabilities` | nothing |

Row counts are equal in every table.

### Writer, version and references

`Document::write_sketch_geometry` unchanged: inside its transaction it checks
the Sketch row is the one prepared, re-derives the edit from the new
coordinates through the same preparation (and so through the Fillet frame
and radius check against the Fillet as it is now), and compares the whole
prepared payload. A forged payload or one that no longer fits is refused.

Staleness is guarded where it is for every coordinate edit: the copy job's
`DocumentVersion` is the content hash of every row, the Fillet's payload
(radius, edge) and every topology reference included; it is checked against
the snapshot copied and again before publication.

Every saved name must resolve after the copy's cold rebuild (a coordinate
edit has always required that; the standalone height exemption is not
involved). The existing job gives snapshot, no-clobber, alias refusal,
cancellation, atomic publication and exit 7 on a lost report.

### Evaluation and cache

No new route. The base Extrude's key covers its profile's coordinates and
`eval.fillet.named` is keyed by the predecessor's key, so a moved rectangle
misses on both and neither returns the old part.

### Clients

* **`inspect --json`**, additive only. The base Sketch's `sketches[]` row
  becomes `editable` with `vertices` and `profile_feature` as for any
  editable plate, and gains `fillet_base` — the same object `features[]`
  carries since §28C (`fillet_feature_id`, `body_id`, `edge`, `corner_mm`,
  `radius_mm`). It is `null` on every other row and in every other document.
  On a Fillet outside the frame the row stays refused, naming the Fillet and
  the reason.
* **`edit-sketch-copy`**: unchanged request, result, envelope and exit codes.
  Stub order: with `--json`, UTF-8 paths; the request read and parsed; the
  source opened; then the kernel — so a well-formed request is
  `unsupported` in a stub build.
* **UI:** the existing Edit Sketch form, canvas drag and Undo/Redo, one
  context line naming the Fillet, its corner and radius and the smallest side
  that radius allows; a refusal shown with its numbers; the draft kept on a
  cancelled Save and a worker refusal; publication through the async Open.

The constraint, circle, annulus, Cut and Revolve editors, the standalone and
Cut-history Sketch routes, and a second Fillet keep their behaviour; the
constraint editor still refuses a filleted part by name.

### Compatibility

No capability, schema, payload version, cache key or archive tag. A copy
carries its source's capability rows.

### Out of scope

An arbitrary quadrilateral, constraints, reordering or reversing the loop, a
rotated plane, a second Fillet, Chamfer, a Cut with a Fillet, preview, face
picking, in-place Save. The Fillet/Chamfer milestone stays open.

## Agent recipe

The recipe below is the whole agent route, with no prior knowledge of the
document:

* create an asymmetric, translated plate with fractional sizes, and round
  one corner with `fillet-edge-copy`;
* find the base Sketch in `inspect --json` by its `fillet_base`, with the
  Fillet's edge, corner and radius, and take its UUID, its Line UUIDs in
  saved order and the version;
* move and grow the rectangle with `edit-sketch-copy`, sending every saved
  Line's new start at the same corner of the new rectangle, then shrink the
  edited copy;
* check each copy against the SQL allowlist cell by cell, the Line UUIDs and
  the Fillet's corner as it moved, then `validate` and a cold `rebuild`; read
  the STL independently (extent, volume, which corner is rounded) and check
  the FBX export;
* check that a rectangle too small for the radius, Lines turned onto other
  sides, a trapezoid, a stale version and the Fillet's own UUID are all
  refused, and that every file is left as it was.

Extract it from this file and run it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-fillet-base-sketch.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28D_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28d-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28d-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_28D_RECIPE_NO_KERNEL` with the typed refusal; a native build prints
`FCAD_28D_RECIPE_OK` with the measured and exact volumes.

```python
# FCAD_28D_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28d-"))
OP = "edit-sketch-copy"

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
            print("FCAD_28D_RECIPE_NO_KERNEL", json.dumps(error))
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

def allowlist(source, copy, sketch):
    """Only the base Sketch row's payload/payload_hash and meta.modified_at
    may differ; every table keeps its rows and every other cell."""
    sid = bytes.fromhex(sketch.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    moved = set()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc and len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u == v:
                    continue
                ok = (t == "objects" and c in ("payload", "payload_hash") and x[ac.index("id")] == sid) \
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
corner, r = [X0 + W, Y0], 2.375

def moved(rect, was, p):
    """The vertex p of the rectangle `was`, at the same corner of `rect`."""
    return [rect[0] if p[0] == was[0] else rect[0] + rect[2],
            rect[1] if p[1] == was[1] else rect[1] + rect[3]]

def measured(copy, rect, at):
    """A cold rebuild resolves every name; the mesh is the plate `rect` of
    height H with only the corner `at` rounded by r, to its tessellation."""
    x0, y0, w, d = rect
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    text = run(["rebuild", copy, "--cold"])
    assert "tip Fillet" in text and "10 of 10 stored references resolved" in text, text
    assert f"r{r} mm" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--linear-deflection", "0.01", "--json"])
    volume, points = stl(out)
    exact = (w * d - (1 - math.pi / 4) * r * r) * H
    assert exact - math.pi / 2 * r * 0.01 * H - 1e-3 <= volume <= exact + 1e-3, (volume, exact)
    xs, ys, zs = ([p[k] for p in points] for k in range(3))
    assert abs(min(xs) - x0) < 1e-4 and abs(max(xs) - x0 - w) < 1e-4, (min(xs), max(xs))
    assert abs(min(ys) - y0) < 1e-4 and abs(max(ys) - y0 - d) < 1e-4, (min(ys), max(ys))
    assert abs(min(zs)) < 1e-4 and abs(max(zs) - H) < 1e-4, (min(zs), max(zs))
    for c in ([x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]):
        for z in (0.0, H):
            near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in points)
            assert near == (c != at), (c, z)
    fbx = run(["export-fbx", copy, "-o", copy.with_suffix(".fbx"), "--json"])["result"]
    assert fbx["complete"] is True, fbx
    return volume, exact

# 1. A plate, and one saved Fillet at its lower-right corner.
create = root / "create.json"
create.write_text(json.dumps({"request_version": 1, "points_mm": CORNERS, "height_mm": H}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", create, "-o", plate, "--json"], plate)
catalog = inspect(plate)
body = catalog["bodies"][0]
chosen = next(c for c in body["fillet_edge"]["target"]["candidates"] if c["corner_mm"] == corner)
request = root / "fillet.json"
request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": r}))
rounded = root / "rounded.fcad"
geometry(["fillet-edge-copy", plate, "--body", body["body_id"], "--expect-version",
          catalog["content_version"], "--request", request, "-o", rounded, "--json"], rounded)

# 2. Discovery: the base Sketch under the Fillet is editable, with the Fillet
#    as context; its joint is named by Line UUIDs, never by a row.
catalog = inspect(rounded)
(fillet,) = catalog["fillets"]
(sketch,) = [s for s in catalog["sketches"] if s["fillet_base"] is not None]
assert sketch["editable"] is True and sketch["refusal"] is None, sketch
assert len(sketch["vertices"]) == 4, sketch
context = sketch["fillet_base"]
assert context["fillet_feature_id"] == fillet["feature_id"], context
assert context["edge"] == chosen["edge"] and context["corner_mm"] == corner, context
assert context["radius_mm"] == r and context["body_id"] == body["body_id"], context
refs = tables(rounded)["topology_refs"]

def ask(vertices, at):
    path = root / "sketch.json"
    path.write_text(json.dumps({"request_version": 1, "vertices": [
        {"curve_id": v["curve_id"], "start_mm": p} for v, p in zip(vertices, at)]}))
    return path

def redrawn(source, row, version, was, rect, name):
    """The rectangle `was` moved and resized to `rect`, corner for corner."""
    out = root / name
    before = source.read_bytes()
    at = [moved(rect, was, v["start_mm"]) for v in row["vertices"]]
    result = run([OP, source, "--sketch", row["sketch_id"], "--expect-version", version,
                  "--request", ask(row["vertices"], at), "-o", out, "--json"])["result"]
    assert result["sketch_id"] == row["sketch_id"], result
    assert source.read_bytes() == before
    changed = allowlist(source, out, row["sketch_id"])
    assert {("objects", "payload"), ("objects", "payload_hash")} <= changed, changed
    assert tables(out)["topology_refs"] == refs, "a name moved"
    after = inspect(out)
    (again,) = after["fillets"]
    assert again["feature_id"] == fillet["feature_id"] and again["radius_mm"] == r, again
    assert again["edge"] == fillet["edge"], again
    at_corner = moved(rect, was, corner if was == SAVED else row["fillet_base"]["corner_mm"])
    assert again["corner_mm"] == at_corner, (again, at_corner)
    (row2,) = [s for s in after["sketches"] if s["sketch_id"] == row["sketch_id"]]
    assert [v["curve_id"] for v in row2["vertices"]] == [v["curve_id"] for v in row["vertices"]]
    assert row2["fillet_base"]["corner_mm"] == at_corner, row2
    return out, row2, after["content_version"], at_corner

# 3. Grow and move, then shrink the edited copy.
SAVED = [X0, Y0, W, D]
UP, DOWN = [-9.25, -2.5, 51.0, 19.625], [1.375, 4.0, 18.5, 8.25]
up, up_row, up_version, up_corner = redrawn(rounded, sketch, catalog["content_version"],
                                            SAVED, UP, "up.fcad")
up_volume, up_exact = measured(up, UP, up_corner)
down, _, _, down_corner = redrawn(up, up_row, up_version, UP, DOWN, "down.fcad")
down_volume, down_exact = measured(down, DOWN, down_corner)

# 4. Refusals write nothing.
before = sorted(p.name for p in root.iterdir())
never = root / "never.fcad"
def refused(target, at, version, kind, words):
    error = run([OP, up, "--sketch", target, "--expect-version", version, "--request",
                 ask(up_row["vertices"], at), "-o", never, "--json"], 2)["error"]
    assert error["kind"] == kind and all(w in error["message"] for w in words), error
    assert not never.exists()
small = [0.0, 0.0, 20.0, 4.0]
refused(up_row["sketch_id"], [moved(small, UP, v["start_mm"]) for v in up_row["vertices"]],
        up_version, "input", ["too large"])
swapped = [v["start_mm"] for v in up_row["vertices"]]
refused(up_row["sketch_id"], swapped[1:] + swapped[:1], up_version, "input", ["keep its side"])
refused(up_row["sketch_id"], [[0, 0], [40, 0], [35, 12], [5, 12]], up_version,
        "unsupported", ["axis-aligned rectangle"])
refused(up_row["sketch_id"], swapped, catalog["content_version"], "input", [])
refused(fillet["feature_id"], swapped, up_version, "unsupported", ["requires its base Sketch"])
assert sorted(p.name for p in root.iterdir()) == before
print("FCAD_28D_RECIPE_OK", f"up={up_volume:.6f}/{up_exact:.6f}",
      f"down={down_volume:.6f}/{down_exact:.6f}", f"sketch={sketch['sketch_id']}")
```
