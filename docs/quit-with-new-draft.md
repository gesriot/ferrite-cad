# §30W — Quit with an unfinished New

## Contract decided before implementation

Base after fetch: `main = origin/main = eb54c46f3d187d0c6bc1dfd183ecbb5e9110b358`
(merge PR #102), tree `62fda5660e1812c21e1182643cd5daaf9b73a5eb`, clean.
Local branch: `quit-with-new-draft`; changes stay unstaged and uncommitted.

`Creates` owns the window's New form and drawing. The saved-object Editor under
New belongs to its tab in `Tabs::aside`. The one §30V `QuitPass` in Tabs gains a
New question addressed by its never-reused Quit generation and `NewGeneration`.
No New tab/session, Editor copy, second Quit machine, GUI serialisation or Apply.

| Event | Owners and transition |
|---|---|
| Quit over idle New | Check all foreground/gesture/I/O guards first; start the existing QuitPass with its addressed New question. Nothing moves yet. |
| Back to New | Validate both generations and the still-current New; abort the whole pass without changing New. |
| Discard New and continue Quit | Validate transition before taking anything. Move the whole New form/Editor into Creates' held value; worker handles stay in Creates. Mark the addressed question confirmed once. Return A's aside form, then continue §30V's form/model/switch route. |
| Late Cancel or any refusal | Tabs aborts and returns every tab Draft. Restore the shown tab's form, then set it aside before moving New back from Creates. New is window-owned: on late Cancel at B it appears above B; A's form remains with hidden A, B's form is aside. No form is overwritten. |
| First LastTabs error | Existing §30R abort policy, then exactly the same whole-New restoration. The next Quit asks again; the existing second-error exit policy is unchanged. |
| Successful final Quit | LastTabs publication/announced second refusal precedes tab/recovery retirement and final destruction of held New. |
| Repeated Quit / stale or duplicate answer | Neither resets a pass nor moves/destroys any New, advances a model question or repeats Save. Consent is only for the named Quit and New. |
| Running/queued Create or candidate not yet accepted | Quit held; no cancellation, no worker transfer, no dropped result. A completed failure leaves idle New eligible again. |
| Create accepted as Untitled | Old New ends; ordinary §30V Quit, no second New question. |
| Empty window with New | Same question; Back returns New, confirmation can reach ordinary empty-window Exit. No fake tab. |

The prompt says **Back to New / Discard New and continue Quit**, names the typed
choices/sizes or drawn sketch being given up, explains that destruction waits for
successful Quit and that already published Saves stay saved. Nothing is created,
applied or written from New by a Quit answer.

Open/Reopen/Recover, Apply/Add/Save, copy/export workers, native modal dialogs,
camera/vertex/drawing gestures and running/queued Create keep their exclusions.
§30T Open-over-New, geometry, solver/FFI, CLI/JSON, schema, checkpoints/recovery
persistence and macOS arm64-only rules are unchanged.

## Verification and independent window review

Implementation, exact executed gates, directed mutations, logs, final manifest,
resource measurements and the input-only independent window recipe will be
recorded here. Author runs no viewer, bundle, CUA or GPU/pixel test. Headless owner
and widget results are not GUI evidence. §30, Milestone 5C and product remain open.

## Independent macOS window recipe (author did not run it)

Reviewer uses a fresh arm64 bundle, the existing staging route, Bash without
login and the 1536 MiB watchdog. `APP` names that fresh bundle. The generator
creates only A/B and pristine inputs outside checkout, never window outputs:

```sh
source target/30t-review/env.sh
ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ferrite-30w-window.XXXXXX")/run"
export FERRITECAD="$APP/Contents/MacOS/ferritecad"
python3 tools/quit-with-new-draft-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery" FERRITECAD_TABS_DIR="$ROOT/tabs"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/a.fcad"
```

1. A: Apply height `18`, reopen height and leave literal ` 26x `. Open B;
   Apply its extrusion height `22`; Edit Circle → centre `-3.5`, `4.25`,
   radius `8`. Confirm draft numbers, Undo/Redo without Apply. Return to A.
2. **Create sketch + Extrude…** over A's form. Line polygon: add `(0,0)`,
   `(20,0)`, `(20,10)`, height `10`. In the first vertex X type `1e999x`,
   then ` 33.0 `; Undo to the invalid value, leaving history both ways.
3. Cmd+Q → **Discard New and continue Quit** → confirm A's form → **Save**.
   B is shown; confirm its form → **Cancel** at B's dirty-model question.
   Require A clean at accepted 18, B still dirty at accepted 22, both tabs
   present, New returned above B. Its Undo/Redo must work. A's hidden literal
   form and B's set-aside form must remain owned. A fresh Cmd+Q → **Back to New**
   cancels the whole attempt and changes no typed value.
4. On returned New, Redo to ` 33.0 `, Undo back to `1e999x`, Undo back to `0`
   (use as many history steps as the actual text entry made). Close contour →
   **Create new document**. A third Untitled appears. Save As `n-created.fcad`;
   Apply its extrusion height `7`; export `n-unsaved.stl` and `n-unsaved.fbx`
   before Save; Save As `n.fcad`.
5. Return to B: the exact pending Circle numbers and its own draft Undo/Redo
   remain. Return to A: literal ` 26x ` is back. Correct to `26` → Apply;
   export `a-unsaved.stl` and `a-unsaved.fbx` before Save. Reopen height and
   leave another invalid ` 99x ` form.
6. System window Close → confirm A's form → Save; confirm B's form → Discard
   B's accepted changes. New's accepted, saved tab needs no old-New question.
   Require watchdog-owned PID normal exit; do not relaunch to inspect exit.
7. Run the comparator below and the existing strict pinned ufbx loop on the
   actual `n-unsaved.fbx`/STL. Record window observations, watchdog exit/peak,
   pressure and swap separately from headless evidence.

```sh
cargo build --release --features planegcs -p ferritecad-cli
python3 tools/quit-with-new-draft-gui.py --compare "$ROOT"
```

Require one exact test, no `skipped:`,
`FCAD_30W_GUI_COMPARE_OK all_SQL_cells=true`. It preflights every actual output
before any peer job; compares A against CLI, B byte-for-byte with its input,
New's original creation semantics against CLI with independent identities, and
New's edited SQL cells/UUID/refs and cold exports against CLI using
`n-created.fcad` as the actual source. Only `meta.modified_at` is excluded from
same-source SQL comparisons. New height 7 mm, volume 700 mm³; A 26 mm,
83,200 mm³. LastTabs names A/B/n; no active/recoverable lane remains.
Five negative controls run on copies: missing New FBX (no peer job), altered New
SQL, wrong New FBX, changed B, wrong LastTabs. Original evidence is never changed.
Generator refuses a destination under any checkout. The author's handoff claimed
no GUI pass; the independent window execution is recorded below.

## Executed local verification — 2026-10-10, macOS arm64

All builds/matrices were sequential, `CARGO_BUILD_JOBS=1`, tests one thread,
Bash without login. Existing `target/30t-review/env.sh` and
`target/30u-check/stub-env.sh` were read and their config/library/wrapper paths
checked before use. Native uses the existing `target`, stub `target/30u-stub`.
No third large target, pinned vendor rebuild, loader-failure probes or visit to
worktree a200. Native CLI was rebuilt/confirmed from the existing target
(`native-cli-build.log`, cached 0.09 s).

Every mandatory exact gate requires its exact name, one passed test, its executed
marker and no `skipped:`. The ten below run in one new small ordinary CI block;
native runs in a new small step of existing runtime-layout, mixed extends its
existing no-solver block, and ufbx extends the existing reader loop. No heavy
workflow duplicate. Logs below are in `target/30w-check/`.

| Exact gate in `sessions::tests::tabs::drafts::quit_new` | Required marker |
|---|---|
| `idle_empty_and_sample_new_back_late_abort_and_original_create_are_executable` | `FCAD_30W_IDLE_FORMS_EXECUTED` |
| `late_cancel_after_save_or_deferred_discard_returns_new_above_b_and_both_tab_forms` | `FCAD_30W_LATE_CANCEL_EXECUTED` |
| `consent_names_quit_and_new_once_and_never_skips_the_dirty_model` | `FCAD_30W_ADDRESS_EXECUTED` |
| `save_refusals_dialog_cancel_save_as_and_external_conflict_return_whole_new` | `FCAD_30W_SAVE_REFUSALS_EXECUTED` |
| `scene_switch_refusals_and_first_last_tabs_error_return_all_owners_second_error_exits` | `FCAD_30W_SWITCH_LASTTABS_EXECUTED` |
| `running_queued_and_unaccepted_create_hold_quit_and_deliver_their_original_result` | `FCAD_30W_CREATE_BARRIERS_EXECUTED` |
| `real_new_quit_widgets_explain_loss_and_back_and_discard_address_the_actual_owners` | `FCAD_30W_WIDGETS_EXECUTED` |
| `stub_real_creation_refusal_returns_idle_new_and_underlying_form` | `FCAD_30W_STUB_EXECUTED` |
| `idle_new_transition_checks_foreground_and_gestures_before_taking_any_form` | `FCAD_30W_GUARDS_EXECUTED` |
| `new_contour_over_saved_sketch_returns_both_editors_with_functional_history` | `FCAD_30W_SHARED_EDITOR_EXECUTED` |
| `native_returned_new_creates_then_publishes_and_matches_cli_sql_uuid_cold_geometry_exports` | `FCAD_30W_NATIVE_EXECUTED volume_n=700.000 all_SQL_cells=true` |
| `mixed_returned_new_creates_with_occt_without_solver_and_keeps_both_forms` | `FCAD_30W_MIXED_EXECUTED` |

| Executed check | Evidence |
|---|---|
| Five full CI `run` bodies as real packed Bash `-c` argv, after mutation restoration | `ci-packed-{720,890,999,1100,1125}.log`, `stub-packed-final.log`: 88 + 101 + 55 inherited summaries, 8 §30V and 10 §30W; 262 successful summaries, no `skipped:` |
| Full affected tabs/forms suite, stub | `stub-tabs-final.log`: 90 green harness results. Native/mixed/opt-in GUI returns here are N/A; they are not native geometry or window executions. |
| Native exact packed workflow step | `native-packed-final.log`, `native-packed-result.log`, `quit-new-native.log`: one executed test, required native marker, no skip; five comparator negative controls passed on copies. |
| Full affected tabs/forms suite, native + PlaneGCS | `native-tabs-final.log`: 90 green harness results, no `skipped:`; old §30T/§30U/§30V native markers all present. Stub-only, mixed-only and opt-in GUI early returns remain N/A. The old comparators still use `show_for_quit`. |
| Separate OCCT/no-solver packed commands extracted from existing workflow | `mixed-packed-final.log`, `mixed-packed-result.log`, `quit-new-mixed.log`: four exact §30T/U/V/W gates, all one passed plus required markers, no skips. |
| Actual small native New FBX, strict pinned ufbx 0.23.0 | `ufbx-pin.log`, `ufbx-small-final.log`, `ufbx/`: checks=6, failures=0, 8 triangles match STL, worst error 0 m, `FCAD_30W_UFBX_EXECUTED`. Cached pinned reader reused, no reader/vendor rebuild or large STEP corpus. |
| True stub/native/mixed dependency inspection | `stub-cache.log`, `stub-imports.log`, `native-imports-final.log`, `mixed-imports-final.log`. Stub cache disables OpenCASCADE discovery; actual stub app/CLI import neither OCCT nor PlaneGCS. Native app/CLI import both. Mixed app imports OCCT (50 libTK libraries), no PlaneGCS. Both release test executables are arm64. |
| Workspace clippy all targets/features, `-D warnings`; fmt | Passed, `clippy.log`, `fmt-final.log`. |
| Licence headers, export boundary, solver ownership, whitespace | Passed; `licence.log` (480 MIT headers), `export-boundary.log`, `solver-ownership.log`; `git diff --check`. |
| actionlint, shellcheck, Python in-memory syntax, Actions run size | Passed; `actionlint-final.log`, `shellcheck-final.log`, `run-size-final.log`: 203 blocks, maximum 20,432 bytes, packed argv maximum 20,463, limit 21,000. Maximum unchanged. |
| External input-only generator, missing-output comparator and checkout refusal | `gui-generator.log`, `gui-refusal.log`, `gui-checkout-refusal.log`, `input-refusals-result.log`; input root `/private/tmp/ferrite-30w-input-only-1wttnxte/run`. No real window outputs were fabricated. |

The native scenario returns New after late model Cancel on B following Save A,
checks New Undo/Redo and creates its original contour through the production job.
It publishes `n-created.fcad`, applies height 7, exports before Save, then Save As
publishes `n.fcad`; A's returned invalid form is corrected to 26 and exported
before final Save/Quit. Same-source comparisons include every SQL cell/schema
except `meta.modified_at`, document/object/segment identities, refs and source.
The independently created CLI graph uses independently minted IDs resolved to
semantic relationships; subsequent CLI edits use the actual New source and must
keep exact IDs/refs. Cold geometry and byte-equal STL/FBX are checked separately.
Final LastTabs contains A/B/n and every recovery/private owner cleans up.
Artifacts for the small reader are `native/quit-new-session/n-unsaved.{stl,fbx}`.

### Directed compiling mutations and positive repeats

Both ran one exact test and failed an executed assertion, not compilation or a
zero-test harness. `mutate.py` used `finally` to restore `main.rs` byte for byte
between and after both mutations (`mutations.log`). Final SHA-256:
`82ca2b3f40e2b39cbc8f5d6064feaa943d4af9f12f8c984c4a6f554a338e76d6`.

* Premature destruction in the abort restoration route: `early-new-loss.log`,
  **held New lost on late cancel**.
* Stale-address substitution mutant: incoming old New reply is readdressed to the
  current Quit question. `stale-new-answer.log`, **stale Quit answer accepted**.
  This changes observable behaviour: an old answer is accepted. It is not an
  observationally equivalent mutant and is not merely removal of one guard.

All five positive packed stub blocks, full stub/native suites, native and mixed
exact gates were rerun after byte restoration on the final production source.

### Findings and corrected verification failures

The initial full stub tabs suite failed three inherited expectations that idle
New must hold Quit (`stub-tabs-initial.log`). They now check §30W's New question,
Back, untouched owners and no premature LastTabs; their operation/gesture holds
are preserved. No switch guard was weakened for an inherited native comparator.

The first new test draft tried to create a drawn contour through the true-stub
production kernel and correctly received refusal. Owner/action evidence now uses
an explicit MockKernel factory for that unit path; the dedicated true-stub gate
still runs and requires real production refusal, and the native gate separately
requires actual geometry. Another initial fixture attempted a no-clobber create
over an existing path; its external-conflict control now creates another file
and renames it into place. These initial harness failures are not passes.
Initial compiler errors while matching existing field/enum names were corrected
before any mandatory gate ran; no infrastructure failure is claimed as geometry.

Owner review also added a live-New guard to the abort restoration helper: a
late callback must leave the shown tab's form aside while a pending/newer New
owns the window. The shared-Editor gate exercises saved Sketch and new contour
as separate moved values with working Undo/Redo; neither form is overwritten.

### Limits and resource handoff

No viewer, bundle, CUA, osascript, GPU/pixel or author window execution. The opt-in
GUI comparator was not run on real window outputs. No Windows/Linux local/window
execution, no complete STEP/FBX/GPU corpus. Native owner/widget results are
headless evidence. Independent reviewer must execute the window recipe and CI;
no remote CI has run for this unstaged diff. §30, Milestone 5C and product remain
open; this does not resolve the earlier OOM. Next slice was not begun.

Measured pressure stayed 1, swap stayed 0 MiB, free disk 154 GiB. Final local
verification directory 552 KiB; existing stub/debug/release targets 1.5 GiB /
247 MiB / 1.0 GiB. No own verification, Cargo or rustc process remains. No foreign
process/directory was cleaned. The temporary Python import cache made by local
orchestration was removed (its own file only).

## Unstaged handoff manifest

HEAD/main/origin/main remain the exact base and tree at the top. Staged diff is
empty. Full status and all-file numstat/diffstat (including untracked) are saved
in `target/30w-check/status-final.txt` and `target/30w-check/diffstat-all.txt`.
No add/commit/push/PR/merge, forbidden Git rewrite/cleanup or config change.

```text
 M .github/workflows/ci.yml
 M .github/workflows/runtime-layout.yml
 M crates/ferritecad-app/src/creates.rs
 M crates/ferritecad-app/src/main.rs
 M crates/ferritecad-app/src/sessions/tests/tabs.rs
 M crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs
 M crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs
 M crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs
 M crates/ferritecad-app/src/sessions/tests/tabs/restore.rs
 M crates/ferritecad-app/src/tabs.rs
 M crates/ferritecad-ui/src/close_form.rs
 M crates/ferritecad-ui/src/lib.rs
 M docs/decisions/0005-document-session.md
 M docs/implementation-plan.md
 M docs/open-during-new.md
 M docs/open-new-recover-with-drafts.md
 M docs/quit-with-tab-drafts.md
 M tools/check-fbx-complex.sh
?? crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs
?? docs/quit-with-new-draft.md
?? tools/quit-with-new-draft-gui.py
```

Final diffstat including untracked files: 21 files, +2012 / -66 lines.


## Independent review — 2026-10-10, PR #103

The unstaged handoff above is historical. The implementation was committed as
`de7137e5f99632fb98e4096897bda9ec1e987284` after review. The reviewer corrected two
stale ownership/idle comments and the description of the observable stale-answer
mutant. No production behaviour needed changing in that review.

Local reviewer evidence is in `target/30w-review/`: fresh release CLI/app build,
workspace clippy with all targets/features and `-D warnings`, fmt, licence headers
(480), export boundary, solver ownership, actionlint, shellcheck, whitespace and
Actions run-size checks passed. The full native tabs block returned 90 passes;
the true-stub block also returned 90 harness passes, with 11 explicit native skips
not counted as geometry. Ten mandatory stub tests were separately executed by
exact name without skips (`stub-exact.log`). The exact OCCT-without-PlaneGCS test
executed `FCAD_30W_MIXED_EXECUTED` (`mixed.log`). Conditional stub/mixed and opt-in
GUI early returns in a whole-suite run remain N/A. Author mutation evidence was
reviewed; the reviewer did not repeat those mutations.

### Real macOS window and published files

A fresh staged/signed arm64 bundle in `target/30w-review/gui/FerriteCAD.app` ran
one viewer, PID 74281, under the 1536 MiB watchdog, without a DYLD override.
Inputs and actual outputs are outside checkout in
`/private/tmp/ferrite-30w-review-window/run`; only the input generator made A/B.
The reviewer operated the live window with CUA. No post-Quit app lookup or AX
capture was used, so this run did not relaunch the viewer outside its watchdog.

Observed: Apply A=18 and B=22; retain A's invalid height and B's confirmed Circle
draft/history; start a new triangle over A; Cmd+Q and Discard New; Save A then
Cancel B; New and both forms return. A second late Cancel exercised New's Redo;
a fresh Cmd+Q/Back left its invalid text intact. The returned contour created an
Untitled tab, which was saved as `n-created.fcad`, edited to height 7, exported
before Save, and saved as `n.fcad`. B retained centre (-3.5,4.25), radius 8 and
working Undo/Redo; A retained its invalid 26x, then applied 26 and exported before
Save. System window Close over a new invalid 99x form led through Save A and
Discard B to a normal exit, with no old-New question after creation.

Input deviations are recorded rather than called the exact recipe verbatim.
While blurring the invalid first coordinate, the operator accidentally selected
Revolve 360 before the first Quit; that variant was restored correctly. The
operator then returned to Extrude, established invalid-text/Redo history and
repeated the late-Cancel sequence before testing Back and creation. Returning the
first coordinate to zero was explicit text entry. B's floating form was collapsed
to reach the tabs, then expanded to verify its retained fields. No positive output
was copied from CLI or a headless run.

The first actual-file comparison failed at LastTabs: the native Save dialog
spelled New's path `/tmp/.../n.fcad`, while the fixture used `/private/tmp/...`.
Both resolve to the same file. The test now resolves paths before comparing the
ordered file list; the application's chosen-path storage is unchanged. The
unmodified window artifacts then passed the exact comparator, including its five
negative controls, with `FCAD_30W_GUI_COMPARE_OK all_SQL_cells=true`
(`gui-compare-fixed.log`). All-SQL comparisons (only the write stamp excluded),
UUID/refs, cold rebuild and byte-equal STL/FBX against CLI passed. A is 26 mm /
83,200 mm³; New is 7 mm / 700 mm³. B remains byte-identical to the input. LastTabs
contains A/B/n in order and no recovery owner remains.

Pinned ufbx 0.23.0 read both actual GUI FBX exports with six checks and zero
failures each. Independent STL/FBX joins found 12 triangles for A (worst difference
3.47e-18 m) and eight for New (0 m). Logs are `a-unsaved-reader.txt`,
`n-unsaved-reader.txt` and their triangle files in the review folder.

Watchdog: normal exit 0 after 1126.513 s, no abort, sampled peak footprint
217.470 MiB, pressure always 1, swap always 0. The earlier OOM is not diagnosed or
claimed fixed. Windows/Linux window interaction and the large STEP/GPU corpora
were not rerun locally. Exact remote CI provenance is recorded after completion;
local checks and the author's base CI are not substitutes for it.


### Linux CI exposed a test scheduling assumption

On `02ec2bd`, Linux ordinary CI failed the idle-New test at the immediate
post-creation `begin_quit()`. The test helper had received and accepted the
channel answer, but the worker could still be returning; the production
`quit_idle()` guard intentionally also checks live worker handles. Receiving an
answer is not a thread join. The idle scenarios now wait for that predicate with
a bounded deadline, without changing owner state or production exclusions.
The existing exact Create-barrier gate additionally holds a worker **after** its
answer has been delivered, proves Quit still refuses, then releases it and proves
idle New can enter and return from Quit. This separates the two moments without
assuming scheduler timing. The failed CI run is retained as evidence and is not
called successful. No production code changed for this test correction.

Positive repeats after the correction: the §30W group executed on true stub
(13 harness passes, one explicit native skip; mixed and the opt-in comparator
N/A) and native (13 harness passes; conditional tests N/A). The idle case and
Create-barrier gate each passed 20 further exact stub executions. Workspace
clippy/fmt and the real-artifact comparator passed again. Evidence:
`stub-ci-race-complete.log`, `stub-idle-repeat.log`, `native-ci-race-final.log`,
`clippy-race-final.log`, `gui-compare-race-final.log` in the review folder.


### Exact CI provenance

Final code and test input: `3a5f2913f15871823c615babe3e49e403316a58d`.
The following documentation-only recording does not change a runtime or test input.

* [Ordinary CI 38060379068](https://github.com/gesriot/ferrite-cad/actions/runs/38060379068):
  all seven jobs succeeded. Downloaded logs independently confirm ten named §30W
  tests and ten execution markers on **each** of Linux, macOS and Windows (30/30),
  each with one passed test and no skip in that block.
* [PlaneGCS pin 38060375374](https://github.com/gesriot/ferrite-cad/actions/runs/38060375374):
  all four jobs succeeded, including the cross-platform comparison.
* [Combined runtime 38060375356](https://github.com/gesriot/ferrite-cad/actions/runs/38060375356)
  is the code-head native/mixed/ufbx/packaging run. At this documentation recording
  it is still running; its eventual conclusion is not inferred from local results.
  Merge requires success, and the final per-platform log audit and docs-head checks
  are recorded in [PR #103's merge assessment](https://github.com/gesriot/ferrite-cad/pull/103).

The base `eb54c46` was separately confirmed at 27 successful check runs. Earlier
branch attempts are superseded, not counted as success: `de7137e` was cancelled on
update; `02ec2bd` includes the Linux scheduling failure described above and was
superseded by its correction. The actual GUI bundle's production source is unchanged
between `de7137e` and the final code head; only the comparator/tests and documentation
changed. Local artifact digests are in `target/30w-review/gui-artifact-hashes.json`.
