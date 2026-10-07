# §30J — verification

[Contract](add-fillet-session.md). Marker `FCAD_30J_ADD_FILLET_SESSION`.
Base `3ea0b4e96fbdb3cc8c78b9f89fd67af388f62fb6` (merge of PR #89), tree
`8980c358b60f3d7f8e6efaf9f5557977be6f280e`. `origin/main` after fetch; PR #89 MERGED
at that commit; clean tree. Branch `add-fillet-session`. Author/committer
`gesriot <gessman1618@gmail.com>`. No merge or auto-merge. Post-merge base CI, the
code/workflow head and the docs-only head are recorded separately below.

## What the gates prove

Three real egui widget gates use the handler's production predicate
`can_add_fillet`; each busy mode is checked only after the same idle request was
shown to be offered. Fixture: §30G's asymmetric, translated plate
`(33,15.5),(33,3.25),(-4.5,3.25),(-4.5,15.5)` × 6.75 mm with reversed object
rowids/ordinals, every name `same`, offset capability rowids, an optional capability
and an extra SQL table.

- `add_fillet_reads_current_selection_confirm_is_draft_only_and_copy_is_clean_only`:
  the form paints **Add fillet**, **Confirm draft edge and radius**, the title
  *Fillet one vertical edge* and the document line, never *Apply radius* (whose
  predicate stays closed for this form) or *new copy*. With the predicate off a press
  asks nothing; on, one press is one request with the unconfirmed corner (by base
  feature and Line joint) and radius text `3.06250`, the saved Body UUID, the
  accepted path and version. Confirm only records the draft for the copy; the clean
  copy is offered; on a dirty session the copy is disabled with words and Add still adds.
- `add_fillet_busy_guard_proves_idle_request_then_excludes_other_work_and_forms`:
  load, a session step, export, an edit worker and another open form each close the
  guard; a running form ignores a real pointer press; the radius form, the saved
  Sketch editor, no session and a dismissed form never offer Add.
- `add_fillet_dirty_discovery_refusals_keep_draft_status_dismissal_and_full_plate`:
  on a dirty session the form opens only when the window says the session is idle,
  on the accepted private snapshot; a rounded corner is not offered; no corner, not a
  number, below 0.01 mm and above half the shorter side ask nothing, keep the corner
  and the exact text and show the document's own reason; the document line is drawn
  inside the form; acceptance dismisses it and keeps the window's word; with four
  Fillets no Add is offered.

Native gates run the session's real worker (`spawn_add_fillet` →
`StepTicket::add_edge_fillet` → `fillet_edge_copy`) and the shipped CLI as peer.

`native_dirty_base_four_adds_radius_edit_history_save_branch_match_cli`:

- Dirty base first: height 9.25 and left wall X −4.5 → −5.75, each equal to its CLI
  peer in every SQL cell.
- Four Adds through the form's own widgets without Confirm: (33, 3.25) r2.375,
  (−5.75, 15.5) r3.0625, (33, 15.5) r1.5 (shares a Line with the first), (−5.75, 3.25)
  r4.25. Each request names the saved Body, the accepted version and the accepted
  snapshot; the form closes on acceptance; the user's file is untouched; the
  allowlist holds (every old object row equal except the Body, whose tip is the new
  Fillet; the new Fillet's `previous` is the old tip and its edge is exactly the
  requested producer + Line joint; every old reference unchanged); Edit Fillet radius
  offers it last in the history.
- Every SQL cell (rowid included, only `meta.modified_at` set aside, raw payload
  hashes checked) equals `fillet-edge-copy` (joint written in the other order) on the
  same accepted snapshot and on an independent CLI chain, under a proved bijection of
  new identities only: new Fillets by their place in the history (`previous`), then
  each new reference by all its other cells. No old UUID, whole payload, row order or
  name is normalized; mapping an old identity fails. Pairing all four from the saved
  file gives exactly the step-by-step bijection.
- A fifth is refused ("every corner of this plate is already rounded").
- The newest Fillet is edited at once through §30G's own form and worker (r 4.5) and
  matches `edit-fillet-radius` with its UUID translated only through the bijection.
- Unsaved STL/FBX bytes equal the CLI peer's; independent B-Rep and signed STL volume
  agree with the analytic value; a closed, oriented STL; and a per-corner
  quarter-circle fit checks each Fillet's own corner and radius, so swapped radii
  with an equal total volume would fail.
