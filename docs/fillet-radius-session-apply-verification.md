# §30G — verification

[Contract](fillet-radius-session-apply.md). Marker
`FCAD_30G_FILLET_RADIUS_SESSION_APPLY`.
Base `29c0826afb3ddea6a122cb58744e5f24928a2ee8`, tree
`681d640f5d76e40ef165493bbd87ddfcd8bf581b`. Clean `main == origin/main`
after fetch; PR #86 MERGED. Branch `fillet-radius-session-apply`.
Author/committer `gesriot <gessman1618@gmail.com>`. No merge/auto-merge.
Post-merge base, code/workflow head and docs-only head CI are recorded separately.

The three real egui widget gates use `can_apply_fillet_radius`, including a
changed idle request before testing busy controls. They cover current unconfirmed
text, confirmation/Apply distinction, numeric spelling no-op, clean no-op copy,
dirty discovery/copy explanation, Add and other-form exclusion, load/export/edit/
session workers, visible refusal, retained draft and accepted-scene dismissal.

Native gates run the actual worker and peer CLI on one Fillet and four different
radii over a clockwise offset rectangle: `(33,15.5),(33,3.25),(-4.5,3.25),(-4.5,15.5)`.
Object rowids and ordinals are reversed, names duplicate, capability rowids move,
and optional capability/extra SQL data survive. Height 6.75 → 9.25 and left X
−4.5 → −5.75 precede first/middle/last radius changes. Common Undo/Redo, unsaved
exports, Save/cold reopen, no-op/Redo, stale/refused work, cancellation before
and after preparation, stale answers, actual scene/GPU preparation seams and
branch/Save As preserve their contracts.

The constrained gate applies H/V, Fixed and sizes through the constraints worker:
solved rectangle 41 × 14.25, stored rectangle still 37.5 × 12.25. A 7.125 radius
exceeds the stored bound and succeeds on solved geometry; 7.25 is refused by the
solved bound, and two 7.125 arcs are refused on their shared 14.25 Line. A solved
square separately exercises both adjacent neighbours of the fourth corner.
Real domain kind/reason/UUIDs, exact text and Redo are checked.

Every SQL table/row/cell is compared, including SQLite rowid, raw payload/hash,
refs, extra data and capabilities. Only `meta.modified_at` is set aside. No UUID
is normalized. Original source bytes are checked before Save and after refusals;
Save As preserves the saved source. Cold and actual warm archive hits resolve
all refs after history moves. B-Rep volume and independent oriented closed STL
volume/per-corner quarter-circle fits distinguish same-named features with
swapped radii. Unsaved STL/FBX bytes equal CLI on this OS. Pinned strict ufbx and
oriented STL/FBX joins read the produced artifacts.

## Local commands and exact gates

