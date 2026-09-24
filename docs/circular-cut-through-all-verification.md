# §26H verification — explicit ThroughAll for circular Cuts

[Contract and recipe](circular-cut-through-all.md).

This file records what was executed and where. There are three places: the
cloud sandbox this slice was developed in, GitHub Actions on the pull request,
and the independent macOS review appended below. A result from one place is never
reported as a result from another. CI on the base branch is not CI on this
diff. The windowed GUI was **not** run in the cloud; the independent macOS
review below records the subsequent real window checks.

## Base and branch

| Fact | Value |
| --- | --- |
| Base | `main` = `6aef5017cc0ef8d6c0a4cc8ae9c7de765209c021` (merge of PR #50), tree `3f84f22a49b6c0068925d99b66cf7221e365cb5e` |
| Branch | `claude/blissful-wright-8rfyr2` (assigned push branch of the cloud session; the preferred name `circular-cut-through-all` was not available to push) |
| Pull request | gesriot/ferrite-cad#51 (ready for independent review at cloud handoff) |
| Commits | `88f9490` implementation, `d10ad56` strict extent parsing, `7050b4b` JSON v1 types kept + `_v2` blocks, `409a31f` CI gate name, then the verification commit that adds this file |
| Author/committer | `gesriot <gessman1618@gmail.com>` on every commit; no AI co-author or generated-by trailers |

Diffstat against `6aef501` before this file: 33 files, +3430/−350. Product
code: `ferritecad-document` (`cut_edit.rs`, `document.rs`, `height_edit.rs`,
`model.rs`, `schema.rs`, `lib.rs`), `ferritecad-eval` (`convert.rs`, `cold.rs`,
`cache.rs`, `lib.rs`), `ferritecad-jobs` (`edit.rs`), `ferritecad-cli`
(`cut.rs`, `edit_cut.rs`, `json.rs`), `ferritecad-app` (`cuts.rs`). Tests:
new `tests/support/circular_cut_through_all.rs` plus mechanical `CutExtent`
updates in the existing §26A–G supports and `edits.rs`/`sketch.rs`.
CI: `runtime-layout.yml`, `tools/check-fbx-complex.sh`. Docs: this file,
`circular-cut-through-all.md`, `cli-json-v1.md`, `cli-capabilities.md`,
`implementation-plan.md`, `circular-cut-history.md`, `README.md`.
No FFI, C ABI, SQLite schema or dependency change.

## Cloud bootstrap (sandbox)

Environment: Linux 6.18.44 x86_64, 4 vCPU (Xeon 2.80 GHz), 15.7 GiB RAM, no swap,
disk 20 GiB free at start. Repository toolchain `rustc 1.96.0`, CMake 3.28.3,
Ninja 1.11.1, GCC 13.3.0, Clang 18.1.3. Missing Ubuntu packages from the
workflow's Linux list were installed: `patchelf libxmu-dev libxi-dev
libgl1-mesa-dev libglu1-mesa-dev`, plus `time`.

Network facts:

- GitHub *archive* URLs (`github.com/.../archive/<commit>.tar.gz`, `codeload`)
  answered 403 through the egress proxy; git over HTTPS worked.
- OCCT was therefore fetched by the pinned commit, which `docs/build-occt.md`
  names as the authoritative pin: `git fetch --depth 1 origin
  b8f597c677811d1f9f4d8a97f5ae2825c0353a42`. `git archive --format=tar
  --prefix=OCCT-b8f597c…/ HEAD | gzip -n | sha256sum` reproduced the pinned
  `OCCT_SHA256` `dba62b81…a35ea` exactly.
- `archives.boost.io` is denied by the organisation's egress policy (403 to
  CONNECT). The pinned Boost 1.91.0 archive cannot be fetched, so **PlaneGCS
  was not built here**. No other Boost or mirror was substituted. Eigen
  (gitlab) and crates.io were reachable.
- `raw.githubusercontent.com` served the pinned ufbx `fcc5d6b…` sources; both
  digests in `fetch_ufbx.sh` matched. `read_production` was built with Clang
  and `-Werror`, as the script prefers. GCC 13 rejects the pre-existing
  reader with `-Werror` (`unused-but-set-variable`); that is not part of this
  diff.

Bootstrap commands (recorded for reuse; all paths are outside the repository):

```sh
git init -q occt && cd occt && git remote add origin https://github.com/Open-Cascade-SAS/OCCT
git fetch --depth 1 origin b8f597c677811d1f9f4d8a97f5ae2825c0353a42 && git checkout -q FETCH_HEAD
cd .. && cmake -S occt -B occt-build -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_LIBRARY_TYPE=Shared -DBUILD_MODULE_Draw=OFF -DUSE_TK=OFF -DUSE_TCL=OFF \
  -DUSE_FREETYPE=OFF -DUSE_VTK=OFF -DBUILD_DOC_Overview=OFF -DCMAKE_INSTALL_PREFIX="$PWD/install"
grep '^BUILD_LIBRARY_TYPE' occt-build/CMakeCache.txt   # Shared
cmake --build occt-build --parallel 3 && cmake --install occt-build
export OpenCASCADE_DIR=$PWD/install/lib/cmake/opencascade LD_LIBRARY_PATH=$PWD/install/lib
export FERRITECAD_REQUIRE_OCCT=1 FERRITECAD_REQUIRE_PLANEGCS=0 CARGO_INCREMENTAL=0 \
  CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
```

Cost, measured separately from development:

- **OCCT build:** 39:54 wall at `--parallel 3`; peak RSS of one process 514 544 KiB; 50 `libTK*.so`. `libTKernel.so` has SONAME `libTKernel.so.8.0` and no RUNPATH.
- **Disk:** `occt-build` 359 MiB, install 125 MiB.
- **Base worktree:** one native `ferritecad-cli` debug build took 39 s after dependencies were cached. The full baseline, two gates included, took 197 s.
- **Cargo parallelism:** early builds used `CARGO_BUILD_JOBS=2`. After the handoff instruction every build ran sequentially at `jobs=1`.
- **Headroom:** no OOM; swap was never needed. The peak target was 14 GiB of debug artefacts; 13 GiB of disk was still free.

## Baseline before the new code (sandbox, native OCCT, no PlaneGCS)

A detached worktree at `6aef501` was built against the same OCCT:

- `sequential::base_height::native_height_transitions_refs_sql_cache_and_mesh` — ok
- `cuts::tests::native_cut_edit_worker_and_cli_publish_the_same_part` (worker + real peer CLI) — ok

## Executed in the sandbox on this diff

Profile: debug with debuginfo off. The build has OCCT but no PlaneGCS; this is
the "mixed" configuration. A genuine stub (no OCCT) was built in a separate
target.

New gates, all executed (none skipped) in the native build:

| Gate | Result |
| --- | --- |
| `sequential::through_all::through_all_discovery_and_requests_without_kernel` | ok |
| `sequential::through_all::an_old_v1_consumer_reads_blind_unchanged_and_through_all_as_unavailable` | ok |
| `sequential::through_all::native_through_all_survives_height_reopen_cold_and_cache` | ok (1/2/4/16 Cuts, 12→14.25→13, cold/Miss/Hit on the same path, early link Hit/Miss×n) |
| `sequential::through_all::native_through_all_transitions_refs_sql_and_refusals` | ok (4 and 16 Cuts; first/middle/last) |
| `sequential::through_all::native_new_consumer_routes_add_edit_height_and_sketch_through_v2` | ok |
| `sequential::through_all::occt_without_solver_keeps_through_all_through` | ok |
| `json::extent_tests::a_cut_extent_is_strict_in_both_kinds` | ok |
| `cut_edit::tests::{through_all_is_stored_as_intent_at_payload_v3_and_never_as_a_depth, mode_transitions_keep_add_or_refuse_names_before_anything_is_minted, the_writer_refuses_a_forged_end_or_a_forged_vocabulary, height_keeps_through_all_through_and_blind_depths_absolute}` | 4 ok |
| `convert::tests::{a_through_all_tool_runs_exactly_the_reach_of_the_body_it_cuts, through_all_without_a_stated_reach_or_on_another_datum_is_refused}` | 2 ok |
| `cuts::tests::native_through_all_widgets_worker_and_cli_keep_intent_draft_and_names` | ok (headless widgets → worker vs real CLI: objects equal, refs equal by full semantics, STL/FBX byte-identical, failed Open restores the exact intent) |
| `cuts::tests::native_through_all_add_widgets_worker_and_cli_publish_one_part` | ok |

**Regression runs (native, no PlaneGCS):**
- **`ferritecad-cli` + `-document` + `-eval` + `-jobs`, whole packages:** 625 passed, 12 failed. Eleven of the failures are native constraint gates, such as `native_circle_radius_and_pinned_centre_drive_the_solid`. With `FERRITECAD_REQUIRE_OCCT=1` and no PlaneGCS they deliberately fail rather than skip, and they failed the same way before this diff. They are **N/A** for a build without the solver and execute in CI with PlaneGCS. The twelfth is `validation_really_read_only_permissions`; see the next item.
- **`validation_really_read_only_permissions`:** the sandbox runs as uid 0, where `chmod` is not evidence, so the test refuses to pass there, as designed. The same compiled binary was run unchanged under uid 1001 with a private `TMPDIR`, and it passed, as did all 4 `validate` tests.
- **`ferritecad-app cuts::tests::`:** 10/10 ok. In addition, `edits::tests::native_cut_base_height_form_worker_cli_preserve_draft_and_new_names` and `sketch::tests::native_cut_base_widgets_worker_cli_preserve_draft` passed.
- **Stub workspace** (earlier default build without OCCT): 1989 passed, 1 failed; the failure is the same uid 0 validate test.

**Genuine stub** (separate target, no `OpenCASCADE_DIR`):
- **What the build said:** `build.rs` printed "Open CASCADE was not usable … OcctKernel::new will refuse". CMake found the sandbox OCCT build tree through the user package registry (`~/.cmake/packages/OpenCASCADE`), but configuring the bridge still failed.
- **What the binary links:** `readelf -d` on the stub `ferritecad` lists only `libc`, `ld-linux`, `libm` and `libgcc_s` as NEEDED; `ldd` shows no `libTK*` and no planegcs.
- **Kernel-free gates (executed):** in `sequential::through_all::*`, the discovery gate and the old-v1-consumer gate.
- **Native gates (skipped):** they print `skipped: this build has no Open CASCADE`. Priority: the CLI constructs the kernel before the job. A v1 edit aimed at a ThroughAll Cut is therefore refused by the kernel in the stub, before preparation; the preparation refusal is asserted only in native builds.

Static checks: `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets --all-features -D warnings` (stub build), `tools/check-licence-headers.sh`
(362 files) and `tools/check-export-boundary.sh` all pass.

## Old reader, measured

A real §26G CLI, built from `6aef501` against the same OCCT, was run on a
ThroughAll document written by this branch:

- `inspect --json` → `document_refusal: "it requires feature.through-all.v1, which this build does not implement"`. The Cut is kept as an unknown object.
- `validate --json` → valid, with the warning `object.unknown-type … feature.extrude at schema v3, which this build preserves but cannot interpret`.
- `rebuild --cold` → exit 2, "preserves but cannot rebuild".
- `edit-extrude` → exit 2, "document cannot be edited: it requires feature.through-all.v1"; no destination was created.
- The source's sha256 was unchanged afterwards.

JSON v1 compatibility was checked the same way. Blind documents with 0, 1, 2
and 3 Cuts were written by the old CLI and inspected by both builds. Removing
only the new `*_v2` keys from this branch's output gives JSON **identical** to
the old output, compared as values and as serialised text. This covers
`existing_cut` (N=1) and `neighboring_tool` (N=2).

## Defects found during verification

1. **Extra fields accepted.** `{"kind":"through_all","depth_mm":4}` was accepted: serde ignores extra fields on a unit variant of an internally tagged enum, even under `deny_unknown_fields`. The kernel-free gate caught it. Fixed in `d10ad56` with `ThroughAll {}` and a unit test.
2. **[P1, independent review] JSON v1 type change.** The v1 tool DTOs had turned `depth_mm` into `null` for ThroughAll. Fixed in `7050b4b`: every v1 block and type is kept, a v1 block is unavailable where it was already nullable, and additive `_v2` blocks carry the end. Covered by the old-DTO consumer gate and the byte-level comparison with main.
3. **Wrong CI gate name.** The CI gate for the extent unit test lacked its `json::` module path, so `--exact` selected nothing. The step failed correctly on `7050b4b` (Linux); fixed in `409a31f`.
4. **Recipe assertion.** The first recipe expected an add refusal that names `_v2` at 16 Cuts, where the limit decides instead. Corrected to check `circular_cut_edit`.
5. **Weak forgery test.** The forged-vocabulary writer test first refused because of a mismatched extent, not the vocabulary. A precise case was added: only the vocabulary is forged on a legitimate ThroughAll → Blind edit.

## Mutations (sandbox, native)

Each mutation was applied by exact string replacement, compiled and run, then
restored from a saved copy. The restore was checked by sha256 against the saved
original, and the positive gates were re-run.

| Mutation | Change | Result |
| --- | --- | --- |
| frozen length | `ExtrudeExtent::blind(reach.distance)` → `blind(12.0)` in `convert.rs` | **caught**: `native_through_all_survives_height_reopen_cold_and_cache` failed at the analytic face count (8 ≠ 7): after the height grew to 14.25 the "ThroughAll" Cut had a floor |
| floor policy | `floor_transition` treats ThroughAll as having a floor | **caught** twice: `mode_transitions_…` failed ("same tool, same names") and the CLI transitions gate got exit 2 where 0 was required |
| writer vocabulary | `rederive_parameters` re-derives with `BlindOrThroughAll` instead of the prepared vocabulary | **equivalent** (survived): the writer compares the whole prepared value, including `edit.vocabulary`, so the precise forgery is still refused. Recorded rather than counted as coverage |

Restored hashes: `convert.rs` `57e38fae…`, `cut_edit.rs` `0710f348…` (before the
vocabulary test was tightened) and `1e8fde14…` (after). Positive gates passed
again after every restore.

## Recipe executed

The `FCAD_26H_AGENT_RECIPE` block was extracted from the Markdown exactly as
the contract says. It was run with the native debug CLI and the pinned
`read_production`:

- Output: `FCAD_26H_COUNT_OK` 1/2/4/16, then `FCAD_26H_RECIPE_OK`, exit 0, 21 s.
- Its two FBX files (`e2-1-intent-grown.fbx` and `e4-1-intent-grown.fbx`) read `checks=6 failures=0`.
- The two CI artefacts `through-0.fbx` and `through-1.fbx` (110 684 and 109 788 bytes) were produced by the gate with `FCAD_CUT_THROUGH_ALL_ARTIFACTS` and read the same way: `checks=6 failures=0`.

## CI (GitHub Actions)

New exact-name gates in `runtime-layout.yml` per OS:

| Where | Gates |
| --- | --- |
| Mixed OCCT/no-solver step | 1 |
| New step | 5 CLI integration + 1 CLI unit + 4 document + 2 eval + 2 app |

That is 15 new gates, making 183 per OS on top of the previous 168. The ufbx
loop reads 2 more FBX files, making 66. The `7050b4b` Linux run executed 13 of
the new gates green before the misnamed unit gate stopped it (defect 3).
Results for the final head, with run links, are reported on PR #51, because this
file cannot contain the CI of the commit that adds it.

## Not executed

- PlaneGCS-linked builds in the sandbox; the 11 solver gates; native macOS and Windows. These run in CI only.
- The windowed viewer, GPU, CUA, the macOS bundle and the loader on a Mac. These are left to independent review.
- `FCAD_ALLOW_LOADER_FAILURE_PROBES` was never enabled.

## GUI scenario for independent review (not run here)

Build a fresh bundle using the README's macOS bundle recipe. Then generate the
fixture with the bundled CLI. UUIDs differ on every run: use the ones the
generator prints.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/circular-cut-through-all-verification.md").read_text()
code = text.split("# FCAD_26H_GUI_FIXTURE\n", 1)[1].split("\n```", 1)[0]
Path("fcad-26h-gui-fixture.py").write_text(code)
EXTRACT
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 fcad-26h-gui-fixture.py "$HOME/Desktop/ferrite-26h-gui"
```

```python
# FCAD_26H_GUI_FIXTURE
import json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
out.mkdir(parents=True, exist_ok=True)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
    assert p.returncode == 0, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout)
