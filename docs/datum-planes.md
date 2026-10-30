# Datum planes and sketches on faces

A sketch names one datum plane by UUID. That plane has one definition:

- `fixed` stores `placement`, the existing model transform.
- `offset` stores `from`, another datum, and a signed `distance_mm` expression.
- `face` stores a feature `result`, a face `EntityReference` and a signed
  `offset_mm` expression. Zero puts the sketch on the selected face.

Expressions use millimetres internally. Both offset signs and zero are valid;
nonfinite values are refused. An Offset has a `plane` dependency; a Face has a
`support` dependency on its result feature. These rows must match the definition.
The ordinary graph validation refuses cycles, including a feature supported by
its own sketch plane.

The frame follows these deterministic conventions:

- The normal is the face's outward normal in the solid, including its orientation.
- The origin is the orthogonal projection of the model origin onto the plane.
- X is the normalized projection of model X. When model X is parallel to the
  normal within the operation's angular tolerance, project model Y instead.
- An offset moves the origin along the normal and preserves X and the normal.

These conventions do not select a face by coordinates. Face selection uses the
same resolver as [Fillet and Chamfer](fillets-and-chamfers.md): the producer is
who minted the digest, which may precede the consumed result. `one` needs one
surviving face. `all_descendants` explicitly accepts a family only when the kernel
checks that every member is planar, lies in the same plane within linear tolerance,
and has the same outward normal within angular tolerance.

Resolution errors are `topology` errors naming the consumer and reference:
`producer ... is not in the consumed result's history`, `unknown name: the producer
minted no name with this digest`, `face deleted from the consumed result`, or
`face split into 2 faces; selection one refuses to choose`. A wrong reference kind
is invalid input when stored; the common resolver also checks kind on use.
A curved face is `input`: `face is not planar`, with the reference and
`cannot support a sketch plane` context. A mismatched family is `input`:
`face descendants do not lie in one plane with one outward normal`.
Native failures remain `kernel` errors. No refusal picks a nearby face.

An edit that newly breaks the datum or its dependents refuses atomically. An
already broken document opens for repair: the datum fails, its sketches and
features are skipped, and independent bodies still build. Cold and cached rebuilds
use the same computed planes. Moving a support changes the dependent geometry's
cache key; it preserves curve identities and generative name digests.

## Edit and inspection

A plane, sketch and feature can be added in one ordered request:

```json
{"request_version":1,"changes":[
  {"kind":"add_datum_plane","object":"<new plane UUID>","name":"Top face",
   "plane":{"kind":"face","result":"<result feature UUID>",
    "face":{"producer":"<minting feature UUID>","kind":"face",
            "digest":"<64 hex digits>","selection":"one"},
    "offset":{"value":0,"unit":"mm"}}}
]}
```

An Offset definition is `{"kind":"offset","from":"<plane UUID>",
"distance":{"value":10,"unit":"mm"}}`. A Fixed definition uses `kind: "fixed"`
and the existing `placement` transform. `set_datum_face` takes `object` and `face`;
it keeps the result feature unchanged. The `datum_offset` slot in `set_expression`
changes either signed offset. Ordinary Undo/Redo restores the whole transaction.

`inspect --json` includes `datum_planes`: each row has `object`, `name`,
`definition` and `computed`. Definition expressions retain their source and value.
`computed` contains `origin`, `x_axis`, `normal`, or is null if that datum cannot be
built. `rebuild` names the definition and prints the computed normal.

The window draws bodies and sketches at these evaluated frames and can edit their
existing dimensions. Creating a datum from a viewport face is separate work. Extrude conditions that
terminate at a face or vertex use the same references; see [sweep ends](generic-sweeps.md).

## Executable recipe

Set `FERRITECAD` to the built CLI. This uses a cap's reported derivation to obtain
its identity, then adds a boss and a second body on an Offset plane.

```python
# DATUM_PLANES_RECIPE
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import uuid

cli = os.environ["FERRITECAD"]
root = Path(tempfile.mkdtemp(prefix="datum-planes-"))


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
    print("DATUM_PLANES_NO_KERNEL unsupported kernel")
    raise SystemExit(0)
facts = call("inspect", source, "--json")["result"]
base = facts["features"][0]["feature_id"]
body = facts["bodies"][0]["body_id"]
lines = call("print-topology", source).splitlines()
caps = []
for i, line in enumerate(lines[:-1]):
    match = re.search(r"producer=([0-9a-f-]+) kind=face digest=([0-9a-f]{64}) entities=1", line)
    if match and "end cap, producer" in lines[i + 1]:
        caps.append({"producer": match[1], "kind": "face", "digest": match[2],
                     "selection": "one"})
assert len(caps) == 1, lines
face = caps[0]
plane, profile, boss = identity(), identity(), identity()
offset, upper_profile = identity(), identity()
added = root / "supported.fcad"
edit(source, added, [
    {"kind": "add_datum_plane", "object": plane, "plane": {
        "kind": "face", "result": base, "face": face, "offset": mm(0)}},
    sketch(profile, plane),
    {"kind": "add_feature", "object": boss, "body": body,
     "feature": extrusion(profile, "add")},
    {"kind": "add_datum_plane", "object": offset, "plane": {
        "kind": "offset", "from": plane, "distance": mm(10)}},
    sketch(upper_profile, offset),
    {"kind": "add_body", "object": identity(), "feature_object": identity(),
     "feature": extrusion(upper_profile, "new_body")},
])
raised = root / "raised.fcad"
edit(added, raised, [
    {"kind": "set_expression", "object": base, "slot": {"kind": "extrude_distance"},
     **mm(20)},
    {"kind": "set_datum_face", "object": plane, "face": face},
    {"kind": "set_expression", "object": offset, "slot": {"kind": "datum_offset"},
     **mm(12)},
])
planes = {p["object"]: p for p in call("inspect", raised, "--json")["result"]["datum_planes"]}
assert planes[plane]["computed"]["origin"]["z"] == 20
assert planes[offset]["computed"]["origin"]["z"] == 32
assert "face plane; normal X 0 Y 0 Z 1" in call("rebuild", raised, "--cold")
before = raised.read_bytes()
refused = root / "refused.fcad"
error = edit(raised, refused, [{"kind": "set_datum_face", "object": plane,
                               "face": {**face, "kind": "edge"}}], code=2)
assert error["error"]["kind"] == "input", error
assert raised.read_bytes() == before and not refused.exists()
print("DATUM_PLANES_OK boss, offset body, moving support and atomic refusal")
```
