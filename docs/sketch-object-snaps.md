# §31C — Object snaps in the Sketch editor

Status at the author handoff: implemented, **unstaged and uncommitted** on branch
`sketch-object-snaps`; no window was run; the diff has no CI of its own. Base:
`c89bd8b250a896ed13a6a2e897e6ab25df95107b` (merge of PR #105; HEAD = main =
origin/main after a fresh fetch, clean tree; the foreign worktree `a200` untouched).
The plan entry is [§31C](implementation-plan.md); §31D (guides, polar tracking,
dimensional input) is not started.

## What the canvas shows, and so what can be snapped

The one Sketch canvas draws a **Line contour**: the vertices of `State.points`, the
edges between neighbours and, once **Close contour** is pressed, the closing edge (and,
for Revolve, the reference line X = 0, which is not Sketch geometry and offers no
point). Circle and Annulus are numeric forms; they draw nothing on this canvas, so
their **centre is not available and not claimed**. No invisible candidate was added for
it and the model of the editor was not widened: the circle centre is the next §31C
addition, together with a drawn circle. Endpoint, midpoint and finite crossing are
implemented.

## Behaviour

* **Switch.** `Object snap` is a checkbox at the end of the grid-step row (`Snap: Off 0.1
  mm 1 mm 5 mm 10 mm | Object snap`), and the canvas caption says `object snap: on|off`.
  It is a field of the canvas beside the grid step: per editor (so it travels with its
  tab like the typed text), off for a new editor, no persistence, no document field. A
  decision recorded here: **default off**, like the grid step and so every earlier
  gate keeps its exact behaviour; flipping it is one initialiser.
* **Candidates** of the contour as drawn: Endpoint (each vertex), Midpoint (each edge
  longer than the document tolerance), Intersection (each pair of non-adjacent edges that
  cross). Closing edge only when the contour is closed with more than two vertices.
* **Aperture and competition**, both in logical egui points and named constants:
  `APERTURE_POINTS = 12` (larger than the 9-point vertex grab, so a midpoint or crossing
  just outside a vertex is reachable) and `COMPETITION_POINTS = 1`. The canvas scale
  (points per mm) is applied once per query; Retina pixels never enter, because egui
  positions are already points, and millimetres stay document coordinates.
* **Ranking, two passes, no epsilon comparator.** Least distance among visible
  candidates inside the aperture; keep those whose distance exceeds it by **strictly
  less than one point**; rank by kind (Intersection, Endpoint, Midpoint), then distance,
  then stored order (vertex index, edge index, the two edge indices of a crossing). At a
  gap of exactly one point the nearer wins. The total order does not depend on layout,
  hashing or on the label. `SnapKind` is an enum; its name is a function of the kind
  painted for the frame only. A segment longer than two points is selectable by its
  midpoint when the pointer is nearer to it; a shorter one honestly resolves to its end.
* **Object beats grid.** Only without an object point does the grid step apply. The
  chosen point is written as the shortest decimal that reads back as the same number
  (`25.400000000000002`, not `25.400`), never re-rounded. Switch off: old behaviour.
* **New points.** A click that would add a point uses the nearest object point; a click
  inside the 9-point vertex grab still selects that vertex. Hovering shows the marker
  (a square, triangle or cross) at the point and the name beside the pointer, only where
  a click would act: over an open drawing, not over a closed contour, where only a
  drag shows it.
* **Drag.** The candidates are fixed at the press, with the switch as it was then (the
  grid step is frozen the same way; a later toggle changes the next gesture; the switch
  is disabled while held). The vertex itself and every candidate within the document
  tolerance of its press position (`PolygonExtrusion::TOLERANCE_MM`, 1e-6 mm, the
  distance below which the polygon policy already calls two vertices one) are excluded;
  the midpoints and crossings of its incident edges stay where the press found them and
  are never recomputed from the moving draft. The query point is where the vertex would
  be (the grab offset is kept, as the grid does). A press that has not moved keeps its
  strings. Nothing is copied into `State` or the history; only the drag holds the set.
* **Hover and click** do not build the whole set: the candidates are made inside the
  aperture's window only, with the edges that reach it (equal to the full set cut to the
  window, tested). The largest drawing (256 vertices, a star with 32 256 crossings: 32 768
  candidates, about 1.3 MB) is built once per drag.
