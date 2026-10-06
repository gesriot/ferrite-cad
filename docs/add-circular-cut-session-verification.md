# §30I — verification

[Contract](add-circular-cut-session.md). Marker `FCAD_30I_ADD_CIRCULAR_CUT_SESSION`.
Base `85a731cd0c48ec4bf3087b0a0e9a5e1483f9fb13` (merge of PR #88), tree
`3ddba9f046c883acb062883ed429b850e9e10353`. `main == origin/main` after fetch;
PR #88 MERGED at that commit. Branch `add-circular-cut-session`.
Author/committer `gesriot <gessman1618@gmail.com>`. No merge or auto-merge.
Post-merge base CI, the code/workflow head and the docs-only head are recorded
separately below.

## What the gates prove

Three real egui widget gates use the handler's production predicate `can_add_cut`;
each busy mode is checked only after the same idle request was shown to be offered:

- `add_cut_reads_current_fields_draft_history_stays_separate_and_copy_is_clean_only`:
  the Add form paints **Add cut**, **Confirm draft numbers**, draft Undo/Redo and the
  title *Add circular cut*, never the edit's *Apply cut* (whose predicate
  `can_apply_cut` stays closed for this form). Add reads unconfirmed typed
  `19.25, 30.5, 2.125, Blind 4.875` as one request naming the saved Body UUID and the
  form's version; the draft history stays empty. Through all is read as intent.
  Confirm draft numbers asks the window for nothing and only feeds the draft's
  history; an empty-field press does not cost the draft its Redo. The clean copy
  remains; on a dirty session the copy is disabled with words and Add still adds.
- `add_cut_busy_guard_proves_idle_request_then_excludes_other_work_and_forms`:
  load, a session step, export, an edit worker and another open form each close the
  guard; a running form ignores a real pointer press; an edit form, the saved Sketch
  editor, no session and a dismissed form never offer Add.
- `add_cut_dirty_discovery_refusals_keep_text_status_dismissal_and_unsupported_bodies`:
  the form opens on a dirty document only when the window says the session is idle,
  on the accepted private snapshot. The document crate's existing `validate_cut`
  rule (unchanged) refuses a point inside the box but outside the concave outline,
  an existing disk, a negative radius and non-numbers with the exact text kept. The
  document's status is drawn inside the form; the source bytes are unchanged. At 16
  Cuts and on a constrained base the catalogue offers no Add.

Native gates run the session's real worker (`spawn_add_cut` →
`StepTicket::add_circular_cut` → `circular_cut_copy`) and the shipped CLI as peer.
The fixture is the §30F six-Line asymmetric concave outline
`(-2.5,-1.25),(84,-1.25),(84,28),(54,28),(54,55),(-2.5,55)` at 12.75 mm with three
Cuts (fractional centres/radii, alternating ThroughAll and Blind), reversed rowids,
duplicated names, offset capability rowids, an optional capability and an extra
SQL table. Base edits are made first: height 15.25 and left-wall X −2.5 → −3.75.

`native_dirty_base_two_adds_edit_history_save_branch_match_cli`:

- Two Adds through the form's own typed fields: Blind `(19.25,30.5) r2.125 d4.875`,
  then ThroughAll `(66.5,10.75) r1.875`. Each: the request names the saved Body and
  the accepted version; the form closes on acceptance; the user's file is untouched;
  the allowlist holds (every old object row equal except the Body, whose tip becomes
  the new Cut; the new Extrude is a Cut, `previous` = the old tip; every old ref
  unchanged); Edit cut discovery offers the new Cut as the last tool.
- Every SQL cell (rowid included, only `meta.modified_at` set aside, raw payload
  hashes checked) equals `cut-circular-copy` on the same accepted snapshot and on an
  independent CLI chain, under a proved bijection of new identities only: new Cut
  features paired by their place in the history (`previous`), each tool Sketch by its
  feature's `profile`, curves in stored order, then each new topology ref by all its
  other cells. No old UUID, whole payload or row order is normalized; mapping an old
  identity fails. Pairing both Adds at once from the saved file gives exactly the
  step-by-step bijection.
