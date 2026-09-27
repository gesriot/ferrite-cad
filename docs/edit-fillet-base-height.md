# §28C — change the height of a rounded plate, in a new copy

[Executed verification and limitations](edit-fillet-base-height-verification.md).
[The Fillet this keeps](single-edge-fillet.md);
[its radius edit](edit-fillet-radius.md).

A person opens a saved rounded plate — the §28A Fillet on one vertical edge —
changes the plate's Blind height in the existing **Edit extrusion** form, and
saves a new `.fcad`. The rounded edge is the same edge with the same radius;
only the height changed. An agent does the same with the existing
`edit-extrude`. There is no new command, request format or copy pipeline.

## Contract recorded before implementation

### The supported source

Exactly the frame §28B edits, read again from the saved history on every
discovery, preparation and write:

* one untransformed XY datum;
* one unconstrained Sketch of four Lines forming an axis-aligned rectangle;
* one forward literal Blind `Extrude`/`NewBody` whose distance is not a
  formula or parameter;
* one Fillet whose `previous` and `edge.feature` are that Extrude, whose joint
  is a corner of that rectangle and whose radius answers to §28A's policy;
* one Body whose tip is the Fillet, and no Cut;
* exactly the plane, profile, predecessor and body-tip dependencies;
* exactly the seven names §28A gives the Fillet, by meaning, and no other
  name owned by it.

This is checked by the same function the radius edit uses
(`fillet_radius::saved_fillet`, over `cut_edit::saved_history_under_fillet`
and the §28A `fillet_references`). There is no second reader, validator or
copier. The selected feature must be that Fillet's `previous`, the base
Extrude; the existing literal-Blind check (`editable_extrude`) still applies
to it.

Only the height becomes editable on this frame. Every other editor keeps
refusing a filleted part by naming the Fillet: the Sketch coordinates and
constraints, circle and annulus edits, Cut add and edit, Revolve edits and a
second Fillet. The radius edit is unchanged.

Refused, each with its reason and nothing written:

* the Fillet itself, the Sketch or any feature other than the base Extrude
  (`--feature`);
* a document whose Fillet is outside the frame above (two Fillets, a Cut, a
  foreign or extra name, a non-rectangle, a constraint, a formula height);
* a zero, negative or non-finite height (`input`, as today).

### Height policy — measured first

Probed through the product's own `GeometryKernel::fillet_edge` on pinned
OCCT 8.0.1: the 37.5 × 12.25 mm plate at (−4.5, 3.25), one corner, radii
0.01, 3.0625 and 6.125 mm (the §28A maximum), with heights from 1e-7 to
1e5 mm.

| height (mm) | every radius |
| --- | --- |
| 2e-5 … 1e5, including every h < r | valid, 7 faces, `Cylinder{r}` on a vertical axis r inward of the corner; B-Rep volume against `(W·D − (1 − π/4)·r²)·h` within 2.5e-16 relative |
| 1e-5 and below | OCCT refuses to round the edge (`kernel`) |

