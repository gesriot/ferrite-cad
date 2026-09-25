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
