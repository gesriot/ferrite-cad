# §30O verification — several documents in tabs of one window

Contract: [document-tabs.md](document-tabs.md). Decision:
[ADR 0005, §30O](decisions/0005-document-session.md#30o-several-documents-in-tabs-of-one-window).

## Base

`main` = `origin/main` = `8aa82ff1375fedd0f738364c0802b41968ffe903` (PR #94 merged), tree
`aca2c5a14f3bf1e1a537026eb23b8659f1a1bbc3`, equal to the reviewed PR head `db9e570`. PR CI:
code head `3231762` CI 7/7 + planegcs pin 4/4 + combined runtime layout 4/4 (15/15), docs
head `db9e570` CI 7/7. Post-merge CI of `8aa82ff` at the time of writing: CI 7/7, planegcs
pin 4/4, rust notices 4/4, rust sbom 4/4, product sbom 4/4; combined runtime layout
still running (linux, macos success; windows in progress) — not counted as passed.

## What is checked, and where

**Kernel-free, every OS (`ci.yml`, step *Open, edit, Undo, Redo and Save one document
without native geometry*).** The window's own owners — `Tabs`, each tab's `Sessions`,
the arrival statement `present` that `App::show` runs, `close_shown`, the edit, save,
checkpoint and picture workers — with pictures built from the documents by the scene
builder on the mock kernel and a stand-in for the GPU upload only:

* `two_copies_of_one_document_are_two_tabs_with_their_own_history_saves_and_checkpoints` —
  two physical copies with one `DocumentId`; Apply/Undo/Redo, a checkpoint and Save in
  each stay in its own tab; per-tab camera; selection reset on switch; each file
  written only by its own Save.
* `opening_a_file_open_by_any_name_shows_its_tab_and_copies_are_new_tabs` — path,
  `./` spelling, symlink (Unix), hard link; a copy is a new tab; a duplicate reaching
  the bind is refused and its private files go.
* `save_as_onto_another_tabs_file_is_refused_and_the_existing_guards_still_hold` —
  including another tab's file deleted from disk; occupied paths still refused by Save.
* `closing_asks_only_about_unsaved_tabs_and_cancel_or_a_failed_save_keeps_the_tab` —
  Now/Show/Ask, Cancel, a failed Save As, Discard, the last tab → empty window, a
  device refusing the empty picture keeps the tab.
* `quit_asks_every_unsaved_tab_in_turn_and_cancel_part_way_closes_nothing` — shown tab
  first, Save As of Untitled, the next unsaved tab shown before it is asked, Cancel part
  way (nothing closed, saved file stays saved, Discard forgotten), the second pass to
  Exit retires every crash copy.
* `each_tab_keeps_its_own_files_and_crash_copy_and_closing_frees_only_its_own` — two held
  records, closing one retires only its own, Recover opens a separate tab and does not
  replace an unsaved one, an undecided exit keeps both copies.
* `the_ninth_document_is_refused_before_anything_changes`.
* `late_cancelled_or_foreign_answers_change_no_tab_and_release_no_slot`
  (`FCAD_30O_LATE_ANSWERS_EXECUTED`) — a cancelled switch's late picture, a newer switch,
  upload/kernel/Cancel refusals, and an Apply answer addressed to another tab with the
  same generation number.
* `stub_switching_is_refused_at_the_picture_and_keeps_the_shown_tab`
  (`FCAD_30O_STUB_TABS_EXECUTED`, executes only without a kernel).
* `ferritecad-jobs --test recovery`: `lanes_of_one_worker_keep_and_end_only_their_own_records`
  and `a_killed_window_with_two_tabs_leaves_both_dirty_documents_recoverable` (a real
  child process with two dirty documents on two lanes, killed by its own PID;
  `FCAD_30O_TWO_TAB_CRASH_EXECUTED`).
* `ferritecad-ui`: `tabs::tests::tabs_show_names_marks_and_answer_with_their_key_only_when_available`.

**Native (`runtime-layout.yml`, OCCT + PlaneGCS):**
`native_two_tabs_alternate_applies_undo_restore_exports_and_saves_like_the_command_line`
(`FCAD_30O_NATIVE_TABS_EXECUTED`): plate A (80×40×12, library create) and circle B
(`create-circle-extrude`, centre (12.5, −7.25), r 10.5, h 15.25, shipped CLI) in two tabs;
checkpoint `A at 12`, A 20, B circle (−3.5, 4.25) r 6.75, A 26, B r 8, Undo/Redo, Restore
`A at 12`, Undo — the Undo/Redo/Restore in A only; unsaved STL/FBX of each tab by the
window's export workers byte-equal to the CLI's exports of `edit-extrude --distance-mm 26`
and `edit-circle` (r 8) copies; Save both, reopen both in a new window. SQL: B equals the
CLI copy in every cell except `meta.modified_at`; A equals it in every cell except
`meta.modified_at` and table `checkpoints` (the list the CLI copy has no reason to hold);
that row is checked by `extract-checkpoint`: the image equals the original A in every SQL
cell, stamp and row identity included. Every object UUID and saved reference kept in
both. Geometry from the STL bytes: A height 26, volume 83 200 mm³; B a closed oriented
cylinder of r 8 at (−3.5, 4.25), h 15.25, volume 3 063.648 mm³ (π·64·15.25 = 3 066.2,
chordal). `native_tabs_scenario_on_session_files_passes_the_comparator_and_its_controls`
(`FCAD_30O_SESSION_FILES_COMPARE_OK negative_controls=6 all_SQL_cells=true`) is the
comparator's self-check on files the window owners produce without a window — **not
window evidence**. Artifacts `tabs-a`/`tabs-b` go to the pinned ufbx block of
`tools/check-fbx-complex.sh` (`FCAD_TABS_SESSION_UFBX_EXECUTED`).

**No solver (`runtime-layout.yml`, OCCT without PlaneGCS):**
`mixed_tabs_switch_and_apply_with_occt_and_no_solver` (`FCAD_30O_MIXED_TABS_EXECUTED`).

## Directed mutations

Both compile, fail an executed assertion, are restored byte for byte (SHA-256 checked)
and followed by a passing positive run. Run on the final source after formatting.

* **M1 — an answer lands in another tab.** `Sessions::addressed` stops comparing the
  tab (`address.tab == self.tab &&` removed). `late_cancelled_or_foreign_answers_…` fails:
  B accepts the answer addressed to A as its own (`left: Failed, right: Ignore`),
  releasing B's slot.
* **M2 — a tab loses its unsaved work while another is opened.** `Tabs::open` adopts the
  new document into the shown controller instead of a new tab (the pre-§30O
  replacement). `two_copies_of_one_document_…` fails with *"A lost its unsaved model and
  history when B was opened"*; `each_tab_keeps_its_own_files_and_crash_copy_…` fails at
  the first crash-copy assertion.

## Execution record (author, macOS arm64, 2026-10-08)

Local, uncommitted, no remote CI for this diff. Native: `source
/private/tmp/ferrite-pr93-review/env.sh`, target `/private/tmp/ferrite-24b-native-target`,
vendor OCCT/PlaneGCS unchanged, `CARGO_BUILD_JOBS=2`; commands run in the sourced shell
(a `bash` launched for a native script loses `DYLD_*` on macOS and was not used for
native runs). Peer CLI rebuilt (`cargo build --release --features planegcs -p
ferritecad-cli`) before the app gates. Stub: `/private/tmp/ferrite-25j-stub-target` with
`OpenCASCADE_DIR=/private/tmp/absent-occt-true-stub`, no `DYLD_*`, no `FCAD_PLANEGCS_DIR`,
`FERRITECAD_REQUIRE_OCCT=0`, `FERRITECAD_REQUIRE_PLANEGCS=0`. `FCAD_ALLOW_LOADER_FAILURE_PROBES`
was never set. No viewer, bundle, GPU or Unity was run. Logs are in the author's session
scratch directory (not part of the repository).

* **Stub is real.** The bridge cache of the build in use holds
  `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE` and `OpenCASCADE_DIR` = the absent folder;
  the build script recorded `CMake Error at CMakeLists.txt:44 (find_package)` and the
  bridge's `No Open CASCADE libraries resolved`; the viewer test binary's `otool -L` has
  no `libTK*` and no PlaneGCS. No OCCT entry reads literally `…-NOTFOUND` in this form of
  the cache. Note: this Mac has a Homebrew OCCT under `/opt/homebrew`; a clean bridge
  configure *without* the disable flag finds it, so the stub depends on that flag.
* `cargo fmt --all -- --check`: clean. `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` (native): clean; the same without `--all-features` in
  the stub target: clean. `tools/check-licence-headers.sh`: 455 files, all MIT.
  `actionlint` (with shellcheck) on `ci.yml` and `runtime-layout.yml`: clean.
* The exact `run:` block of the edited `ci.yml` step, extracted from the YAML and run in
  the stub target: exit 0, 179 gate lines ok, 0 `skipped:`, markers
  `FCAD_30O_TWO_TAB_CRASH_EXECUTED`, `FCAD_30O_LATE_ANSWERS_EXECUTED`,
  `FCAD_30O_STUB_TABS_EXECUTED` (and the §30M/§30N ones). The first attempt failed on a
  real defect — the crash test printed its marker on libtest's line — fixed with a
  leading newline.
* The exact §30O lines of `runtime-layout.yml`: native block exit 0 with
  `FCAD_30O_NATIVE_TABS_EXECUTED volume_a=83200.000 height_b=15.250 volume_b=3063.648` and
  `FCAD_30O_SESSION_FILES_COMPARE_OK negative_controls=6 all_SQL_cells=true`, four
  artifacts; no-solver block exit 0 with `FCAD_30O_MIXED_TABS_EXECUTED`.
* The §30O block of `tools/check-fbx-complex.sh`, run alone with the pinned reader built
  exactly as the script builds it (ufbx `fcc5d6b…`, strict): `checks=6 failures=0` for
  `tabs-a` and `tabs-b`, STL↔FBX joins of 12 and 352 triangles,
  `FCAD_TABS_SESSION_UFBX_EXECUTED`. The script's heavy complex-STEP part was not rerun.
* Native suites: `ferritecad-app` 547 passed, 1 ignored (+3 in its integration test);
  `ferritecad-jobs` 59 + checkpoints 5 + recovery 17 (1 ignored child entry) + save 21 +
  session 12 + unnamed 4; `ferritecad-ui` 108; `ferritecad-cli` recovery 4, checkpoints 2.
  In the native app run the stub-only and no-solver-only tests and the env-gated GUI
  comparators return without executing; they are N/A there, not passes.
* Stub app suite: 534 + the §30O tests; kernel-free tab gates 9 + the stub gate executed.

## Real window recipe (macOS)

Not run by the author. One freshly staged arm64 bundle `APP`; one viewer under the
1536 MiB watchdog, normal memory pressure; a new root outside any checkout; the
recovery folder is the root's own. Never call `getApp`/`getAX` after Quit; check that
the owned PID is gone.

```sh
ROOT=/private/tmp/ferrite-30o-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 tools/document-tabs-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/a.fcad"
```

The generator writes only `a.fcad` (CLI sample 80×40×12), `b.fcad` (CLI circle, centre
12.5, −7.25, r 10.5, h 15.25), their pristine copies in `inputs/`, `facts.json` and the
empty `recovery/`; it refuses a root inside a checkout and makes no window output.

1. `a.fcad` opens; the tab row shows `a.fcad`, selected.
2. **Checkpoints** → type `A 12` → **Create checkpoint**. Tab and title `*a.fcad`.
3. **Edit extrusion…** → 20 → **Apply**; close the form.
4. **Open…** `$ROOT/b.fcad`: no question; a second tab `b.fcad` is selected, `*a.fcad`
   stays in the row.
5. **Edit circle …** → centre −3.5, 4.25, radius 8 → **Apply circle**; close the form.
   Tab `*b.fcad`.
6. Press the `*a.fcad` tab: the 20 mm plate appears. **Edit extrusion…** → 26 →
   **Apply**; close. **Undo** (20), **Redo** (26). **Restore** on `A 12` (12 mm plate),
   **Undo** (26 again). **Export STL…** → `$ROOT/a-unsaved.stl`; **Export FBX…** →
   `$ROOT/a-unsaved.fbx`.
7. **Open…** `$ROOT/a.fcad`: no new tab (*already open here*). **Open…** `$ROOT/b.fcad`:
   the `*b.fcad` tab is shown, no new tab. Export `$ROOT/b-unsaved.stl` and `.fbx`.
8. **New…** → **Empty document** → **Create document**: a third tab `*Untitled`. **Save
   As…** `$ROOT/a.fcad`: refused, *open in another tab*; nothing is written.
9. **×** on `*Untitled` → **Cancel**: it stays. **×** again → **Discard**: closed, `*b.fcad`
   is shown.
10. Quit (`Cmd+Q`): asked about `b.fcad` → **Save**. Then `*a.fcad` is shown and asked →
    **Cancel**: the window stays with two tabs, `b.fcad` without `*`, `*a.fcad` unsaved.
11. Quit again: asked about `a.fcad` → **Save**. The window ends; check the owned PID is
    gone.

```sh
python3 tools/document-tabs-gui.py --compare "$ROOT"
```

`--compare` refuses (`FCAD_30O_GUI_COMPARE_REFUSED`) before any build or peer job when
an output is missing or `a.fcad`/`b.fcad` was never saved; the Rust comparator refuses a
missing output again before its first peer job. It reads only the window's outputs and
requires `FCAD_30O_GUI_COMPARE_OK negative_controls=6 all_SQL_cells=true`: `a.fcad` lists
exactly `A 12`, equals the CLI's `edit-extrude 26` copy in every SQL cell but the stamp
and the list, its extracted checkpoint equals the pristine `a.fcad` in every cell, stamp
included; `b.fcad` equals the CLI's `edit-circle` (r 8) copy in every cell but the stamp
and has no checkpoint; UUIDs and references kept; the four unsaved exports byte-equal to
the CLI's exports of those copies; A 26 high, 83 200 mm³; B a closed cylinder r 8; no
recovery record left. Controls (on copies of the real outputs): a missing output (no
peer job may run first), an unsaved `a.fcad`, `b.fcad` holding A's model, a lost
checkpoint, swapped exports, a recovery record left behind.

## Limits

* No real window was run by the author; the comparator's session-file self-check is not
  window evidence. Widget tests of the tab row are not window evidence either.
* Switching rebuilds the target's picture with the kernel (no CPU or GPU picture is kept
  per hidden tab): a large model takes as long to show again as Undo takes to draw it.
* Forms are not kept per tab: showing or closing another tab waits until the open form
  is closed.
* A case-only respelling of another tab's missing file on a case-insensitive disk is not
  recognised by Save As's refusal (the save's version guard still refuses the second
  writer later).
