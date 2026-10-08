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

At the time of the code push: CI 37707500898, planegcs pin 37707500848, product sbom
37707500856, rust sbom 37707500872 and rust notices 37707500879 success; combined
runtime layout 37707500851 still running (linux 113085259837 and macOS 113085259572
success, Windows 113085259786 in progress) — not counted as success; recorded again
when complete.

### PR CI

Reported on the PR by exact head SHA.
