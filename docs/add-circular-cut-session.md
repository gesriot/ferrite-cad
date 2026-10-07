# §30I — add a new circular Cut inside the open document

Marker `FCAD_30I_ADD_CIRCULAR_CUT_SESSION`. [Verification and real-window recipe](add-circular-cut-session-verification.md).
The existing [Add Cut](circular-cut-copy.md) with its [1–16-link history](circular-cut-history.md),
[explicit end](circular-cut-through-all.md) and [Line-polygon](polygon-cut-history.md)
contracts joins the [document session](document-session.md), beside the existing-Cut
edit of [§30F](cut-session-apply.md). It is the same operation moved, not a new
geometry class.

## What a person can do

After unsaved base or Cut edits (or on a clean document), open **Cut circle into … —
Body UUID…** on the accepted model, type the centre, radius and Blind depth or
explicit Through all, and press **Add cut**. No file dialog and no draft confirmation:
the current valid fields are read. The new Cut is one step of the common document
history, immediately offered by **Edit cut …** (§30F), and a second Add can follow.
Undo/Redo cross base, Add and edit steps; exports read the accepted unsaved snapshot;
only Save writes the user's file and Save As keeps the previous one.

**Confirm draft numbers** (the button formerly labelled *Apply cut* on the Add form) and
the form's **Undo/Redo** only move the draft's own checkpoints; they are never the
document's history. **Save cut copy…** keeps its meaning on a clean document; with
unsaved changes it is disabled with its reason and the handler refuses again in words
before any dialog.

## Route

`StepTicket::add_circular_cut` pins the form version, reads the current accepted
snapshot and calls the existing `circular_cut_copy` with the existing
`CircularCutRequest` (Body named by its saved UUID). The worker uses the existing
lease, cleanup and two-phase scene acceptance. No second writer, evaluator, copier,
SQLite route or history; no CLI, JSON, payload, capability, ABI or dependency change.
One production predicate (`can_add_cut`) serves the button and the handler: the Add
form allows its own Add; other forms, load/export and session work exclude it.

## Rules

* One accepted Add = one document step. The new feature, tool Sketch, Circle and
  references get UUIDs only from the real operation; Redo returns exactly those UUIDs
  from the accepted snapshot and never runs the job again. The Body UUID and every
  older identity are unchanged; the new Cut's predecessor is the previous tip and the
  Body tip is the new Cut. After Undo the undone Cuts leave discovery; after Redo they
  return with the same UUIDs. A new accepted step after Undo drops Redo; a refused one
  does not.
* The class is unchanged: unconstrained simple XY Line polygon base, 0–16 Cuts, the
  real finite outline (not the box), all disk clearances, Blind or ThroughAll, face
  provenance and strict references. ThroughAll stays intent and follows later height;
  Blind keeps its absolute depth. A constrained base is refused by the catalogue as
  before (`read_history` is called with `constrained_base=false`), also after an
  unsaved constraint Apply. A 17th Cut, a tool outside the outline or touching another
  disk are the domain's refusals. A busy worker starts no second job; repeating the
  same disk is a real attempt and receives the domain's clearance refusal.
* Refusal, cancellation, stale form or answer, and failed scene/GPU preparation keep
  the accepted scene, history/Redo, checkpoint, logical file and the exact typed draft,
  with the outcome visible in the form. The unaccepted private file is removed. No
  private path reaches the title or a dialog.

## Remains copy-only

Add Fillet, Add Chamfer, the clean-only copy routes (Cut copy, Fillet radius copy,
Chamfer distance copy) and creation of a new document or Revolve. No feature deletion
or reordering, face picking, plane choice, preview, autosave or tabs. §30, Milestone 5C
and the product remain open; the next slice has not started.
