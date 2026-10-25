# §28L — all four vertical corners of a rectangular plate, in a new copy

[Executed verification and limitations](rectangular-fillet-history-verification.md).
Builds on [§28G](sequential-edge-fillets.md) (a second Fillet),
[§28H](edit-sequential-fillet-radii.md) (radii),
[§28I](edit-two-fillet-base-height.md) (height),
[§28J](edit-two-fillet-base-sketch.md) (coordinates) and
[§28K](edit-two-fillet-base-constraints.md) (constraints).

A person rounds a **third and a fourth** different vertical corner of the same
plate, one at a time, with the existing **Fillet edge of …** form and
`fillet-edge-copy`. The existing editors of a radius, of the plate's height, of
the base Sketch's coordinates and of its Line constraints keep working on the
whole supported history. The history is Extrude → F1 → … → Fn → Body tip, with
`n` from 0 to 4. Every Fillet is a real Open CASCADE fillet of the result of
its predecessor; nothing is replaced by one operation over a rebuilt profile.
No command, request version, copier, payload, capability, archive or SQLite
schema is added: Fillet 3 and Fillet 4 mean what Fillet 2 already means.

## Contract recorded before implementation

### The supported class (unchanged)

One untransformed XY datum; one Sketch of four Lines whose stored coordinates
are an axis-aligned rectangle, unconstrained or carrying the constraint
editor's managed Line family; one forward literal Blind `Extrude`/`NewBody`;
`n ≤ 4` Fillets in one unbranched chain; no Cut. **One Fillet per joint** of
the base Lines' UUIDs, so at most four. Refused with kind and reason: a joint
rounded twice, a fifth Fillet, a branch (two Fillets with one `previous`), a
Fillet whose `previous` is neither the Extrude nor a Fillet of the chain, a
foreign producer, a non-rectangular or ambiguous profile, and everything the
earlier slices refuse. Arbitrary edges, Chamfer, Cut with Fillet, new
constraint kinds, in-place Save and live preview are out of scope; the 5C
milestone stays open.

### One reader

`fillet_radius::fillets_over_plate` is the only reader of the Fillets of a
plate and is bounded by `MAX_PLATE_FILLETS = 4`. It returns the Fillets in
history order as typed `SavedFillet`s, each with its place, its corner on the
stored Lines, its radius and the **list of every other Fillet** of the history
(`neighbours`), each with the Line it shares with this one, or none for the
opposite corner. It reads the real predecessor chain (from the Body's tip down
to the base Extrude), proves that every Fillet belongs to the one Body and
that each edge's producer is the base Extrude, and checks the exact object and
dependency sets (`cut_edit::read_history`, already general in the chain) and
the exact names by meaning: 7 per Fillet and one `OriginFilletFace` per
earlier Fillet, so `7k + k(k−1)/2` in all (7, 15, 24, 34).

`fillet::chain_below` is its pure structural half, which `evaluable_fillet` uses too:
the chain below a Fillet, walked by `previous`, bounded, cycle-safe, each
member a Fillet that rounds an edge of the base Extrude, joints distinct. There
is no `second`/`third`/`fourth` field and no check by the number of objects.
The older one-Fillet and two-Fillet shapes are projections of this list (see
*Wire*), never a second source of facts.

### Radii and gaps (policy unchanged, applied to every neighbour)

Per corner: `MIN_RADIUS_MM ≤ r ≤ MAX_RADIUS_FRACTION × shorter side`. Per pair
of **geometrically adjacent** corners (they share a Line, whatever their order
in the history): in history order, the later radius `r_b ≤ L − r_a − MIN_RADIUS_MM`
(`check_pair`). Opposite corners share no Line and add nothing. The fourth
corner closes the perimeter, so its two neighbours are both checked.

