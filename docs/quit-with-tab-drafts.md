# §30V — Quit with unfinished saved-object forms

## Decision before implementation

`Tabs` owns one Quit attempt, addressed by its never-reused generation and runtime
`TabId`. Its form holds use the existing `Draft` and `Forms::park` move; neither
the editor nor a document registry is copied. Form and model decisions also check
the identity of the accepted `Arc<Snapshot>`. A confirmation authorises only this
attempt. All forms stay owned until final successful Quit, including forms whose
tabs have already been saved or answered Discard.

Abort returns each held Draft through the existing hidden-tab draft or shown-tab
aside/restore route. It creates no history step. Published Saves stay published;
model Discard stays deferred. LastTabs publication precedes destruction of forms
and recovery retirement. The first publication refusal returns every form; the
next Quit retries and may exit after a second refusal, as in §30R.

Only idle saved-object forms participate. New, foreground operations, dialogs,
exports and gestures retain their guards. Hidden tabs use the existing switch
worker before any question. Single-tab Close retains §30U ownership and address.

Base after fetch: `main = origin/main = 9d23f42c61b3bb435490c643e10a400efdd89eff`,
tree `fcd2b8357d67833445b18c4cbf927ba2755ba75f`, clean checkout. PR #101 is merged.
Its docs head `13b1706fb65fe74181f7ff6fe293a6f2fe4a11ae` has 7/7 successful
checks. At initial inspection merge-SHA CI and runtime-layout were in progress;
those runs are separate from the PR's checks and this uncommitted diff.

No form persistence, automatic Apply, geometry/schema/CLI changes or session
restore. §30, Milestone 5C and the product remain open.

## Independent macOS window recipe

The author runs no viewer, bundle, CUA, osascript, GPU/pixel or window smoke.
Reviewer: use a fresh arm64 bundle built by the existing staging route, at pressure
1. `APP` is that bundle; generator makes input documents only, outside checkout.
Use Bash without login, separate recovery/tabs folders and the existing watchdog:

```sh
source target/30t-review/env.sh
ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ferrite-30v-window.XXXXXX")/run"
export FERRITECAD="$APP/Contents/MacOS/ferritecad"
python3 tools/quit-with-tab-drafts-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery" FERRITECAD_TABS_DIR="$ROOT/tabs"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/a.fcad"
```

1. In A, Edit extrusion → height `18` → Apply. Reopen height and type ` 26x `,
   including both spaces. Open B; Edit Circle → centre `-3.5`, `4.25`, radius `8`.
   Use its draft Undo/Redo and leave the values unapplied. New → Empty → Create
   makes C an Untitled tab while retaining B's form.
2. Show A; Cmd+Q. The question names A and the unapplied values. **Discard form
   and continue Quit** → **Save** writes accepted 18. B is shown before its form
   question; confirm it. At C's unsaved-model question choose **Cancel**.
3. Require all three tabs still present. A is clean, accepted height 18, literal
   ` 26x ` returned; B has its exact original form, picks and draft Undo/Redo; C
   is still Untitled and unsaved. Record these observations before proceeding.
   Verify Back to form on a fresh Quit also cancels the whole attempt.
4. In A correct the returned height to `26` → Apply. Export STL to `a-unsaved.stl`
   and FBX to `a-unsaved.fbx` in ROOT before Save. Reopen height, type ` 99x `.
5. Close the window. Confirm A's form → Save; confirm B's form; Discard C's
   accepted Untitled model. Require the watchdog's owned PID to exit normally.
   Do not observe the application after exit in a way that relaunches it.
6. Run `python3 tools/quit-with-tab-drafts-gui.py --compare "$ROOT"` with the
   native env sourced. Require `FCAD_30V_GUI_COMPARE_OK all_SQL_cells=true`.
   Read the actual small FBX with pinned ufbx; record peak memory, pressure, swap
   and watchdog exit. The final list contains A and B; no recovery lane remains.

The generator refuses a checkout root and creates no positive window outputs.
The comparator refuses missing actual STL/FBX, unsaved A or changed B before any
Cargo/peer job. It compares every SQL cell except `meta.modified_at`, original
UUIDs/references, the unsaved exports against CLI and the saved file's exports,
independent height/volume, LastTabs and recovery cleanup. Headless artifacts are
never passed to it as window evidence. Windows/Linux interaction remains untested.

## Local verification — 2026-10-09, macOS arm64