- Undo through every step and Redo back compare all SQL cells with **no** mapping;
  undone Fillets leave discovery and return with the same UUIDs; Save writes the file;
  cold rebuild and reopen are clean.
- From three Fillets (Undo of the edit and the fourth Add): stale form, the same
  corner again (refused naming the Fillet that rounds it), r 6.2 ("too large"); then
  six modes — cancel after the answer, stale answer, failed scene preparation, failed
  GPU preparation, cancel during binding, cancel before the answer. Each keeps the
  accepted picture, export path, dirty, Undo/Redo, saved version, private files, the
  exact Add draft and the saved bytes.
- An Add after Undo is accepted and drops Redo; Save As writes the branch and keeps
  the previous file; the branch matches its CLI peer in all cells and exports
  byte-equal STL/FBX.

`native_add_on_unsaved_constraints_is_measured_on_solved_lines`: after an accepted
unsaved constraint Apply (V/H on all Lines, a fixed corner, depth 10 mm, width 41 mm;
stored drawing 37.5 × 12.25 mm), the form sees a constrained plate. r 5.5 at (33, 15.5)
is allowed by the stored guess (≤ 6.125) but refused by the evaluator on the solved
plate (≤ 5) with the draft and state kept; r 4.75 is accepted, equals the CLI on the
same snapshot, and B-Rep, STL and corner fit use the solved 41 × 10 mm plate. No
second solver or validator.

`native_window_scenario_on_session_files_passes_the_comparator_and_its_controls` runs
the window recipe below through the session's own forms and workers, lays the files
out as the window would and runs the comparator with its seven controls. **That is a
self-check of the comparator, not window evidence.**

`stub_add_fillet_refuses_without_publication` (no kernel: refusal, no file, nothing
accepted); `mixed_add_fillet_uses_occt_without_solver` (the whole native history gate
with OCCT and no PlaneGCS, and a constrained plate refused without the solver).

Artifacts `add-fillet-history.{stl,fbx}` and `add-fillet-history-branch.{stl,fbx}` are
read by the pinned strict ufbx reader (`--identity` checks=6 failures=0,
`--triangles`) and joined with oriented STL triangles; runtime CI requires
`FCAD_ADD_FILLET_SESSION_UFBX_EXECUTED`.

## CI wiring

Older gates stay; new exact-name, no-skip gates are added to existing steps:

- ci.yml *Open, edit, Undo, Redo and Save one document without native geometry*:
  the three widget gates and `stub_add_fillet_refuses_without_publication`.
- runtime-layout.yml native session step: the three `native_*` gates with
  `FCAD_ADD_FILLET_SESSION_ARTIFACTS`, both artifact pairs required, exported as
  `FCAD_ADD_FILLET_SESSION_FBX_DIR`.
- runtime-layout.yml no-solver step: `mixed_add_fillet_uses_occt_without_solver`.
- tools/check-fbx-complex.sh: the strict reader loop over both artifact pairs.

Nothing prints inside a gated test (negative controls run under a silent panic hook),
so the exact `test <gate> ... ok` line cannot be split as in §30I's first head.

## Directed mutations

M30J-1 replaces only `self.check_form_version(expected)?;` in
`StepTicket::add_edge_fillet` with `let _ = expected;`. It compiles; the native
history gate fails its executed stale-form refusal (`left: Show(…)`, `right: Failed`).
M30J-2 drops only `self.can_add &&` from the Add button's enablement in `fillets.rs`.
It compiles; the widget gates fail their executed assertions "disabled asked" and
`busy mode`'s no-request check. Each file was saved first, restored byte-for-byte
(SHA-256 verified) and the positive gates rerun green.

## Real window recipe

Use a freshly staged bundle and exactly one owned viewer/watchdog; never a CLI
stand-in for a window output. `APP` is that bundle; choose a new root outside any
checkout. About 15 minutes.

```sh
FCAD_30J_GUI_ROOT=/private/tmp/ferrite-30j-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/add-fillet-session-gui.py "$FCAD_30J_GUI_ROOT"
python3 tools/watch-viewer-memory.py --log "$FCAD_30J_GUI_ROOT/watch.jsonl" \
 --limit-mib 1536 -- "$APP/Contents/MacOS/ferritecad-viewer"
```