* The window limit is 8 tabs; private history up to 8 × 512 MiB. The earlier OOM
  investigation is not closed by this slice.
* The tab set and order are not restored after a restart; recovery offers each crash
  copy separately, as in §30M.


## Independent review (2026-10-08)

The exact base `8aa82ff` was rechecked: all six post-merge workflows succeeded,
including runtime run 37866319945. The author's pending-base statement above is
historical, not the final base result.

Review corrected two window state defects:

* Quit entered its clean-document fast path with an open form or foreground work.
  The native/window-close entry now checks the real form and operation owners before
  beginning a Quit pass. A regression covering New, edit, load, Apply and switch
  ownership failed on the old route (`Quit must keep an open new`), then passed.
* Cancel invalidated a switch only when another switch replaced it. Its late answer
  could still reach upload and overwrite a subsequent Apply's status. Cancel now
  retires the switch immediately; the retired worker retains its snapshot lease until
  it finishes, even if the hidden tab is closed. The actual Cancel → Apply regression
  failed before the correction and passed after it; a separate blocked-reader case
  proves file lifetime through closing the target tab.

All three regressions are exact-name gates in the existing CI step. The static
export-boundary check also needed its pre-load function range instead of an eight-line
window: tab deduplication now precedes export cancellation. Cancellation still occurs
before `begin_load`; same-tab Open and refused Open do not cancel an unrelated export.

