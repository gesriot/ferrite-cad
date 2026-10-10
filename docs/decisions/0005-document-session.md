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

Autosave and crash recovery (until §30M a crashed process left its private
directory in the system temporary directory and nothing reopened it; §30M below
recovers the last published copy of the accepted model), persistent revisions, multiple
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

## §30M: the last published crash copy of the accepted model

**Decided before implementation.** Until §30M a crashed viewer left its private
directory in the system temporary directory and nothing ever read it again. The
decision:

* **Where.** A per-user recovery folder outside the system temporary directory and
  outside any checkout: `~/Library/Application Support/FerriteCAD/Recovery` on macOS,
  `$XDG_STATE_HOME/ferritecad/recovery` (default `~/.local/state/ferritecad/recovery`)
  on Linux and other Unix, `%LOCALAPPDATA%\FerriteCAD\Recovery` on Windows.
  `FERRITECAD_RECOVERY_DIR` names another folder explicitly (tests, the macOS recipe);
  the CLI also takes `--recovery-dir`. Created `0700`. Only entries named
  `r-<uuid>` holding a FerriteCAD lease header are ever read or removed; nothing else
  in that folder, and no temporary, session or cache directory anywhere, is scanned or
  cleaned.
* **What is recorded.** Exactly the session's *current accepted* version, and only
  while `is_dirty()`: after Open, New, Apply/Add, Undo/Redo and recovery have made a
  version current (the moment its picture is shown), never a produced, stale,
  cancelled or unshowable candidate and never form text. A clean document (a clean
  Open, Undo back to the saved version, after a published Save) has no copy.