The generator writes only `source/plate.fcad`, `work/plate.fcad`, its request and
`facts.json`; it refuses a destination inside a checkout. All captures go into the root.

1. Open `work/plate.fcad`. Apply height 9.25. Edit base Sketch: both X −4.5 → −5.75;
   Apply vertices.
2. **Fillet edge of … — UUID…**: corner (33, 3.25), radius 2.375; **Add fillet** without
   confirming. No dialog; the form closes; the title shows unsaved changes.
3. **Edit Fillet radius** of the new Fillet: 2.75; **Apply radius**.
4. Fillet edge: corner (33, 15.5) (shares the right Line), radius 1.5; Add fillet.
   Copy the still-original `work/plate.fcad` to `after-add.fcad`.
5. Export `unsaved.stl` and `unsaved.fbx`. Document Undo once; export `undo.stl`.
   Undo to the opened state, Redo all, Save. Copy `work/plate.fcad` to `saved.fcad`.
6. Fillet edge: corner (−5.75, 3.25), radius 6.2: Add fillet stays disabled with
   "too large", the corner and text stay. The document is clean after Save, so **Save
   fillet copy…** appears only after **Confirm draft edge and radius**. Copy
   `work/plate.fcad` to `after-refusal.fcad`.
   Cancel the draft.
7. Document Undo once (the second Add). Fillet edge: corner (−5.75, 15.5), radius
   3.0625; Add fillet; Redo disappears. Save As `branch.fcad` in the root; copy
   `work/plate.fcad` to `after-saveas.fcad`.
8. Quit. Check only the owned PID/watchdog.

```sh
python3 tools/add-fillet-session-gui.py --compare "$FCAD_30J_GUI_ROOT"
```

`--compare` refuses (`FCAD_30J_GUI_COMPARE_REFUSED`) before any build or peer job
when an output is missing; the Rust comparator refuses again before its first peer job
(proved by the peer-run counter). It builds a temporary CLI chain from the untouched
source and requires `FCAD_30J_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true`.
Each control breaks one compared fact on a copy of the root and must be refused for
that reason: missing output, original instead of Save, branch instead of Save, Save
instead of branch, stale export, a refusal that wrote the file, and a new reference's
owner. Read the actual `unsaved.fbx` with the pinned strict reader and join it with
`unsaved.stl`.

## Execution record (cloud, Linux x86_64)

Rust 1.96.0, existing pinned OCCT/PlaneGCS builds and existing targets,
`CARGO_BUILD_JOBS=2`, sequential builds. Local runs used the debug profile (no third
large target); CI runs release. No window and no GPU were run here.

- `cargo test -p ferritecad-app -p ferritecad-jobs` (OCCT + PlaneGCS): 496 app tests and
  every jobs test passed; `cargo clippy --workspace --all-targets --all-features
  -D warnings` and `cargo fmt --all --check` clean.
- Packed run blocks extracted from the workflows, executed verbatim except
  `matrix.name` → `linux`, `--release` dropped and (mixed) `target/release` →
  `target/debug`; the native block's library path pointed at the local PlaneGCS:
  - stub (ci.yml, existing stub target, native/solver env unset): exit 0, 108 exact
    gates ok, the four new ones included;
  - native session step: exit 0, 30 exact gates ok, the three new ones included, both
    artifact pairs exported;
  - no-solver step: exit 0, 15 exact gates ok, both new mixed gates included.
- `tools/check-fbx-complex.sh` with clang (as CI): exit 0, both artifact pairs
  `checks=6 failures=0`, STL joins 228 and 216 triangles, worst 1.73e-18 m,
  `FCAD_ADD_FILLET_SESSION_UFBX_EXECUTED`. gcc refuses the pinned reader under
  `-Werror` (an unused variable gcc reports and clang does not); not a §30J change.
- Both directed mutations failed executed assertions and were restored as above.
- Generator: refused a checkout destination; produced inputs whose catalogue offers
  the Add; `--compare` refused the missing window outputs.

**Not executed here:** the real window recipe (left for the independent macOS GUI
stage), macOS/Windows native runs (CI only), release-profile local runs.

### Base CI (post-merge `3ea0b4e`)

CI 37636781158, planegcs pin 37636781159, product sbom 37636781272, rust sbom
37636781236 and rust notices 37636781131: success. Combined runtime layout
37636781102: macOS and Linux success; Windows still in progress when this was
written, so it is not claimed passed.
