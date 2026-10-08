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

Every gate runs with `--exact`; a gate fails on `skipped:` and on zero tests. Gates
that print a marker are checked by their exact `test NAME ... ` line,
`test result: ok. 1 passed; 0 failed` and the marker on its own line (libtest puts
`ok` after printed output, so `test NAME ... ok` is not one line for them).

**Kernel-free, every platform (`ci.yml`, stub step):**

* `ferritecad-jobs --test recovery` (12): a killed child process for a named dirty
  document, an Untitled Empty document and an Undo/Redo history, each recovered by
  another process, compared in the complete content version (every SQL cell and row id)
  and model version, InPlace refused, occupied Save As refused, Save As published and the
  source byte-identical; a live lease neither offered, claimed nor deleted, released by
  the kill, and a held claim refusing a second; a kill in each of three publication
  phases leaving the previous whole copy and the adopted record dropping the partials;
  stale request numbers refused; Save/Undo-to-saved/failed Save/Discard/Cancel endings;
  a Discard racing its own copy twenty times; extraction refusing an occupied output and
  one inside the folder, keeping the record and the externally replaced source;
  unknown version, truncated, bit-flipped, escaping, missing and foreign records each
  refused alone, foreign files untouched; a write failure (name taken by a directory,
  root-independent) keeping the previous copy and reporting `Failed`, and the limit
  refusing a new record without deleting anything; coalesced quick changes ending with
  the newest version; a recovered session adopting its record without a write.
* `a_folder_without_write_permission_fails_the_copy_and_loses_nothing` (Unix, marker
  `FCAD_30M_PERMISSION_GATE_EXECUTED`): a `0500` record directory. Root ignores
  directory permissions, so as root it says `skipped:` and the gate fails by design; it
  must run as a normal user (GitHub runners are).
* `ferritecad-cli --test recovery` (4): JSON and text listing, extraction through the
  claim, occupied/inside-the-folder refusals, `recovery_refusal` `active`/`mismatch`/
  `not-found` as data, a missing folder not created, `FERRITECAD_RECOVERY_DIR`, usage
  stays clap text (also for a malformed record id), exit 7 after a published extraction.
* App (`ferritecad-viewer`, 4): only a version shown with its picture is copied (a
  failed picture, a cancelled candidate and a stale answer are not), Undo to saved and
  a published Save empty the record, Redo copies again; Cancel keeps and Discard (once
  the replacement is accepted) ends the copy, an exit nobody decided keeps it and Quit
  after the guard retires it; a dropped recovered candidate lets its record go
  untouched, an accepted one is `*plate.fcad (recovered)`, untitled, suggested as
  `plate (recovered)`, every SQL cell as recorded, the record adopted not duplicated,
  Save As empties it; stub: Recover refused at the picture with the record and the open
  document unchanged (marker `FCAD_30M_STUB_RECOVERY_EXECUTED`).
* UI (1): Recover and Delete ask nothing while the window says they cannot; Later is
  always offered; nothing is drawn when there is nothing to offer.

**Native (`runtime-layout.yml`, OCCT + PlaneGCS, three OSes):**

* `native_a_killed_window_recovers_named_untitled_and_edited_models_as_accepted`
  (marker `FCAD_30M_NATIVE_CRASH_MATRIX_EXECUTED variants=3`): a child process edits
  through the window's own workers and pictures — named plate: Apply height (OCCT),
  constraints (PlaneGCS), Add Fillet (OCCT), Undo, Redo; Untitled drawn polygon: create
  (OCCT) and vertex Apply; Untitled Empty — waits until the copy is written and is
  killed. Another process lists it, recovers it through `recover_for_view` and
  `Bind::Open`, and compares every SQL cell with the version accepted before the kill;
  STL and FBX of the recovered and the accepted model are byte-identical (artifacts for
  the pinned reader); Save As, cold rebuild equal; source byte-identical; record retired.
* `native_recovery_window_scenario_on_session_files_passes_the_comparator_and_its_controls`
  (marker `FCAD_30M_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true`): the macOS
  recipe through the window's own owners, then the comparator and its seven controls.
  A self-check of the comparator, **not window evidence**.
* Pinned ufbx (`tools/check-fbx-complex.sh`, `FCAD_RECOVERY_SESSION_UFBX_EXECUTED`):
  `recovered-plate` and `recovered-drawn` read with `checks=6 failures=0` and joined
  with their STL.

**No solver (`runtime-layout.yml`):** `mixed_recovery_draws_without_a_solver` (marker
`FCAD_30M_MIXED_RECOVERY_EXECUTED`).

## Directed mutations

