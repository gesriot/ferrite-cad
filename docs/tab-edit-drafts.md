# §30P — each tab keeps its unfinished edit form

Markers `FCAD_30P_*`. Builds on [§30O document tabs](document-tabs.md) and
[ADR 0005](decisions/0005-document-session.md). This one file is the contract and the
verification record of the slice.

## What a person can do

Open A and B. In A, open **Edit extrusion…**, type a height — even one that is not a
number yet, such as `2..6` — and press B's tab without Apply or Cancel. B shows its own
document with none of A's form. Open B's circle (or constraints, or any other form
below), type, and press A's tab: A's form is back exactly as it was left — the same
text, the same picks, the same draft Undo/Redo. Apply, Cancel and every other button of
a form act on the tab the form belongs to. Switching is never Apply, Cancel or Save.

## The owners (completeness check)

The draft is not a new protocol per form: the window's two existing form owners are
moved whole, as values, in the statement that hides or shows a tab.

| Form | Real owner in the window | What it keeps | Its foreground work |
|---|---|---|---|
| Extrude height | `edits::Edits.form` (`edits::Form`) | chosen extrusion, height text, refusal, the reading it was opened on | Apply: the tab's `Sessions` operation; *Save new file…*: `Edits.running` |
| Saved Sketch vertices | `sketch::Editor` (`draft`, `undo`/`redo`, `next`, `canvas`, `editing`) | every coordinate as typed, closed flag, draft history, canvas pick/zoom/snap | Apply: `Sessions`; a vertex drag: `Canvas.gesture` |
| Saved Circle / Annulus | `sketch::Editor` (`circle`/`annulus`, `editing_*`, `*_undo`/`*_redo`/`*_applied`) | centre and radii as typed, draft history | `Sessions` |
| Revolve angle | `sketch::Editor` (`angle_edit`, `editing_angle`, `angle_*`) | angle text, draft history | `Sessions` |
| Constraints | `sketch::Editor.constraints` (`constraints::Editor.draft`) | pending removals and additions (stored ids), picked Line and pair, typed length/radius/point, request Undo/Redo | `Sessions` |
| Cut Add / Edit | `sketch::Editor.cuts` | numbers as typed, picks, draft history | `Sessions` |
| Fillet Add / Edit | `sketch::Editor.fillets` | picked edges, radius text, draft history | `Sessions` |
| Chamfer Add / Edit | `sketch::Editor.chamfers` | picked edges, distance text, request history | `Sessions` |
| **New** form | `creates::Creates.form` | — not a tab's; holds the window | `Creates.running` |
| New drawing (polygon, circle, annulus) | `sketch::Editor.draft` with no `editing` | — not a tab's; holds the window | `Creates.running` |
| STL options / export question | `exports::Exports` | — holds the window | export worker |

## Rules

1. **One owner per tab.** The shown tab's forms are the window's `Edits` form and
   `sketch::Editor`; a hidden tab keeps them as a `tabs::Draft` in its `Hidden` entry,
   keyed by its runtime `TabId` — never a position, a path or the `DocumentId` (two
   copies of one file have the same ids and two independent drafts). Nothing is
   serialised, re-read or rebuilt from text: `Tabs::bind` moves the very values
   (`Forms::park` / `Forms::restore`). Only idle forms move; a worker never leaves the
   window, and a hidden form holds no worker and no extra file — its version is the
   session's own current snapshot (`Draft::base`, one `Arc`).
