# §31D-1 — Gesture guides and a temporary Length in the Sketch editor

Status at the author handoff: implemented, **unstaged and uncommitted** on branch
`sketch-gesture-guides`; no window was run; the diff has no CI of its own. Base:
`687a65e9296ec1eabbb221f83ff0b12700444688` (merge of PR #106; HEAD = main =
origin/main after a fresh fetch, clean tree; the foreign worktree `a200` untouched).
The plan entry is [§31D](implementation-plan.md). This slice is **not** all of §31D:
acquiring another anchor by hovering is not implemented (see "Not done").

## What the canvas offers

The one Sketch canvas draws a **Line contour**. A new row under the grid step reads
`Guides: Off | Orthogonal | Polar 45° | Length mm [ optional ]`, with a standing
explanation under it. It is editor state beside the grid step and Object snap: it
lives in the editor (so it travels with its tab and ends with its form, exactly like
them), is **Off and blank by default** so every earlier input keeps its exact
behaviour, and nothing is persisted. Typed X/Y and Next X/Y still bypass every snap
and keep their exact text.

Two anchors, both explicit and both drawn (a ring, apart from the target point):

| Gesture | Anchor |
| --- | --- |
| Click that adds a point (open New drawing) | the last vertex already entered; none before the first point, so nothing is offered there |
| Drag of a vertex (New or saved Line contour) | **the vertex's position at the press**, frozen for the gesture. The label says `vertex 3 moved 12.5 mm from its start`: the length is a movement from that anchor, never the length of a neighbouring edge |

An anchor the canvas does not show (scrolled out of view), one whose text is not a
number, a closed New drawing (a click adds nothing) and a full drawing offer no
guide. The anchor never follows the pointer and is not released by distance:
a drag 260 points along its guide still holds it.

## Contract

* **Directions.** In the XY plane (+X right, +Y up), not in pixels: Orthogonal is
  0°, 90°, 180°, 270° (exact unit vectors); Polar 45° adds 45°, 135°, 225°, 315°.
* **Acquisition**, in logical egui points (the canvas's points-per-mm applied once per
  query; Retina pixels never enter), named constants in `sketch/guide.rs`:
  `APERTURE_POINTS = 8` *across* the guide, `MIN_REACH_POINTS = 16` *along* it from
  the anchor, `COMPETITION_POINTS = 1`. The distance is to the guide, not to its
  origin, so a far pointer 3 points off the line holds it and one 9 points off does
  not: no far cursor is forced onto an angle. Nearer than the minimum reach (in
  particular on the anchor, where every direction is equally far) there is no
  direction, hence no zero vector; the free rules apply. A scale that is not a positive
  finite number, or any non-finite number, offers nothing.
* **Ties and change.** The held guide is kept until another is nearer by **more than**
  one point (at exactly one point it is kept). A first choice groups the guides whose
  distance exceeds the nearest by **strictly less** than one point and ranks them axis
  before diagonal, then distance, then table order (E, N, W, S, NE, NW, SW, SE), so the
  answer does not depend on rounding noise. A held guide that is no longer offered is
  dropped. The held guide is frame state; history never sees it.
* **Precedence.** (1) An object point of §31C within its aperture; (2) a held guide;
  (3) the grid step; (4) the free point. An object point is taken exactly (no guide, no
  grid, no rounding). A guided point is the pointer's projection onto the guide; an axis
  guide keeps the anchor's **own text** on the other axis (`20.0` stays `20.0`), every
  other coordinate is rounded to 1e-9 mm (one picometre) and written as the shortest decimal that
  reads back (`0.1 + 0.2` is `0.3`). **The grid step is not applied to a guided point** —
  rounding either coordinate can take a diagonal off its direction and an axis guide
  behind its anchor — and the standing note says so. Away from every guide the grid
  rules as before.
* **Length mm.** Optional. Positive and finite, else refused (`0`, `-0`, negative, `NaN`,
  `inf`, `1e999`, `12 mm`, `1,5`, unparsed text). It is applied only to a gesture that
  has a shown anchor **and** a held guide; elsewhere it is not in use and the free rules
  apply. With a held guide the point is exactly anchor + length × direction, never rounded to the grid; the coordinate text
  uses the same 1e-9 mm precision described above; its distance does not depend on how far the pointer
  is. A length whose point cannot be drawn (beyond 1 000 000 mm, the canvas's limit for a
  vertex) is refused.
