# §30L — a new unsaved document and its first Save

Marker `FCAD_30L_UNNAMED_DOCUMENT_SESSION`. Decision: [ADR 0005, §30L](decisions/0005-document-session.md#30l-a-new-document-is-a-session-before-it-is-a-file).
Verification and the real-window recipe: [unnamed-document-session-verification.md](unnamed-document-session-verification.md).

## What a person can do

**New** (Empty or sample plate) and the drawing forms (**Create sketch + Extrude**,
the Circle and annulus Extrudes, **Revolve** and **Revolve by angle**) make a new
document without any file dialog. It is shown as **Untitled** with the unsaved mark.
Height, vertices, constraints, Circle/annulus, angle, Cut, Fillet and Chamfer Apply
and Add, Undo/Redo and STL/FBX export work before the first Save, wherever the
document's own catalogue offers them. **Save** (or **Save As**) asks where to put the
file the first time; after that Save writes in place under the existing version
guard. Open, Quit and making another new document ask Save / Discard / Cancel while
the document is untitled or has unsaved changes.

## Rules

1. An untitled session has no logical path and no saved checkpoint
   (`DocumentSession::logical_path() == None`). Its private snapshot, the working
   directory or an invented `untitled.fcad` are never used as "the saved file". The
   private directory is not shown in the title, a form, a dialog or an error.
2. Until a Save is published `is_dirty()` is true, Empty included, and Undo back to
   the created version does not change that. After the first Save the existing
   model-content comparison, no-op and Redo rules and the in-place version guard apply.
3. Creation runs on a worker inside the candidate session's own private directory
   through the existing `create_document_with_kernel` (Empty and the sample plate need
   no kernel and no solver; drawn profiles are cold-checked as before). The candidate
   is accepted only with its successfully prepared picture (`Bind::Open`). A refusal,
   cancellation, stale answer or scene/GPU failure keeps the previous session, picture,
   history and typed draft, and removes the candidate's directory.
4. Nothing writes a user `.fcad` before Save. Exports read the accepted private
   version; their dialogs suggest a name from **Untitled**, never the private path,
   and refuse destinations inside the private directory. Copy workflows stay
   available only on a saved, clean document and say why otherwise.
5. The first Save is the existing no-clobber Save As: a cancelled dialog, an occupied
   path, a path inside the private directory (or through an alias of it) or a failed
   publication leave the session, history, picture and drafts unchanged. Only a
   published save sets the logical path and checkpoint; the document id, every UUID
   and every reference are the ones the session already had, and history stays.
6. Open, Quit and creation of another document are guarded at the moment of
   replacement. Save from the question continues exactly once after it is published;
   Cancel, a cancelled dialog or a failed save continue nothing; Discard keeps the old
   session until the new document is accepted. A new document starts its own history.

## Unchanged

The CLI `create`, `create-sketch-extrude`, `create-sketch-revolve`, the analytic
create commands and their JSON/text/exit contracts. No new geometry class, no second
writer, copier or evaluator, no CLI session/RPC, autosave, recovery, tabs or persistent
revisions. §30, Milestone 5C and the product remain open.
