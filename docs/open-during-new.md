# §30T — Open while New is not finished

Single-tab Close over a form is now described by [§30U](close-tab-with-draft.md);
this slice's Open/New rules are unchanged.

Markers `FCAD_30T_*`. Builds on [§30Q](open-new-recover-with-drafts.md),
[§30O document tabs](document-tabs.md), [§30P](tab-edit-drafts.md) and
[ADR 0005](decisions/0005-document-session.md). This one file is the contract and the
verification record of the slice.

## What a person can do

Press **New** (the form, or **Create sketch + Extrude…**), type, even while the document
is being made, and press **Open…**. Cancelling the system file dialog changes nothing.
Choosing a file makes the window ask, in its own words, under the New form:

> **New is not finished.** You chose *b.fcad* while a new document is still being
> prepared. Opening it means giving up the new document: *the choices and sizes typed in
> the New form will be discarded*. Documents that are already open, with their unsaved
> changes, are not touched.
> **[Discard New and open]** **[Back to New]**

The clause names what is lost: the choices and sizes of the form, the sketch that was
drawn, or the document being made (nothing of it is kept). *Back to New* withdraws the
question; New keeps every typed string, choice and drawing Undo/Redo exactly. *Discard
New and open* is the only moment New's draft is thrown away: the file is then opened by
the ordinary Open, beside the tab that was shown, whose unsaved model, history, crash
copy and any form it had are as they were. If New turns into a tab of its own before the
person answers (the creation finished), nothing is thrown away: the chosen file opens
beside that tab by the ordinary Open, and that tab is an ordinary unsaved document.

## Decided before implementation: events and owners

New belongs to `creates::Creates` (its form, the drawing in `sketch::Editor`, the
creation workers) with the shown tab's forms set aside in `tabs::Tabs::aside` (§30Q).
The question is one typed value, `creates::AskedOpen { path, over: NewGeneration }`,
held by `Creates`: `NewGeneration` numbers one opening of New (a form or a drawing) from
its press to its end, so a question can never be answered against a later New. It is a
file name and a number: no document, tab or session is made to hold it.

| Event | Owner of the decision | Result |
|---|---|---|
| New form / drawing idle | `Creates.form`, `Editor::drawing_new`; A's forms in `Tabs::aside` | **Open…** offered (`can_open` drops only `!making_new`) |
| Create running | `Creates` (`Running{g}`, worker in `running`) | Open… offered; the question says a document is being made |
| Result queued | worker finished, `AppEvent::Created{g}` waits; `Creates` still `Running{g}` | a decision sees `Running`; the late answer is judged by `Creates::answered` against `current` |
| Picture acceptance | `Created` handler: `answered` → `show`/`Bind::Open` → `shown`, one statement on the event loop | nothing interleaves; success makes New a tab and settles the question |
| Open… chosen | `choose_open`: `can_open`, then New in progress? | New unfinished → `Creates::ask_open`; otherwise the ordinary `read_document` |
| Dialog Cancel / Failed | `Dialogs::receive` | `None`: no path, no question; model and New untouched |
| Back to New | `Creates::keep_new` | question withdrawn, nothing else changes |
| Discard New and open | `stop_new_for_open`: every check first (`can_open`, `Tabs::opening`), then `Creates::stop_new`, `end_new`, `read_document` | New's form, drawing and creation end; A's forms return; the file is read |
| Late Create success / failure | `Creates::answered` (`current` is `None` after the stop) | candidate dropped by its owner; status, `Loads`, `Sessions`, a newer generation untouched |
| New ends otherwise (its Cancel, accepted as a tab) | `Creates::new_ended` from `App::end_new` and the `Created` handler | the chosen file opens by the ordinary Open beside what is shown; nothing is destroyed |
| Second Open… | `choose_open` again | replaces the question's file; a newer reading retires an older one as before |
| Full window (8 tabs) | `Tabs::opening` before asking and again before stopping | refused in words; nothing asked, nothing discarded |
| Open fails / refused picture | `Loaded` handler, `Tabs::bind` | A stays shown with its forms; New is not recreated |
| Cancel of the reading | `cancel_load` | A stays; New is not recreated |
| Quit | `begin_window_quit` | [§30W](quit-with-new-draft.md) asks about idle New in the existing Quit pass; running/queued Create and §30T's pending Open decision keep their holds. Open-over-New is unchanged. |