M30M-1 retires the record at the press of Recover: `DocumentSession::recover_in`
calls `claim.discard()` instead of holding the claim. It compiles; it fails executed
assertions in `a_crash_in_every_phase_of_a_publication_leaves_the_previous_whole_copy`,
`a_recovered_session_adopts_its_record_without_writing_or_duplicating_it`,
`a_killed_window_leaves_its_last_published_copy_…` (jobs) and
`a_recovered_document_is_untitled_named_recovered_and_takes_over_its_record`,
`stub_recovery_is_refused_at_the_picture_…` (app: the record was gone, `left: 0`).

M30M-2 makes a decided ending keep the record (`Ending::Retire => drop(record)` in the
recorder): a discarded document's copy would outlive the Discard. It compiles; it
fails `a_discarded_document_is_not_brought_back_by_a_copy_still_in_flight`,
`save_discard_cancel_and_failed_saves_end_the_record_as_the_person_decided`,
`a_recovered_session_adopts_…` (jobs) and `discard_and_quit_end_the_copy_…`
("Discard ended the copy"), `only_a_version_shown_…`, `a_recovered_document_is_…` (app).

Both files were saved first, restored byte-for-byte (SHA-256 checked) and the positive
gates rerun green.

## CI wiring

Existing steps only. `ci.yml` stub step: 12 jobs gates, the Unix permission gate,
4 CLI gates, 3 app gates in the session loop plus the stub recovery gate, 1 UI gate.
`runtime-layout.yml`: the native session step (crash matrix and comparator self-check,
`FCAD_RECOVERY_SESSION_ARTIFACTS`, two artifact pairs required,
`FCAD_RECOVERY_SESSION_FBX_DIR`), the no-solver step (mixed gate), and the strict reader
loop in `tools/check-fbx-complex.sh` with its runtime grep.

## Real window recipe (macOS)

One freshly staged bundle; every viewer is started by the 1536 MiB watchdog, which
owns it and writes its PID to `<log>.pid`. The controlled crash kills **that PID only**
(`kill -KILL`), never by name; after each window ends only the PID is checked (no
getApp/getAX/screenshot that could relaunch it). The recovery folder is an explicit
test folder; your own recovery folder is never touched. About 20 minutes.

```sh
ROOT=/private/tmp/ferrite-30m-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 tools/document-crash-recovery-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery"
watch() {  # one owned viewer per session: watch NAME [document]
  python3 tools/watch-viewer-memory.py --log "$ROOT/watch-$1.jsonl" --limit-mib 1536 \
    -- "$APP/Contents/MacOS/ferritecad-viewer" "${@:2}" &
}
crash() { kill -KILL "$(cat "$ROOT/watch-$1.pid")"; sleep 1; ! kill -0 "$(cat "$ROOT/watch-$1.pid")"; }
```

The generator writes only `plate.fcad`, `occupied.fcad`, their pristine copies in
`inputs/`, `facts.json` and the empty `recovery/` folder; it refuses a destination
inside a checkout and produces no window output.

1. `watch a "$ROOT/plate.fcad"`. The plate opens; no recovery line.
2. **Edit extrusion…** → height 21.5 → **Apply**. Title `*plate.fcad`; the line says
   `Recovery copy written <time> UTC.`
3. Export STL → `$ROOT/accepted.stl`; Export FBX → `$ROOT/accepted.fbx`.
4. `crash a`.
5. `watch b`. The empty window lists **Recover unsaved work**:
   `plate.fcad — copy written <same time> UTC`.
6. **Recover**. Title `*plate.fcad (recovered) — FerriteCAD`; the plate is 21.5 high.
7. Export STL → `recovered.stl`; Export FBX → `recovered.fbx` (the dialogs suggest
   `plate (recovered)`).