* **One owner, one mechanism.** `ferritecad_jobs::recovery` owns the folder: a store
  (list, claim, create, delete), a record (one session's copy, held under a lease), a
  claim (an orphaned record, validated and held under its lease) and a recorder (one
  worker thread per process that copies on behalf of the window). The window calls the
  recorder at the one place the session changes hands (`Sessions` after an accepted
  picture, a published save and a replacement); no editor has its own hook. The copy is
  the SQLite online backup the session already uses (`Document::snapshot_to`); there is
  no second serializer, evaluator or copier. The CLI lists and extracts through the
  same claim.
* **Write and its guarantee.** Copy to a partial name, `fsync`, verify (document id,
  complete content version, model version; length and BLAKE3 of the bytes), rename to
  `c<n>.fcad`, `fsync` the directory, write the manifest the same way (partial,
  `fsync`, rename, directory `fsync`), then remove the previous copy. A process crash
  at any point leaves the previous complete copy or the new one, never a partial one;
  partial names are never read. What is recovered is the **last fully published copy**:
  edits accepted after it was published can be lost. Durability across power loss
  depends on the OS and disk honouring `fsync` and is not claimed (a killed process is
  not a power cut; macOS `fsync` does not flush the drive cache).
* **Order.** The recorder numbers every request; its single worker drops superseded
  requests, a record refuses an older number than it has written, and once a
  session's record has been retired no later request for that session can write it
  again (a late answer cannot resurrect a discarded model).
* **Ownership.** The record's owner holds an exclusive advisory lock on its `lease`
  file (`File::try_lock`, the primitive Save's lock uses) for the whole life of the
  record; the operating system releases it when the process ends in any way. A lease
  that can be locked and carries the header is an orphan; one that cannot is a live
  viewer's and is neither offered nor touched. No PID, host or age is consulted. A
  claim keeps holding the lease, so two processes cannot restore or extract one
  record at once. A filesystem without advisory locks refuses recovery rather than
  guess.
* **Recovery.** A claim is restored into a new **untitled** `DocumentSession`
  (`recover_in`): no logical path, no checkpoint, every UUID, SQL row and reference
  as recorded, named `<name> (recovered)` — a name, never a path. Its first Save is the
  existing no-clobber Save As; the original file is never written by recovery. The
  picture is prepared and the session accepted through the same `Bind::Open` as Open;
  a dirty open document is guarded by Save/Discard/Cancel first. A refused claim, a
  failed restore or picture keeps the record and the current document. On acceptance
  the claimed record *becomes* the new session's record (same bytes, same lease), so
  nothing is deleted at the press of a button and nothing is duplicated.
* **End of life.** A published Save empties the record; Discard retires it when the
  replacement is accepted (Cancel, a failed Save or a failed replacement keeps it);
  Quit after the guard retires it. An exit nobody chose (the window could not draw)
  keeps it. A crash before a retirement is processed leaves the older copy listed —
  the safe direction.
* **Limits.** One copy per record. At most 32 records in the folder: at the limit a new
  record is not created and the window says so; nothing recoverable is ever deleted to
  make room. Records with no manifest (nothing to recover) are removed when a new
  record is made. Deleting a recoverable record is the user's explicit choice.
* **Not decided here:** Undo history across a crash, draft form values, tabs,
  persistent revisions, autosave to the user's file, power-loss durability, a CLI
  session protocol.

Contract and checks: [../document-crash-recovery.md](../document-crash-recovery.md).

## §30N: named checkpoints inside the document

**Decided before implementation.** Until §30N a useful version of the model lived
only as long as the session's Undo history or as a separate file. The decision:

* **Where.** In the `.fcad` itself: a new table `checkpoints`, SQL schema v4, added
  by an ordinary migration. One row is one checkpoint: a UUIDv7 (`CheckpointId`, the
  identity — never the name), the name, the SQLite UTC time it was made, the image's
  length, BLAKE3 and model version, and `image`: a complete FerriteCAD document file
  holding the model of that moment. Not the recovery folder (per-user, disposable)
  and not `.fcad-cache` (regenerable by definition): a closed document stays one
  self-contained file. `FORMAT_VERSION`, `MINIMUM_READER_VERSION` and every
  capability are unchanged: a checkpoint stores a whole model; it adds no geometric
  meaning, and the model inside an image carries its own capability declarations.
* **Compatibility.** A schema v3 document stays readable without migration: the
  read-only path accepts v3 and v4, and an absent table reads as an empty list.
  Reading, listing and extracting never write the source. Anything that writes (an
  Apply, a checkpoint change, every edit copy) writes a private copy or a new
  destination, migrated to v4 there; the user's file becomes v4 only when the person
  saves. A build that knows schema v3 refuses a v4 file as "written by a newer
  FerriteCAD" (the existing SQL policy; an old build cannot be changed). Because a
  migrated version has a different SQL schema, the session counts it as a change:
  even a no-op Apply on a v3 file is dirty, which is the truth — Save would rewrite
  the file in the new schema. `clear-cache` stops migrating the document it is
  given (it only needs its name), so no read-only command upgrades a file.
* **What an image is.** The current accepted version copied by the SQLite online
  backup the session already uses (`snapshot_to`), its own checkpoint rows deleted,
  then compacted with `VACUUM INTO` (which keeps row identities). Every table,
  UUID, reference, dependency, parameter, imported source byte, unknown payload and
  unknown table is the version's own; nothing is decoded or re-serialised. No draft
  form value, GPU state or cache sidecar. Images never contain checkpoints, so size
  is linear in the number of checkpoints, never recursive. Each image is verified
  before it is stored: same document id, and its model with the catalog set aside
  (`Document::model_without_checkpoints`) equals the version's.
* **Ownership.** The catalog belongs to the working document, not to any model. A
  stored row is immutable: nothing updates it; Create inserts a new UUID, Delete
  removes one row. Restore replaces the model with the image's and keeps the working
  version's catalog row for row (row identities included), so restoring never brings
  back an old list, and a later edit never rewrites a checkpoint.
* **Dirty, Undo, Save.** The catalog is document content, so the existing rule needs
  no exception: `model_version` covers it, Create/Delete are ordinary session steps
  (two-phase, version-guarded), the document is dirty until Save/Save As publishes
  them through the existing guarded publication, Undo takes a created checkpoint
  away or brings a deleted one back, Redo returns the same accepted file (same UUID,
  no job re-run). A step whose model without the catalog is unchanged
  (`ProducedStep::keeps_picture`) is accepted without a rebuild: the window keeps
  its picture and re-reads only the kernel-free edit facts of the new version on a
  worker. No geometric cache keys on the catalog (the sidecar keys object inputs).
  Restore is a model step: its picture is prepared and accepted as one step, like
  any Apply; a Restore to the model already shown is "no change".
* **Limits.** At most 32 checkpoints and 16 MiB of images per document. A Create
  past either limit is refused with the reason; nothing is ever deleted to make room.
  With the catalog inside every private version, the session's existing bound
  (64 versions, 512 MiB) still keeps at least 31 versions; a crash copy grows by at
  most 16 MiB. Listing never reads an image; extraction streams one.
* **Names.** Surrounding whitespace is trimmed; 1–80 characters; no control
  characters. Equal names are allowed: the list shows the time beside each, and
  every operation takes the UUID.
* **Window and CLI.** A small panel lists the current version's checkpoints with
  Create / Restore / Delete… (Delete asks first). The three reserve the same
  operation slot as Apply/Undo/Save/Open/Recover and wait while a form is open, as
  document Undo does. The CLI lists, extracts a checkpoint's model to a new file,
  and creates/deletes a checkpoint in a copy (`--expect-version` required,
  no-clobber, JSON v1 additive, exit 7 unchanged). Restore in the CLI is extraction.
* **Recovery (§30M).** Unchanged: the crash copy is the accepted version file, so
  the catalog travels with it.
* **Not decided here:** tabs, persisted Undo history, branching or merging
  checkpoints, automatic checkpoints, cloud sync.

Contract and checks: [../named-document-checkpoints.md](../named-document-checkpoints.md).

## §30O: several documents in tabs of one window

**Decided before implementation.** Until §30O the window held at most one session, so
Open, New and Recover had to replace the document on screen. The decision:

* **A tab is one window controller of one session.** A tab is the existing
  `sessions::Sessions` value (operation slot, status, recovery lane) holding exactly one
  `DocumentSession`; it gets a runtime `TabId` when it is made, never reused and never
  derived from the tab's position or from the `DocumentId` (two copies of one `.fcad`
  share a `DocumentId` and are two tabs). Path or Untitled, dirty, accepted history,
  checkpoints and Save are the session's, unchanged; a tab's name, `*` and which
  actions it offers are read from its session. The tab list (`tabs::Tabs`) owns only
  which tabs exist, their order, the hidden tabs' controllers and their view state
  (camera, typed checkpoint name): no Save, Apply or Undo rule is copied into it.
* **One active tab, one picture.** Only the active tab has a GPU scene; a hidden tab
  keeps its session files and a camera, no scene and no GPU buffer, and nothing
  rebuilds hidden tabs in the background. Switching reads the target's current version
  on a worker exactly like an Undo's picture, then in one statement makes the target
  active, parks the previous tab (with its camera) and replaces the picture; a refused
  kernel, upload or a Cancel leaves the previous tab active and shown. The camera is
  kept per tab; selection and visibility are reset on every switch (no identity of
  picks is carried between pictures).
* **One foreground operation per window.** A switch reserves the active session's
  operation slot the way Recover does. Switching, closing and Quit wait while any
  operation, load, export or create is running or a form is open (a form never moves
  to another document; the minimal honest rule, not per-tab drafts). *§30P replaced
  the form part of this rule: an idle form is kept with its own tab, moved as a value in
  the same statement as the switch ([../tab-edit-drafts.md](../tab-edit-drafts.md)).* Every session
  answer carries its tab and generation (`sessions::Address`); an answer for another
  tab, a closed tab or an older generation changes nothing and releases nothing.
* **Open, New, Recover add a tab.** The document on screen stays, so nothing is asked
  about it. A candidate becomes a tab only through the same `Bind` that shows its
  picture; a failed, cancelled or stale candidate adds nothing. Opening a file (or a
  link, a hard link or another spelling of it) that a tab already names activates that
  tab instead of making a second writer; different physical copies are separate tabs.
  Save As refuses a destination another tab names, in addition to the existing
  no-clobber and save guards. At most **8** tabs: a ninth Open, New or Recover is
  refused in words before any work starts.
* **Close and Quit.** Closing a clean tab closes it; a dirty or Untitled one is first
  made active and asked Save / Discard / Cancel through the existing guard; Cancel, a
  failed or cancelled Save, or a tab that cannot be shown keeps it. Closing the last
  tab leaves an empty window. Quit (window close, `Cmd+Q`) asks about every dirty tab in
  turn, active first, each made active before it is asked. Cancel at any tab stops Quit:
  no tab is closed, files saved earlier in the pass stay saved, and tabs answered
  Discard stay open, dirty and recoverable (Discard takes effect only when the window
  actually ends).
* **Recovery belongs to the session.** The process keeps one recorder worker; each tab
  writes through its own lane (`RecoveryRecorder::lane`), so one tab's accepted step,
  save or close never ends another tab's record. Close after Save/Discard retires only
  that tab's record; a crash keeps every dirty tab's last published copy; Recover opens
  the copy as a new tab. Leases and the 32-record limit are unchanged.
* **Limits are per window, not per session.** 8 tabs × 64 versions / 512 MiB of private
  history each (at most 4 GiB of private files), one recovery record per dirty tab
  within the store's 32, one scene in memory and on the GPU at a time plus the
  transient picture being prepared for a switch. Nothing here proves the earlier OOM
  gone.
* **Not decided here:** restoring the set or order of tabs after a restart (*§30R
  below reopens the saved files of the last window that quit*), per-tab
  drafts, background work in hidden tabs, several windows, docking, a CLI session
  protocol. The command line is unchanged.

Contract and checks: [../document-tabs.md](../document-tabs.md).

## §30R: reopen the saved files of the last window

**Decided before implementation.** Until §30R a person who quit with A, B and C open had
to find each file again. The decision:

* **What is kept.** The logical paths of the open tabs' *saved files*, in the row's
  order, and which one was shown — nothing else: no model, Undo, checkpoint state, form,
  camera or selection, and no copy of any document. What a later start reopens is each
  file as it is on disk then; unsaved accepted models after a crash remain §30M's. An
  Untitled tab has no path and is left out; a named tab answered Discard is listed, and
  its saved file is what reopens.
* **When.** Only when the window's Quit pass really ends: `Tabs::quit_step` is `Exit`
  (every unsaved tab saved or answered Discard, no form shown, hidden or set aside for
  New, no worker). `end_window_quit` asks that itself, so no other route — a Cancel, a
  failed Save, a late continuation, an exit nobody decided — can publish. An empty end
  publishes an empty list, which clears the offer.
* **Where and how.** One descriptor, `last-window`, in a per-user folder beside the
  recovery folder (`~/Library/Application Support/FerriteCAD/Tabs`,
  `%LOCALAPPDATA%\FerriteCAD\Tabs`, `$XDG_STATE_HOME/ferritecad/tabs`; the platform
  rule is the recovery folder's own, shared through `per_user_state_folder`;
  `FERRITECAD_TABS_DIR` names another). A publication writes a temporary file of its own
  (exclusive create, unique name), syncs it and renames it over the descriptor: a reader
  sees the previous whole list or the new whole list. Two windows (processes) that quit
  one after the other each publish their whole list; the last rename stays; lists are
  never merged; no process removes another's temporary file. Power-loss durability is
  not claimed. A list that cannot be kept stops the Quit once, in words; the next Quit
  ends the window anyway.
* **Format.** Text, version 1: magic and version, platform, count (at most `MAX_TABS`),
  shown index or `none`, then one line per path holding its native units in lowercase
  hexadecimal (Unix bytes, Windows UTF-16 units) — lossless for Unicode, spaces,
  newlines and names that are not UTF-8 or valid UTF-16. Size is bounded before reading;
  every deviation is refused (`unknown-version`, `damaged`, `too-large`, `foreign` for a
  link or a non-file, `io`) and the file is left as it is. Reading creates and writes
  nothing.
* **Reopen.** Offered at start, asked by one predicate (`can_restore`) for its button and
  handler, started only by the person. A queue (`restores::Restores`) reads one file at a
  time through the existing Open route (`Loads`, `open_for_view`, `Bind::Open`) and waits
  for that reading's generation; there is no second session type, Open, parser or GPU
  scene, and no new parallel foreground work. A file a tab already names is not read
  again; copies with one `DocumentId` are separate tabs; the 8-tab limit ends the queue in
  words; every file not opened is said by name and stays said until **Not now**. Cancel of
  the reading stops the queue and keeps what opened; a late answer cannot revive it. At
  the end the old shown file's tab is shown, or the first listed file that opened. While
  the queue runs, Open waits for it (`can_open`); New, Recover, tab changes and Quit wait
  for the reading as they always did. The crash-copy offer stays separate; neither
  decides for the other.
* **Not decided here:** form drafts, Undo or camera across a restart; several windows'
  sets at once; a workspace or session database; a CLI for window tabs (the command line
  is unchanged; parity is about models); power-loss durability.

Contract and checks: [../restore-saved-tabs.md](../restore-saved-tabs.md).

## §30S: reading the recovery folder is not owning a record

**Proven before it was decided.** `RecoveryStore::list` and `claim` shared one
`inspect`, which took the record's *exclusive* lease and verified the copy under it.
While one process listed a record, a Recover, an `extract-recovery`, a Delete or another
listing of it was refused as `active` — "a FerriteCAD window that is still running" —
though the only other party was a reader. It reproduced deterministically on the base
with a listing held inside its verification, in one process and across two, and with
the real `ferritecad extract-recovery` (see the contract). PR #97 had fixed the same
shape for the cleanup sweep only.

* **Decision.** The lease's lock has two modes, and the folder's operations choose by
  what they do. *Looking* (`list`) takes it **shared**, per record, only while that
  record is verified. *Owning* (the record's owner, a claim, a removal) still takes it
  **exclusive**. A shared lock cannot be granted while an exclusive holder exists, so a
  reader knows nobody owns, claims or removes the record while it reads, and the bytes it
  verifies cannot be mid-publication, mid-adoption or gone. Readers do not exclude each
  other. No new file, format, PID, age, daemon or registry; the manifest, header and
  layout are §30M's.
* **A claim or removal waits for readers, not for holders.** When its exclusive try is
  refused it asks for a shared lock: refused means an exclusive holder (`active`, said as
  such — a window, a claim or a removal; the primitive cannot say which); granted means
  only readers (let go at once), which end by themselves, so the caller polls for at most
  5 s and ends the wait the moment its `CancelToken` is cancelled (`busy`, `cancelled`).
  The wait is off the event loop: the Recover worker, the delete thread, the command
  line's own thread. The window gives its Recover worker a token that Cancel and Quit
  cancel. A listing never turns into a claim: the claim verifies the record again under
  its own lock.
* **Rejected.** A lock-free read (a changing record would be listed as damaged, or a
  live one read mid-publication, and a pre-check would be a race of its own); a short
  exclusive probe (still refuses a concurrent claim for its length); retrying `active`
  (a real owner would be waited for and the answer would still be a guess); a larger
  timeout; calling every busy record an orphan; PIDs or ages; a lock service.
* **Honest limits.** Advisory, cooperative locks; old builds still list exclusively; a
  stream of listings can starve a claim until its bound; a reader that hangs holds off
  its record's claim until it dies or the bound passes. Windows and Linux legs are CI's.

Contract and checks: [../recovery-inspection-contention.md](../recovery-inspection-contention.md).


## §30U: close one tab with an unfinished form

Decided before implementation in the [event/owner table](../close-tab-with-draft.md).
The existing Forms/Draft owners move whole into one Close attempt in Tabs; no
per-editor cancellation protocol or form copy. The attempt names runtime TabId,
a once-only generation and the accepted snapshot identity. Back returns the exact
form. Explicit form abandonment authorises continuing Close, while the Draft stays
held through the existing dirty/Untitled question and Save continuation. Every
cancel/refusal returns it; only actual successful Close drops it with its tab.
Save reads only the accepted model. Quit and other transitions remain unchanged.

## §30V: Quit with unfinished saved-object forms

Decided before implementation in [quit-with-tab-drafts.md](../quit-with-tab-drafts.md).
One Quit pass in Tabs holds the existing whole Drafts, including already confirmed
ones. Form answers, model questions, Save continuations and switch arrivals name
that pass, runtime TabId and accepted snapshot identity. Hidden tabs use the existing
switch route before being asked. Back or any refusal returns every original form
without a history step. Published Saves stay saved; model Discard is deferred.
LastTabs publication still precedes final form destruction and recovery retirement,
including the existing first-refusal/second-Quit rule. New and foreground holds
remain; no form persistence, automatic Apply or document registry is introduced.
