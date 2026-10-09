# §30Q — Open, New and Recover beside an unfinished form

Markers `FCAD_30Q_*`. Builds on [§30P tab drafts](tab-edit-drafts.md), [§30O document
tabs](document-tabs.md) and [ADR 0005](decisions/0005-document-session.md). This one
file is the contract and the verification record of the slice.

## What a person can do

In A, open **Edit extrusion…** and type `2..6` (not a number yet), or open any other
form below. Without Apply, Cancel or Save: **Open…** B, **New**, **Create sketch +
Extrude…**, or **Recover** a crash copy. Each new document comes up in a tab of its own
with no form; press A's tab and A's form is exactly as it was left — the same text,
picks, pending changes and draft Undo/Redo — over A's unchanged, still unsaved model
and its document history. Opening, making or recovering a document never applies,
cancels or saves A, and A's form never has to be closed for it.

## Entry points and their owners

One predicate decides whether the shown tab may be left now:
`may_leave_tab` (`main.rs`) — no session operation (Apply, Add, Undo, Redo, Save, a
switch, Recover), no **New** (its form, drawing or worker), no copy worker, no pointer
gesture (camera or vertex drag). An idle form does not hold it. `can_leave_tab` is
`may_leave_tab` plus no load, export, export question or STL options form. Every
widget and its handler ask the same function.

| Entry point | Widget availability | Handler (route) | Predicate | The shown tab's forms |
|---|---|---|---|---|
| Toolbar **Open…** (native dialog) | `Activity.can_open` | `App::pick_and_open` (asked before the modal dialog) → `open_chosen` (asked again) → `read_document` | `may_leave_tab` | stay in the window while the file is read; move with A in the `Bind::Open` statement (`Tabs::open` → `Tabs::leaving`) |
| Document named at start-up | — | `resumed` → `open_chosen` | `may_leave_tab` | none exist yet |
| Platform file-open event | none: this build has no `openFiles`/opened-URL handler | — | N/A | — |
| A file a tab already names (same path, link, hard link, other spelling) | as Open | `Tabs::opening`: `Shown` (words only) or `Show(tab)` → `switch_to` | `can_leave_tab` | that tab's own come back (§30P `Tabs::activate`); the left tab's are parked; never a ninth tab, also at 8 |
| A newer Open while one is read | as Open | `Loads::open` retires the older generation (its worker is cancelled at once and joined later) | `may_leave_tab` | the older answer is dropped by `Loads::accepted_path`; nothing moves |
| Copy workflow's file (*Save new file…* of a form) | — | `App::open` → `read_document`, its §30A–§30K guard unchanged | unchanged | its form went with the publication (`published_draft`/`published_form`), unchanged |
| Toolbar **New** (Empty, sample plate) | `can_create_document = can_leave_tab` | `App::begin_new(false)` → `open_new` → `ask_new` | `can_leave_tab` | **set aside** as A's draft (`Tabs::set_aside`) |
| **Create sketch + Extrude…** (Line polygon, Circle, Circle with hole, full and partial Revolve) | the editor's button, offered beside a tab's form too, enabled by `can_leave_tab` (`Sections::can_new`) | a request (`Editor::take_drawing_request`) → `App::begin_new(true)` → `open_new` → `Editor::begin` | `can_leave_tab` | **set aside** |
| New's Create (form or drawing) | `can_create` (unchanged) | `App::create_new`: the 8-tab room first, then the worker | `can_create` | stay aside; an accepted candidate hides A **with them** (`Tabs::open` takes the set-aside draft) |
| New's Cancel (form **Cancel**, drawing **Cancel draft**) | — | `answer_form`/`Editor::dismiss`, then `App::end_new` → `Tabs::bring_back` | — | back, exactly as left |
| **Recover** (start-up list) | `RecoveryPanel.can_act = can_recover` | `App::recover`: the 8-tab room first, then the claim's worker holding A's slot | `can_recover = can_leave_tab && !recoveries.running()` | stay in the window during the worker; move with A at `Bind::Open` |
| Recovery **Delete** | `can_recover` | `App::delete_recovery` | `can_recover` | untouched |
| Tab row (show, ×) | §30P, unchanged | `switch_to`, `close_tab` | `can_leave_tab` | §30P |

