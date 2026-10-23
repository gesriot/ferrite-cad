# Model edit transactions

`ferritecad edit source.fcad --expect-version HASH --request changes.json --output edited.fcad --json`
applies an ordered list to one candidate, validates the final payload, reconciles
output names and rebuilds it before publishing. UI Apply uses the same library path.
An edit refuses every newly failed or skipped feature and every previously resolved
name that would be lost. Existing failures may be repaired. A refusal leaves the
source and destination unchanged.

Use `inspect source.fcad --json` to obtain `content_version`, object UUIDs and
`expression_slots`. Each `sketches` row contains the saved curve UUIDs,
construction flags and geometry in millimetres, plus the exact constraint UUIDs
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

A generic edit may make a circular Cut an open slot, overlap earlier tools, or
constrain its tool sketch. The structural Cut commands and forms currently accept
a narrower domain. Their refusal identifies the saved Cut and directs the user to
`edit`; returning the wall or tool inside, or Undo, restores availability. Tangent
circle rims within `WALL_CLEARANCE_MM` of a Line boundary or an earlier circular Cut
are refused by the evaluator. Missing Cut output names are reconciled from the
written candidate after all changes; existing names are retained.
