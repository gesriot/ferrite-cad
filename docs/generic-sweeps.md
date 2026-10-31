# Extrude and Revolve

Both sweeps support NewBody, Add, Cut and Intersect. NewBody has no predecessor;
other operations consume the saved `previous` feature. The kernel combines the target
and the swept tool with Fuse, Cut or Common. A result contains the complete nonempty
set of valid solids, each with finite positive volume. Disconnected additions and cuts
splitting the target keep every piece. Empty intersections and a Cut that removes no
material still refuse. [KeepSolids](keep-solids.md) explicitly selects pieces by named
face witnesses; [ADR 0008](decisions/0008-multibody-results.md) defines the contract.

Extrude supports Blind and Symmetric in either normal direction for all four operations.
Symmetric extends the specified distance on each side of the sketch plane.
ThroughAll applies to Cut and Intersect: the target's bounds projected onto the sweep
direction determine the far end plus a named 1 mm margin. The sketch plane may differ
from the body's first plane. A target entirely behind the plane refuses with “nothing
ahead”. NewBody and Add refuse ThroughAll in validation. Cached rebuilds use the target
key and saved intent without asking the kernel for bounds.

Revolve supports full and partial turns around SketchX, SketchY or any main or
construction Line in its profile sketch. A Line axis is infinite, directed from start
to end; positive partial angles follow the right hand rule. Profiles contain closed Line/Arc
chains and Circles, grouped into regions by even/odd nesting. The kernel decides axis contact
and rejects invalid geometry. There is no persisted axis segment or profile class.
A profile edge collapsing on the axis produces no face and receives no invented face
name. Complete sweep and boolean provenance follows ADR 0007; ambiguity refuses an
exact consumed selection.

[Edit requests](edit-transactions.md) add sweeps, create bodies and change operation,
direction, axis or extent through the shared transaction. An edit is accepted only
when no previously built feature becomes failed or skipped. Undo/Redo uses this same
candidate and rebuild path. Unconsumed topology may disappear; references are never
silently retargeted. [Fillet and Chamfer](fillets-and-chamfers.md) consume these names without profile
classification or corner policies.

The existing create commands are convenience templates. `create-sketch-revolve` uses
local SketchY; the generic `edit` request supports all axes. Existing window forms
continue to create and edit their saved features through the shared library; there are
no additional generic sweep forms. Constraint recipes remain in
[Revolve profiles](revolve-profile-constraints.md) and
[axis-contact profiles](axis-closed-revolve-constraints.md).

The file format is unstable. Revolve stores `previous`, `operation`, `axis` and `extent`;
fixtures must be regenerated. There are no migrations or compatibility fallbacks.

## Sketch regions

Every main curve belongs to one closed boundary. Construction geometry is excluded.
Disjoint boundaries use even/odd nesting: an outer boundary and its immediate holes
form a region; an island inside a hole forms another region. Intersecting, touching,
self-intersecting and open boundaries fail with the offending curve identities.
Line/Arc/Circle intersections and ray parity are analytic, without tessellation or
sampled containment. The existing endpoint join tolerance is unchanged.

Extrude and Revolve store `regions: Vec<StableEntityId>`. Empty selects every region.
Each entry identifies a curve on a selected region's outer boundary; multiple entries
on one outer boundary select it once. Missing or construction curves and curves on
holes refuse by identity. Reordering, moving or resizing a contour does not retarget
its selection. The sweep produces one face per selected region, with complete curve
history. NewBody, tools and final boolean results may contain several solids;
no component is silently discarded.

The edit JSON feature for `add_feature` or `add_body` accepts `"regions": ["curve-uuid"]`
for either sweep. Omitting this CLI field selects all regions. To replace an existing
selection use `{"kind":"set_regions","object":"feature-uuid","regions":["curve-uuid"]}`;
`regions: []` selects all again. The stored field is required: regenerate fixtures,
without a document migration or version branch. Cache keys include the selected
profile; the `brep.generative.v2` archive stores the complete result set. Existing window forms
select all regions and continue through the shared transaction.

## Referenced end conditions

All four Extrude operations also accept `up_to_face` and `up_to_vertex`.
The `result` UUID names the feature whose result is consumed; the reference's
`producer` is the feature that minted its name, which may be an earlier feature.
The existing `reversed` flag fixes the direction. Neither condition changes direction
or picks another entity to make a feature build. Revolve is unchanged; “up to next”
remains separate work.