## Rules

1. **One policy, one narrow exception.** `can_open` is `idle_but_for_new && !restores.running()`;
   `may_leave_tab` is `!making_new && idle_but_for_new`. The toolbar's Open, its dialog
   handler, the question's Discard button and its handler ask `can_open`. A session
   operation (Apply, Add, Undo, Redo, Save, a switch, Recover), a pointer or vertex
   gesture, a copy worker and a Reopen still hold Open. `can_leave_tab`, `can_recover`,
   `can_restore`, `can_create`, New itself and the tab row are untouched.
2. **Checks before the destructive step.** `choose_open` says a full window and the
   shown tab's own file in words and asks nothing. `stop_new_for_open` asks `can_open`
   and `Tabs::opening` again before it ends anything; a refusal leaves the question or
   withdraws it, with New as typed. The decision is made against the state then,
   whatever a worker answered while a dialog or the question was up.
3. **What Discard ends, and what it keeps.** New's form (`Creates.form`), its drawing
   (`Editor::dismiss`) and its creation: the workers are told to stop, `current` becomes
   `None`, the status is *Stopped*, and the workers stay in `Creates.running` until they
   end — never joined on the event loop (`stop_all` at exit is the one wait). The
   decision removes nothing a worker uses; a candidate and its scratch folder are dropped
   by whoever holds the candidate (`Creates::answered`, or the worker when it was cut
   short). Then `end_new` gives the shown tab its forms back exactly (`Tabs::bring_back`),
   and the ordinary Open moves them with the tab when the file is accepted.
4. **A late answer is nobody's.** An abandoned generation never reaches `show`, never
   rewrites the status (not to *Failed* either), never touches `Loads.current`, the
   reading's status or a newer creation. A newer New may start at once.
5. **New became a tab first.** An accepted creation settles the question (`new_ended`):
   the chosen file opens by the ordinary Open beside that tab. A later *Discard* finds
   no question. The tab is an ordinary Untitled document with the dirty prompt, Save As
   and recovery of any other. A creation whose picture the device refuses leaves New and
   the question as they were. An Open that cannot start at that moment says so and
   asks the person to choose again.
6. **Nothing comes back.** An unreadable file, a refused picture or Cancel of the reading
   leaves the shown tab with its forms; New is not recreated and no creation is started.
7. **Words.** The question and its buttons say what is lost; nothing in them is a
   generation, a lease or a private folder. The status after a stop is *"New was stopped
   to open another file; nothing was made."*
8. **Unchanged.** Tabs and alias dedup, `MAX_TABS`, the dirty prompt, Save As,
   Undo/Redo, checkpoints and crash copies; the accepted model only is saved, exported
   and recorded; §30S shared inspection and §30R descriptor; CLI, JSON, document schema,
   kernel/FFI and every geometric operation.

## Found defects

* **The question above New's form took the keyboard from the box being typed in.** A
  real-widget gate (`the_question_beside_new_keeps_its_text_and_focus_and_both_buttons_answer`)
  failed on the first layout: with the question drawn above the form, egui lost the
  keyboard focus of New's Width box the frame the question appeared (`memory.focused()`
  became `None`), because it numbers a widget by what is drawn before it. Wrapping the
  form in its own id scope did not fix it; drawing the question **under** the form did.
  Both are now one function, `ferritecad_ui::new_document_section`, which the window and
  the gate call, so the order cannot drift apart.