* **Finite crossings.** Extensions do not count. Zero-length edges (≤ 1e-6 mm), parallel
  and nearly parallel edges (sine of the angle ≤ 1e-9, about 6e-8 degrees),
  collinear overlap and collinear end-to-end give no crossing (an overlap has no single
  point). An end touching the other edge within 1e-6 mm is a crossing (T-junction); a
  meeting that is an end of both edges is an Endpoint. Non-finite input or output gives
  nothing. A snap that makes the contour degenerate or crossing stays a visible draft
  and is refused by the existing `PolygonExtrusion` check when it is taken; there is no
  second validator.
* **Unchanged.** Typed X/Y and Next X/Y keep their exact text and bypass snapping; no
  Coincident or other constraint, no UUID or topology reference, no new field; hovering,
  the label and toggling are no history; one finished drag is one Undo; Escape, lost
  focus and a vanished pointer restore the press-time strings with Undo and Redo as they
  were; packed pointer events, press and release in one frame and a release outside the
  canvas are as for the grid. Nothing is offered through another window, while a job
  holds the form, or from outside the visible canvas. Close contour, Restore saved
  vertices and every §31B layout gate are untouched.

## Code

`sketch/object_snap.rs` (pure geometry and ranking, no egui, no state), the wiring in
`sketch.rs` (`Canvas.object_snap`, `VertexDrag.objects/hit`, `Canvas::object_hit`,
`preview`, the Snap row, `paint_object_snap`), tests in `sketch/object_snap.rs` and
`sketch/object_snap_tests.rs`. `drag_tests.rs` only gained `pub(super)` on six helpers
and a parameterised `verify_published` (the old `verify_publications` is a call of it
with the old numbers). No dependency, schema, kernel, solver, ABI, job or CLI change.
The CLI already reaches the same result with explicit coordinates; no pointer CLI.

## Executed (author, local, `CARGO_BUILD_JOBS=1`)

