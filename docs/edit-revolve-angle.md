# §27E — edit the angle of a saved partial Revolve in a new copy

[Executed verification and limitations](edit-revolve-angle-verification.md).

A person or an agent opens a saved sector (§27D) — for example a bushing
turned through 137.5° — and changes only its angle, say to 220°. The result
is a new `.fcad` with rebuilt geometry. It has the same objects, the same
Line UUIDs and the same two `RevolveCap` UUIDs, which now name the end faces
of the wider sector. The source file is never written.

The angle is the only intent this edit changes. It cannot:
* change a full turn into a partial one or back;
* change the axis or the direction of the turn;
* move the profile (coordinate editing of a sector's Sketch stays refused,
  as in §27D);
* touch booleans or several bodies.

It has no live preview, and there is no in-place Save. Every earlier edit and
every full-turn coordinate edit keeps its behaviour.

## Contract recorded before implementation

### Which documents are editable

A saved Revolve's angle can be edited exactly when all of the following hold:

1. **Document.** It is standalone: four root objects, namely the
   untransformed XY datum, one Sketch, one Revolve of that Sketch and one
   Body.
2. **Revolve.** The Revolve is **partial**, turns about the sketch Y axis and
   is NewBody. That is payload **v3** (a sector with a bore) or **v4** (a
   sector closed on the axis).
3. **Body.** The Body's tip is that Revolve.
4. **Dependencies.** The dependency set is exactly Sketch→plane (Plane),
   Revolve→Sketch (Profile) and Body→Revolve (BodyTip).
5. **Profile.** The profile passes the same class check every reader
   applies ([`stated_revolution`]): a closed sequence of plain Lines whose
   derived class (with a bore, or closed on the stated axis Line) is the one
   the Revolve states.
6. **Payload.** The Revolve payload round-trips losslessly through this
   reader. A payload with bytes this build would not write back is refused,
   not rewritten.
7. **Access.** The document is not read-only for copying. The document-wide
   refusal keeps its priority, as in every other edit.

The frame (items 1–4) is the one the §27B Revolve profile edit uses, shared
by the two edits rather than copied. Only the extent check differs: the
profile edit requires a full turn, and this edit requires a partial one.

Refusals are named:
* **Full-turn Revolve** (v1/v2): `unsupported`, "a full-turn Revolve has no
  angle to edit; changing between a full turn and a partial angle is not
  supported".
* **Object that is not a Revolve:** `unsupported`, naming its type.
* **UUID that does not exist** in the document: `input`.
* **Any other composition** (a second Revolve, a transformed plane, extra
  dependencies, a profile outside the class): `unsupported`, naming the
  failed requirement.

### The angle

* **Parsing.** The one domain policy, `RevolveAngle::new`, decides. There
  are no checks copied into the UI or the CLI.
* **Range.** Accepted from 0.01° to 359.99°, both inclusive. Refused: 0,
  negative angles, NaN or infinity, anything below 0.01°, anything above
  359.99°, exactly 360 (with the message "state a full turn"), and more
  than one turn.
* **No normalization.** Nothing is rounded, clamped or reduced modulo 360.
  360 never becomes a full turn, and a negative angle never flips the
  direction.
* **Stored exactly as given.** The f64 bits of the requested number are
  what the payload stores, and what the cache key feeds.

### The same angle

Requesting the saved angle is **accepted** and publishes a copy, as a height
edit to the same height does. The copy's Revolve payload and `payload_hash`
are byte-identical to the source's. Only `meta.modified_at` may differ, so
"edit" always means "a new file was written" and never silently does
nothing. The UI's Apply does not add an undo checkpoint for an unchanged
number, and Save stays enabled.

### What the copy may change

In the published copy, compared with the source:

* **Changes:**
  * `objects.payload` of the selected Revolve row (only `extent.degrees`
    inside the envelope);
  * its `objects.payload_hash`;
  * `meta.modified_at`.
* **Stays the same, cell for cell:** every other cell of every table. That
  includes:
  * `schema_version` of the row and of the document;
  * `kind`, `parent_id`, `ordinal`, `name`;
  * the Sketch, Body and datum rows;
  * every UUID, the dependencies and the topology refs;
  * the capability rows (the partial capability stays as it was);
  * `meta` other than `modified_at`.

