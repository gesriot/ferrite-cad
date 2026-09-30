# §28G — verification record

[Contract and recipe](sequential-edge-fillets.md).

## Independent macOS review — 2026-09-29

Reviewed `a046469b2fb08acf27fb8ff77eff83a092be01d3` against `931da6d`.
No production defect was found. The public README, CLI capabilities and JSON
contract still said a second Fillet was unavailable; those statements were
corrected and §28G was added to the implementation plan. Production sources,
workflow commands and executable recipes were unchanged during review.

**Local native checks.** Fresh release CLI/viewer, pinned OCCT 8.0.1 and the
existing pinned PlaneGCS delivery on Apple Silicon. Kernel, OCCT, topology,
document, jobs and eval suites reported 818 harness passes: 816 executed and
two explicit no-solver-only N/A; one existing timing benchmark was ignored.
The complete CLI Fillet suite reported 43 passes: 41 executed and two explicit
mixed-build-only N/A. All eight headless Fillet app tests executed. No required
native gate skipped. Formatting and workspace clippy (all targets/features,
`-D warnings`) passed. The heavy STEP campaign was not repeated locally.

**Real window.** A fresh staged arm64 bundle passed strict/deep codesign and
its bundled solver probe without DYLD overrides. One viewer, PID 16712, ran
under the 1536 MiB watchdog. The fixture and comparator below ran against its
bundled CLI. Open showed the first Fillet; the form listed only the other three
corners and the first radius. Radius 0.005 was rejected. Radius 7.12 survived
Save Cancel, then failed after solving because the two arcs would leave only
0.004999999999999893 mm flat on the shared 14.25 mm Line. `never.fcad` was not
created and the draft survived. Radius 5.5 published `gui.fcad` and opened it
asynchronously; the top view showed both rounded corners. Third Fillet and
both saved-radius editors were disabled with the documented reason.

GUI STL/FBX exports matched the CLI copy byte for byte; the comparator reported
`FCAD_28G_GUI_COMPARE_OK cells=167`, including source preservation and SQL/ref
allowlists. Pinned ufbx 0.23.0 reported six checks, zero failures. Independent
oriented STL/FBX comparison matched 172 triangles (worst coordinate difference
6.94e-18 m). Peak physical footprint was **202.704 MiB**, pressure stayed normal,
swap stayed zero and Cmd+Q returned exit 0. No CUA viewer call was made after
Quit. An initial watchdog invocation failed before launching any child because
its stdout log name was already reserved by the invoking shell; a new log stem
was used for the sole viewer process. The earlier OOM remains unexplained.