| Check | Result |
| --- | --- |
| New tests | 13 geometry/ranking, 10 real-event canvas/editor, 1 two-tab, 1 native = 25 |
| True stub, whole viewer suite under `tools/watch-viewer-memory.py` (1536 MiB) | 676 passed, 0 failed, 2 ignored; peak 168.1 MiB; pressure 1; swap 0; exit 0. Native gates skip themselves there (203 skip lines in stderr, one of them this slice's) and are **not** a geometry pass |
| Stub proof | bridge `CMakeCache`: `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE`, `OpenCASCADE_DIR-NOTFOUND`, `find_package` error; `otool -L` of the stub test binary 0 × libTK (native 50) |
| Native (OCCT + planegcs, release), same suite under the watchdog | 676 passed, 0 failed, 2 ignored; peak 292.0 MiB; pressure 1; swap 0; exit 0; 23 skips are the stub/GUI/no-solver alternatives, none of this slice's; marker `FCAD_31C_NATIVE_EXECUTED sql_cells=2 volume=14000 triangles=20` and the §31B region gate both executed |
| ci.yml step (the real `run:` body, 18 new exact gates) | stub and native-without-solver: exit 0, 40 × `1 passed; 0 failed`, 18 × `FCAD_31C_*` markers, 0 skipped |
| runtime-layout.yml new native step (real body, macOS substitutions) | exit 0; `$GITHUB_ENV` receives `FCAD_SKETCH_SNAP_FBX` |
| occt-pin.yml marker line, ufbx hook of `tools/check-fbx-complex.sh` | exact-line grep passes on the native suite log; hook block run against the snap FBX prints `FCAD_SKETCH_SNAP_UFBX_EXECUTED` |
| Native scenario | New L by the peer CLI → Object snap on, vertex 2 (60, 20) dragged with slightly inexact pixels onto the press-time middle (40, 20) of its own edge → Undo/Redo → Save edited copy → worker; the numbers typed to `edit-sketch-copy`. Same semantics, meta, objects, dependencies and topology refs; SQL against the source differs in exactly two cells (`objects.payload` and `payload_hash` of the sketch row); IDs and unknown claims kept; cold rebuild and validate by the CLI; STL/FBX of worker and CLI byte-identical |
| Independent readers | STL read by a separate Python script: 20 triangles, box (0,0,0)–(60,40,10), volume 14000.0 mm³; FBX by pinned ufbx 0.23.0 strict (digests of the cached `ufbx.c/.h` match the pin): `checks=6 failures=0` |
| fmt, `clippy --workspace --all-targets --all-features -D warnings` | pass (stub env) |
| actionlint on all workflows; `tools/check-actions-run-size.py` | pass; 204 blocks, largest 20 432 of 21 000 bytes (not a block of this slice), packed argv 20 463 |
| Licence headers (490), export boundary, solver ownership, STEP corpus, planegcs pins, notice ownership | pass; no manifest, lock or deny change |
| Native OCCT **without** solver, whole viewer suite (the occt-pin shape, `REQUIRE_OCCT=1`) | 668 passed, **8 failed**, 2 ignored; the failures are seven `constraints::tests::native_*` and `sessions::tests::recovery::native_a_killed_window…`, all of which need PlaneGCS (`this build did not link planegcs`, `REQUIRE_OCCT=1` without a solver); none touches this slice. It is not a configuration the repo runs a whole suite in, so it is not offered as a pass; it did execute this slice's native gate (marker line grep of occt-pin.yml passes) and the §31B region gate. Peak 309.7 MiB, pressure 1, swap 0 |
| Mixed (OCCT without solver) run | **N/A, reasoned:** no kernel, solver, ABI or job boundary is touched (`check-solver-ownership` passes); the ci.yml block was run in the native no-solver configuration instead |

### Mutations (both compiled and failed on executed assertions; files restored byte for byte, SHA-256 `ace84c77…8e15` before and after)

1. **Grid after the object hit** (`Snap::apply` on the object point, in the click and the
   drag path). `an_object_point_beats_the_grid…` failed: `left ["15","0"]`, `right
   ["16.5","0"]`; with only the drag path mutated, `left ["15","20"]`, `right
   ["16.5","21"]`.
2. **Candidates recomputed from the moving draft** at every preview.
   `a_drag_lands_on_the_frozen_incident_midpoint…` failed: "no middle follows the
   draft", `left ["15","0"]`, `right ["16","1"]`.

The numbers in the table are from the final tree, which holds the restored file and the
test cases added after the mutations. Logs: ignored directory `target/31c-check/`.

## Not done, not claimed

A window (author ran none), a viewer, the CUA, any CI of this diff, Windows or Linux
evidence, the circle centre, quadrants, nearest, tangency, apparent crossings, keyboard
cycling of overlapping candidates, §31D, §30, Milestone 5C, the product and the OOM
investigation. Base CI (not this diff), checked at the start and again at 20:53 UTC before this
handoff: for `c89bd8b` the `CI` run 38460746586, the planegcs pin, both SBOMs and the notices
succeeded; `combined runtime layout` 38460746459 had its linux job succeeded and its macos
and windows jobs still `in_progress`, so that run is not counted as evidence of anything.

## Independent macOS window recipe (the author does not run it)

Inputs only, outside the checkout, made with the peer CLI and **no** expected output:
`/private/tmp/ferrite-31c-window/inputs/L.json` and `L.fcad` (the editor's L, 60 × 20
foot with a 20-wide column to y = 40, height 10; `shasum -a 256 L.fcad` =
`80f8822c…63f2`). Work on a copy; if any GPU or viewer run is needed, only through
`tools/watch-viewer-memory.py` with `--limit-mib 1536` and a time limit, pressure normal.

1. Open a copy of `L.fcad`; in the Actions panel press **Edit Sketch … —** for its Sketch
   (the form is titled *Edit saved Sketch*; the Model tree opens the same form); **Fit drawing**. Tick
   **Object snap** (caption now says `object snap: on`; grid step still Off).
2. Hover the middle of the edge from (60, 20) to (20, 20): nothing is painted (this
   contour is closed, so a click here would add no point); Undo draft stays disabled.
3. Press vertex 3 (60, 20) and drag towards that middle, the pointer a few points off:
   a triangle marker is on (40, 20) and `Midpoint` is beside the pointer; the line under
   the canvas reads `Vertex 3 · X 40 mm · Y 20 mm`. Release: one **Undo draft** restores
   (60, 20), **Redo draft** returns (40, 20). Escape during a second drag returns it.
4. Untick **Object snap**, drag vertex 3 near (40, 20): it follows the pixels, no marker.
   Tick it again; type 40.5 into vertex 3's X box: the text stays `40.5`.
   Before applying, restore vertex 3 to exactly X `40`, Y `20`; the following
   comparison measures that snapped draft, not the later manual-input probe.
5. **Apply vertices**, **Save**; then, only now, produce the comparison with the CLI
   (`edit-sketch-copy` on the original with the explicit vertices (0,0), (60,0), (40,20),
   (20,20), (20,40), (0,40) and the original curve IDs) and compare SQL cells, UUIDs and
   references, `export-stl` and `export-fbx` bytes; independent STL should be 14000 mm³
   and strict ufbx should accept the FBX. Negative control: leave one vertex at (60, 20).
6. New drawing: click three points, switch on, hover and click near the middle of the
   first edge (`Midpoint`) and near a crossing made by a later edge (`Intersection`);
   typed `Next X/Y` stay exact. Try a window over the canvas and a running job: no marker.
7. Observe memory pressure and exit status; do not call the OOM investigation closed.

## Independent review

The cropped candidate query initially missed a T-junction accepted by the crossing
tolerance when the other edge's exact bounds stopped just outside the query window.
The existing window-equivalence test now exercises this boundary: it failed on the
original implementation with an empty cropped set versus one full-set intersection.
The broad phase now expands edge bounds by the same document tolerance. This test
also runs by exact name and marker in CI (19 snap gates in that loop).

After the fix, the 25 object-snap tests passed with OCCT and PlaneGCS, including
the native publication comparison. Workspace clippy with all targets/features and
`-D warnings` passed. Local logs are in the ignored `target/31c-review/` directory.
The window recipe also explicitly restores the snapped coordinate after its separate
manual-text probe. The independent macOS window run used the fresh staged bundle from code commit
`9dfe3f3`, a private copy of the L input, and the production memory watchdog. In the
saved editor, an inexact drag of vertex 3 from (60, 20) snapped to exactly (40, 20).
One Undo restored (60, 20), Redo restored (40, 20). Switching object snap off allowed
free pixel-following coordinates; typing `40.5` with the switch on preserved that
text. Undo restored the snapped draft before **Apply vertices → Save**.

The resulting file was made by the window before constructing its CLI comparison.
All SQL cells equal the explicit-coordinate CLI copy; compared with the source,
only the selected Sketch's `objects.payload` and `payload_hash` differ. UUIDs and
refs are unchanged. The original input hash is unchanged. Both copies pass validate
and cold rebuild. The actual window exports and both cold CLI exports are byte-equal:
STL 1084 bytes, 20 triangles, closed directed edges, bounds 60 × 40 × 10 mm and signed
volume 14000 mm³; FBX 4913 bytes, pinned ufbx 0.23.0 strict `checks=6 failures=0`.
An unchanged-source negative control has volume 16000 mm³ and different SQL/STL.

In a separate unaccepted New contour, all three glyphs and labels were observed:
Intersection, Midpoint, Endpoint. With grid 5 mm enabled, adding the crossing kept
its computed Y near 25.047 rather than rounding to 25; adding the midpoint kept
40.047. Clicking just outside the existing vertex-selection radius added the exact
endpoint. Closing that repeated-vertex draft produced the existing visible refusal;
it remained editable and was then cancelled without publication.

The one viewer (PID 77588) exited normally with code 0 after 319 seconds. Sampled peak
footprint was **204.892 MiB**, pressure stayed 1 and swap stayed 0; the 1536 MiB watchdog
did not abort. Exit was checked by PID, without another CUA app lookup. Held-drag
Escape, busy/covered-canvas and 1x/2x scenarios were checked by the event tests, not
claimed as additional window observations. OOM remains unexplained.

Independent true-stub rerun: 24 non-native tests executed; one native test explicitly
returned with `skipped: no OCCT` (25 harness successes, not 25 geometry executions).
The rebuilt stub reports Open CASCADE unavailable. Logs, the SQL/STL comparator,
window measurement and ufbx output are in ignored `target/31c-review/`.
Remote CI is tracked on [PR #106](https://github.com/gesriot/ferrite-cad/pull/106);
the local results above do not claim completion of that separate matrix.

## Manifest

`crates/ferritecad-app/src/sketch/object_snap.rs` and `object_snap_tests.rs` (new),
`sketch.rs`, `sketch/drag_tests.rs`, `sessions/tests/tabs/drafts.rs`,
`.github/workflows/{ci,occt-pin,runtime-layout}.yml`, `tools/check-fbx-complex.sh`,
`docs/implementation-plan.md`, this file. The full diffstat is in the handoff report.