* **A stop over a question about a New that had ended could have opened a file unasked
  (author review, fixed before the gates).** The first `Creates::stop_new` handed the file
  back even when the question belonged to an earlier New, so a Discard reaching a later
  New #2 would have read the file while New #2 stayed open. The window cannot reach it
  (`asking()` hides such a question), but the owner now refuses it: a stop finds nothing
  to stop and `new_ended` hands the file back to be asked about again over New #2
  (`a_question_is_bound_to_the_new_it_was_asked_over`; the mutation that removes the
  generation check fails it).
* Two guards, not a gap: after a stop, `Creates::answered` refuses the late answer both
  by `current` (`accepts`) and by the status (*Stopped*, not `Running{g}`). The first
  directed mutation therefore removes the whole abandonment, not one guard.

## Not here

Leaving an unfinished New in any way but Open (Recover, Reopen, the tab row and a second
New still wait for it), several foreground jobs, drafts across a restart or crash, a draft
marker in the tab row, new geometry. §30, Milestone 5C and the product stay open; the
earlier OOM investigation is not closed by this slice.

## Verification

### What is checked, and where

**Kernel-free, every OS (`ci.yml`, step *Open, edit, Undo, Redo and Save one document
without native geometry*, exact name and marker each):**

* `creates::tests::a_question_is_bound_to_the_new_it_was_asked_over`
  (`FCAD_30T_QUESTION_GENERATION_EXECUTED`) — no New, no question; a newer choice replaces
  an older; *Back to New*; New's Cancel ends the question and hands the file back once; a
  question whose New ended unsettled does not discard the next New (literal `-` stays,
  status not *Stopped*) and is handed back.
* `creates::tests::stopping_new_ends_its_draft_and_leaves_the_worker_to_end_on_its_own`
  (`FCAD_30T_STOP_NEW_EXECUTED`) — a creation held at a barrier in three timings (still
  working; candidate made and held; answer queued): form, running state and the question
  end; `can_cancel` is false; the worker stays accounted; its scratch folder is still
  there after the decision and gone after its owner drops the candidate; the late answer
  owes no frame and leaves *Stopped*.
* `creates::tests::an_abandoned_creation_never_answers_for_a_newer_one`
  (`FCAD_30T_ABANDONED_NEVER_ANSWERS_EXECUTED`) — New again and Create again: the old
  answer arrives, the newer generation is still awaited and its status untouched; only the
  newer candidate is accepted.
* `sessions::tests::tabs::drafts::open_over_new::open_over_an_unfinished_new_asks_first_and_discards_only_when_told`
  (`FCAD_30T_OPEN_OVER_NEW_EXECUTED`) — A accepted-dirty, a literal `2..6` form and a crash
  copy; New's form with `77x`, ` 33.0 ` and an empty box; dialog Cancel/Failed (real
  `Dialogs::receive`); the question; Quit held; a second choice; *Back to New* (literal
  strings); a stale Discard; Discard then an unreadable file (A shown with its form, New
  gone, nothing created); New's drawing with draft history both ways, a vertex gesture
  holding Open, *Back*, Discard then Cancel of the reading; Discard then an accepted file:
  A hidden with its form and model, back with `2..6`, its crash copy still written, one
  active record, the input files byte for byte.
* `…::a_saved_object_form_survives_a_discarded_drawing_exactly`
  (`FCAD_30T_SAVED_FORM_SURVIVES_EXECUTED`) — A's saved-Sketch form lives in the very editor
  New's drawing uses: its text and draft Undo/Redo are set aside when New opens, the
  drawing keeps its own history through *Back*, and Discard gives A's form back exactly
  before the file is read; accepted, A is hidden with it and it is there as left, Redo
  working.
* `…::a_creation_under_way_is_abandoned_and_its_late_answer_changes_nothing`
  (`FCAD_30T_OPEN_OVER_RUNNING_NEW_EXECUTED`) — four timings (working, made, queued,
  refused); Open offered during the creation; Discard; worker accounted, scratch kept;
  the Open's slot and *Opening…* status survive the late answer; no Untitled tab; the Open
  lands beside A, which keeps its form.
