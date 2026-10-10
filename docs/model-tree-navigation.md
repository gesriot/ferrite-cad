# §31A — Model history and existing editors

## Contract decided before code

Base after fetch: HEAD = main = origin/main =
`203cffd495218f01f56d553ea133ceb24cfb700f` (PR #103), clean checkout.
Branch `model-tree-navigation`; changes remain unstaged and uncommitted.

The projection belongs to `ExtrudeEditSource`, read once with the accepted scene
from its pinned private snapshot. `LoadedScene`/`PreparedLoad` carry it through the
existing scene acceptance statement; a refused preparation or bind changes neither
the projection nor navigation. Checkpoint-only retargeting replaces these facts too.
No logical-path read, SQLite work or rebuild occurs in a frame.

Body rows follow actual BodyTip and Predecessor dependencies backwards, then show
the chain in construction order. Feature profiles are explicitly labelled references;
Sketches and other stored objects remain in an independent objects group. Shared
profiles and shared history occurrences do not claim exclusive ownership. Siblings
are ordered by UUID for deterministic presentation, never used to infer history.
Imported and unknown objects remain visible with their type and editing limitation.
The existing STEP definitions, visibility controls and topology inspector remain.
Branched/cyclic history is not flattened into a fictional chain. If dependency
reading is refused, stored objects still appear with a history-unavailable reason;
the previous edit catalogue's per-route refusal policy is preserved.

Tabs owns window-only selection and expansion, keyed by runtime TabId and stored
ObjectId plus occurrence context (Body for history, feature for profile reference).
Two copied files sharing every UUID have independent navigation. Successful rebuild
retains an existing identity; disappearance clears it. Refused rebuild preserves it.
No navigation state is written to documents, recovery or LastTabs.

Select is navigation only, separate from geometry pick. An explicit editor action
names TabId, a never-reused scene acceptance epoch and row identity; the handler
also checks the current pinned snapshot's DocumentVersion against the catalogue.
Stale/foreign actions do nothing. Supported actions come from the same existing
typed edit catalogues.
Sketch geometry and constraints and Extrude height/Revolve angle remain distinct.
The handler and widget use one availability result: finish/cancel any open form first;
foreground work, New, dialogs and gestures retain their exclusions. No form is copied,
cancelled, applied or reconstructed by navigation; ordinary tab drafts remain §30P–W.

The panel is a bounded, scrolling side panel beneath the toolbar/tab strip. No
intermediate bodies or geometry highlights are built for an old history feature.
CLI, jobs, schema, FFI, solver, UUIDs and feature semantics are unchanged. No new
dependencies, persistence, rename/reorder/suppression or STEP assembly explorer.

## Verification

Executed locally on macOS arm64, with serial builds (`CARGO_BUILD_JOBS=1`) and
single-thread tests. Logs below are in the ignored `target/31a-check/` directory.
The exact existing workflow run bodies were executed through Bash argv, with normal
macOS matrix interpolation; each exact gate required one passed test, its execution
marker and no `skipped:`. No new workflow, dependency or weakened old gate.

New exact gates share the prefix
`sessions::tests::tabs::drafts::model_tree::`:

| Gate suffix | Execution marker | Configuration |
| --- | --- | --- |
| `history_uses_dependencies_exact_uuids_and_honest_shared_references` | `FCAD_31A_PROJECTION_EXECUTED cuts=16` | true stub + native |
| `accepted_snapshot_tab_identity_stale_actions_and_form_history_are_preserved` | `FCAD_31A_OWNERS_EXECUTED` | true stub + native |
| `navigation_open_uses_one_foreground_gesture_and_new_refusal` | `FCAD_31A_GUARDS_EXECUTED` | true stub + native |
| `real_widgets_select_uuid_then_send_the_supported_action_and_bound_the_list` | `FCAD_31A_WIDGETS_EXECUTED` | true stub + native |
| `native_early_cut_navigation_apply_history_save_matches_existing_cli` | `FCAD_31A_NATIVE_EXECUTED all_SQL_cells=true` | native |
| `native_fillet_chamfer_revolve_and_sketch_open_their_distinct_existing_routes` | `FCAD_31A_NATIVE_ROUTES_EXECUTED routes=5` | native |

Projection assertions use real stored UUIDs/dependencies, reversed SQL ordinals,
identical names, sixteen Cuts, shared history/profile references and an unknown
envelope. Real owners exercise two copies with identical DocumentId/object UUIDs,
stale tab/epoch/actions, accepted/refused scene binding, disappearing selection,
and an executable old Cut form's typed values, request UUID and draft Undo/Redo.
The kernel-free owner seam supplies an empty render scene with real pinned facts;
it is not geometric evidence. Real egui widgets select the early UUID, emit its
typed action, disable the next editor with the open-form reason and clip the list.

The native Cut gate uses the production scene/apply workers and old form widgets.
Early Cut radius changes to 1.625; SQL cells and old UUIDs/dependencies match the
existing same-source CLI operation. Only modified_at and explicitly matched new
topology-ref IDs are normalized by the existing comparator. Unsaved STL/FBX are
byte-equal, independent cold rebuild resolves refs and yields volume 51418.038517.
Undo/Redo, Save, fourth-Cut Add, disappearing selection on Undo and checkpoint
Restore all replace the projection on acceptance. Separate native fixtures open
Fillet radius, Chamfer distance, Revolve angle, Sketch vertices and constraints;
each keeps its own typed route and refuses a duplicate while its form is open.

| Executed check | Result | Log |
| --- | --- | --- |
| Existing stub CI body, ten §30W + four §31A exact gates | 14 executed, no skip | `packed-stub-final.log`, `packed-stub-result.log` |
| Existing native CI body, §30W + two §31A exact gates | 3 executed, no skip | `packed-native-final.log`, `packed-native-result.log` |
| Full `sessions::tests::tabs` block, including §30T/U/V/W | 97 green harness results in each configuration | `stub-tabs-final.log`, `native-tabs-final.log` |
| `ferritecad-ui --lib` | 110 passed, headless | `ui-final.log` |
| Document `edit::` filter | 47 passed; 1 ignored manual benchmark | `document-edit-final-repeat.log` |
| Existing `edits::tests` | 17 green harness results in each configuration | `edits-stub-final.log`, `edits-native-final.log` |
| `cargo fmt --all -- --check` | passed | `fmt-final.log` |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed on final Rust source | `clippy-final-repeat.log` |
| Licence headers / export boundary / solver ownership | passed; 482 tracked headers plus five new source headers | `licence-final.log`, `export-boundary-final.log`, `solver-ownership-final.log`, `untracked-source-final.log` |
| actionlint / shellcheck / Actions run-size / diff whitespace | passed; 203 run blocks, max 20432 bytes, packed argv 20463 < 21000 | `actionlint-final.log`, `shellcheck-final.log`, `run-size-final.log`, `diff-check-final.log` |
| Input-only generator + missing-output and checkout refusals | executed; no GUI output manufactured | `gui-generator-final.log`, `gui-missing-refusal-final.log`, `gui-checkout-refusal.log` |
| One small current FBX through existing pinned ufbx reader | identity/triangles checks=6, failures=0; STL match 532 triangles, worst 6.94e-18 m | `ufbx-pin-final.log`, `ufbx-reader-final.log`, `ufbx-identity-final.log`, `ufbx-triangles-final.log`, `ufbx-match-final.log` |

Shellcheck ran on the two actual Bash run bodies, with the shell specified and
SC2194 excluded because matrix interpolation makes `case 'macos'` constant.
The initial lint diagnostics are retained in `shellcheck-initial.log`; workflow
source also passed actionlint. No CI guard or lint policy was changed.

Full-block counts are harness counts, not counts of native executions. The stub
tabs block has thirteen explicit native skips; native-only returns in stub edits,
stub-only returns in native, conditional solver variants and opt-in window
comparators are N/A. Mixed OCCT/no-solver was not run: navigation does not change
solver availability or execute constraints; its native route only opens the existing
constraints form. The four new kernel-free gates executed in a true stub, proven
by disabled OpenCASCADE discovery and no OCCT/PlaneGCS imports in the actual test
executable (`stub-cache-final.log`, `stub-imports-final.log`). Native executable/CLI
are arm64; native imports include OCCT and PlaneGCS (`architecture-final.log`,
`native-imports-final.log`). No loader-failure opt-in was set.

Two directed compiling mutations were executed and caught by assertions. The
equivalent stale saved-projection mutant reinstated the old tree after scene
commit, and failed **tree must follow the accepted snapshot, never the old file**.
The positional-action mutant opened the first supported Cut instead of the named
UUID, and failed **navigation must open exactly the selected UUID** through the
real widgets/request. `mutations-final.log` records byte restoration SHA-256:
main.rs `7f106af8934e61ceeb37dfc8b86d2f76e11a9096aa6ad5d71cf9ae3c3754f43a`,
app/model_tree.rs `11cf6b1692dab96af736605e826f89e1824b1c57d4050a0f435d429b21fc9e0d`.
Failure logs: `mutation-stale-projection.log`, `mutation-position-instead-of-uuid.log`;
positive repeats are the final exact packed stub gates and full native block.
No mutation framework was added.

An existing document test found a real regression: an unknown dependency role made
the new projection abort the whole edit catalogue. Stored-object fallback now
preserves its previous partial-refusal policy; the failed and corrected runs are
`document-edit-final.log` and `document-edit-final-repeat.log`. The generator's
first run assumed a dump-graph JSON option; corrected to the existing text route.
Three native headless comparator controls (missing FBX, altered SQL, stale export)
also fail with the expected reasons. Their caught assertion output in the positive
native log is deliberate; the exact test ends with one passed result.

Validated existing `target/30t-review/env.sh` and `target/30u-check/stub-env.sh` paths,
using only common target and existing `target/30u-stub`. Pinned OCCT/PlaneGCS were
not rebuilt; no third large target, large STEP corpus, foreign process cleanup or
foreign worktree access. Sampled pressure remained 1, swap 0 MiB, free disk about
154 GiB; no per-process peak was collected. Final resource/process readings:
`resources-final.log`, `processes-final.log`. No own cargo/rustc/viewer remained.

Base CI was read twice: latest `base-ci-final.json` reports 25 successful checks
and one Windows runtime-layout job still in progress (run 38067349815). This is
not a claim of CI for the uncommitted diff. HEAD/main/origin/main and read-only
remote main lookup still equal the base SHA (`remote-main-final.txt`). Full
modified/untracked manifest, per-file diffstat and source hashes:
`status-final.txt`, `diffstat-all.txt`, `source-hashes-final.txt` in the log directory.

Author runs no viewer, CUA, bundle, osascript, GPU/pixel or window test. Headless
widget/owner tests are not GUI. §30, Milestone 5C, product and OOM remain open.
The real-window comparator is prepared but its positive execution is N/A here.
Input-only fixtures are `/private/tmp/ferrite-31a-input-only.7RCh2y/run`; missing
actual exports/LastTabs are correctly refused. Window layout, real focus/dialog
interactions and non-macOS window evidence remain for independent review. The
panel is not a full STEP assembly explorer or a general editor for unsupported
objects; linear history is intentionally refused for branching/cyclic dependencies.
No next slice was started.

## Independent macOS window recipe (author does not run it)

Use a fresh arm64 bundle through the existing staging route, pressure 1, and a
Bash shell without login. Inputs are created outside checkout; no positive window
outputs are created by this generator. Use the existing watchdog and separate state:

```sh
source target/30t-review/env.sh
ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ferrite-31a-window.XXXXXX")/run"
export FERRITECAD="$APP/Contents/MacOS/ferritecad"
python3 tools/model-tree-navigation-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery" FERRITECAD_TABS_DIR="$ROOT/tabs"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/work/cuts.fcad"
```

1. Expand/collapse Body and its feature profile references in **Model**. Compare
   the UUIDs and construction order with `facts.json`'s existing inspect/dump-graph
   output. Select the first Cut, expand UUID, explicitly **Edit Cut…**: its radius
   is 1.375. Type 1.625, **Confirm draft numbers**, use the form's Undo/Redo.
   Select the base Extrude: **Edit height…** is unavailable with the open-form
   reason; the Cut's typed numbers and history remain. Re-select the Cut.
2. **Open…** `work/copy.fcad`. It has the same DocumentId and object UUIDs but an
   independent selection/expansion. Return to cuts: the original Cut form remains
   executable, including Undo/Redo. **Apply cut**; require the same early UUID still
   selected and the form closed. Export `unsaved.stl` and `unsaved.fbx` in ROOT
   before Save. Document **Undo**, export `undo.stl`; **Redo**, then **Save**.
3. Open `work/fillets.fcad`, then `work/chamfer.fcad`, then `work/revolve.fcad`, in
   that order. Select the stored Fillet/Chamfer/Revolve and explicitly open their
   radius/distance/angle editor; verify the stored values, then Cancel. Select the
   Revolve's Sketch reference: vertices and constraints are distinct actions;
   open each in turn and Cancel. Simple selection must change no geometry pick.
   Existing definitions, Frame/Hide/Isolate and topology inspector remain reachable.
4. Return to cuts and **Quit** normally. The five named tabs remain in their opening
   order in LastTabs, with cuts shown. Require the watchdog's own PID to exit, without
   looking it up or relaunching after exit. Record interactions, peak footprint,
   pressure/swap and exit separately from comparator evidence.
5. Build the current peer CLI if needed, then compare the actual output files:

```sh
source target/30t-review/env.sh
cargo build --release --features planegcs -p ferritecad-cli
python3 tools/model-tree-navigation-gui.py --compare "$ROOT"
```

Require `FCAD_31A_GUI_COMPARE_OK all_SQL_cells=true controls=3`, not a zero-test
success or an opt-in return. The comparator preflights actual files before peer
jobs, compares saved SQL/UUID/refs and unsaved STL/FBX to the same-source CLI edit
of the early Cut, checks Undo against the original, cold rebuild and references,
unchanged copies/other models, recovery cleanup and actual LastTabs file identities.
`/tmp` and `/private/tmp` aliases are compared by canonical file identity. Three
negative controls require missing FBX, altered SQL and a stale export to fail.
Headless owner outputs never substitute for these real window outputs.
