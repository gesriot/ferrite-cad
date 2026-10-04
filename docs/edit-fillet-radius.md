# §28B — change the radius of the saved Fillet, in a new copy

[Executed verification and limitations](edit-fillet-radius-verification.md).
[The Fillet this edits](single-edge-fillet.md).

A person opens a saved rounded plate — the §28A Fillet on one vertical edge —
changes the radius, and saves a new `.fcad`. An agent discovers the same
editable Fillet in `inspect --json` and gets the same copy from
`edit-fillet-radius`. The feature stays the same feature, rounding the same
named edge; only its radius changes.

* **UI:** **Edit Fillet radius — <UUID>…** opens a form that shows the
  Fillet, the edge it rounds (the base Extrude and the two Lines of the
  corner, and the corner's coordinates), the saved radius and the bounds.
* **CLI:**

  ```text
  ferritecad edit-fillet-radius <source.fcad> --feature UUID --expect-version HASH \
      --request <request.json> -o <copy.fcad> [--json]
  ```

  The request is v1 `{"request_version":1,"radius_mm":3.0625}`. The edge is
  the saved one and is not part of the request.

## Contract recorded before implementation

### The supported source

Exactly the §28A result, read again from the saved history on every
discovery, preparation and write — never from a cached label and never by
finding a Fillet object somewhere in the document:

* one untransformed XY datum;
* one unconstrained Sketch of four Lines forming an axis-aligned rectangle;
* one forward literal Blind `Extrude`/`NewBody`;
* one Fillet whose `previous` and `edge.feature` are that Extrude, and whose
  joint is a corner of that rectangle;
* one Body whose tip is the Fillet;
* exactly the plane, profile, predecessor and body-tip dependencies;
* exactly the seven names §28A gives the Fillet, by meaning, and no other
  name owned by it;
* every row losslessly rewritable by this reader.

The frame is checked by the same reader the Cut editors and §28A use
(`cut_edit::saved_history`). It is asked about the history *under* the
Fillet: the Fillet row and its two edges are set aside, and the Body's tip is
read as the Fillet's predecessor. It then requires zero Cuts. There is no
second frame reader.

Each of these is refused with its reason:

* a document with no Fillet, or with more than one;
* a Fillet that is not the Body's tip;
* a Cut, a Revolve, a circle or annulus, constraints, a placement, a formula
  height, a non-rectangle, several Bodies;
* a Fillet whose names are not exactly the seven §28A gives;
* an unknown field or a payload this reader could not rewrite byte for byte;
* a feature UUID that is not that Fillet.

### Identity — what may change

The only model change is `radius_mm` in the selected Fillet's payload, which
changes that row's `payload` and `payload_hash`, plus the established
`meta.modified_at` stamp.

Everything else is preserved: the document id; the UUIDs of the Body,
Extrude, Sketch, Lines and Fillet; the producer and `ProfileJoint` the Fillet
rounds; every topology reference UUID and meaning; object names, ordinals and
parents; dependencies; the Body's tip; and every capability row. Nothing is
reminted, deleted or recreated.

### The exact SQL allowlist

Between the source and the copy:

| table | allowed to differ |
| --- | --- |
| `objects` | the selected Fillet row's `payload` and `payload_hash`; nothing else in that row or any other row |
| `meta` | `modified_at` |
| every other table, including `deps`, `topology_refs`, `capabilities` | nothing |

Row counts are equal in every table.

### Radius policy

It is §28A's, unchanged and shared: `0.01 mm ≤ r ≤ ½ · min(adjacent Lines)`
of the saved corner. Nothing is clamped. A radius outside it is refused
(`input`) with the numbers, in discovery validation, in preparation, in the
writer's re-derivation and in the evaluator.

**Same radius.** Asking for the radius already saved is accepted, not
refused. It publishes a copy whose Fillet payload is byte-identical (the
same `payload_hash`); only `meta.modified_at` differs. Nothing is deleted or
recreated. This is the same rule `edit-revolve-angle` follows for an
unchanged angle.

### Writer

`Document::write_fillet_radius(prepared)` runs one checked transaction:

* It re-reads the document and re-derives the prepared value inside the
  transaction: the same content version as prepared, the same frame, the same
  Fillet and a radius the policy accepts.
* It then compares the whole prepared row with the one derived. A stale
  version, a forged payload (another edge, another predecessor, another name,
  an out-of-policy radius) or another row is refused.
* It updates exactly one `objects` row's `payload`/`payload_hash` and stamps
  `modified_at`.

### Copy job

`edit_fillet_radius_copy` runs through the existing `edit_object_copy`, for UI
and CLI alike. That gives:

* a read-only source snapshot and the expected `DocumentVersion`;
* no-clobber publication, and refusal of source and alias destinations;
* a baseline cold rebuild;
* the write;
* a strict cold rebuild in which every saved reference resolves under its
  original meaning;
* the version checked again, SQLite closed, and atomic publication;
* cancellation boundaries throughout.

The reference check covers:

* every historical name at the base Extrude;
* every `OriginCap`/`OriginSide` at the Fillet;
* the `EdgeFilletFace`.

No name is minted, so none is added to the check. A published copy whose
report is lost stays published, and the CLI exits 7. It is never retried or
rolled back.

### Cache

Nothing new is keyed. `eval.fillet.named` already includes the radius bits,
so a changed radius misses and rebuilds the Fillet, while the unchanged base
Extrude keeps its key and is reused. Cold, Miss and Hit agree.

### Clients

* **`inspect --json`** gains an additive top-level `fillets[]`, one row per
  saved Fillet, read from the existing pinned snapshot with no kernel. Each
  row has:
  * `feature_id` and `name`;
  * `body_id` and `previous_feature_id`;
  * `edge` (`feature_id`, `joint`), `corner_mm` and `radius_mm`;
  * `radius_edit`: `available`, `refusal`, `document_refusal`,
    `min_radius_mm` and `max_radius_mm`.

  No existing field, type, operation or schema changes.
* **`edit-fillet-radius`**: JSON v1, strict and bounded (65536 bytes).
  Unknown, duplicate and escape-duplicate keys, a non-object request and a
  non-number radius are refused as `input`. Another `request_version` is
  `unsupported`.
  * **Result:** `destination`, `document_id`, `body_id`, `feature_id`, the
    saved `edge`, `corner_mm`, `previous_radius_mm` and `radius_mm`. All of
    it comes from the job's own facts, with no second read or rebuild.
  * **Exit codes:** 0 published, 2 refused, 7 report lost.
  * **Order of checks in a stub build:** the request is read and parsed
    first, then the kernel is asked for. So a malformed request is `input`
    in a stub build too, and a well-formed one is `unsupported` (no Open
    CASCADE) before any structural check. Discovery works in a stub build.
* **UI:** the same discovery row and request/job.
  * The form shows the Fillet UUID, the edge meaning, the corner, the saved
    radius and the bounds.
  * Confirm draft number validates the copy draft; Save radius copy asks for a destination.
    §30G adds Apply radius to the current document independently of confirmation.
  * The draft survives a cancelled Save, a worker refusal and a stale reply.
  * Publication goes through the existing async Open, and geometry runs off
    the window thread.

### Out of scope

* a second Fillet, or retargeting to another edge;
* radius zero as deletion;
* cap-edge fillets, chains and a variable radius;
* Chamfer;
* editing the upstream Sketch or height of a filleted part;
* live preview, mouse picking and in-place Save.

The broader Fillet/Chamfer milestone stays open.

## Agent recipe

The recipe below is the whole agent route, with no prior knowledge of the
document:

* create an asymmetric, translated plate with fractional sizes, and round
  one corner with `fillet-edge-copy`;
* find the saved Fillet in `inspect --json` `fillets[]`, with its edge,
  corner, radius and bounds;
* raise the radius, then ask for the same radius, then lower it;
* check each copy against the SQL allowlist cell by cell, then run
  `validate` and a cold `rebuild`, and read the STL independently;
* check that the edge, the Fillet's UUID and every name survive every edit;
* check that an out-of-bounds radius, a strict-JSON violation, another
  request version, another feature and a stale version are all refused, and
  that every file is left as it was.

Extract it from this file and run it:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-fillet-radius.md").read_text(encoding="utf-8")
code = text.split("# FCAD_28B_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-28b-recipe.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28b-recipe.py
```

A build without Open CASCADE stops at the first geometry and prints
`FCAD_28B_RECIPE_NO_KERNEL` with the typed `unsupported` error. A complete
run prints `FCAD_28B_RECIPE_OK` with the measured volumes. No sketch solver
is involved.

```python
# FCAD_28B_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-28b-"))
OP = "edit-fillet-radius"

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
            print("FCAD_28B_RECIPE_NO_KERNEL", json.dumps(error))
            sys.exit(0)
    assert p.returncode == 0, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout)

def tables(path):
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    out = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        rows = sorted(cur.fetchall(), key=repr)
        out[t] = ([d[0] for d in cur.description], rows)
    db.close()
    return out

def allowlist(source, copy, feature):
    """Only the Fillet row's payload/payload_hash and meta.modified_at may
    differ; every table keeps its rows and every other cell."""
    fid = bytes.fromhex(feature.replace("-", ""))
    a, b = tables(source), tables(copy)
    assert a.keys() == b.keys()
    moved = set()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc and len(arows) == len(brows), t
        if t == "objects":
            key = ac.index("id")
            arows = sorted(arows, key=lambda r: r[key])
            brows = sorted(brows, key=lambda r: r[key])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                if u == v:
                    continue
                ok = (t == "objects" and c in ("payload", "payload_hash") and x[ac.index("id")] == fid) \
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
corner = [X0, Y0 + D]

def measured(copy, r):
    """A cold rebuild resolves every name; the mesh is the plate with only the
    chosen corner rounded by r."""
    assert run(["validate", copy, "--json"])["result"]["valid"] is True
    text = run(["rebuild", copy, "--cold"])
    assert "tip Fillet" in text and "10 of 10 stored references resolved" in text, text
    out = copy.with_suffix(".stl")
    run(["export-stl", copy, "-o", out, "--linear-deflection", "0.01", "--json"])
    volume, points = stl(out)
    exact = W * D * H - (1 - math.pi / 4) * r * r * H
    assert exact - math.pi / 2 * r * 0.01 * H - 1e-3 <= volume <= exact + 1e-3, (volume, exact)
    for c in CORNERS:
        near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 for p in points)
        assert near == (c != corner), c
    fbx = run(["export-fbx", copy, "-o", copy.with_suffix(".fbx"), "--json"])["result"]
    assert fbx["complete"] is True, fbx
    return volume, exact

