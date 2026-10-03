# §30B — move the vertices of a saved Line Sketch inside the open document

Marker `FCAD_30B_SKETCH_SESSION_APPLY`. This moves **one existing family of
editors** onto the document session of [§30A](document-session.md) and nothing
else: no new geometry, no second Chamfer, no new UI framework, no change to
`edit-sketch-copy` or to any JSON. [Verification and the macOS window
scenario](sketch-session-apply-verification.md).

## What a person can do

1. Open a `.fcad` with a saved Line Sketch the *Edit Sketch…* form already
   accepts (the offset, fractional polygon of the examples; a rectangle under one
   Chamfer; and whatever else that form already edits).
2. Optionally **Apply** a new height first (§30A). The document is now unsaved.
3. *Edit Sketch…* is still available with unsaved changes. Change the vertices in
   the form (exact coordinates, drag, *Undo draft* / *Redo draft*, *Restore saved
   vertices*) and press **Apply vertices**. No file dialog. The same two-phase step
   as Apply height runs: the existing `edit-sketch-copy` operation on the accepted
   snapshot, a cold rebuild with strict reference checking, then the picture; the
   version becomes current in the same statement that shows it. The form closes —
   it described the picture that was replaced.
4. **Undo / Redo** (toolbar, `Cmd/Ctrl+Z`, `Cmd/Ctrl+Shift+Z`) move over height
   steps and vertex steps alike. Export STL/FBX reads the accepted working model.
   **Save** / **Save As** write it. The file the document was opened from changes
   only at Save.
5. *Save edited copy…* (the old workflow) is still there on a clean document. With
   unsaved changes it is withheld, and the form says why.

## Boundaries (decided, and tested)

* **Draft Undo/Redo** (the form's buttons; no keyboard shortcut) change only the
  typed draft. A drag is one checkpoint; *Restore saved vertices* is one step.
  Their history ends when the change is applied, the form is cancelled, or the
  document moves to another version.
* **Document Undo/Redo** (toolbar and `Cmd/Ctrl+Z`) are unavailable while *any* form
  is open (`form_open`), so one cannot silently end the other's work. Apply or
  Cancel the form first. `Cmd/Ctrl+Z` inside a text box stays the text box's.
* **One Apply = one document step.** A request that changes no coordinate is
  *No change*: nothing is recorded and Redo survives. A new step after Undo drops
  the Redo it replaces, only after it succeeded.
* **A form never applies to a version it was not opened on.** The request carries the
  version of the picture the form was opened on; the session step refuses a request
  whose version is not the current one ("the document changed after this form was
  opened"). Independently, every accepted scene change (Apply, Undo, Redo, Open)
  ends the forms about saved objects (`finish_session_change`); a drawing for a
  *new* document is not about the open one and stays.
* **Failure keeps everything.** A refused edit (the Chamfer no longer fits, a
  collapsed plate), a cancellation between computing and committing, a stale answer
  or a picture the device cannot show leave the accepted scene, the file, the history,
  the checkpoint and the typed draft as they were, and no private file behind.
* **Open / New / Quit** keep their §30A Save/Discard/Cancel guard and wait while the
  document is being replaced; the macOS Quit guard and the Command mapping for
  non-Latin layouts are unchanged.

## What is reused, and what is not

Reused: `edit_sketch_copy` (through `StepTicket::edit_sketch_vertices`), the
session's two-phase step, its history, dirty-by-comparison, Save, the working-copy
lease for exports, and the private-folder protections. Not introduced: a second
copier, evaluator, history or writer, or any write to the logical file before Save.

Private paths stay private: the form and the dialogs name the user's file and
folder (§30A), and nothing can be published inside the session's own folder.

## Refusals that stay explicit

A **dimensioned** (constrained) plate offers no vertex form — its Lines are the
solver's; the §29D constraint editor and the height Apply work on it as before
(constraints stay byte for byte). The other saved-object editors (Revolve angle,
Cut, Fillet, Chamfer) still write a copy and
are unavailable while the document has unsaved changes; moving them onto the
session is later work. The constraint editor joined the session in
[§30C](constraint-session-apply.md), and the Circle and annulus editors in
[§30D](analytic-sketch-session-apply.md). `edit-sketch-copy` and every JSON are unchanged.

## Not claimed

The whole of §30, Milestone 5C or the beta. Native evidence in this slice is the
plain polygon and the rectangle under a Chamfer, and the refusal of a dimensioned
plate; the form's other families run through the same step but were not re-run
natively in a session here.