Local review logs are in `/private/tmp/ferrite-30o-review/`. The shared native
OCCT/PlaneGCS target was reused, peer CLI and viewer rebuilt. Sessions suite: 112
harness passes and one pre-existing ignored case; environment-specific GUI/stub/mixed
branches are N/A in that run, not executed geometry. After the Cancel correction,
the tabs suite passed 16 harness cases: 11 kernel-free and two native scenarios
executed; mixed, stub-only and real-window comparator were N/A. Jobs recovery: 17
passed, one child-process entry ignored; UI: 108 passed. Fmt, workspace all-targets /
all-features clippy, licences, actionlint, shellcheck and export boundary passed.

Local stub review: 12 tests executed, including the three new regressions and
`FCAD_30O_STUB_TABS_EXECUTED`. The active bridge cache disables OpenCASCADE discovery;
the current test executable has no OCCT/PlaneGCS imports (`stub-imports.log`). An old
Homebrew cache also remains in the target and is not used as stub evidence.

### Independent real-window run

A fresh arm64, ad-hoc-signed bundle from code `85ef339` was staged at
`/private/tmp/ferrite-30o-review/gui/layout/FerriteCAD.app`. One viewer (PID 79909)
used only `/private/tmp/ferrite-30o-window-review/` and its private recovery folder.
The initial fixtures came from the bundled CLI; all six final document/export files
were produced by the real window. No GUI output was repaired or filled in by a script.