**Remote evidence read independently.** Ordinary CI on `a046469` succeeded
([run 36647824593](https://github.com/gesriot/ferrite-cad/actions/runs/36647824593)).
The actual logs of the complete runtime rerun on `5860705`
([36640255679](https://github.com/gesriot/ferrite-cad/actions/runs/36640255679))
contained all 12 distinct new exact gate names, 13 executions per OS because
one discovery gate also runs in the mixed build, two recipe markers per OS
and one sequential-FBX reader marker per OS. All three platforms and comparison
passed. Changes after that head were verification prose only; pin run
[36637802226](https://github.com/gesriot/ferrite-cad/actions/runs/36637802226)
also passed on its applicable code inputs. The later review documentation
commit must pass its own ordinary CI before merge; it is not covered by the
older head's status.

Local evidence: `/private/tmp/ferrite-pr71-review`, with retained small logs,
models and screenshots under the review artifact directory. No foreign
worktree or user model was changed.

## Where and how this was run

* **Base.** `origin/main` at `931da6d9a450db34e2a613286465acdedf551463`
  (§28F, merged). The branch `sequential-edge-fillets` was created from it.
  Its merge-triggered workflows are recorded under [CI](#ci), separately from
  this change's runs.
* **Cloud container.** Linux x86_64, the session's existing OCCT 8.0.1 install
  and native/stub targets, one build at a time, `CARGO_BUILD_JOBS` 2–4. OCCT,
  Boost and PlaneGCS were not rebuilt.
* **PlaneGCS: local, unpinned.** The proxy refuses the pinned FreeCAD and Boost
  archives, so the pinned library cannot be built here; no network restriction
  was worked around. Every local solver result below is from the library an
  earlier slice (§27H) built from FreeCAD 1.0.1 `planegcs` sources with
  Ubuntu's Eigen 3.4.0 and Boost 1.83. **The named CI gates on Linux, macOS
  (Apple Silicon) and Windows with the pinned delivery are the authoritative
  solver evidence.**
* **No window, GPU test, browser or large STEP corpus was run**, and
  `FCAD_ALLOW_LOADER_FAILURE_PROBES` was never set. The widget tests drive real
  egui widgets headlessly; that is not a window test. The window scenario
  below is for the user's Mac.

## OCCT facts measured before implementing

A C++ probe linked against the installed OCCT 8.0.1 (`TKFillet`, `TKBool`,
`TKBO`, `TKPrim`, …), on the 37.5 × 12.25 × 6.75 mm plate:

* `BRepFilletAPI_MakeFillet::IsDeleted` answers `true` for edges and vertices
  the result still holds unchanged — 7 of 8 edges and 6 of 8 vertices after
  one vertical edge of a box is rounded — and `false` for every kept face.
  §28A's bridge asked it first; for faces the answer was right, for edges it
  would have reported every surviving sweep edge as gone. The bridge now
  classifies by `Modified`, then by membership of the result, and refuses a
  reported output of another shape type. Every face answer is unchanged
  (§28A–F tests pass unchanged).
* Adjacent second Fillet across the 12.25 mm side: `r1 + r2 = L` (touching)
  is not built at all; flats of 1e-7, 1e-5, 1e-4, 1e-3, 5e-3, 0.01 and 0.025
  mm build a valid 8-face solid whose volume matches
  `(W·D − (1 − π/4)(r1² + r2²))·h` to 2e-16 relative, Fillet 1's face kept.
  Opposite corners were always valid. The pair policy is therefore
  `r2 ≤ L − r1 − 0.01 mm` (the smallest radius this build rounds).

## What changed

* **bridge** (`fc_occt_fillet_edge`): history classification as above.
* **topology:** `CarriedName::SweepEdge(joint)` (an edge) and
  `CarriedName::FilletFace { edge_feature, joint }`; `record_fillet` carries
  the predecessor's sweep edges and fillet face under their origin through the
  fillet's own history (the rounded edge is recorded removed), with a kind
  check on every carried output; `FeatureNames::origin_sweep_edge`. Archive
  names `OriginSweepEdge` / `OriginFilletFace`, codec tags 19/20, **format
  v3 → v4** (a v3 entry is refused and rebuilt). Resolution of the new role.
* **document:** `Fillet::schema_version` — v2 when `edge.feature ≠ previous`;
  `readable_schema_versions` `[2, 1]`; capability
  `feature.fillet.sequential.v1` (supported list, Fillet v2's required set,
  and `SemanticRole::OriginFilletFace`'s). The second-Fillet target from the
  §28B frame (`fillet_over_plate`): three remaining corners, the saved Fillet
  as `ExistingFillet`, `adjacent_fillet`, `max_radius_mm`, `corner_for` that
  names an already-rounded corner; `check_pair` / `pair_bound` / `shared_side`;
  preparation with eight names and the `F2 → F1` / `Body → F2` / `−Body → F1`
  dependencies; `evaluable_fillet` checks the chain (previous is the one other
  Fillet, on the same base, another corner, exactly two Fillets) and both
  corners plus the pair on the built Lines. Validator rule
  `fillet.edge-outside-history`. Editors' refusals of a two-Fillet history
  name it. No SQLite schema change.
* **eval:** the plate's built Lines are the edge producer's; the kernel edge
  is the predecessor's own sweep edge or (§28G) the origin sweep edge Fillet 1
  carried, exactly one, else a typed `topology` refusal; the fillet's tracked
  set includes the predecessor's sweep edges and fillet faces.
* **jobs:** the written Fillet's built corner is asked of `edge.feature`.
* **cli:** additive JSON — `target.previous_feature_id`, `target.fillets[]`,
  candidates' `adjacent_fillet_feature_id` and `shared_line_id`, a pair-aware
  `max_radius_mm` for an unconstrained plate; role `origin_fillet_face`.
* **app:** the form shows the history and the two-Fillet editing limit, lists
  only the other corners and marks the neighbours with their bound.

## New tests

CLI `crates/ferritecad-cli/tests/fillet.rs`, module `sequential`:

* `sequential_discovery_and_protocol_without_native` — discovery on a plate
  rounded without a kernel (three candidates, two marked adjacent, the saved
  Fillet's history), the protocol per build (with a kernel: same corner and
  bad radii `input`; without: `unsupported` before any domain check), a
  two-Fillet document written by the shipped writer: `fillet_edge` refuses
  "third", both radius rows, height, Sketch and constraint editors refuse,
  `validate` valid, the third request `unsupported`.
* `native_second_fillets_on_adjacent_and_opposite_corners_are_what_the_numbers_say`
  — CCW, CW from the upper right, CCW from the third corner; adjacent across
  the 12.25 mm side (r 2.375 → 3.0625), opposite (4.5625), and across the
  37.5 mm side (1.625).
* `native_the_pair_policy_is_exact_at_its_bound` — r1 = 6.125: `D − r1`
  (touching), bound + 1e-9 and 6.12 refused `input` «flat»; the bound
  discovery states (6.115, flat 0.01 mm) published and measured.
* `native_a_dimensioned_plate_is_rounded_twice_at_its_solved_corners` — a
  plate dimensioned to 30.5 × 8 mm at (2.5, −1.75): stored and solved corners
  differ visibly; r2 = 3.995 fits the stored 12.25 mm side and is refused by
  the solved 8 mm one («flat»), nothing written; adjacent 3.25 and opposite
  3.875 published, measured on the solved plate, Sketch row byte-identical.
* `native_second_fillet_refusals_races_cancellation_and_report_loss_are_atomic`
  — source/occupied/hard-link outputs, the same corner, radii 0, 0.009,
  6.125 + 1e-9, 100, a stale version, cancellation at 0 and 0.95, exit 7 with
  the copy kept; no scratch file left.
* `native_a_second_fillet_on_a_restored_first_and_invalidated_suffixes` —
  Fillet 2's radius changed in place: Extrude Hit, Fillet 1 **Hit**, Fillet 2
  Miss, built on the restored Fillet 1 and equal to a cold build; then all
  Hit. Fillet 1's radius: Extrude Hit, both Fillets Miss. The Sketch moved
  1.5 mm: all Miss, both axes moved.
* `native_the_evaluator_refuses_a_saved_second_fillet_outside_the_class` —
  forged Fillet 2: the same corner («already rounded»), touching arcs
  («flat»), Fillet 1 as the edge producer («not an Extrude»).

Every published second Fillet goes through one checker (`second_fillet`):
discovery (history, three candidates, adjacency, bound or `null`), the result
(`previous_feature_id` = Fillet 1, canonical edge named by the base, solved
`corner_mm`, eight roles), source bytes, the stored history (Fillet 2 v2 with
`previous` = Fillet 1 and `edge.feature` = base; Fillet 1 still v1; SQL
`schema_version` 2/1; Body tip), every old name kept and eight new ones by
role, the exact allowlist (`only_a_fillet_was_added(…, 8,
[feature.fillet.sequential.v1])`), the catalogue of the copy (no third Fillet,
every editor refusing), `validate`, cold `rebuild` resolving every name, the
B-Rep cold, through a real Miss (3 features) and Hit: 8 faces, volume
`(W·D − (1 − π/4)(r1² + r2²))·h` within 1e-9 relative (analytic, separate
from the mesh), shapes 3; **Fillet 2's own cylinder** (r2, on the tip, axis
r2 inward of its corner), **Fillet 1's cylinder under Fillet 2's
`origin fillet face`** (r1, on the tip, its own axis — the separate proof that
it survived), Fillet 1's own name still on Fillet 1's result, both caps and
four sides planes under the base; the STL read independently (closed, one
orientation, extents, exactly the two corners without vertices, each wall on
its radius, chord-bounded volume); the FBX complete.

Document: `fillet::tests::a_second_fillet_is_written_on_the_first_and_a_third_is_refused`,
`fillet::tests::the_validator_refuses_an_edge_outside_the_fillets_history`;
the §28A writer test now asserts the second target instead of a refusal.
Topology: `codec::tests::origin_edges_and_fillet_faces_survive_their_byte_form_and_v3_is_refused`
(and v1/v2/v3 all refused in the existing test; tags 19/20 pinned).
App: `fillets::tests::second_fillet_widgets_show_the_history_and_keep_the_draft`
(no kernel) and `fillets::tests::native_second_fillet_worker_and_cli_publish_the_same_part`
(worker and CLI: SQL equal after matching Fillet 2 and its eight names, STL
and FBX byte-identical, async Open, a refused Open restores the draft).

## Local results

Solver build (OCCT 8.0.1 + the unpinned PlaneGCS):

* `tests/fillet.rs`: 43 passed, 0 failed (36 existing + 7 new), debug and
  release. The release run wrote the FBX/STL artifacts below.
* Every new gate executed by its packed argv (`cargo test … "$gate" --
  --exact --nocapture --test-threads=1`, `test $gate ... ok`, no
  `skipped:`): 7 CLI, 2 document, 1 topology, 2 app — 12 of 12. (A first
  attempt with `-q` found no `test … ok` line and failed all twelve, as the
  gate must: `-q` prints dots. CI does not pass `-q`.)
* Pair bound: r1 = 6.125 on the 12.25 mm side; discovery offers
  6.115 (= 12.25 − 6.125 − 0.01 in f64); that radius publishes a valid
  8-face part whose volume is analytic to 1e-9 and whose Fillet 1 cylinder is
  still named; bound + 1e-9, 6.12 and touching (6.125) are refused `input`.
* Dimensioned plate (30.5 × 8 at (2.5, −1.75), stored 37.5 × 12.25): r2 =
  3.995 refused by the solved 8 mm side («… would leave 0.004999999999999893 mm of it flat»), 3.25
  and 3.875 (opposite) published on the solved corners.
* Restored predecessor: Fillet 2 changed in place → Extrude Hit, Fillet 1
  Hit, Fillet 2 Miss, identical to a cold build (8 faces, both axes); Fillet 1
  changed → Hit, Miss, Miss; Sketch moved → Miss ×3.
* Document: 9 `fillet::` unit tests (2 new), app: 8 fillet widget tests (2
  new).
* Recipe: `FCAD_28G_RECIPE_OK first=3092.537750/3092.610453
  second=3078.847572/3079.024522` (measured mesh / exact volume, mm³).
* FBX: `tools/check-fbx-complex.sh --release --features planegcs` exactly as
  CI runs it, with the release run's artifacts: all seven Fillet markers
  including the new `FCAD_FILLET_SEQUENTIAL_UFBX_EXECUTED`; the ten files
  `second-{ccw,cw,third}-{short,opposite}`, `second-ccw-long`,
  `second-bound`, `second-dimensioned{,-opposite}` read by pinned ufbx 0.23.0
  (`checks=6 failures=0` each), 108–172 triangles, STL↔FBX worst 6.94e-18 m.
* fmt, workspace clippy (`--all-targets --all-features -D warnings`) and
  `git diff --check`: clean.

Regression of the affected crates, solver build, debug (`--no-fail-fast`):
`ferritecad-document`, `-jobs`, `-eval`, `-topology`, `-occt`, `-kernel`
(lib and integration tests), every `ferritecad-cli` test target except the
heavy ones listed under [Limits](#limits), the CLI binaries, and
`ferritecad-app`: **1438 passed, 2 failed, 1 ignored**. The two are the
root-only permission tests (`dump_graph`'s
`read_only_permissions_still_dump_when_the_file_can_be_read`, `validate`'s
`validation_really_read_only_permissions`), which refuse privileged chmod as
evidence and fail under this container's root on base too; run as `nobody`
(`runuser -u nobody`, their own test binaries, a temporary HOME/TMPDIR, the
solver library copied there) both pass. The ignored one is the pre-existing
timing test.

OCCT without PlaneGCS (`--no-default-features`,
`FERRITECAD_REQUIRE_PLANEGCS=0`, the CI argv):

* gate `sequential::sequential_discovery_and_protocol_without_native`:
  `test … ok`, no `skipped:`.
* The whole `sequential` module: 6 executed and passed; the dimensioned test
  printed `skipped: constrained geometry requires PlaneGCS` (N/A here, not a
  success; it runs in the solver build and in CI).
* Recipe: `FCAD_28G_RECIPE_OK` with the same volumes — the recipe's plate is
  unconstrained, so no solver is asked.

Stub (no OCCT; CMake configured through an explicit toolchain file with the
package registries off and the native prefixes ignored):

* the five `ci.yml` gates (CLI discovery, app widgets, two document tests,
  codec): `test … ok`, no `skipped:`. The CLI discovery test first failed
  here: a kernel-free build asks for the kernel before any domain check, so a
  well-formed request is `unsupported`, not `input`; the test now expects the
  kind per build (the domain test refuses the same corner without a kernel).
* Recipe: `FCAD_28G_RECIPE_NO_KERNEL` at the first geometry step.
* The rest of `tests/fillet.rs` was not rerun in the stub build.

### Mutations — local, executed, restored byte for byte

Each is a compiled change run against the new tests, then the file restored
from a copy and checked by `sha256sum -c`.

* **M1 — Fillet 1's face not carried** (`record_fillet` skips
  `named_fillet_edges`). Caught by 7 of 7 CLI `sequential` tests and the
  native app test: every publication is refused by the writer's own check,
  `topology` «… names the face the named fillet made of feature …», before
  anything is written; the kernel-free widget test passes, as it must.
* **M2 — the wrong edge on a cached predecessor** (the archive binds each
  carried sweep edge to its neighbour's handle; cold builds unaffected).
  Caught only by `native_a_second_fillet_on_a_restored_first_…`: Fillet 2
  built on the restored Fillet 1 rounds another corner, and its axis differs
  from the cold build's. The other six pass — which is why the restored-cache
  test exists.
* **M3 — §28A's history classification restored in the bridge**
  (`IsDeleted` first). The adjacent/opposite test is refused `topology`
  «… already removed it»; §28A's `native_two_corners_both_windings_…` still
  passes, confirming faces are unaffected.

A CLI binary built during M3 was stale when the first old-reader attempt ran
and refused the second Fillet the same way; it was rebuilt from the restored
tree before any result below was taken.

### Compatibility with the reader on `main`

`931da6d`, extracted with `git archive` and built (release, PlaneGCS) in the
scratchpad, against copies this build wrote:

* **Two Fillets** (`two.fcad`): `validate`, `rebuild --cold`, `export-stl` and
  `export-fbx` refuse `unsupported: topology reference … requires unsupported
  capabilities: feature.fillet.sequential.v1` before building anything;
  `inspect` succeeds, lists only Fillet 1 (Fillet 2 is kept verbatim as an
  object it does not read) with its radius edit unavailable and
  `fillet_edge.available: false`; `edit-fillet-radius` (both), `edit-extrude`
  and `fillet-edge-copy` refuse `document cannot be edited: it requires
  feature.fillet.sequential.v1`. The file's SHA-256 is unchanged and no output
  exists.
* **One Fillet** written by this build: `validate` valid, cold `rebuild`
  resolves every name; its STL **and** FBX are byte-identical to this build's.
  Its `fillet-edge-copy` of a second corner refuses with §28F's sentence.
* The cache sidecar has no CLI path in either build; the v3 → v4 refusal is
  shown by the codec test, and the cached-rebuild tests run the v4 path.

## CI

* **Base `931da6d`, merge-triggered, checked separately** (workflow runs read
  through the API after they finished): CI
  ([36621937335](https://github.com/gesriot/ferrite-cad/actions/runs/36621937335)),
  combined runtime layout
  ([36621937666](https://github.com/gesriot/ferrite-cad/actions/runs/36621937666)),
  planegcs pin
  ([36621937658](https://github.com/gesriot/ferrite-cad/actions/runs/36621937658)),
  rust notices, product sbom and rust sbom: all concluded **success**.
* **First round, head `e017d82`:** the combined runtime layout's Linux job
  failed in the §28A public recipe (`docs/single-edge-fillet.md`), which
  asserted that a rounded copy refuses a second Fillet by name — exactly what
  §28G changes. The recipe now asserts the second target (previous is the
  saved Fillet, its corner not offered) and that the same corner is refused
  `input` «already rounded». Locally before pushing: all seven Fillet recipes
  §28A–§28G print `FCAD_28x_RECIPE_OK` against the release CLI, and in the
  stub build `tests/fillet.rs` (43), the app Fillet tests (14) and the
  document Fillet tests (22) pass, the §28A recipe printing
  `FCAD_28A_RECIPE_NO_KERNEL`. This had not been run locally in the first
  round: only the §28F and §28G recipes had.
* **Head `5860705`:** CI
  ([36638648061](https://github.com/gesriot/ferrite-cad/actions/runs/36638648061):
  lint, test on Ubuntu, macOS and Windows — with the stub step's five §28G
  gates and `FCAD_28G_RECIPE_NO_KERNEL` — sbom, notices, supply-chain) and the
  combined runtime layout
  ([36640255679](https://github.com/gesriot/ferrite-cad/actions/runs/36640255679):
  Linux, macOS on Apple Silicon, Windows and the three-platform comparison,
  including the OCCT-without-solver step, the Fillet step with the new native
  gates and `FCAD_28G_RECIPE_OK` against the pinned PlaneGCS, and the FBX
  campaign with `FCAD_FILLET_SEQUENTIAL_UFBX_EXECUTED`) concluded
  **success**. The runtime layout's push trigger does not list `docs/**`,
  although it executes the recipes kept there, so it was started on this
  head by `workflow_dispatch`. The planegcs pin
  ([36637802226](https://github.com/gesriot/ferrite-cad/actions/runs/36637802226))
  passed on `e017d82`; `5860705` changes none of its inputs. Step
  conclusions are what is claimed here; the gate counts were not read from
  the job logs.
* This record's own commit changes no input of the runtime layout or the pin;
  CI runs on it.

## Limits

* The class: exactly two Fillets on different vertical corners of one XY
  axis-aligned rectangular Blind/NewBody plate, unconstrained or carrying the
  managed Line family. Not supported: a third Fillet, the same corner twice,
  touching or merged arcs (a flat under 0.01 mm), cap edges, edge chains, Cut
  with Fillet, Chamfer, editing a two-Fillet history, picking, live preview,
  in-place Save.
* Under §28A's per-corner bound (`r ≤ ½` the shorter side) two arcs on one
  Line can at most touch, never overlap; the pair policy therefore decides
  only touching and near-touching. It is stated as a bound on the second
  radius, the expression discovery reports, so the offered maximum is
  accepted (an earlier form, `L − r1 − r2 ≥ 0.01`, refused it by 2e-16).
* Discovery cannot bound a constrained plate; the copy's rebuild does, before
  publication.
* Local solver evidence is from an unpinned PlaneGCS.
* A mesh check is chord-bounded; exact claims are the B-Rep's.
* Not run here: any window, GPU, browser or STEP-corpus test, and the heavy
  CLI targets `complex_step_pixels`, `export_scene_complex`,
  `occurrence_identity_complex`, `imported_step_pixels`,
  `fillet_shell_corpus`, `export_fbx_identity`, `shared_step_import`,
  `import_step`, `export_fbx_complex` (CI runs them).

## macOS fixtures and window scenario (for the user's Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised here with a
second CLI invocation standing in for the window, which says nothing about the
window: `FCAD_28G_GUI_COMPARE_OK cells=167`; a tampered `objects.name` of
Fillet 2 in `gui.fcad` was caught (`AssertionError: objects`); a missing
`gui.stl` was refused without anything being created.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon (arm64) Mac, and do not set `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `rounded.fcad` — the plate drawn clockwise from (33, 15.5),
37.5 × 12.25 × 6.75 mm, dimensioned by the shipped command to 41 × 14.25 mm
with Segment 1's start fixed at (36.5, 17.75), and rounded once by
`fillet-edge-copy` at the stored corner (33, 3.25) (solved (36.5, 3.5)) with
r 7.125 mm, half the solved side — and `facts.json`:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/sequential-edge-fillets-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28G_GUI_FIXTURE\n", "ferrite-28g-fixture.py"),
                   ("# FCAD_28G_GUI_COMPARE\n", "ferrite-28g-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28g-fixture.py "$FCAD_28G_DIR"
```

```python
# FCAD_28G_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-second-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# Clockwise from the upper right: Segment 1 runs -Y, 2 runs -X, 3 runs +Y,
# 4 runs +X. Dimensioned by the shipped command to 41 x 14.25 mm with
# Segment 1's start fixed at (36.5, 17.75): the stored corners (33, 3.25)
# and (33, 15.5) solve to (36.5, 3.5) and (36.5, 17.75), and share the
# solved 14.25 mm Line.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
FIRST, SECOND, H = [33.0, 3.25], [33.0, 15.5], 6.75
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H, "points_mm": PLATE}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
catalog = run("inspect", plate, "--json")["result"]
(sketch,) = catalog["sketches"]
lines = [c["curve_id"] for c in sketch["constraint_edit"]["curves"]]
add = [{"curve_id": lines[0], "rule": "vertical"}, {"curve_id": lines[1], "rule": "horizontal"},
       {"curve_id": lines[2], "rule": "vertical"}, {"curve_id": lines[3], "rule": "horizontal"},
       {"curve_id": lines[0], "rule": "fixed", "at": "start", "x_mm": 36.5, "y_mm": 17.75},
       {"curve_id": lines[1], "rule": "distance", "distance_mm": 41.0},
       {"curve_id": lines[0], "rule": "distance", "distance_mm": 14.25}]
request.write_text(json.dumps({"request_version": 1, "remove": [], "add": add}))
dimensioned = out / "dimensioned.fcad"
run("edit-sketch-constraints-copy", plate, "--sketch", sketch["sketch_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", dimensioned, "--json")
# The first Fillet, by the shipped command: r 7.125 mm, half the solved
# 14.25 mm side, the largest the first corner takes.
catalog = run("inspect", dimensioned, "--json")["result"]
first = next(c for c in catalog["bodies"][0]["fillet_edge"]["target"]["candidates"]
             if c["stored_corner_mm"] == FIRST)
request.write_text(json.dumps({"request_version": 1, "edge": first["edge"], "radius_mm": 7.125}))
source = out / "rounded.fcad"
run("fillet-edge-copy", dimensioned, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", source, "--json")
for p in (request, plate, dimensioned):
    p.unlink()
catalog = run("inspect", source, "--json")["result"]
body = catalog["bodies"][0]
target = body["fillet_edge"]["target"]
assert body["fillet_edge"]["available"] is True and len(target["candidates"]) == 3, body
(saved,) = target["fillets"]
chosen = next(c for c in target["candidates"] if c["stored_corner_mm"] == SECOND)
assert chosen["adjacent_fillet_feature_id"] == saved["feature_id"], chosen
assert chosen["max_radius_mm"] is None, "a dimensioned plate defers the bound"
(out / "facts.json").write_text(json.dumps({
    "body_id": body["body_id"], "body_name": body.get("name"),
    "content_version": catalog["content_version"], "first_fillet_id": saved["feature_id"],
    "first_edge": saved["edge"], "first_radius_mm": 7.125, "edge": chosen["edge"],
    "stored_corner_mm": SECOND, "refused_radius_mm": 7.12, "radius_mm": 5.5, "height_mm": H,
    "gui_rect_mm": [-4.5, 3.5, 41.0, 14.25],
    "gui_corners_mm": [[36.5, 3.5, 7.125], [36.5, 17.75, 5.5]],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28G_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_28G_DIR/../watch-28g.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

1. **Open** `$FCAD_28G_DIR/rounded.fcad` (asynchronously). **Fillet edge of
   <Body> — <UUID>…** is enabled (before §28G it was refused for a filleted
   plate); **Edit Fillet radius** is enabled too (one Fillet).
2. **Form.** Press **Fillet edge of …**. It shows "History: Extrude <UUID> →
   Fillet <UUID> (Lines … | …, r7.125 mm) → new Fillet. … Editing a part with
   two Fillets is not supported yet.", the constrained-plate notice, and
   **three** corners (stored (33, 15.5), (−4.5, 3.25), (−4.5, 15.5)); the
   rounded (33, 3.25) is not listed. Two carry "shares Line <UUID> with Fillet
   <UUID> (r7.125 mm), and the solved plate decides the room left".
3. **Select the second corner.** Choose "Lines … — stored corner (33, 15.5)".
   Radius `0.005`, **Apply fillet**: refused "… at least 0.01 mm …"; nothing
   applied; the selection stays.
4. **Save Cancel.** Radius `7.12`, **Apply fillet** (accepted: the stored
   drawing does not decide it). **Save fillet copy…** → **Cancel**: nothing
   starts; the draft stays.
5. **Refusal keeps the draft.** **Save fillet copy…** →
   `$FCAD_28G_DIR/never.fcad`. The job is refused; the form comes back with
   its draft; the status line reads "Could not save edited model: invalid
   input: as the plate is built, fillets of 7.125 mm and 7.12 mm at the two
   ends of Line … (14.25 mm) would leave 0.00499… mm of it flat; …".
   `never.fcad` does not exist.
6. **Publish, async Open.** Radius `5.5`, **Apply fillet**, **Save fillet
   copy…** → `$FCAD_28G_DIR/gui.fcad`. The viewer opens the copy
   asynchronously: a 41 × 14.25 mm plate whose corners (36.5, 3.5) and
   (36.5, 17.75) are rounded, nothing else. **Fillet edge of …** is now
   refused ("third"), and both **Edit Fillet radius** rows are refused
   ("two Fillets").
7. **Exports.** From the window export `gui.stl` and `gui.fbx` of `gui.fcad`
   into `$FCAD_28G_DIR`, at the default tessellation.
8. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28g-compare.py "$FCAD_28G_DIR"
```

The comparator never creates or overwrites a `gui*` file; a missing one, or an
existing `never.fcad`, is a failure. It checks `gui.fcad` against its source
by §28G's allowlist (Fillet 1's and the Sketch's rows byte for byte, eight
names, only `feature.fillet.sequential.v1` added), makes the peer copy with
`fillet-edge-copy` from the same request and exports it, compares every table
of `gui.fcad` with the peer's once Fillet 2 and its eight name UUIDs are
matched (only the stamp and the hashes over minted UUIDs excepted, their
payloads compared), requires byte-identical STL and FBX, and reads from the
window's STL the solved extents, exactly the two solved corners rounded and
each wall on its own radius; it prints `FCAD_28G_GUI_COMPARE_OK`.

```python
# FCAD_28G_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
source = out / "rounded.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
for name in ("gui.fcad", "gui.stl", "gui.fbx"):
    assert (out / name).exists(), f"produce {name} from the window first"
assert not (out / "never.fcad").exists(), "the refused Save published something"

def run(*args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == code, (args, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def tables(path):
    db = sqlite3.connect(f"{path.as_uri()}?mode=ro", uri=True)
    result = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        result[t] = ([d[0] for d in cur.description], cur.fetchall())
    db.close()
    return result

def second_allowlist(src, copy, body):
    """§28G's allowlist: the Body row's payload/hash, one new object, +2/-1
    edges, eight names, only feature.fillet.sequential.v1 added, the stamp.
    The Sketch row, Fillet 1's row and every older name are the same bytes."""
    bid = bytes.fromhex(body.replace("-", ""))
    a, b = tables(src), tables(copy)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        if t == "objects":
            k = ac.index("id")
            mine = {r[k]: r for r in brows}
            assert len(brows) == len(arows) + 1, "one new object"
            for row in arows:
                for c, u, v in zip(ac, row, mine[row[k]]):
                    assert u == v or (row[k] == bid and c in ("payload", "payload_hash")), f"objects.{c}"
        elif t in ("deps", "topology_refs"):
            lost = [r for r in arows if r not in brows]
            assert all(t == "deps" and bid in r for r in lost), (t, lost)
            assert len(brows) - (len(arows) - len(lost)) == (2 if t == "deps" else 8), t
        elif t == "capabilities":
            assert set(arows) <= set(brows)
            assert {r[0] for r in set(brows) - set(arows)} == {"feature.fillet.sequential.v1"}
        elif t == "meta":
            for x, y in zip(arows, brows):
                assert all(u == v or c == "modified_at" for c, u, v in zip(ac, x, y)), "meta"
        else:
            assert sorted(arows, key=repr) == sorted(brows, key=repr), t

def second_of(path):
    fillets = run("inspect", path, "--json")["result"]["fillets"]
    assert len(fillets) == 2, fillets
    (f,) = [f for f in fillets if f["feature_id"] != facts["first_fillet_id"]]
    return f

def same_publication(gui, peer):
    """Every SQL cell equal once the second Fillet's UUID and its eight names,
    the identifiers this operation minted, are matched; the stamp and the
    hashes over minted UUIDs (compared through their payloads) excepted."""
    g, p = second_of(gui)["feature_id"], second_of(peer)["feature_id"]
    gb, pb = bytes.fromhex(g.replace("-", "")), bytes.fromhex(p.replace("-", ""))
    pairs = [(pb, gb)]
    def mapped(v):
        if isinstance(v, bytes):
            for x, y in pairs:
                v = v.replace(x, y)
        return v
    left, right = tables(gui), tables(peer)
    (rc, rrows), (lc, lrows) = right["topology_refs"], left["topology_refs"]
    i, o, pl = rc.index("id"), rc.index("owner_id"), rc.index("payload")
    mine = {(r[o], r[pl]): r[i] for r in lrows if r[o] == gb}
    for r in rrows:
        if r[o] == pb:
            pairs.append((r[i], mine[(gb, mapped(r[pl]))]))
    assert len(pairs) == 9, pairs
    cells = 0
    for t in left:
        (cols, lrows), (_, rrows) = left[t], right[t]
        keep = [k for k, c in enumerate(cols)
                if not (t == "meta" and c == "modified_at") and c != "payload_hash"]
        norm = lambda rows, f: sorted((tuple(f(r[k]) for k in keep) for r in rows), key=repr)
        assert norm(lrows, lambda v: v) == norm(rrows, mapped), t
        cells += len(keep) * len(lrows)
    return cells

def stl(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    return [struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k)
            for i in range(count) for k in range(3)]

# The window's copy against its source.
second_allowlist(source, out / "gui.fcad", facts["body_id"])
f = second_of(out / "gui.fcad")
assert f["edge"] == facts["edge"] and f["radius_mm"] == facts["radius_mm"], f
assert f["previous_feature_id"] == facts["first_fillet_id"], f
assert f["radius_edit"]["available"] is False, "editing two Fillets is not offered"
after = run("inspect", out / "gui.fcad", "--json")["result"]
assert after["bodies"][0]["fillet_edge"]["available"] is False, "no third Fillet"
# The same request through the shipped CLI, and its exports.
peer = out / "peer.fcad"
if not peer.exists():
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "edge": {
        "feature_id": facts["edge"]["feature_id"], "joint": facts["edge"]["joint"][::-1]},
        "radius_mm": facts["radius_mm"]}))
    done = run("fillet-edge-copy", source, "--body", facts["body_id"], "--expect-version",
               facts["content_version"], "--request", request, "-o", peer, "--json")["result"]
    assert done["stored_corner_mm"] == facts["stored_corner_mm"], done
    assert all(abs(u - v) < 1e-9 for u, v in zip(done["corner_mm"], facts["gui_corners_mm"][1])), done
for fmt in ("stl", "fbx"):
    if not (out / f"peer.{fmt}").exists():
        run(f"export-{fmt}", peer, "-o", out / f"peer.{fmt}", "--json")
cells = same_publication(out / "gui.fcad", peer)
for fmt in ("stl", "fbx"):
    assert (out / f"gui.{fmt}").read_bytes() == (out / f"peer.{fmt}").read_bytes(), fmt
# The window's mesh is the solved plate with exactly the two solved corners
# rounded, each wall on its own radius about its own axis.
x0, y0, w, d = facts["gui_rect_mm"]
pts = stl(out / "gui.stl")
xs, ys, zs = ([p[k] for p in pts] for k in range(3))
for got, want in ((min(xs), x0), (max(xs), x0 + w), (min(ys), y0), (max(ys), y0 + d),
                  (min(zs), 0.0), (max(zs), facts["height_mm"])):
    assert abs(got - want) < 1e-4, (got, want)
rounded = [c[:2] for c in facts["gui_corners_mm"]]
for c in ([x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]):
    near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 for p in pts)
    assert near == (c not in rounded), c
for cx, cy, r in facts["gui_corners_mm"]:
    ax = cx + (r if abs(cx - x0) < 1e-9 else -r)
    ay = cy + (r if abs(cy - y0) < 1e-9 else -r)
    wall = [p for p in pts if abs(p[0] - cx) < r - 1e-4 and abs(p[1] - cy) < r - 1e-4]
    assert len(wall) >= 4, (cx, cy)
    assert all(abs(math.hypot(p[0] - ax, p[1] - ay) - r) < 1e-3 for p in wall), (cx, cy)
print("FCAD_28G_GUI_COMPARE_OK", f"cells={cells}")
```
