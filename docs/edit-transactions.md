# Model edit transactions

`ferritecad edit source.fcad --expect-version HASH --request changes.json --output edited.fcad --json`
applies an ordered list to one candidate, validates the final payload and rebuilds it before publishing. UI Apply uses the same library path.
An edit refuses every newly failed or skipped feature ; an unresolved consumed reference fails its feature. Existing failures may be repaired. A refusal leaves the
source and destination unchanged.

Use `inspect source.fcad --json` to obtain `content_version`, object UUIDs and
`expression_slots`. Each `sketches` row contains the saved curve UUIDs,
plane UUID, construction flags and geometry in millimetres, plus the exact constraint UUIDs
and rules. Constraint values carry display units; angles use display degrees.
Stored coordinates remain the starting guess when constraints are present.

Every change has `kind`. All request numbers carry a unit (`mm`, `cm`, `m`, `in`
for length; `deg` or `rad` for angles). Model values use millimetres and radians.

```json
{
  "request_version": 1,
  "changes": [
    {"kind": "set_expression", "object": "<Fillet UUID>", "slot": {"kind": "fillet_radius"}, "value": 2, "unit": "mm"},
    {"kind": "set_curve_geometry", "sketch": "<Sketch UUID>", "curve": "<Line UUID>", "geometry": {"kind": "line", "start": {"x": {"value": 0, "unit": "mm"}, "y": {"value": 0, "unit": "mm"}}, "end": {"x": {"value": 10, "unit": "mm"}, "y": {"value": 0, "unit": "mm"}}}},
    {"kind": "remove_constraint", "sketch": "<Sketch UUID>", "constraint": "<constraint UUID>"},
    {"kind": "add_constraint", "sketch": "<Sketch UUID>", "constraint": {"id": "<new constraint UUID>", "rule": {"kind": "radius", "curve": "<Circle UUID>", "radius": {"value": 3, "unit": "mm"}}}}
  ]
}
```

Curve geometry keeps its kind. A Circle uses `center` with unitful `x`/`y` and a
unitful `radius`. Moving a polygon vertex updates the ends of both incident Lines;
moving a side updates both vertices in one transaction. The library helper
`move_sketch_vertices` accepts all moved vertices together and emits each changed
Line once. Repeated changes to the same curve or expression slot take the last
value. Constraint additions preserve the supplied UUID.

The result contains `destination`, `document_id`, `content_version` and `rebuild`.
Curve, constraint and expression edits can be mixed: for example, shrink a plate's
side and reduce its Fillet radius together. The kernel decides whether the
changed geometry can support that radius. The transaction refuses if the consumed
edge disappears, becomes ambiguous under `one`, or the kernel cannot build the result.

Sweep additions and edits use this path for every supported operation and axis.
The kernel decides contact and validity. Every rebuilt feature has complete generative
names; an unconsumed floor or collapsed axis face may disappear. A consuming feature
whose reference no longer resolves refuses the candidate with its UUID and reason.

`inspect --json` includes each Body's `history` from its first feature to its tip.
Extrude rows include `profile_sketch_id`, `previous_feature_id`, `operation`,
`reversed` and a unitful `end_condition`. Fillet/Chamfer rows include `edges`, each reference's producer, kind, digest,
selection and diagnostic UUID, plus previous feature, expression, `display_value`
and `display_unit`. These are saved facts; they do not declare eligibility.

New objects and curves have caller-supplied UUIDv7 identities. Changes apply in
order: a feature can use a sketch added earlier in the request. `add_feature`
sets its predecessor to the Body's current candidate tip and moves that tip;
dependencies are derived from each final payload. Occupied identities, missing
objects and wrong object kinds refuse the whole transaction. Rewriting any saved
object, including a Body tip, refuses if its payload cannot round-trip losslessly.

For example, add a circular tool and a Cut (replace every UUID placeholder):

```json
{"request_version":1,"changes":[
  {"kind":"add_sketch","object":"<new Sketch UUID>","name":"Tool","plane":"<plane UUID>",
   "curves":[{"id":"<new Circle UUID>","construction":false,"geometry":{"kind":"circle",
    "center":{"x":{"value":20,"unit":"mm"},"y":{"value":20,"unit":"mm"}},
    "radius":{"value":2,"unit":"mm"}}}],"constraints":[]},
  {"kind":"add_feature","object":"<new Cut UUID>","body":"<Body UUID>","name":"Cut",
   "feature":{"kind":"extrude","operation":"cut","profile":"<new Sketch UUID>","reversed":false,
    "end_condition":{"kind":"blind","distance":{"value":3,"unit":"mm"}}}}
]}
```

A Fillet feature payload is:

```json
{"kind":"fillet","edges":[{"producer":"<producer UUID>","kind":"edge",
 "digest":"<64 hex digits>","selection":"one"}],"radius":{"value":2,"unit":"mm"}}
```

A Chamfer uses `"kind":"chamfer"` and `"distance"` instead of `"radius"`.
Both use `add_feature` with an explicit new object and Body. `print-topology`
prints the producer and digest; use `selection: "all_descendants"` only when all
surviving descendants are intended. Diagnostic reference UUIDs are assigned by the
request parser and do not affect the meaning or cache key. Replace the set with
`{"kind":"set_edges","object":"<feature UUID>","edges":[<references>]}`.
An empty set, duplicate meanings or a non-edge kind is invalid input. The kernel
decides geometric validity; see [Fillet and Chamfer](fillets-and-chamfers.md).

`{"kind":"set_end_condition","object":"<Extrude or Cut UUID>",
"end_condition":{"kind":"through_all"}}` changes the end condition; Blind uses
the same unitful `distance` shown above. Unconsumed floors may disappear when
changing Blind to ThroughAll. Consumed references
must still resolve. Circle and depth edits use `set_curve_geometry` and `set_expression`.

`up_to_face` carries `result`, a complete face reference and unitful `offset`;
`up_to_vertex` carries `result` and a complete vertex reference with `selection: one`.
Both use `set_end_condition`; `set_expression` slot `extrude_end_offset` edits the
face offset. New support targets are validated for existing as well as new features;
cycles refuse before publication. Undo/Redo includes the condition and dependency row.
See [referenced sweep ends](generic-sweeps.md#referenced-end-conditions) for JSON and
an executable recipe.

Generic sweeps accept `NewBody`, `Add`, `Cut` and `Intersect`. `add_feature` derives
`previous` from the candidate Body tip. `add_body` supplies `object`, `name`,
`feature_object`, `feature_name` and `feature`; its first feature must use `NewBody`.
An existing feature's operation changes with `set_operation` (`object`, `operation`),
its extrusion direction with `set_reversed` (`object`, `reversed`).

A Revolve addition has `kind: "revolve"`, `profile`, `operation`, `axis` and `extent`.
Axes are `"sketch_x"`, `"sketch_y"`, or `{"line":"<Line UUID>"}`.
Extents are `{"kind":"full_turn"}` or
`{"kind":"partial","angle":{"value":137.5,"unit":"deg"}}`.
Use `set_revolve_axis` (`object`, `axis`) and `set_revolve_extent` (`object`, `extent`)
for existing features. Numbers still use expression slots; no new numeric edit route
is introduced. See [sweep semantics](generic-sweeps.md).

Extrude and Revolve additions also accept `regions: ["<outer curve UUID>"]`.
Omitting this CLI field is equivalent to an empty list, selecting all regions.
Change an existing feature with
`{"kind":"set_regions","object":"<feature UUID>","regions":["<curve UUID>"]}`.
The shared transaction rebuilds the candidate and refuses any newly failed
consumer before publication. Undo and Redo restore the same curve references.

## Datum planes

`add_datum_plane` takes `object`, optional `name` and `plane`, whose `kind` is
`fixed`, `offset` or `face`. A Face carries `result`, `face` and unitful `offset`;
an Offset carries `from` and unitful `distance`. `set_datum_face` takes `object`
and a face reference, retaining the result feature. `set_expression` with slot
`{"kind":"datum_offset"}` changes either offset. All new dependency targets are
checked against the complete candidate; graph cycles refuse publication.

A plane, its sketch and its feature can be added together. Face resolution uses
the minting producer, kind, digest and selection. A newly unbuildable plane refuses
the edit just like a failed feature. See the complete [datum recipe](datum-planes.md).

## KeepSolids

Use `add_feature` on an existing Body with
`"feature":{"kind":"keep_solids","selection":{"kind":"all"}}`.
To select pieces, use `"selection":{"kind":"containing_faces","faces":[<face references>]}`.
Replace an existing selection with
`{"kind":"set_keep_selection","object":"<KeepSolids UUID>","selection":{"kind":"all"}}`.
The ordinary reference format is `producer`, `kind: face`, `digest`, `selection`;
CLI allocates the diagnostic identity for each new use. References resolve in the
predecessor result before native containment is checked. Repeated witnesses select a
solid once. A family spanning several solids refuses, even if another witness selects
one of them. `All` always retains the complete set.

A splitting Cut and KeepSolids may be added in one request. The shared transaction moves
the Body tip and saves payloads/dependencies together. A lost dependent witness refuses
publication; Undo/Redo restores the whole candidate. See the executable
[KeepSolids recipe](keep-solids.md) and [ADR 0008](decisions/0008-multibody-results.md).