The observed window sequence covered both tabs' Apply routes, switching back to A,
A's Undo/Redo, checkpoint Restore/Undo, unsaved STL/FBX export from each tab, duplicate
Open of A and B, New in a third tab, Save As refusal on A's occupied tab path,
Close/Cancel then Close/Discard of Untitled, and Quit: Save B → Cancel A kept both
open (B clean, A dirty); the next Quit saved A and exited normally. An additional
clean-A + open Edit extrusion + Cmd+Q check kept the form/window alive with the
explicit before-quitting reason, exercising the review fix in the real window.

There were two declared recipe adaptations. The keyboard layout did not reliably
enter Latin letters, so the actual checkpoint was named `12` instead of `A 12`;
B's edit/export and the Untitled cases were done before A's checkpoint/edit sequence.
A was still pristine when its checkpoint was made. The comparison changed only its
literal expected name (`names == ["12"]`) temporarily; all other checks and all six
negative controls ran unchanged on copies of the actual outputs. The test source was
restored byte-for-byte afterwards (SHA-256
`edc7345ad6de0ceb6cfc9a6a8c9ded5a9976181a9c98d4128d041ef856782657`). Production source and
the running bundle were unchanged. This is a documented window-recipe variant, not a
claim that the original Latin-name input was executed.

`tools/document-tabs-gui.py --compare` executed exactly one real-artifact test:
`FCAD_30O_GUI_COMPARE_OK negative_controls=6 all_SQL_cells=true`, exit 0. The saved
models match their CLI peers in all SQL cells under the comparator's narrow allowlist;
A's extracted checkpoint matches pristine A including timestamps, B has no checkpoint,
and UUIDs/references are preserved. All four unsaved STL/FBX files are byte-identical
to CLI exports. Independent geometry confirms A's 80×40×26 mm / 83 200 mm³ plate and
B's centre (−3.5, 4.25), radius 8, height 15.25 cylinder. Pinned ufbx 0.23.0 also read
both actual GUI FBX files: 6 checks, 0 failures each; 12 and 352 triangles.

