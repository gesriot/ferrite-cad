# §30C — apply the existing constraints of a saved Sketch inside the open document

Marker `FCAD_30C_CONSTRAINT_SESSION_APPLY`. This moves **one existing family of
editors** (*Edit constraints…*) onto the document session of
[§30A](document-session.md) and [§30B](sketch-session-apply.md), and nothing else:
no new constraint kind, no new geometry, no change to the solver policy, to
`edit-sketch-constraints-copy`, to any JSON or to the document format.
[Verification and the macOS window scenario](constraint-session-apply-verification.md).

## What a person can do

1. Open a `.fcad` whose saved Sketch the *Edit constraints…* form already accepts: a
   dimensioned offset plate (also under a Chamfer or Fillets), a circle, an annulus, a
   Revolve profile — whatever that form offers today.
2. Optionally Apply a new height or new vertices first. The document is now unsaved.
3. *Edit constraints…* is still available with unsaved changes. Add, **Replace
   length** (an exact remove of the stored length and an add, one atomic edit) or
   remove constraints in the draft as before, then press **Apply constraints**. No
   file dialog. The reused `edit-sketch-constraints-copy` operation runs on the accepted
   snapshot (solver, cold rebuild, strict reference check), the picture is built, and
   the version becomes current in the statement that shows it. The form closes, it
   described the picture that was replaced.
4. **Undo / Redo** (toolbar, `Cmd/Ctrl+Z`, `Cmd/Ctrl+Shift+Z`) pass through height,
   vertex and constraint steps alike. Redo returns the very same constraint UUIDs
   (nothing is re-generated). Export STL/FBX reads the accepted state. **Save** /
   **Save As** write it; the file the document was opened from changes only at Save.
5. *Save constraints copy…* (the old workflow) is still there on a clean document. With
   unsaved changes it is withheld and the form says why.

## Boundaries (decided, and tested)

* **One Apply = one document step**, and the only way to a step is a draft that changes
  something. An empty draft offers no **Apply constraints** (the document's own rule
  *1..512 changes* applies), so no step is made and the Redo of the draft and of
  the document survive. Whether a request is a no-change is judged by the existing
  model rules; replacing a constraint by one of equal geometry is a new UUID and is a
  change.
* **Draft Undo/Redo** are the form's (buttons); **document Undo/Redo** wait while any
  form is open (`form_open`).
* **Availability is one predicate** (`can_apply_constraints`): the constraints form of
  this Sketch is open, no creation or other form is running, nothing is loading or
  exporting, no session operation is in flight, and a document is open. The form is not
  an operation blocking itself (the trap `Creates::busy()` set for §30B). A press
  while a worker runs starts no second worker.
* **A form never applies to a version it was not opened on**; the session step refuses
  a request whose version is not the current one. Independently, every accepted scene
  change ends the forms about saved objects (`finish_session_change`), and the form is
  closed only after the new state is accepted.
* **Failure keeps everything.** A solver conflict, a Chamfer or wall that no longer
  fits, a stale version, a cancellation before commit, or a picture that cannot be shown
  accept no version: the scene, the history (including the Redo), the checkpoint, the
  source file and the typed draft are as they were, and no private file is left.
* **Open / New / Quit** keep their Save/Discard/Cancel guard.

## What is reused, and what is not

Reused: `edit_sketch_constraints_copy` (through `StepTicket::edit_sketch_constraints`),
the session's two-phase step, its history, dirty-by-comparison, Save, the working-copy
lease for exports, and the private-folder protections. Not introduced: a second copier,
solver path, evaluator, history or writer, or any write to the logical file before Save.
The other copy-only editors (creating Revolve, Add Cut, Add Fillet, Add Chamfer,
*Edit extrusion…* copy) stay copy workflows; the Circle and annulus editors joined the
session in [§30D](analytic-sketch-session-apply.md), and a saved partial Revolve
angle in [§30E](revolve-angle-session-apply.md).

## Review finding fixed here

`sketch::Editor::dismiss()` reset the nested constraints editor, so its session flags
were forgotten for the frame after a draft was replaced. It now carries them across.
(The test that found it is `a_new_accepted_version_ends_the_draft_and_keeps_what_the_window_said`.)

## Not claimed

The whole of §30, Milestone 5C or the beta; Windows or macOS here; a window run. The
families other than a dimensioned Line plate under a Chamfer and a Circle were covered
by the same step and the same headless predicates, but not run natively in a session.

Existing Fillet radii now Apply through the same session, including while dirty:
[§30G](fillet-radius-session-apply.md). Fillet copy and Add Fillet remain clean-only.

Existing Chamfer distances now Apply through the same session, including after
constraints Apply, judged on the solved plate: [§30H](chamfer-distance-session-apply.md).
Distance copy and Add Chamfer remain clean-only.
