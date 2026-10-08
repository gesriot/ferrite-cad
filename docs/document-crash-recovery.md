# §30M — the last published crash copy of the accepted model

Marker `FCAD_30M_DOCUMENT_CRASH_RECOVERY`. Decision:
[ADR 0005, §30M](decisions/0005-document-session.md#30m-the-last-published-crash-copy-of-the-accepted-model).
Verification and the real-window recipe:
[document-crash-recovery-verification.md](document-crash-recovery-verification.md).

## What a person can do

While a document has unsaved changes — a named document after an Apply, Add, Undo or
Redo, or an **Untitled** one, Empty included — FerriteCAD keeps one copy of the model
on screen in its recovery folder, written in the background. If the window ends
without the person deciding (a crash, a forced quit, a power-off before the next
start), the next start lists that copy under **Recover unsaved work** with its name and
the time the copy was confirmed written. **Recover** opens it as a new unsaved document
named `<name> (recovered)`; it is checked like any opened document and saved with
**Save As**. **Later** hides the list for this run; **Delete…** removes one copy after
asking. The original file is never written by any of this. The window's status line
says whether the copy is being written, when it was last written, or that it could not
be written (Save is unaffected).

The command line lists the same copies and extracts one to a new `.fcad`:

```text
ferritecad list-recovery [--recovery-dir DIR] [--json]
ferritecad extract-recovery RECORD --output PATH [--recovery-dir DIR] [--json]
```

## Rules

1. **Folder.** Default: `~/Library/Application Support/FerriteCAD/Recovery` (macOS),
   `$XDG_STATE_HOME/ferritecad/recovery` or `~/.local/state/ferritecad/recovery`
   (Linux, other Unix), `%LOCALAPPDATA%\FerriteCAD\Recovery` (Windows).
   `FERRITECAD_RECOVERY_DIR` (viewer and CLI) and `--recovery-dir` (CLI, wins) name
   another one. Created with mode `0700` where the platform has modes. Only
   `r-<uuid>` directories carrying a FerriteCAD lease header are read or removed.
2. **What is copied.** The current accepted version of the session, only while
   `is_dirty()`, only after it was made current together with its picture. Produced
   but unaccepted, stale, cancelled or unshowable candidates and form text are never
   copied. A clean session (a clean Open, Undo back to the saved version, after a
   published Save) has no copy; its record is emptied.
3. **Background.** One worker thread per process does all copying, fsyncs and
   verification; the event loop only sends the accepted snapshot (an `Arc`, so the
   private file outlives the history) and a request number. Requests superseded
   before the worker reaches them are dropped. The status shows *writing*, *written
   at* (UTC, from the manifest the worker published) or *could not be written* — a
   pending or failed copy is never shown as written, and a failed copy does not
   change the model or look like a failed Save.
4. **Record layout and publication.** `r-<uuid>/lease`, `r-<uuid>/manifest`,
   `r-<uuid>/c<n>.fcad`, and transient `.c<n>.fcad.partial` / `.manifest.partial`.
   Publication: SQLite online backup of the snapshot to the partial name, `fsync`,
   re-open and verify the document id, content version and model version, measure
   length and BLAKE3, rename to `c<n>.fcad`, `fsync` the directory, write the
   manifest to its partial name, `fsync`, rename, `fsync` the directory, remove the
   previous `c<m>.fcad`. A request numbered no higher than the record's last one is
   refused (*stale*). Hashing streams the file instead of allocating its whole size.
   If the final directory sync fails after the manifest rename, the new named copy
   stays intact and the status is failed, not confirmed written. Its sequence is
   consumed; retry writes a new sequence. The older copy can remain until cleanup.
5. **Manifest v1.** Text, every line `key value`, exactly in this order:
   `FERRITECAD-RECOVERY 1`, `record <uuid>`, `sequence <n>`, `file c<n>.fcad`,
   `bytes <n>`, `blake3 <64 hex>`, `document <uuid>`, `content <64 hex>`,
   `model <64 hex>`, `written-unix-ms <n>`, `name <text>`. Any other first line is an
   unknown version; any other deviation is damage. `file` is a name inside the record
   and is never followed anywhere else.
6. **Lease.** The owner holds an exclusive advisory lock on `lease` (header
   `FERRITECAD-RECOVERY-LEASE 1`) for the life of the record. A lockable lease with
   the header is an orphan and may be claimed; a held lease is a live viewer's and is
   not listed as recoverable, claimed or removed. A claim holds the lease until it is
   dropped, finished or handed to a recovered session. No PID, host or age is used.
   A filesystem that cannot lock refuses.
7. **Validation of a claim.** Directory and files are plain (no symbolic links);
   manifest parses; its record id is the directory's; the copy's length and BLAKE3
   match; it opens read-only as a current-schema document whose id, content version
   and model version match. Each failure is a typed refusal of that record only:
   `active`, `unknown-version`, `damaged`, `mismatch`. Other records are unaffected.
8. **Recover (window).** Asked only from the start-up list. A dirty open document is
   guarded first (Save / Discard / Cancel, as for Open). The worker claims, restores
   into a new untitled session (`DocumentSession::recover_in`, a private copy, the
   record left untouched) and prepares its picture; it is accepted through the same
   `Bind::Open` as Open. Any refusal or failure keeps the record and the current
   document. On acceptance the claimed record becomes the new session's record: its
   copy already is that model, so nothing is written or deleted at that moment.
   Recovery reserves the same busy slot as document edits, Save and Open/New.
   Cancel invalidates its generation; a late answer cannot replace the document or
   release the reservation of a newer operation.
9. **Recovered document.** No logical path, no saved checkpoint, every UUID, SQL row
   and reference as recorded; dirty until a Save As is published; Save asks where
   (no-clobber; occupied paths refused). It has no right to save in place to the file
   it came from.
10. **End of life.** A published Save empties the record. Discard retires it only when
    the replacement document is accepted; Cancel, a failed Save and a failed or
    refused replacement keep it. Quit after the guard retires it; an exit without that
    decision keeps it. After a retirement no later request can rewrite that record.
11. **Limits.** One manifest-selected copy per record (interrupted publication can
    leave unselected files until cleanup). At most 32 records; at the limit a new session
    gets no record and the status says so. Records with no manifest are removed when a
    new record is made; nothing recoverable is removed except by **Delete…**, by
    extraction of the session that adopted it, or by its own session's end of life.
    Lease acquisition rejects a record-directory symlink before accessing children;
    this also protects explicit deletion and the empty-orphan sweep.
12. **CLI.** `list-recovery` reads the folder (no claim is kept, nothing is changed,
    no kernel). `extract-recovery` claims the record, copies its model with the same
    identities to `--output` by the shared no-clobber publication, and releases the
    claim: the record stays. It refuses an active record, an occupied output and an
    output inside the recovery folder.

## Guarantee

*Process crash:* at any point of a publication the record holds either the previous
complete copy (manifest still names it) or the new one; partial names are ignored.
What is recovered is the last copy whose manifest was published, so changes accepted
after it are lost. *Power loss:* the order above uses `fsync` on file and directory;
whether that survives power loss depends on the platform and drive (macOS `fsync`
does not flush the drive cache) and is not proven or claimed.

## CLI wire contract (JSON v1)

Same envelope as every `--json` command: `schema_version`, `operation`, `ok`,
`result` or `error {kind, message, causes}`; usage errors stay clap text (exit 2);
execution refusals are structured (exit 2); losing stdout after an extraction
published exits 7 and leaves the file.

`list-recovery` result:

```json
{"folder":"…","records":[
  {"record":"<uuid>","state":"recoverable","name":"plate.fcad",
   "written_unix_ms":0,"document_id":"<uuid>","content_version":"<hex>","bytes":0},
  {"record":"<uuid>","state":"refused","reason":"damaged","message":"…"}],
 "active":0}
```

`extract-recovery` result:

```json
{"record":"<uuid>","destination":"…","document_id":"<uuid>",
 "content_version":"<hex>","name":"plate.fcad"}
```

## Unchanged

Save, Save As, Open, New, every editor and Add, Undo/Redo, exports and every existing
CLI command and contract. No Undo history, draft form values, tabs or persistent
revisions are recovered; nothing writes the user's file except Save. §30, Milestone 5C
and the product remain open.
