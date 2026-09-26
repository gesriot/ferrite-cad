# §27F — edit the profile of a saved partial Revolve in a new copy

[Executed verification and limitations](edit-partial-revolve-profile-verification.md).

A person or an agent opens a saved sector (§27D) — a stepped part with a bore,
a solid cylinder or a solid cone — and moves the profile's vertices, by
dragging or with exact coordinates, in the existing **Edit Sketch** editor or
through the existing `edit-sketch-copy` command. The result is a new `.fcad`
whose Body is the same sector of the new profile. It keeps:
* the saved angle, the axis and the direction of the turn;
* every object and Line UUID;
* both `RevolveCap` names, which now name the end faces of the new profile.

The source is never written.

This extends the coordinate editor that full turns already use (§27B/§27C).
There is no new command, copier or endpoint.

## Contract recorded before implementation

### Which documents are editable

Everything the §27B/§27C full-turn profile edit accepts, now also for a
partial Revolve:

1. **Document.** It is standalone: four root objects, namely the
   untransformed XY datum, the Sketch, one Revolve of that Sketch and one
   Body.
2. **Revolve.** It turns about the sketch Y axis and is NewBody. Its extent
   is either a full turn (payload v1/v2, unchanged) or **partial** (v3 with a
   bore, v4 closed on the axis).
3. **Body.** The Body's tip is the Revolve.
4. **Dependencies.** They are exactly Sketch→plane, Revolve→Sketch
   (Profile) and Body→Revolve (BodyTip).
5. **Profile.** The Sketch holds 3–256 unconstrained, non-construction
   Lines in a closed sequence, and its payload round-trips losslessly.

It is the same shared `revolve_document` frame the §27E angle edit uses.
Only the extent check changes: the profile edit now accepts both extents,
and each is reported as its own kind.

### What an edit may change

* **The request.** It is the existing `edit-sketch-copy` request v1: every
  saved `curve_id` exactly once, in saved order, each with a new `start_mm`.
  There is no angle, axis or extent in the request. The angle is not part of
  a coordinate edit and cannot change.
* **The profile policy.** A draft is checked by the same
  `stated_revolution` policy as a full turn, against the class the Revolve
  **states**. Refused:
  * **Class changes.** A part with a bore stays one with a bore, and a solid
    part closed on the axis stays solid. There is no hollow↔solid change.
  * **The axis Line.** The saved `axis_segment` stays the same Line on
    X = 0. The axis section cannot move to another Line.
  * **The profile itself.** It may not cross the axis, touch it at a point
    or come within the clearance of it. Self-intersection, zero area and
    more than the linear limit are refused. So is changing the winding, as
    for a full turn.
* **The copy.** Only the selected Sketch row's `payload` and `payload_hash`
  change. That is the existing §25B/§27B writer rule: `meta.modified_at` is
  not stamped by a coordinate write. Everything else stays cell for cell:
  * the Revolve row (its payload, and so the angle, the axis and the
    `axis_segment`);
  * the Body and the datum;
  * every UUID, dependency and topology ref (both `RevolveCap` and every
    `RevolveFace`);
  * the capabilities;
  * `meta`.

No payload, capability or SQL schema changes. A §27E reader reads the copy.

### Discovery (JSON v1, additive)

* **Sketch rows.** A sector's Sketch row now reports `editable: true`, with
  its `vertices` and a `profile_feature` of a new kind:

  ```json
  {"kind":"partial_turn_revolve","feature_id":…,"body_id":…,"axis":"sketch_y",
   "extent":"partial_turn","angle_deg":137.5,"axis_clearance_mm":1e-6}
  {"kind":"partial_turn_revolve_axis_closed","feature_id":…,"body_id":…,
   "axis":"sketch_y","extent":"partial_turn","angle_deg":137.5,
   "axis_curve_id":…,"off_axis_clearance_mm":1e-6}
  ```

  These are new kinds, so a client that knows only the full-turn ones stops
  instead of taking a sector for a full turn. The existing kinds, fields and
  types are unchanged.
* **The catalogue.** It comes from the same pinned `ExtrudeEditSource`
  reading (`sketches[]`) with the same `content_version`. There is no second
  read of the file for the DTO.
* **Refusals.** A sector outside the class keeps `editable: false`, with a
  named refusal.
* **Outside this slice.** Other axes, operations or compositions are
  refused as before, as are constraints and Circle/Arc curves.

### Request strictness

This slice changes the `edit-sketch-copy` route, so its request gets the
same protection the §27E review added to `edit-revolve-angle`:
* **One strict decode.** The original bytes are deserialized once into the
  strict request. They never pass through an intermediate `Value` that
  would drop duplicate keys. Duplicate keys, including escaped duplicates,
  are refused at every level.