# 1. A plate, and one saved Fillet at the upper-left corner.
create = root / "create.json"
create.write_text(json.dumps({"request_version": 1, "points_mm": CORNERS, "height_mm": H}))
plate = root / "plate.fcad"
geometry(["create-sketch-extrude", create, "-o", plate, "--json"], plate)
catalog = inspect(plate)
assert catalog["fillets"] == []
body = catalog["bodies"][0]
chosen = next(c for c in body["fillet_edge"]["target"]["candidates"] if c["corner_mm"] == corner)
request = root / "request.json"
request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": 2.375}))
rounded = root / "rounded.fcad"
geometry(["fillet-edge-copy", plate, "--body", body["body_id"], "--expect-version",
          catalog["content_version"], "--request", request, "-o", rounded, "--json"], rounded)

# 2. Discovery: the one Fillet, its edge, corner, radius and bounds.
catalog = inspect(rounded)
(row,) = catalog["fillets"]
assert row["edge"] == chosen["edge"] and row["corner_mm"] == corner, row
assert row["radius_mm"] == 2.375 and row["body_id"] == body["body_id"], row
edit = row["radius_edit"]
assert edit["available"] is True, edit
assert edit["min_radius_mm"] == 0.01 and edit["max_radius_mm"] == D / 2, edit
feature = row["feature_id"]
refs = tables(rounded)["topology_refs"]

