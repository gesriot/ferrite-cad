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
side and reduce its Fillet radius together. The evaluator still requires the
Fillet's rectangular profile. Moving only one corner therefore refuses that Fillet;
a plain extrusion can accept the same nonrectangular profile.

Sweep additions and edits use this path for every supported operation and axis.
The kernel decides contact and validity. Every rebuilt feature has complete generative
names; an unconsumed floor or collapsed axis face may disappear. A consuming feature
whose reference no longer resolves refuses the candidate with its UUID and reason.

`inspect --json` includes each Body's `history` from its first feature to its tip.
Extrude rows include `profile_sketch_id`, `previous_feature_id`, `operation`,
`reversed` and a unitful `end_condition`. Fillet/Chamfer rows include their edge's
feature and pair of segment UUIDs, previous feature, expression, `display_value`
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

A Fillet feature payload is `{"kind":"fillet","edge":{"feature":"<base UUID>",
"joint":["<Line UUID>","<Line UUID>"]},"radius":{"value":2,"unit":"mm"}}`.
A Chamfer uses `"kind":"chamfer"` and `"distance"` instead of `"radius"`.
Both use `add_feature` with an explicit new object and Body. The edge pair may be
supplied in either order. The evaluator's existing rectangle, radius, distance
and adjacent-Fillet policies still decide the result.

`{"kind":"set_end_condition","object":"<Extrude or Cut UUID>",
"end_condition":{"kind":"through_all"}}` changes the end condition; Blind uses
the same unitful `distance` shown above. Unconsumed floors may disappear when
changing Blind to ThroughAll. Consumed references
must still resolve. Circle and depth edits use `set_curve_geometry` and `set_expression`.

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