* **Length against an object point.** The object point nearest the pointer is *shown and
  taken* only if it lies at the length's point within the document tolerance
  (`OBJECT_MATCH_MM` = `PolygonExtrusion::TOLERANCE_MM`, 1e-6 mm), and then the object's
  own coordinates are written. Otherwise the length wins, the object marker is not drawn,
  and the label names the conflict (`… · Midpoint is not at this length`). One function
  decides, draws and writes, so the point drawn is the point written.
* **A refused length** stays in the box exactly as typed; the standing note turns red
  and quotes it, the canvas caption says `length: refused`, and a gesture that holds a
  guide places nothing: no point is added, a dragged vertex keeps its strings, no Undo
  step. It is never read as zero, as a free distance or as a neighbour's length. A nearby object point cannot bypass this refusal on an acquired guide; off every
  guide and with Guides Off the text is not in use.
* **After a gesture.** The Length text is not consumed, stored or turned into a
  constraint: it stays in the box (shown in the caption and in the note: `Length 15 mm is
  used for every point or drag on a shown guide until you clear it … no constraint,
  nothing saved, no Undo step`) and ends with the form. There is no hidden active
  dimension: while the box holds a number, the preview of every gesture shows it.
* **Gestures.** The guide choice, the length as typed, the anchor and the candidate set
  are frozen at the press (at the click for a new point), like the grid step and the
  object candidates. A toggle of the row is replayed at the event that made it, so in a
  packed frame the earlier drag keeps its setting and only the next gesture changes. The
  row and the box are inert while a gesture is held, while a job runs and while Guides are
  Off (the box is made non-interactive, not only greyed: egui keeps feeding text to a
  focused disabled box). A text key typed in the very frame of a press is read before
  the press. One drag or one added point is one Undo step; hover, direction changes, the
  label and the row take none; Escape, lost focus and a vanished pointer restore the
  press-time strings with Undo and Redo as they were; release (also outside the canvas)
  and cancel clear the gesture. No input is offered under a modal or running job, nor
  through a window that covers the canvas. A result that breaks the contour stays a
  visible draft and is refused by the existing `PolygonExtrusion` check when it is taken.

## Code

`sketch/guide.rs` (pure geometry and decision: `Mode`, `Length`, `acquire`, `resolve`,
`strings`, `label`, `note`; no egui, no state), the wiring in `sketch.rs`
(`Canvas.guides/length/held_guide`, `VertexDrag.guide/guided`, `add_anchor`,
`add_placement`, `preview`, the row, `paint_guide`), tests in `sketch/guide.rs` and
`sketch/guide_tests.rs`. `Canvas::vertex` (one vertex, with the old 1e6 mm limit as
`DRAWABLE_MM`) is what `Canvas::points` already did, now reusable for the anchor. The
paint of the object-snap label became `paint_pointer_label`, shared. `object_snap_tests.rs`
only gained `pub(super)` on the helpers the new tests reuse. No dependency, schema,
kernel, solver, ABI, job or CLI change; the CLI reaches the same result with explicit
coordinates; no pointer CLI.

## Executed (author, local)

All runs sequential (`CARGO_BUILD_JOBS=1`), existing targets and pinned libraries,
pressure level 1 and swap 0 before and during; no viewer, window, CUA or GPU test was
started by the author (the whole suites run under `tools/watch-viewer-memory.py`, 1536 MiB).
Logs: the ignored directory `target/31d-check/`.