* **Objects only.** The top level and each vertex must be JSON objects: a
  struct written as an array is refused.

Other JSON commands are not touched.

### Error priority

The command checks in this order:
1. the paths (UTF-8, when `--json` is given);
2. the request's size, JSON shape and version;
3. the source's identity;
4. the kernel: a stub build refuses here with `unsupported`;
5. the job:
   1. source equals output, and an existing output;
   2. the expected version and copy access;
   3. preparation (class, UUIDs, profile);
   4. snapshot and baseline rebuild;
   5. write with re-derivation, rebuild of every name;
   6. source re-check, then a no-clobber publish.

This is unchanged from §27B.

### Spine

`replace_sketch_coordinates` → `Document::write_sketch_geometry` →
`edit_object_copy`, all unchanged in structure:
* **The writer's check.** Inside the transaction it compares the current
  Sketch row with the prepared one, derives the coordinates again through
  the same class check, and requires the whole prepared payload to be
  equal.
* **The copy spine.** It rebuilds cold and requires every saved name
  (including both caps). It re-checks the source path's version and
  publishes atomically without clobbering.

### UI

* **Starting.** For a sector, `Edit Sketch <name> — <UUID>…` becomes
  enabled.
* **The draft.** The same draft, selection/drag, exact X/Y fields and
  bounded Undo/Redo, with the same worker.
* **The header** reads "Revolve through an angle about the sketch Y axis".
* **The Angle ° field** shows the saved angle **read-only**, as the height
  of an extrusion edit is shown.
* **The text** says that the curve IDs, order, closure and the saved
  **θ° sector** about Y are retained, and names the axis Line of a solid
  part.
* **Cancel.**
  * Cancelling the Save dialog keeps the draft.
  * A refused publication keeps the draft.
  * Publication hands the draft to `draft_published`, then to
    `draft_load_finished` for the accepted async Open only.
* **Scene and title** change only when that load is accepted.

### Unchanged and out of scope

* **Unchanged:** full-turn profile edits (§27B/§27C) and the §27E angle
  edit.
* **Out of scope:**
  * full↔partial;
  * new axes and operations;
  * constraints on a Revolve profile (added for profiles with a bore by
    [§27G](revolve-profile-constraints.md));
  * Circle/Arc;
  * booleans and several bodies;
  * live preview;
  * in-place Save.
* **The cone.** Its sector mesh has T-junctions (§27D). It is checked as
  closed through T-junctions, never as a strict 2-manifold.

### What replaces the §27D gate

§27D's `partial::native_saved_partial_revolve_refuses_coordinate_editing_atomically`
asserted that every sector refuses coordinate editing. §27F replaces that
promise, deliberately, with the one above. The test becomes
`partial::native_saved_partial_revolve_refuses_coordinate_edits_outside_the_class`:
* hollow→solid is refused;
* moving the axis Line is refused;
* a moved plane is refused;
* the stated sector is kept.

The discovery assertions of §27D/§27E are replaced the same way
(`editable: true`, with the new kind). The §27D and §27E recipes are updated
accordingly.

## Executable agent recipe

Set `FERRITECAD` to a fresh native CLI. Optionally set `FCAD_UFBX_READER` to
the pinned `read_production` reader built from
`tools/unity-fbx-smoke/scripts/read_production.c`. The marked block uses
only:
* the public CLI and its JSON;
* Python's own `sqlite3`;
* its own STL parser, Pappus and shoelace;
* the reader's output.

It checks:
* **Sectors.** A stepped part with a bore, a solid cylinder and a solid
  cone, at fractional sizes and shifted along Y. Each is saved at 137.5° and
  at 220°, and its profile is edited A → B (radial and axial sizes) and back
  B → A.
* **Discovery.** The profile row is `editable`, of the
  `partial_turn_revolve*` kind, and states the angle.
* **Each A → B copy:**
  * the source is byte-identical;
  * SQL: only the Sketch row's `payload`/`payload_hash` differ, and `meta`
    is untouched;
  * the Revolve payload is byte-identical, so the angle, axis and axis Line
    are kept;
  * `validate` and `rebuild --cold` pass;
  * `print-topology` resolves as many names as the source, both caps
    included;
  * an independent STL is closed through T-junctions only and outward,
    within the chord band of Pappus(B) × θ/360, inside [0°, θ], and both
    end faces cover the profile's area;
  * with a reader, the 220° copy's FBX is read by ufbx
    (`checks=6 failures=0`), and its triangles equal the STL's.
* **B → A.** Every SQL cell equals the source's, and the default STL is
  byte-identical.