The payload, capability and SQL schemas are unchanged: v3/v4 already carry
an arbitrary angle, and a §27D reader reads the edited copy.

### Discovery (JSON v1, additive)

Each `inspect --json` entry in `revolves[]` gains an `angle_edit` object,
from the same pinned `ExtrudeEditSource` reading and the same
`content_version` as every other catalogue:

```json
"angle_edit": {
  "available": true,
  "refusal": null,
  "document_refusal": null,
  "min_deg": 0.01,
  "max_deg": 359.99
}
```

* **`available`** is the folded answer: the Revolve's own refusal and the
  document refusal are both `null`.
* **`refusal`** is this Revolve's own reason.
* **`document_refusal`** is the shared document-wide reason, with the same
  priority as `circle_edit`/`annulus_edit`.
* **The saved angle** stays in the existing `angle_deg`. No existing field
  changes type or meaning.
* **Sketch rows** reported a sector's Sketch as `editable: false` with
  the §27D refusal. §27F makes it editable
  ([contract](edit-partial-revolve-profile.md)). The angle stays this edit's
  alone.

The Rust catalogue is additive: `ExtrudeEditSource.revolve_angles:
Vec<RevolveAngleChoice>` is computed from the same `objects()`. The shared
type keeps its name.

### Command

```
ferritecad edit-revolve-angle SOURCE --feature REVOLVE_UUID \
    --expect-version CONTENT_VERSION --request REQUEST.json -o OUTPUT.fcad [--json]
```

* **Request v1:** `{"request_version": 1, "angle_deg": 220}`.
  * It has exactly these two fields. It is strict, with `deny_unknown_fields`
    and at most 65536 bytes. Duplicate fields are rejected as input errors,
    including equivalent names written with JSON escapes.
  * The angle must be a JSON number: a string, `null`, an array or an object
    is invalid JSON input.
  * Any `request_version` other than 1 is `unsupported`.
* **Result (JSON v1 envelope, operation `edit-revolve-angle`):**
  `{"destination", "document_id", "feature_id"}`.
* **Exit codes** are unchanged: 0 published, 2 refused, 7 report lost after
  publication (the copy remains). Old commands and their result fields are
  unchanged.

### Error priority

The command checks in this order:
1. UTF-8 paths, when `--json` is given;
2. the request's size, JSON shape and version;
3. the source's identity (read-only open);
4. the kernel: a stub build refuses here with `unsupported`;
5. the job:
   1. source equals output, and an existing output;
   2. the expected version and copy access;
   3. preparation (UUID, class, angle);
   4. snapshot, cancellation, baseline rebuild;
   5. write with re-derivation, rebuild of every name;
   6. source re-check, then a no-clobber publish.

So a stub build reports malformed requests exactly as a native build does,
and reports every well-formed request as the missing kernel. The domain
refusals (full turn, angle range, stale version) are reached only natively.
The UI worker and its tests take the same job path.

### The spine

`edit_revolve_angle_copy` goes through the shared `edit_object_copy`:
* **Prepare.** `prepare_revolve_angle(document, feature, degrees)` returns
  a `PreparedRevolveAngle` holding the selected Revolve row with only the
  angle replaced, plus the source `content_version`.
* **Snapshot and baseline.** The spine snapshots, then cold-rebuilds the
  baseline. Every saved name must resolve.
* **Write.** `Document::write_revolve_angle` runs inside one transaction. Its
  check re-derives from the current document: the same `content_version`,
  then `prepare_revolve_angle` again with the prepared angle, and the result
  must equal the prepared value. After that it updates only `payload` and
  `payload_hash` of that one row (exactly one row changed) and stamps
  `meta.modified_at`. There is no capability rebuild, because the object set
  and payload versions do not change.
* **Rebuild and publish.** A cold rebuild follows, and every baseline name
  must still resolve. That means the two caps, every Line's side face and
  the Body. Then the spine closes the copy, re-checks the source path's
  version and publishes atomically without clobbering.

