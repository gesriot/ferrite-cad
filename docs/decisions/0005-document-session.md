# 5. One session owns the open document; the file on disk changes only on Save

**Status:** accepted for §30A. It decides the owner and the rules of the open
document; it does not decide autosave, recovery, persistent revisions, tabs or
how the other editors join (those are the next slices and are listed at the end).
The executable contract and its checks are in [../document-session.md](../document-session.md)
and [../document-session-verification.md](../document-session-verification.md).

## The problem

Until §29D the window had no *current document*. It had a picture
(`LiveScene`) and the path that picture was last read from (`LiveScene.document`),
and every edit was "write a new `.fcad` somewhere, then Open it". That is the
right shape for an agent (the CLI) and the wrong one for a person: pressing Apply
asked for a file name, the accepted model was whatever the last Open read, there
was no unsaved state to be honest about, Undo meant a visibility toggle, and
Export read a path on disk that might have been replaced since.

Plan §4.5 says the window and the CLI are two clients of one library layer. So the
answer cannot be a second evaluator, writer or copier inside the window: it has to
be a library object the window is a client of, and one the CLI's old command
reaches the same bytes through.

## What was decided

**One owner: `ferritecad_jobs::DocumentSession`.** It alone owns, for the document
that is open: the *logical path* (where Save writes, and the name the window
shows), the *saved checkpoint* (what is on disk as of the last Open or Save), the
*accepted history* (the versions the user has accepted, with a current one), and
its private working directory. The window holds at most one session and holds no
second copy of any of those facts. `LiveScene` stays what it was — a picture
derived from the current version — and `LiveScene.document` now names the current
version's private snapshot, not the user's file.

**Private on-disk snapshots, not an in-memory database.** Every job we reuse
(`edit_object_copy`, the STL and FBX exports, `snapshot_of`, `validate`) takes a
path and opens SQLite itself, re-opens the path to re-check its version, and
publishes with a no-clobber hard link. An in-memory SQLite would need new entry
points in the document crate and each job would still open by path; a private file
is what the existing SQLite/job contract already is. Each accepted version is one
immutable file in a private (`0700`) directory under the system temporary
directory. The directory is created by the session, removed when the session is
dropped, and its path is never shown (not in the title, a dialog, stdout or a
status line) and never counted as a save.

**One Apply is one reused copy operation.** Apply height runs
`edit_extrude_copy` with source = the current snapshot, expected version = the
current snapshot's version, destination = the next private file: the same
prepare, cold rebuild, strict reference check, version re-check and atomic
publication the CLI's `edit-extrude` uses, and nothing else. The window changes
which path it reads and writes, not what the operation is.

**Accepted means shown.** A step becomes current only at the moment its scene has
been prepared for display, in one statement with the scene replacement. A failed,
cancelled, stale or GPU-failed Apply changes nothing: not the current version, not
the dirty state, not the history, not the last good scene; the typed draft stays.

**Dirty is a comparison, not a flag.** Dirty means "the current version's *model
content* differs from the saved checkpoint's". Model content is the document's
complete logical content with only `meta.modified_at` set aside
(`Document::model_version`, a sibling of `content_version`, whose bytes are
unchanged). So undoing to the checkpoint, redoing away from it, editing a value
and editing it back, and a no-op Apply (which rewrites only the stamp) all do the
right thing without a counter.

**Save is its own operation with its own guard.** Save replaces the logical path
only if it still holds exactly the version the session last read or wrote (same
document id, same complete content version): an externally changed, replaced or
deleted file is a typed conflict and is never overwritten silently. It is a
different operation from the copy jobs, whose refusal to write over their source
is unchanged. Save As is no-clobber: an occupied path is refused and the session
keeps the accepted scene and the draft.

**Open, New and Quit ask first.** With unsaved changes they offer Save / Discard /
Cancel; Cancel, a failed dialog and a failed or cancelled Save all stop the
replacement. The old session is dropped only after the new document has been
accepted.

## Invariants

1. The user's file is written by Save and Save As and by nothing else. Apply, Undo
   and Redo never touch it.
2. There is one current version, and the picture shown is derived from it. A
   session mutation and the scene replacement that shows it happen together or not
   at all.
3. `dirty ⇔ model_content(current) ≠ model_content(saved)`; the saved checkpoint
   moves only on a successful publication of exactly that version.
4. Accepted history is bounded (64 versions and 512 MiB), holds files and numbers
   only (no OCCT or GPU handle), and drops its *oldest* entries first; the current
   version is never dropped. A step that is a no-op on model content is not a step.
5. Every file the session created is removed by the session; it never removes a
   file it did not create.
6. A typed outcome tells the truth about publication: `Published` means the file
   was replaced or linked; every failure means nothing was.

## Refusals

Save conflict (changed, replaced by another document, or missing); Save busy
(another saver holds the path); Save over a read-only file or a file with other
hard links; Save As on an occupied path; Apply while another operation runs, on a
document whose editor says it is unavailable, or whose snapshot changed; a stale
worker answer; a scene that cannot be prepared. Remaining copy workflows (Cut copy, Fillet copy, Chamfer copy, Chamfer distance copy; creating a new document, Revolve included, is guarded by the question instead since §30L)
are unavailable while the session is dirty — they
read a source and write a *new* file, and silently reading the old file would drop
the unsaved change — and work unchanged on a clean session.