* **Profile, then angle; angle, then profile.** Both orders reach the same
  cells (except the angle edit's `modified_at`) and the same STL and FBX
  bytes.
* **Refusals** (exit 2, nothing written, the source unchanged):
  * hollow→solid;
  * the axis on another Line;
  * a duplicate version, and an escaped duplicate `start_mm`;
  * a vertex written as an array;
  * an `angle_deg` in the request;
  * a foreign curve UUID;
  * an occupied output, whose bytes are kept.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-partial-revolve-profile.md").read_text()
code = text.split("# FCAD_27F_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27f-recipe.py").write_text(code)
EXTRACT
FERRITECAD=/path/to/ferritecad FCAD_UFBX_READER=/path/to/read_production python3 ferrite-27f-recipe.py
```

```python
# FCAD_27F_AGENT_RECIPE
import json, math, os, pathlib, sqlite3, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ.get("FCAD_UFBX_READER")
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-27f-"))
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

def allowlist(source, copy, feature, changes, stamp=True):
    """Only the row `feature`'s payload/payload_hash may differ, and
    meta.modified_at when the edit stamps it (the angle edit does, a
    coordinate edit does not)."""
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
                if t == "meta" and stamp:
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

def edit_profile(source, points, out, code=0, body=None):
    c = inspect(source)
    row = c["sketches"][0]
    request = root / f"{out.stem}.json"
    ids = [v["curve_id"] for v in row["vertices"]]
    request.write_text(body if body is not None else json.dumps(
        {"request_version": 1,
         "vertices": [{"curve_id": i, "start_mm": p} for i, p in zip(ids, points)]}))
    return run(["edit-sketch-copy", source, "--sketch", row["sketch_id"],
                "--expect-version", c["content_version"], "--request", request,
                "-o", out, "--json"], code)

def comparable(path):
    """Every cell but meta.modified_at, which only the angle edit stamps."""
    all_ = cells(path)
    for r in all_["meta"]:
        r.pop("modified_at")
    return all_

# Each saved profile and its edit: radial and axial sizes change together;
# the class and the axis Line stay.
PROFILES = {
    "stepped": ([[4.25, 1.5], [10.75, 1.5], [10.75, 6.5], [7.5, 6.5], [7.5, 16.25], [4.25, 16.25]],
                [[3.75, 0.5], [11.25, 0.5], [11.25, 7.25], [8.125, 7.25], [8.125, 18.5], [3.75, 18.5]]),
    "cylinder": ([[0, -2.5], [9.75, -2.5], [9.75, 12.25], [0, 12.25]],
                 [[0, -3.75], [7.625, -3.75], [7.625, 14.5], [0, 14.5]]),
    "cone": ([[0, 0.5], [8.5, 0.5], [0, 13.75]], [[0, -1.25], [11.5, -1.25], [0, 9.875]]),
}
edited = 0
for name, (a, b) in PROFILES.items():
    solid = any(p[0] == 0 for p in a)
    for degrees in (137.5, 220):
        request = root / f"{name}-{degrees}.json"
        request.write_text(json.dumps({"request_version": 2, "points_mm": a, "axis": "sketch_y",
                                       "extent": {"kind": "angle", "degrees": degrees}}))
        source = root / f"{name}-{degrees}.fcad"
        run(["create-sketch-revolve", request, "-o", source, "--json"])
        first = inspect(source)
        row, [r] = first["sketches"][0], first["revolves"]
        use = row["profile_feature"]
        assert row["editable"] and use["angle_deg"] == degrees and use["feature_id"] == r["feature_id"]
        assert use["kind"] == ("partial_turn_revolve_axis_closed" if solid else "partial_turn_revolve")
        count = int(run(["print-topology", source]).rsplit(" of ", 1)[1].split()[0])
        kept, revolve = source.read_bytes(), revolve_payload(source)
        out = root / f"{name}-{degrees}-b.fcad"
        reply = edit_profile(source, b, out)["result"]
        assert reply["sketch_id"] == row["sketch_id"] and reply["document_id"] == first["document_id"]
        assert source.read_bytes() == kept, "the source is never written"
        allowlist(source, out, row["sketch_id"], True, stamp=False)
        assert revolve_payload(out) == revolve, "the angle, axis and axis Line are the source's"
        c = inspect(out)
        assert [v["start_mm"] for v in c["sketches"][0]["vertices"]] == b
        assert c["sketches"][0]["profile_feature"]["angle_deg"] == degrees
        run(["validate", out])
        assert "1 shape built" in run(["rebuild", "--cold", out])
        topology = run(["print-topology", out])
        assert f"{count} of {count} references resolved" in topology
        assert "revolve start cap" in topology and "revolve end cap" in topology
        mesh = out.with_suffix(".stl")
        run(["export-stl", out, "-o", mesh, "--linear-deflection", LINEAR,
             "--angular-deflection", ANGULAR, "--json"])
        check_mesh(stl(mesh), b, degrees)
        if reader and degrees == 220:
            fbx, plain = out.with_suffix(".fbx"), root / f"{out.stem}-default.stl"
            assert run(["export-fbx", out, "-o", fbx, "--json"])["result"]["complete"]
            run(["export-stl", out, "-o", plain, "--json"])
            ident = subprocess.run([reader, "--identity", fbx], capture_output=True,
                                   text=True, check=True).stdout
            assert "FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0" in ident
            fbx_matches(fbx, plain)
        # B -> A: every cell is the source's again, and so is the mesh.
        back = root / f"{name}-{degrees}-back.fcad"
        edit_profile(out, a, back)
        assert cells(back) == cells(source), "A -> B -> A"
        s1, s2 = root / f"{name}-{degrees}-s.stl", root / f"{name}-{degrees}-back.stl"
        run(["export-stl", source, "-o", s1, "--json"])
        run(["export-stl", back, "-o", s2, "--json"])
        assert s1.read_bytes() == s2.read_bytes()
        edited += 2
    # The same final intent two ways: profile then angle, angle then profile.
    source = root / f"{name}-137.5.fcad"
    feature = inspect(source)["revolves"][0]["feature_id"]
    p = root / f"{name}-p.fcad"
    edit_profile(source, b, p)
    pa = root / f"{name}-p-a.fcad"
    edit(p, feature, inspect(p)["content_version"], 220, pa)
    ap_first = root / f"{name}-a.fcad"
    edit(source, feature, inspect(source)["content_version"], 220, ap_first)
    ap = root / f"{name}-a-p.fcad"
    edit_profile(ap_first, b, ap)
    assert comparable(pa) == comparable(ap), "whichever edit came first"
    for x, y in ((pa, ap),):
        for op, ext in (("export-stl", "stl"), ("export-fbx", "fbx")):
            run([op, x, "-o", x.with_suffix("." + ext), "--json"])
            run([op, y, "-o", y.with_suffix("." + ext), "--json"])
            assert x.with_suffix("." + ext).read_bytes() == y.with_suffix("." + ext).read_bytes()

# Refusals: exit 2, nothing written, the source unchanged.
bored, solid_ = root / "stepped-137.5.fcad", root / "cylinder-137.5.fcad"
def ids_of(path):
    return [v["curve_id"] for v in inspect(path)["sketches"][0]["vertices"]]
i0, rest = ids_of(bored)[0], ids_of(bored)[1:]
tail_ = "".join(',{"curve_id":"%s","start_mm":[%s,%s]}' % (i, x, y)
                for i, (x, y) in zip(rest, PROFILES["stepped"][1][1:]))
cases = [
    (bored, [[0, 1.5], [10.75, 1.5], [10.75, 6.5], [7.5, 6.5], [7.5, 16.25], [0, 16.25]], None,
     "input", "hollow and solid"),
    (solid_, [[0, 12.25], [0, -2.5], [9.75, -2.5], [9.75, 12.25]], None, "input", "axis Line"),
    (bored, None, '{"request_version":1,"request_version":1,"vertices":[]}', "input", "duplicate"),
    (bored, None, '{"request_version":1,"vertices":[{"curve_id":"%s","start_mm":[3.75,0.5],'
                  '"start_\\u006dm":[9,9]}%s]}' % (i0, tail_), "input", "duplicate"),
    (bored, None, '{"request_version":1,"vertices":[["%s",[3.75,0.5]]%s]}' % (i0, tail_),
     "input", "object"),
    (bored, None, '{"request_version":1,"angle_deg":220,"vertices":[{"curve_id":"%s",'
                  '"start_mm":[3.75,0.5]}%s]}' % (i0, tail_), "input", "angle_deg"),
    (bored, None, '{"request_version":1,"vertices":[{"curve_id":"01a0dca8-0000-7000-8000-'
                  '000000000000","start_mm":[3.75,0.5]}%s]}' % tail_, "input", "exactly once"),
]
refused = 0
for path, points, body, kind, words in cases:
    before, names = path.read_bytes(), sorted(p.name for p in root.iterdir() if p.suffix != ".json")
    reply = edit_profile(path, points, root / "never.fcad", 2, body)
    assert reply["error"]["kind"] == kind and words in reply["error"]["message"], reply
    assert path.read_bytes() == before
    assert sorted(p.name for p in root.iterdir() if p.suffix != ".json") == names
    refused += 1
taken = root / "taken.fcad"
taken.write_bytes(b"keep")
edit_profile(bored, PROFILES["stepped"][1], taken, 2)
assert taken.read_bytes() == b"keep"
print("FCAD_27F_RECIPE_OK", edited, refused + 1, "fbx" if reader else "no-fbx-reader", root)
```