## Rules

1. **The draft is the tab's.** A's forms are §30P's `tabs::Draft` — the window's
   `Edits` height form and `sketch::Editor` (vertices, Circle/annulus, Revolve angle,
   constraints, Cut/Fillet/Chamfer Add and Edit), moved whole as values, keyed by A's
   runtime `TabId`, bound to A's current `Snapshot` by identity. Nothing is copied into
   a new document, serialised, re-read or normalised; two copies of one file (one
   `DocumentId`) are two tabs with two drafts. No second, per-form protocol: Open and
   Recover use the existing `Bind::Open` move; New uses the same `Forms::park` /
   `Forms::restore`.
2. **New's ownership (decided before implementation).** New has its own forms: the
   window's `Creates.form` and, for drawings, the same `sketch::Editor` A's saved-object
   forms live in (the drawing's polygon, circle and annulus states share fields with the
   saved Circle/annulus forms). So opening New first **sets A's forms aside**
   (`Tabs::set_aside`, one `Option<(TabId, Draft)>`), leaving the window's forms empty;
   New never writes into A's. While New is open: New holds the window (no tab, Open,
   Recover, Apply, document Undo, Save or Quit), as it did before §30Q. New's typed
   values survive a size that is not a number, a document the kernel refuses, a picture
   the device refuses, a cancelled or late creation and a full window — the existing
   §30L promise — and A's forms stay aside meanwhile. **Cancel** discards New's values
   (as before) and gives A's back exactly (`Tabs::bring_back`, never over a form the
   window shows). An **accepted** New is a separate Untitled tab with no form, and A is
   hidden with the set-aside draft in the same statement. An unfilled New form is not a
   tab.
3. **Open and Recover accept together.** A new tab, its session and its picture become
   current in the one `Bind::Open` statement after the picture is prepared. Cancel of
   the dialog, an unreadable file, a refused CPU/GPU preparation, a cancelled, stale or
   foreign answer change no tab, leave A shown with its form and add no empty tab.
4. **Limit before anything is spent.** At 8 tabs Open is refused before reading
   (`Tabs::opening`), New at Create before the worker, Recover before the claim; a
   reading, candidate or claim that reached the bind anyway is refused there and drops
   its files and claim. Nothing typed is lost by a refusal.
5. **Recovery.** Every refusal leaves the record whole and recoverable; a live
   window's record is never offered or claimed; an accepted Recover's adopted record
   belongs to the new tab only (closing it retires that record only). Cancel releases
   the slot at once and the late answer is refused (`Sessions::finish_recovery`); the
   explicit lease unlock of PR #96 is unchanged.
6. **The accepted model only.** Save, Save As, exports, checkpoints, crash copies and
   document Undo/Redo still read only accepted versions; A stays unsaved through
   Open/New/Recover. Close and Quit stop at any open form, also one set aside for New
   (`Tabs::close_step`/`quit_step` count the set-aside draft); a Save or Discard answer
   about the accepted model never closes a form.
7. **Unchanged.** One GPU scene; at most 8 tabs; copy workflows still need a saved,
   clean document; every Apply/Add keeps its version guard; a running worker, a
   gesture, a native dialog and New itself still hold the window. New/Recover also
   wait for exports and their questions; Open retains its prior export cancellation. CLI, document schema, capabilities and JSON v1 are unchanged. Jobs recovery cleanup
   avoids locking published records (review fix below).

A reading that arrives during a vertex drag in A's form parks the form with it; when A
is shown again the canvas sees no pressed button and reverts the unfinished drag
(its existing lost-release rule), so the draft returns as it was before that drag.

## Found defects

* **Recovery cleanup briefly owned published orphans (pre-existing §30M defect,
  fixed during independent review).** Creating New's recovery record probed every
  record with an exclusive lease. A simultaneous Recover therefore reported an
  orphan as a live window. Cleanup now skips any record with a manifest without
  taking its lease; an apparently empty record is locked and checked again before
  removal. Actual ownership/claim locks, active-owner protection, and explicit lease
  unlock are unchanged. The deterministic regression holds the cleanup probe open
  while a claimant locks the same published orphan: it failed before the fix and
  passes afterward. The native New → Recover scenario no longer waits for unrelated
  recorder lanes. Concurrent explicit listing/claim operations still use the existing
  exclusive protocol; this change addresses automatic cleanup, not all contention.