* The largest radius offered for a Fillet is the **minimum over every bound that
  applies**: its corner bound, `pair_bound(L, r_earlier)` for each earlier
  neighbour and `pair_bound_of_first(L, r_later)` (the bounded bisection of #72)
  for each later one. It is accepted by `check_radius`, and the next
  representable value above it is refused. Expressions are not reordered.
* Refusals name the guilty Fillet, joint and shared Line UUIDs.
* A change of any radius is judged against every other saved radius, earlier and
  later, so the check on the whole history is the one a rebuild applies.

### Constrained Sketch

Stored coordinates are the solver's starting guess and prove nothing. At every
cold and warm rebuild `evaluable_fillet` judges, on the **solved** Lines of the
same four UUIDs, the sides, every corner, every radius and every shared side of
the Fillet being built against all earlier ones. There is no second solved
check beside the evaluator. Free and closure-only plates may be edited with
Edit Sketch; the others with Edit constraints; removing the user's constraints
leaves the closure and returns the coordinate editor.

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

### Storage and the old reader

Fillet payload v2 and capability `feature.fillet.sequential.v1`, both written
for Fillet 2 and now for Fillet 3 and 4 unchanged: the stored meaning is "a
Fillet whose `edge.feature` is not its `previous`". The reader of `main`
(§28K) opens the document, reads every row, reports the Fillet editors
unavailable (it holds a different count of Fillets than it supports) and
**refuses** the rebuild of three or four Fillets with a typed error: it never
publishes a partial Body. This is exercised with the binary of `main` on a new
document.

### SQL allowlists (every cell outside them byte-identical)

* **fillet-edge-copy, Fillet 3 or 4:** `objects`: one new Fillet row (v2) and the
  Body row's `payload`/`payload_hash` (the tip). `deps`: `+ F_k → F_{k−1}`
  (Predecessor), `+ Body → F_k` (BodyTip), `− Body → F_{k−1}` (BodyTip).
  Output names are not SQL rows. `capabilities`: none new
  (`feature.fillet.sequential.v1` is already declared). `meta.modified_at`.
* **edit-fillet-radius:** the selected Fillet row's `payload` and `payload_hash`;
  `meta.modified_at`.
* **edit-extrude (base height):** the Extrude row's `payload` and `payload_hash`;
  `meta.modified_at`.
* **edit-sketch-copy:** the base Sketch row's `payload` and `payload_hash`;
  `meta.modified_at`.
* **edit-sketch-constraints-copy:** the base Sketch row's `schema_version`,
  `payload`, `payload_hash`, the one `sketch.constraints.v1` capability row
  by the existing contract; `meta.modified_at`. Constraint UUIDs as §28E.

Every writer re-derives its preparation from the document **inside its
transaction** and compares the whole list of Fillets; the version guard,
baseline rebuild, no new failed features, aliases, no-clobber, cancellation, cleanup
and exit 7 are the shared copy job's, unchanged.

### Wire (JSON v1, additive; no scalar changes meaning)

* `history_index` of a Fillet stays the position of that Fillet; its range is
  now 1…4.
* New `fillets[].radius_edit.neighbours`: every other Fillet of the history, in
  history order, each with `feature_id`, `history_index`, `edge`,
  `stored_corner_mm`, `radius_mm`, `shared_line_id`, `stored_shared_length_mm`
  (`null`/`null` for the opposite corner).
* New `fillet_history` beside `fillet_base` (on a base Extrude row and on the
  base Sketch row, and in the constraint editor's context): the whole history,
  `{ count, fillets: [ { fillet_feature_id, previous_feature_id, history_index,
  edge, radius_mm, corner_mm } ] }`, for one to four Fillets.
* The old shapes are projections for their supported class only:
  `radius_edit.neighbour` is non-null **only in a history of exactly two
  Fillets** (the other one) and `null` otherwise; `fillet_base` with
  `second_fillet` describes histories of one or two Fillets and is `null` for
  three or four, which `fillet_history` describes. A scalar never becomes an
  array or `null`.
* `bodies[].fillet_edge` lists only unrounded corners; a plate with four
  rounded corners gives a typed refusal.

### UI

The existing forms and workers. The candidates are the corners still sharp.
The history line and the radius editor name the selected Fillet and every
neighbour present; the form's height is bounded and scrolls, so Save and Cancel
stay reachable with four Fillets. Sketch and constraint editors retain their
existing whole-request Undo/Redo. Fillet creation and radius forms use Apply;
they do not have whole-request Undo/Redo. Save-dialog Cancel and a worker
refusal keep their draft; the explicit Cancel draft button dismisses it. A
successful copy opens asynchronously.

## Out of scope

Chamfer (§29A adds one Chamfer on a plate with nothing else over it), Cut with Fillet, arbitrary edges, a Fillet of a Fillet's edge, a
profile other than the rectangle, new constraint kinds, in-place Save, live
preview, a window run from the cloud.

## Recipe

Extract the code between the markers and run it with the real command line:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/rectangular-fillet-history.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28L_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28l-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28l-recipe.py
```

A build without Open CASCADE stops at the first geometry step and prints
`FCAD_28L_RECIPE_NO_KERNEL`. A build with Open CASCADE and without the solver
stops at the first constraint step and prints `FCAD_28L_RECIPE_NO_SOLVER`. A
build with both prints `FCAD_28L_RECIPE_OK` with the exact and the measured
volumes at each step.

```python
# FCAD_28L_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile, uuid, time
sys.path.insert(0, str(pathlib.Path.cwd() / "tools"))
from edit_requests import constraint_request, line_request, line_rule, distance, fixed as pin_rule
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28l-"))

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


def edit_args(source, feature, height, version, out):
    request = root / "expression.json"
    request.write_text(json.dumps({"request_version": 1, "changes": [
        {"kind": "set_expression", "object": feature, "slot": {"kind": "extrude_distance"}, "value": float(height), "unit": "mm"}]}))
    return ["edit", source, "--expect-version", version, "--request", request, "-o", out, "--json"]
MIN_RADIUS = 0.01

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
    """A step that needs the kernel (or, for constraints, the solver): a build
    without it refuses typed."""
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    if p.returncode == 2 and not out.exists():
        error = json.loads(p.stdout)["error"]
        if error["kind"] == "unsupported" and "Open CASCADE" in error["message"]:
            print("FCAD_28L_RECIPE_NO_KERNEL", json.dumps(error))
            sys.exit(0)
        if error["kind"] == "unsupported" and "sketch solver" in error["message"]:
            print("FCAD_28L_RECIPE_NO_SOLVER", json.dumps(error))
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

def added_fillet(source, copy, body, names):
    """§28L's allowlist for the k-th Fillet: every source cell survives except
    the Body row's payload/payload_hash and its old tip edge; the copy adds one
    object, two edges and `names` names, and stamps modified_at."""
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
        elif t == "deps":
            kept = [r for r in arows if r in brows]
            lost = [r for r in arows if r not in brows]
            assert all(t == "deps" and bid in r for r in lost), (t, lost)
            assert len(brows) - len(kept) == (2 if t == "deps" else names), t
        elif t == "meta":
            for x, y in zip(arows, brows):
                assert all(u == v or c == "modified_at" for c, u, v in zip(ac, x, y)), "meta"
        else:
            assert arows == brows, t

def only_row(source, copy, row_id, columns=("payload", "payload_hash")):
    """One row's payload and hash (and the stamp) may differ; nothing else."""
    rid = bytes.fromhex(row_id.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        assert len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows = sorted(arows, key=lambda r: r[k])
            brows = sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                ok = u == v or (t == "objects" and c in columns and x[ac.index("id")] == rid) \
                    or (t == "meta" and c == "modified_at")
                assert ok, f"{t}.{c} moved"

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
TEMPLATE = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]
ORDER = [1, 3, 0, 2]                       # the corners, in the order they are rounded
RADII = [6.12, 3.0625, 1.5, 3.0]           # 6.12 + 3.0 is within 12.25 - 0.01 on the closing Line
RECT = (X0, Y0, W, D)
ROUNDED = [(TEMPLATE[i], r) for i, r in zip(ORDER, RADII)]

def at(rect, p):
    x0, y0, w, d = rect
    return [x0 if p[0] == X0 else x0 + w, y0 if p[1] == Y0 else y0 + d]

def measured(copy, rect, rounded, height=H):
    """After reopening: valid, a cold rebuild resolves every name, and the
    independently read mesh is `rect` extruded `height` with exactly the
    `rounded` corners (template corner, radius) rounded, each by its own
    radius; the exact volume is the B-Rep's analytic one."""
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    n = len(output_names(copy))
    text = run(["rebuild", copy, "--cold"])
    assert "tip Fillet" in text and text.count(" entities=1") == n, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--linear-deflection", "0.01", "--json"])
    volume, points = stl(out)
    x0, y0, w, d = rect
    corners = [(at(rect, c), r) for c, r in rounded]
    exact = (w * d - (1 - math.pi / 4) * sum(r * r for _, r in corners)) * height
    slack = sum(math.pi / 2 * r * 0.01 * height for _, r in corners)
    assert exact - slack - 1e-3 <= volume <= exact + 1e-3, (volume, exact)
    for c in [at(rect, t) for t in TEMPLATE]:
        for z in (0.0, height):
            near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in points)
            assert near == all(c != cut for cut, _ in corners), (c, z)
    for corner, r in corners:
        sx = 1 if corner[0] == x0 else -1
        sy = 1 if corner[1] == y0 else -1
        centre = (corner[0] + sx * r, corner[1] + sy * r)
        arc = [p for p in points if 1e-6 < sx * (p[0] - corner[0]) < r - 1e-6
               and 1e-6 < sy * (p[1] - corner[1]) < r - 1e-6]
        assert len(arc) >= 6, (corner, r, len(arc))
        assert all(abs(math.hypot(p[0] - centre[0], p[1] - centre[1]) - r) < 1e-3 for p in arc), (corner, r)
    fbx = run(["export-fbx", copy, "-o", copy.with_suffix(".fbx"), "--json"])["result"]
    assert fbx["complete"] is True, fbx
    return volume, exact

def request(name, value):
    path = root / name
    path.write_text(json.dumps(value))
    return path

def fillet(source, corner, radius, out, names, code=0):
    catalog = inspect(source)
    body = catalog["bodies"][0]
    chosen = edge_at(catalog, corner)
    req = request("fillet.json", addition(catalog, "fillet", {"feature_id": chosen["edge"]["feature_id"],
                                           "joint": chosen["edge"]["joint"][::-1]}, radius))
    args = ["edit", source, "--expect-version", catalog["content_version"], "--request", req, "-o", out, "--json"]
    if code:
        reply = run(args, code)
        assert not out.exists(), "a refusal published something"
        return reply["error"]
    done = geometry(args, out)["result"]
    added_fillet(source, out, body["body_id"], names)
    return tip(inspect(out))

def rounded_row(path, index):
    return inspect(path)["fillets"][index]

# 1. A plate.
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", request("create.json", {"request_version": 1,
          "points_mm": TEMPLATE, "height_mm": H}), "-o", plate, "--json"], plate)

