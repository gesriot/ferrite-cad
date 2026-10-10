# §30U — close a tab with an unfinished form

## Decision before implementation

Use `Tabs`, the existing `Forms::park` / `restore` and `Draft`, and the session's
Save continuation. One Close attempt moves the whole form into one `Draft`; it
does not duplicate it or make an accepted session step. It holds the original
snapshot identity and a once-only runtime `{TabId, close generation}` address.
While it is pending, the form cannot be edited or replaced by another action.

| Event | Owner | Effect |
|---|---|---|
| × request | App availability, `Tabs::close_step` | Existing work and gesture guards first; hidden form → Show |
| Show hidden tab | Existing switch worker / `Bind::Switch` | Restore that tab's literal form only after scene preparation; refusals change nothing |
| First question | `Tabs`, one parked `Draft` | Name the document and the unapplied form values; Back restores the same owners |
| Discard form and continue Close | `Tabs`, addressed decision | Authorise this attempt only; keep its Draft until actual Close succeeds |
| Dirty / Untitled question | `Sessions::replacing`, App native dialog | Existing Save / Discard / Cancel about accepted model |
| Save / Save As | `Sessions`, addressed worker and continuation | Save accepted snapshot; publication is required before continuation |
| Cancel / error | `Tabs::cancel_form_close`, `Forms::restore` | Give original Draft back, including history and original base; no reconstruction from model |
| Actual Close | Empty scene preparation then `Tabs::close` | Remove only this TabId, its session files/recovery and its parked Draft |
| Late / duplicate / changed-version answer | `Tabs` close address and snapshot identity | No authority for another attempt, tab or version; no state consumed |

Quit, New, Open, Recover and Reopen keep their existing rules. No automatic Apply,
form persistence, background hidden work, geometry/schema/CLI changes. §30,
Milestone 5C, the product and the old OOM investigation remain open.

## Base