## Guarantee and its limit

Replacing a file is atomic (rename); comparing its version and replacing it is not
one filesystem operation. Cooperating FerriteCAD savers are serialised by an
advisory lock beside the file held across the compare and the rename, and the
second one gets a typed `Busy`, or `Conflict` once the first has published. An
external writer that ignores the lock can still change the file between the
compare and the rename; that interval has no promised duration, it is tested, and it is
not claimed to be closed.

## Not in this slice

Autosave and crash recovery (a crashed process leaves its private directory in the
system temporary directory; nothing reopens it), persistent revisions, multiple
documents or tabs, a feature tree and inspector, the other editors joining the
session. AppKit Quit now enters the same guarded event-loop route as window
close. Milestone 5C and the general beta are not complete.

§30B–E now apply supported saved vertices, constraints, Circle/annulus geometry
and partial Revolve angles through this same owner. Creation and copy workflows
retain their own guards.

§30F now applies parameters of an existing supported circular Cut through this
same session. Add Cut remains copy-only; existing-Cut copy still requires a clean
document. See [Cut session contract](../cut-session-apply.md).

§30G applies an existing Fillet radius through this session, including while dirty.
Add Fillet and radius copy remain clean-only. See [Fillet session contract](../fillet-radius-session-apply.md).

§30H applies an existing Chamfer distance through this session, including while dirty.
Add Chamfer and distance copy remain clean-only. See [Chamfer session contract](../chamfer-distance-session-apply.md).

§30I adds a new circular Cut through this session, including while dirty. Cut
copy, Add Fillet and Add Chamfer remain clean-only. See [Add Cut session contract](../add-circular-cut-session.md).

§30J adds a new Fillet through this same session, including while dirty (**Add
fillet**); Save fillet copy, Add Chamfer and the other copies remain clean-only. See
[Add Fillet session contract](../add-fillet-session.md).

§30K adds the plate's one Chamfer through this same session, including while dirty
(**Add chamfer**), on a free or closure-only plate only; Save chamfer copy and the
other copies remain clean-only. See [Add Chamfer session contract](../add-chamfer-session.md).

## §30L: a new document is a session before it is a file

**Decided before implementation.** Until §30L the window's New asked for a file
name first, published a `.fcad` there and then opened it. Now that every editor
and Add works inside the session, that is the last place where a person must name
a file before they have a model. The decision:

* **State.** A session may have no logical path and no saved checkpoint
  (`DocumentSession::create_in`). Both are *absent* (`Option`), never a private
  snapshot, the working directory, an empty path or an invented `untitled.fcad`
  standing in for a saved file. The window calls it **Untitled**; the private
  directory still appears nowhere a person reads. An empty window with no document
  and an accepted new Empty document are different states (the first has no
  session).
* **One fact, one owner.** `is_dirty()` stays the single "would closing lose
  something" answer and is owned by the session: with no checkpoint it is true for
  every version, Empty included, and Undo back to the first version does not make
  it saved. After the first Save it is again exactly the model-content comparison
  with the checkpoint, so no-op, Redo and the version guard are unchanged; no
  "modified" flag is added.
* **Creation route.** The worker creates the first version inside the candidate
  session's own private directory through the existing `create_document_with_kernel`
  (one `needs_kernel` classification: Empty and the sample plate need no kernel),
  then builds the picture from that private file. Nothing is published anywhere
  else and nothing is rebuilt twice for a DTO. The candidate becomes the window's
  session only through the same `Bind::Open` that Open uses, in one statement with
  the scene replacement; a failed, cancelled, stale or scene/GPU-refused candidate
  is dropped (its directory with it) and the previous session, picture and typed
  draft stay.
* **First Save is Save As.** Save on an untitled session asks for a path and runs
  the existing no-clobber Save As publication. Only a published save gives the
  session its logical path and checkpoint; a cancelled dialog, an occupied
  destination, a path inside the private directory (or reaching it through an
  alias) or a failed publication changes nothing. A cancellation that arrives after
  publication does not undo it. Later Saves are the existing in-place Save with
  its version guard. A library `InPlace` save of an untitled session is refused.
* **Guard at the point of replacement.** Open, Quit and *creating* a new document
  ask about an untitled or dirty session; the question is asked when the new
  document is about to be made (from any create form), not only when New is
  pressed, so no toolbar route can drop an unsaved document. Save from the
  question continues exactly once after a published save; Cancel, a cancelled
  dialog or a failed save continues nothing. Discard keeps the old session until
  the replacement is accepted.
* **Not decided here:** autosave, recovery, tabs, persistent revisions, a CLI
  session protocol. The command line's `create`, `create-sketch-*` and analytic
  create commands keep publishing the path they are given, with unchanged output
  and exit codes. Copy workflows keep requiring a saved, clean document and say so.

Contract and checks: [../unnamed-document-session.md](../unnamed-document-session.md).
