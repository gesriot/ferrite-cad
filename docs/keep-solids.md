# KeepSolids

A feature returns the complete unordered, nonempty set of valid solids. A Body presents
that whole result through one history tip. Rebuild creates no new document objects.
Use `KeepSolids` after a sweep or boolean to retain an explicit subset. Its default
`All` retains every piece. `ContainingFaces` requires one or more existing face references.

Each reference first resolves in the predecessor through the common naming resolver.
`one` requires exactly one face. `all_descendants` may resolve several faces, but they
must all belong to one native solid. Containment is topological, without coordinates,
volume ranking or enumeration indices. Repeated witnesses select the same solid once;
different witnesses select their union. A family spanning several solids refuses.
If a piece has no uniquely resolving face witness, it cannot be selected individually;
`All` remains available. See [ADR 0008](decisions/0008-multibody-results.md).

Naming occurs on the complete geometric result before selection. KeepSolids retains or
deletes elementary topology and creates no derivation nodes. A surviving split face
keeps its split name, even if only one piece remains. One atomic `brep.generative.v2`
archive stores the entire selected result, names, ancestry and incidence. Restoration
checks that the trusted result contains solids and no free lower dimensions, and validates
the complete incidence before publishing anything; geometry and member volumes were checked
when the record was created.

The feature result is checked once where it is built. NewBody checks its sweep;
Add/Cut/Intersect check the boolean result, and KeepSolids checks its retained set.
A prism used only as the same feature's boolean tool defers member-volume checks to
that result. Existing Revolve and up-to-surface tool validation remains unchanged.
Boolean removed-volume accounting reuses the checked output member-volume sum.

## Requests and refusals

Add to a Body using the ordinary transactional `edit` request:

```json
{"request_version":1,"changes":[
  {"kind":"add_feature","object":"<new feature UUID>","body":"<Body UUID>",
   "feature":{"kind":"keep_solids","selection":{"kind":"all"}}}
]}
```

Replace its selection with named faces:

```json
{"request_version":1,"changes":[
  {"kind":"set_keep_selection","object":"<KeepSolids UUID>",
   "selection":{"kind":"containing_faces","faces":[
    {"producer":"<producer UUID>","kind":"face","digest":"<64 hex digits>",
     "selection":"one"}]}}
]}
```

`inspect --json` reports saved references including diagnostic IDs, selection, predecessor
and each built feature's solid count. Text `inspect` and `rebuild` describe the selection
in words. A splitting Cut and KeepSolids can share one request. The common transaction
rejects any newly failed dependent feature, leaving the source and destination unchanged.
An already broken saved document opens for repair. Undo/Redo restores payloads, tip and
references together. There is no new window selection form yet.

- `input`: empty face list, a non-face reference, missing predecessor or invalid request.
- `topology`: deleted/unknown/split exact reference, or a family with zero or several owners.
  Resolution reports the consumer and reference UUIDs before membership is attempted.
  Membership says “KeepSolids FEATURE: reference REF resolves to faces in 2 solids;
  one containing solid is required. Replace this reference or choose All.”
- `unsupported`: an operation leaves no solids, or a Cut removes no material. An empty result
  names the operation: “the cut result is empty and contains no solid”. The kernel completed;
  the requested geometry has no solid result.
- `kernel`: a null shape, invalid member, non-finite/non-positive member volume, or free lower
  dimensions outside solids. A positive total volume cannot hide an invalid member.

Fillet/Chamfer on multiple solids still refuse with the observed count. Referenced ends
and ThroughAll over whole sets, new selection forms and export evidence are later steps.

## Executable recipe

Set `FERRITECAD` to the built CLI. This adds a detached cylinder and KeepSolids together,
then retains the original plate by its named cap. The witness is a derivation, never a
solid ordinal or coordinate search.

```python
# KEEP_SOLIDS_RECIPE
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import uuid

cli = os.environ["FERRITECAD"]
root = Path(tempfile.mkdtemp(prefix="keep-solids-"))


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


def edit(source, target, changes, code=0):
    version = call("inspect", source, "--json")["result"]["content_version"]
    request = root / "request.json"
    request.write_text(json.dumps({"request_version": 1, "changes": changes}))
    return call("edit", source, "--expect-version", version, "--request", request,
                "--output", target, "--json", code=code)


source = root / "plate.fcad"
call("create", source, "--sample", "--size", 60, 40, 10, "--json")
probe = subprocess.run([cli, "rebuild", str(source), "--cold"],
                       capture_output=True, text=True)
if probe.returncode:
    text = probe.stdout + probe.stderr
    assert "Open CASCADE" in text and "unsupported" in text.lower(), text
    print("KEEP_SOLIDS_NO_KERNEL unsupported kernel")
    raise SystemExit(0)
facts = call("inspect", source, "--json")["result"]
body = facts["bodies"][0]["body_id"]
base = facts["features"][0]["feature_id"]
plane = facts["features"][0]["profile_sketch_id"]
plane = next(s["plane_id"] for s in facts["sketches"] if s["sketch_id"] == plane)
lines = call("print-topology", source).splitlines()
witnesses = []
for line, detail in zip(lines, lines[1:]):
    match = re.search(rf"producer={base} kind=face digest=([0-9a-f]{{64}}) entities=1", line)
    if match and "end cap, producer" in detail:
        witnesses.append({"producer": base, "kind": "face", "digest": match[1],
                          "selection": "one"})
assert len(witnesses) == 1, witnesses
profile, addition, keep = identity(), identity(), identity()
all_pieces = root / "all.fcad"
edit(source, all_pieces, [
    {"kind": "add_sketch", "object": profile, "plane": plane, "constraints": [],
     "curves": [{"id": identity(), "construction": False, "geometry": {
         "kind": "circle", "center": {"x": mm(75), "y": mm(20)}, "radius": mm(3)}}]},
    {"kind": "add_feature", "object": addition, "body": body, "feature": {
        "kind": "extrude", "profile": profile, "operation": "add", "reversed": False,
        "end_condition": {"kind": "blind", "distance": mm(10)}}},
    {"kind": "add_feature", "object": keep, "body": body,
     "feature": {"kind": "keep_solids", "selection": {"kind": "all"}}}])


def count(path):
    facts = call("inspect", path, "--json")["result"]
    return next(r["solid_count"] for r in facts["feature_results"] if r["feature_id"] == keep)


assert count(all_pieces) == 2
selected = root / "selected.fcad"
edit(all_pieces, selected, [{"kind": "set_keep_selection", "object": keep,
                            "selection": {"kind": "containing_faces", "faces": witnesses}}])
assert count(selected) == 1
call("rebuild", selected, "--cold")
before = selected.read_bytes()
refused = root / "refused.fcad"
error = edit(selected, refused, [{"kind": "set_keep_selection", "object": keep,
                                 "selection": {"kind": "containing_faces", "faces": []}}], code=2)
assert error["error"]["kind"] == "input"
assert selected.read_bytes() == before and not refused.exists()
print("KEEP_SOLIDS_OK complete set, named selection, cold rebuild and atomic refusal")
```