# 2. Four Fillets, one at a time, each on the result of the one before it.
docs, ids = [plate], []
for k, (index, radius) in enumerate(zip(ORDER, RADII)):
    out = root / f"fillet-{k + 1}.fcad"
    done = fillet(docs[-1], TEMPLATE[index], radius, out, 7 + k)
    ids.append(done["feature_id"])
    assert done["previous_feature_id"] == (ids[k - 1] if k else done["edge"]["feature_id"]), done
    docs.append(out)
    catalog = inspect(out)
    assert {f["feature_id"] for f in catalog["fillets"]} == set(ids)
    for f in catalog["fillets"]:
        slot = next(s for s in catalog["expression_slots"] if s["object"] == f["feature_id"] and s["slot"]["kind"] == "fillet_radius")
        assert slot["refusal"] is None and slot["expression"] == f["expression"]
    base = catalog["sketches"][0]


    seen = measured(out, RECT, ROUNDED[:k + 1])
results = {"four": seen}
four = docs[-1]
catalog = inspect(four)
assert len(catalog["bodies"][0]["history"]) == 5

# 3. The largest radius of the fourth Fillet is bound by the first, across the
#    Line the perimeter closes on: accepted to the float, refused one float above.
refs = output_names(four)
bound = D - RADII[0] - MIN_RADIUS
row = rounded_row(four, 3)
def edit_radius(source, feature, radius, out, code=0):
    catalog = inspect(source)
    req = request("radius.json", {"request_version": 1, "changes": [{"kind": "set_expression", "object": feature, "slot": {"kind": "fillet_radius"}, "value": radius, "unit": "mm"}]})
    args = ["edit", source, "--expect-version", catalog["content_version"], "--request", req, "-o", out, "--json"]
    return run(args, code) if code else geometry(args, out)