The limit does not depend on r, and r's own bound (`½ · min(adjacent
Lines)`) does not depend on h. So the domain adds no height bound of its own:
the policy stays the existing `edit-extrude` rule, a finite, positive
distance. A height OCCT cannot round is refused by the kernel during the
copy's strict rebuild, typed as `kernel`, and nothing is published. Nothing is
clamped.

### Identity — what may change

The only model change is the base Extrude's Blind distance, which changes
that row's `payload` and `payload_hash`, plus the established
`meta.modified_at` stamp.

Everything else is preserved byte for byte: the document id; every object
row other than that one, including the Fillet's payload (its UUID,
`previous`, edge producer and joint, and radius); every dependency; every
topology reference UUID and meaning; object names, ordinals and parents; the
Body's tip; every capability row. Nothing is minted, deleted or recreated.

### The exact SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Extrude row's `payload` and `payload_hash`; nothing else in it or in any other row |
| `meta` | `modified_at` |
| every other table, including `deps`, `topology_refs`, `capabilities` | nothing |

Row counts are equal in every table. Asking for the saved height is accepted
and publishes an identical payload; only `modified_at` may differ, as for
every other height edit.

### References

Every saved name must resolve after the copy's cold rebuild, under its
original meaning: the base Extrude's historical names, the Fillet's
`OriginCap`/`OriginSide` and its `EdgeFilletFace`. The legacy standalone
extrusion edit keeps its weaker promise (an already unresolved name may stay
unresolved); that exemption is named for a height edit with neither a Cut
history nor a Fillet, and does not extend to this one.

### Writer

`Document::write_extrude_height` takes the checked-history branch: one exact
`UPDATE objects SET payload, payload_hash` of the base row and the
`modified_at` stamp, with no rewrite of capabilities and no new names.
Inside the transaction it re-derives the whole prepared value from the
current document: the same content version, the same frame, the same Fillet
facts and the same row. A stale version, a forged row or another frame is
refused.

### Copy job, cache and evaluation

The existing `edit_extrude_copy` over `edit_object_copy`, for UI and CLI
alike: read-only snapshot and `DocumentVersion`, no-clobber, source and alias
refusal, baseline cold rebuild, the write, strict cold rebuild, the version
checked again, atomic publication, cancellation boundaries, exit 7 on a lost
report without rollback.

No new evaluation, history, cache or FFI route. The base Extrude's key
covers its distance, and `eval.fillet.named` is keyed by the predecessor's
key, so a changed height misses on both; unchanged inputs (datum, Sketch)
are not invalidated.

### Clients

* **`inspect --json`**, additive only:
  * `edit_extrude.available` becomes `true` on a supported rounded plate, and
    stays `false` with the Fillet's reason otherwise.
  * The base Extrude's `features[]` row is `editable` and gains
    `fillet_base`: `fillet_feature_id`, `body_id`, `edge` (`feature_id`,
    `joint`), `corner_mm` and `radius_mm`. It is `null` on every other row
    and in every other document. It is not a Cut history, and the
    `base_height_edit*` fields stay `null`.
  * No existing field, type or value changes meaning.
* **`edit-extrude`**: unchanged arguments, text and JSON result, envelope and
  exit codes (0 published, 2 refused, 7 report lost).
  * **Order of checks in a stub build:** with `--json`, UTF-8 paths first;
    then the source is read (an unreadable one is refused as today); then the
    kernel is asked for. So every well-formed request, whatever its height or
    feature, is `unsupported` (no Open CASCADE). Discovery works there.
* **UI:** the existing **Edit extrusion** form and job, and async Open. The
  base row shows the Fillet as context ("Rounded by Fillet … at (x, y),
  r … mm; the Fillet keeps its edge and radius"). The draft survives a
  cancelled Save, a worker refusal and a stale reply.

### Compatibility

No capability, schema, payload version or archive tag is added. A copy
carries the same capability rows as its source, so any reader that opens a
§28A copy opens this one the same way.

### Out of scope

* editing the profile coordinates, the radius here, the edge, constraints or
  any other parameter;
* a second Fillet, cap edges, a Cut with a Fillet, a Revolve, an arbitrary
  plane, deleting or retargeting the Fillet;
* live preview, mouse picking and in-place Save.

The broader Fillet/Chamfer milestone stays open.

## Agent recipe

The recipe below is the whole agent route, with no prior knowledge of the
document:

* create an asymmetric, translated plate with fractional sizes, and round
  one corner with `fillet-edge-copy`;
* find the base Extrude in `inspect --json` by its `fillet_base`, with the
  Fillet's edge, corner and radius, and take its UUID and the version;
* raise the height with `edit-extrude`, then lower the edited copy below the
  radius;
* check each copy against the SQL allowlist cell by cell, then `validate`
  and a cold `rebuild`; read the STL independently and check the FBX export;
* check that the Fillet row, its edge and radius, and every name survive;
* check that a zero or negative height, the Fillet's own UUID, a stale
  version and a height OCCT cannot round are all refused, and that every file
  is left as it was.

Extract it from this file and run it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-fillet-base-height.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28C_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28c-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28c-recipe.py
```

A build without Open CASCADE stops at the first geometry and prints
`FCAD_28C_RECIPE_NO_KERNEL` with the typed `unsupported` error. A complete
run prints `FCAD_28C_RECIPE_OK` with the measured volumes. No sketch solver
is involved.