`up_to_face` uses the entire geometric surface, beyond the trimmed face boundary.
It ends at the first forward intersection from every point of the profile. Its signed
`offset` follows the face's outward normal in its solid. A positive offset from the
lower face of a ceiling therefore lowers the end. The kernel constructs curved offset
surfaces; no radius or coordinate rule substitutes for that operation.

Coverage is topological. The kernel splits an auxiliary long prism at the surface
and retains each solid containing a whole starting cap. No retained solid may touch
any surviving part of the auxiliary far cap. Bounds only size the auxiliary prism;
there are no sampled rays or centre-point coverage decisions. Every retained solid
must be valid with finite positive volume. Separate regions give separate solids, as
with Blind, and a new body retains all of them. The new far caps retain the ordinary
sweep end-cap provenance, including region refinement; auxiliary geometry creates no
names. Blind and a parallel face at the same height have identical names, including
edges consumed by Fillet.

`up_to_vertex` projects the named vertex onto the fixed extrusion direction. The
finite distance must be strictly positive; the resulting perpendicular end plane
uses the ordinary Blind sweep and names. Only `selection: "one"` is accepted.

A face reference with `all_descendants` is explicit: planar members must share a
plane and outward normal; curved members must share the kernel surface, location
and orientation. The common resolver runs before cache lookup. Cache keys include
the supporting result's key, reference meaning and offset, preserving the existing
Blind, Symmetric and ThroughAll keys. Each referenced condition has exactly one
`support` dependency on `result`. Changing an existing end condition updates that
row and checks new targets and cycles in the transaction.

Refusals retain their categories:

- `topology`: absent producer, unknown name, deleted face/vertex, or an exact face
  reference split into multiple faces; the consumer and diagnostic reference UUID
  identify the failed consumption.
- `input`: `end surface does not cover the profile ahead of the sketch plane`
  (including parallel or behind surfaces), `face descendants do not lie on one surface`,
  `end vertex is not ahead of the sketch plane`, wrong kind or vertex-family selection.
- `kernel`: a failed native operation or an invalid kernel result.

An edit that newly breaks a feature refuses atomically with every failure reason.
A saved broken document still opens for repair: that feature fails, dependents are
skipped, and independent features build. There is no silent reference retargeting.

```json
{"request_version":1,"changes":[
  {"kind":"set_end_condition","object":"<Extrude UUID>","end_condition":{
    "kind":"up_to_face","result":"<support feature UUID>",
    "face":{"producer":"<minting feature UUID>","kind":"face",
            "digest":"<64 hex digits>","selection":"one"},
    "offset":{"value":2,"unit":"mm"}}}
]}
```

The vertex form is `{"kind":"up_to_vertex","result":"<feature UUID>",
"vertex":{"producer":"<minting feature UUID>","kind":"vertex",
"digest":"<64 hex digits>","selection":"one"}}`. `set_expression` with slot
`{"kind":"extrude_end_offset"}` changes a face condition's signed offset.
`inspect --json` includes the complete reference and its diagnostic `id`, and the
unitful offset. `rebuild --cold` and text inspection say “up to face …, offset … mm”
or “up to vertex …”. A Face datum on the end cap exposes its computed origin.

The window draws the evaluated shape. Its existing Cut editor shows
“this feature's end condition is edited through `edit`” for these conditions.
Viewport creation of referenced end conditions is separate work.

## Executable referenced-end recipe

Set `FERRITECAD` to the built CLI. Face and vertex identities below come from their
reported sweep derivations, never coordinate searches.

