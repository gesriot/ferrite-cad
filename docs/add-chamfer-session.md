# §30K — add a Chamfer inside the open document

Marker `FCAD_30K_ADD_CHAMFER_SESSION`. [Verification and real-window recipe](add-chamfer-session-verification.md).
The existing [rectangular-corner Chamfer](rectangular-corner-chamfer.md) joins the
[document session](document-session.md), beside the existing Chamfer distance edit of
[§30H](chamfer-distance-session-apply.md) and the session Adds of a Cut
([§30I](add-circular-cut-session.md)) and a Fillet ([§30J](add-fillet-session.md)).
It is the same operation moved, not a new geometry class.

## What a person can do

After unsaved base edits (or on a clean document), open **Chamfer edge of … — Body
UUID…** on the accepted model, choose one offered vertical corner (named by the base
Extrude and its two Line UUIDs), type a distance and press **Add chamfer**. No file
dialog and no confirmation: the current corner and valid distance are read. The new
Chamfer is one step of the common document history and is offered at once by **Edit
Chamfer distance …** (§30H), whose Apply distance changes it inside the same session.
Undo/Redo cross base, Add and distance steps; exports read the accepted unsaved
snapshot; only Save writes the user's file and Save As keeps the previous one.

**Confirm draft edge and distance** (formerly *Apply chamfer*) keeps its exact meaning:
it records the corner + distance in the form's own request history, which **Undo
request** / **Redo request** step through; that history never touches the model and is
not the document's Undo/Redo. **Save chamfer copy…** keeps its meaning on a clean
document; with unsaved changes it is disabled with its reason and the handler refuses
again in words before any dialog. Apply distance is unchanged.

## Route

`StepTicket::add_edge_chamfer` pins the form version, reads the current accepted
snapshot and calls the existing `chamfer_edge_copy` with the existing
`EdgeChamferRequest` (Body by its saved UUID, edge by base feature and Line joint). The
worker uses the existing lease, cleanup and two-phase scene acceptance. No second
writer, evaluator, copier, SQLite route or history; no CLI, JSON, payload, schema,
capability, archive, FFI or dependency change. One production predicate
(`can_add_chamfer`) serves the button and the handler: the Add form allows its own
Add; other forms, load/export and session or copy work exclude it.

## Rules

* One accepted Add = one document step. The new Chamfer and its references get UUIDs
  only from the real operation; Redo returns exactly those from the accepted snapshot
  and never runs the job again. The Body UUID and every older identity are unchanged;
  the new Chamfer's `previous` is the base Extrude and the Body tip is the new Chamfer.
  A new accepted step after Undo drops Redo; a refused one does not.
* The class is unchanged. Add Chamfer accepts only a **free or closure-only**
  axis-aligned XY rectangle of four Lines with a literal forward Blind NewBody and no
  Chamfer, Fillet or Cut yet (`saved_target` → `require_free_profile`). The managed
  constraint family that §29D/§30H allow under an **already saved** Chamfer does not
  open creation: after an unsaved constraint Apply the form is not offered and an Add
  request is refused with the same domain reason. A second Chamfer, mixed histories,
  other edges or planes are refused as before. Bounds are the domain's:
  `MIN_DISTANCE_MM` 0.001 mm to `max_distance_of` = the shorter adjacent side less
  `MIN_FLAT_MM` 0.01 mm, measured along each face, never clamped. A busy worker starts
  no second job.
* Refusal, cancellation, stale form or answer, and failed scene/GPU preparation keep
  the accepted scene, history/Redo, checkpoint, logical file and the exact draft
  (corner, distance text and request history), with the outcome visible in the form.
  The unaccepted private file is removed. Nothing is saved to clear dirty.

## Remains copy-only

The clean-only copy routes (Cut copy, Fillet copy, Fillet radius copy, Chamfer copy,
Chamfer distance copy) and creation of a new document or Revolve. No constrained
creation, arbitrary edges, Cut/Fillet beside a Chamfer, feature deletion or
reordering, picking, preview, autosave or tabs. §30, Milestone 5C and the product
remain open; the next slice has not started.