Logs are under `target/30v-check/`. All builds were sequential with jobs=1, using
`target/30t-review/env.sh` and `target/30u-check/stub-env.sh` from Bash without
login. No pinned OCCT/PlaneGCS vendor rebuild, additional large target, loader
failure probe, foreign cache cleanup or visit to worktree a200.

The true stub cache has `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE` and
`OpenCASCADE_DIR-NOTFOUND`; its actual owner-test executable and peer CLI import
neither OCCT nor PlaneGCS (`stub-cache-imports.log`). Native imports both; mixed
imports OCCT and no PlaneGCS (`native-imports-final.log`, `mixed-imports-final.log`).

| Executed exact-name gate in `sessions::tests::tabs::drafts::quit_forms` | Required executed marker |
|---|---|
| `late_cancel_returns_all_forms_and_published_save_then_final_quit_cleans_up` | `FCAD_30V_LATE_CANCEL_EXECUTED all_forms=true saved_a=18` |
| `deferred_discard_and_refused_saves_keep_forms_and_models` | `FCAD_30V_SAVE_REFUSALS_EXECUTED` |
| `form_and_model_answers_address_attempt_tab_and_snapshot_once` | `FCAD_30V_ADDRESS_EXECUTED` |
| `switch_and_first_last_tabs_refusal_return_previously_confirmed_forms` | `FCAD_30V_SWITCH_LASTTABS_EXECUTED` |
| `real_quit_widgets_name_loss_back_and_repeat_preserve_literal_form` | `FCAD_30V_WIDGETS_EXECUTED` |
| `quit_keeps_foreground_new_export_and_gesture_holds` | `FCAD_30V_HOLDS_EXECUTED` |
| `stub_quit_switch_refusal_keeps_both_original_forms` | `FCAD_30V_STUB_EXECUTED` |
| `every_saved_form_family_returns_whole_after_confirmed_quit_is_aborted` | `FCAD_30V_FORM_FAMILIES_EXECUTED families=12` |
| `native_cancelled_quit_returns_form_for_apply_export_save_and_final_quit` | `FCAD_30V_NATIVE_EXECUTED volume_a=83200.000 all_SQL_cells=true` |
| `mixed_quit_with_occt_without_solver_returns_form_on_cancel` | `FCAD_30V_MIXED_EXECUTED` |

Each mandatory gate requires one passed test, the exact test name and its marker,
with no `skipped:`. Eight run in the existing ordinary CI workflow; native/mixed
and small FBX reading extend the existing runtime-layout workflow. All inherited
§30U gates are preserved, including their original names and markers.

| Check | Result / log |
|---|---|
| Three complete inherited CI session run bodies, executed as real packed Bash `-c` argv | 244 successful Rust summaries, no `skipped:`; `ci-packed-{720,890,999}.log` |
| New complete CI body, real packed Bash argv | 8/8 exact gates, no skips; `ci-packed-1100.log`, `stub-packed-final.log` |
| Tabs/drafts regression on stub | 77 green Rust results; conditional native/mixed/GUI returns are N/A, not native/window evidence; `stub-tabs-regression.log` |
| Final native and mixed packed commands | Each 1 passed, required marker, no skips; `native-packed-final.log`, `mixed-packed-final.log` |
| Actual small native FBX, strict pinned ufbx 0.23.0 | 6 identity checks, 0 failures; 12 triangles match STL, worst error 3.47e-18 m; `FCAD_30V_UFBX_EXECUTED`, `ufbx-small-final.log` |
| Workspace clippy all targets/features, `-D warnings`; fmt | Passed; `clippy-final.log`, `fmt.log` |
| Licence headers / export boundary / solver ownership / whitespace | Passed; `licence.log`, `export-boundary.log`, `solver-ownership.log` |
| actionlint / shellcheck / Python syntax / run sizes | Passed; `actionlint-final.log`, `shellcheck-final.log`, `run-size-final.log`; 201 blocks, maximum 20,432 bytes, packed argv maximum 20,463, limit 21,000 |
| External input-only generator and refusing comparator | Passed; `/private/tmp/ferrite-30v-input-only/run`; absent real exports refused before Cargo, checkout destination refused; `gui-generator.log`, `gui-refusal.log`, `gui-checkout-refusal.log` |

Native applies the returned literal A form after late Cancel, exports before Save,
then saves accepted 26 over another invalid form and completes Quit across A/B/C.
Every SQL cell except `meta.modified_at`, original UUIDs/refs and byte-equal real
STL/FBX match CLI. Independent STL height is 26 mm, volume 83,200 mm³ (tolerances
1e-4 mm / 1e-2 mm³). Final LastTabs contains A/B, excludes Untitled C; recovery
lanes and private files end through their existing owners.

