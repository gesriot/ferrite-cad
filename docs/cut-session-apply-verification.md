# §30F — verification

[Contract](cut-session-apply.md). Marker `FCAD_30F_CUT_SESSION_APPLY`.
Base `306c54b76dff6fd6ec2345b5626141629286c861`, tree
`8536281c097a68433fa9a2d578d0cc4664123da6`, equal to final PR #85 head's tree.
Clean `main == origin/main` after fetch; PR #85 MERGED. Branch
`cut-edit-session-apply`. Author/committer `gesriot <gessman1618@gmail.com>`.
No merge or auto-merge. Post-merge base CI and published-head CI are separate.

The new structural widget gates use real egui and `can_apply_cut`, including a
changed idle request before busy controls. They cover unconfirmed current fields,
draft/document action separation, numeric/ThroughAll no-op and draft Redo, Add
exclusion, dirty discovery, clean/dirty copy, invalid ranges/actual concave outline/
far-tool clearance with retained text, busy/other-form/no-session, readable status
inside the form and accepted-scene dismissal.

Native gates use a six-Line asymmetric concave outline:
`(-2.5,-1.25),(84,-1.25),(84,28),(54,28),(54,55),(-2.5,55)` at height 12.75 mm.
The 16-tool fixture permutes positions by `i*7 mod 16`, varies radii, alternates
ThroughAll and absolute Blind pockets, reverses object rowids/ordinals, duplicates
names, offsets capability rowids and adds optional capability and an extra SQL
table. Height 15.25 and left-wall X −3.75 are applied before first/middle/last
Cut edits. Separate one- and three-link chains cover Blind-through ↔ ThroughAll,
through → pocket with own/downstream floors, refused protected-floor loss with
UUIDs, wrong tool UUID, excess depth, far-disk collision and a point inside the
bbox but outside the concave part.

Real session workers and independent peer CLI compare every SQL cell, including
rowid where present. Only `meta.modified_at` and explicit UUID pairing of genuinely
new `topology_refs.id` cells differ. Existing refs are equal without normalization;
new refs match complete role, owner, producer, selection and links. No table, row,
payload or hash is dropped. Raw object and topology payload hashes are validated;
new reference IDs are stored in separate SQL columns and never occur in payloads,
so normalized payloads/hashes equal their checked raw bytes. Cold rebuild resolves
every ref; Redo restores exact new UUIDs. Unsaved STL/FBX bytes match CLI on this OS.
Independent B-Rep volume and signed STL volume, closure, bore direction/reach and
floor/far-cap coverage distinguish holes from bosses and pockets. Pinned strict
ufbx reads artifacts and joins oriented triangles with STL.

## Local commands

Use the already pinned arm64 libraries and existing targets; native inputs were
not rebuilt. OCCT 8.0.1, FreeCAD/PlaneGCS 1.0.1, Eigen 3.4.0 and Boost 1.91.0.
All three archive SHA-256 values were independently checked against the pin.
Builds are sequential with `CARGO_BUILD_JOBS=2`; loader failure probes are unset.

```sh
source /private/tmp/ferrite-pr85-review/env.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --release --features planegcs -p ferritecad-app cuts::tests::session_apply -- --nocapture --test-threads=1
cargo test --release --features planegcs -p ferritecad-app --bin ferritecad-viewer cuts::tests:: -- --nocapture --test-threads=1
```

Extract the exact run blocks from the existing workflows, replacing only
`${{ matrix.name }}` with `macos`, into `/private/tmp/ferrite-30f/`:

- ci.yml: `Open, edit, Undo, Redo and Save one document without native geometry`
- runtime-layout.yml: `Edit, Undo, Redo, export and Save one open document through the native window state`
- runtime-layout.yml: `Chamfer a plate with Open CASCADE and no solver`

Execute the packed argv, not another approximate command list. Source the native
environment *inside* bash because SIP removes DYLD variables when launching bash.
Set `RUNNER_TEMP=/private/tmp/ferrite-30f/gates`, `GITHUB_ENV=$RUNNER_TEMP/github-env`,
`FCAD_OCCT_LIB_DIR=$PWD/vendor/install/lib`. For the mixed block use a temporary
symlink facade whose `target` points to the selected existing native target,
following the [§30E recipe](revolve-angle-session-apply-verification.md). Restore
the planegcs CLI/app before peers/GUI. Stub uses only the existing
`/private/tmp/ferrite-25j-stub-target`; its bridge cache disables OCCT package
discovery, and native/solver env is unset. Configuration-specific skips are not
execution evidence. New exact-name/no-skip gates extend both workflows; older
gates remain. `FCAD_CUT_SESSION_UFBX_EXECUTED` is required by runtime CI.

## Directed mutations