* `…::a_creation_that_becomes_a_tab_first_is_never_discarded`
  (`FCAD_30T_OPEN_AFTER_NEW_BECAME_A_TAB_EXECUTED`) — a refused picture keeps New and the
  question; an accepted creation settles it: the file opens beside the new tab
  (`[A, N, B]`), N is Untitled and untouched, closing it asks; a late Discard has no
  question; a creation accepted before the file was chosen is a plain Open.
* `…::a_full_window_is_checked_before_anything_is_asked_or_discarded`
  (`FCAD_30T_OPEN_OVER_NEW_FULL_WINDOW_EXECUTED`) — 8 tabs: a ninth file and the shown
  tab's own file are said and nothing is asked; a hidden tab's file is asked about and,
  confirmed, shows that tab with no ninth one.
* `…::open_alone_leaves_an_unfinished_new_and_every_other_hold_stays`
  (`FCAD_30T_OPEN_NARROW_EXCEPTION_EXECUTED`) — over New only `can_open` is true (the tab
  row, Recover, Reopen and `may_leave_tab` are false; no second New; Create follows its own
  predicate); an Apply, a camera gesture, a copy worker and a Reopen each hold Open and ask
  nothing; a worker's answer before the decision is judged against the state then.
* `sketch::tests::open_over_new::the_question_beside_new_keeps_its_text_and_focus_and_both_buttons_answer`
  (`FCAD_30T_QUESTION_WIDGETS_EXECUTED`) — **real egui widgets, headless, not a window**:
  typing into New's box, the question appears and the box keeps the keyboard, *Back to
  New* answers and the boxes are literally as typed, typing without a click changes
  nothing and a click makes the box editable again; Discard greyed answers nothing,
  offered it answers and, after the window ends New's form, nothing is left to type into.
* `ferritecad-ui open_over_new::tests::the_question_says_what_is_lost_and_both_buttons_answer`
  — the words, both buttons, the held-back state and nothing drawn without a question.
* `…::stub_open_over_a_running_new_is_refused_at_the_picture_and_new_stays_gone`
  (`FCAD_30T_STUB_OPEN_OVER_NEW_EXECUTED`, executes only without a kernel) — the production
  `run_create` and `open_for_view` are refused at their picture: the abandoned creation's
  refusal leaves *Stopped* (not *Failed*), the Open is refused, A's form is as typed.

**No solver (`runtime-layout.yml`, OCCT without PlaneGCS):**
`…::mixed_open_over_a_running_new_with_occt_and_no_solver`
(`FCAD_30T_MIXED_OPEN_OVER_NEW_EXECUTED`) — the production creation made and held, Discard,
the production reading accepted, the late candidate dropped, A's form applied after
returning.

**Native (`runtime-layout.yml`, OCCT + PlaneGCS):**
`…::native_confirmed_open_over_a_running_new_then_late_create_and_back_to_a_match_the_command_line`
(`FCAD_30T_NATIVE_OPEN_OVER_NEW_EXECUTED volume_a=93600.000`,
`FCAD_30T_SESSION_FILES_COMPARE_OK negative_controls=7 all_SQL_cells=true`) runs the
window recipe below on the owners (production workers, no window) and then the
comparator. **Not window evidence.** The comparator: `a.fcad` equals the CLI's
`edit-extrude 26` + `edit-sketch-copy` (80 → 90) in every SQL cell but `meta.modified_at`,
its ids and references equal the input's; `a-unsaved.stl`/`.fbx` byte-equal the CLI's
exports of that copy and the saved file exports the same bytes; 90 × 40 × 26 = 93 600 mm³
by an independent STL reading; `b.fcad` byte-equal its input (never written); exactly two
`.fcad` files exist (nothing New was making reached a file); the recovery folder is
empty. Seven controls, each on a copy and each refused for its own reason: a missing
output (and no peer job ran first), an unsaved `a.fcad`, an accepted change lost (height
only), `b.fcad` written, swapped exports, a third document, a record left behind.
`native_compare_real_open_during_new_gui_artifacts_with_negative_controls` runs the same
comparator on real window outputs in `FCAD_30T_GUI_DIR`; without the variable it does
nothing.