refused = edit_radius(four, ids[3], math.nextafter(bound, 99), root / "no.fcad", code=2)["error"]
assert refused["kind"] == "input" and ids[0] in refused["message"] and "flat" in refused["message"], refused
assert not (root / "no.fcad").exists()
exact = root / "exact.fcad"
edit_radius(four, ids[3], bound, exact)
only_row(four, exact, ids[3])
assert output_names(exact) == refs, "a name moved"
measured(exact, RECT, ROUNDED[:3] + [(ROUNDED[3][0], bound)])

# 4. The early radius, and the height: every other row and name kept.
early = root / "early.fcad"
edit_radius(four, ids[0], 2.375, early)
only_row(four, early, ids[0])
assert output_names(early) == refs
results["early"] = measured(early, RECT, [(ROUNDED[0][0], 2.375)] + ROUNDED[1:])
base_id = inspect(four)["features"][0]["feature_id"]
taller = root / "taller.fcad"
geometry(edit_args(four, base_id, "9.5", inspect(four)["content_version"], taller), taller)
only_row(four, taller, base_id)
assert output_names(taller) == refs
measured(taller, RECT, ROUNDED, 9.5)

# 5. The base rectangle, moved and resized: every corner follows its two Lines.
sketch = inspect(four)["sketches"][0]