* **Drawing button permission (review).** The old saved-form fallback could enable
  Create sketch + Extrude during a camera gesture even though its handler refused
  the request. The button now uses the window's New permission alone. The real-widget
  regression failed before the change and passes after it; a dirty document still
  permits New when idle.
* A test-only timing error (reading the store before asynchronous retirement finished)
  was corrected by waiting for lease and directory, as §30O's tests do.

## Not here

Drafts across a restart or crash, a draft marker in the tab row, Open over a running
New, several foreground jobs, automatic Apply,
new geometry. §30, Milestone 5C and the product stay open; the earlier OOM
investigation is not closed by this slice.

## Verification

### What is checked, and where

**Kernel-free, every OS (`ci.yml`, step *Open, edit, Undo, Redo and Save one document
without native geometry*, exact name and marker each):** the real owners (`Loads`,
`Creates`, `Recoveries`, `Tabs`, `Sessions`, `Edits`, `sketch::Editor`, `present`,
`open_new`/`end_new`, the predicates) on the mock kernel.

* `open_over_unfinished_forms_keeps_them_with_their_tab_through_every_answer`
  (`FCAD_30Q_OPEN_OVER_FORMS_EXECUTED`) — A with an accepted change (14 mm, dirty, one
  document step) and the literal height `2..6`; Open held by an Apply and a camera
  drag; a newer Open retires the older one (its token cancelled at once) whose late
  answer adds nothing; Cancel, an unreadable file and a refused upload add no tab; an
  accepted copy of A (same `DocumentId`) is a tab with no form; its vertex draft with
  draft Undo/Redo; B's document Undo/Redo move B only; a hard link of A shows A's tab
  with `2..6`, dirty, not held; 8 tabs: a ninth file is refused before reading and at
  the bind, the alias still shows A; Apply `26` reaches A only; no input file written.
* `new_over_unfinished_forms_sets_them_aside_and_gives_them_back_exactly`
  (`FCAD_30Q_NEW_OVER_FORMS_EXECUTED`) — A's constraints draft with a request Redo; New
  held by an Apply, a camera drag, an export and its question; New's form and drawing
  set A's forms aside; while open: no tab/Open/Quit, Close and Quit stop at the
  set-aside forms; `77x`, a refused plate, a refused upload, a creation cancelled while
  running (its late candidate dropped) keep New's `50` and A aside; Cancel gives A's
  draft and history back; the drawing's Cancel too; an accepted New is an Untitled tab
  and A is hidden with its draft; A's Apply is offered again; at 8 tabs New is refused
  at Create and at the bind, Cancel gives back the shown tab's `x7`.
* `recover_over_an_unfinished_form_keeps_the_record_on_every_refusal`
  (`FCAD_30Q_RECOVER_OVER_FORMS_EXECUTED`) — A's live record neither offered nor
  claimable; Recover beside A's vertex draft: Cancel, a refused upload and a stale
  generation leave the record whole and the draft as left; accepted: a recovered tab
  owning the record; 8 tabs refuse before the claim and at the bind (claim let go);
  closing the recovered tab retires its record only.
