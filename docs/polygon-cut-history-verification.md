# §26I verification — circular Cut history on a simple Line polygon

[Contract and recipe](polygon-cut-history.md).

Executed in a Linux x86_64 cloud container (Linux 6.18, 16 GiB RAM) on branch
`polygon-cut-history` from base `9d224d7` (tree `d8f08376`), with the pinned
OCCT 8.0.1 native install, `CARGO_BUILD_JOBS=1`, sequential builds. All base
workflows on `9d224d7` were green before implementation started. PlaneGCS
cannot be built here (its Boost archive host is denied by network policy), so
every local native run is OCCT-with-no-solver; the solver-backed matrix is
covered only by the three-OS CI.

## Domain

* `cut_boundary::tests` (5): the L is its Lines, not its box — (40, 30) is in
  the bounding box but outside the part; a disk centred (17, 17) clears the
  reflex vertex at r 4.2 although both concave edges' *supporting lines* are
  3 mm away (finite segments, not lines), and is refused at r 4.3; the disk
  across the concave edge is refused; clearance exactly 0 is refused, 1e-3 is
  accepted; winding and translation (up to 9.5e5 mm) change no answer for a
  triangle, a quadrilateral and the L; a sloped hypotenuse is measured to the
  segment and names its curve UUID; a rectangle is still exactly a rectangle;
  a bow tie, an open chain, two Lines and circles are refused by the shared
  `PolygonExtrusion` validator.
* `ferritecad-document` lib: 89 passed, 1 ignored (pre-existing), including the
  updated class test (sloped quad accepted, bow tie refused).

## CLI and JSON (native, OCCT, no solver)

`circular_cut` test binary: 42/42 passed (all §26A–§26H history tests on the
generalized measurement helpers, plus four new `sequential::polygon` tests):

* `polygon_discovery_refusals_and_writer_checks_without_kernel` — L with 1, 2
  and 16 Cuts: `_v3` boundary equals the saved Lines by UUID and order, area
  1600, `counter_clockwise`, `bounds_mm` only; every link editable; base
  height and `cut_history_v3` present; an old rectangle DTO (required
  `extents_mm`) deserialises and finds **no** rectangle, every add/edit
  refusal names `_v3`; every Cut names all six sides by their saved UUIDs;
  `validate` passes; source bytes unchanged. Draft and preparation give the
  same refusal for notch, reflex vertex, concave edge, clearance 0 at one and
  at two walls, and outside. A base edit moving the far right wall to x 56
  refuses naming exactly link 11 (slot 9) and no other Cut; x 57 passes; a
  moved concave wall, reordered UUIDs and a reversed winding are refused;
  height 13 is accepted. Writer transactions: a base edit prepared before a far
  tool moved is refused naming that Cut with the version unchanged; an add
  prepared before the top wall moved is refused, version unchanged.
* `a_rectangle_keeps_every_older_block_and_gains_v3` — rectangle: 10 old
  blocks carry `extents_mm` and no `_v3` refusal; `_v3` equals `_v2` apart from
  `bounds_mm`/`boundary`; a sloped quadrilateral's old blocks describe nothing.
* `native_l_profile_history_matrix` — L made by `create-sketch-extrude`, mesh
  volume 16000 mm³; 1/2/16 Cuts via request v2 (ThroughAll, Blind-through,
  pockets); kernel volume = area·h − Σπr²·reach within 1e-6 mm³, cold = Miss
  = Hit; every saved name resolves to one face, sides checked on their finite
  segment with outward normal from the saved Line UUID; independent STL parse
  (closed, oriented, bores inward, start 0, reach, caps only over the real
  outline, volume in the inscribed bound); notch/reflex/concave refusals exit
  2 with no output; first/middle/last edits pass the SQL allowlist and
  measure; edit into the notch refused; base height 13 (Miss/Hit equal);
  coordinate edit to a wider L with a moved notch measured; far-wall refusal
  names link 11; source bytes unchanged.
* `native_sloped_polygons_both_windings_translated` — triangle and
  quadrilateral, both windings, shifted by (1000.25, −250.5): ThroughAll +
  pocket measured and meshed, `_v3` orientation correct, a disk inside the
  bounding box over a sloped wall refused.

Other updated tests (intentional behaviour changes, each asserted):
`edit_cut_base_sketch` case 3 now uses a self-intersecting base (a sloped base
is supported); its "tools did not move" check compares tools and asserts the
edited boundary; `edit_sketch` compares Body blocks except `cut_edit_v3`, which
now reports the edited outline (area 2000).

