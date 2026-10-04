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
worker answer; a scene that cannot be prepared. Remaining copy workflows (Cut, Fillet, Chamfer and creation of Revolve)
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