Watchdog: 3 097 samples, peak footprint **216.657 MiB**, pressure always normal (1),
swap **1018.0625 MiB before and after**, at least **143.94 GiB** free disk. The viewer
exited 0 after 1696 seconds; no watchdog abort. Its absence was verified by PID only;
no CUA/getApp/getAX call was made after final Quit. The recovery folder has no active
or recoverable record. Logs, name-adaptation runner, memory summary and FBX readings
are under `/private/tmp/ferrite-30o-review/`; window files and watchdog samples are
under `/private/tmp/ferrite-30o-window-review/`. The earlier OOM cause remains unknown.

### CI exposed an asynchronous test race

The later documentation-head CI (run 37879009208, Ubuntu job 113654127167) failed the
record-cleanup test: one active record remained, but two record directories still
existed. `remove_record` unlinks/drops the lease before removing the directory; the
test treated the former observation as a barrier for the latter. A temporary 500 ms
pause at exactly that boundary reproduced the same executed assertion (`2 != 1`).
The test now waits, under its existing 60-second deadline, for both one active record
and one directory, retaining the final private-file and directory assertions. It
passes with that delay. A second temporary probe omitted directory removal entirely:
the corrected test failed on the executed `B's record was never fully retired`
assertion after 60 seconds. Both probes compiled; neither is a compile failure or
zero-test pass. Production recovery source was restored byte-for-byte. Logs are
`cleanup-race-before.log`, `cleanup-race-delayed-after.log` and
`cleanup-missing-negative.log` under the review log directory. This correction changes
only the test's completion condition, not product cleanup or the GUI-tested binary.
After restoring production source, the complete tabs suite passed again (16 harness
cases: 13 executed, three environment-specific N/A); fmt and workspace all-targets /
all-features clippy with `-D warnings` passed. The six-control headless comparator
self-check and native two-model geometry gate both executed in that rerun.

### Remote review CI

Initial production/workflow head: `85ef339113905002fb68766eb4cf25b52787728c`.
At this documentation update, [CI run 37875736052](https://github.com/gesriot/ferrite-cad/actions/runs/37875736052)
has succeeded (7/7 jobs), and [PlaneGCS run 37875726448](https://github.com/gesriot/ferrite-cad/actions/runs/37875726448)
has succeeded (4/4). The CI logs on all three OS were read independently: each actually
emits the two-tab-crash, late-answer and stub markers and passes the three review
regressions by exact name. [Combined runtime run 37875726425](https://github.com/gesriot/ferrite-cad/actions/runs/37875726425)
completed successfully on Linux and macOS; Windows was still running when the
asynchronous test correction above was prepared. That initial run is not claimed as
a complete final-head pass. The final commit contains the corrected test plus this
record, while all production inputs remain identical to the GUI-tested `85ef339`.
The existing CI, PlaneGCS and runtime workflows are required again on that final head;
merge waits for successful completion and an independent audit of actual three-OS
markers. The exact final SHA, run links and audit results are recorded in
[PR #95](https://github.com/gesriot/ferrite-cad/pull/95) before merge. Earlier passes and
local checks do not stand in for those final checks.
