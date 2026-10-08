# §30L — verification

[Contract](unnamed-document-session.md), [decision](decisions/0005-document-session.md#30l-a-new-document-is-a-session-before-it-is-a-file).
Marker `FCAD_30L_UNNAMED_DOCUMENT_SESSION`. Base `fae2337aba01c2818ede09319c4c3bef68f72612`
(merge of PR #91); `origin/main` after fetch; PR #91 MERGED at that commit; clean tree.
Branch `unnamed-document-session`. Author/committer `gesriot <gessman1618@gmail.com>`.
No merge or auto-merge. Base, code/workflow head and docs-only head CI are recorded
separately below.

## What was found in the code before the change

`main.rs::ask_where_to_create` opened a Save dialog for every create form (New's
**Choose where to save…**, the drawing forms' **Create in new file…**, **Save circle
extrusion…**, **Save annular extrusion…**), `creates::run_create` published the file
there through `create_document_with_kernel`, and `AppEvent::Created` then opened the
published file through the ordinary Open. The New toolbar button alone was guarded
(`Continuation::New`); the drawing forms were not, so a dirty document could be
replaced from them without the question.

## What changed

* `DocumentSession` holds `Option` logical path and checkpoint. `create_in` writes
  the first version into the session's own private directory;
  `create_document_in` is that with the existing `create_document_with_kernel`
  (one `needs_kernel` classification; Empty and the sample plate create no kernel).
  `is_dirty` is true while there is no checkpoint; `SavePlan` refuses an in-place save
  of an untitled session; `record_saved` gives the session its path and checkpoint
  only for a published Save As. `UNTITLED = "Untitled"`.
* The window's create worker (`sessions::create_for_view`) makes the candidate and
  its picture; `AppEvent::Created` carries `Candidate { scene, session }`; `show`
  binds it with the same `Bind::Open` as Open. A refused, cancelled, stale or
  unshowable candidate is dropped with its directory; the New form and the drawing
  draft are kept (`draft_published` / `draft_load_finished`, `Creates::shown`).
* One `can_create` predicate is written into the New form (`NewDocumentForm::can_create`,
  **Create document**) and the drawing forms (**Create new document**, **Create circle
  document**, **Create annular document**) each frame and asked again by
  `create_new`. The guard moved to `create_new` (`Continuation::Create`, the content
  held as `pending_create` until the save is published; dropped otherwise). The New
  button only opens the form.
* Save on an untitled session asks for a path (`save_then` → `choose_save_as_path`,
  `Untitled.fcad` in the dialog's folder) and runs the existing no-clobber Save As.
  Export and Save As suggestions use `Sessions::suggestion()` (the logical file, or
  `Untitled` beside the dialogs' folder), never the private file. `SHOWN_AS` maps the
  private directory to **Untitled** for an untitled session.
* Found while testing and fixed: two refusals printed a path inside the private
  working folder (the library Save As refusal and the dialog refusal); both now name
  only the file.

## Gates

Library (`crates/ferritecad-jobs/tests/unnamed.rs`, no kernel needed):
`a_new_empty_document_has_no_file_no_checkpoint_and_is_unsaved_until_published`
(no path/checkpoint/Undo, Untitled, in-place refused without a private path in the
words, occupied and private/alias Save As refused with the session unchanged, first
Save As publishes exactly the current version, history kept),
`edits_undo_and_redo_before_the_first_save_never_make_it_saved` (Undo to the created
version stays unsaved; after the first Save the ordinary comparison, no-op and the
in-place version guard incl. a replaced-file conflict),
`a_failed_or_cancelled_creation_leaves_no_session_and_no_file`,
`the_first_save_is_published_or_not_and_a_late_cancellation_does_not_undo_it`.

Window, stub-safe: `creates::tests::the_form_stays_until_the_new_document_is_accepted`,
`creates::tests::stub_creation_is_refused_at_the_picture`,
`sessions::tests::new_document::the_first_save_names_the_document_and_only_then_continues_once`
(occupied, private-folder, in-place and cancelled first saves publish nothing,
continue nothing and keep the session; a published one names it and hands back the
continuation once), `sessions::tests::new_document::stub_new_document_is_refused_at_the_picture_and_keeps_the_open_one`,
`tests::the_create_predicate_lets_open_forms_create_and_excludes_other_work`
(idle-positive with the New form open, then a running creation, a load, an export,
an edit form and a session operation each close it),
`panels::tests::new_form_create_asks_nothing_while_the_window_says_it_cannot`, and
the drawing widgets now assert that a press asks nothing while the predicate is off.
The other `creates.rs` gates were rewritten for the candidate route (stale answer,
cancel early/late, shutdown join, a size the document refuses, first-Save refusal of
a taken name, both clients write the same plate/empty document).

Native (`sessions::tests::new_document`):
`native_every_new_document_variant_is_untitled_until_its_first_save_like_the_cli` —
one parameterised matrix over Empty, the sample plate 83×47×13, an L polygon, a Circle,
an annulus, a full-turn Revolve and a 137.5° partial Revolve. Each goes through the
window's create worker and `Bind::Open`; is Untitled, dirty, with no Undo and no private
path in the title or `shown_as`; equals `ferritecad create*` of the same content in
every SQL cell (rowid, ordinals, raw payload hashes) except `meta.created_at`,
`meta.modified_at` and a proved bijection of its identities (all new; positional,
consistent, one-to-one, none shared, payload hashes excluded from the scan as derived);
for the four with an Extrude a height Apply, Undo to the created version (byte-equal
SQL, still unsaved) and Redo; unsaved STL byte-equal to the CLI's and FBX equal under
the bijection; the first Save As writes exactly the current version (all cells, no
remap) with the same document id; reopen is clean; `rebuild --cold` reports the same
as the CLI document; exports from the saved file equal the unsaved ones; Redo history
survives the first Save; the private folder is gone after stop.
`mixed_new_documents_need_no_solver` runs the same matrix with OCCT and no PlaneGCS.
Artifacts `new-{polygon,annulus}-{before,after}-save.{stl,fbx}` are read by the pinned
strict ufbx reader and joined with oriented STL; runtime CI requires
`FCAD_NEW_DOCUMENT_SESSION_UFBX_EXECUTED`.

`native_window_scenario_on_session_files_passes_the_comparator_and_its_controls` runs
the window recipe below through the same worker/session/save owners and the
comparator with its seven controls. **A self-check of the comparator, not window
evidence.**

## Directed mutations

M30L-1 makes `DocumentSession::create_in` treat the private snapshot as the saved file
(`logical = Some(private path)`, checkpoint = the created version). It compiles;
`unnamed` fails three executed assertions (`left: Some(".../ferritecad-session-…/v0.fcad")`,
"Undo to the created version is not a save", `is_untitled() && is_dirty()`).
M30L-2 makes `Sessions::finish_save` return the continuation after a failed save. It
compiles; `the_first_save_names_the_document_and_only_then_continues_once` fails
(`SaveReport { published: false, continuation: Some(Create) }`). Both files were saved
first, restored byte-for-byte (SHA-256 checked) and the positive gates rerun green.

## CI wiring

Existing steps only: ci.yml stub step (four `--test unnamed` gates, five app gates,
one UI gate), runtime-layout native session step (two native gates, four artifact pairs
required, `FCAD_NEW_DOCUMENT_SESSION_FBX_DIR`), no-solver step (mixed gate), and the
strict reader loop in `tools/check-fbx-complex.sh` with its runtime grep.

## Real window recipe (macOS)

One freshly staged bundle, one owned viewer under the 1536 MiB watchdog; after Quit only
the PID is checked (no getApp/getAX/screenshot that could relaunch it). Choose a new
root outside any checkout. About 15 minutes.

```sh
FCAD_30L_GUI_ROOT=/private/tmp/ferrite-30l-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/new-document-session-gui.py "$FCAD_30L_GUI_ROOT"
python3 tools/watch-viewer-memory.py --log "$FCAD_30L_GUI_ROOT/watch.jsonl" \
 --limit-mib 1536 -- "$APP/Contents/MacOS/ferritecad-viewer"
```

The generator writes only `occupied.fcad`, its pristine copy `inputs/occupied.fcad` and
`facts.json`; it refuses a destination inside a checkout and produces no window output.

1. Start with no document. **New** → Sample plate, 83 × 47 × 13 → **Create document**.
   No file dialog; the title is `*Untitled — FerriteCAD`.
2. **Edit extrusion…** → height 21.5 → **Apply**.
3. Export FBX → `unsaved.fbx`, Export STL → `unsaved.stl` (the dialogs suggest
   `Untitled…`, never a temporary folder).
4. **Save** → Cancel the dialog: nothing changes. **Save** → choose `occupied.fcad`
   (accept the system's replace prompt if shown): FerriteCAD refuses ("already exists"),
   the title stays `*Untitled`.
5. **Save** → `plate.fcad`. The title is `plate.fcad — FerriteCAD`. Copy `plate.fcad`
   to `first-save.fcad`.
6. **Undo** (title dirty) → Export STL → `undo.stl`. **Redo** (clean). **Undo** again.
7. Quit (Cmd+Q) → **Cancel**: the window stays with the dirty plate.
8. **Create sketch + Extrude…** → Profile *Circle with hole*: center 12, −7; outer 10;
   inner 4; height 15 → **Create annular document** → **Cancel**: the plate stays, the
   numbers stay. Press it again → **Save**: `plate.fcad` is saved in place, then the
   annulus appears as `*Untitled`. Copy `plate.fcad` to `after-guard.fcad`.
9. Export STL → `annulus.stl`, Export FBX → `annulus.fbx`.
10. **New** → Empty → **Create document** → **Discard**: an empty `*Untitled`.
11. **Save** → `empty.fcad`. Quit (no question). Check the owned PID only.

```sh
python3 tools/new-document-session-gui.py --compare "$FCAD_30L_GUI_ROOT"
```

`--compare` refuses (`FCAD_30L_GUI_COMPARE_REFUSED`) before any build or peer job when an
output is missing; the Rust comparator refuses again before its first peer job (peer-run
counter). It requires `FCAD_30L_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true`:
the occupied file is unchanged; `first-save.fcad` is `create --sample` + `edit-extrude
21.5` under a bijection of new identities; `after-guard.fcad` (= the final `plate.fcad`)
has exactly the same identities as the first save and is the unedited plate;
`unsaved.stl`/`.fbx` and `undo.stl` match the CLI exports; `annulus.stl` matches and
`annulus.fbx` matches up to a bijection of UUIDs; `empty.fcad` is `create`. Controls:
missing output, occupied file written, old document as the first save, another document
as the guarded save, export of the old version, the plate's FBX as the annulus, and a
reference with the wrong owner. Read the actual `unsaved.fbx` and `annulus.fbx` with the
pinned strict reader and join them with their STL.

## Execution record (cloud, Linux x86_64)

Existing targets and pinned OCCT/PlaneGCS, `CARGO_BUILD_JOBS=2`, sequential builds,
debug profile locally (CI runs release). No window and no GPU were run here; the
headless widget gates are not window evidence.

- `cargo test -p ferritecad-app -p ferritecad-jobs -p ferritecad-ui` (OCCT + PlaneGCS):
  app 514, jobs incl. 4 `unnamed`, UI 105; `cargo clippy
  --workspace --all-targets --all-features -D warnings` and `cargo fmt --all --check`
  clean.
- Stub build (`/home/user/stub-target`, native/solver env unset): app 514, jobs, UI
  and CLI suites pass except `dump_graph::read_only_permissions_still_dump_when_the_file_can_be_read`,
  which requires enforced write denial and this container runs as root (uid 0); no CLI
  or document code changed.
- Packed run blocks executed (`matrix.name` → `linux`, `--release` dropped, native and
  mixed `target/release` → `target/debug`): stub exit 0, 122 exact gates (all ten new);
  native session step exit 0, 36 exact gates (both new), four artifact pairs; no-solver
  step exit 0, 17 exact gates (the mixed one included).
- `tools/check-fbx-complex.sh` with clang: exit 0; the four §30L pairs `checks=6
  failures=0`; oriented joins 20 triangles (polygon, worst 0 m) and 652 (annulus, worst
  3.47e-18 m); `FCAD_NEW_DOCUMENT_SESSION_UFBX_EXECUTED`.
- Both mutations failed executed assertions and were restored as above.
- Generator: refused a checkout destination; produced inputs only; `--compare` refused
  the missing outputs before any build.

**Not executed here:** the real window recipe (macOS, by the maintainer), macOS/Windows
native runs (CI), release-profile local runs, Windows/Linux windows.

### Limits

* The window's `create_new`/`guard_replacing`/`save_then` are methods of the winit `App`
  and are not constructed headlessly; their parts are gated separately (the predicate,
  `Sessions::replacing`/`finish_save` continuations, the create state machine, the
  dialog refusal) and the composition is covered only by the window recipe.
* Copy workflows still need a saved, clean document; their existing reason reads
  "unsaved changes: Save or Undo them first", for an untitled document only Save helps.

### Base CI (post-merge `fae2337`)

All success: CI 37707500898, planegcs pin 37707500848, product sbom 37707500856, rust
sbom 37707500872, rust notices 37707500879, combined runtime layout 37707500851
(linux 113085259837, macOS 113085259572, Windows 113085259786, compare 113109670143).

### PR CI (code and workflow head `3cf8275`)

The last commit that changes code or workflows is `3cf8275`; later commits only record
CI in this file. On `3cf8275`, all success: CI 37714482083, planegcs pin 37714461568
(compare 113109733400), combined runtime layout 37714461505 (linux 113107654201, macOS
113107654421, Windows 113107654482, compare 113131015381). In each of the three native
jobs the no-solver, native session and ufbx steps passed, and
`FCAD_NEW_DOCUMENT_SESSION_UFBX_EXECUTED` is present in the job log tail on linux,
macOS and Windows. Only the last ~5000 log lines are reachable from this environment
(the full-log host is outside its network policy), so the per-test exact-name gates are
counted from the steps' own pass/fail status, not re-read from complete logs. CI for the
final docs-only head is reported on the PR by its exact SHA.

## Independent macOS review (2026-10-07)

Review work is under `/private/tmp/ferrite-pr92-review`, with the existing native
target, pinned OCCT/PlaneGCS, two build jobs, and sequential builds. The original
code head `3cf8275` was checked from complete GitHub logs: all 15 jobs succeeded,
all 39 new exact gate executions (10 stub + 2 native + 1 mixed on each OS) were
found, no skip was accepted, and the new-document ufbx marker occurred on all
three platforms. The base was checked separately (27 successful checks).

Two application defects were fixed during review:

* `ed96e0c`: the creation status kept saying "Untitled, not saved yet" after Save
  or Open. It now reports the historical fact, "Created a new document."
* `177772b`: the drawing-form opener still required a clean document, making
  step 8 above unreachable. It now uses the existing idle-session permission,
  while copy routes retain their saved/clean requirement. An actual pointer-driven
  widget regression failed once against the old condition (executed assertion,
  not compilation), then passed with the fix. The stale Revolve help was corrected.

The new widget test first passed locally in release, but debug CI detected an
unhandled egui texture delta. `17bfabf` fixes only this test's frame helper by
clearing the delta, as the existing helpers do. That failed CI attempt is not a
passing result. Window evidence below uses `177772b`; the later change affects
only the test helper, not application code. The exact widget gate was also
re-run locally in debug on `17bfabf`: one executed test passed.

Local verification: 37 jobs session/save/unnamed tests, 105 UI tests, 69 sketch
tests, the native seven-variant matrix and comparator self-test, creation and
first-Save component tests, fmt, workspace clippy with all targets/features and
`-D warnings`, and actionlint. Native-inapplicable stub/mixed gates were reported
as skips and were not counted as executed geometry. No new native dependency
build or large STEP campaign was needed locally.

### Actual window and actual output files

One freshly staged, signed Apple Silicon bundle was launched under the 1536 MiB
watchdog. The first attempt exposed the drawing-opener defect and was closed
normally (189.45 MiB peak). After the fix and a fresh build/stage, the recipe was
completed using only files under `gui-models` outside the checkout:

* New plate, Apply height, unsaved STL/FBX exports, Save Cancel, occupied-output
  refusal, first Save, Undo/Redo and export after Undo.
* Quit Cancel; drawing an annulus alongside a dirty plate; replacement Cancel
  kept the plate and typed values. Save wrote the plate and continued creation
  exactly once, yielding an unsaved annulus, which was exported.
* Additional nested first-Save checks: New Empty from the unsaved annulus,
  choose Save then cancel the file dialog; retry Save to occupied output and
  refuse. Both kept the annulus and the Empty form. Discard then accepted Empty.
* Quit Empty, choose Save then cancel its dialog: the window remained. Repeating
  Quit/Save to `empty.fcad` published the file and exited normally. This exercises
  the first-Save continuation rather than only toolbar Save followed by Quit.

The fixture generator produced only its declared inputs. Before GUI execution,
the comparator refused all missing outputs. After execution, it read the actual
window artifacts and reported
`FCAD_30L_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true` (one executed
test, no skips). The only shell copies were `plate.fcad` to `first-save.fcad`
immediately after first Save and to `after-guard.fcad` immediately after guarded
Save, as the recipe requires. No CLI substitute was used for a GUI output.

Pinned ufbx 0.23.0 independently read actual `unsaved.fbx` and `annulus.fbx`:
6 checks / 0 failures each. Their oriented triangle joins with the actual STL
were respectively 12 and 652 triangles, worst error `3.47e-18` metres.

Final owned PID 4714 exited 0 after 600.7 seconds, without watchdog intervention;
peak footprint **209.74 MiB**, pressure normal, swap delta **0**. After Quit only
the process/watchdog was observed; no CUA app lookup could relaunch the viewer.
Windows/Linux window behaviour and the historical OOM cause remain unverified.

### CI at the time this review record was written

Code/test/workflow head: `17bfabfb257b3ab17b126bbedfcbad361697a63d`.
CI run [37727885547](https://github.com/gesriot/ferrite-cad/actions/runs/37727885547)
has 7 successful jobs (the macOS-only retry followed a DNS failure resolving
`index.crates.io` before compilation, not a test failure). Pin run
[37727881930](https://github.com/gesriot/ferrite-cad/actions/runs/37727881930)
has 4 successful jobs. Native runtime run
[37727881939](https://github.com/gesriot/ferrite-cad/actions/runs/37727881939)
is still executing at the time of this documentation commit and is not claimed
as passed here. The merge review must verify its final outcome separately. The
review adds one exact widget gate per OS, so the new-document campaign now has
42 required executions (11 stub + 2 native + 1 mixed on each OS). A docs-only
head has its own 7-job CI; it does not stand in for this code-head runtime run.
