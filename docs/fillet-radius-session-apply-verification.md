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

GUI **unverified**: CUA reported the Mac locked and automatic unlock unavailable
before selecting the viewer or performing any window operation. Exactly one
fresh viewer was launched under `--limit-mib 1536`; the owned stop marker ended
PID 34674 with SIGTERM, watchdog exit 125, at 33.127 s. Peak footprint
199.735 MiB, pressure 1, swap 1,093,664,768 bytes unchanged. Journal:
`/private/tmp/ferrite-30g-window/watch.jsonl`. No viewer restart/AX observation
followed the stop. The generator made only inputs; missing-output comparator
failed at `after-apply.fcad` before any peer job. No GUI artifact or historical
OOM fix is claimed. The recipe above remains for independent review.

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
another native code/workflow run. The PR remains open without auto-merge.