```python
# FCAD_28C_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28c-"))
OP = "edit-extrude"

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
            print("FCAD_28C_RECIPE_NO_KERNEL", json.dumps(error))
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

def allowlist(source, copy, base):
    """Only the base Extrude row's payload/payload_hash and meta.modified_at
    may differ; every table keeps its rows and every other cell."""
    bid = bytes.fromhex(base.replace("-", ""))
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
                ok = (t == "objects" and c in ("payload", "payload_hash") and x[ac.index("id")] == bid) \
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
corner, r = [X0 + W, Y0 + D], 2.375

def measured(copy, h):
    """A cold rebuild resolves every name; the mesh is the plate of height h
    with only the chosen corner rounded by r, to its tessellation."""
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    text = run(["rebuild", copy, "--cold"])
    assert "tip Fillet" in text and "10 of 10 stored references resolved" in text, text
    assert f"r{r} mm" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--linear-deflection", "0.01", "--json"])
    volume, points = stl(out)
    exact = (W * D - (1 - math.pi / 4) * r * r) * h
    assert exact - math.pi / 2 * r * 0.01 * h - 1e-3 <= volume <= exact + 1e-3, (volume, exact)
    zs = [p[2] for p in points]
    assert abs(min(zs)) < 1e-4 and abs(max(zs) - h) < 1e-4, (min(zs), max(zs), h)
    for c in CORNERS:
        for z in (0.0, h):
            near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 and abs(p[2] - z) < 1e-4
                       for p in points)
            assert near == (c != corner), (c, z)
    fbx = run(["export-fbx", copy, "-o", copy.with_suffix(".fbx"), "--json"])["result"]
    assert fbx["complete"] is True, fbx
    return volume, exact

# 1. A plate, and one saved Fillet at its upper-right corner.
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

# 2. Discovery: the base Extrude under the Fillet, editable, with the Fillet
#    as context. It is no Cut history.
catalog = inspect(rounded)
assert catalog["edit_extrude"]["available"] is True, catalog["edit_extrude"]
(fillet,) = catalog["fillets"]
(base,) = [f for f in catalog["features"] if f["fillet_base"] is not None]
assert base["editable"] is True and base["distance_mm"] == H, base
assert base["base_height_edit"] is None and base["base_height_edit_v3"] is None
context = base["fillet_base"]
assert context["fillet_feature_id"] == fillet["feature_id"], context
assert context["edge"] == chosen["edge"] and context["corner_mm"] == corner, context
assert context["radius_mm"] == r and context["body_id"] == body["body_id"], context
refs = tables(rounded)["topology_refs"]

def raised(source, version, h, name):
    out = root / name
    before = source.read_bytes()
    result = run([OP, source, "--feature", base["feature_id"], "--distance-mm", h,
                  "--expect-version", version, "-o", out, "--json"])["result"]
    assert result["feature_id"] == base["feature_id"], result
    assert result["document_id"] == catalog["document_id"], result
    assert source.read_bytes() == before
    moved = allowlist(source, out, base["feature_id"])
    assert {("objects", "payload"), ("objects", "payload_hash")} <= moved, moved
    assert tables(out)["topology_refs"] == refs, "a name moved"
    after = inspect(out)
    (row,) = [f for f in after["features"] if f["feature_id"] == base["feature_id"]]
    assert row["distance_mm"] == h and row["fillet_base"] == context, row
    (again,) = after["fillets"]
    assert again["feature_id"] == fillet["feature_id"] and again["radius_mm"] == r, again
    assert again["edge"] == fillet["edge"], again
    return out, after["content_version"]

# 3. Up, then down below the radius, from the edited copy.
up, up_version = raised(rounded, catalog["content_version"], 11.4375, "up.fcad")
up_volume, up_exact = measured(up, 11.4375)
down, _ = raised(up, up_version, 1.1875, "down.fcad")
down_volume, down_exact = measured(down, 1.1875)

# 4. Refusals write nothing.
before = sorted(p.name for p in root.iterdir())
never = root / "never.fcad"
def refused(feature, h, version, kind):
    error = run([OP, up, "--feature", feature, "--distance-mm", h, "--expect-version",
                 version, "-o", never, "--json"], 2)["error"]
    assert error["kind"] == kind, error
    assert not never.exists()
refused(base["feature_id"], "0", up_version, "input")
refused(base["feature_id"], "-1", up_version, "input")
refused(fillet["feature_id"], "9", up_version, "unsupported")
refused(base["feature_id"], "9", catalog["content_version"], "input")
refused(base["feature_id"], "0.000001", up_version, "kernel")
assert sorted(p.name for p in root.iterdir()) == before
print("FCAD_28C_RECIPE_OK", f"up={up_volume:.6f}/{up_exact:.6f}",
      f"down={down_volume:.6f}/{down_exact:.6f}", f"base={base['feature_id']}")
```