2. **One statement.** A switch is accepted where §30O accepts it: after the target's
   picture is prepared, `Tabs::activate` makes the target active, parks the left tab's
   forms with its camera, and gives back the target's forms. A refused picture, a
   kernel refusal, Cancel, a stale or foreign answer never reach it: both drafts stay
   where they were. §30O's fixes are kept: Cancel retires the switch at once, its input
   lease lives until its worker ends, Quit still refuses a shown form or running work.
   `present` ends forms about a replaced picture after an Apply, Undo, Redo or Open
   arrival, as before, but not after a switch (the forms are that tab's own).
3. **What holds the window.** One predicate, `can_leave_tab`, answers for the tab row
   and for the handlers: showing or closing another tab waits for any session operation
   (Apply, Add, Undo, Redo, Save, Save As, a checkpoint, a switch, Recover), a load, an
   export or its question, the STL options form, **New** (its form, drawing or worker),
   a *Save new file…* copy, and a pointer gesture (camera drag or vertex drag). Native
   dialogs are modal and run before the window draws again. An idle form does not hold
   the window. A document that is clean is no proof that no form is open.
4. **Text is kept literally.** `''`, `-`, `33.0`, ` 33.0 `, `1e999x`, `2..6` come back as
   typed; switching parses and normalises nothing. Picks, pending removals/additions and
   each form's draft history stay in their tab. Each tab's forms are drawn under ids of
   that tab's own (`in_tab_scope`, with explicit scoped IDs for floating `form_window`s), so keyboard focus, cursor or selection in one tab's
   field never edits the field of the same name in another tab.
5. **Bound to its version.** A draft knows the accepted version it was made on (the
   snapshot's identity, not its content). Shown again on the same version, nothing is
   re-opened. Apply goes through the tab's existing session route and checks, and an
   accepted Apply ends the form as before. A hidden tab's version cannot change through
   the window; if it ever does, the draft comes back *held*: its text stays readable,
   Cancel works, and no Apply or Add of it is offered or started
   (`Sessions::takes_form_apply`, in every form's availability) until it is closed.
6. **Save is the accepted model.** Save, Save As, exports, checkpoints, crash copies
   and document Undo/Redo never read a form. Drafts are not kept across a crash or a
   restart.
7. **Close and Quit never lose a form.** × on a tab with a form — hidden (shown first)
   or shown, clean or not — leaves the form for the person to Apply or Cancel: *"This
   tab has an open form. Apply or Cancel it before closing the tab; nothing was
   closed."* Quit refuses while the shown tab has a form (as before); its pass also
   shows each hidden tab that keeps a form (clean or not) and stops there with the same
   words for quitting. Nothing is closed, saved or applied by that stop.
8. **Limits.** Still 8 tabs and one GPU scene. A hidden draft holds its form values and
   one lease of its tab's current version; a hundred switches copy no history and make
   no file. Closing a tab (only possible once its form is done) frees only its own
   files and crash copy. CLI, document schema, capabilities and JSON v1 are unchanged.

**Still blocking.** A **New** form or drawing, an export in progress (or its question,
or the STL options form), any native dialog, any operation and any gesture. Open, New
and Recover also still wait while the shown tab has a form, as before §30P.

## Not here

Drafts that survive a restart or crash, automatic Apply, several GPU scenes, work in
hidden tabs, a draft marker in the tab row or any other UI change, Open/New/Recover
over an open form. §30, Milestone 5C and the product stay open; the earlier OOM
investigation is not closed by this slice.

## Verification

### What is checked, and where

**Kernel-free, every OS (`ci.yml`, step *Open, edit, Undo, Redo and Save one document
without native geometry*, each test by exact name with its marker):** the window's own
owners (`Tabs`, each tab's `Sessions`, `Edits`, `sketch::Editor` with
`constraints::Editor`, `present`, the availability predicates) on the mock kernel, A→B→A:

* `height_drafts_of_two_copies_stay_literal_and_apply_only_to_their_own_tab` — two copies
  with one `DocumentId` and one feature id; `-`, `''`, `1e999x`, ` 33.0 `, `-0` against
  B's `33.0`; no version made and nothing dirty until an accepted Apply; Apply after
  returning reaches its own tab only; Save writes the accepted model, not a form.
* `constraint_drafts_with_one_set_of_ids_keep_picks_changes_and_history_per_tab` — the
  same Sketch and Line ids in both tabs, different pending additions and picks;
  request Undo in A leaves B alone, Redo comes back; 100 switches: one lease, no files.
* `vertex_drafts_of_two_copies_keep_their_own_text_and_draft_history` — draft Undo/Redo.
* `running_work_holds_the_window_and_failed_or_foreign_answers_keep_both_drafts` — New
  form, New drawing, copy worker, camera drag, vertex drag, Apply; refused upload, kernel
  refusal, Cancel, stale switch; an Apply answer addressed to A arriving in B.
* `close_and_quit_keep_an_open_form_even_over_a_clean_model` — Close shows, then
  refuses; Quit stops at a hidden form after a Discard; Cancel, then Close frees A only.
* `a_draft_made_on_another_version_comes_back_held_and_is_never_applied`.
* `the_same_control_in_two_tabs_shares_no_focus_or_text` — real egui focus and typing.
* `stub_a_refused_switch_keeps_both_drafts` (`FCAD_30P_STUB_DRAFTS_EXECUTED`, executes
  only without a kernel).

**Native (`runtime-layout.yml`, OCCT + PlaneGCS):**
`native_drafts_of_two_models_apply_after_returning_and_save_like_the_command_line` — A
(an offset Line-Sketch plate): height `9.5x` kept, `9.5` applied after returning; its
polygon (one vertex 33 → 41.25) kept across a switch and applied; B (CLI circle): circle
form kept and applied after returning; A's Undo/Redo; unsaved exports byte-equal to the
CLI's (`edit-extrude` + `edit-sketch-copy`; `edit-circle`); Save; every SQL cell equal to
the CLI copy except `meta.modified_at`; equal `model_version`; every UUID and reference
kept; exports of the saved files equal the unsaved ones; STL geometry read independently
(A: height 9.5, shoelace area × 9.5; B: closed cylinder r 8 at (−3.5, 4.25), h 15.25).
Artifacts `drafts-a`/`drafts-b` go to the pinned ufbx block of
`tools/check-fbx-complex.sh` (`FCAD_TAB_DRAFTS_UFBX_EXECUTED`).
`native_replace_length_drafts_of_two_copies_apply_to_their_own_tab_like_the_command_line` —
one pending removal and one addition over the same stored ids in two copies; each
applied in its own tab and compared with `edit-sketch-constraints-copy` by the existing
explicit-new-id comparison (the new constraint UUID is random; nothing else is set aside).
`native_tab_drafts_scenario_on_session_files_passes_the_comparator_and_its_controls`
(`FCAD_30P_SESSION_FILES_COMPARE_OK negative_controls=6 all_SQL_cells=true`) is the
comparator's self-check on files the window owners produce without a window — **not
window evidence**.

**No solver (`runtime-layout.yml`, OCCT without PlaneGCS):**
`mixed_drafts_switch_and_apply_with_occt_and_no_solver` (`FCAD_30P_MIXED_DRAFTS_EXECUTED`).

### Execution record (author, macOS arm64, 2026-10-09)

Base: `main` = `origin/main` = `764d551633b4f92eb8f2c1a2fdfb2d3970c84b43` (PR #95 merged,
reviewed head `ef414a1`); its six post-merge workflows (CI, planegcs pin, combined
runtime layout, rust notices, rust sbom, product sbom) had all completed with success
when this slice began. Work on branch `tab-edit-drafts`, uncommitted; **no remote CI has
run for this diff.** `rustc 1.96.0`, CMake 4.3.3, `CARGO_BUILD_JOBS=2`. Vendor OCCT and
PlaneGCS were not rebuilt. No viewer, bundle, GPU, `osascript` or browser was run.

* **Native** (`source /private/tmp/ferrite-pr93-review/env.sh`, target
  `/private/tmp/ferrite-24b-native-target`, commands in the sourced zsh): `cargo fmt --all
  -- --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` clean; `tools/check-export-boundary.sh` passes; `ferritecad-app` suite
  (`--features planegcs`) **563 passed, 1 ignored** (pre-existing) + 3 integration. In
  that run the stub-only and no-solver-only tests and the env-gated real-window
  comparators return without executing: N/A there, not passes.
* **Runtime-layout blocks, exact lines** (extracted from the edited YAML, only
  `RUNNER_TEMP`/`GITHUB_ENV` and the matrix name supplied; release peer CLI rebuilt
  first): the §30O+§30P native loop exit 0 with `FCAD_30O_NATIVE_TABS_EXECUTED`,
  `FCAD_30O_SESSION_FILES_COMPARE_OK …`, `FCAD_30P_NATIVE_DRAFTS_EXECUTED
  volume_a=5324.156 height_b=15.250 volume_b=3063.648`,
  `FCAD_30P_NATIVE_CONSTRAINT_DRAFTS_EXECUTED`, `FCAD_30P_SESSION_FILES_COMPARE_OK
  negative_controls=6 all_SQL_cells=true`, eight artifacts present; the two no-solver
  blocks (`FERRITECAD_REQUIRE_PLANEGCS=0`, no `planegcs` feature) exit 0 with
  `FCAD_30O_MIXED_TABS_EXECUTED` and `FCAD_30P_MIXED_DRAFTS_EXECUTED`.
* **Pinned ufbx:** the §30P block of `tools/check-fbx-complex.sh`, extracted with its
  preamble and the reader build unchanged (ufbx `fcc5d6b…`, strict), on those artifacts:
  `checks=6 failures=0` for `drafts-a` and `drafts-b`, STL↔FBX joins of 12 and 352
  triangles, `FCAD_TAB_DRAFTS_UFBX_EXECUTED`. The script's complex-STEP part was not
  rerun.
* **True stub** (target `/private/tmp/ferrite-25j-stub-target`, no `DYLD_*`, no
  `FCAD_PLANEGCS_DIR`, `OpenCASCADE_DIR=/private/tmp/absent-occt-true-stub`,
  `FERRITECAD_REQUIRE_OCCT=0`, `FERRITECAD_REQUIRE_PLANEGCS=0`). The first invocation
  with this toolchain made a new bridge directory (`ferritecad-occt-db51f87441ed7696`),
  whose fresh configure found the Homebrew OCCT 7.9 under `/opt/homebrew`; that run
  printed the OCCT-only markers and **was discarded as stub evidence**. Its cache was
  then reconfigured with `-DCMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE` (the method the
  older stub cache used) and the build script re-run by adding
  `CMAKE_PREFIX_PATH=/private/tmp/absent-occt-true-stub` (kept for every later stub
  command). Proof for every stub result below: the cache in use holds the flag, the build
  warned *"Open CASCADE was not usable … configuring the bridge failed"* (`No Open
  CASCADE libraries resolved`), and `otool -L` of the app test binary and of the twelve most recent
  `ferritecad-jobs`/`ferritecad-cli`/`ferritecad-ui`/`ferritecad-document` test binaries
  shows no `libTK*` and no PlaneGCS.
  * The exact `run:` block of the edited `ci.yml` step, extracted and run with
    `RUNNER_OS=macOS`: exit 0, 196 `test result: ok` lines, 0 `skipped:`, all eight §30P
    markers (`HEIGHT`, `CONSTRAINT`, `VERTEX`, `REFUSED_SWITCHES`, `CLOSE_QUIT_FORMS`,
    `STALE_DRAFT`, `FORM_FOCUS`, `STUB_DRAFTS`) with the §30M/§30N/§30O ones. The first
    attempt stopped at `RUNNER_OS: unbound variable` (a runner variable, not a defect).
  * Stub `ferritecad-app` suite: 563 passed, 1 ignored; its 193 `skipped:` lines are the
    native-only gates returning early in a build with no kernel — N/A, not passes.
  * Stub `cargo clippy --workspace --all-targets -- -D warnings`: clean.
* `tools/check-licence-headers.sh`: 459 files, all MIT. `actionlint` (with shellcheck)
  on `ci.yml` and `runtime-layout.yml` and `shellcheck tools/check-fbx-complex.sh`: clean.
  `git diff --check`: clean.
* `tools/tab-drafts-gui.py`: a root inside the checkout is refused
  (`FCAD_30P_GUI_FIXTURE_REFUSED`, nothing made); a root outside makes only the inputs;
  `--compare` on it refuses before any build (`FCAD_30P_GUI_COMPARE_REFUSED: missing
  real GUI output …`).

**Directed mutations** (native debug, run on the final formatted source; both compile
and fail an executed assertion; `tabs.rs` restored byte for byte, SHA-256
`498af693…c4c3` before and after, then the whole app suite passed again):

* **M1 — a tab loses its form at a switch.** `Tabs::activate` drops the target's draft
  instead of restoring it. `height_drafts_of_two_copies_…` fails on returning to A:
  `left: None, right: Some((Some(…), "-"))`.
* **M2 — a form goes to the other tab's session.** `activate` parks the target's draft
  under the tab being left and gives the left tab's form to the target.
  `height_drafts_of_two_copies_…` fails: *"A's form came along to B"* (`left: Some((…,
  "-")), right: None`) — B's Apply would have applied A's text to B.

A preliminary probe (not one of the two) removed the tab key from `in_tab_scope`: the
focus test failed with B's height `1299` instead of `12`; restored by SHA-256.

Logs are in the author's session scratch directory, not in the repository.

### Real window recipe (macOS)

Not run by the author: no viewer, GUI, `osascript` or browser was started for this
slice. One freshly staged arm64 bundle `APP`; one viewer under the 1536 MiB watchdog; a
new root outside any checkout; the recovery folder is the root's own. Never call
`getApp`/`getAX` after Quit; check that the owned PID is gone.

```sh
ROOT=/private/tmp/ferrite-30p-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 tools/tab-drafts-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/a.fcad"
```

The generator writes only `a.fcad` (CLI sample 80×40×12), `b.fcad` (CLI circle, centre
12.5, −7.25, r 10.5, h 15.25), their pristine copies in `inputs/`, `facts.json` and the
empty `recovery/`; it refuses a root inside a checkout and makes no window output.

1. `a.fcad` opens. **Open…** `$ROOT/b.fcad`: a second tab, shown.
2. Press `a.fcad`. **Edit extrusion…** → choose the extrusion → type `2..6` (the form
   says why it is not a height). Do not Apply.
3. Press `b.fcad` (allowed with A's form open). No height form is shown. **Edit circle
   …** → centre −3.5, 4.25, radius 8. Do not Apply.
4. Press `a.fcad`: the form shows `2..6` exactly. Replace it with `26` → **Apply**.
5. **Edit Sketch Profile …** → replace both `80` with `90`. Do not Apply.
6. Press `*b.fcad`: the circle form shows −3.5, 4.25, 8 → **Apply circle**.
7. Press `*a.fcad`: both `90` are there → **Apply vertices**. **Undo**, **Redo**.
   **Export STL…** → `$ROOT/a-unsaved.stl`; **Export FBX…** → `$ROOT/a-unsaved.fbx`.
8. **Edit extrusion…** → type `30`. Do not Apply.
9. Press `*b.fcad`. Export `$ROOT/b-unsaved.stl` and `.fbx`.
10. Quit (`Cmd+Q`): asked about `b.fcad` → **Save**. `*a.fcad` is shown with its form
    showing `30`, and the window says *Apply or Cancel it before quitting*; the window
    stays, two tabs, nothing applied.
11. **Cancel** A's form. Quit: asked about `a.fcad` → **Save**. The window ends; check
    the owned PID is gone.

```sh
python3 tools/tab-drafts-gui.py --compare "$ROOT"
```

`--compare` refuses (`FCAD_30P_GUI_COMPARE_REFUSED`) before any build or peer job when
an output is missing or a document was never saved; the Rust comparator refuses a
missing output again before its first peer job. It reads only the window's outputs and
requires `FCAD_30P_GUI_COMPARE_OK negative_controls=6 all_SQL_cells=true`: `a.fcad`
equals the CLI's `edit-extrude 26` + `edit-sketch-copy` (80 → 90) copy and `b.fcad` the
CLI's `edit-circle` copy in every SQL cell but the write stamp; UUIDs and references
kept; the four unsaved exports byte-equal to the CLI's exports of those copies, and the
saved files export the same bytes; A 26 high, 93 600 mm³; B a closed cylinder r 8; no
recovery record left. Controls (on copies of the real outputs): a missing output (no peer
job may run first), an unsaved `b.fcad`, `a.fcad` holding B's model, `a.fcad` with the
form left open at Quit applied (30), swapped exports, a recovery record left behind.

### Limits

* No real window was run by the author; the comparator's session-file self-check and
  the headless widget test are not window evidence.
* The window's per-frame wiring (`forms_scope`, the release of a held draft once its
  form is closed, the tab row's availability) is exercised only through the functions
  it calls (`in_tab_scope`, `can_leave_tab`, `Sessions`), not by a drawn window.
* Open, New and Recover still wait while the shown tab has a form; the tab row shows no
  mark for a hidden tab that keeps one.
* The *Save new file…* status line under the height form is the window's, not a tab's.

## Independent review (2026-10-09, in progress)

The original `push_id` scoped embedded controls, but egui floating `Window`s derive
their IDs from their titles and do not inherit the parent Ui scope. A real Circle
widget regression reproduced focus crossing tabs: typing in A, then showing B and
typing without selecting B's field changed its centre from `12` to `1299`.
`form_window` now explicitly includes the parent Ui ID for all saved Sketch, Circle,
Annulus, angle, constraints, Cut, Fillet and Chamfer windows. The same executable
regression passes and is required by exact name and marker in the existing CI step.
The initial test-harness attempt dropped an unhandled `TexturesDelta`; that harness
error was corrected before the failing assertion above was obtained.

Review logs: `/private/tmp/ferrite-30p-review/`. Remote CI and the real window recipe
are still pending at this point; headless results do not claim a window pass.