Two temporary compilable mutations were run on the final source and restored in
`finally`: early `QuitPass.forms.clear()` failed the executed assertion **early
confirmed A form destroyed**; bypassing model/address validation failed **form
answer bypassed model question**. Both logs have `Finished`, `running 1 test` and
`1 failed`, not a compile failure or zero-test success (`early-form-loss.log`,
`model-address-bypass.log`, `mutations-final.log`). Restored `tabs.rs` SHA-256:
`9ca15c809efdb71770ccf399a8c31ba5e1ce5556b4085073b24ee1b17963b967`.
The old Close block and all new positive gates were rerun after restoration.

The first local native packed-command harness changed its tee path without changing
its grep paths: the assertion gate passed, then that local wrapper failed. Both
paths were corrected and the actual packed workflow body passed. Initial clippy
found two style-only condition/match suggestions; those were fixed and the full
clippy command passed. Neither failure is counted as positive evidence.

## Skips, N/A and limits

No mandatory exact gate skipped or ran zero tests. Conditional native/mixed tests
inside the broad stub regression and the optional real-GUI comparator are N/A in
that run. No author window, viewer/bundle, GPU/pixel, CUA or osascript execution.
No full STEP/FBX/GPU corpus, heavy STEP import or Windows/Linux local execution.
The recipe is ready for independent window review; no window pass is claimed.

PR #101's code/workflow SHA `e5818db1b3ab533b04d63ff120afa1a9e5fe1f4f` was
independently verified 15/15 success; docs head `13b1706fb65fe74181f7ff6fe293a6f2fe4a11ae`
7/7 success. Exact merge-base [CI 38027382329](https://github.com/gesriot/ferrite-cad/actions/runs/38027382329)
completed success at the final inspection; [runtime-layout 38027382314](https://github.com/gesriot/ferrite-cad/actions/runs/38027382314)
was still in progress. Four other merge-base workflows completed success. These
are base/PR records, not remote CI for this uncommitted diff (`base-ci-final.json`,
`base-pr.json`, `base-code-checks.json`).

## Working-tree manifest

All changes remain unstaged/uncommitted on `quit-with-tab-drafts`; HEAD, main and
origin/main remain the exact base above. Full final numstat, including untracked
files, is in `target/30v-check/diffstat-all.txt`; full status is in
`target/30v-check/status-final.txt`. No add/commit/push/PR/merge or Git config change.

```text
 M .github/workflows/ci.yml
 M .github/workflows/runtime-layout.yml
 M crates/ferritecad-app/src/main.rs
 M crates/ferritecad-app/src/sessions.rs
 M crates/ferritecad-app/src/sessions/tests/tabs.rs
 M crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs
 M crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs
 M crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs
 M crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs
 M crates/ferritecad-app/src/sessions/tests/tabs/restore.rs
 M crates/ferritecad-app/src/tabs.rs
 M crates/ferritecad-ui/src/close_form.rs
 M crates/ferritecad-ui/src/lib.rs
 M docs/close-tab-with-draft.md
 M docs/decisions/0005-document-session.md
 M docs/document-tabs.md
 M docs/tab-edit-drafts.md
 M tools/check-fbx-complex.sh
?? crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs
?? docs/quit-with-tab-drafts.md
?? tools/quit-with-tab-drafts-gui.py
```

Final resource check: memory pressure 1, swap 0 MiB, free disk 154 GiB. The local
verification directory is 592 KiB; existing stub/debug/release targets are
1.5 GiB / 247 MiB / 1.0 GiB. No own Cargo, rustc, test worker or verification-script
process remained. No viewer was started. The next slice was not begun.

Final diffstat including untracked files: 21 files, +1730 / −120 lines.

## Independent review — 2026-10-10

The full native tabs regression initially failed two inherited comparator scenarios:
`native_tabs_scenario_on_session_files_passes_the_comparator_and_its_controls` and
`native_tab_drafts_scenario_on_session_files_passes_the_comparator_and_its_controls`.
Both used an ordinary switch after starting Quit; the new guard correctly refused
that route. Their switch now uses the existing `show_for_quit` helper. No production
guard or assertion was weakened. The initial failure is retained in
`target/30v-review/native-tabs.log`; subsequent review checks and window evidence
are recorded separately. The implementation plan now includes this slice.
