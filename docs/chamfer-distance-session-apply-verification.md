# §30H — verification

[Contract](chamfer-distance-session-apply.md). Marker
`FCAD_30H_CHAMFER_DISTANCE_SESSION_APPLY`.
Base `48c6d66c5a541b0f42d46ac39912ac820c9f3869`, tree
`649d02382dcc32e0e46e07098f5ca468e8f205a8` (merge of PR #87). Clean
`main == origin/main` after fetch; PR #87 MERGED. Branch
`chamfer-distance-session-apply`. Author/committer `gesriot <gessman1618@gmail.com>`.
No merge/auto-merge. Post-merge base, code/workflow head and docs-only head CI are
recorded separately below.

## What the gates prove

Three real egui widget gates use the handler's production predicate
`can_apply_chamfer_distance`, and each busy mode is checked only after the same
changed idle request was shown to be offered:

- `distance_current_text_draft_history_noop_clean_copy_and_add_exclusion`: Apply
  reads unconfirmed text `3.06250`; Confirm draft number feeds only the form's
  request history (`2.375 → 3.06250 → 4.5`), Undo/Redo request move the field and
  apply nothing, Apply after a draft Undo uses the field, the clean copy follows the
  confirmed request; `2.3750` offers no Apply but still a clean no-op copy; the Add
  Chamfer form has no session Apply.
- `distance_busy_guard_proves_changed_idle_request_and_excludes_other_work`: load,
  session step, export, edit worker, an existing Fillet form beside it, New's form,
  the saved Sketch editor and a missing session each close the guard; the guard
  reopens after every removable mode.
- `distance_dirty_discovery_copy_reason_refusal_retains_text_and_acceptance_closes_form`:
  on a dirty session the existing Chamfer's button opens the form on the accepted
  private snapshot (not the logical file) while the copy gate is shut; Add stays
  disabled even with that gate open; parse, minimum and `next_up(max)` refusals
  keep the exact text; the dirty copy is disabled with its reason and the handler's
  `refuse_unsaved_distance_copy` refuses it again before a dialog; the outcome is
  read inside the form; acceptance closes it and keeps session availability.

Native gates run the actual worker and the peer CLI. Fixture: the clockwise,
fractional, offset plate `(33,15.5),(33,3.25),(-4.5,3.25),(-4.5,15.5)` with one
Chamfer of 2.375 mm at `(-4.5,3.25)` — the second and third Lines, neither the first
corner nor the first Line — written by the shipped preparation and writer. Then the
base Extrude gets one more resolvable name (a copy of its own ref under a new UUID),
every object is named `same`, SQLite rowids and ordinals run against the feature
history, capability rowids move, and an optional capability and an extra table ride
along.

- `native_height_vertices_then_distances_cross_history_and_match_cli`: height
  6.75 → 9.25, left X −4.5 → −5.75 (the Chamfer's corner moves to `(-5.75,3.25)` by
  its two Line UUIDs), then distances `3.06250`, `4.4375`, `6.125` through the
  widgets' Apply. Each step: only the Chamfer row's payload/hash differ from the
  previous accepted snapshot (UUID, previous, joint, every ref and every other cell
  equal), and all SQL equals the CLI peer chain. Unsaved STL/FBX bytes equal the CLI's.
  Undo through all five steps to clean, Redo all; Undo to 4.4375 and its unsaved
  export equal the CLI's; Save; cold reopen. No-op keeps Redo and leaves no file
  while the CLI still publishes the no-op copy; stale form refused; `next_up(12.24)`
  refused (`input`, both Line UUIDs, both numbers, "nothing is clamped"); cancel
  before and after the answer, a stale answer, a failed scene and a failed GPU
  preparation keep scene, history, checkpoint, file and draft; the exact bound 12.24
  branches, drops Redo, Save As keeps the saved file and equals the CLI.
- `native_constraints_then_distance_use_solved_sides_and_keep_refused_draft_redo`:
  constraints Apply (H/V, Fixed `(33,15.5)`, depth 14.25, width 41) through the
  session; stored Lines unchanged (stored bound 12.24). 13.5 — above the stored and
  below the solved bound — and the exact solved bound 14.24 are accepted from the
  same accepted constrained snapshot as the CLI. `next_up(14.24)` is refused by the
  evaluator: `input`, Chamfer UUID, both Line UUIDs, "does not fit the solved
  plate", both numbers, readable in the form; draft, Redo, checkpoint and file kept.
  A Parallel between a vertical and a horizontal Line stays a typed `constraint`
  refusal. Undo/Redo crosses the constrained base; Save; cold reopen.
- `stub_distance_apply_refuses_without_publication` and
  `mixed_distance_apply_uses_occt_without_solver` (the unconstrained gate without a
  solver).

Geometry (every cold, cached-miss and warm-hit rebuild; the warm pass requires real
archive hits of the base Extrude and the Chamfer; every saved ref resolves): 7 B-Rep
faces, volume `(W·D − d²/2)·H` within 1e-9 of the block, the one named
`EdgeChamferFace` is a plane whose outward normal is the diagonal of exactly the
chosen corner, through the two points `d` along each adjacent side, with the corner
`d/√2` beyond it and area `d·√2·H`. The independent STL reader requires a closed
oriented mesh, its volume, the flat facing out of that corner adding up to `d·√2·H`,
new vertex columns `d` along each side and no vertex left at the corner. A plane at
another corner with the same volume fails.

## Local commands and exact gates

This container is Linux x86_64 (no macOS arm64 host, no screen). Existing
Open CASCADE 8.0.1 and pinned PlaneGCS builds and existing targets were reused
with `CARGO_BUILD_JOBS=2`; nothing native was rebuilt. `FCAD_ALLOW_LOADER_FAILURE_PROBES`
was not set.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --features planegcs -p ferritecad-app --bin ferritecad-viewer chamfers:: -- --nocapture --test-threads=1
cargo test --features planegcs -p ferritecad-app --bin ferritecad-viewer sessions::tests::chamfer:: -- --nocapture --test-threads=1
```

Packed argv: the complete `run:` blocks of `ci.yml` *Open, edit, Undo, Redo and Save
one document without native geometry* and `runtime-layout.yml` *Edit, Undo, Redo,
export and Save one open document through the native window state* and *Chamfer a
plate with Open CASCADE and no solver*, extracted by step name with
`${{ matrix.name }}` → `linux`. The stub block ran unchanged in the existing stub
target (OCCT discovery fails there, so Cargo builds the no-kernel adapter). The two
native blocks ran with only these substitutions, to reuse the existing debug native
target instead of creating a release one: `--release` removed;
`$PWD/target/release/ferritecad` → `$PWD/target/debug/ferritecad`; the native block's
`$PWD/vendor/planegcs` → the existing pinned PlaneGCS directory. As in CI, the
no-solver CLI was built first for the mixed block and the combined CLI/viewer were
restored afterwards. `RUNNER_TEMP`, `GITHUB_ENV` and `FCAD_OCCT_LIB_DIR` were set.
The strict-reader part is the five session blocks of `tools/check-fbx-complex.sh`
(verbatim), with the reader built by the script's own commands from the pinned,
digest-checked ufbx and `clang` (the script's first choice).

## Directed mutations

M30H-1 removes only the distance ticket's `check_form_version(expected)`.
M30H-2 makes only the distance ticket read `v0.fcad` — the session's snapshot of the
logical file taken at Open — with that file's own version, instead of the accepted
snapshot. Each must compile and fail an executed assertion in
`sessions::tests::chamfer::native_height_vertices_then_distances_cross_history_and_match_cli`.
`session.rs` is then restored byte for byte and its SHA-256 verified, and the
positive packed gates run again. Compiler errors, zero tests and equivalent
mutations do not count.

## Extractable real-window recipe

Set `APP` to a freshly staged arm64 bundle. The generator creates only inputs and
facts and refuses a destination inside a checkout; the comparison refuses missing
window outputs before any peer job. Use one owned viewer under the watchdog; do not
redirect to its reserved stdout/stderr files.

```sh
FCAD_30H_GUI_ROOT=/private/tmp/ferrite-30h-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/chamfer-session-gui.py "$FCAD_30H_GUI_ROOT"
python3 tools/watch-viewer-memory.py --log "$FCAD_30H_GUI_ROOT/watch.jsonl" \
 --limit-mib 1536 --seconds 1200 -- "$APP/Contents/MacOS/ferritecad-viewer"
```

1. Open `work/chamfer.fcad`; Apply height 9.25. Edit the base Sketch and change both
   left-wall X coordinates −4.5 → −5.75; Apply vertices. The title is dirty.
2. **Edit Chamfer distance same — `chamfer_uuid` from `facts.json`**; type `3.0625`,
   **Apply distance** without confirming. Edit again: type `4.4375`, **Confirm draft
   number** (form stays, model unchanged), type `5`, **Confirm draft number**,
   **Undo request** (field back to `4.4375`, model unchanged), **Apply distance**.
   Edit again: `6.125`, **Apply distance**. Copy actual `work/chamfer.fcad` to
   `after-apply.fcad`.
3. Close any form; document **Undo** once (6.125 → 4.4375; Redo available). Edit
   Chamfer distance, type `12.25`: the domain bound (12.24 mm) disables Apply, the
   text stays, nothing is clamped; Save distance copy is disabled with the unsaved
   reason. Worker/evaluator refusals are covered by the native constrained gate.
   **Cancel distance draft**; **Redo** (Redo was kept). Copy actual
   `work/chamfer.fcad` to `after-refusal.fcad`.
4. Export `unsaved.stl` and `unsaved.fbx`. Document Undo once, export `undo.stl`.
   Undo through the distance and base edits to a clean title; Redo all; **Save**.
   Copy the actual file to `saved.fcad`.
5. Undo once; Edit Chamfer distance, `12.24`, **Apply distance**: Redo disappears.
   **Save As** `branch.fcad`; copy the previous `work/chamfer.fcad` to
   `after-saveas.fcad`. Cold reopen `saved.fcad`, then Quit.
6. After Quit check only the owned PID/watchdog journal, never getApp/AX.

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/chamfer-session-gui.py --compare "$FCAD_30H_GUI_ROOT"
```

It prints `FCAD_30H_GUI_COMPARE_OK` only when all eight window files exist, the
source was unchanged before Save and after the refusal, Save As kept the saved file,
`saved.fcad`/`branch.fcad` equal the CLI chain in every SQL cell except
`meta.modified_at`, the three exports equal the CLI's bytes, all refs resolve, and
both STL files measure the chosen corner's flat. Read actual `unsaved.fbx` with the
pinned `tools/unity-fbx-smoke/scripts/read_production.c`, `--identity` and
`--triangles`; run `tools/fbx/stl-matches-fbx.py unsaved.stl triangles.txt`. CLI
peers never stand in for GUI outputs. If CUA stalls, stop only the owned PID and
record exactly which operations were not performed. The historical OOM is not
claimed fixed.

## Execution record

Local evidence (Linux x86_64 container, 2026-10-06), logs under
`/home/user/fcad-logs/30h`:

- New gates: the three widget gates and both native gates passed on the combined
  OCCT 8.0.1 + PlaneGCS debug build; all four earlier Chamfer widget/native tests
  still pass (the copy form's button is now *Confirm draft number*).
- Packed stub: **100** exact-name gates passed (the 96 of §30G plus 4 new), no skip;
  Cargo built the no-kernel adapter in that target.
- Packed mixed OCCT/no-solver: **13** exact gates passed (12 plus
  `mixed_distance_apply_uses_occt_without_solver`), no skip; recipes 29A–29C printed
  `_RECIPE_OK` and 29D its expected `FCAD_29D_RECIPE_NO_SOLVER`.
- Packed combined native: **24** exact gates passed (22 plus the two Chamfer gates),
  no skip; `GITHUB_ENV` received `FCAD_CHAMFER_SESSION_FBX_DIR` and the four
  Chamfer artifacts were non-empty.
- Strict pinned ufbx (digest-checked `fcc5d6ba`, reader built with `-Werror`): all
  five session blocks of `check-fbx-complex.sh` passed and printed
  `FCAD_CHAMFER_SESSION_UFBX_EXECUTED`. Each Chamfer FBX passed 6 identity checks
  with 0 failures; oriented STL/FBX joins: history 16 triangles, worst 4.34e−19 m;
  constrained 16 triangles, worst 1.73e−18 m. (A first local attempt compiled the
  reader with `gcc`, which the script does not pick first; `-Werror` stopped it at
  an existing unused variable before any check, so it is not counted.)
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` (0 warnings), `check-licence-headers.sh` (420
  files), `check-export-boundary.sh`, `check-solver-ownership.sh`,
  `check-planegcs-pins.sh`, workflow YAML parsing and `git diff --check` passed.
- M30H-1 compiled and failed the executed assertion at
  `sessions/tests/chamfer.rs:84` (the stale form was shown as `Show`, expected
  `Failed`). M30H-2 compiled and failed at `sessions/tests/chamfer.rs:111` with
  "objects changed" (the first distance step was built from `v0`, losing the
  height and vertex steps). `session.rs` was restored after each, SHA-256
  `d3def245d3197656543ae53e40116617766c6eef70a4ddc05dc07c2d9e525230` verified, and
  the packed native and stub blocks were run again on the restored source.
- The GUI generator refused a destination inside the checkout and produced only
  inputs; its comparator refused the missing `after-apply.fcad` before any peer
  job. A separate comparator self-test filled the eight names with CLI results
  only to exercise its logic (`FCAD_30H_GUI_COMPARE_OK analytical_mm3=4217.349609375`);
  that is not GUI evidence.

Measured volumes (B-Rep checked to 1e−9 of the block on every cold/miss/hit pass):

| Native model | Analytical mm³ | Independent STL mm³ |
| --- | ---: | ---: |
| History, d 6.125 on 38.75 × 12.25 × 9.25 | 4217.349609375 | 4217.349609375 |
| Constrained, d 14.24 on solved 41 × 14.25 × 6.75 | 3259.313100000 | 3259.313122000 |

**GUI: unverified.** This container has no macOS arm64 host, app bundle, screen
or CUA, so no real window was operated, no bundle was staged and no watchdog viewer
was started. The extractable recipe above is left for an independent review; no GUI
artifact and no fix of the historical OOM is claimed. Linux/Windows GUI and the full
STEP/pixel campaigns were not rerun locally; the existing CI campaign keeps them.

Exact post-merge base CI is separate and complete: core CI/pin/runtime 15/15 and
standalone notices/Rust SBOM/product SBOM 12/12, all successful on
`48c6d66c5a541b0f42d46ac39912ac820c9f3869` (push runs of the merge SHA itself):
[CI](https://github.com/gesriot/ferrite-cad/actions/runs/37232791635),
[pin](https://github.com/gesriot/ferrite-cad/actions/runs/37232791685),
[runtime](https://github.com/gesriot/ferrite-cad/actions/runs/37232791604),
[notices](https://github.com/gesriot/ferrite-cad/actions/runs/37232791612),
[Rust SBOM](https://github.com/gesriot/ferrite-cad/actions/runs/37232791620),
[product SBOM](https://github.com/gesriot/ferrite-cad/actions/runs/37232791608).
The code/workflow head's own CI is recorded in a following docs-only commit.