Reuse pinned arm64 libraries and existing native/stub targets, sequentially with
`CARGO_BUILD_JOBS=2`. Do not set `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

```sh
source /private/tmp/ferrite-pr86-review/env.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --release --features planegcs -p ferritecad-app --bin ferritecad-viewer fillets::tests:: -- --nocapture --test-threads=1
```

Extract the complete run blocks from `.github/workflows/ci.yml` and
`runtime-layout.yml`, replacing only `${{ matrix.name }}` with `macos`:

- `Open, edit, Undo, Redo and Save one document without native geometry`
- `Edit, Undo, Redo, export and Save one open document through the native window state`
- `Chamfer a plate with Open CASCADE and no solver`

Execute these packed argv with their exact-name/no-skip assertions. Native needs
`RUNNER_TEMP`, `GITHUB_ENV`, `FCAD_OCCT_LIB_DIR`; source native env inside bash
to preserve DYLD loader settings. Stub uses the existing
`/private/tmp/ferrite-25j-stub-target` with its package-discovery-disabled bridge
cache and native/solver env unset. Mixed uses a temporary symlink facade whose
`target` points to the existing native target, following the
[§30E recipe](revolve-angle-session-apply-verification.md). Restore combined
CLI/app afterwards. Configuration-specific skips are N/A, never execution proof.
The workflows extend all old gates and require `FCAD_FILLET_SESSION_UFBX_EXECUTED`.

## Directed mutations

M30G-1 removes only the radius ticket's `check_form_version(expected)`.
M30G-2 redirects the feature UUID to another valid same-named Fillet in a
multi-link history. Each must compile and fail an executed assertion in
`sessions::tests::fillet::native_one_and_four_fillets_cross_base_history_and_match_cli`.
Restore `session.rs` byte-for-byte and verify its SHA-256, then rerun positive
packed gates. Compiler errors, zero tests and equivalent mutations do not count.

## Extractable real-window recipe

Set `APP` to a freshly staged arm64 bundle. Generator creates only inputs/facts;
comparison refuses missing window outputs before any peer job. Use one owned
viewer under the watchdog; do not redirect to its reserved stdout/stderr files.

```sh
FCAD_30G_GUI_ROOT=/private/tmp/ferrite-30g-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/fillet-session-gui.py "$FCAD_30G_GUI_ROOT"
python3 tools/watch-viewer-memory.py --log "$FCAD_30G_GUI_ROOT/watch.jsonl" \
 --limit-mib 1536 -- "$APP/Contents/MacOS/ferritecad-viewer"
