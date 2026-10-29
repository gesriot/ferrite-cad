# Fillet and Chamfer

Fillet rounds a nonempty set of named edges at one radius. Chamfer cuts them at
one equal distance along their adjacent faces. Both consume `previous`, the saved
predecessor result, and produce exactly one valid solid. Radius and distance must
be finite and positive; Open CASCADE decides what fits the supplied geometry.
There is no rectangle classification, corner catalogue, minimum flat or limit on
how many edge features a history may contain.

References work on edges of any body, including pocket rims, Revolve results and
edges modified by earlier Fillets. Each stores a producer UUID, kind, name digest
and selection. The producer is the feature that minted the name, not a later
feature whose result still holds the edge. `one` requires exactly one surviving
edge in the consumed result; `all_descendants` explicitly selects every surviving
descendant. A diagnostic UUID identifies this particular use of the reference. It
does not change its meaning or the cache key. See the
[stored representation](file-format.md) and
[edit transaction format](edit-transactions.md).

Use `ferritecad print-topology model.fcad` to obtain copyable producer, kind and
digest values. An `edit` addition supplies a Body and a new feature identity:

```json
{"request_version":1,"changes":[
  {"kind":"add_feature","object":"<new feature UUID>","body":"<Body UUID>",
   "name":"Round edges","feature":{"kind":"fillet","edges":[
    {"producer":"<producer UUID>","kind":"edge","digest":"<64 hex digits>",
     "selection":"one"}],"radius":{"value":2,"unit":"mm"}}}
]}
```

For Chamfer replace `fillet`/`radius` with `chamfer`/`distance`. Replace an existing
set using `set_edges` with `object` and `edges`. Empty sets and repeated meanings
are input errors. `inspect --json` reports the references and their diagnostic
UUIDs. `rebuild` reports the amount, resolved edge count and reference count. Numeric changes
use the existing `fillet_radius` and `chamfer_distance` expression slots.

Resolution checks every reference before building. Its topology error names the
consuming feature and every unresolved reference, with one of these reasons:

- producer UUID is not in the consumed result's history;
- unknown name: the producer minted no name with this digest;
- edge deleted from the consumed result;
- edge split into a stated number of edges; selection one refuses to choose;
- reference does not name an edge.

There is no nearest-edge fallback. An intervening feature that leaves the named
edge intact does not invalidate the reference. Kernel failures remain kernel
errors. A transaction that introduces a failed consumer refuses atomically and
leaves source and destination untouched. An already broken document opens for
repair with the same feature diagnostic. Undo/Redo follows the shared transaction.

In the window, select an edge in the viewport, open the Body's Fillet or Chamfer
form and press **Add selected edge**. The form lists each reference and allows it
to be removed. A stale scene refuses to contribute a reference. Enter the amount
and apply; the document stores the scene meaning, not a pixel, index or coordinate.
The radius/distance editors, unsaved exports and Save/Save As remain available.
Replacing a saved edge set is available through CLI; viewport multiselect and
window edge-set editing are separate work.

All selected edges enter one kernel builder. Complete Modified/Generated/Deleted
history includes vertex junction faces. Names follow the common operation path;
cold and cached rebuilds preserve the same meanings. The document format is
unstable: regenerate fixtures after changing payloads, without migration code.

## Executable recipe

This example selects all named edges of a simple plate by identity. It exercises
both operations, replacement, a duplicate refusal and cold rebuilds. Set
`FERRITECAD` to the built CLI executable, then run the Python block.

```python
# NAMED_EDGES_RECIPE
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import uuid

cli = os.environ["FERRITECAD"]
root = Path(tempfile.mkdtemp(prefix="named-edges-"))


def call(*args, code=0):
    result = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
    assert result.returncode == code, (args, result.stdout, result.stderr)
    return json.loads(result.stdout) if "--json" in args else result.stdout


def identity():
    raw = bytearray(int(time.time() * 1000).to_bytes(6, "big") + os.urandom(10))
    raw[6] = (raw[6] & 15) | 112
    raw[8] = (raw[8] & 63) | 128
    return str(uuid.UUID(bytes=bytes(raw)))


source = root / "plate.fcad"
call("create", source, "--sample", "--size", 60, 40, 10, "--json")
probe = subprocess.run([cli, "rebuild", str(source), "--cold"],
                       capture_output=True, text=True)
if probe.returncode:
    text = probe.stdout + probe.stderr
    assert "Open CASCADE" in text and "unsupported" in text.lower(), text
    print("NAMED_EDGES_NO_KERNEL unsupported kernel")
    raise SystemExit(0)
facts = call("inspect", source, "--json")["result"]
body = facts["bodies"][0]["body_id"]
text = call("print-topology", source)
edges = [{"producer": producer, "kind": "edge", "digest": digest, "selection": "one"}
         for producer, digest in re.findall(
             r"producer=([0-9a-f-]+) kind=edge digest=([0-9a-f]{64}) entities=1", text)]
assert len(edges) == 12, text


def edit(path, target, changes, code=0):
    version = call("inspect", path, "--json")["result"]["content_version"]
    request = root / "request.json"
    request.write_text(json.dumps({"request_version": 1, "changes": changes}))
    return call("edit", path, "--expect-version", version, "--request", request,
                "--output", target, "--json", code=code)


for kind, amount in [("fillet", "radius"), ("chamfer", "distance")]:
    feature = identity()
    added = root / (kind + ".fcad")
    edit(source, added, [{"kind": "add_feature", "object": feature, "body": body,
                         "name": kind, "feature": {"kind": kind, "edges": edges,
                                                   amount: {"value": 0.5, "unit": "mm"}}}])
    reordered = root / (kind + "-reordered.fcad")
    edit(added, reordered, [{"kind": "set_edges", "object": feature,
                            "edges": list(reversed(edges))}])
    call("rebuild", reordered, "--cold")
    before = reordered.read_bytes()
    refused = root / (kind + "-duplicate.fcad")
    result = edit(reordered, refused, [{"kind": "set_edges", "object": feature,
                                       "edges": [edges[0], edges[0]]}], code=2)
    assert result["error"]["kind"] == "input", result
    assert reordered.read_bytes() == before and not refused.exists()
print("NAMED_EDGES_OK both operations, replacement and atomic duplicate refusal")
```