A forged prepared value (another angle, row or version) is caught by the
re-derivation. A changed source is caught by the version guard, both before
the snapshot and before publication.

### UI

* **Starting the edit.** Every saved Revolve gets an **Edit Revolve angle
  {name} — {uuid}…** row among the saved-object actions. A refused one is
  disabled and shows its refusal on hover, as circle rows do.
* **The form.** The window is titled **"Edit Revolve angle — new copy"**. It
  shows:
  * the feature and the source path;
  * the saved angle and the class (with a bore or closed on the axis);
  * the retained intent (the profile, the axis and the direction);
  * one **Angle °** field with exact text input.
* **Refusals in the form.** A number the domain refuses is shown in the
  form, with Save disabled.
* **Apply, Undo and Redo.** **Apply angle change** adds a checkpoint to the
  same bounded history (`DRAFT_HISTORY` = 128) as the other drafts, and
  Undo/Redo move through it. **Save edited Revolve copy…** asks for a new
  path (`edited-revolve.fcad` by default) and requires the current number to
  be applied.
* **Cancel.**
  * **Cancel draft** discards the draft.
  * Cancelling the Save dialog keeps the draft and its history.
  * Cancelling the running job keeps the draft, as for every edit.
* **Publication.**
  * Saving uses the same worker, the same `draft_published` /
    `draft_load_finished` hand-off and the same async Open of only the
    published document.
  * The accepted scene and the window title change only when that load is
    accepted.
  * If the load is refused, the draft comes back.
* **Coordinates.** Editing a sector's Sketch coordinates stays disabled,
  with the §27D refusal.

### Honesty about the mesh

The OCCT solid is valid. Its STL/FBX tessellation is closed and outward
(§27D), but the cone sector's mesh has T-junctions where the two caps meet
the axis. It is checked as closed through T-junctions, not claimed as a
strict 2-manifold.

## Executable agent recipe

Set `FERRITECAD` to a fresh native CLI. Optionally set `FCAD_UFBX_READER` to
the pinned `read_production` reader built from
`tools/unity-fbx-smoke/scripts/read_production.c`. The marked block uses
only:
* the public CLI and its JSON;
* Python's own `sqlite3`, for the cell comparison;
* its own STL parser, its own Pappus and shoelace;
* the reader's `--identity` and `--triangles` output.

It never parses prose beyond the `print-topology` count.

* **Sectors.** A stepped part with a bore, a solid cylinder and a solid
  cone:
  * fractional sizes, shifted along Y;
  * created at 137.5°, then edited 137.5 → 220 → 90 → 137.5, each copy
    from the previous one. That is 9 edited copies.
* **Each copy:**
  * the reply names the same Revolve and document;
  * the file it was made from is byte-identical;
  * SQL cells: only the Revolve row's `payload`/`payload_hash` and
    `meta.modified_at` differ;
  * `angle_deg` is the new angle and `angle_edit` is available. Since
    §27F, the Sketch is editable and states the new angle;
  * `validate` and `rebuild --cold` pass;
  * `print-topology` resolves the same number of names as the source,
    including both caps.
* **STL, parsed independently.**
  * closed (T-junctions excepted), outward, with a signed volume within the
    chord band of Pappus × θ/360;
  * nothing outside [0°, θ];
  * both end faces on their planes, facing out, covering the profile's area.
* **A → B → A.** Back at 137.5°:
  * the stored Revolve payload is byte-identical to the created one;
  * the default STL is byte-identical to the source's.
* **The same angle.** 137.5 → 137.5 publishes a copy in which no `objects`
  cell differs.
* **FBX.** With a reader, each 220° copy is read by pinned ufbx. The
  identity channel reports `checks=6 failures=0`, and its triangles equal
  the same copy's STL under (x, z, −y) · 0.001, winding included.
* **Refusals, each exit 2, nothing written, the source unchanged:**
  * a full turn and a Sketch UUID (`unsupported`);
  * an unknown UUID, a stale version, and 0°, −90°, 0.005°, 359.995°, 360°,
    400° (`input`);
  * `[1, 90]`, a string angle and an unknown field (`input`);
  * request v2 (`unsupported`);
  * an occupied output, whose bytes are kept.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-revolve-angle.md").read_text()
