# §28E — dimension the rectangle under a Fillet, in a new copy

[Executed verification and limitations](fillet-base-constraints-verification.md).
[The Fillet this keeps](single-edge-fillet.md); [its radius](edit-fillet-radius.md),
[its plate's height](edit-fillet-base-height.md) and
[its rectangle's coordinates](edit-fillet-base-sketch.md).
[The Line constraints themselves](sketch-constraints-copy.md).

A person opens a saved rounded plate — the §28A Fillet on one vertical edge of
a rectangular plate — and, in the existing **Edit constraints** form, adds,
changes or removes the Line constraints the constraint editor already knows
(Horizontal/Vertical, length and Replace length, one Fixed endpoint, equal
length, Parallel, Perpendicular) on the plate's base Sketch, then saves a new
`.fcad`. An agent does the same with `inspect --json` and
`edit-sketch-constraints-copy`. The rounding stays on the same corner, named by
the same two Line UUIDs, with the same radius. The stored coordinates stay the
solver's starting approximation; the Body is built from the PlaneGCS solution.
No command, request format, copy pipeline, constraint family, capability,
payload version or schema is added.

## Contract recorded before implementation

### The supported source

The frame §28B–§28D read, by the same reader
(`fillet_radius::fillet_over_plate` over
`cut_edit::saved_history_under_fillet`): one untransformed XY datum; one Sketch
of four Lines whose **stored** coordinates are an axis-aligned rectangle; one
forward literal Blind `Extrude`/`NewBody`; one Fillet whose `previous` and
`edge.feature` are that Extrude and whose joint is a corner of the stored
rectangle; one Body whose tip is the Fillet; no Cut; exactly the plane,
profile, predecessor and body-tip dependencies; exactly the seven §28A names.

What is new: the base Sketch may carry constraints, and only the constraint
editor's own managed Line family (read by its own `managed` check): Coincident
closure links at the adjacent joints, and at most one each of the existing
H/V, length, Fixed endpoint, equal length and Parallel/Perpendicular slots.
Anything else on it is refused, naming the Fillet.

Unchanged: creating a Fillet (§28A) still requires an unconstrained profile;
a second Fillet, a Cut with a Fillet, Chamfer, another plane and any other
profile are still refused.

### Stored and solved geometry are separate facts

* **Structure** (document, kernel-free, every discovery, preparation and
  write): the frame above, the stored rectangle, the joint found in the stored
  Lines by its two UUIDs, the managed constraint family.
* **Solved geometry** (the evaluator, every cold *and* cached rebuild): the
  Fillet's policy is asked of the Lines its predecessor Extrude was actually
  built from — the presentation of the profile the rebuild already solved,
  once, for that Extrude. No second solve. For an unconstrained Sketch those are
  the stored Lines, so §28A–§28D behave exactly as before. The policy:
  * the same curve UUIDs, in stored order, all Lines, none construction;
  * an axis-aligned rectangle by the shared rectangle reader, at its unchanged
    tolerance (`Tolerance::DEFAULT_LINEAR`, 1e-7 mm); nothing is snapped and
    no wider tolerance is introduced;
  * every Line keeps its side — the same axis and direction as stored, by its
    dominant component, the rule §28D uses — so the rounded corner is the same
    corner of the part;
  * the saved joint is a corner of that solved rectangle, found by its two
    Line UUIDs (`corner_for`), never by position or nearest coordinates;
  * the saved radius fits: `r ≤ ½ · min(adjacent solved sides)` (§28A's
    unchanged policy, `check_radius`). Nothing is clamped and the radius,
    height and Fillet row are never changed to fit.
* **Stored dimensions are not evidence** that the radius fits a constrained
  part. For a constrained base, discovery and preparation do not apply the
  radius bound to stored Lengths in either direction; the rebuild applies it
  to the solved ones.

### Identity — the exact SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Sketch row's `schema_version`, `payload`, `payload_hash` |
| `capabilities` | the `sketch.constraints` row, upserted with `required = 1` by the existing policy when the saved Sketch has constraints (already present when the source had them) |
| `meta` | `modified_at` only if the existing writer stamps it (measured and reported) |
| every other table and column, including `deps` and `topology_refs` | nothing |

Row counts are equal except for the one possible new `capabilities` row.
Preserved: every object, curve, Fillet, joint and topology-reference UUID; the
Lines' order, winding and sides; every dependency; the stored coordinates.
Constraint UUIDs change only by the existing atomic remove-then-add: the
removed UUIDs disappear, new ones are minted for additions and for missing
closure Coincidents.

### Removing constraints

The existing rule stays: closure Coincidents are never removable, and are added
with the first addition. Removing the last user constraint leaves the four
closure links, so the Sketch remains constrained (solved = stored, since the
stored loop already closes). The coordinate editor (§28D) then honestly
refuses, as it refuses every constrained profile since §25E; nothing is dropped
silently.

### Radius and height after constraints

`edit-fillet-radius` (§28B) and `edit-extrude` (§28C) accept the constrained
plate and keep its constraints byte for byte. The radius bound for a
constrained plate is checked by the copy's rebuild on the solved predecessor;
preparation only checks the radius is finite and at least the minimum. The
height needs no Fillet bound (§28C).

### Writer, version, references

`Document::write_sketch_constraints` re-derives inside its transaction instead
of trusting the prepared payload: the Sketch row is the one prepared; the frame
and managed family are read again from the current document (so the Fillet
frame too); the prepared constraint list must equal the stored list without
the removed UUIDs followed by the added constraints; curves and plane are
unchanged; the result is inside the managed family. The existing job keeps the
content-version guard (it covers the Fillet row and every name), `copy_access`,
alias/no-clobber, cancellation, strict resolved references after the cold
rebuild, SQLite close, atomic publication and exit 7. A solver conflict stays
the typed `constraint` refusal naming UUIDs; a solved plate outside the Fillet
policy is a typed domain refusal; the source, destination and draft are left
as they were.

### Discovery (`inspect --json`), additive

* `sketches[].constraint_edit` of the base Sketch becomes `available`, with
  its stored curves and constraints as for any managed profile, and gains
  `fillet_base`: `fillet_feature_id`, `body_id`, `edge`, `radius_mm` and
  `stored_corner_mm` — the corner in the **stored** coordinates, named as such.
  `null` everywhere else.
* `features[].fillet_base`, `sketches[].fillet_base` and `fillets[]` gain
  `profile_constrained`. When it is `true`, their `corner_mm` is the stored
  approximation, and `fillets[].radius_edit.max_radius_mm` is `null`: the
  bound is judged on the solved plate when a copy is published.
* Kernel-free, one pinned snapshot, old fields, types, operation names and exit
  codes unchanged; the request stays strict.

### UI

The existing Edit constraints form and worker: bounded Undo/Redo, Replace
length, Save/Cancel, refusal and stale-reply preservation. One context line
names the Fillet, the two Lines of its corner and its radius, says the
coordinates shown are the stored ones, and says the new copy is published only
if the solved plate is still this rectangle with room for the radius.

### Compatibility

No new capability, payload version or SQLite schema: the saved Sketch uses the
existing constraint payload (v2) and capability. A build before this one
refuses to rebuild the rounded plate once its base carries constraints (its
Fillet reader requires an unconstrained profile) — a typed refusal, never a
part of the wrong shape. Checked with the real CLI of the base commit.

### Out of scope

Creating a Fillet on a constrained source, a second Fillet, Cut with Fillet,
Chamfer, an arbitrary quadrilateral, a rotated plane, new constraint families,
live preview, picking edges, in-place Save. The Fillet/Chamfer milestone stays
open.