def edited(source, version, r, name, previous):
    request.write_text(json.dumps({"request_version": 1, "radius_mm": r}))
    out = root / name
    before = source.read_bytes()
    result = run([OP, source, "--feature", feature, "--expect-version", version,
                  "--request", request, "-o", out, "--json"])["result"]
    assert result["feature_id"] == feature and result["edge"] == chosen["edge"], result
    assert result["corner_mm"] == corner and result["body_id"] == body["body_id"], result
    assert result["previous_radius_mm"] == previous and result["radius_mm"] == r, result
    assert source.read_bytes() == before
    moved = allowlist(source, out, feature)
    assert tables(out)["topology_refs"] == refs, "a name moved"
    (again,) = inspect(out)["fillets"]
    assert again["feature_id"] == feature and again["radius_mm"] == r, again
    return out, moved

# 3. Up, the same, and down: one cell pair moves, and nothing else.
up, moved = edited(rounded, catalog["content_version"], 4.8125, "up.fcad", 2.375)
assert {("objects", "payload"), ("objects", "payload_hash")} <= moved, moved
up_volume, up_exact = measured(up, 4.8125)
up_version = inspect(up)["content_version"]
same, moved = edited(up, up_version, 4.8125, "same.fcad", 4.8125)
assert moved <= {("meta", "modified_at")}, moved
down, _ = edited(up, up_version, 1.1875, "down.fcad", 4.8125)
down_volume, down_exact = measured(down, 1.1875)

# 4. Refusals write nothing.
before = sorted(p.name for p in root.iterdir())
never = root / "never.fcad"
def refused(body_text, version=up_version, target=feature, kind=None):
    request.write_text(body_text)
    error = run([OP, up, "--feature", target, "--expect-version", version,
                 "--request", request, "-o", never, "--json"], 2)["error"]
    assert kind is None or error["kind"] == kind, error
    assert not never.exists()
refused(json.dumps({"request_version": 1, "radius_mm": D / 2 + 0.5}), kind="input")
refused(json.dumps({"request_version": 1, "radius_mm": 0}), kind="input")
refused(json.dumps({"request_version": 1, "radius_mm": 3, "edge": chosen["edge"]}), kind="input")
refused(json.dumps({"request_version": 1, "radius_mm": "3"}), kind="input")
refused(json.dumps({"request_version": 2, "radius_mm": 3}), kind="unsupported")
refused(json.dumps({"request_version": 1, "radius_mm": 3}), target=body["body_id"], kind="input")
refused(json.dumps({"request_version": 1, "radius_mm": 3}), version=catalog["content_version"])
assert sorted(p.name for p in root.iterdir()) == before
print("FCAD_28B_RECIPE_OK", f"up={up_volume:.6f}/{up_exact:.6f}",
      f"down={down_volume:.6f}/{down_exact:.6f}", f"feature={feature}")
```

The current viewer also offers [§30G Apply radius](fillet-radius-session-apply.md)
on the accepted unsaved document. Confirm draft number is the copy confirmation;
Add Fillet and Save radius copy still require a clean document.
