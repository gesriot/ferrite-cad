# §30J — add a Fillet inside the open document

Marker `FCAD_30J_ADD_FILLET_SESSION`. [Verification and real-window recipe](add-fillet-session-verification.md).
The existing [single-edge Fillet](single-edge-fillet.md), with its
[constrained plate](fillet-constrained-plate.md) and [0–4 Fillet history](rectangular-fillet-history.md)
contracts, joins the [document session](document-session.md), beside the existing
Fillet radius edit of [§30G](fillet-radius-session-apply.md) and the session Add of a
Cut in [§30I](add-circular-cut-session.md). It is the same operation moved, not a new
geometry class.

## What a person can do

After unsaved base edits (or on a clean document), open **Fillet edge of … — Body
UUID…** on the accepted model, choose one offered vertical corner (named by the base
Extrude and its two Line UUIDs), type a radius and press **Add fillet**. No file dialog
and no confirmation: the current selection and valid radius are read. The new Fillet
is one step of the common document history and is offered at once by **Edit Fillet
radius …** (§30G), whose Apply radius changes it inside the same session. Undo/Redo
cross base, Add and radius steps; exports read the accepted unsaved snapshot; only
Save writes the user's file and Save As keeps the previous one.

**Confirm draft edge and radius** (formerly *Apply fillet*) only records the draft the
existing copy workflow saves; the form has no Undo/Redo of its own. **Save fillet
copy…** keeps its meaning on a clean document; with unsaved changes it is disabled
with its reason and the handler refuses again in words before any dialog.

## Route

`StepTicket::add_edge_fillet` pins the form version, reads the current accepted
snapshot and calls the existing `fillet_edge_copy` with the existing
`EdgeFilletRequest` (Body by its saved UUID, edge by base feature and Line joint). The
worker uses the existing lease, cleanup and two-phase scene acceptance. No second
writer, evaluator, copier, SQLite route or history; no CLI, JSON, payload, schema,
capability, archive, ABI or dependency change. One production predicate
(`can_add_fillet`) serves the button and the handler: the Add form allows its own
Add; other forms, load/export and session or copy work exclude it.

## Rules

* One accepted Add = one document step. The new Fillet and its references get UUIDs
  only from the real operation; Redo returns exactly those from the accepted snapshot
  and never runs the job again. The Body UUID and every older identity are unchanged;
  the new Fillet's `previous` is the old tip and the Body tip is the new Fillet. A new
  accepted step after Undo drops Redo; a refused one does not.
* The class is unchanged: axis-aligned XY rectangle of four Lines, literal forward
  Blind NewBody, 0–4 Fillets on distinct vertical corners, free/closure-only or the
  already supported constrained base, the same reader, solved-geometry evaluator,
  bounds (`MIN_RADIUS_MM` 0.01 mm to `MAX_RADIUS_FRACTION` 0.5 × the shorter adjacent
  side, and the flat length kept beside a neighbouring Fillet) and
  UUID/reference policy. A fifth Fillet, a corner already rounded, a radius out of
  bounds or one the solved plate has no room for are the domain's or evaluator's
  refusals. A constrained base without the solver refuses. A busy worker starts no
  second job.
* Refusal, cancellation, stale form or answer, and failed scene/GPU preparation keep
  the accepted scene, history/Redo, checkpoint, logical file and the exact draft
  (corner and radius text), with the outcome visible in the form. The unaccepted
  private file is removed. Nothing is saved to clear dirty.

## Remains copy-only

Add Chamfer, the clean-only copy routes (Cut copy, Fillet copy, Fillet radius copy,
Chamfer distance copy) and creation of a new document or Revolve. No arbitrary edges,
Cut/Chamfer beside a Fillet, feature deletion or reordering, picking, preview, autosave
or tabs. §30, Milestone 5C and the product remain open; the next slice has not started.
