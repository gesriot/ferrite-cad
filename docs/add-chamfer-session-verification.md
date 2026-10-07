# §30K — verification

[Contract](add-chamfer-session.md). Marker `FCAD_30K_ADD_CHAMFER_SESSION`.
Base `870107f00243505cef935981b9dacca137616b41` (merge of PR #90), tree
`37f16d8eab5a28d97cd5ed25f3cb8feb932c2ab6`. `origin/main` after fetch; PR #90 MERGED
at that commit; clean tree. Branch `add-chamfer-session`. Author/committer
`gesriot <gessman1618@gmail.com>`. No merge or auto-merge. Post-merge base CI, the
code/workflow head and the docs-only head are recorded separately below.

## What the gates prove

Three real egui widget gates use the handler's production predicate
`can_add_chamfer`; each busy mode is checked only after the same idle request was
shown to be offered. Fixture: §30G's asymmetric, translated plate
`(33,15.5),(33,3.25),(-4.5,3.25),(-4.5,15.5)` × 6.75 mm with reversed object
rowids/ordinals, every name `same`, offset capability rowids, an optional capability
and an extra SQL table (no Fillet).

- `add_chamfer_reads_current_fields_request_history_stays_separate_and_copy_is_clean_only`:
  the form paints **Add chamfer**, **Confirm draft edge and distance**, **Undo
  request**, **Redo request**, the title *Chamfer one vertical edge* and the document
  line, never *Apply chamfer*, *Apply distance* (whose predicate stays closed for this
  form) or *new copy*. With the predicate off a press asks nothing; on, one press is
  one request with the unconfirmed corner (base feature + Line joint) and distance
  text `1.06250`, the saved Body UUID, the accepted path and version, and it confirms
  nothing. Confirm records only the form's request history; Undo/Redo request move
  only the fields; an Add after Undo request reads the restored fields and keeps the
  request Redo. The clean copy is offered for the confirmed request; on a dirty
  session the copy is disabled with words and Add still adds.
- `add_chamfer_busy_guard_proves_idle_request_then_excludes_other_work_and_forms`:
  load, a session step, export, an edit worker and another open form each close the
  guard; a running form ignores a real pointer press; the saved Sketch editor, no
  session and a dismissed form never offer Add.
- `add_chamfer_dirty_discovery_refusals_keep_draft_status_dismissal_and_closed_classes`:
  on a dirty session the form opens only when the window says the session is idle, on
  the accepted private snapshot; no corner, not a number, 0.0005 mm and 12.2401 mm
  (shorter side 12.25 less 0.01) ask nothing, keep the corner and the exact text and
  show the document's own reason; 12.24 is the bound itself and is asked unclamped;
  the document line is drawn inside the form; acceptance dismisses it and keeps the
  window's word; a plate with a Chamfer offers no second one; a plate whose Sketch
  carries a written Vertical constraint is not a creation target and gives the
  domain's "free or closure-only plate" reason.

The §30H gate `distance_dirty_discovery_copy_reason_refusal_retains_text_and_acceptance_closes_form`
now asserts that the Add form opens while dirty and idle (it was closed before §30K),
and `distance_current_text_draft_history_noop_clean_copy_and_add_exclusion` presses the
renamed **Confirm draft edge and distance** and checks that **Add chamfer** is not the
distance form's Apply.

Native gates run the session's real worker (`spawn_add_chamfer` →
`StepTicket::add_edge_chamfer` → `chamfer_edge_copy`) and the shipped CLI as peer.

`native_dirty_base_add_distance_history_save_bound_and_branch_match_cli`:

- Dirty base first: height 9.25 and left wall X −4.5 → −5.75, each equal to its CLI
  peer in every SQL cell (plate 38.75 × 12.25 × 9.25 mm).
- One Add through the form's own widgets without Confirm: corner (−5.75, 15.5),
  d 2.375. The request names the saved Body, the accepted version and the accepted
  snapshot; the user's file is untouched; the allowlist holds (every old object row
  equal except the Body, whose tip is the new Chamfer; the new Chamfer's `previous` is
  the old tip, the base Extrude, and its edge is exactly the requested producer + Line
  joint; every old reference unchanged).
- Every SQL cell (rowid included, only `meta.modified_at` set aside, raw payload
  hashes checked) equals `chamfer-edge-copy` (joint written in the other order) on the
  same accepted snapshot and on an independent CLI chain, under a proved bijection of
  new identities only: the one new Chamfer, then each new reference by all its other
  cells. No old UUID, whole payload, row order or name is normalized; mapping an old
  identity fails.
- A second Add (another corner) is refused with the domain's words ("… and a second
  Chamfer are not supported yet"); nothing moves.
- The new Chamfer's distance is applied at once through §30H's own form and worker
  (d 3.0625) and matches `edit-chamfer-distance` with its UUID translated only
  through the bijection.