| Check | Result |
| --- | --- |
| New tests | 11 geometry/decision (`sketch::guide::tests`), 13 real-event canvas/editor (`sketch::guide_tests`), 1 two-tab (`sessions…::guides_a_typed_length_and_the_guided_draft_follow_their_tab`), 1 native = 26 |
| True stub, whole viewer suite under the watchdog | 702 passed, 0 failed, 2 ignored; peak 148.6 MiB; pressure 1; swap 0; exit 0; 204 skip lines in stderr, one of them this slice's native gate (`skipped: no OCCT for the guide native gate`), which therefore is **not** a geometry pass there; 25 `FCAD_31D_*` markers printed |
| Stub proof | bridge `CMakeCache`: `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE`, `OpenCASCADE_DIR-NOTFOUND`, build warning "Open CASCADE was not usable"; `otool -L` of the stub test binary 0 × libTK and 0 × planegcs (native: 50 and 1) |
| Native (OCCT + PlaneGCS, release), whole suite under the watchdog | 702 passed, 0 failed, 2 ignored; peak 308.7 MiB; pressure 1; swap 0; exit 0; 23 skip lines (stub / real-GUI / no-solver alternatives, none of this slice); 26 markers including `FCAD_31D_NATIVE_EXECUTED sql_cells=2 volume=14500 triangles=20`; the §31B region and §31C native markers also present |
| ci.yml new step (the real `run:` body, 3 940 bytes, 25 exact gates) | true stub and native OCCT without solver: exit 0, 25 × `1 passed; 0 failed`, 25 markers, 0 skipped |
| runtime-layout.yml new native step (real body, macOS substitutions) | exit 0; `$GITHUB_ENV` receives `FCAD_SKETCH_GUIDE_FBX` |
| occt-pin.yml marker line | exact-line grep passes on the native suite log; the gate also ran and passed in the OCCT-without-solver debug shape that workflow uses |
| Native scenario | editor's L by the peer CLI → Orthogonal, Length `15`, vertex 3 (60, 20) pulled west with pixels a little off the line → ring at the press-time vertex and target at (45, 20) → one Undo / Redo → Save edited copy → worker; the numbers typed to `edit-sketch-copy`. Same semantics, meta, objects, dependencies and topology refs; SQL against the source differs in exactly two cells (`objects.payload` and `payload_hash` of the sketch row); all SQL cells of worker and CLI copies equal; IDs and unknown claims kept; cold rebuild and validate by the CLI; STL/FBX of worker and CLI byte-identical; the source file is byte-identical with the same mtime |
| Independent readers | STL read by a separate Python script (`python3 -I`): 20 triangles, box (0,0,0)–(60,40,10), volume 14500.000 mm³, closed directed edges; FBX by pinned ufbx 0.23.0 strict through the real hook lines of `tools/check-fbx-complex.sh` (digests of the cached `ufbx.c/.h` match the pin): `checks=6 failures=0`, `FCAD_SKETCH_GUIDE_UFBX_EXECUTED`; a truncated copy of the same FBX is refused (`checks=1 failures=2`, exit 1) |
| fmt, `clippy --workspace --all-targets --all-features -D warnings` | pass (stub env) |
| actionlint on all workflows; `tools/check-actions-run-size.py` | pass; 206 blocks, largest 20 432 of 21 000 bytes (not a block of this slice; the new ci.yml step is 3 940 bytes), packed argv 20 463 |
| Licence headers (492), export boundary, solver ownership, STEP corpus, planegcs pins, notice ownership, `git diff --check` | pass; no manifest, lock or deny change |
| Mixed (OCCT without solver) whole-suite run | **N/A, reasoned:** no kernel, solver, ABI or job boundary is touched (`check-solver-ownership` passes); the ci.yml block and the native gate were run in the native no-solver configuration instead. The whole viewer suite in that shape fails 8 solver-dependent tests as recorded for §31C, so it is not offered as a pass and was not run |

### Mutations (all compiled and failed on executed assertions; files restored byte for byte)

SHA-256 of `sketch.rs`, `guide.rs`, `guide_tests.rs` before the first mutation and after the
last restoration: `2531738a…`, `604b32d1…`, `32481fb4…` (identical). Two edits with no
effect on these four cases came afterwards (a doc comment in `guide.rs`, one Revolve-profile
assertion block and one open-drawing busy case in `guide_tests.rs`; `sketch.rs` is still
`2531738a…`), and every figure in
the table above is from the final tree.