* `sketch::tests::new_over_forms::new_opened_over_a_floating_form_shares_no_focus_or_text_with_it`
  (`FCAD_30Q_NEW_FORM_FOCUS_EXECUTED`) — real egui widgets drawn as the window draws
  them: A's floating Circle form has the keyboard; the drawing's button beside it
  raises a request (A's form untouched); after `open_new` keys reach none of New's
  fields; New's own Circle field takes typing; after its Cancel A's text and history are
  exact and typing without a click changes nothing; the same with New's form fields.
* `stub_open_new_and_recover_beside_a_form_are_refused_at_the_picture_and_keep_it`
  (`FCAD_30Q_STUB_OVER_FORMS_EXECUTED`, executes only without a kernel) — the
  production `open_for_view`, `run_create` and `recover_for_view` are refused at their
  picture; A's form, New's form until its Cancel, and the record stay.

**No solver (`runtime-layout.yml`, OCCT without PlaneGCS):**
`mixed_open_new_and_recover_beside_forms_with_occt_and_no_solver`
(`FCAD_30Q_MIXED_OVER_FORMS_EXECUTED`) — production Open, New and Recover drawn by Open
CASCADE beside each tab's height form; both applied after returning.

**Native (`runtime-layout.yml`, OCCT + PlaneGCS):**
`native_open_new_recover_beside_forms_then_apply_save_and_compare_with_the_command_line`
(`FCAD_30Q_NATIVE_OVER_FORMS_EXECUTED`, `FCAD_30Q_SESSION_FILES_COMPARE_OK
negative_controls=8 all_SQL_cells=true`) runs the window recipe below on the owners
(production workers, no window) and then the recipe's comparator and its eight controls.
**Not window evidence.** The comparator: `a.fcad` equals the CLI's `edit-extrude 26` +
`edit-sketch-copy` (80 → 90) in every SQL cell but `meta.modified_at` (the one stamp
every writer refreshes; the existing `all_sql` allowlist); A's ids and references equal
the input's; `a-unsaved.stl`/`.fbx` byte-equal the CLI's exports of that copy, and the
saved file exports the same bytes; A 90 × 40 × 26 = 93 600 mm³ by an independent STL
reading; `b.fcad` byte-equal its input (never written); `c.fcad` equals the CLI's
`edit-extrude 22` of the crashed plate in every SQL cell but the stamp, its ids equal
the plate's, its STL equals the CLI's, 80 × 40 × 22 = 70 400 mm³; `n.fcad` has a
`DocumentId` of its own and is 50 × 30 × 7 = 10 500 mm³; the recovery folder is empty.
Controls, each on a copy, each refused for its own reason: a missing output (and no peer
job ran first), an unsaved `a.fcad`, A's vertex draft lost at New (height only), `b.fcad`
written, swapped exports, `c.fcad` not the recovered model, `n.fcad` not a new document,
a record left behind. Artifacts `over-a` (the window's unsaved export of A), `over-n`,
`over-c` (CLI exports of the saved files) go to the pinned ufbx block of
`tools/check-fbx-complex.sh` (`FCAD_OPEN_NEW_RECOVER_UFBX_EXECUTED`).
`native_compare_real_open_new_recover_gui_artifacts_with_negative_controls` runs the
same comparator on real window outputs in `FCAD_30Q_GUI_DIR`; without the variable it
does nothing.

### Execution record (author, macOS arm64, 2026-10-09)

Base: `main` = `origin/main` = `210b2e666f92941b8415445a443e8e03ddba4143` (PR #96 merged
at 15:36:58 UTC, reviewed head `ffa29ff`). When this slice began, its post-merge
`product sbom` run had succeeded and CI, rust sbom, rust notices, combined runtime
layout and planegcs pin were **in progress** (not counted as passed here). When this
record was written, CI, planegcs pin, rust notices, rust sbom and product sbom had
completed with success on that commit and combined runtime layout was still in
progress — not counted as passed. Branch
`open-new-recover-with-drafts`, uncommitted; **no remote CI has run for this diff.**
`rustc 1.96.0`, `CARGO_BUILD_JOBS=2`. Vendor OCCT and PlaneGCS were not rebuilt. No
viewer, bundle, GPU window, `osascript` or browser was run. Memory pressure stayed
normal (55–63 % free), swap unchanged at 1018 MiB, 142 GiB disk free.

* **Native** (`source /private/tmp/ferrite-pr93-review/env.sh`, target
  `/private/tmp/ferrite-24b-native-target`, commands in that sourced zsh):
  `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` clean; `ferritecad-app --features planegcs` **572
  passed, 1 ignored** (pre-existing) + 3 integration, 0 `skipped:` lines in the
  captured log. In that build the stub-only and no-solver-only gates and the env-gated
  GUI comparator return without executing: N/A, not passes.
* **Runtime-layout, exact lines** (extracted from the edited YAML; only `RUNNER_TEMP`,
  `GITHUB_ENV` and the matrix name supplied; release viewer and peer CLI built first):
  the native tabs block exit 0 with all §30O/§30P markers, `FCAD_30Q_NATIVE_OVER_FORMS_EXECUTED
  volume_a=93600.000 volume_n=10500.000 volume_c=70400.000` and
  `FCAD_30Q_SESSION_FILES_COMPARE_OK negative_controls=8 all_SQL_cells=true`, 14
  artifacts, three `GITHUB_ENV` lines; the two no-solver blocks (`FERRITECAD_REQUIRE_PLANEGCS=0`,
  no `planegcs` feature) exit 0 with `FCAD_30P_MIXED_DRAFTS_EXECUTED` and
  `FCAD_30Q_MIXED_OVER_FORMS_EXECUTED`.
* **Pinned ufbx:** the §30Q block of `tools/check-fbx-complex.sh` with the script's
  preamble and reader build unchanged (ufbx `fcc5d6b…`, cached, strict): `checks=6
  failures=0` for `over-a`, `over-n`, `over-c`, STL↔FBX joins of 12 triangles each,
  `FCAD_OPEN_NEW_RECOVER_UFBX_EXECUTED`. The script's complex-STEP part was not rerun.
* **True stub** (target `/private/tmp/ferrite-25j-stub-target`, a clean environment:
  no `DYLD_*`, no `FCAD_PLANEGCS_DIR`, `OpenCASCADE_DIR` and `CMAKE_PREFIX_PATH` =
  `/private/tmp/absent-occt-true-stub`, `FERRITECAD_REQUIRE_OCCT=0`,
  `FERRITECAD_REQUIRE_PLANEGCS=0`): the build used bridge directory
  `ferritecad-occt-bd6e1e14777ad9eb`, whose cache holds
  `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE`, and warned *"Open CASCADE was not
  usable … configuring the bridge failed"*; `otool -L` of the app test binary and of
  the CLI shows no `libTK*` and no PlaneGCS.
  * The exact `run:` block of the edited `ci.yml` step, extracted and run with
    `RUNNER_OS=macOS` from an ignored directory: exit 0, 202 `test result: ok` lines, 0
    `skipped:`, all five §30Q markers with the §30O/§30P ones.
  * Stub `ferritecad-app` suite: 572 passed, 1 ignored.
  * Stub `cargo clippy --workspace --all-targets -- -D warnings`: clean.
* `tools/check-export-boundary.sh` passes; `tools/check-licence-headers.sh`: 461 files,
  all MIT; `actionlint` (with shellcheck) on `ci.yml` and `runtime-layout.yml` clean
  after grouping the three `GITHUB_ENV` appends (SC2129 flagged the third separate
  append); `shellcheck tools/check-fbx-complex.sh` clean; `git diff --check` clean.
* `tools/open-new-recover-gui.py`: a root inside the checkout is refused
  (`FCAD_30Q_GUI_FIXTURE_REFUSED`, nothing made); a root outside made only the inputs —
  `a.fcad`, `b.fcad`, `child/plate.fcad`, `inputs/`, `facts.json` and one recoverable
  `plate.fcad` record (`active` 0) by the controlled child, which was killed and left
  no process; `--compare` refused before any build on missing outputs and, with
  placeholder outputs, on an unsaved `a.fcad` (`FCAD_30Q_GUI_COMPARE_REFUSED`).

**Directed mutations** (native debug, on the final formatted source; both compile and
fail an executed assertion; the files restored byte for byte — `tabs.rs`
`334678c7…bcfef6`, `main.rs` `53294694…b8c926` before and after — then the affected
tests and the whole suite passed again):

* **M1 — New's Cancel loses A's draft.** `Tabs::bring_back` drops the set-aside draft
  instead of restoring it. `new_over_unfinished_forms_…` fails on the Cancel (`left:
  None, right: Some(SketchConstraintEdits { … })`) and the focus gate fails
  (`creates.sketch.editing_analytic()`).
* **M2 — a late Open is accepted.** `Loads::accepted_path` no longer asks whether the
  generation is still awaited. `open_over_unfinished_forms_…` fails: the answer to the
  Open given up on (Cancel) added a tab (`!w.deliver_load(… second …)`).

A first M2 candidate, `Recoveries::accepts` taking any generation, did **not** fail:
a stale Recover answer is refused again by `Sessions::finish_recovery` (the slot is held
for the newer generation) — two guards, not a gap. It was restored by SHA-256 and
replaced by the M2 above.

Logs are in the author's checkout under the ignored `target/30q-logs` and
`target/30q-ci`, not in the repository.

### Real window recipe (macOS)

Not run by the author. One freshly staged arm64 bundle `APP`; run from the checkout in
the native environment (`source <env.sh>`: the generator builds the recovery tests'
controlled child with `--features planegcs`, and `--compare` runs the Rust
comparator). One viewer under the 1536 MiB watchdog; a new root outside any checkout;
the recovery folder is the root's own — the person's own recovery folder is never
touched. Never call `getApp`/`getAX` after Quit; check that the owned PID is gone.

```sh
ROOT=/private/tmp/ferrite-30q-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 tools/open-new-recover-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/a.fcad"
```

The generator writes only the inputs: `a.fcad` (CLI sample 80×40×12), `b.fcad` (CLI
circle, centre 12.5, −7.25, r 10.5, h 15.25), `child/plate.fcad` (CLI sample 80×40×12)
and its crash copy at 22 mm in `recovery/` (the controlled child `named-dirty` of
`ferritecad-jobs --test recovery`, killed by its own PID), pristine copies in
`inputs/`, and `facts.json`. It refuses a root inside a checkout and makes no window
output.

1. `a.fcad` opens; the recovery list offers `plate.fcad — copy written … UTC`.
2. **Edit extrusion…** → choose the extrusion → type `2..6`. Do not Apply. **Open…**,
   **New** and **Recover** stay enabled.
3. **Open…** `$ROOT/b.fcad`: a second tab, shown, with no height form.
4. Press `a.fcad`: the form shows `2..6`. Replace it with `26` → **Apply** (`*a.fcad`).
5. **Edit Sketch …** (the saved Sketch) → replace both `80` with `90`. Do not Apply.
6. **Create sketch + Extrude…** (beside A's form): the drawing opens and A's Sketch
   form goes. Choose **Circle**, type a Center X. **Cancel draft**: A's Sketch form is
   back with both `90`.
7. **New** → **Sample plate** → Width `50`, Depth `30`, Height `7` → **Create
   document**: a `*Untitled` tab, shown, with no form.
8. **Recover** `plate.fcad`: a `*plate.fcad (recovered)` tab; the list is empty.
9. Press `*a.fcad`: both `90` are there → **Apply vertices**. **Export STL…** →
   `$ROOT/a-unsaved.stl`; **Export FBX…** → `$ROOT/a-unsaved.fbx`. **Save** (`Cmd+S`).
10. Press `*plate.fcad (recovered)` → **Save** → `$ROOT/c.fcad`.
11. Press `*Untitled` → **Save** → `$ROOT/n.fcad`.
12. Quit (`Cmd+Q`): nothing is asked; the window ends; check the owned PID is gone.

```sh
python3 tools/open-new-recover-gui.py --compare "$ROOT"
```

`--compare` refuses (`FCAD_30Q_GUI_COMPARE_REFUSED`) before any build or peer job when
an output is missing or `a.fcad` was never saved; the Rust comparator refuses a missing
output again before its first peer job. It reads only the window's outputs and requires
`FCAD_30Q_GUI_COMPARE_OK negative_controls=8 all_SQL_cells=true` (the comparison and
controls listed above).

### Limits

* No real window was run by the author; the owners' scenario and the headless widget
  test are not window evidence.
* The window's per-frame wiring (`App::begin_new`/`end_new` placement in the frame, the
  toolbar's flags, the drawing button's request) is exercised through the functions it
  calls (`open_new`, `end_new`, `may_leave_tab`, `can_leave_tab`, `can_recover`,
  `Editor::take_drawing_request`), not by a drawn window.
* Recovery cleanup was fixed during review; the native gate no longer waits it out.
* Open still abandons an export in flight, as before §30Q (an export cannot coexist with
  an open form).
