# §29D — dimension the rectangle under a Chamfer, in a new copy

[Executed verification and limitations](edit-chamfer-base-constraints-verification.md).
[The Chamfer this keeps](rectangular-corner-chamfer.md); [its height](edit-chamfer-base-height.md)
and [its rectangle's coordinates](edit-chamfer-base-sketch.md).
[The Line constraints themselves](sketch-constraints-copy.md); [the same slice under a
Fillet](fillet-base-constraints.md).

A person opens a saved plate with the one §29A Chamfer and, in the existing
**Edit constraints** form, adds, replaces or removes the Line constraints the
constraint editor already knows (Horizontal/Vertical, length and Replace length,
one Fixed endpoint, equal length, Parallel, Perpendicular, with the Coincident
closure links) on the plate's base Sketch, then saves a new `.fcad`. An agent does
the same with `inspect --json` and `edit-sketch-constraints-copy`. PlaneGCS
decides the rectangle's size and position; the Chamfer stays on the **same
corner**, named by the same two Line UUIDs, with the same distance. No command,
request format, copy pipeline, solver, constraint family, capability, payload
version or schema is added. Milestone 5C is not complete.

## Contract recorded before implementation

### The supported source

The class of §29A–§29C, read by the one reader the distance, height and
coordinate edits use (`chamfer::saved_chamfer` over
`cut_edit::saved_history_under_chamfer`): one untransformed XY datum; one Sketch
of four Lines whose **stored** coordinates are an axis-aligned rectangle; one
forward literal Blind `Extrude`/`NewBody`; one terminal Chamfer on a corner of
that rectangle; one Body whose tip is the Chamfer; the Chamfer's seven owned
names. Additional base-owned references that resolve are allowed and are never
forbidden here (§29B review: **every saved reference must resolve before and
after** the edit; unchanged).

What is new: the base Sketch may carry constraints, and only the constraint
editor's managed Line family (its own `managed` check): the Coincident closure
links and at most one each of the existing H/V, length, Fixed endpoint, equal
length and Parallel/Perpendicular slots. Anything else on it is refused, naming
the guilty constraint. Unchanged and still refused: creating a Chamfer on a
constrained source (§29A creation requires an unconstrained profile), a second
Chamfer, a Fillet or Cut beside it, another plane, another corner, ThroughAll,
new constraint families and circles, in-place Save, live preview.

### Stored and solved geometry are separate facts

* **Structure** (document, kernel-free, every discovery, preparation and write):
  the frame above, the stored rectangle, the corner found in the stored Lines by
  its two UUIDs, the managed constraint family.
* **Solved geometry** (the evaluator, at every cold *and* cached rebuild): the
  Chamfer's policy is asked of the Lines its predecessor Extrude was built from —
  the presentation the rebuild already solved, once, for that Extrude. No second
  solve and no parsing of prose. For an unconstrained Sketch those are the stored
  Lines, so §29A–§29C behave exactly as before. The policy:
  * the same curve UUIDs, in stored order, all Lines;
  * an axis-aligned rectangle by the shared rectangle reader at its unchanged
    tolerance; nothing is snapped and no wider tolerance is introduced;
  * every Line keeps its side (same axis and direction as stored, the rule §29C
    uses), so the cut corner is the same corner of the part;
  * the saved joint is a corner of that solved rectangle, found by its two Line
    UUIDs, never by position;
  * `0.001 mm ≤ d ≤ min(adjacent solved sides) − 0.01 mm`, the one §29A
    expression, exact at the bound. Nothing is clamped; the distance, the height
    and the Chamfer row are never changed to fit.
* **Stored dimensions are not evidence.** For a constrained base, discovery and
  preparation never apply the upper bound to stored coordinates — neither to
  refuse a distance the solved plate allows nor to accept one it does not. The
  rebuild applies it to the solved sides.

### Error policy

A real solver conflict or redundancy is the typed `constraint` refusal naming its
constraint UUIDs (unchanged). A solved plate that is not the rectangle, moves a
Line off its side, loses the corner or leaves no room for the saved distance is a
separate domain refusal naming the Chamfer UUID and the violated bound. Neither
publishes anything: the source, the destination and the draft are left as they were.

### Identity — the exact SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Sketch row's `schema_version`, `payload`, `payload_hash` |
| `capabilities` | the `sketch.constraints.v1` row, upserted with `required = 1` by the existing policy when the saved Sketch has constraints (already present when the source had them) |
| `meta` | `modified_at` |
| every other table and column, including `deps` and `topology_refs` | nothing |

Preserved: every object, curve, the Chamfer (UUID, `previous`, edge, joint,
distance), the Extrude (height), the Body tip, every dependency and topology
reference, the stored coordinates (the solver's starting approximation — solved
numbers are never written back). Constraint UUIDs change only by the existing
atomic remove-then-add; closure Coincidents are never removable and are added with
the first addition.

### The lifecycle after constraints

* `edit-chamfer-distance` accepts the constrained plate and keeps its constraints
  byte for byte. Preparation checks only that the distance is finite and at least
  0.001 mm; the true maximum is the rebuild's, on the solved predecessor.
* `edit-extrude` accepts it too and keeps the constraints; the Chamfer adds no
  bound on the height.
* The coordinate editor (§29C) refuses while a **user** constraint exists, naming
  the Chamfer and pointing at the constraint editor. Removing every user
  constraint leaves the closure Coincidents, and the closure-only coordinate edit
  works again. Nothing is silently dropped and no solved coordinate is baked in.
* The strict reference rule of the shared job covers all of it.

### Writer, version, references

`Document::write_sketch_constraints` re-derives inside its transaction: the Sketch
row is the one prepared; the frame, the managed family and the Chamfer context are
read again from the current document; the prepared constraint list must equal the
stored list without the removed UUIDs followed by the added ones; curves and plane
are unchanged. A forged or stale prepared value is refused and nothing is written.
The existing job keeps the content-version guard, `copy_access`, alias/no-clobber,
cancellation, strict resolved references after the cold rebuild, atomic
publication and exit 7. No new payload, capability or SQLite version is needed.

### Discovery (`inspect --json`), additive

* `sketches[].constraint_edit` of the base Sketch becomes `available` and gains
  `chamfer_base` (the same object as `features[].chamfer_base`) — `null`
  everywhere else.
* `features[].chamfer_base`, `sketches[].chamfer_base` and `chamfers[]` gain
  `profile_constrained`. When it is `true`, their `corner_mm` is the **stored**
  approximation, `chamfers[].distance_edit.max_distance_mm` is `null` (the bound is
  judged on the solved plate when a copy is published), and `sketches[].editable`
  is `false` for the coordinate editor with a refusal that names the Chamfer.
* Kernel-free, one pinned snapshot; every old field, type, operation name and exit
  code is unchanged; inspection proves no geometry.

### UI

The existing Edit constraints form and worker (Undo/Redo, Replace length,
Save/Cancel, refusal and stale-reply preservation). One context line names the
Chamfer, the two Lines of its corner and its distance, says the coordinates shown
are the stored ones, and says a new copy is published only if the solved plate is
still this rectangle with room for the distance. The coordinate form refuses a
constrained plate with that reason. The form holds no copy of the geometry rule.

### Compatibility

The saved Sketch uses the existing constraint payload (Sketch v2) and capability
`sketch.constraints.v1`. A build before this one refuses to rebuild the chamfered
plate once its base carries constraints (its Chamfer reader requires a free or
closure-only profile) — a typed refusal, never a part of the wrong shape; checked
with the real CLI of the base commit.

### Out of scope

Creating a Chamfer on a constrained source, a second Chamfer, a Fillet or Cut
beside it, an arbitrary quadrilateral, a rotated plane, new constraint families,
live preview, picking edges, in-place Save.
