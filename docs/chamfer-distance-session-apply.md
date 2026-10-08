# §30H — edit an existing Chamfer distance inside the open document

Marker `FCAD_30H_CHAMFER_DISTANCE_SESSION_APPLY`.
[Verification and real-window recipe](chamfer-distance-session-apply-verification.md).
The existing [distance edit](rectangular-corner-chamfer.md) of the one §29A Chamfer,
over the plate whose [height](edit-chamfer-base-height.md),
[coordinates](edit-chamfer-base-sketch.md) and [constraints](edit-chamfer-base-constraints.md)
already change under it, joins the [document session](document-session.md).

## What a person can do

After unsaved height, base vertices or constraints Apply, **Edit Chamfer distance
… — UUID… (d… mm)…** opens on the accepted snapshot. **Apply distance** reads the
current valid text and changes the distance of that existing feature, selected by
its UUID, without a file dialog or a prior confirmation. The source is the current
accepted private snapshot, never the logical file on disk. Each distance Apply is
one step of the common document history: Undo/Redo crosses base edits and different
distance edits; unsaved STL/FBX exports read the accepted snapshot. Only Save
changes the logical file; Save As keeps the prior file. A successful new step after
Undo truncates Redo; a refusal or no-op keeps it.

**Confirm draft number** (formerly the form's *Apply distance*) only confirms the
text for **Save distance copy…** and records it in the form's own request history.
**Undo request** / **Redo request** step through those confirmed requests; they are
the form's, not the document's Undo/Redo, and move no model. A clean copy remains
available, including a numerical no-op; a dirty copy is disabled with its reason in
the form and refused again, in words, before any dialog. **Add Chamfer** and its form
remained the previous copy-only route in this slice; since [§30K](add-chamfer-session.md)
that form has its own document route, **Add chamfer**, and never goes through this
Apply distance.

## What the library and the window own

`StepTicket::edit_chamfer_distance` checks the form's `DocumentVersion`, then calls
`edit_chamfer_distance_copy` with the existing `EditChamferDistanceRequest`, the
ticket's accepted snapshot, the exact chosen feature UUID and the next private path.
`sessions::spawn_apply_chamfer_distance` owns the kernel on its worker. The existing
copier, writer and its re-derivation, evaluator, version guard, model no-op
comparison, export lease, cleanup and two-phase scene acceptance are reused. No
second history, writer, schema, capability, request/CLI/JSON version, geometric
algorithm or supported document class is introduced.

One production predicate (`can_apply_chamfer_distance`) controls the button and the
handler. The distance form permits its own Apply; Add Chamfer, any other form,
New, load/export/edit workers and session work exclude it. A numerically unchanged
distance, including another string spelling, offers no Apply. Model comparison
independently discards a worker no-op and releases the unaccepted private file
without losing Redo or the checkpoint. The CLI still publishes its no-op copy.

Domain refusal, evaluator/solved-bound refusal, cancellation, a stale form or
answer and a failed scene/GPU preparation keep the accepted scene, history/Redo,
dirty/checkpoint, file and the exact typed draft. The document outcome is readable
inside the open form. Successful scene acceptance closes the form about the
previous version.

## Identity and policy

One existing Chamfer on a vertical corner of the supported rectangular plate. The
operation changes only the Chamfer row's payload/hash and `meta.modified_at`. Its
UUID, `previous`, the Body tip, the joint of the same two Line UUIDs, its
`EdgeChamferFace` and every other ref (including additional resolvable refs owned
by the base), stored geometry/constraints and every other SQL cell are unchanged.
UI/CLI comparison maps no identity; selection follows the UUID, also with duplicate
names and SQLite rowids/ordinals that disagree with the feature history. After a
constraints Apply both routes start from the same accepted constrained snapshot.

The policy stays the domain's and the evaluator's: `MIN_DISTANCE_MM = 0.001`, at
most `min(adjacent sides) − 0.01` by the same `max_distance_of` expression, exact at
the bound, refused at `next_up`, never clamped. `distance_mm` is along each adjacent
face, not the slanted flat's width `d·√2`. On a constrained base the upper bound is
the solved geometry's: the stored approximation neither limits an allowed number
nor admits a refused one, and the refusal names the Chamfer UUID, its joint and the
numbers. A real solver conflict stays a typed `constraint` refusal. The UI asks
only the document's own check; strict refs are unchanged.

## Not here

Add Chamfer/Fillet/Cut through the session, a second Chamfer, mixed feature
histories, removal or reordering, another corner or edge, uneven or angle Chamfers,
a new profile, preview/picking, autosave/recovery/tabs, UI redesign. §30,
Milestone 5C and the product remain open; no next slice starts.
