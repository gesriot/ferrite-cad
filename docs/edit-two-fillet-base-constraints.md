# §28K — edit the constraints of a plate rounded twice, in a new copy

[Executed verification and limitations](edit-two-fillet-base-constraints-verification.md).
[The same edit under one Fillet](fillet-base-constraints.md);
[two sequential Fillets](sequential-edge-fillets.md);
[their radii](edit-sequential-fillet-radii.md),
[height](edit-two-fillet-base-height.md) and
[Sketch coordinates](edit-two-fillet-base-sketch.md).

A person opens a plate that §28G rounded twice — Extrude → Fillet 1 →
Fillet 2 → Body tip — adds, replaces or removes Line constraints of its base
Sketch in the existing **Edit constraints** form, and saves a new `.fcad`. The
solved rectangle is the new plate; both roundings stay on the same named
corners with the same radii. An agent does the same with the existing
`edit-sketch-constraints-copy`. There is no new command, request format, copy
pipeline, solver or geometric route: this is §28E's edit on §28H–§28J's
reading of the history.

## Contract recorded before implementation

### The supported source

Exactly the §28G class, read again from the saved history on every
discovery, preparation and write by the reader the radius, height and
coordinate edits use for two Fillets (`fillet_radius::fillets_over_plate` →
`saved_sequential_fillet`, over `cut_edit::saved_history_under_fillets`): one
untransformed XY datum; one Sketch of four Lines whose **stored** Lines are an
axis-aligned rectangle, carrying no constraint or only the constraint
editor's managed Line family (§25E: H/V, Start/End length, one Fixed endpoint,
equal length, Parallel/Perpendicular, and the Coincident closure links); one
forward literal Blind `Extrude`/`NewBody`; **Fillet 1** with `previous` =
`edge.feature` = that Extrude; **Fillet 2** with `previous` = Fillet 1,
`edge.feature` = that Extrude, another corner, the Body's tip; no Cut and no
other object; exactly the plane, profile, two predecessor and one body-tip
dependencies; Fillet 1's seven §28A names and Fillet 2's eight §28G names, by
meaning. The selected Sketch must be that plate's base Sketch.

The add-Fillet editor keeps reading `fillet_over_plate` (exactly one Fillet)
and keeps refusing; so does any history outside the class above. Nothing is
admitted by an object count.

### Typed context

`ConstraintSketchChoice.fillet` stays the Fillet on the base (Fillet 1);
`second_fillet` carries Fillet 2 (`history_index` 2, `previous` = Fillet 1,
`base_feature` = the Extrude). Both are the `SavedFillet`s
`fillets_over_plate` returns, carried into `PreparedSketchConstraints` and
compared whole by the writer's re-derivation. Fillet 2 is never described as
a direct child of the base.

### What the solved plate must be

The stored coordinates stay the solver's **starting guess** and are never
replaced by solved ones. Their lengths and bounds prove nothing about the
part (for a Sketch that carries constraints, the reader asks only that each
radius is a finite number of at least the minimum), and the stored corners
are not judged. At every rebuild — the copy's strict cold rebuild before
publication, and every later one — the evaluator asks, of the **solved** Lines
of the same four UUIDs, what it already asks for two Fillets
(`evaluable_fillet`, unchanged): the same four Lines in stored order, an
axis-aligned rectangle, every Line on its saved side, each saved joint a
corner, each saved radius within ½ × the shorter solved side at its corner
and, on adjacent corners, the pair rule in history order (`check_pair`: the
Line they share keeps at least `MIN_RADIUS_MM` flat between the arcs;
opposite corners share no Line). Nothing is clamped, reselected or
approximated, no epsilon is added, and no second check is written. A plate the
solved Lines make too small for either radius, for the flat, or that loses a
side is `input`, naming the Fillet's joint UUIDs, and publishes nothing; a
conflict or redundancy of the solver is its own `constraint` refusal with the
real constraint UUIDs.

### SQL allowlist

Exactly §28E's, unchanged: `objects`, the base Sketch row's `schema_version`,
`payload`, `payload_hash`; `capabilities`, the `sketch.constraints.v1` row,
upserted with `required = 1` by the existing constraints contract when the
saved Sketch has constraints (already present when the source had them);
`meta.modified_at` only if the existing writer stamps it. Every other table
and column, including `deps` and `topology_refs`, nothing; row counts are
equal except for that one possible capability row. The curves, their order and
plane, both Fillet rows (UUIDs, `previous`, edges, radii), the Extrude and its
height, the Body tip, `document_id` and every ref are kept. Constraint UUIDs
follow §28E: every untouched constraint and every closure Coincident keeps its
UUID; only new rules and newly needed closure links mint one; removals
disappear. Payload/capability versions change only as the constraints
contract already says — by what the Sketch holds, not by the presence of two
Fillets.

### Writer, version, rebuild and cache

`Document::write_sketch_constraints` is unchanged in shape: inside its
transaction it re-derives the edit from the current document (frame read
again, both Fillets compared whole, stored list minus removals plus additions,
curves and plane unchanged, result inside the managed family). Version,
alias, no-clobber, cancellation, cleanup and strict references are the copy
job's. Every saved name must resolve after the cold rebuild, both cylinders
under their own names on their own solved axes. The base Extrude's key covers
the constraints; each Fillet is keyed by its predecessor, so a changed
constraint misses the plate and both Fillets and a repeat hits; the height
and each radius remain editable afterwards. Removing every user constraint
leaves the closure links, so the solved plate is the stored one and the §28J
Sketch-coordinate edit is offered again.

### Clients

* `inspect --json`: `constraint_edit.fillet_base` of the base Sketch gains the
  additive `second_fillet` (Fillet 2 with its `previous_feature_id` = Fillet 1,
  `history_index` 2, edge, `stored_corner_mm`, `radius_mm`); `null` with one
  Fillet. Other fields keep their shape and type; `constraint_edit.available`
  becomes `true` for the class above.
* `edit-sketch-constraints-copy`: unchanged request, result, envelope, exit
  codes and stub order of checks.
* UI: the existing Edit constraints form names both Fillets in history order
  and what the copy must be; Undo/Redo over the whole request, Save Cancel and
  a worker refusal keep the draft, a published copy opens asynchronously.

### Out of scope

A third Fillet, arbitrary edges, Cut with Fillet, Chamfer, new constraint
kinds, automatic radius fitting, retargeting a corner, saving solved
coordinates, in-place Save, live preview.