M30F-1 removes only `check_form_version(expected)` in `StepTicket::edit_circular_cut`.
The exact native 16-link gate must compile and fail its executed stale-refusal
assertion. M30F-2 deliberately redirects the Cut edit to another saved Cut while
retaining valid relative geometry and that target's extent; the same gate must
compile and fail its executed selected-Cut assertion. Save source bytes first,
restore them exactly, verify SHA-256 and rerun positive gates. No universal mutation
framework or destructive Git operation.

## Real macOS window recipe

Use a freshly staged arm64 bundle and exactly one owned viewer/watchdog. Set
`APP` to that bundle and choose a new fixture root; the generator and watchdog
deliberately refuse existing destinations/logs. Do not redirect shell output to
the watchdog's reserved `.stdout` / `.stderr` files.

```sh
FCAD_30F_GUI_ROOT=/private/tmp/ferrite-30f-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/cut-session-gui.py "$FCAD_30F_GUI_ROOT"
python3 tools/watch-viewer-memory.py --log "$FCAD_30F_GUI_ROOT/watch.jsonl" \
 --limit-mib 1536 -- "$APP/Contents/MacOS/ferritecad-viewer"
```

The generator creates only source/work inputs and facts. It never creates the
required real window outputs. Missing output makes comparison fail before peer
jobs run. All artifacts below belong in the fixture root.

1. Open `work/cuts.fcad`. Apply height 15.25. Edit base Sketch: the two left-wall
   X coordinates −2.5 → −3.75; Apply vertices.
2. On the dirty document edit the first Cut (centre 7.125,6.375). Centre X 7.5,
   radius 1.625, Blind depth 6.125. Confirm draft numbers → Undo → Redo. Apply cut
   without Save dialog; form closes and three new floor/Origin names are accepted.
3. Edit middle Cut (43.125,18.375): X 43.5, radius 1.75, depth 5.625; Apply without
   confirmation. Edit last (31.125,42.375): X 31.5, radius 2.0, ThroughAll; Apply.
   Capture the actual user's still-original `work/cuts.fcad` as `after-apply.fcad`.
4. Edit first, choose Through all: concrete protected-floor refusal and retained
   draft; capture actual `work/cuts.fcad` as `after-refusal.fcad`. Cancel draft.
5. Export `unsaved.stl` and `unsaved.fbx`. Document Undo last, export `undo.stl`.
   Undo through middle/first/base steps, then Redo all; Save. Capture actual saved
   `work/cuts.fcad` as `saved.fcad` and cold reopen it.
6. For branching retain the original session history: perform this before the
   optional cold reopen, or repeat the editing chain in a fresh source session.
   Undo last, edit last to X 31.625, radius 2.125, ThroughAll; Apply. Redo disappears.
   Save As `branch.fcad`; capture prior `work/cuts.fcad` as `after-saveas.fcad`
   (byte-equal to saved.fcad).
7. Quit. Afterwards check only the owned PID/watchdog, never getApp/AX.

```sh
source /private/tmp/ferrite-pr85-review/env.sh
python3 tools/cut-session-gui.py --compare "$FCAD_30F_GUI_ROOT"
```

The comparator compares actual GUI SQL/geometry/export results with temporary
CLI peers and rejects seven controls: missing artifact, original instead of Save,
wrong chosen Cut, wrong branch, stale export, corrupt raw payload hash and wrong
new-ref owner. Independently read actual `unsaved.fbx` with pinned
`tools/unity-fbx-smoke/scripts/read_production.c`, `--identity` and `--triangles`,
then `tools/fbx/stl-matches-fbx.py unsaved.stl triangles.txt`. No CLI stand-in is a
GUI artifact. A locked/unavailable screen leaves this recipe unverified.

## Execution record

Local macOS arm64: initial packed native block passed all 20 exact gates without
skips, including §30A–E regressions and both new Cut geometry gates. All three
widget tests passed after making the document status readable at the top of the
form. Full workspace/all-targets/all-features clippy with `-D warnings` passed.
Export boundary, solver ownership, 414 licence headers, PlaneGCS pin ownership,
workflow YAML syntax and diff whitespace passed. System Python lacked PyYAML;
system Ruby/Psych successfully parsed both workflow files instead.

M30F-1 compiled and its actual stale-request refusal assertion failed: the worker
returned Show instead of Failed. M30F-2 compiled and its selected-Cut centre
assertion failed: actual (7.125,6.375), requested (7.5,6.375). Restored session.rs
SHA-256 `160e6698adaf5157139ba46726ad9800bff0bd8eb8cd00d60c15ffcbe84301d4`.
The positive packed gates are repeated after restoration. Initial M30F-1 log
validation expected a later diagnostic string; the earlier executed refusal
assertion already caught the bypass and was inspected directly.

| Artifact | Analytical / B-Rep mm³ | Independent STL approximation mm³ |
|---|---:|---:|
| cut-history (16) | 61492.588703250 | 61498.062971822 |
| cut-single | 52483.033749354 | 52483.490788396 |
| cut-floor | 51472.997980512 | 51473.849071463 |

