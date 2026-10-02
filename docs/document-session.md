# §30A — a working document session: Apply, Undo/Redo, Save, Save As

The decision and its reasons are in
[ADR 5](decisions/0005-document-session.md); the record of what was run is in
[document-session-verification.md](document-session-verification.md). This page is
the contract: what a person sees, what the library guarantees, what is refused, and
what is deliberately not here.

## What a person can do

One route is complete end to end: the **Blind height of an Extrude**, on an ordinary
plate and on a plate under one §29A Chamfer with §29D constraints (the same plates
`edit-extrude` already accepts).

1. **Open** a `.fcad`. The title is the file's name followed by ` — FerriteCAD`.
2. Type a new height and press **Apply**. No file dialog appears. The window rebuilds
   the model with the existing copy operation, shows the new picture, and the title
   becomes `*name — FerriteCAD`. The panel says in words that the file on disk is
   unchanged until Save.
3. **Undo** (`Cmd/Ctrl+Z`) and **Redo** (`Cmd+Shift+Z`, `Ctrl+Shift+Z` or `Ctrl+Y`
   off macOS) move through the accepted versions. Returning to the saved version
   removes the marker; moving away restores it. A focused text field keeps its own
   Undo.
4. **Save** (`Cmd/Ctrl+S`) writes the accepted model to the file it was opened from.
   **Save As** (`Cmd/Ctrl+Shift+S`, toolbar *Save As…*) writes it to a new path,
   refuses an occupied one, and from then on names that path.
5. **Open**, **New** and **closing the window** with unsaved changes ask *Save /
   Discard / Cancel*. Cancel, a closed dialog, and a Save that fails or is cancelled
   all keep the document as it was.
6. **Export STL/FBX** always writes the accepted working model, saved or not, never
   a stale read of the file on disk, and still refuses to write over the document the
   user has open.
7. While the document has unsaved changes the other editors (Sketch, constraints,
   Cut, Fillet, Chamfer, circles, Revolve and the *copy* workflow of the height form) are
   shown disabled with the reason in words. They read a file and write a new file,
   so using them on an unsaved document would silently discard the change. When the
   document is clean they work exactly as before.

The title shows the logical name and the marker and nothing else. The private files
of the session are never named in the title, a dialog, a status line or a log.

## What the library owns

`ferritecad_jobs::DocumentSession` (`crates/ferritecad-jobs/src/session.rs`, save in
`save.rs`) is the only owner of the logical path, the saved checkpoint, the accepted
history and the private directory. The window is a client; it keeps a picture and
the typed draft, never a second copy of those facts.

* **History.** Accepted versions are immutable files in a private `0700` directory
  under the system temporary directory. Bounded at 64 versions and 512 MiB, oldest
  first; the current version is never dropped; a reader holding a version keeps it.
  Removed when the last holder lets go.
* **A step** is two-phase: `begin_step` hands a worker a ticket, `commit_step`
  makes the produced version current only if the session has not moved
  (`STALE_STEP` otherwise). The window commits at the moment the picture has been
  prepared, in one statement with the scene replacement.
* **Dirty** is the comparison `model_version(current) ≠ model_version(saved)`.
  `Document::model_version` is the logical content with only `meta.modified_at` set
  aside; `content_version` is unchanged and is what Save's disk guard uses.
* **Apply** is the copy operation `edit-extrude` runs (prepare, cold rebuild, strict
  reference check, version re-check, atomic publication) with a private source and
  destination. There is no second evaluator, copier or writer.
* **Save** replaces the logical path only if it still holds the version the session
  last read or wrote. A typed `SaveFailure` says why not:
  `Conflict(Modified | Replaced | Missing)`, `Busy`, `Occupied` (Save As),
  `Unwritable` (read-only, or other hard links), `Cancelled`, `Failed`. The checkpoint
  moves only on `Saved`. Symlinks are saved through. Publication is a rename
  (`Existing::Replace`) or a no-clobber hard link (Save As, `Existing::Keep`).
  Cancellation is honoured until the rename; after it the save is a success and says
  so.
* **Exit codes.** The CLI's contract is unchanged: `edit-extrude` and every other
  command keep their arguments, output, refusals and exit status (7 still means the
  report was lost, never "retry"). A saved window document and the peer CLI result
  are the same model: every SQL cell equal except `meta.modified_at`.

## Honest limits

* An external writer that ignores the sidecar advisory lock can change the file in
  the microseconds between Save's last compare and the rename. Cooperating
  FerriteCAD savers are serialised and the second gets `Busy`/`Conflict`. The window
  is tested (`a_writer_that_ignores_the_lock_can_win_the_instant_before_the_rename`)
  and is not claimed closed.
* A crashed process leaves its private directory in the system temporary directory;
  nothing recovers it. No autosave, no persistent revisions, no tabs.
* On macOS `Cmd+Q` terminates through AppKit without a winit close request, so only
  closing the window and the in-app commands are guarded; the AppKit menu hook is a
  later slice.
* Only the Extrude height is on the session. The other editors join in later slices.
* This does not complete Milestone 5C or the general beta.

## Checks

Headless state-machine and Save/race tests run on every OS in `ci.yml` (step *Open,
edit, Undo, Redo and Save one document without native geometry*). The window's
workers against the peer CLI, with Open CASCADE and PlaneGCS, run in
`runtime-layout.yml` (*Edit, Undo, Redo, export and Save one open document through
the native window state*), and the Open CASCADE-without-solver refusal runs in the
no-solver step of the same workflow.