8. **Save**: the dialog suggests `plate (recovered).fcad`; choose `occupied.fcad`
   (accept the system's replace prompt if shown) → refused, title unchanged. **Save**
   again → `recovered.fcad`. Title `recovered.fcad — FerriteCAD`; the recovery line goes.
9. Quit (no question). Check the owned PID is gone.
10. `watch c`. Nothing is listed. **New** → Empty → **Create document**: `*Untitled`,
    `Recovery copy written …`. `crash c`.
11. `watch d`. The list shows `Untitled — copy written …`. **New** → Empty → **Create
    document** (another dirty Untitled document).
12. **Recover** → the question → **Cancel**: nothing changes, the list stays.
    **Recover** → **Discard**: `*Untitled (recovered)`.
13. **Save** → `empty-recovered.fcad`. Quit. Check the owned PID is gone.

```sh
python3 tools/document-crash-recovery-gui.py --compare "$ROOT"
"$APP/Contents/MacOS/ferritecad" list-recovery --recovery-dir "$ROOT/recovery" --json
```

`--compare` refuses (`FCAD_30M_GUI_COMPARE_REFUSED`) before any build or peer job when
an output is missing; the Rust comparator refuses again before its first peer job. It
requires `FCAD_30M_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true`: the source
and the occupied file unchanged; no record left in the recovery folder (none listed,
none active); `recovered.fcad` equal in every SQL cell (only `modified_at` set aside)
to the CLI's `edit-extrude` of the untouched plate to 21.5; accepted and recovered STL
and FBX byte-identical to each other and to the CLI's exports of that file;
`empty-recovered.fcad` a document of neither input's identity and equal to a CLI
`create` in every cell but its id and creation stamp. Controls: a missing output, the
source written, the occupied file written, the old document as the recovered one, an
export of the old version, a record left behind, and an input as the recovered empty
document. The watchdog's own exit status for the two killed sessions is not success;
that is the experiment, not a failure.

## Execution record (cloud, Linux x86_64, running as root)

Existing targets, pinned OCCT and PlaneGCS (not rebuilt), `CARGO_BUILD_JOBS=2`,
sequential builds, debug profile locally (CI runs release). No window and no GPU were
run here; the viewer was never started.

* **Packed CI steps executed** (extracted from the YAML and run as bash):
  * `ci.yml` stub step: exit 0, **144** exact gates (142 `test … ok` lines and the two
    marker gates), every §30M gate among them (12 jobs, permission, 4 CLI, 4 app,
    1 UI), no `skipped:`. One local substitution: this container runs as root, so the
    Unix permission block ran the same built test binary as `nobody` (`runuser`) with
    the same four checks; it printed `FCAD_30M_PERMISSION_GATE_EXECUTED`.
  * `runtime-layout.yml` native session step (debug; `FCAD_OCCT_LIB_DIR` and the local
    PlaneGCS directory in place of the CI-populated `vendor/planegcs`): exit 0, **38**
    exact gates, both §30M marker gates, two artifact pairs written.
  * no-solver step: exit 0, **18** exact gates including
    `FCAD_30M_MIXED_RECOVERY_EXECUTED`.
  * `tools/check-fbx-complex.sh` with `FCAD_RECOVERY_SESSION_FBX_DIR`: exit 0,
    `FCAD_RECOVERY_SESSION_UFBX_EXECUTED`; `recovered-plate` `checks=6 failures=0`,
    64 triangles joined, worst 3.47e-18 m; `recovered-drawn` `checks=6 failures=0`,
    20 triangles, worst 1.73e-18 m.
* **Suites.** Stub: jobs all green (13 recovery + 4 recovery unit tests among them);
  UI 106; app 523 + 5; CLI 347 in 31 test binaries, plus two failures that are this
  container's root account, not this slice:
  `dump_graph::read_only_permissions_still_dump_when_the_file_can_be_read` and
  `validate::validation_really_read_only_permissions` fail as root and pass as `nobody`
  on the same binary. Native (OCCT + PlaneGCS): app 521 before the comparator tests were
  added, then the 8 recovery tests (crash matrix and comparator self-check included).
  As `nobody`: the jobs recovery suite 13/13 with the permission gate executed.
* **Mutations:** M30M-1 and M30M-2 killed as above, restored, gates green.
* `cargo fmt --check`; `cargo clippy --workspace --all-targets -D warnings` on the stub
  build and on the native build with `planegcs`: clean.

### Found and fixed during the slice

* A recovered document would have been suggested as `plate.fcad (recovered).fcad` by
  Save As and exports: now `plate (recovered)`.
* A failed publication said only "reserving document snapshot": it now says
  `writing the recovery copy: …`, so the status cannot be read as a document error.
* Gates for tests that print a marker cannot use `test NAME ... ok` (libtest puts
  `ok` on the next line); caught by executing the packed steps before any push.
* On Windows, removing a record whose lease another process has open is retried
  briefly (deletion completes when that handle closes).

### Limits

* Power loss is not tested or claimed: the order is `fsync` of file and directory, but
  macOS `fsync` does not flush the drive cache (no `F_FULLFSYNC`) and Windows has no
  directory sync.
* The window's `recover`, `delete_recovery` and start-up listing are methods of the
  winit `App` and are not constructed headlessly; their parts are gated (the claim,
  `recover_for_view`, `Bind::Open`, the guard's `replacing`, the predicate-driven
  widget) and their composition is covered only by the window recipe.
* A crash between a published Save and the worker emptying the record leaves the older
  copy listed although the file on disk is newer; the person decides (the table above).
* A filesystem without advisory locks gets no crash copies (the lease cannot be made)
  and lists other records as `unlockable`.
* The start-up listing verifies every record (SQLite read, BLAKE3) on a thread; it is
  bounded by the 32-record limit.
* The CLI lists and extracts but does not delete; Delete is the window's.
* Undo history, draft form values and the original file's path are not part of a crash
  copy (the name is a file name only).