1. **Anchor drifts with the pointer** (the drag's anchor taken at start + offset instead of
   the press-time vertex). Six editor tests failed, e.g. the saved drag: `left ["27","20"]`,
   `right ["45","20"]` (15 mm west of (60, 20)); the far-anchor, packed, scale, cancel and
   degenerate drags failed the same way.
2. **An object point silently beats a typed length** (the match test removed). The
   geometry test (`a_typed_length_is_exact_and_beats_…`) and the editor test
   (`an_object_point_beats_a_guide_and_only_a_matching_length_keeps_it`, "no object marker
   for a point not taken") failed.
3. **A refused length becomes a free distance.** `a_length_that_is_not_a_positive_finite_number…`
   failed with `left Guide{… millimetres: 10 …}`, `right Refused{heading: 0}`, and the editor
   refusal loop failed on the missing "Length refused" label.
4. **Hysteresis boundary** (`≤` one point changed to `<`). `a_held_guide_is_kept_until_…`
   failed: "exactly one point: kept", `left Some(4)`, `right Some(0)`.

After each restoration the affected positive gates were repeated (the final stub and
native suites and the ci/runtime step bodies above are from the restored tree).

### Found while testing, and fixed

* A negative canvas scale gave a negative "points across" after `abs()`, so `acquire`
  offered a guide; the geometry test caught it. A scale that is not a positive finite
  number now offers nothing.
* A text box that is only disabled (egui `add_enabled`) keeps taking text while it still
  has the keyboard focus; the packed-event test saw `"15"` become `"159"` during a held
  drag. The box is now also non-interactive whenever the row is inert (gesture, job, Guides
  Off).

### Design choices flagged for review

Guides default **Off** (flipping it is one initialiser); the Length text **stays** after a
gesture and ends with the form (it is visible, named in the note and the caption); the grid
is **not** applied to guided points (permitted by the task; explained in the UI); a refused
length refuses only a gesture that holds a guide; the constants (8, 16, 1 points; 1e-6 mm;
1e-9 mm) are named in `guide.rs`; a text key in the very frame of a press is read before the
press; a typed length whose point is beyond the canvas's 1 000 000 mm vertex limit is refused.

### Base CI (not this diff)

For the merge commit `687a65e`, read at the start and again at the end of the work: the
`CI` run 38483533478 (7 of 7 jobs), the planegcs pin 38483533571, both SBOMs and the notices
completed with success; `combined runtime layout` 38483533596 had its linux job succeeded
and its macos and windows jobs still `in_progress`, so that run is not counted as evidence
of anything. Nothing here is a result of a CI run of this diff.

### Diffstat (modified, then untracked; the index is untouched)

| File | State | Lines |
| --- | --- | --- |
| `.github/workflows/ci.yml` | modified | +47 −0 |
| `.github/workflows/occt-pin.yml` | modified | +3 −0 |
| `.github/workflows/runtime-layout.yml` | modified | +33 −0 |
| `crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs` | modified | +56 −0 |
| `crates/ferritecad-app/src/sketch.rs` | modified | +324 −52 |
| `crates/ferritecad-app/src/sketch/object_snap_tests.rs` | modified | +17 −17 |
| `docs/implementation-plan.md` | modified | +23 −11 |
| `tools/check-fbx-complex.sh` | modified | +10 −0 |
| `crates/ferritecad-app/src/sketch/guide.rs` | new / untracked | +888 |
| `crates/ferritecad-app/src/sketch/guide_tests.rs` | new / untracked | +1231 |
| `docs/sketch-gesture-guides.md` | new / untracked | +257 |

Totals are in the handoff report (`git diff --stat` plus `wc -l` of the untracked files).

## Not done, not claimed

A window (author ran none), a viewer, the CUA, any CI of this diff, Windows or Linux
evidence. **Hover acquisition of another anchor** with its hysteresis (acquire inside the
aperture, release outside twice it, replace only when nearer by more than one point) —
this slice has the two anchors that need no hover, and does not implement half of that
mechanism. The circle centre (no circle is drawn here), quadrants, nearest, tangency,
apparent crossings, keyboard cycling, a length typed *during* a held drag, a persistent
dimension, angles and any constraint adapter. §31C as a whole, §31D as a whole, §30,
Milestone 5C, the product and the OOM investigation stay open.

## Independent macOS window recipe (the author does not run it)

Inputs only, outside the checkout, made with the peer CLI and **no** expected output:
`/private/tmp/ferrite-31d-window/inputs/L.json` and `L.fcad` (the editor's L: 60 × 20
foot with a 20-wide column to y = 40, height 10; `shasum -a 256` is recorded in
`SHA256.txt` beside them). Work on a copy; any viewer run only through
`tools/watch-viewer-memory.py` with `--limit-mib 1536` and a time limit, pressure normal.

1. Open a copy of `L.fcad`; press **Edit Sketch …** for its Sketch; **Fit drawing**. The
   row reads `Guides: Off` and the box is greyed.
2. Choose **Orthogonal**, type `15` into **Length mm** (keyboard only), then press vertex 3
   (60, 20) and drag it roughly west, a few points off the line: a ring stays on (60, 20),
   a dashed ray and a circle at (45, 20) appear, the label reads `180° · Length 15 mm ·
   vertex 3 moved from its start`, the line under the canvas `Vertex 3 · X 45 mm · Y 20
   mm`. Release: one **Undo draft** restores (60, 20), **Redo draft** returns (45, 20).
   Escape in a second drag returns it.
3. Clear the box; drag vertex 3 far west along y = 20 (past the canvas edge): the ring
   does not move and X follows the pointer, Y stays `20`. Move 10 points off the line:
   the vertex is free again.
4. Type `-3` or `abc`: the note turns red, the text stays; a guided drag moves nothing.
   Clear it. Switch **Polar 45°** and Snap **5 mm**: a diagonal drag gives unrounded
   equal offsets (the grid is not applied to a guided point); off every guide the 5 mm
   grid still rounds.
5. Restore vertex 3 to exactly X `45`, Y `20` (type it), **Apply vertices**, **Save**;
   only then compare with the CLI (`edit-sketch-copy` on the original with the explicit
   vertices (0,0), (60,0), (45,20), (20,20), (20,40), (0,40) and the original curve IDs):
   SQL cells, UUIDs and references, `export-stl` and `export-fbx` bytes; independent STL
   should be 14500 mm³ and strict ufbx should accept the FBX. Negative control: leave one
   vertex at (60, 20).
6. New drawing: click three points, choose **Polar 45°**; a ring marks the last vertex;
   hover along a diagonal and click: the point lies exactly on it; with `Length 7.5`
   the next click is exactly 7.5 mm away. Turn on **Object snap**: near an endpoint on
   the guide the object wins; with a length that does not reach it the label names the
   conflict. Try a window over the canvas and a running job: nothing is offered.
7. Observe memory pressure and exit status; do not call the OOM investigation closed.

## Independent review

A refused Length on an acquired guide could be bypassed by a nearby object snap,
despite the visible promise that the gesture places nothing. A real-event regression
first failed on the original code; the common resolver now refuses this case before
accepting an object. The same test verifies the unchanged draft and existing Redo.
Off a guide, the field remains unused as documented above.

Typed lengths now keep their full numeric value in the guide label and standing note;
a positive 0.0004 mm no longer reads as 0 mm. Measured distances may still use three
decimals, but a nonzero measurement is not displayed as zero. The coordinate precision
is 1e-9 mm (one picometre), distinct from the 1e-6 mm document tolerance; this corrects
the earlier unit wording.

After these fixes, all 26 focused guide tests passed with OCCT and PlaneGCS, including
the tab owner and native SQL/geometry comparison (14500 mm³, 20 triangles, two changed
SQL cells). The failing-first log and independent repeats are in ignored
`target/31d-review/`. Window and remote CI evidence are recorded separately below when
completed; the author's headless results do not establish those runs.

## Manifest

`crates/ferritecad-app/src/sketch/guide.rs` and `guide_tests.rs` (new), `sketch.rs`,
`sketch/object_snap_tests.rs`, `sessions/tests/tabs/drafts.rs`,
`.github/workflows/{ci,occt-pin,runtime-layout}.yml`, `tools/check-fbx-complex.sh`,
`docs/implementation-plan.md`, this file. The full diffstat is in the handoff report.