- The second new Cut is edited through §30F's own form and worker (X 66.25, r 2)
  and matches `edit-circular-cut` with UUIDs translated only through that bijection.
- Unsaved STL and FBX bytes equal the CLI peer's exports (the FBX carries no new
  feature identity, so byte equality holds without mapping); independent B-Rep and
  signed STL volume agree.
- Undo through every step and Redo back compare all SQL cells with **no** mapping;
  undone Cuts leave discovery and return with the same UUIDs. Save writes the file;
  cold rebuild and reopen are clean.
- Refusals from Undo-of-edit: stale form (document changed after this form was
  opened), the same disk again (the domain's refusal naming the second new Cut's
  UUID), `(70,42)` outside the concave outline (`stay inside`). Then five
  cancellation/failure modes: cancel after the answer, stale answer, failed scene,
  failed GPU, cancel before the answer. Each keeps export path, dirty, Undo/Redo,
  saved version, private files, the exact typed draft and the user's file bytes.
- A third Add after Undo is accepted and drops Redo; Save As writes `branch.fcad`
  and keeps the previous file; the branch has six Cuts, matches its CLI peer in all
  cells and exports byte-equal STL/FBX.
- The window comparator (below) runs on these session-worker files laid out as the
  window would leave them, and rejects its seven negative controls. **That is a
  self-check of the comparator, not window evidence.**