(out / "plate.json").write_text(json.dumps({"request_version": 1,
    "points_mm": [[0, 0], [60, 0], [60, 40], [0, 40]], "height_mm": 10}))
src = out / "gui-plate.fcad"
run("create-sketch-extrude", out / "plate.json", "-o", src, "--json")
for i, (center, radius, extent) in enumerate([
        ([15.25, 12.5], 3.125, {"kind": "blind", "depth_mm": 10}),
        ([40.125, 12.375], 4.25, {"kind": "blind", "depth_mm": 4.5}),
        ([30.5, 30.25], 2.625, {"kind": "through_all"})]):
    c = run("inspect", src, "--json")["result"]
    (out / "cut.json").write_text(json.dumps({"request_version": 2, "center_mm": center,
        "radius_mm": radius, "extent": extent}))
    dest = out / f"gui-{i + 1}.fcad"
    run("cut-circular-copy", src, "--body", c["bodies"][0]["cut_edit_v2"]["target"]["body_id"],
        "--expect-version", c["content_version"], "--request", out / "cut.json", "-o", dest, "--json")
    src = dest
final = out / "gui-mixed.fcad"
final.write_bytes(src.read_bytes())
c = run("inspect", final, "--json")["result"]
tools = c["bodies"][0]["cut_edit_v2"]["target"]["tools"]
print("fixture", final)
print("content_version", c["content_version"])
print("body_id", c["bodies"][0]["body_id"])
for name, t in zip(["A blind-through 10", "B pocket 4.5", "C through_all"], tools):
    print(name, "feature", t["feature_id"], "curve", t["tool_curve_id"], t["center_mm"], t["radius_mm"], t["extent"])