N/A, deliberately: no new pinned-ufbx block (the exports are compared byte for byte
with the existing CLI, and `tools/check-fbx-complex.sh` is unchanged); no new corpus;
no new workflow.

### Execution record (author, macOS arm64, 2026-10-09)

Base: `main` = `origin/main` = `454f6b4e5d604e09ca8556d29305231240f32b60` (merge of PR #99,
§30S; tree `2fec44cd…` equals the reviewed PR head's), fetched fresh; the branch is
`open-during-new`, uncommitted and unstaged. **No remote CI has run for this diff.** When
the slice began the base's post-merge `CI`, `planegcs pin`, `product sbom`, `rust sbom`
and `rust notices` had succeeded and `combined runtime layout` (38004645491) was **in
progress, not counted**; it had completed with success when the work ended. `rustc
1.96.0`. Vendor OCCT and PlaneGCS were not rebuilt; no third target. No viewer, bundle,
GPU window, `osascript` or browser was run, and no `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

* **Resources.** `CARGO_BUILD_JOBS=2` for debug builds and tests, `1` for the two release
  test builds. System pressure sat at *warn* (level 2, 38–41 % free) for the whole session,
  while other applications were running, with swap constant at 986 MiB and 139–141 GiB
  of disk free. No causal memory profiling was performed.
  The warning was first seen right after the 8-second stub test build. Heavy builds were
  then deferred for 15–25 minutes (waiting, documentation); the incremental workspace
  clippy (13 s) and the two release test harnesses (about 80 s each) ran afterwards,
  serially, at `nice 10` and under a watcher that would have aborted them on +300 MiB of
  swap or critical pressure. It never fired. No application was closed.
* **Native** (`source /private/tmp/ferrite-pr93-review/env.sh`, target
  `/private/tmp/ferrite-24b-native-target`, in that sourced zsh): `cargo fmt --all -- --check`
  clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean;
  `ferritecad-app --features planegcs` **601 passed, 2 ignored** (both pre-existing
  child-process helpers) + 3 integration; `ferritecad-ui` **110 passed**. Of the 14 new app
  tests, the stub-only one and the mixed one return without executing in this build (N/A, not
  passes), as does the environment-gated GUI comparator.
* **Extracted workflow lines** (cut programmatically from the edited YAML; only
  `RUNNER_OS`/environment supplied; run from an ignored directory): the whole ci.yml step
  *Open, edit, Undo, Redo and Save one document without native geometry* in the true-stub
  environment exits 0 with 237 `test result: ok` lines, 0 `skipped:` and all eleven
  `FCAD_30T_*` markers; the §30T native block of runtime-layout (release, `--features
  planegcs`) exits 0 with `FCAD_30T_NATIVE_OPEN_OVER_NEW_EXECUTED volume_a=93600.000` and
  `FCAD_30T_SESSION_FILES_COMPARE_OK negative_controls=7 all_SQL_cells=true`; the §30T
  no-solver block (release, no `planegcs` feature, `FERRITECAD_REQUIRE_PLANEGCS=0`, no
  `FCAD_PLANEGCS_DIR`, OCCT libraries only on `DYLD_LIBRARY_PATH`) exits 0 with
  `FCAD_30T_MIXED_OPEN_OVER_NEW_EXECUTED`. The rest of those two steps belongs to earlier
  slices and was not rerun.
* **True stub** (target `/private/tmp/ferrite-25j-stub-target`, clean environment: no
  `DYLD_*`, no `FCAD_PLANEGCS_DIR`, `OpenCASCADE_DIR` and `CMAKE_PREFIX_PATH` =
  `/private/tmp/absent-occt-true-stub`, `FERRITECAD_REQUIRE_OCCT=0`,
  `FERRITECAD_REQUIRE_PLANEGCS=0`): the build used bridge directory
  `ferritecad-occt-bd6e1e14777ad9eb`, whose cache holds
  `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE` and which warned *"Open CASCADE was not
  usable … configuring the bridge failed"*; `otool -L` of the app test binary shows no
  `libTK*` and no PlaneGCS. The app suite: 601 passed, 2 ignored + 3, ui 110; the 196
  `skipped:` lines are the native-only gates of earlier slices, N/A here. Stub `cargo clippy
  --workspace --all-targets -- -D warnings` clean.
* `tools/check-export-boundary.sh` passes; `tools/check-licence-headers.sh`: 470 files, all
  MIT; `actionlint` on `ci.yml` and `runtime-layout.yml` clean; `git diff --check` clean and
  no trailing whitespace in the new files. No shell script changed.
* `tools/open-during-new-gui.py`: a root inside the checkout is refused
  (`FCAD_30T_GUI_FIXTURE_REFUSED`, nothing made); a root outside made only the inputs
  (`a.fcad`, `b.fcad`, `inputs/`, `facts.json`, an empty `recovery/`); `--compare` refused
  before any build on missing outputs and, with two empty placeholder files, on an unsaved
  `a.fcad` (`FCAD_30T_GUI_COMPARE_REFUSED`). The temporary root was removed; no positive
  artifact was made.

**Directed mutations** (native debug, on the final formatted source; each compiles and
fails executed assertions; `creates.rs` `9ea2ac0a2737…` and `main.rs` `b0290a2371f6…`
byte for byte the same before and after, then the whole suites passed again):

* **M1 — a stop abandons nothing.** `Creates::stop_new` ends the form and the drawing but
  neither stops the worker, nor forgets `current`, nor says *Stopped*. Five gates fail,
  among them the native scenario at *"the late candidate was shown"* — the late Create is
  accepted again.
* **M2a — Discard erases the shown tab's height form** (`edits.cancel()` after `end_new`).
  Four gates fail on A's literal `2..6` (`left: None`).
* **M2b — Discard erases the shown tab's saved-object form** (`sketch.dismiss()` after
  `end_new`). `a_saved_object_form_survives_a_discarded_drawing_exactly` fails.
* **M3 — the question is not bound to its New** (`in_progress` ignores the generation).
  `a_question_is_bound_to_the_new_it_was_asked_over` fails at *"New #2 survives a stale
  decision"*.

An earlier M1 on a first formatted source and an earlier M2 gave the same results; they were
repeated here on the final bytes.

### Real window recipe (macOS)

Not run by the author. One freshly staged arm64 bundle `APP`; run from the checkout in the
native environment (`source <env.sh>`: `--compare` runs the Rust comparator). One viewer
under the 1536 MiB watchdog; a new root outside any checkout; the recovery and Tabs
folders are the root's own — the person's own folders are never touched. Never call
`getApp`/`getAX` after Quit; check that the owned PID is gone. Use only the inputs the
generator makes.

```sh
ROOT=/private/tmp/ferrite-30t-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 tools/open-during-new-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery" FERRITECAD_TABS_DIR="$ROOT/tabs"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/a.fcad"
```

The generator writes only the inputs: `a.fcad` (CLI sample 80×40×12), `b.fcad` (CLI circle,
centre 12.5, −7.25, r 10.5, h 15.25), pristine copies in `inputs/`, an empty `recovery/`
and `facts.json`. It refuses a root inside a checkout and makes no window output.

1. `a.fcad` opens. **Edit Sketch…** → replace both `80` with `90` → **Apply vertices**
   (`*a.fcad`).
2. **Edit extrusion…** → choose the extrusion → type `2..6`. Do not Apply.
3. **New** → **Sample plate** → Width `50x`, Depth ` 30 `, Height `7`. Do not press
   **Create document**. **Open…** is enabled.
4. **Open…**, then **Cancel** in the system dialog: nothing changes; no question.
5. **Open…** → `$ROOT/b.fcad`: *New is not finished* appears under New's form, naming
   `b.fcad` and the loss. Press **Back to New**: the question is gone and the three boxes
   still read `50x`, ` 30 `, `7`.
6. **Open…** → `b.fcad` → **Discard New and open**: New's form is gone; `b.fcad` opens as a
   second tab with no form; there is no Untitled tab.
7. Press `*a.fcad`: its form shows `2..6`; replace it with `26` → **Apply**. **Export STL…**
   → `$ROOT/a-unsaved.stl`; **Export FBX…** → `$ROOT/a-unsaved.fbx`. **Save** (`Cmd+S`).
8. Quit (`Cmd+Q`): nothing is asked; the window ends; check the owned PID is gone.

```sh
python3 tools/open-during-new-gui.py --compare "$ROOT"
```

`--compare` refuses (`FCAD_30T_GUI_COMPARE_REFUSED`) before any build or peer job when an
output is missing or `a.fcad` was never saved; the Rust comparator refuses a missing output
again before its first peer job. It reads only the window's outputs and requires
`FCAD_30T_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true`.

The race with a creation still running is **not** part of this recipe: an ordinary New is
too quick for a person to catch. It is proved by the deterministic owner tests above, at a
barrier, in each timing; no manual smoke is claimed to have caught it.

### Limits

* No real window was run by the author; the owners' scenario and the headless widget test
  are not window evidence. Independent macOS window evidence is recorded below.
* Windows/Linux window interaction is untested; headless transition and widget tests are
  separate evidence.
* The base's post-merge `combined runtime layout` run was still in progress when this
  slice began (not counted as passed at that point). The implementation handoff had no
  remote CI for its diff; independent publication and checks are recorded below.

## Independent review — PR #100

Base `454f6b4e5d604e09ca8556d29305231240f32b60` was fetched again; its six
post-merge workflows finished successfully. The reviewer read the production
transition/worker changes and the owner/widget tests. Two initial documentation
corrections distinguish the seven comparator controls from six, and observed
memory pressure from an unproven attribution to other applications.

GitHub rejected the first publication (`4a231daa`, runtime run `38012900266`)
before starting any jobs: `Exceeded max expression length 21000`, at the session
step's `run` block. That block was 21,224 characters; local `actionlint` had passed.
Review moved the §30T native gate, byte for byte, into an adjacent step with the
same OCCT/PlaneGCS requirements and platform library paths. All previous session
commands remain byte for byte unchanged in their old step, now 20,118 characters.
All literal `run` blocks in the workflow were checked against GitHub's limit;
`actionlint` and whitespace checks pass. No gate or execution marker was removed.
The corrected code/workflow revision is `430e3ba10fcba85acd227856a2222a33e45dc6fd`.

Local review started under system memory pressure 2 with 986.38 MiB swap in use;
no viewer was launched then. At pressure 1 the fresh release CLI/viewer build
passed (one Cargo job), and a relocatable arm64 bundle was staged with a working
bundled solver. During compilation of the app test harness pressure rose to 2
again; the reviewer interrupted only that owned compilation (exit 130). This is
an interrupted check, not a failed assertion or a passed test. Swap at that point
was 978.38 MiB. Local fmt, licence headers (474 files), and export boundary checks
passed. The window inputs were generated by the fresh bundled CLI; no positive
window outputs were generated.

The user then rebooted macOS. The old `/private/tmp/ferrite-*` build targets,
review environment and unpublished evidence disappeared; the checkout and pinned
libraries in `vendor/` remained. Review reused the repository's existing `target/`
with one Cargo job and the same pinned OCCT/PlaneGCS. It did not rebuild either
library or assume the deleted targets still existed. Fresh local logs and the
native environment are in ignored `target/30t-review/`.

The rebuilt release CLI and app test harness passed: `creates::` 21 harness
passes, `sessions::tests::tabs::drafts::` 31, the new app real-widget gate 1, and
the UI question gate 1. These totals include deliberately inactive stub/mixed and
GUI-artifact entry points; they are not all native executions. The actual native
Open-over-running-New scenario emitted both `FCAD_30T_NATIVE_OPEN_OVER_NEW_EXECUTED
volume_a=93600.000` and `FCAD_30T_SESSION_FILES_COMPARE_OK negative_controls=7
all_SQL_cells=true`. The GUI artifact gate was separately executed after the
real window actions below. Local stub/mixed rebuilds and local workspace clippy
were not repeated by the reviewer; their remote evidence is recorded separately.

### Independent macOS window, 2026-10-09 PDT

The viewer was the macOS release archive from the successful macOS job of run
`38013229193`, artifact `ferritecad-release-macos` (11656097516). Its manifest
names source revision `430e3ba10fcba85acd227856a2222a33e45dc6fd` and
`aarch64-apple-darwin`. Deep/strict ad-hoc signature verification and the bundled
`--solver-info` passed. This is the code being reviewed, not an older staged app.
Its own CLI generated fresh inputs at `/private/tmp/ferrite-30t-window-review`,
with private recovery and last-tabs folders. Positive outputs came from the
window only.

One viewer, PID 6392, was launched under the 1536 MiB watchdog. The reviewer:

1. Opened A, changed its two right vertices from x=80 to x=90 with Apply, then
   left the height form at literal `2..6`.
2. Opened New sample plate and typed width `50x`, depth ` 30 ` and height `7`.
   Open's native dialog Cancel kept that form without a question.
3. Chose B in the native Open dialog. The question appeared below New. **Back
   to New** kept its values. Chose B again and **Discard New and open**: B became
   the second tab, with no new/Untitled document.
4. Returned to A: the same height form still held `2..6`. Changed it to `26`,
   pressed Apply, exported `a-unsaved.stl` and `a-unsaved.fbx` through the native
   Save dialogs, then saved A. Its title became clean; B stayed open.
5. Quit normally. Exit was checked by the owned process/watchdog, with no CUA
   query after Quit that could launch another viewer.

The post-window comparator executed once and emitted
`FCAD_30T_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true`. It compared
actual window SQL/identities and exports with the peer CLI, checked B and the
absence of an extra document, and rejected all seven directed corruptions.
A is 90 × 40 × 26 mm, volume 93600 mm³. Its STL has 12 triangles/684 bytes;
FBX has 4103 bytes. Both exports equal the CLI bytes. The digest-checked pinned
ufbx 0.23.0 reader was rebuilt and read the actual window FBX in strict triangle
mode: one instance, 12 triangles, 6 checks, 0 failures.

The watchdog recorded 638 samples over 345.67 seconds: peak physical footprint
208.1574 MiB, pressure 1 throughout, swap 0 throughout, exit 0, `aborted=false`.
Logs and a copy of the window artifacts are retained in ignored
`target/30t-review/`. This window did not manually catch a running-Create race;
the deterministic barrier/owner tests remain its evidence. Windows/Linux GUI
and the historical OOM cause remain unverified.

### Remote review checkpoint — 2026-10-10 02:48 UTC

The corrected code/workflow head has CI `38013232100` success (7 jobs) and
planegcs pin `38013291706` success (4 jobs). The reviewer downloaded the ordinary
Linux/macOS/Windows test logs and checked all eleven §30T execution markers and
the exact UI question test, not just job colour. Combined runtime `38013229193`
was awaiting Windows and comparison at this documentation checkpoint;
Linux/macOS had passed with the native and mixed execution markers and the
seven-control SQL comparator. This checkpoint alone does not claim a completed
three-platform runtime pass. The final status and remaining log audit are part
of the merge decision in [PR #100](https://github.com/gesriot/ferrite-cad/pull/100).
All three runs target the code/workflow head above; this documentation commit
changes no executable or workflow inputs.

Exact runs: [CI](https://github.com/gesriot/ferrite-cad/actions/runs/38013232100),
[runtime layout](https://github.com/gesriot/ferrite-cad/actions/runs/38013229193),
[planegcs pin](https://github.com/gesriot/ferrite-cad/actions/runs/38013291706).