```python
# UP_TO_END_RECIPE
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import uuid

cli = os.environ["FERRITECAD"]
root = Path(tempfile.mkdtemp(prefix="up-to-end-"))


def call(*args, code=0):
    result = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
    assert result.returncode == code, (args, result.stdout, result.stderr)
    return json.loads(result.stdout) if "--json" in args else result.stdout


def identity():
    raw = bytearray(int(time.time() * 1000).to_bytes(6, "big") + os.urandom(10))
    raw[6] = (raw[6] & 15) | 112
    raw[8] = (raw[8] & 63) | 128
    return str(uuid.UUID(bytes=bytes(raw)))


def mm(value):
    return {"value": value, "unit": "mm"}


def edit(path, target, changes, code=0):
    version = call("inspect", path, "--json")["result"]["content_version"]
    request = root / "request.json"
    request.write_text(json.dumps({"request_version": 1, "changes": changes}))
    return call("edit", path, "--expect-version", version, "--request", request,
                "--output", target, "--json", code=code)


def sketch(object_id, plane):
    return {"kind": "add_sketch", "object": object_id, "plane": plane,
            "curves": [{"id": identity(), "construction": False,
                        "geometry": {"kind": "circle", "radius": mm(3),
                                     "center": {"x": mm(25), "y": mm(20)}}}],
            "constraints": []}


def extrusion(profile, operation):
    return {"kind": "extrude", "profile": profile, "operation": operation,
            "reversed": False, "end_condition": {"kind": "blind", "distance": mm(4)}}


source = root / "plate.fcad"
call("create", source, "--sample", "--size", 60, 40, 10, "--json")
probe = subprocess.run([cli, "rebuild", str(source), "--cold"],
                       capture_output=True, text=True)
if probe.returncode:
    text = probe.stdout + probe.stderr
    assert "Open CASCADE" in text and "unsupported" in text.lower(), text
    print("UP_TO_END_NO_KERNEL unsupported kernel")
    raise SystemExit(0)


def reference(path, producer, kind, witness):
    lines = call("print-topology", path).splitlines()
    found = []
    pattern = rf"producer={producer} kind={kind} digest=([0-9a-f]{{64}}) entities=1"
    for i, line in enumerate(lines[:-1]):
        match = re.search(pattern, line)
        if match and witness in lines[i + 1]:
            found.append({"producer": producer, "kind": kind, "digest": match[1],
                          "selection": "one"})
    unique = {row["digest"]: row for row in found}
    assert len(unique) == 1, (kind, witness, found)
    return next(iter(unique.values()))


facts = call("inspect", source, "--json")["result"]
base = facts["features"][0]["feature_id"]
body = facts["bodies"][0]["body_id"]
top = reference(source, base, "face", "end cap, producer")
plane, profile, boss = identity(), identity(), identity()
roof_plane, roof_profile, roof = identity(), identity(), identity()
roof_sketch = sketch(roof_profile, roof_plane)
roof_sketch["curves"][0]["geometry"]["radius"] = mm(10)
boss_feature = extrusion(profile, "add")
boss_feature["end_condition"]["distance"] = mm(20)
roof_feature = extrusion(roof_profile, "new_body")
roof_feature["end_condition"]["distance"] = mm(3)
blind = root / "blind.fcad"
edit(source, blind, [
    {"kind": "add_datum_plane", "object": plane, "plane": {
        "kind": "face", "result": base, "face": top, "offset": mm(0)}},
    sketch(profile, plane),
    {"kind": "add_feature", "object": boss, "body": body, "feature": boss_feature},
    {"kind": "add_datum_plane", "object": roof_plane, "plane": {
        "kind": "offset", "from": plane, "distance": mm(20)}},
    roof_sketch,
    {"kind": "add_body", "object": identity(), "feature_object": roof,
     "feature": roof_feature}])
face = reference(blind, roof, "face", "start cap, producer")
vertex = reference(blind, roof, "vertex", "start cap, producer")
cap = reference(blind, boss, "face", "end cap, producer")
probe_plane = identity()
condition = {"kind": "up_to_face", "result": roof, "face": face, "offset": mm(0)}
ended = root / "ended.fcad"
edit(blind, ended, [
    {"kind": "set_end_condition", "object": boss, "end_condition": condition},
    {"kind": "add_datum_plane", "object": probe_plane, "plane": {
        "kind": "face", "result": boss, "face": cap, "offset": mm(0)}}])


def height(path, z, kind):
    facts = call("inspect", path, "--json")["result"]
    saved = next(row for row in facts["features"] if row["feature_id"] == boss)
    assert saved["end_condition"]["kind"] == kind
    field = "face" if kind == "up_to_face" else "vertex"
    assert saved["end_condition"][field]["id"]
    datum = next(row for row in facts["datum_planes"] if row["object"] == probe_plane)
    assert abs(datum["computed"]["origin"]["z"] - z) < 1e-6, datum
    assert kind.replace("_", " ") in call("rebuild", path, "--cold")


height(ended, 30, "up_to_face")
offset = root / "offset.fcad"
edit(ended, offset, [{"kind": "set_expression", "object": boss,
                    "slot": {"kind": "extrude_end_offset"}, "value": 2, "unit": "mm"}])
height(offset, 28, "up_to_face")
vertex_end = root / "vertex.fcad"
edit(offset, vertex_end, [{"kind": "set_end_condition", "object": boss,
                         "end_condition": {"kind": "up_to_vertex", "result": roof,
                                           "vertex": vertex}}])
height(vertex_end, 30, "up_to_vertex")
print("UP_TO_END_OK face, outward offset, vertex and cap height")
```
