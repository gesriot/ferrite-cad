# §30N verification — named checkpoints inside the document

Contract: [named-document-checkpoints.md](named-document-checkpoints.md). Decision:
[ADR 0005, §30N](decisions/0005-document-session.md#30n-named-checkpoints-inside-the-document).

## Base

`origin/main` = merge of PR #93, `e8ab0e331ab3b795268b5364c57421db96a52309`; its tree
`99f77f91b55bb10bd4327237a2babec415526e9d` equals the tree of the final PR head
`c8b088ebabab56ac3ff81a42d128c84bfe3a3bfa`. Reviewed code head `956317f` CI: success.
Post-merge CI on `e8ab0e3` when this slice started: CI, planegcs pin, rust notices, rust
sbom, product sbom succeeded; *combined runtime layout* was still in progress, with no
completed failure. Branch `named-document-checkpoints`, local and uncommitted.

## What was found before the change

* Every read path (`Document::open_read_only`) refused any schema but the current one,
  so a schema bump would have made every existing v3 file unreadable by the window and
  every read-only command. `clear-cache` opened with migration and would have upgraded
  a v3 file in place.
* `model_version` already covers every table, so a catalog in the document is dirty
  content without any exception; `snapshot_to` (online backup), `Temporary` /
  `Existing::Keep` (no-clobber publication), `StepTicket::run`/`commit_step` (two-phase
  steps) and the recorder (§30M) are reused unchanged.
* No geometric cache keys on anything but object inputs; the window's picture is the
  only other derived state, and nothing needed to rebuild it for a list change.

## What changed

* Document: schema v4 (`checkpoints`); read-only acceptance of v3; `CheckpointId`;
  `Document::{checkpoints, model_without_checkpoints, write_checkpoint_image,
  add_checkpoint, remove_checkpoint, extract_checkpoint_image, replace_checkpoints_from}`
  in a child module of `document.rs`; limits and the name rule; `rusqlite` `blob`
  feature for streaming (no new crate).
* Jobs: `checkpoint` module (`list_checkpoints`, `create_checkpoint_copy`,
  `delete_checkpoint_copy`, `extract_checkpoint`, crate-private Restore copy) on the
  edit copies' pattern; `Snapshot` carries the drawn model and the list;
  `StepTicket::{create,delete,restore}_checkpoint`; `ProducedStep::keeps_picture`.
* Window: `Sessions::{begin_checkpoint, finish_checkpoint, commit_kept}`,
  `Kind::Checkpoint`, `Edited::Keep`, `spawn_checkpoint` (no kernel),
  `AppEvent::CheckpointStepped`, `App::keep_picture` with `retarget_scene`, one
  availability predicate (`checkpoints::availability`), Delete confirmation,
  `ferritecad_ui::checkpoint_panel`.
* CLI: four commands, JSON v1 additive; `clear-cache` opens read-only.
* Tools/CI: exact gates in the existing stub, native, no-solver and pinned-ufbx steps;
  `tools/named-document-checkpoints-gui.py`.

## Gates

Every gate runs `--exact`; a gate fails on `skipped:` and on zero tests; gates that
print a marker are checked by `test NAME ... `, `test result: ok. 1 passed; 0 failed`
and the marker line.

**Kernel-free, every platform (`ci.yml`, stub step):**

* `ferritecad-document` lib (3): v3 is read as it is and reaches v4 with an empty
  catalog, v2 still needs a migration; SQL `CHECK`s refuse malformed rows; the name rule.
* `ferritecad-document --test checkpoints` (6), on the committed schema v3 plate: open,
  list and a refused extraction write not one byte (bytes, mtime, `user_version`); in a
  v3 file a table called `checkpoints` is an unknown table, not a catalog, and v4 is
  refused over it without a write; an
  image equals its version in every SQL cell, row identity, schema entry and an unknown
  table, holds no checkpoints, and B is not A plus B; Restore keeps the catalog row for
  row; Delete leaves the model; 32 and 16 MiB refuse without removing (a 6 MiB model
  fits twice, not three times); a deleted image leaves none of its bytes; a flipped image
  byte refuses that checkpoint only and leaves no file; a hand-written bad name damages
  the list in words; another document's model is never stored.
* `ferritecad-jobs --test checkpoints` (5): the v3 file through a session (Create is
  dirty, Undo clean, Redo the same file and UUID, Save publishes v4); Restore A after
  two changes as one step keeping [A, B], Undo/Redo by identity, Restore to the current
  model is no change, Delete/Undo/Redo, a deleted checkpoint refused, a stale step;
  Empty Untitled through Save As, reopen and extraction; required version, occupied,
  source, alias, dangling link and linked-folder outputs refused, and cancel / occupant
  / source change injected at the 0.95 progress mark publish nothing and leave no
  scratch; a crash copy carries the list and restores from it.
* `ferritecad-cli --test checkpoints` (2): JSON and text, required `--expect-version`,
  v3 source stays v3 and byte-identical, v4 copy, extraction equals the v3 model,
  stale/occupied/unknown/self/name-as-UUID refused; exit 7 after a published copy.
* App (4 in the session loop + 1 marker gate): Create keeps the picture, is dirty,
  Undo/Redo, Save, Delete/Undo; the shared slot refuses competing Apply and a second
  checkpoint, Cancel before and after the answer, an answer for a replaced document is
  ignored, a refusal said in words; the predicate (no document, busy, form open, name,
  32, unreadable list); the window's crash copy carries the list; stub
  (`FCAD_30N_STUB_CHECKPOINTS_EXECUTED`): Create, Delete and extraction without a
  kernel, Restore refused at its picture with nothing changed.
* UI (1): buttons ask nothing while disabled, each disabled kind says why once, the
  section opens from its counted heading.

**Native (`runtime-layout.yml`, OCCT + PlaneGCS):**

* `native_checkpoints_restore_undo_redo_save_reopen_and_extract_like_the_command_line`
  (`FCAD_30N_NATIVE_CHECKPOINTS_EXECUTED volume_a=… volume_b=…`): plate 80×40×12,
  checkpoint A, height 20 and Add Cut (r 5, through all) on the window's workers,
  checkpoint B, Restore A with its picture, Undo, Redo, unsaved STL/FBX, Save, reopen,
  CLI `list-checkpoints` and `extract-checkpoint` of A and B. SQL: extracted A equals the
  version A was made from in every cell, stamp and schema row (no allowlist); extracted
  B likewise except the `checkpoints` rows (an image never holds checkpoints); the
  restored version equals extracted A except `meta.modified_at` and the kept catalog.
  UUIDs and references of the restored model equal A's. Geometry from an independent
  STL reading: height 12 / 20, volume 38 400 mm³ and 64 000 − π·5²·20 mm³, more
  triangles for B. Window STL/FBX byte-equal to the CLI's of extracted A. Artifacts
  `checkpoint-a/b.{stl,fbx}` for the pinned reader.
* `native_an_imported_step_checkpoint_needs_no_step_file`
  (`FCAD_30N_NATIVE_STEP_CHECKPOINT_EXECUTED`): `import-step` of `01-single-part.step`,
  the private STEP copy deleted, checkpoint, Save, CLI extraction; the stored source
  bytes equal the fixture and the extracted file is drawn from them alone.
* Pinned ufbx (`tools/check-fbx-complex.sh`, `FCAD_CHECKPOINT_SESSION_UFBX_EXECUTED`).

**No solver:** `mixed_checkpoint_restore_draws_with_occt_and_no_solver`
(`FCAD_30N_MIXED_CHECKPOINTS_EXECUTED`).

## Real window recipe (macOS)

Not run by the author (the window, bundle and GPU are left to the reviewer). One
freshly staged arm64 bundle `APP`; one viewer under the 1536 MiB watchdog; a new root
outside any checkout; the recovery folder is the root's own.

```sh
ROOT=/private/tmp/ferrite-30n-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 tools/named-document-checkpoints-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/plate.fcad"
```

The generator writes only `plate.fcad` (CLI sample 80×40×12), `old.fcad` (the committed
schema v3 plate), their pristine copies in `inputs/`, `facts.json` and the empty
`recovery/`; it makes no window output.

1. The plate opens. Open **Checkpoints (0)**. With the name field empty, **Create
   checkpoint** is disabled and says how to name one.
2. Type `A 12 mm`, **Create checkpoint**. Title `*plate.fcad`; the row
   `A 12 mm — <time> UTC · on screen`; the name field empties.
3. **Edit extrusion…** → height 20 → **Apply**; close the form.
4. **Cut circle into …**: centre 40, 20, radius 5, **Through all** → **Add cut**.
5. Type `B 20 mm hole`, **Create checkpoint**. Two rows; B is *on screen*.
6. Open **Edit extrusion…** again: Restore and Delete… are disabled and say "Close the
   open form first…". Close the form.
7. **Restore** on A: the 12 mm plate without a hole; the list still has A and B, A
   *on screen*. Export STL → `$ROOT/restored.stl`; Export FBX → `$ROOT/restored.fbx`.
8. **Undo**: 20 mm with the hole. **Redo**: the 12 mm plate again.
9. **Delete…** on B → **Cancel**: nothing changes. **Delete…** on B → **Delete**: one
   row. **Undo**: B is back.
10. **Save** (title loses `*`). **Open…** `old.fcad` (no question).
11. Type `old`, **Create checkpoint**; **Save As…** `$ROOT/old-saved.fcad`. Quit (no
    question). Check the owned PID is gone.

```sh
python3 tools/named-document-checkpoints-gui.py --compare "$ROOT"
```

`--compare` refuses (`FCAD_30N_GUI_COMPARE_REFUSED`) before any build or peer job when
an output is missing or `plate.fcad` was never saved; the Rust comparator refuses a
missing output again before its first peer job. It reads only the window's outputs and
requires `FCAD_30N_GUI_COMPARE_OK negative_controls=4 all_SQL_cells=true`: `old.fcad`
byte-identical and still schema v3; `plate.fcad` lists exactly `A 12 mm`, `B 20 mm hole`
and holds A's model; CLI-extracted A equals the pristine plate in every SQL cell, stamp
and row identity, B is 20 high with one Cut; the saved plate equals extracted A except
the stamp and the list; `restored.stl`/`.fbx` byte-equal to the CLI's exports of
extracted A; `old-saved.fcad` is v4 with one checkpoint `old` holding `old.fcad`'s
model; no recovery record left. Controls (on a copy of the real outputs): a missing
output, an unsaved plate, the old file upgraded, B's STL as the restored one.

## Execution record (author, macOS arm64, 2026-10-08)

Local, uncommitted; no remote CI exists for this diff. Native environment: the existing
target `/private/tmp/ferrite-24b-native-target` with the pinned OCCT and PlaneGCS of
`vendor/` (`CARGO_BUILD_JOBS=2`, sequential builds); stub: the existing
`/private/tmp/ferrite-25j-stub-target` with no OCCT variables. Nothing native was
rebuilt from source. `WGPU_BACKEND=vulkan` was set for every test run: this Mac has no
Vulkan loader, so every pixel/GPU test found no adapter and said `skipped:` (checked
with `--nocapture` on `imported_step_pixels`); `ferritecad-viewport-gpu` was excluded.
No viewer, bundle, CUA, osascript, GPU or Unity was run.

* `cargo fmt --all -- --check`: clean. `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`: clean (forced re-check of 16 crates).
* Whole native workspace (`--release --features planegcs`, all packages but
  `ferritecad-viewport-gpu`): 120 test binaries, **2421 passed, 0 failed, 4 ignored**.
  After the last source edits, the changed packages again: document 157 + 6 + existing
  integration suites, jobs 59 + 5 + session/save/recovery/unnamed, types 35, CLI
  checkpoints 2 / dump_graph 8 / validate 4 / recovery 4, app 534 (1 ignored), UI 107 —
  all passed.
* The exact `run:` block of the `ci.yml` stub step, extracted from the YAML and run in
  the stub target: exit 0, 166 gate lines ok, every §30N gate present once, markers
  `FCAD_30N_STUB_CHECKPOINTS_EXECUTED` (and the existing §30M ones). The gate added
  after that run (`in_a_v3_file_a_table_called_checkpoints_is_somebody_elses`) was run
  alone with the same command: ok.
* The exact §30N lines of `runtime-layout.yml`, native step (OCCT + PlaneGCS): exit 0,
  `FCAD_30N_NATIVE_CHECKPOINTS_EXECUTED volume_a=38400.000 volume_b=62431.253`
  (expected 64 000 − π·25·20 = 62 429.204; tessellated hole), and
  `FCAD_30N_NATIVE_STEP_CHECKPOINT_EXECUTED`; four artifacts written. No-solver step
  (release without PlaneGCS): `FCAD_30N_MIXED_CHECKPOINTS_EXECUTED`.
* `tools/check-fbx-complex.sh --release --features planegcs`, sourced in bash as the
  workflow does, with `FCAD_CHECKPOINT_SESSION_FBX_DIR`: pinned ufbx 0.23.0 strict,
  `checks=6 failures=0` for `checkpoint-a` and `checkpoint-b`, STL↔FBX joins of 12 and
  300 triangles, `FCAD_CHECKPOINT_SESSION_UFBX_EXECUTED`; its existing complex gate
  passed too (256 checks). (Started from zsh or through `/bin/bash script`, it fails for
  reasons of the harness — empty `BASH_SOURCE`, stripped `DYLD_*` — not of the product.)
* `actionlint` (with shellcheck) on both edited workflows: clean. The lint job's
  scripts — licence headers (444 files), STEP corpus, export boundary, the Unity/FBX
  identity records, solver ownership, PlaneGCS pins, notice ownership: all pass.
  `Cargo.lock` is unchanged (`blob` is a feature of the existing `rusqlite`).
* The CLI recipe of the contract, run with the release binary: as documented. Sizes:
  the sample plate is 94 208 bytes, its checkpoint image 94 208, the copy with one
  checkpoint 188 416; after `delete-checkpoint` the copy stays 188 416 bytes with the
  freed pages zeroed (they are reused by the next checkpoint; the 16 MiB limit bounds
  them). That observation led to `secure_delete` and its test.

## Directed mutations

Both compiled, failed executed assertions, and were restored byte for byte (SHA-256 of
`git diff` plus every untracked file identical before and after:
`a592bb15…3a36`); the positive gates above were run after restoration.

* **M30N-1 — Restore loses the restored model.** `restore_checkpoint_copy` publishes the
  current model instead of the checkpoint's image. Failed:
  `restore_is_one_step_that_keeps_the_catalog_and_undo_redo_move_through_it` ("a
  restored model has to be drawn"), `a_crash_copy_carries_the_checkpoints_of_the_accepted_version`
  (height 15 ≠ 8), and the native app gate (`NoChange` instead of a restored picture).
* **M30N-2 — a list change is not dirty.** `model_version` sets the catalog aside.
  Create then becomes `NoChange` and the checkpoint is silently lost: four jobs gates
  (`left: NoChange, right: Accepted`) and five app gates (`not a catalog-only step:
  NoChange`, `matches!(edited, Edited::Keep(_))`) failed.

## Not executed, limits

* The real window recipe above (left to the independent macOS review); no window
  output exists and the comparator has never been run on one. It deliberately has no
  self-check that fabricates window outputs.
* Windows and Linux: only through CI, which does not exist yet for this diff.
* Old builds reading v4 files are refused by design; not exercised against an old
  binary here beyond the schema rule's own test.
* Power-loss durability: not claimed (publication is the existing no-clobber link;
  Save is unchanged).
* Each private version carries the catalog, so with many large checkpoints every Undo
  step costs up to 16 MiB more on disk and `Snapshot::adopt` hashes the catalog once
  more; measured only for small documents.

## Files

Changed: `.github/workflows/ci.yml`, `.github/workflows/runtime-layout.yml`, `README.md`, `crates/ferritecad-app/src/dialogs.rs`, `crates/ferritecad-app/src/main.rs`, `crates/ferritecad-app/src/sessions.rs`, `crates/ferritecad-cli/src/json.rs`, `crates/ferritecad-cli/src/main.rs`, `crates/ferritecad-cli/tests/dump_graph.rs`, `crates/ferritecad-document/Cargo.toml`, `crates/ferritecad-document/src/document.rs`, `crates/ferritecad-document/src/lib.rs`, `crates/ferritecad-document/src/schema.rs`, `crates/ferritecad-jobs/src/lib.rs`, `crates/ferritecad-jobs/src/session.rs`, `crates/ferritecad-types/src/ids.rs`, `crates/ferritecad-types/src/lib.rs`, `crates/ferritecad-ui/src/lib.rs`, `crates/ferritecad-ui/src/panels.rs`, `docs/architecture-decisions.md`, `docs/cli-capabilities.md`, `docs/decisions/0005-document-session.md`, `docs/document-session.md`, `docs/implementation-plan.md`, `tools/check-fbx-complex.sh`

New: `crates/ferritecad-app/src/checkpoints.rs`, `crates/ferritecad-app/src/sessions/tests/checkpoints.rs`, `crates/ferritecad-cli/src/checkpoint.rs`, `crates/ferritecad-cli/src/json/checkpoint.rs`, `crates/ferritecad-cli/tests/checkpoints.rs`, `crates/ferritecad-document/src/document/checkpoint.rs`, `crates/ferritecad-document/tests/checkpoints.rs`, `crates/ferritecad-jobs/src/checkpoint.rs`, `crates/ferritecad-jobs/tests/checkpoints.rs`, `crates/ferritecad-ui/src/checkpoint.rs`, `docs/named-document-checkpoints-verification.md`, `docs/named-document-checkpoints.md`, `tools/named-document-checkpoints-gui.py`

## Independent review (2026-10-08, in progress)

The base merge now has six successful workflows, including runtime 37842033936.
Review found and corrected:

* Storage failure cleanup unlinked an occupied extraction/image/scratch path.
  It now cleans only files it exclusively created. An executed regression failed
  before the fix because the occupied file was gone, then passed, including a
  source-as-output check.
* `add_checkpoint` accepted a prepared image after the model had changed. The
  writer now compares the model inside its transaction; an executed regression
  failed before the fix and passed afterwards without a source write.
* Metadata-only Undo/Redo used the geometry worker despite retaining the same
  drawn model. It now reads form facts on a kernel-free worker and uses the same
  guarded picture-retarget/commit route as Create/Delete. The existing history
  test now executes this worker rather than accepting a fabricated scene.
* Restoring an intact image was blocked by a damaged sibling's hash. Restore now
  preserves unrelated catalogue rows byte-for-byte, including a damaged image;
  only the restored image is validated. The damage test covers this case.
* Catalogue reads now enforce the count/byte bounds before accumulating more rows,
  including a catalogue inserted outside the product writer.

The three new storage regressions are mandatory exact-name gates on all CI OSes.
Window evidence and remote CI follow below when completed.
