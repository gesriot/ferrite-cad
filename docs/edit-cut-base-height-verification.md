# §26G — verification record

## Base and scope

Work is on `edit-cut-base-height`, starting from a clean checkout after a fresh
fetch. `HEAD`, `main` and `origin/main` were
`eacce8f17fa0059dd163e308feb34ddd24a4404d` (PR #49 merge). Parents:
`dd32ae36d524b1f22752fbc11bc4b49b03a8dd41` and
`2adc7d1c6a250a626beedf6757ff3f132d3bad24`. Merge and reviewed-head trees:
`8db37513e6918b07b4ad0c3448b4461da832866c`.
Fresh GitHub API evidence confirms 27/27 successful checks and 6/6 workflows.
The downloaded complete runtime log for run `35434089815` contains each of the
161 required tests, no skips, and 61 pinned ufbx reads with zero failures on
**each** of macOS/Linux/Windows. This is evidence for the **base**, not this
uncommitted diff. Evidence lives under `/private/tmp/ferrite-26g/` in
`base-pr.json`, `base-checks.json`, `base-workflows.json`, `base-runtime.log`,
`baseline-runtime-evidence.json` and `base-gates.json`.

The original bug was reproduced before changing production code: a fresh real
16-Cut CLI fixture at 12 mm was copied to 14 and 10 mm, both exit 0, validation
without warnings/errors, all 299 old refs resolved. Inspection nevertheless
refused the Sketch/Cut history: missing floor names at 14, excessive tool depth
at 10. See `baseline/reproduction.json`, `reproduction.log` and baseline topology
logs. The policy in [the contract](edit-cut-base-height.md) was written before
implementation. §26F is reused, not reimplemented.

## Implementation and assertions

One shared `CutHistory` yields Cut/Sketch/height discovery on the pinned snapshot.
The height dispatch checks boolean/edge/tip/carried-name signals and forces the complete
reader for damaged histories. `BaseHeightContext` validates every absolute tool
with the existing cut clearance and exact depth/floor classification. The Cut
editor and height editor share floor transitions and new own/descendant Origin
name construction. There is no new schema, boolean or copier.

`write_extrude_height` re-reads and re-derives the proposed payload, complete
current history and required new names inside the existing checked transaction.
A full content version catches changed tools even when the base row is unchanged.
Thirteen forged preparation variants exercise payload/context/name/UUID
protection, including cross-domain collisions. Direct SQLite failure retains a
typed IO error and cause. The common job requires every baseline and new ref for
a Cut history; the legacy standalone path keeps its former weaker ref policy.

Native matrices cover 1/2/4/16 tools, mixed through/pocket and 4/16 all-pocket
histories, simultaneous new floors, same height, allowed shrink and next edit.
Assertions examine every saved reference by UUID, producing feature, semantic
role and actual B-Rep surface, including historical results. Analytical solid
volume/extents and the independent STL parser check manifold edge use, winding,
bores/floors/through openings and tessellated volume with the existing mesh
error bound. No nearest-geometry fallback is used. SQL comparisons include all
cells and rowids; only base payload/hash, strictly necessary new names and
`modified_at` may change. Existing capabilities, old refs and source bytes are
preserved. The same-document cache is populated, height changes invalidate base
and all descendants (Miss), repeated rebuilds Hit and equal cold results.

Guards cover distant excessive depth (Cut 10 of 16), every protected floor UUID,
invalid/foreign/stale requests, occupied targets, source aliases/hardlinks,
0.1/0.4/0.95 cancellation, late source changes and live SQLite handle cleanup via
the existing guard helper. Closed stdout returns actual exit 7 after publication;
the result is inspected, never blindly retried.

The app gate operates the actual form and worker on 4/16 histories, renders in
the complete 988×768 layout, edits the field through egui input, preserves Save
Cancel and failed-worker input, ignores stale replies and restores the draft
after a failed matching asynchronous Open. UI and CLI compare all SQL tables
with an explicit mapping of **only new ref UUIDs**; old IDs remain exact and STL
and FBX bytes match. Widgets/worker tests are distinct from the native GUI below.

## Native regression execution

Sequential builds, `CARGO_BUILD_JOBS=1`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, reused
`/private/tmp/ferrite-24b-native-target`; vendor OCCT/PlaneGCS was not rebuilt.

| Invocation/log | Result |
| --- | --- |
| document/topology/eval/jobs, release + planegcs, `core.log` | 511 harness passes; 509 applicable tests, 2 solver-absence tests explicitly return as inapplicable; 1 existing ignored benchmark |
| circular_cut/edit_circular_cut/edit_sketch/json_v1/print_topology, `cli.log` | 59 passed, zero skips |
| app `edits::`, `edits-app.log` | 9 passed, zero skips |
| app `cuts::`, `cuts-app.log` | 8 passed, zero skips |
| exact redraw scheduling test, `redraw.log` | 1 passed, zero skips |
| workspace clippy, all targets/all features, `clippy.log` | `-D warnings` passed |

Empty doc-test or filtered secondary harnesses are not counted as executed tests.
Counts above are suites; later exact repeats are not added to inflate totals.

## Runtime integration

Existing 161 runtime gate names and the old 61 reader calls are retained.
The old runtime workflow lines remain in order, and the old reader script is an
exact prefix of the extended script. Seven new runtime exact names (six native,
one OCCT/no-solver) make **168**; three small height FBXs extend the same pinned
reader loop to **64** calls per OS. `runtime-gates.json` records the name sets.
The diff has not run in remote CI; these are configured totals and local packed
step executions, not a claim of Linux/Windows success for this diff.

## Mutation proof

Two temporary changes were compiled and executed sequentially, with full source
byte backups and restoration in `finally`:

1. New floor generation yielded zero refs. The native matrix failed at
   `every new own/descendant floor has exactly its required name`.
2. The height writer's transaction re-derivation closure became `Ok(())`.
   The forged-preparation test failed at `forged height must be rederived`.

Both runs executed exactly one test and reported one assertion failure (not a
compile error/empty harness). Logs: `missing-floors.log`, `unchecked-writer.log`;
restoration digests and results: `mutations.json`. The corresponding restored
positive runs are recorded separately in `*-restored.log`.

## Stub execution

Reused `/private/tmp/ferrite-25j-stub-target` **debug**, with jobs=1. Its old
artifacts had partly disappeared, so only the necessary CLI/test dependencies
were rebuilt. A local CMake wrapper adds ignore prefixes for Homebrew, `/usr/local`
and the vendor install, disables package registries and requests NOTFOUND; it
runs real CMake, without changing repository build logic or system packages.
Before executing tests, the fresh bridge CMakeCache said
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`; `otool -L` of both fresh CLI and
circular_cut harness contained only libiconv/libSystem, no libTK/planegcs.
Paths, binary SHA256 and imports: `stub-proof.json`; build/configuration logs:
`stub-build.log`, `stub-test-build.log`, `stub-artifacts.jsonl`, `stub-env.sh`.

The extracted CI block executed all three new exact discovery/writer gates,
zero skips (`packed-stub.log`). Existing CLI edit-extrude protocol and real
no-kernel refusal tests also both passed (`stub-standalone.log`). Structural
refusals are exercised directly through document preparation and JSON discovery.
The valid CLI height request refuses kernel-first with source bytes/output
unchanged; this is **not** evidence of a late copy guard running without OCCT.

The final dispatch check also rejects a fixture whose Cut payloads were changed
to NewBody and predecessor/target edges removed: saved Carried/Origin roles still
require full history validation. This fifth corruption variant runs in the same
exact discovery gate, native and stub. The final packed runs use a fresh
`final-runtime/cut-base-height` directory: reusing the earlier FBX destinations
correctly triggered the existing no-clobber guard (`packed-repeat-occupied-artifacts.log`).
No output was overwritten to make that repeat pass.

## Mixed execution and packed argv

The extracted runtime mixed block executed **9 exact tests**, no skips: plain
circles/annuli, Cut, Cut edit, sequential creation/edit, bounded history, base
Sketch and base height. `packed-mixed.log` records the first run;
`packed-mixed-final.log` repeats it after the final conservative dispatch guard.
The fresh selected no-default-features harness and CLI import libTK and **no
planegcs** (`mixed-proof.json`, final `mixed-proof-final.json`). No constrained
geometry is promised without the solver.

`packed-native-final.log` has all six new native exact gates, zero skips;
`packed-stub-final.log` has all three stub gates, zero skips. The actual saved
shell scripts match their YAML run blocks after only blank-line normalization
and the local macOS matrix substitution (`packed-source-equality.json`). These
are executed cargo argv, not just bash/YAML syntax checks. The final document
catalogue regression filter has 40 passes and the same one existing ignored
benchmark (`catalogue-final.log`). Old standalone job/app/JSON tests remain in
the regression suites above, plus the exact CLI protocol gate (`standalone.log`).

## Pinned reader and executed Markdown recipe

The existing pinned ufbx 0.23.0 source at commit
`fcc5d6ba444cfd3eb80677dba5e37e493941abe5` passed the fetch script's SHA256 checks.
The existing production C reader was compiled sequentially and the literal added
reader block ran on `height-0/1/2.fbx`, each `checks=6 failures=0`:
`stage-and-reader-final.log`, `height-*-reader.txt`. No large STEP/pixel campaign
was repeated, and unchanged inventories were not regenerated.

The recipe was extracted from Markdown and run using the fresh final staged
CLI with both DYLD variables unset, producing `FCAD_26G_RECIPE_OK` in `recipe.log`.
It creates fresh models, discovers exact UUID/version through JSON, executes
1/2/4/16 height edits, checks SQL/source preservation, validates/cold-rebuilds,
parses STL and reads three additional small FBXs. It exercises both refusals and
actual lost stdout/exit 7, then inspects the published file without retrying the
edit. The first recipe run exposed a wrong DTO field assumption; the corrected
recipe derives existing floor facts from `protected_floors`, then ran completely
on fresh fixtures. The earlier partial fixture was not overwritten.

## Native GUI, parity and resource limits

macOS reported unlocked; native CUA was available. Fresh final bundle:
`/private/tmp/ferrite-26g/stage-final/FerriteCAD.app`. Both executables passed
runtime closure inspection (50 OCCT dylibs + 1 PlaneGCS, zero unexpected), staging
and `codesign --verify --deep --strict`; 55 staged files, 95,527,171 bytes.
With DYLD unset, CLI help ran and viewer `--solver-info` reported the pinned
FreeCAD 1.0.1 solver/archive SHA256. No raw viewer or loader-failure probes ran.

The primary process, PID 73170, was started stopped and measured before first
opening by `tools/watch-viewer-memory.py --limit-mib 1536 --seconds 1200`.
Native CUA performed this functional scenario in its own staged viewer:

1. Opened a fresh 16-Cut source, selected the original NewBody and entered 14.25.
   The form explained unchanged absolute Cut depths.
2. Opened Save, cancelled, observed the same 14.25 input, then published
   `/private/tmp/ferrite-26g/gui-published.fcad`; asynchronous Open accepted it.
3. Opened the existing Cut editor: thickness 14.25, unchanged tool depth 12,
   now a pocket with saved floor. Opened Edit Sketch: same 80×50 rectangle,
   16 tools, height 14.25. Cancelled both unmodified drafts. Add Cut is correctly
   unavailable at the 16-tool limit, while all 16 Cut entries remain editable
   in JSON/native assertions.
4. Entered 12 for the base height. The form retained that input, showed the first
   Cut UUID and all 16 protected floor-reference UUIDs, and disabled Save.
5. Compared the published GUI file against a peer CLI edit of the same original
   source/version to 14.25. All 299 original refs/SQL cells remained exact;
   72 new names matched by full semantic meaning, with only their UUIDs and
   `modified_at` normalized. Both files validated/cold-rebuilt, passed the
   independent STL check and pinned reader, and had byte-identical exports.

| GUI/CLI artifact | Bytes | SHA256 (both files) |
| --- | ---: | --- |
| STL | 403884 | `954a3b2236dd4ef6dded9a70b9042111375d763e9689a37168582b27646def29` |
| FBX | 504420 | `2500cac679df68e3f1db554ccc3188305bbd508675b893e0f50486f0e8e36832` |

`gui-input.json`, `gui-parity.py/log/json` and `gui-*-ufbx.txt` record source
hash/version, explicit new-UUID mapping and comparisons. CUA screenshots/steps
are in this task's tool trace; they are not substitutes for the headless tests.
The primary viewer closed normally, exit 0, 322.900 s, no watchdog abort. Peak
physical footprint **213.126 MiB**, peak RSS 134.844 MiB, pressure 1 throughout,
swap 0. `gui-watch.jsonl`, `.stdout`, `.stderr`, `gui-resources.json` preserve the
measurements. The 1536 MiB limit was never raised. This does not establish the
cause or resolution of the earlier OOM.

**GUI protocol deviation:** immediately after normal close, CUA's `getAXState`
automatically relaunched the same staged application as a second, empty process
(PID 74226), outside the now-completed watchdog. It was closed immediately
through its native close button, without opening a document or interacting with
other applications. A final process check found no viewer remaining. Therefore
the functional guarded scenario passed, but the requested single-process /
every-launch-guarded GUI procedure was **not perfectly met**. No bounded memory
claim is made for that brief automatic relaunch. No further GUI run was attempted.

## Final checks and limits

Final `clippy-final.log`: workspace all-targets/all-features with `-D warnings`
passed after all Rust changes. `audit-final.log`: fmt, actionlint for both changed
workflows, shellcheck for the reader script, export-boundary, 360 tracked MIT
headers plus the two new Rust files, and diff whitespace passed. Neither index
nor HEAD changed. No commit, push, PR, merge or subsequent slice was started.
The detached `a200` worktree remains at
`5b20719145222befc0bd25647f47ba3d67fee66d`; its files/processes were not touched.

Initial free disk was about 81 GiB, final about 78 GiB; swap stayed 0 in the
recorded checks. Native target 2.6 GiB, stub target 2.3 GiB: only these two reused
targets, no third target and no vendor rebuild/foreign cleanup. Builds/tests ran
sequentially with jobs=1. macOS local results do not establish Linux/Windows or
remote CI success for this uncommitted diff. The preserved base CI evidence and
configured 168/64 runtime totals must not be confused with executed diff CI.

The 13 operation-owned CI `tee` logs have been moved from the repository root
to `/private/tmp/ferrite-26g/step-logs/`; no unrelated file was removed. Compact
logs, proof JSON, scripts, patch, new source files and GUI fixtures are also
preserved outside temporary storage at
`/Users/drt/.codex/visualizations/2026/09/19/01a0b877-07d6-7701-a649-b82de0e4a3ee/ferrite-26g-evidence/`.

## Unstaged review inventory

All 21 modified tracked files and 4 new untracked files are listed below. The
index is empty; all edits remain unstaged and uncommitted.

| Status | File | Added lines | Removed lines |
| --- | --- | ---: | ---: |
| M | `.github/workflows/ci.yml` | 19 | 0 |
| M | `.github/workflows/runtime-layout.yml` | 47 | 0 |
| M | `README.md` | 5 | 0 |
| M | `crates/ferritecad-app/src/edits.rs` | 401 | 11 |
| M | `crates/ferritecad-app/src/main.rs` | 1 | 0 |
| M | `crates/ferritecad-cli/src/json.rs` | 35 | 0 |
| M | `crates/ferritecad-cli/tests/support/edit_cut_base_sketch.rs` | 1 | 1 |
| M | `crates/ferritecad-cli/tests/support/sequential_circular_cuts.rs` | 25 | 8 |
| M | `crates/ferritecad-document/src/cut_edit.rs` | 226 | 61 |
| M | `crates/ferritecad-document/src/document.rs` | 56 | 0 |
| M | `crates/ferritecad-document/src/edit.rs` | 17 | 1 |
| M | `crates/ferritecad-document/src/lib.rs` | 6 | 0 |
| M | `crates/ferritecad-jobs/src/edit.rs` | 19 | 37 |
| M | `crates/ferritecad-ui/src/edit.rs` | 31 | 19 |
| M | `docs/circular-cut-history.md` | 4 | 0 |
| M | `docs/cli-capabilities.md` | 6 | 0 |
| M | `docs/cli-json-v1.md` | 10 | 0 |
| M | `docs/edit-circular-cut-copy.md` | 3 | 0 |
| M | `docs/edit-extrude-copy-verification.md` | 5 | 0 |
| M | `docs/implementation-plan.md` | 17 | 0 |
| M | `tools/check-fbx-complex.sh` | 12 | 0 |
| ?? | `crates/ferritecad-cli/tests/support/edit_cut_base_height.rs` | 588 | 0 |
| ?? | `crates/ferritecad-document/src/height_edit.rs` | 258 | 0 |
| ?? | `docs/edit-cut-base-height-verification.md` | 287 | 0 |
| ?? | `docs/edit-cut-base-height.md` | 297 | 0 |

Tracked diffstat: **21 files, +946/−138**. New files: **4, +1430 lines**.
Combined review size: **25 files, +2376/−138** (new files counted from empty).

## Independent review — 2026-09-24

Fresh fetch confirmed the same PR #49 merge and 27 successful check runs on
`eacce8f17fa0059dd163e308feb34ddd24a4404d`. Code review covered history dispatch,
shared floor transitions, transactional re-derivation, old/new reference checks,
standalone compatibility, and draft/current-load handling. No blocking defect
was found; production code did not need a review correction.

Independent release runs required OCCT and PlaneGCS: 509 applicable domain tests
(511 harness passes, two explicitly inapplicable no-solver cases, one existing
ignored benchmark), 59 CLI tests, nine edit workers, eight Cut workers, and the
idle-redraw regression passed. Both standalone edit process tests passed as well.
Workspace all-targets/all-features clippy with `-D warnings`, fmt, actionlint,
shellcheck, export boundary and whitespace checks passed. The genuine stub ran
five discovery/writer/standalone checks without skips; its CMake cache says
`OpenCASCADE_DIR-NOTFOUND` and its CLI imports no OCCT/PlaneGCS. The new mixed
OCCT/no-solver height gate executed successfully; native binaries were restored
afterward. The implementer's source mutations were reviewed, not rerun here.

A newly staged, strictly signature-verified bundle ran without DYLD overrides.
Its `--solver-info` confirmed the pinned solver. One viewer (PID 92548) was
started under the independent 1536 MiB watchdog before CUA selected it. On the
private 16-Cut source, actual native controls exercised:

- base height 12 → 14.25 mm, Save Cancel with the value retained;
- Save publication and async Open, with the accepted copy showing 14.25 mm;
- return to 12 mm refused with the Cut and protected floor UUIDs, Save disabled;
- reopening Edit Cut showed unchanged depth 12 mm and a named pocket floor;
- reopening Edit Sketch showed the unchanged 80×50 rectangle and height 14.25 mm.

CUA's simulated typing dropped characters in the native filename field: the
actual new file was `-ld.cd.fcad`, not the requested `gui-published.fcad`. The
observed destination was used for all subsequent comparisons; no original file
was replaced. This was a GUI automation deviation, not a claimed filename test.
The actual GUI copy and a fresh CLI copy matched every SQL cell after mapping
only 72 genuinely new reference UUIDs by producer/role and `meta.modified_at`.
All 299 original refs stayed exact; the source SHA-256 stayed unchanged.
STL and FBX were byte-identical (403884 and 504420 bytes, respectively), the STL
was independently measured, and pinned ufbx read both FBXs with 6 checks and
zero failures each. The public Markdown recipe also ran against the fresh bundle.

The single guarded viewer exited normally after 222.9 s: **201.001 MiB** peak
footprint, normal pressure, swap 0, exit 0. No CUA app query was made after Quit;
completion was checked through the watchdog, so this review did not repeat the
automatic unguarded relaunch recorded above. These observations do not establish
the cause of the earlier OOM. GUI on other operating systems was not exercised.

Logs, private models, parity proof and the watch trace are in
`/private/tmp/ferrite-26g-review/`; compact evidence is also preserved in the
reviewer's durable visualization directory. Static extraction confirmed all 161
previous runtime gates remain and seven are added (168 per OS); ufbx adds three
reads (64 per OS). Execution of the published head and merge is audited
separately from these local checks. The large STEP campaign is left to that CI.