Fresh fetch: `main = origin/main = f0c9471a2427159a3fe3ba7be063a4c8a39240a4`,
tree `fb4d168cf6a03da3d30eab2564340a80ac1d37c3`, initially clean. PR #100 MERGED,
head `e8084dc07ee035be43327375dc9ae6d147a78313`, merge is the base itself.
At the initial check, CI run 38020030437 and runtime-layout 38020030436 were
**in_progress**; product sbom, rust sbom, planegcs pin and rust notices completed
successfully. At 2026-10-10 04:03 UTC, [CI 38020030437](https://github.com/gesriot/ferrite-cad/actions/runs/38020030437)
was completed **success**, while [runtime-layout 38020030436](https://github.com/gesriot/ferrite-cad/actions/runs/38020030436)
was still **in_progress**. This is base CI only; no CI has run for the
uncommitted §30U diff.

## Contract

The question says **This tab has an unfinished form**, names the document, and
identifies what giving up the form would lose: entered values not yet applied.
**Back to form** restores the exact form. **Discard form and continue Close**
confirms this attempt only; a dirty/Untitled model then uses the ordinary native
Save / Discard / Cancel question. No Apply is started by either Close decision.
While the decision or its Save is pending the form is parked, and other document
actions and document shortcuts are unavailable. Save cancellation waits for the
worker's publication outcome, as before.

`FormCloseId` is `{TabId, close generation}`. `Tabs` also checks the parked
Draft's `Arc<Snapshot>` by identity. First confirmation is consumed once; a newer
attempt, another runtime tab (even with the same DocumentId), a changed accepted
snapshot or a repeated confirmation has no authority. The final Close requires
both confirmation and the existing accepted-model decision; Save continues with
`Continuation::CloseForm(id)` only after publication and a clean model. A changed
base cannot be closed by that answer. Returning its original form on a changed
base retains the existing stale-draft hold, so it is readable/cancellable but
cannot Apply. No newer shown form is overwritten.

The form is one existing `Draft`, holding one snapshot lease; nothing is cloned,
serialised or reconstructed. The completeness list is the existing
[Forms owners table](tab-edit-drafts.md#the-owners-completeness-check): height,
saved Sketch, Circle/Annulus, Revolve angle, constraints, Cut/Fillet/Chamfer Add
and Edit. New forms stay window-owned and continue to block Close. Closing an
active tab still prepares the empty scene first; refusal returns its original
form. A successful Close drops the Draft lease before its session, removes only
that runtime tab's private files when existing worker leases end, and retires only
its recovery lane.

## Verification and requirements

All new owner tests live beside the existing tab/draft fixtures in
`sessions::tests::tabs::drafts::close_form`; widgets are real egui with synthetic
input, **not an OS window**. `ci.yml` names each kernel-free gate exactly and
requires its execution marker, one passed assertion test and no `skipped:`:

| Gate suffix | Evidence / marker |
|---|---|
| `same_document_tabs_keep_literal_forms_and_history_until_their_own_close_succeeds` | Same DocumentId, separate TabIds; shown Back, hidden scene refusal, clean Close, refused empty scene, original constraint picks and Undo/Redo; `FCAD_30U_TAB_FORMS_EXECUTED` |
| `cancelled_close_and_refused_saves_return_the_original_form_and_accepted_model` | Dirty Cancel, Save As Cancel, tab-owned/occupied/I/O refusals, cancelled worker, external conflict; Untitled Save As writes accepted 18, never `99x`; explicit model Discard, B unchanged; `FCAD_30U_CANCEL_SAVE_EXECUTED` |
| `close_decisions_are_once_only_and_address_the_tab_attempt_and_snapshot` | Old/repeated/foreign/new-base decisions; no Close before dirty-model decision; original stale form kept; `FCAD_30U_ADDRESS_EXECUTED` |
| `every_existing_form_family_uses_the_same_move_and_keeps_its_entire_state` | Twelve saved Add/Edit families, whole editor state (text, choices, histories, reading) before/after the same move; `FCAD_30U_FORM_FAMILIES_EXECUTED families=12` |
| `real_close_buttons_name_unapplied_values_and_parked_fields_cannot_take_typing` | Real buttons and document/loss words; focus cannot type into a parked or newly returned field without clicking; `FCAD_30U_WIDGETS_EXECUTED` |
| `close_request_preserves_foreground_work_and_gesture_holds` | Apply/Save, load, export question, New/drawing, copy, camera/vertex gestures through the production entry predicate; `FCAD_30U_HOLDS_EXECUTED` |
| `stub_hidden_close_refusal_keeps_both_forms_and_no_native_work_is_claimed` | Production scene worker refuses without OCCT and keeps both forms; `FCAD_30U_STUB_EXECUTED` |

`runtime-layout.yml` separately requires OCCT without PlaneGCS for
`mixed_close_with_occt_and_no_solver_keeps_the_form_on_cancel`
(`FCAD_30U_MIXED_EXECUTED`), and OCCT **with** PlaneGCS for
`native_close_cancel_apply_export_save_and_close_matches_cli_and_keeps_b`
(`FCAD_30U_NATIVE_EXECUTED volume_a=83200.000 all_SQL_cells=true`). The native
scenario applies 18 to A, keeps its separate invalid ` 26x ` form through Close
and model Cancel, applies 26, exports, and saves/closes over a new ` 99x ` form.
B keeps its own literal Circle form, original base, and unchanged file. Close
happens after publication; only A's private files and recovery record disappear.

The comparator checks every SQL cell except `meta.modified_at`, every original
UUID/reference, and byte-equal STL and FBX against CLI `edit-extrude 26` and the
exports of the saved file. No new UUID or geometry tolerance is set aside; the
independent STL extent/volume assertions allow only 1e-4 mm / 1e-2 mm³ arithmetic
error. Strict pinned ufbx reads the actual small native FBX, with the existing
identity reader and STL/FBX triangle join (`FCAD_30U_UFBX_EXECUTED`). The optional
`native_compare_real_close_draft_gui_outputs` executes only with real outputs in
`FCAD_30U_GUI_DIR`; without it, it is N/A, not a GUI pass.

`tools/check-actions-run-size.py` checks every workflow's run scalar against
21,000 bytes (stricter than characters), measures packed argv and really passes
Bash blocks to `bash -n -c`. Platform cmd/PowerShell bodies pass as actual argv to
a size probe here; their syntax/execution remains platform CI's. Actionlint
remains the full YAML/workflow validator. The old oversized kernel-free step was
split at complete loops into three steps; inherited job env, commands and markers
are preserved. Native/mixed/ufbx gates extend existing workflows.

## Local execution — 2026-10-10, macOS arm64

Native env is the existing `target/30t-review/env.sh`, sourced directly in Bash
without login. OCCT is `vendor/install`, PlaneGCS is `vendor/planegcs`; `otool -L`
proves both imports in the native CLI and test executable. Peer CLI was rebuilt
before the worker gates. Mixed uses the same repo target with no `planegcs`
feature, `FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=0`, and OCCT-only
DYLD paths; its test executable imports OCCT and no PlaneGCS.

No surviving stub target existed. One new `target/30u-stub` uses jobs=1 and an
absent OCCT path **plus** `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE` from the
first configure. Its cache proves `OpenCASCADE_DIR-NOTFOUND`; the actual stub
test executable and peer CLI import neither OCCT nor PlaneGCS. Environment,
cache/import evidence and logs remain locally under `target/30u-check`.

| Executed check | Result / local log |
|---|---|
| Seven new kernel-free exact-name gates, true stub | Each 1 passed, no skip, required marker; `kernel-free-final.log` and named logs |
| Three complete split CI session blocks, true stub | Exit 0, 244 successful Rust summaries, no `skipped:`; `ci-block-{720,890,999}.sh.log` |
| Directed early-form-loss and model-guard-bypass mutations | Both compiled and failed executed assertions; final `tabs.rs` bytes restored, positive gates rerun; `mutation-summary.log`, two failure logs |
| New native exact gate, OCCT + PlaneGCS required | 1 passed, no skip, native marker; `native-gate.log`, actual STL/FBX in `native/close-draft-session` |
| Compact existing tabs/drafts regression, native build | 66 green Rust results; conditional mixed/stub/GUI tests are N/A in this run, **not 66 geometry/window executions**; `native-tabs-regression.log` |
| New mixed exact gate, OCCT required / no solver | 1 passed, no skip, mixed marker; `mixed-gate.log` |
| Only the small new pinned-ufbx block | 6 identity checks, 0 failures; 12 STL/FBX triangles, worst error 3.47e-18 m; `ufbx-small.log` |
| fmt, workspace all-targets/all-features clippy `-D warnings` | Passed; `clippy-final.log` |
| Licence / export boundary / solver ownership / whitespace | Passed; 474 MIT headers checked |
| actionlint / shellcheck / Python syntax / all run sizes | Passed; 199 blocks, max 20,432 bytes, original packed argv max 20,463, limit 21,000 |
| Input generator and refusing GUI comparator | External input-only directory created; missing actual STL/FBX refused before Cargo; checkout destination refused; `gui-generator.log` |

The first native rerun exposed a test timing assumption: a recovery-copy worker
may still lease A's private files just after successful Close. The test now waits
up to 10 seconds for that existing lease to end and still requires A's files to
disappear; the exact native gate and regression then passed. No production
cleanup was bypassed. Mutation restoration SHA-256 of final `tabs.rs`:
`2582fd4020655b88407113fc1a86be5a4a34d7ab6acc97616a758fd9a75e80b5`.

Builds were sequential, jobs=1, with no pinned vendor rebuild. Pressure remained
1 at resource checks, swap 0; final free disk 155 GiB. Stub target is 1.2 GiB;
existing repo debug/release dirs are 247 MiB / 995 MiB. No foreign process/cache
cleanup, no loader-failure probes, no visit to worktree a200. No viewer/bundle,
OS window, CUA, GPU/pixel or osascript execution; no full STEP/FBX/GPU campaign.
The optional real-window comparator is N/A without window outputs. These are
local checks of an uncommitted diff, separate from the base's remote CI.

## Independent macOS window recipe

Author does not run viewer, bundle, CUA, GPU/pixels, osascript or loader-failure
probes. Reviewer: use one fresh arm64 bundle at pressure 1, the existing watchdog,
and a new external temporary directory. Source the native env directly in Bash
without login, and set `FERRITECAD_RECOVERY_DIR` and `FERRITECAD_TABS_DIR` to this
root's private folders. `APP` is the reviewer's fresh bundle from
`tools/macos-bundle.sh`; generator makes inputs only:

```sh
source target/30t-review/env.sh
ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ferrite-30u-window.XXXXXX")/run"
export FERRITECAD="$APP/Contents/MacOS/ferritecad"
python3 tools/close-tab-with-draft-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery" FERRITECAD_TABS_DIR="$ROOT/tabs"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/a.fcad"
```

1. In A, Edit extrusion → select its extrusion → height `18` → Apply. Open
   another height form and type ` 26x `, including the surrounding spaces.
2. Open B. Edit Circle → centre `-3.5`, `4.25`, radius `8`; leave it unapplied.
3. Press × on hidden A. It is shown first, then the form question names A.
   **Back to form**: exact ` 26x ` returns. × again → **Discard form and continue
   Close** → **Cancel** on the unsaved-model question: exact ` 26x ` returns, A
   stays dirty with accepted height 18. Return to B once and verify its own form.
4. In A correct the height to `26` → Apply. Export STL to `a-unsaved.stl` and FBX
   to `a-unsaved.fbx` in ROOT. Open another height form, type ` 99x `.
5. × → **Discard form and continue Close** → **Save**. Only A closes. B comes
   back with its original form, and the row contains B only. Cancel B's form and
   Quit; check the owned PID exited. Do not look up the app after Quit.
6. Run `python3 tools/close-tab-with-draft-gui.py --compare "$ROOT"`. Require
   `FCAD_30U_GUI_COMPARE_OK all_SQL_cells=true`. Independently read this real FBX
   with the pinned reader, and record memory peak, pressure, swap and exit.

The generator refuses a checkout root. The comparator refuses missing window
outputs or an unsaved A before starting Cargo/peer jobs and writes no positive
window artifact. Headless owner outputs are tested separately and never passed
as GUI evidence. Windows/Linux window interaction remains untested. The full
STEP/FBX/GPU corpus and native vendor rebuilds are outside this form slice; their
existing CI gates remain.