Full `ferritecad-cli` run: every failure is either the solver matrix
(`annular_constraints` 3, `circle_constraints` 2, `edit_constraints` 6; they
refuse when `FERRITECAD_REQUIRE_OCCT=1` and no solver exists) or
`validation_really_read_only_permissions` as root. That test passes as uid
1001 (`fcad`, TMPDIR outside root's home), unweakened.

## Old consumers

* 9d224d7 CLI vs this branch, `inspect --json` on rectangle documents with 0,
  1, 2 and 3 Cuts (Blind pocket, Blind-through, ThroughAll): after deleting
  only the `*_v3` keys the documents are identical including key order.
* On an L history the 9d224d7 reader reports `cut_edit.target: null` with its
  old rectangle refusal; this branch reports `null` with a refusal naming
  `cut_edit_v3`.

## UI

`cuts::tests` 11/11, including `native_polygon_widgets_worker_and_cli_publish_one_part`:
the form paints "outline of 6 Lines, 1600 mm²" and never "60 × 40 mm";
notch/reflex/concave Apply refused with the document's message and no history
step; ThroughAll at (10.125, 18.625) across y = 20 (inside the part) applied;
Undo/Redo; the widget request through the app worker and the same request
through the CLI give byte-identical STL and FBX and the same stored objects,
schema versions, reference roles and dependency count. Full app bin tests:
321 passed; the 7 failures are the solver-backed constraint worker tests.
The GUI was not launched; the macOS scenario is in the contract.

## Mutations

Applied one at a time, both killers run, bytes restored (SHA-256 of the file
before apply equals after restore), positives rerun green:

| Mutation | Killed by |
| --- | --- |
| bbox-only: `validate` uses `bounds_mm` instead of the boundary | kernel-less test ("inside the bounding box, in the notch" accepted) and native matrix |
| writer-skip: sketch-coordinate writer skips its in-transaction re-derivation | kernel-less test ("stale against a far tool" written) |
| far-tool-skip: `validate_base` checks only the first tool | kernel-less test and native matrix (far-wall edit published) |

## Stub, artefacts, recipe, checks

* Genuine stub (`FERRITECAD_REQUIRE_*`, `OpenCASCADE_DIR`, `LD_LIBRARY_PATH`
  unset; separate target): the two kernel-less polygon tests pass; the two
  native ones print `skipped:` (never counted by the CI gates); `ldd` of the
  stub CLI shows no OCCT/PlaneGCS library.
* Pinned ufbx reader: `polygon-0.fbx` (L, 2 Cuts, height 13) and
  `polygon-1.fbx` (after the coordinate edit) → `checks=6 failures=0` each;
  added to `tools/check-fbx-complex.sh` as `FCAD_POLYGON_CUT_UFBX_EXECUTED`.
* `FCAD_26I_AGENT_RECIPE`, extracted from the Markdown exactly as documented,
  run against the branch CLI with the pinned reader: `FCAD_26I_COUNT_OK` 1, 2,
  16 and `FCAD_26I_RECIPE_OK`.
* `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`
  (OCCT, no solver), licence headers, export boundary, solver and notice
  ownership: clean.
* CI: one new step in `runtime-layout.yml` with exact-name gates (4 CLI, 5
  domain, 1 UI) and one OCCT-without-solver gate; no gate removed, no new
  workflow.

## Not applicable here

* PlaneGCS-backed runs and the solver-backed constraint matrix (no solver
  buildable in this container) — three-OS CI only.
* GUI, macOS and Windows execution — CI and the manual macOS scenario.
* No FFI, schema, capability or cache-protocol change was made, so none was
  verified.

## Independent macOS review — 2026-09-25

Reviewed implementation head `f0a78b13aefc9a17cb8243abcae9ca8b3bd45179`
against `9d224d7eecf439b76db3c8c25d644bc1de04c94b`. Both implementation commits
have the repository owner's author/committer identity and no AI coauthor
trailers. No blocking product defect was found. The contract's explanation of
infinite supporting lines was corrected: they can over-refuse a disk near a
reflex vertex; finite segments measure the actual wall. Product sources,
fixtures, dependencies and workflow definitions are unchanged by this review.

### Local execution

Fresh release CLI/viewer, existing pinned OCCT 8.0.1 and PlaneGCS, sequential
builds with one job; existing native/stub targets reused. The native selection
ran **652 tests**: document/topology/eval/jobs 520, CLI 82, app Cut 11, edit
worker 9 and Sketch 30. Two additional harness passes are explicitly N/A tests
for a build without the solver; one old timing benchmark remained ignored.
No geometry gate was credited through a skip. `fmt`, workspace release
`clippy --all-targets --all-features -- -D warnings`, licence headers,
export boundary and whitespace checks passed.

The genuine stub has `OpenCASCADE_DIR-NOTFOUND` in CMakeCache and no libTK or
PlaneGCS imports in `otool -L`: 35 kernel-free tests executed (polygon discovery,
old consumer, 28 Cut document tests, 5 boundary tests). The mixed OCCT/no-solver
build executed the exact sloped/winding/translation gate without a skip; its
imports contain OCCT and no PlaneGCS. The full native binaries were rebuilt
afterward. No native dependency was rebuilt from source.

The Markdown recipe was extracted and executed with the staged CLI and pinned
ufbx: counts 1, 2 and 16, sloped profiles and `FCAD_26I_RECIPE_OK` passed.
The actual previous bundled CLI and current CLI were also compared on the
same rectangular documents with 0/1/2/3 Cuts, including ThroughAll. Removing
only the four additive `_v3` blocks leaves identical JSON values and field
order. The old CLI honestly refuses polygon Cut discovery; current `_v3`
reports its real boundary. These reads leave the inputs unchanged.

### Actual window and persisted results

A fresh, strictly verified ad-hoc signed `FerriteCAD.app` was staged from the
reviewed release binaries. Bundled CLI help and viewer solver-info ran with
loader environment variables unset before the window test. One owned viewer
ran under `watch-viewer-memory.py`, cap 1536 MiB; no other viewer was launched.
The GUI used only private temporary models, starting with the documented
six-Line L-profile (area 1600 mm², height 10 mm).

Observed in the actual window:

* The Cut form states six Lines and 1600 mm². Centres/radii `(40,30)/3`,
  `(17,17)/4.3`, `(40,17)/4` refuse respectively in the notch, across the reflex
  vertex and across the concave wall. The saved scene stays accepted.
* ThroughAll `(10.125,18.625)/2.25` → Apply, then `(45,10)/5` → Apply;
  Undo/Redo/Undo restores the exact requests. Save Cancel preserves the first
  draft. Saving `gui-cut.fcad` publishes and asynchronously opens it.
* Moving the saved Cut into the notch refuses. Moving it to `(10.5,18.5)`
  publishes/opens `gui-moved.fcad` with the same Cut identities.
* Moving the two inner-wall vertices from x20 to x12 refuses and names the
  affected Cut UUID; x21 publishes/opens `gui-wider.fcad`. The tool stays at
  its absolute coordinates. Base height 13 publishes/opens `gui-tall.fcad`.
* The final model exports through the actual STL and FBX UI actions. The
  window and title show the accepted file at each step.

All four GUI results were cold rebuilt and compared with independently
requested CLI copies: STL/FBX bytes match in every pair. The three edits have
identical SQL except `meta.modified_at`; separate source-to-copy allowlists
confirm only the intended object payload/hash changes. The independent add
allocates new Cut UUIDs, so its whole database is not called byte-identical.
The eight FBX files pass pinned ufbx (`6 checks, 0 failures` each).

The independent binary STL parser checks directed-edge closure/orientation,
caps over the real L rather than its notch, and a bore reaching both z0 and
z13 after the height change. Final STL: 216 triangles, 10884 bytes, measured
volume 20853.833899 mm³ versus analytic 20853.243933 mm³, within the explicitly
derived tessellation band (0.01 mm linear, 0.5 rad angular deflection). The
comparison does not label the mesh volume as an exact B-Rep measurement.

Viewer PID 8403: peak physical footprint **207.423 MiB**, pressure normal,
swap 0 throughout, exit 0, no watchdog abort. After Quit only the process/log
was checked; no CUA query relaunched the viewer. One clipboard-paste timeout
occurred before input and was recovered using normal text entry. The previous
OOM cause remains unknown. Windows/Linux windowed GUI was not exercised.

### Remote evidence and reproducibility

Implementation head CI: **15/15 checks, 3/3 workflows success**. Independent
job-log audit found all **193 distinct required test names** on each OS,
including both native and mixed executions of the new sloped-profile test,
and **68 pinned ufbx reads per OS**, no reader failures. Repeated executions
of existing tests are not counted as additional distinct gates. This is
evidence for the implementation SHA above, not a claim that a later review or
merge SHA has already run. Publication records carry their own check state.

* [CI](https://github.com/gesriot/ferrite-cad/actions/runs/36076669282)
* [Combined runtime](https://github.com/gesriot/ferrite-cad/actions/runs/36076640176)
* [PlaneGCS pin](https://github.com/gesriot/ferrite-cad/actions/runs/36076640156)

Local review scripts, logs, GUI files, SQL/export comparisons and memory
samples: `/private/tmp/ferrite-26i-review/`; a copy is retained in the task's
artifact directory. The GUI protocol supplements the cloud/headless proofs.
The large STEP/partial-FBX campaign was read from exact-head CI rather than
duplicated locally. No source fixtures or foreign worktrees were modified.