base = [f for f in c["features"] if f["base_height_edit_v2"]][0]
print("base_extrude", base["feature_id"])
```

A sandbox run of this generator printed, for illustration only: body
`01a0d4f1-e668-…`, A `01a0d4f1-e6a9-…`, B `01a0d4f1-e702-…`, C
`01a0d4f1-e767-…`, base `01a0d4f1-e668-71d1-…`.

Steps and expected results:

1. **Start the viewer.** Start it under a watchdog before selecting it in CUA, and open `gui-mixed.fcad`. The part is 60 × 40 × 10 mm and has three bores. A and C are open at both faces; B is a 4.5 mm pocket.
2. **Open C.** Choose `Edit cut … — <C>…`.
   - The form shows `End: Through all` selected and an empty, disabled `Depth (mm)`.
   - The saved line reads `… cut through all — a hole through the part`.
   - Typing into the depth box changes nothing.
3. **C → Blind 3.25 mm.** Choose `Blind depth`, enter `3.25`, then `Apply cut`.
   - `Undo` restores Through all exactly; `Redo` returns to Blind 3.25.
   - `Save cut copy…`, then Cancel in the dialog, keeps the draft.
   - Save again to a new name. The published copy opens with a floor at z = 3.25 under C.
4. **A → Through all.** On the source, `Edit cut … — <A>…`: choose `Through all`, then `Apply cut` and `Save cut copy…`. A stays open, and no name is added (Sketch solves/inspector unchanged).
5. **Grow the base.** With the copy from step 4, `Edit extrusion` the base to 14.25 and publish.
   - A and C stay open at z = 14.25.
   - B remains a 4.5 mm pocket.
6. **B → Through all is refused.** Choose `Edit cut … — <B>…` and `Through all`, then `Apply cut`. The refusal names B and its protected floor UUIDs, and nothing is published.
7. **Add a Cut.** `Cut circle into …` with `Through all` at (48, 30) r2 publishes a fourth through hole.
8. **Quit.** Confirm termination by PID/watchdog only. Do not call `getApp` or `getAXState` on the closed viewer: that can relaunch it.

SQL, STL and FBX of GUI copies should match CLI copies produced with the same
request v2, after mapping only newly minted reference UUIDs (steps 3 and 5).

## Independent macOS review — 2026-09-24

The cloud handoff was fetched as `761eec085a8ac292788a2590e5fd6e518e9c23d3`.
All five commits have the owner's author and committer identity and no AI
co-author trailers. The foreign detached worktree was not changed.

Review found a protocol regression in the first implementation: a previously
required JSON v1 floating-point `depth_mm` became nullable. Commit `7050b4b`
restored the old shapes and introduced additive `_v2` discovery. The review
compared Blind-document JSON with an actual §26G executable and ran the old
reader against a ThroughAll document: read-only discovery, refusal to rebuild
or publish an edit, and unchanged source bytes. The public recipe's 16-Cut
refusal assertion was also corrected before this handoff. The corrected recipe
was extracted from Markdown and passed all 1/2/4/16-Cut cases with pinned ufbx.

The remaining review edit clarifies the height form and README: Blind depths
stay fixed, whereas ThroughAll follows the plate thickness. No geometry,
publication, wire or naming policy changed in that edit.

Local validation reused the pinned OCCT/PlaneGCS installation and existing
targets, with sequential builds. The initial native review ran 515 applicable
core tests, two explicit solver-absent-only N/A cases and one old ignored timing
benchmark, 76 selected CLI tests, 10 Cut workers and nine height/edit workers.
After the compatibility change, CLI/app were rebuilt and affected CLI suites,
fmt and workspace all-target/all-feature clippy with `-D warnings` passed.
A true stub was verified by CMake NOTFOUND and zero native imports: 29 wholly
kernel-free tests passed; an additional discovery gate executed structural
checks and explicitly skipped its native subsection. The mixed OCCT/no-solver
ThroughAll exact gate passed separately. These are not interchangeable results.
On the final review source, a fresh release CLI/app build, the strengthened
writer test, strict extent parser test, nine edit workers and app clippy passed.
The official staged macOS bundle passed strict signature and loader checks.

### Real window checks

Only owned viewers and temporary models were used, each launched under the
1536 MiB watchdog before CUA selected the running app. Native Open/Save dialogs,
publication and asynchronous Open were observed. After Quit, termination was
checked through the PID/watchdog, without querying the closed app again.

| Run | Scope and outcome | Peak footprint | Exit / swap |
| --- | --- | ---: | --- |
| Initial `d10ad56`, PID 44403 | Apply, Undo/Redo and Save Cancel passed; CUA coordinate access failed after Cancel, so this was an incomplete smoke | 206.798 MiB | 0 / 0 |
| `7050b4b`, PID 47820 | Blind-through → ThroughAll, publish/Open, height 12 → 14.25, protected-pocket refusal with UUIDs | 199.626 MiB | 0 / 0 |
| `761eec0` plus review wording, PID 62261 | Published generator fixture; saved ThroughAll → Blind 3.25, exact Undo/Redo, Save Cancel and retry, publish/Open, fourth ThroughAll Cut at (48,30) r2, corrected height context | 215.376 MiB | 0 / 0 |

The successful runs together cover the eight-step scenario above; the first
partial run is not counted as complete. No watchdog fired or OOM occurred in
these runs. The historical OOM cause remains unproved.

For the 7050 copies, every SQL cell matched the CLI except `meta.modified_at`;
STL and FBX were byte-identical. An additional CLI shrink to 8 mm, below the
original 12 mm base, kept the explicit through hole open and the other pocket.
For the final ThroughAll → pocket copy, SQL matched after allowing only
`meta.modified_at` and the single newly allocated floor-reference UUID;
all pre-existing references were identical. Both final GUI copies (three and
four Cuts) produced byte-identical STL/FBX to equivalent CLI requests. For the
fourth-Cut creation, new feature/Sketch/curve identities are allocated separately;
full raw SQL equality is not claimed for those new objects. Existing tool
history and the new extent were compared explicitly.

An independent binary STL parser checked the bounding box, consistently closed
oriented mesh, expected cylindrical walls and open-hole/pocket-floor positions,
and mesh volume within the documented deflection bound. The eight GUI/peer FBX
files across the two successful runs passed pinned ufbx, six checks each and
zero failures. Source bytes were preserved during the parity runs.

### Remote evidence and artifacts

The review downloaded logs for exact cloud head `761eec0`, rather than counting
workflow declarations: **183/183 required native gates, no required skips, and
66 ufbx reads with zero failures on each of Linux, macOS and Windows**. Its
11 check runs and two triggered workflows passed (CI `36050817842`, combined
runtime `36050812752`). Earlier commit checks are not substituted for this head.
The new review commit and merge checks are audited separately when published.

Logs, parsed summaries, copied GUI models, parity scripts and the final recipe
are under `/private/tmp/ferrite-26h-review/`, with a durable review copy under
`~/.codex/visualizations/2026/09/05/01a0722f-a4b4-7532-b72b-07ef6b78698d/ferrite-26h-review/`.
`gui-final/parity-final.json` records the exact export hashes and memory result.
No Windows/Linux windowed smoke, new heavy STEP campaign or OCCT rebuild was
performed in this macOS review.