The GUI fixture generator refused a checkout destination; comparison with missing
real output failed before any peer job. Logs and recoverable artifacts are under
`/private/tmp/ferrite-30f/`. §30, Milestone 5C and product completion are not
claimed; the next slice has not started.


The restored native packed block passed its 20 exact gates again. All three new
widget gates then passed on the reordered fixture (an old test-only assumption
that the first Sketch was the base was replaced by its saved UUID). The mixed
OCCT/no-solver packed block passed all 11 exact tests and its existing CLI recipes
without skips. No native inputs were rebuilt.

The complete strict-reader script passed the existing complex corpus (256
independent checks, 986837 triangles, 233291656 bytes), §30D/E exports and all
three Cut artifacts. Each Cut FBX passed six strict identity checks; oriented
STL/FBX joins: cut-history 2660 triangles, cut-single 192, cut-floor 528, worst
difference 6.94e-18 m. Actual `FCAD_CUT_SESSION_UFBX_EXECUTED` was emitted.

**GUI unverified.** Fresh arm64 staged bundle:
`/private/tmp/ferrite-30f/gui/layout/FerriteCAD.app`, 55 files, 97565827 bytes.
The first watchdog refused before creating a process because the shell redirect
used its reserved stdout filename; no GUI evidence comes from that attempt.
The second watchdog owned PID 36323 with the required 1536 MiB limit. CUA
getApp calls stalled (57 minutes on Finder, almost five hours on the viewer)
despite a requested 10-second timeout. The watchdog reached its 1800-second
time limit and stopped PID 36323 (exit −15, experiment aborted). Rechecked the
owned watchdog journal: armed at 2026-10-04 01:11:44 PDT, aborted with reason
`time limit` at 01:41:44 after 1800.059 seconds, then exited at 01:41:45. This was
the elapsed-time limit, not the memory limit. The late getApp
then launched our temporary bundle as PID 46248 outside the expired guard; its
exact executable path was checked and only that owned PID was terminated. Both
PIDs were verified absent. No further app/AX call was made. No geometry edit or
real GUI output was produced; missing-output refusal remains mandatory and
CLI peers never fill these artifacts. The recipe above remains reproducible.
The seven comparison controls that require actual GUI artifacts were **not run**;
only checkout-destination and missing-output preflight controls ran.

Guarded startup peak footprint 223.67 MiB, pressure normal, swap unchanged at
1127219200 bytes. After cleanup memory pressure reported 60% free and disk 138 GiB
free. No foreign process, cache, native pin or detached worktree was stopped or
changed. Historical OOM cause remains unestablished.

Exact post-merge base 306c54b completed 15/15 core checks separately:
[CI 7/7](https://github.com/gesriot/ferrite-cad/actions/runs/37183335361),
[combined runtime 4/4](https://github.com/gesriot/ferrite-cad/actions/runs/37183335342),
[PlaneGCS pin 4/4](https://github.com/gesriot/ferrite-cad/actions/runs/37183335357).
All saved job responses name the exact base SHA and success.

The existing stub packed argv passed all 92 exact gates without skips, including
the three Cut widget gates and native-kernel refusal. After restoring the combined
native CLI/viewer, `cargo fmt --all -- --check` and the final full workspace
clippy command passed. Stub and combined builds used the existing targets and
two build jobs sequentially.

All 14 affected Cut widget/copy tests passed on the final native sources, including
the prior Add/Edit, polygon, sequential/history and ThroughAll worker/CLI paths.

## Published code/workflow head

Exact head `bf659c3d3dd02cd6be9a6f251dca987d13b395f0` completed 15/15 checks:
[CI 7/7](https://github.com/gesriot/ferrite-cad/actions/runs/37204689689),
[combined runtime 4/4](https://github.com/gesriot/ferrite-cad/actions/runs/37204654720),
[PlaneGCS pin 4/4](https://github.com/gesriot/ferrite-cad/actions/runs/37204654706).
Each run and job was checked against this head separately from the post-merge base.
Saved full logs contain 21 actual new exact-name Cut gate executions: four
widget/stub gates, two native gates and one mixed gate on each of the three OSes.
All three runtime jobs emitted the actual `FCAD_CUT_SESSION_UFBX_EXECUTED` marker.
The workflow's no-skip and strict-reader assertions succeeded.

GitHub CLI labelled some Linux runtime output `UNKNOWN STEP`. Those lines were
matched only to the exact successful step's timestamp interval from authoritative
job metadata (completedAt has one-second precision), then to the literal executed
`test … ... ok` lines or reader marker. Commands merely echoing gate names were
not counted. Raw logs, metadata and extracted evidence remain under
`/private/tmp/ferrite-30f/`.

This evidence follow-up changes only this verification and the plan. Its separate
docs-only head/CI result is recorded in [PR #86](https://github.com/gesriot/ferrite-cad/pull/86).
The PR stays open for independent review, without merge or auto-merge. Real GUI
and its seven artifact-dependent negative controls remain unverified as above.