```

1. Open `work/fillets.fcad`; Apply height 9.25. Edit base Sketch and change both
   left-wall X coordinates −4.5 → −5.75; Apply vertices.
2. On dirty Edit Fillet 1, radius 2.75; Apply without confirmation. Edit Fillet 2,
   3.4375; Confirm draft number (no model change), then Apply. Edit Fillet 4,
   4.625; Apply. All selections use UUIDs in `facts.json`, never the name `same`.
   Copy actual `work/fillets.fcad` to `after-apply.fcad`.
3. Edit Fillet 1, type 6.25: the domain corner-bound message disables Apply and
   keeps the text. Worker/solver refusals are covered separately by the native
   constrained gate above. Copy actual
   `work/fillets.fcad` to `after-refusal.fcad`; Cancel radius draft.
4. Export `unsaved.stl` and `unsaved.fbx`. Document Undo last, export `undo.stl`.
   Undo through radius/base edits and Redo all; Save. Copy actual file to
   `saved.fcad`. For a branch keep this session history until step 5.
5. Undo last; Edit Fillet 4, radius 4.75; Apply. Redo disappears. Save As
   `branch.fcad`; copy previous `work/fillets.fcad` to `after-saveas.fcad`.
   Cold reopen `saved.fcad`, then Quit.
6. After Quit check only the owned PID/watchdog journal, never getApp/AX.

```sh
source /private/tmp/ferrite-pr86-review/env.sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/fillet-session-gui.py --compare "$FCAD_30G_GUI_ROOT"
```

Read actual `unsaved.fbx` with pinned
`tools/unity-fbx-smoke/scripts/read_production.c`, `--identity` and `--triangles`;
run `tools/fbx/stl-matches-fbx.py unsaved.stl triangles.txt`. CLI peers never
stand in for GUI outputs. If CUA stalls, stop only the owned PID and record
exactly the incomplete operations. Historical OOM is not claimed fixed.

## Execution record

Local macOS arm64 evidence is under `/private/tmp/ferrite-30g`:

- Packed stub: 96 exact-name gates passed; packed mixed OCCT/no-solver: 12;
  packed combined native: 22. No skip in any required gate. The independent
  mixed recipe's constraint operations are explicitly solver-unavailable N/A.
- All 16 legacy/new Fillet widgets/native regressions passed. The new two native
  scenarios include both neighbours of the fourth Fillet and actual scene/GPU
  preparation failures. Unsaved SQL/STL/FBX match peer CLI.
- Fmt and workspace/all-targets/all-features clippy with `-D warnings` passed;
  export boundary, solver ownership, SPDX (417 files), PlaneGCS pins, YAML parsing
  and `git diff --check` passed.
- M30G-1 and M30G-2 each compiled and failed the executed native assertion;
  `session.rs` restored SHA-256
  `4e12b999348412f789f909aa99c1336efb2a050a757470760a433d2b54887f9f`.
  Restored positive native packed gates passed again.
- Pinned arm64 OCCT/PlaneGCS inputs and existing targets were reused, jobs=2;
  no native dependency rebuild or new target. Combined CLI/viewer were restored
  after mixed mode. Fresh staged viewer is Mach-O arm64.

B-Rep volume is measured on all cold/warm rebuilds and checked within `1e-7`
relative to these analytical values.

| Native model | Analytical mm³ | Independent STL mm³ |
| --- | ---: | ---: |
| One Fillet | 4375.847305899 | 4375.732107839 |
| Four Fillets | 4305.462723652 | 4304.902540397 |
| Constrained | 3827.140469952 | 3826.600114928 |

Strict pinned ufbx passed: complex/JSON 3/3, all prior session markers and
`FCAD_FILLET_SESSION_UFBX_EXECUTED`. Each of the three Fillet FBX files passed
6 identity checks, triangle reading and an oriented STL/FBX join (history/single/constrained:
236/68/260 triangles; worst error ≤ 6.94e−18 m). Log: `strict-reader-final.log`.
The first local invocation lost DYLD at a system-shell exec and failed before
assertions; the successful retry uses `source` as the workflow already does.

Original implementation attempt — GUI **unverified**: CUA reported the Mac locked and automatic unlock unavailable
before selecting the viewer or performing any window operation. Exactly one
fresh viewer was launched under `--limit-mib 1536`; the owned stop marker ended
PID 34674 with SIGTERM, watchdog exit 125, at 33.127 s. Peak footprint
199.735 MiB, pressure 1, swap 1,093,664,768 bytes unchanged. Journal:
`/private/tmp/ferrite-30g-window/watch.jsonl`. No viewer restart/AX observation
followed the stop. The generator made only inputs; missing-output comparator
failed at `after-apply.fcad` before any peer job. No GUI artifact or historical
OOM fix is claimed by that attempt. Independent review completed the recipe below.

Exact post-merge base CI is separate and complete: core CI/pin/runtime 15/15,
plus standalone notices/Rust SBOM/product SBOM 12/12, all successful on
`29c0826afb3ddea6a122cb58744e5f24928a2ee8`. This is the merge SHA's own evidence:
[CI](https://github.com/gesriot/ferrite-cad/actions/runs/37219115925),
[pin](https://github.com/gesriot/ferrite-cad/actions/runs/37219115949),
[runtime](https://github.com/gesriot/ferrite-cad/actions/runs/37219115972),
[notices](https://github.com/gesriot/ferrite-cad/actions/runs/37219115906),
[Rust SBOM](https://github.com/gesriot/ferrite-cad/actions/runs/37219115932),
[product SBOM](https://github.com/gesriot/ferrite-cad/actions/runs/37219115965).

Code/workflow `63feb377f7f0e0b3212cef02ff5ddf7d98396bd3`:
[CI](https://github.com/gesriot/ferrite-cad/actions/runs/37223610478) 7/7 and
[pin](https://github.com/gesriot/ferrite-cad/actions/runs/37223590271) 4/4 passed.
[Runtime](https://github.com/gesriot/ferrite-cad/actions/runs/37223590254) 4/4,
including the platform/release-set aggregate, also passed: core total **15/15**.
Completed logs and authoritative step timestamps proved all seven new exact
gates on every OS: **21 actual executions**, no required skip, plus
`FCAD_FILLET_SESSION_UFBX_EXECUTED` on Linux/macOS/Windows. Echoed argv are not
execution evidence. `code-ci-evidence.json` records each matched test line and
marker; `verify-published-ci.py` performed the audit.

The subsequent evidence commit changes only this verification and the
implementation plan. Its own CI/head is checked separately in
[PR #87](https://github.com/gesriot/ferrite-cad/pull/87), rather than treated as
another native code/workflow run.

## Independent PR #87 review — 2026-10-04

Reviewed `53b9c73d32b9a7f1c1ae1cb2f9a764eee728f3a3`, whose code/workflow
head is `63feb377f7f0e0b3212cef02ff5ddf7d98396bd3`. No production fix was
needed. Read the ticket/worker/form/acceptance changes and their native/widget
tests. GitHub API and completed logs independently confirmed code CI 15/15,
all 21 new exact executions without skips and three Fillet ufbx markers;
the original docs head independently passed CI 7/7.

Review evidence is under `/private/tmp/ferrite-pr87-review`. Fresh release
CLI/app, fmt, workspace/all-targets/all-features clippy `-D warnings`, all 16
Fillet tests and all 22 packed native session gates passed. Existing pinned
libraries/target were reused, jobs=2. One initial local wrapper wrote its log
outside the path its exact-name grep expected; after correcting only temporary
log paths, the complete packed step was repeated successfully. That invocation
error is not counted as a passing gate. Stub/mixed evidence remains the separate
implementation and three-platform CI evidence above, not a new local rerun.

The fresh arm64 bundle at `gui/layout/FerriteCAD.app` (viewer Mach-O UUID
`E553246C-CA7F-35FC-82F2-A93A92065285`) passed staging/closure and solver-info.
The real window, operated through CUA, executed the recipe on `gui-models`:

- Open; Apply height 9.25 and both left vertices −5.75; then Apply Fillet 1
  radius 2.75 without confirmation, Fillet 2 radius 3.4375 after the separate
  Confirm draft number, and Fillet 4 radius 4.625. Confirmation kept the form
  open and the saved value 3.0625 until Apply. Duplicate names were disambiguated
  by the displayed history index and exact UUID.
- Fillet 1 radius 6.25 was refused against the displayed 6.125 bound; Apply
  was disabled, exact input retained and no number clamped. Cancel dismissed
  that draft. The logical file was copied after Apply and refusal for comparison.
- Actual unsaved STL/FBX exports used native Save dialogs. One document Undo
  restored radius 4.25 and its STL was exported. Four more Undo returned to a
  clean title; five Redo restored the edited model. Save cleared the dirty title.
- Undo then radius 4.75 Apply removed Redo. Native Save As wrote `branch.fcad`,
  preserving the previous saved file. Native Open reopened `saved.fcad` with a
  clean title and all accepted radii. The model rendered in isometric view.

`tools/fillet-session-gui.py --compare` consumed all eight actual window
artifacts, not substitutes generated by CLI, and printed
`FCAD_30G_GUI_COMPARE_OK analytical_mm3=4305.462723652 stl_mm3=4304.902540397`.
All SQL cells except `meta.modified_at`, source preservation, Save/Save As,
Undo export and exact unsaved STL/FBX bytes agreed with the peer CLI. GUI STL:
236 triangles, 11,884 bytes; FBX: 43,652 bytes. Pinned ufbx 0.23.0 strict read
the GUI export and three independently regenerated native Fillet exports:
each passed 6 identity checks; oriented joins passed at 236/236/68/260 triangles,
worst coordinate difference ≤ 6.94e−18 m. Logs: `gui-compare.log`,
`strict-joins.log`, `native-all.log`, `fillets.log`, `clippy.log`.

Exactly one owned viewer, PID 67696, ran under the 1536 MiB/1200 s watchdog.
It closed normally after 531.975 s, exit 0, `aborted:false`. Across 968 samples:
peak footprint 213.673 MiB, pressure always 1, swap unchanged at 1,093,664,768
bytes, minimum available disk 137.198 GiB. Journal: `watch.jsonl`. After closing,
only the watchdog and PID were checked; no viewer AX/getApp call restarted it.
This is a successful bounded GUI scenario, not a fix for the historical OOM.
Linux/Windows GUI and full GPU/pixel/large-STEP reruns were not performed here.