rect = (1.5, -2.25, 30.0, 12.75)
vertices = [{"curve_id": v["curve_id"], "start_mm": at(rect, v["geometry"]["start_mm"])} for v in sketch["curves"]]
moved = root / "moved.fcad"
geometry(["edit", four, "--expect-version",
          inspect(four)["content_version"], "--request",
          request("sketch.json", line_request(sketch["sketch_id"], [v["curve_id"] for v in vertices], [v["start_mm"] for v in vertices])), "-o", moved, "--json"], moved)
only_row(four, moved, sketch["sketch_id"])
results["moved"] = measured(moved, rect, ROUNDED)

# 6. Line constraints (a solver is needed): the plate dimensioned; the solved
#    plate is the new plate, the coordinate editor is refused, and removing the
#    user's constraints returns it.
edit = sketch


LINES = [c["curve_id"] for c in edit["curves"]]
def constrain(source, remove, add, name):
    catalog = inspect(source)
    out = root / name
    reply = geometry(["edit", source, "--expect-version", catalog["content_version"], "--request",
                      request("constraints.json", constraint_request(inspect(source)["sketches"][0], remove, add)),
                      "-o", out, "--json"], out)
    return out, reply["result"]
rect_c = (0.5, 1.25, 28.0, 12.5)
dims = [line_rule(LINES[i], "horizontal" if i % 2 == 0 else "vertical") for i in range(4)] + [
    pin_rule(LINES[0], "start", 0.5, 1.25),
    distance(LINES[0], 28.0),
    distance(LINES[1], 12.5)]
dimensioned, result = constrain(four, [], dims, "dimensioned.fcad")

only_row(four, dimensioned, sketch["sketch_id"])
assert output_names(dimensioned) == refs
after = inspect(dimensioned)

results["dimensioned"] = measured(dimensioned, rect_c, ROUNDED)
user = [c["constraint_id"] for c in after["sketches"][0]["constraints"]
        if c["rule"]["kind"] != "coincident"]
freed, _ = constrain(dimensioned, user, [], "freed.fcad")

measured(freed, RECT, ROUNDED)  # the closure alone: the stored plate again

print("FCAD_28L_RECIPE_OK", " ".join(f"{k}={v[0]:.6f}/{v[1]:.6f}" for k, v in results.items()))
```
