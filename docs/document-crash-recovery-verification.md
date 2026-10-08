# §30M verification — the last published crash copy of the accepted model

Marker `FCAD_30M_DOCUMENT_CRASH_RECOVERY`. Contract: [document-crash-recovery.md](document-crash-recovery.md).
Decision: [ADR 0005, §30M](decisions/0005-document-session.md#30m-the-last-published-crash-copy-of-the-accepted-model).

## Base

`origin/main` = merge of PR #92, `f31539a156ccae3c094a226bf8586d985f35952c` (parents
`fae2337`, `751c5f7`). Its tree `3dcefc9c60e2df5f4b1b07761df016e130b98a04` is the tree
of the reviewed PR head `751c5f7`, so the merge adds nothing unreviewed. Post-merge CI
on `f31539a`, all completed success: CI 37734492719, planegcs pin 37734492707, product
sbom 37734492819, rust sbom 37734492733, rust notices 37734492655, combined runtime
layout 37734492838 (macOS 113170845697, linux 113170845863, Windows 113170845880,
platform comparison 113200793237).

## What was found in the code before the change

* A session's versions live in a private directory under the system temporary
  directory, removed when the session is dropped; a killed process left it behind and
  nothing ever read it again (ADR 0005, "Not in this slice").
* Every accepted change already passes one place: `Sessions::bind` →
  `adopt` (Open, New, Recover) or `commit_staged` (Apply/Add, Undo, Redo), and
  `Sessions::finish_save` for a published save. That is where the recorder is told;
  no editor has its own hook.
* `Document::snapshot_to` (SQLite online backup) is the copy the session already uses;
  `Temporary`/`Existing::Keep` is the no-clobber publication; `File::try_lock` is the
  advisory lock Save's lock already relies on. All three are reused.

## What changed

* `ferritecad_jobs::recovery` (new): `RecoveryStore` (open/at, list, claim, delete,
  create_record with the 32-record limit and empty-orphan sweep), `RecoveryRecord`
  (publish copy → fsync → verify → rename → manifest → fsync → remove previous;
  ordered request numbers; clear; retire), `RecoveryClaim` (verified, held orphan:
  restore_to, extract_to, into_record, discard), `RecoveryRecorder` (one worker per
  window, coalescing, `observe`/`end`/`finish`, status Off/Writing/Written/Failed and
  `settled`), manifest v1, lease, `default_recovery_root`, `format_utc`.
* `DocumentSession`: an identity (`id`), `recover_in` (new untitled session from a
  claim, named `<name> (recovered)`, holding the claim until the recorder takes it),
  `take_recovery_claim`, `recovery_name`, `is_recovered`.
* Window: `Sessions::keep_recovery`/`record` at adopt, commit_staged and a published
  save; `decide_exit` and the Keep/Retire choice in `stop_all`; `recover_for_view`
  (claim → restore → picture); `recoveries` module (start-up list, Later, the Recover
  in flight); `AppEvent::{RecoveryListed, Recovered, RecoveryChanged}`;
  `Continuation::Recover` guarded like Open; one `can_recover` predicate for buttons
  and handler; Delete asks first; `ferritecad_ui::recovery_panel`.
* CLI: `list-recovery`, `extract-recovery` (JSON v1 `list-recovery`/`extract-recovery`,
  `error.recovery_refusal`).
* Tools and CI: exact-name/no-skip gates in the existing stub, native, no-solver and
  ufbx steps; `tools/document-crash-recovery-gui.py`.

## Crash phases and what each leaves

| Where the process dies | Record afterwards | Recovered |
|---|---|---|
| before the first manifest of a session | lease (+ partial or `c1.fcad`), no manifest | nothing (empty orphan, swept by the next new record) |
| after `.c<n>.fcad.partial` was synced | previous manifest + previous copy + partial | previous copy |
| after `c<n>.fcad` was renamed | previous manifest + both copies | previous copy |
| after `.manifest.partial` was written | previous manifest + both copies + partial manifest | previous copy |
| after the manifest rename | new manifest + new copy (+ previous until removed) | new copy |
| after Save published, before the worker emptied the record | the last dirty copy | that copy (the file on disk is newer; the person decides) |
| after Discard's replacement was accepted, before the worker retired | the discarded copy | that copy — the safe direction |
| during Recover before acceptance | the claimed record, untouched | the same copy again |
| after acceptance, before the recorder adopted the claim | the claimed record, untouched | the same copy again |

Partial names are never read; a claim that adopts a record removes them. Each row
above except the window-only ones is executed by a test (below). Power loss is not
tested and not claimed.

## Gates

(Filled in from the executed runs below.)
