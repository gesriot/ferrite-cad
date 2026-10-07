# §30D — apply the geometry of a saved Circle or annulus inside the open document

Marker `FCAD_30D_ANALYTIC_SKETCH_SESSION_APPLY`. This moves **one family of
editors** — the two existing editors of an analytic Sketch, *Edit circle* and
*Edit annulus* — onto the document session of [§30A](document-session.md), beside
the height ([§30A](document-session.md)), the vertices ([§30B](sketch-session-apply.md))
and the constraints ([§30C](constraint-session-apply.md)). Nothing else: no new
geometry, no constraint, no document format, no command, no JSON, no writer or
evaluator, no second history. [Verification and the macOS window scenario](analytic-sketch-session-apply-verification.md).

## What a person can do

1. Open a `.fcad` whose saved Sketch is one analytic Circle or one concentric pair
   (the classes of [`edit-circle`](edit-circle-copy.md) and
   [`edit-annular`](edit-annular-copy.md)).
2. Optionally Apply a new height (or vertices or constraints of another Sketch)
   first. The document is now unsaved.
3. *Edit circle…* / *Edit annulus…* is still offered with unsaved changes. Change the
   centre and the radius (radii), then press **Apply circle** / **Apply annulus**. No
   file dialog. The reused `edit-circle` / `edit-annular` operation runs on the accepted
   snapshot (cold rebuild, strict reference check), the picture is built, and the
   version becomes current in the statement that shows it. The form closes: it described
   the picture that was replaced. The height is not part of the request.
4. **Undo / Redo** (toolbar, `Cmd/Ctrl+Z`, `Cmd/Ctrl+Shift+Z`) pass through height,
   vertex, constraint and geometry steps alike. Export STL/FBX reads the accepted
   working model. **Save** / **Save As** write it; the file the document was opened
   from changes only at Save.
5. *Save edited circle copy…* / *Save edited annulus copy…* (the old workflow) is still
   there on a clean document. With unsaved changes it is withheld and the form says why.

## The two buttons that used to be called "Apply"

The old forms had **Apply circle change** / **Apply annulus change**. It never
changed the document: it confirmed the typed fields into the draft's own history so
that *Save edited … copy…* could be offered. That is now **Confirm draft numbers**,
and it still does exactly that, no more (draft Undo/Redo, the bound of 128, no step for
an equivalent spelling of the same number). **Apply circle** / **Apply annulus** is
the only action that changes the open document, and it applies the numbers as typed
whether or not they were confirmed. Pressing the confirmation never asks the window
for anything.

## Boundaries (decided, and tested)

* **One Apply = one document step.** Apply is offered only when the typed numbers are
  not the stored ones (for the annulus: not both stored centres, both radii); an
  equivalent spelling is the same number. If a request nevertheless leaves the model as
  it was, the session says *No change*: nothing is recorded, Redo survives, no file
  is left. A new successful step after Undo cuts the Redo, an unsuccessful one does not.
* **Availability is one predicate** (`can_apply_analytic`, for the button and the
  command): the form of a saved Circle or annulus is open, no creation or other form is
  open or running, nothing is loading or exporting, no session operation is in flight
  and a document is open. The form is not an operation blocking itself (the trap
  `Creates::busy()` set for §30B). While a worker is busy the form offers nothing, and a
  press that arrives anyway starts no second worker.
* **Draft Undo/Redo** are the form's (buttons only); **document Undo/Redo** wait while
  any form is open.
* **A form never applies to a version it was not opened on**; the session step refuses a
  request whose version is not the current one. Every accepted scene change ends the forms
  about saved objects (`finish_session_change`), exactly then, and the next form starts with
  what the window already said (`Editor::dismiss` carries the session flags).
* **Failure keeps everything.** A refusal (a radius or a wall the document will not store),
  a stale version, a cancellation between computing and committing, or a picture that cannot
  be shown accept no version: the scene, the history (the Redo included), the checkpoint, the
  source file and the typed draft are as they were, and no private file is left.
* **The refusal is readable.** The form's window used to cover the toolbar line that says
  why an Apply failed. The form now draws the session's own last line (`sessions.status`,
  passed in each frame, never copied into a second owner) under its buttons.
* **Identities.** Nothing here creates an identity: the Sketch, the curve(s), the Extrude,
  the Body and every topology reference keep their UUIDs, the annulus keeps its roles by
  curve UUID (never by position in the Sketch), and the saved document equals the command
  line's copy in every SQL cell but the write stamp.
* **Constraints stay out.** A profile with constraints is refused by the existing catalogue
  and is not made editable here. The catalogue is read again for every accepted version, so
  a constraint applied through the session (§30C) withdraws the geometry editor and removing
  it offers the editor again (tested natively).
* **No sketch solver.** Neither edit runs the solver; a build with Open CASCADE and no
  PlaneGCS applies them.
* **Open / New / Quit** keep their Save/Discard/Cancel guard.

## What is reused, and what is not

Reused: `edit_circle_copy` / `edit_annulus_copy` (through `StepTicket::edit_circle` and
`StepTicket::edit_annulus`), the session's two-phase step, its history, dirty-by-comparison,
Save, the working-copy lease for exports and the private-folder protections. Not introduced: a
second copier, evaluator, history or writer, or any write to the logical file before Save. The
other copy-only editors (Add Chamfer, *Edit extrusion…* copy; Add Fillet until [§30J](add-fillet-session.md)) stay copy
workflows.

## Not claimed

The whole of §30, Milestone 5C or the beta; Windows or macOS from this container; a window
run from this container; that the historical OOM is fixed.

The saved partial Revolve angle joins the session in [§30E](revolve-angle-session-apply.md).

Existing Fillet radii now Apply through the same session, including while dirty:
[§30G](fillet-radius-session-apply.md). Fillet copy remains clean-only; a new Fillet joined
the session in [§30J](add-fillet-session.md).

Existing Chamfer distances now Apply through the same session, including while dirty:
[§30H](chamfer-distance-session-apply.md). Distance copy and Add Chamfer remain clean-only.