- Unsaved STL/FBX bytes equal the CLI peer's; the B-Rep's named Chamfer face is the
  plane of that corner with 3.0625 along both faces, area `d·√2·h`, volume
  `(W·D − d²/2)·h` = 4347.481933594 mm³; the STL is closed and oriented and its
  signed volume agrees.
- Undo through every step (distance, Add, vertices, height) and Redo back compare all
  SQL cells with **no** mapping; the undone Chamfer leaves discovery and returns with
  the same UUIDs; the user's file is unchanged until Save; Save writes it; pairing
  from the saved file alone gives exactly the step-by-step bijection; cold rebuild
  and reopen are clean.
- Undo to the pre-Add state: stale form, 12.2401 ("too large"), 0.0005 ("at least
  0.001 mm"); then six modes — cancel after the answer, stale answer, failed scene
  preparation, failed GPU preparation, cancel during binding, cancel before the
  answer. Each keeps the accepted picture, export path, dirty, Undo/Redo, saved
  version, private files, the exact Add draft and the saved bytes.
- Another corner after Undo, (33, 3.25) d 1.5, is accepted and drops Redo; Save As
  writes the branch and keeps the previous file; the branch matches its CLI peer in
  all cells and exports byte-equal STL/FBX; volume 4380.453125 mm³.

`native_every_corner_adds_its_own_plane_like_the_cli`: each of the four corners on its
own fresh plate — (33, 15.5) d 0.75, (33, 3.25) d 1.5, (−4.5, 3.25) d 2.25,
(−4.5, 15.5) d 3.0625 — is added through the session, satisfies the allowlist, equals
the CLI on the same snapshot in every cell under the new-identity bijection, and its
B-Rep Chamfer face is that corner's plane with that distance along both faces.

`native_constraints_close_add_but_constraints_and_distance_after_add_still_apply`:
after an accepted unsaved constraint Apply (the §30H set: V/H on all Lines, a fixed
corner, depth and width 41 mm) the plate is no creation target: discovery carries the
domain's "free or closure-only plate" reason, the form is not offered, and an Add
request is refused with the same reason, the draft, state and file kept. After an
accepted Add on the free plate the same constraint Apply and an Apply distance of
13.5 (above the stored bound 12.24, within the solved 14.24) still work exactly like
the CLI on the same snapshot; cold rebuild uses the solved plate (−8, 1.25)–(33, 15.5).

`native_window_scenario_on_session_files_passes_the_comparator_and_its_controls` runs
the window recipe below through the session's own forms and workers, lays the files
out as the window would and runs the comparator with its seven controls. **That is a
self-check of the comparator, not window evidence.**

`stub_add_chamfer_refuses_without_publication` (no kernel: refusal, no file, nothing
accepted); `mixed_add_chamfer_uses_occt_without_solver` (the whole native history
gate above with OCCT and no PlaneGCS, the CLI peers built without the solver).

Artifacts `add-chamfer-history.{stl,fbx}` and `add-chamfer-history-branch.{stl,fbx}`
are read by the pinned strict ufbx reader (`--identity` checks=6 failures=0,
`--triangles`) and joined with oriented STL triangles; runtime CI requires
`FCAD_ADD_CHAMFER_SESSION_UFBX_EXECUTED`.

## CI wiring

Older gates stay; new exact-name, no-skip gates are added to existing steps:

- ci.yml *Open, edit, Undo, Redo and Save one document without native geometry*:
  the three widget gates and `stub_add_chamfer_refuses_without_publication`.
- runtime-layout.yml native session step: the four `native_*` gates with
  `FCAD_ADD_CHAMFER_SESSION_ARTIFACTS`, both artifact pairs required, exported as
  `FCAD_ADD_CHAMFER_SESSION_FBX_DIR`.
- runtime-layout.yml no-solver step: `mixed_add_chamfer_uses_occt_without_solver`.
- tools/check-fbx-complex.sh: the strict reader loop over both artifact pairs; the
  runtime step greps its marker.

Nothing prints inside a gated test (negative controls run under a silent panic hook).

## Directed mutations

M30K-1 replaces only `self.check_form_version(expected)?;` in
`StepTicket::add_edge_chamfer` with `let _ = expected;`. It compiles; the native
history gate fails its executed stale-form refusal (`left: Show(…)`, `right: Failed`,
status "Applied. Undo is available; Save writes the file."). M30K-2 drops only
`self.can_add &&` from the Add button's enablement in `chamfers.rs`. It compiles; the
widget gates fail their executed assertions "disabled asked" and busy `mode 0`. Each
file was saved first, restored byte-for-byte (SHA-256 verified) and the positive gates
rerun green.

## Real window recipe

Use a freshly staged bundle and exactly one owned viewer/watchdog; never a CLI
stand-in for a window output. `APP` is that bundle; choose a new root outside any
checkout. About 15 minutes.

```sh
FCAD_30K_GUI_ROOT=/private/tmp/ferrite-30k-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/add-chamfer-session-gui.py "$FCAD_30K_GUI_ROOT"
python3 tools/watch-viewer-memory.py --log "$FCAD_30K_GUI_ROOT/watch.jsonl" \
 --limit-mib 1536 -- "$APP/Contents/MacOS/ferritecad-viewer"
```

The generator writes only `source/plate.fcad`, `work/plate.fcad`, its request and
`facts.json`; it refuses a destination inside a checkout. All captures go into the root.

1. Open `work/plate.fcad` (clean). **Chamfer edge of … — Body UUID…**: corner
   (33, 3.25), distance 1.5; **Confirm draft edge and distance**; distance 2;
   Confirm; **Undo request** (1.5 returns), **Redo request** (2 returns). The title
   never shows unsaved changes; Undo/Redo in the document menu stay unavailable.
   **Save chamfer copy…** opens the native dialog: Cancel. Cancel the chamfer draft.
   Copy `work/plate.fcad` to `after-confirm.fcad`.
2. Apply height 9.25. Edit base Sketch: both X −4.5 → −5.75; Apply vertices.
3. Chamfer edge: corner (−5.75, 15.5), distance 2.375; **Add chamfer** without
   confirming. No dialog; the form closes; the title shows unsaved changes.
4. **Edit Chamfer distance** of the new Chamfer: 3.0625; **Apply distance**. Copy the
   still-original `work/plate.fcad` to `after-add.fcad`.
5. Export `unsaved.stl` and `unsaved.fbx`. Document Undo once; export `undo.stl`.
   Undo to the opened state (three more), Redo all four, Save. Copy
   `work/plate.fcad` to `saved.fcad`.
6. Document Undo twice (distance and Add): no Chamfer, the dirty base kept. Chamfer
   edge: corner (−5.75, 15.5), distance 12.2401: **Add chamfer** stays disabled with
   "too large", the corner and text stay. Copy `work/plate.fcad` to
   `after-refusal.fcad`.
7. Corner (33, 3.25), distance 1.5; Add chamfer; Redo disappears. Save As
   `branch.fcad` in the root; copy `work/plate.fcad` to `after-saveas.fcad`.
8. Quit. Check only the owned PID/watchdog.

The clean-copy dialog is exercised in step 1 because it is offered only on a clean
document without a Chamfer: after Save the plate has its Chamfer (no Add form), and
after Undo to pre-Add the document is dirty (copy disabled with its reason).

```sh
python3 tools/add-chamfer-session-gui.py --compare "$FCAD_30K_GUI_ROOT"
```

`--compare` refuses (`FCAD_30K_GUI_COMPARE_REFUSED`) before any build or peer job
when an output is missing; the Rust comparator refuses again before its first peer job
(proved by the peer-run counter). It builds a temporary CLI chain from the untouched
source (height, vertices, Add, distance; and the branch Add on the base) and requires
`FCAD_30K_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true`. Each control breaks
one compared fact on a copy of the root and must be refused for that reason: missing
output, original instead of Save, branch instead of Save, Save instead of branch,
stale export, Confirm/copy-dialog that wrote the file, and a new reference's owner.
Read the actual `unsaved.fbx` with the pinned strict reader and join it with
`unsaved.stl`.

## Execution record (cloud, Linux x86_64)

Rust 1.96.0, existing pinned OCCT/PlaneGCS builds and existing targets,
`CARGO_BUILD_JOBS=2`, sequential builds. Local runs used the debug profile (no third
large target); CI runs release. No window and no GPU were run here.

- `cargo test -p ferritecad-app -p ferritecad-jobs` (OCCT + PlaneGCS): 506 app tests and
  every jobs test passed; `cargo clippy --workspace --all-targets --all-features
  -D warnings` and `cargo fmt --all --check` clean.
- Packed run blocks extracted from the workflows, executed verbatim except
  `matrix.name` → `linux`, `--release` dropped and (native, mixed) `target/release` →
  `target/debug`; the native block's library path pointed at the local PlaneGCS:
  - stub (ci.yml, existing stub target, native/solver env unset): exit 0, 112 exact
    gates ok, the four new ones included;
  - native session step: exit 0, 34 exact gates ok, the four new ones included, both
    artifact pairs exported;
  - no-solver step (no-solver CLI built first): exit 0, 16 exact gates ok, the new
    mixed gate included.
- `tools/check-fbx-complex.sh` with clang (as CI): exit 0, both §30K artifact pairs
  `checks=6 failures=0`, STL joins 16 triangles each, worst 4.34e-19 m,
  `FCAD_ADD_CHAMFER_SESSION_UFBX_EXECUTED`.
- Both directed mutations failed executed assertions and were restored as above.
- Generator: refused a checkout destination; produced inputs; `--compare` refused the
  missing window outputs before any build.

**Not executed here:** the real window recipe (left for the independent macOS GUI
stage), macOS/Windows native runs (CI only), release-profile local runs.

### Base CI (post-merge `870107f`)

CI 37684553204, planegcs pin 37684552864, product sbom 37684554227, rust sbom
37684553011 and rust notices 37684552846: success. Combined runtime layout
37684552867: linux 113008883196, macOS 113008883269, Windows 113008883413 and the
cross-platform compare 113046545281 success (the run was still in progress when the
code head was pushed and is counted only from its completion).

### PR #91 CI

Code/workflow head `cf0ec9401deed400f7ec75539f661573d23866e3`: CI 37689720322 success
(lint, sbom, supply-chain, notices, test on ubuntu/macos/windows); planegcs pin
37689689646 success (three platforms and compare 113030059416). Combined runtime
layout 37689689488, attempt 1: linux 113026395239, macOS 113026395103, Windows
113026394841 and *Compare what the three platforms measured* 113060305181 all success,
run success. On every platform the no-solver step (mixed Add-chamfer gate), the native
session step (four native Add-chamfer gates) and the pinned-ufbx step succeeded; each
gate step fails unless the exact `test <gate> ... ok` line is present without
`skipped:`. The log tails read here contain `FCAD_ADD_CHAMFER_SESSION_UFBX_EXECUTED`
on linux, macOS and Windows. Access limit: the GitHub tool returns only the last ~5000
log lines and the full-log host is denied by this environment's network policy, so the
earlier `test … ok` lines were established by step conclusion, not read verbatim.

This docs-only head changes no code or workflow; its CI is reported on the PR.


## Independent macOS review (2026-10-07)

Reviewed PR #91 at `046c34d4c288f5eee3685e3791864762fc63c7b7` against
`870107f00243505cef935981b9dacca137616b41`. No blocking code finding and no
production changes were needed. This review commit changes only documentation.

The full GitHub CI and runtime logs were downloaded independently, closing the
cloud log-access limitation above. On the exact code/workflow head `cf0ec940`,
all 15 checks succeeded. All 27 new exact executions were read verbatim: three
widget gates plus the stub gate, four native gates and the mixed gate on each
of Linux, macOS and Windows. Each platform's strict-reader step completed with
`FCAD_ADD_CHAMFER_SESSION_UFBX_EXECUTED`. The incoming docs head's seven checks
also succeeded; they are separate from the code-head native evidence.

Local arm64 verification used the existing pinned OCCT/PlaneGCS libraries,
`/private/tmp/ferrite-24b-native-target`, two build jobs and sequential builds.
Fresh release CLI/viewer, fmt and workspace clippy (all targets/features,
`-D warnings`) passed. All nine `chamfers::tests::` tests and four native
Add-chamfer session tests executed successfully. The real-output comparator
initially reported its explicit missing-artifact skip; that was not counted as
execution. After the window run it executed once and passed.

The freshly staged and signed arm64 bundle under
`/private/tmp/ferrite-pr91-review/gui/layout/FerriteCAD.app` completed all eight
window steps above via CUA. Observed: request Confirm/Undo/Redo left the clean
model alone; native copy Save Cancel retained the draft; height and vertices
Apply, Add without Confirm and subsequent distance Apply used one document
history; unsaved STL/FBX export, four Undo/Redo steps, Save, the disabled
12.2401 mm refusal with retained corner/text, branched Add and Save As all
behaved as specified. The title returned to clean after Undo to the opened
state and after each save; a branched Add cleared Redo.

Only the input generator created fixtures. The nine actual window outputs in
`/private/tmp/ferrite-pr91-review/gui-models` passed
`FCAD_30K_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true`. Source and
pre-Save captures stayed unchanged; Save As preserved the former destination.
The actual GUI FBX and both native artifact FBX files passed pinned ufbx 0.23.0
strict (`checks=6 failures=0` each). Their oriented STL joins each matched all
16 triangles, worst difference `4.34e-19` metres.

The one owned viewer PID 54882 ran for 354.38 seconds under the 1536 MiB watchdog,
peaked at 211.689 MiB, observed only normal pressure (1), and exited normally
with code 0. System swap decreased from 1561.5 to 1553.5 MiB; no attribution of
that system-wide change to the viewer is made. After Cmd+Q, process inspection
confirmed the PID absent; no further viewer CUA lookup or relaunch occurred.
Evidence: `watch.jsonl`, `memory-summary.json`, `gui-compare.log`, native logs,
and `code-ci-evidence.json` under `/private/tmp/ferrite-pr91-review`.

The historical OOM remains unexplained. This review does not add a constrained
Chamfer creation class, complete §30/5C, or claim a Windows/Linux window test.