code = text.split("# FCAD_27E_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27e-recipe.py").write_text(code)
EXTRACT
FERRITECAD=/path/to/ferritecad FCAD_UFBX_READER=/path/to/read_production python3 ferrite-27e-recipe.py
```

```python
# FCAD_27E_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ.get("FCAD_UFBX_READER")
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27e-"))
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

def inspect(path):
    return run(["inspect", path, "--json"])["result"]

def edit(source, feature, version, degrees, out, code=0, body=None):
    request = root / f"{out.stem}.json"
    request.write_text(body if body is not None
                       else json.dumps({"request_version": 1, "angle_deg": degrees}))
    return run(["edit-revolve-angle", source, "--feature", feature, "--expect-version", version,
                "--request", request, "-o", out, "--json"], code)

def cells(path):
    """Every cell of every table, as SQLite stores it."""
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    tables = [r[0] for r in db.execute(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
    out = {}
    for t in tables:
        cur = db.execute(f'SELECT * FROM "{t}"')
        names = [d[0] for d in cur.description]
        out[t] = [dict(zip(names, row)) for row in cur.fetchall()]
    db.close()
    return out

def allowlist(source, copy, feature, changes):
    """Only the Revolve row's payload/payload_hash and meta.modified_at may differ."""
    fid = bytes.fromhex(feature.replace("-", ""))
    a, b = cells(source), cells(copy)
    assert a.keys() == b.keys()
    changed = 0
    for t in a:
        assert len(a[t]) == len(b[t]), t
        def strip(rows):
            kept = []
            for r in rows:
                r = dict(r)
                if t == "objects" and r["id"] == fid:
                    r.pop("payload"); r.pop("payload_hash")
                if t == "meta":
                    r.pop("modified_at")
                kept.append(sorted(r.items(), key=lambda kv: kv[0]))
            return sorted(kept, key=repr)
        assert strip(a[t]) == strip(b[t]), f"{t}: a cell outside the allowlist"
        if t == "objects":
            changed = sum(1 for r in a[t] if r not in b[t])
    assert changed == (1 if changes else 0), changed

def revolve_payload(path):
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    (p,) = db.execute("SELECT payload FROM objects WHERE kind='feature.revolve'").fetchone()
    db.close()
    return p

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
    return volume

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
    "stepped": [[4.25, 1.5], [10.75, 1.5], [10.75, 6.5], [7.5, 6.5], [7.5, 16.25], [4.25, 16.25]],
    "cylinder": [[0, -2.5], [9.75, -2.5], [9.75, 12.25], [0, 12.25]],
    "cone": [[0, 0.5], [8.5, 0.5], [0, 13.75]],
}
edited = 0
for name, points in PROFILES.items():
    request = root / f"{name}.json"
    request.write_text(json.dumps({"request_version": 2, "points_mm": points, "axis": "sketch_y",
                                   "extent": {"kind": "angle", "degrees": 137.5}}))
    source = root / f"{name}-137.5.fcad"
    run(["create-sketch-revolve", request, "-o", source, "--json"])
    first = inspect(source)
    [r] = first["revolves"]
    feature = r["feature_id"]
    assert r["angle_edit"] == {"available": True, "refusal": None, "document_refusal": None,
                               "min_deg": 0.01, "max_deg": 359.99}, r["angle_edit"]
    names = run(["print-topology", source])
    count = int(names.rsplit(" of ", 1)[1].split()[0])
    original_payload = revolve_payload(source)
    original_stl = root / f"{name}-source.stl"
    run(["export-stl", source, "-o", original_stl, "--json"])
    previous = source
    for degrees in (220, 90, 137.5):
        before = previous.read_bytes()
        version = inspect(previous)["content_version"]
        out = root / f"{name}-{edited}-{degrees}.fcad"
        reply = edit(previous, feature, version, degrees, out)["result"]
        assert reply["feature_id"] == feature and reply["document_id"] == first["document_id"]
        assert previous.read_bytes() == before, "the source is never written"
        allowlist(previous, out, feature, True)
        c = inspect(out)
        [r] = c["revolves"]
        assert r["angle_deg"] == degrees and r["extent"] == "partial_turn"
        assert r["angle_edit"]["available"]
        # §27F: the profile is editable and states the copy's angle.
        assert c["sketches"][0]["editable"]
        assert c["sketches"][0]["profile_feature"]["angle_deg"] == degrees
        run(["validate", out])
        assert "1 shape built" in run(["rebuild", "--cold", out])
        topology = run(["print-topology", out])
        assert f"{count} of {count} references resolved" in topology
        assert "revolve start cap" in topology and "revolve end cap" in topology
        mesh = out.with_suffix(".stl")
        run(["export-stl", out, "-o", mesh, "--linear-deflection", LINEAR,
             "--angular-deflection", ANGULAR, "--json"])
        check_mesh(stl(mesh), points, degrees)
        if reader and degrees == 220:
            fbx, plain = out.with_suffix(".fbx"), root / f"{out.stem}-default.stl"
            assert run(["export-fbx", out, "-o", fbx, "--json"])["result"]["complete"]
            run(["export-stl", out, "-o", plain, "--json"])
            ident = subprocess.run([reader, "--identity", fbx], capture_output=True,
                                   text=True, check=True).stdout
            assert "FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0" in ident
            fbx_matches(fbx, plain)
        previous = out
        edited += 1
    # A -> B -> A: the stored Revolve and the default mesh are what they were.
    assert revolve_payload(previous) == original_payload
    back = root / f"{name}-back.stl"
    run(["export-stl", previous, "-o", back, "--json"])
    assert back.read_bytes() == original_stl.read_bytes()
    # The same angle publishes a copy whose Revolve is byte-identical.
    same = root / f"{name}-same.fcad"
    edit(source, feature, first["content_version"], 137.5, same)
    allowlist(source, same, feature, False)

# Refusals: exit 2, nothing written, the source unchanged.
source = root / "stepped-137.5.fcad"
c = inspect(source)
feature, version = c["revolves"][0]["feature_id"], c["content_version"]
kept, listing = source.read_bytes(), sorted(p.name for p in root.iterdir())
full_request = root / "full.json"
full_request.write_text(json.dumps({"request_version": 2, "points_mm": PROFILES["stepped"],
                                    "axis": "sketch_y", "extent": {"kind": "full_turn"}}))
full = root / "full.fcad"
run(["create-sketch-revolve", full_request, "-o", full, "--json"])
f = inspect(full)
assert not f["revolves"][0]["angle_edit"]["available"]
listing = sorted(p.name for p in root.iterdir())
never = root / "never.fcad"
cases = [
    (full, f["revolves"][0]["feature_id"], f["content_version"], 90, None, "unsupported"),
    (source, c["sketches"][0]["sketch_id"], version, 90, None, "unsupported"),
    (source, "01a0dca8-0000-7000-8000-000000000000", version, 90, None, "input"),
    (source, feature, "00" * 32, 90, None, "input"),
    *[(source, feature, version, d, None, "input") for d in (0, -90, 0.005, 359.995, 360, 400)],
    (source, feature, version, 0, "[1, 90]", "input"),
    (source, feature, version, 0, '{"request_version":1,"angle_deg":"90"}', "input"),
    (source, feature, version, 0, '{"request_version":1,"angle_deg":90,"axis":"x"}', "input"),
    (source, feature, version, 0, '{"request_version":2,"angle_deg":90}', "unsupported"),
]
refused = 0
for path, target, v, degrees, body, kind in cases:
    before = path.read_bytes()
    reply = edit(path, target, v, degrees, never, 2, body)
    assert reply["error"]["kind"] == kind, reply
    assert path.read_bytes() == before
    names = sorted(p.name for p in root.iterdir() if not p.name.endswith(".json"))
    assert names == sorted(n for n in listing if not n.endswith(".json")), names
    refused += 1
taken = root / "taken.fcad"
taken.write_bytes(b"keep")
edit(source, feature, version, 220, taken, 2)
assert taken.read_bytes() == b"keep" and source.read_bytes() == kept
print("FCAD_27E_RECIPE_OK", edited, refused + 1, "fbx" if reader else "no-fbx-reader", root)
```
