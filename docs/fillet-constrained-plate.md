# §28F — round a dimensioned plate, in a new copy

[Executed verification and limitations](fillet-constrained-plate-verification.md).
[The Fillet this creates](single-edge-fillet.md);
[the same Fillet dimensioned afterwards](fillet-base-constraints.md), whose
stored/solved policy this reuses; [its radius](edit-fillet-radius.md),
[its plate's height](edit-fillet-base-height.md).
[The Line constraints themselves](sketch-constraints-copy.md).

Until now a person had to round the plate first and dimension it afterwards
(§28A, then §28E). This slice allows the natural order: draw the rectangle,
give it H/V, a Fixed point and its sizes with the existing **Edit
constraints** form, then round one vertical edge with the existing **Fillet
edge of Body** form. An agent does the same with `edit-sketch-constraints-copy`
and then `fillet-edge-copy`. The Fillet is a real saved §28A Fillet over a
constrained base, the same stored shape §28E already writes and reads, so no
command, request, copy pipeline, solve, constraint family, capability, payload
version or schema is added.

## Contract recorded before implementation

### The supported source

The plate `fillet-edge-copy` already accepts (§28A, read by
`cut_edit::read_history` through `CutHistory::target_for_fillet`): one
untransformed XY datum; one Sketch of four Lines whose **stored** coordinates
are an axis-aligned rectangle; one forward literal Blind `Extrude`/`NewBody`;
one Body whose tip is that Extrude; no Cut; no Fillet; exactly the plane,
profile and body-tip dependencies.

What is new: the Sketch may carry constraints, and only the constraint
editor's own managed Line family, checked by the same
`sketch_constraints::managed_lines` §28E uses for the Sketch under a Fillet:
Coincident closure links at the adjacent joints and at most one each of the
existing H/V, length, Fixed endpoint, equal length and Parallel/Perpendicular
slots. Anything else is refused by name. The four stored Lines keep their
UUIDs and order; the stored rectangle is the solver's starting guess and still
has to be a rectangle, because it is where the candidate joints come from.

Unchanged: a Cut history stays unconstrained (the Cut editors' reader keeps
refusing a constrained plate); a second Fillet, Cut with Fillet, Chamfer,
another plane, a rotated plane and any other profile are still refused.

### One policy for stored structure and solved geometry

This is §28E's policy, used as it is:

* **Structure** (document, kernel-free: discovery, preparation, the writer's
  re-derivation): the source above, the stored rectangle, the joint found in
  the stored Lines by its two UUIDs, the managed constraint family, and the
  part of the radius policy that holds at every corner (finite, ≥
  `MIN_RADIUS_MM` = 0.01 mm).
* **Solved geometry** (`evaluable_fillet`, every cold *and* cached rebuild,
  unchanged): the Lines the predecessor Extrude was actually built from — the
  presentation of the profile the rebuild already solved, once. The same
  curve UUIDs in stored order; an axis-aligned rectangle by the shared reader
  at its unchanged 1e-7 mm tolerance, nothing snapped; every Line on its
  stored side; the saved joint a corner; `r ≤ MAX_RADIUS_FRACTION` (½) × the
  shorter solved side at that corner.

The copy job already rebuilds the written copy cold before publishing it
(`edit_object_copy` → `checked_rebuild`), so a Fillet the solved plate cannot
carry is refused there and nothing is published. The radius bound therefore
works in both directions: a radius beyond the stored guess's bound publishes
when the solved sides allow it, and a radius within the stored bound is
refused when the solved sides are too short. The stored coordinates are never
replaced by solved ones and no constraint is dropped to satisfy a reader.

For an unconstrained plate nothing changes: preparation still applies the
full §28A bound to the stored Lines, and no solver is involved.

### Identity — the exact SQL allowlist

Exactly §28A's:

* **objects:** one new Fillet row; the Body row's `payload` and `payload_hash`
  (the tip). The Sketch row — `schema_version` 2, `payload` with its
  constraints and their UUIDs, `payload_hash` — is byte-identical, as is every
  other row.
* **deps:** + `Fillet → base` (Predecessor), + `Body → Fillet` (BodyTip),
  − `Body → base` (BodyTip).
* **topology_refs:** the Fillet's seven new rows; every existing row
  byte-identical.
* **capabilities:** `feature.fillet.v1`, `topology.origin-face.v1` and
  `feature.predecessor.v1`, added only if missing; `sketch.constraints.v1`
  stays as it was.
* **meta.modified_at.**

Every other cell is byte-identical, measured after reopen.

### Writer, version, references

Unchanged pipeline: snapshot, version guard against the exact snapshot and
again before publication, read-only source, baseline rebuild, every baseline
and every minted name resolving after the write, cancellation, no-clobber
atomic publication, typed refusals, exit 7 on a lost report. The writer
re-derives the prepared Fillet inside its transaction (`fillet::rederive`,
which reruns preparation against the transaction's document), so a forged or
stale preparation is refused. The baseline rebuild of a constrained plate
needs the solver: a build without PlaneGCS refuses before writing anything,
while the unconstrained route still needs none.

### Order of operations

constraints → Fillet and Fillet → constraints (§28E) end in the same stored
model: the same Sketch payload (up to the constraint UUIDs each run mints),
the same Fillet payload, dependencies, names and capabilities (up to the
Fillet's and the names' minted UUIDs), and the same geometry. Radius (§28B),
height (§28C) and constraint (§28E) edits then work on the result as they do
on a §28E part; the coordinate editor keeps refusing a constrained Sketch.

### Discovery (`inspect --json`), additive

`bodies[].fillet_edge` keeps every field, type and meaning for an
unconstrained plate. For a constrained one:

* `available` is `true` when the structure is supported. As before it is
  structural only: it promises neither native libraries nor that the solved
  plate will carry the radius.
* `target.profile_constrained` (new, `bool`; `false` for every plate this
  reported before).
* each candidate names a structurally admissible joint: `edge` (the two Line
  UUIDs, the identity a request repeats) and a `label` that says the numbers
  are stored.
* `corner_mm`, `adjacent_lengths_mm` and `max_radius_mm` describe the part.
  They are unchanged numbers for an unconstrained plate and `null` for a
  constrained one, whose part only a solve knows. Inspect does no solve and no
  rebuild.
* `stored_corner_mm` and `stored_adjacent_lengths_mm` (new, always numbers):
  the stored drawing. For an unconstrained plate they equal `corner_mm` and
  `adjacent_lengths_mm`.
* `min_radius_mm` and `max_radius_fraction` keep their meaning: the policy
  judged on the solved plate when the copy is built.

`fillet-edge-copy` JSON result: `corner_mm` is the corner of the plate the
copy was built from (for an unconstrained plate the stored corner, as before).
New: `profile_constrained`, `stored_corner_mm`, and `adjacent_lengths_mm` of
the built plate.

### UI

The existing **Fillet edge of Body** form. For a constrained plate it says the
Sketch carries constraints, lists each corner by its two Lines with its
stored position and sides labelled as stored, states that the corner's
position and the `r ≤ ½ × shorter side` bound are the solved plate's and are
checked when the copy is saved, and applies only the value part of the radius
policy before Save. A refusal at Save arrives in the status line with the draft
kept, as for every copy job.

### Compatibility

No new stored semantics: the document is exactly what §28E writes (a Fillet
over a constrained plate), so the reader on `main` after #69 reads and
rebuilds it; that is measured with a real build of that commit, not inferred
from the schema version. That build's `fillet-edge-copy` still refuses a
constrained plate, typed.

### Out of scope

Creating or editing an arbitrary quadrilateral, a second Fillet, Cut with
Fillet, Chamfer, picking, live preview, new constraint families, another or a
rotated plane, in-place Save. Milestone 5C is not closed.