Other gates: `native_seventeenth_cut_is_refused_and_keeps_history` (a light 16-Cut
fixture; the 17th is the domain's refusal, nothing moves);
`native_constrained_base_refuses_add_even_after_an_unsaved_constraint_apply`
(`read_history` with `constrained_base=false` keeps refusing; not widened);
`stub_add_cut_refuses_without_publication` (no kernel: refusal, no file, nothing
accepted); `mixed_add_cut_uses_occt_without_solver` (the whole native gate with OCCT
and no PlaneGCS).

Artifacts `add-cut-history.{stl,fbx}` (edited state) and
`add-cut-history-branch.{stl,fbx}` (six Cuts) are read by the pinned strict ufbx
reader (`--identity` checks=6 failures=0, `--triangles`) and joined with oriented STL
triangles by `tools/fbx/stl-matches-fbx.py`; runtime CI requires
`FCAD_ADD_CUT_SESSION_UFBX_EXECUTED`.

## CI wiring

Older gates stay; new exact-name, no-skip gates are added:

- ci.yml *Open, edit, Undo, Redo and Save one document without native geometry*:
  the three widget gates and `stub_add_cut_refuses_without_publication`.
- runtime-layout.yml native session step: the three `native_*` gates with
  `FCAD_ADD_CUT_SESSION_ARTIFACTS`, both artifact pairs required, exported as
  `FCAD_ADD_CUT_SESSION_FBX_DIR`.
- runtime-layout.yml no-solver step: `mixed_add_cut_uses_occt_without_solver`.
- tools/check-fbx-complex.sh: the strict reader loop over both artifact pairs.

## Directed mutations

M30I-1 replaces only `self.check_form_version(expected)?;` in
`StepTicket::add_circular_cut` with `let _ = expected;`. It compiles; the native
history gate fails its executed stale-refusal assertion (`left: Show(…)`,
`right: Failed`). M30I-2 removes only `&& !sessions.busy()` from the production
`can_add_cut` in main.rs. It compiles; the busy-guard widget gate fails its executed
assertion `busy mode 1`. Each file was saved first, restored byte-for-byte
(SHA-256 verified) and the positive gates rerun green.

## Real window recipe

Use a freshly staged bundle and exactly one owned viewer/watchdog; never a CLI
stand-in for a window output. `APP` is that bundle; choose a new root outside any
checkout.

```sh
FCAD_30I_GUI_ROOT=/private/tmp/ferrite-30i-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/add-cut-session-gui.py "$FCAD_30I_GUI_ROOT"
python3 tools/watch-viewer-memory.py --log "$FCAD_30I_GUI_ROOT/watch.jsonl" \
 --limit-mib 1536 -- "$APP/Contents/MacOS/ferritecad-viewer"
```

The generator writes only `source/`, `work/cuts.fcad`, requests and `facts.json`;
it refuses a destination inside a checkout. All captures go into the root.

1. Open `work/cuts.fcad`. Apply height 15.25. Edit base Sketch: both left-wall X
   −2.5 → −3.75; Apply vertices.
2. **Cut circle into … — UUID…**: centre 19.25, 30.5, radius 2.125, Blind 4.875.
   Confirm draft numbers → Undo → Redo (draft only). **Add cut**: no dialog, the form
   closes, the title shows unsaved changes.
3. Open it again: 66.5, 10.75, radius 1.875, Through all; Add cut without
   confirming. Copy the still-original `work/cuts.fcad` to `after-add.fcad`.
4. **Edit cut** on the new Cut at (66.5, 10.75): X 66.25, radius 2; Apply cut.
   Export `unsaved.stl` and `unsaved.fbx`. Document Undo once; export `undo.stl`.
   Undo to the opened state, Redo all, Save. Copy `work/cuts.fcad` to `saved.fcad`.
5. Undo once (the edit). Open Cut circle: 66.5, 10.75, 1.875, Through all; Add cut:
   the refusal names the new Cut's UUID and the typed numbers stay. Copy
   `work/cuts.fcad` to `after-refusal.fcad`. Close the form. With unsaved changes,
   **Save cut copy…** is disabled with its reason.
6. Open Cut circle: 8.5, 44.25, radius 1.5, Blind 3.25; Add cut; Redo disappears.
   Save As `branch.fcad` in the root; copy `work/cuts.fcad` to `after-saveas.fcad`.
7. Quit. Check only the owned PID/watchdog.

```sh
python3 tools/add-cut-session-gui.py --compare "$FCAD_30I_GUI_ROOT"
```

`--compare` refuses (`FCAD_30I_GUI_COMPARE_REFUSED`) before any build or peer job
when an output is missing; the Rust comparator checks again. It builds a temporary
CLI chain from the untouched source and requires `FCAD_30I_GUI_COMPARE_OK
negative_controls=7 all_SQL_cells=true`. The seven controls: missing output, original
instead of Save, unedited new Cut, wrong branch, stale export, swapped new
identities, corrupt raw hash. Read the actual `unsaved.fbx` with the pinned strict
reader and join it with `unsaved.stl`.

## Execution record (cloud, Linux x86_64)

Rust 1.96.0, existing pinned OCCT/PlaneGCS builds and existing targets,
`CARGO_BUILD_JOBS=2`, sequential builds. Local runs used the debug profile; CI runs
release. No window and no GPU were run here.

- `cargo test -p ferritecad-app -p ferritecad-jobs` (OCCT + PlaneGCS): 486 app tests
  and every jobs test passed; clippy `-D warnings` and `cargo fmt --all --check` clean.
- Mixed (OCCT, no solver, CLI without planegcs): `mixed_add_cut_uses_occt_without_solver`
  and the §30F `mixed_cut_apply_uses_occt_without_solver` executed and passed
  (no `skipped:`), including `FCAD_30I_GUI_COMPARE_OK negative_controls=7`.
- Real stub: the packed ci.yml step, extracted verbatim, ran in the existing stub
  target with native/solver variables unset: exit 0, 104 exact gates ok, the four
  new ones included.
- Strict ufbx (the pinned reader rebuilt from this checkout's source, gcc without
  `-Werror`): both artifacts `checks=6 failures=0`; STL joins 884 and 1036
  triangles, worst 1.39e-17 m.
- Both directed mutations failed executed assertions and were restored as above.
- Generator: refused a checkout destination; produced inputs in the scratchpad;
  `--compare` and the Rust comparator refused the missing window outputs.

**Not executed here:** the real window recipe (left for the independent macOS GUI
stage), macOS/Windows native runs (CI only), release-profile local runs.

### Base CI (post-merge `85a731c`)

CI 37517778819 success; planegcs pin 37517779052 success; product sbom
37517778796, rust sbom 37517778876 and rust notices 37517778873 success. Combined
runtime layout 37517778797: macOS and Linux success; Windows still in progress when
this was written, so it is not claimed passed.
