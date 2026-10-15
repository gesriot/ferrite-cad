# §31D-2 — Guide anchors taken from visible geometry by hovering (Sketch editor, Line canvas)

Status at the author handoff: implemented, **unstaged and uncommitted** on branch
`sketch-hover-guide-anchors`; no window was run; the diff has no CI of its own. Base:
`853a6bf95a11430a6becfdbfcd12aaee3d015975` (merge of PR #110, §31G collinear Lines; HEAD =
main = origin/main after a fresh fetch, clean tree, no commits ahead; the foreign worktree
`a200` untouched). The plan entry is [§31D](implementation-plan.md). This closes the part of
§31D that [sketch-gesture-guides.md](sketch-gesture-guides.md) listed as "Not done" for the
**present Line canvas** only; the circle centre (§31C), the other constraint adapters, §30,
Milestone 5C, the product and the OOM investigation stay open.

## The one gesture, and why it is reachable

§31D-1 measures a guide from the last vertex (a click that adds a point) or from the
position the dragged vertex had at the press. Neither can start from *another* shown point:
a press on an end selects and drags that end, and a click in New adds a point. A hover
anchor therefore cannot be added to a helper alone; it needs a press that means "start
from this point". The chosen, explicit gesture is an **editor-only mode "From object"**, a
checkbox at the end of the Guides row (`Guides: Off | Orthogonal | Polar 45° | Length mm
[ ] | ☐ From object`). It is **off by default**, available only while Guides are on, and
nothing is persisted.

| State | Event (egui) | Result | Strings / selection / history |
| --- | --- | --- | --- |
| Mode off, or Guides Off | anything | exactly the §31C/§31D-1 behaviour: a press on an end selects and drags it, a click in New adds a point | unchanged |
| On, New open drawing with room, or saved Sketch with a **selected** vertex | `PointerMoved` within 12 logical points of an Endpoint, Midpoint or finite Intersection of the drawn Lines | **provisional**: a blue ring (radius 11) on the point, its words beside the pointer, standing words on the canvas | none changed |
| provisional | pointer moves, anchor still ≤ 24 points away | held | none |
| provisional | another candidate that would be offered on its own is nearer by **more than 1 point** | replaced (at exactly 1 point the held one stays) | none |
| provisional | Escape, anchor > 24 points away, pointer off the canvas, `PointerGone`, focus lost, Guides Off, mode off, covered by another window, job running, vertex text unreadable, tab hidden, document changed, form closed | **cleared** | none |
| provisional | `PointerButton` press | **confirmed**: anchor, mode, Length text, grid step, candidates and scale are frozen; the press is *not* a vertex press (nothing is selected or dragged) | selection untouched |
| confirmed | pointer moves, any distance, past any other point | preview from the frozen anchor; never re-acquired or released by distance | no history |
| confirmed | release (also outside the canvas) | **one** Undo step if the strings changed; the anchor is cleared | New: one vertex added; saved: the selected vertex moved |
| confirmed | release without a movement | nothing; no duplicate of the anchor in New | unchanged |
| confirmed | Escape, window focus lost, `PointerGone` | the press-time strings come back; Undo/Redo exactly as they were | restored |
| On, saved Sketch, **no selected vertex** | — | nothing is offered; the canvas says "select the vertex to move first"; a press on a vertex is the old press that selects it | — |
| On, saved Sketch | press on the **selected vertex itself** (or a vertex stacked on it) | it is no anchor (excluded, as in §31C): the ordinary drag | as before |
| On, New, closed or full | — | nothing is offered; the canvas says so | — |

Only a press on a **shown ring** is taken by the mode. Without a ring every press and
click is the old one. With the mode on and a vertex selected in a saved Sketch, a press on
*another* vertex starts a gesture from it instead of selecting it: to select another vertex
with the mouse, switch the mode off (or press it while nothing is selected). That cost of
the explicit mode is stated here, not hidden.

### Reachability by real egui events

Every anchor in `sketch/anchor_tests.rs` is reached by `PointerMoved`, and every gesture
starts with a `PointerButton` press on the ring the previous frame painted, through the
production `Editor::draw` (no helper is called in place of the interface):

* New, open L `(0,0) (40,0) (40,30)`, Orthogonal, mode on: hover 3 points from vertex 1 →
  ring and words `From Endpoint · vertex 1 at (0, 0) mm`; press there; move to
  `(0.5, 22.5)` mm → vertex 4 `["0","22.5"]` previewed, label `90° · 22.5 mm from
  Endpoint · vertex 1 · adds vertex 4` and the confirmed ring on vertex 1 (not on the last
  vertex `(40,30)`); release → history `(1,0)`; Undo/Redo restore and return it. The same
  with a finite crossing (`Intersection · edges 1>2 × 3>4`) and a midpoint
  (`Midpoint · edge 2→3`).
* Saved L, mode on, vertex 3 selected by an ordinary press: hover vertex 5 → ring; press;
  move east with Length 15 → vertex 3 becomes `("35","40")`, vertex 5 untouched, selection
  still vertex 3, label `0° · Length 15 mm from Endpoint · vertex 5 · moves vertex 3`.
* The same sequence in **one frame** (`Moved, Press, Moved, Release`), two whole gestures
  in one frame (the second anchored on the vertex the first added), a mode toggle after a
  gesture in the same frame (the gesture keeps what it was pressed under).

## Contract

* **Sources.** Only the Endpoints, Midpoints and finite-segment Intersections of the
  contour the canvas draws and can read now (§31C `object_snap::collect`, its ranking and
  its coordinate transform). No hidden circle or centre, no apparent crossing, no extension
  crossing, no new profile schema. A candidate outside the visible canvas is not offered.
* **Acquisition.** In logical points (the canvas scale is applied once per query):
  `APERTURE_POINTS = 12` to acquire, `RELEASE_POINTS = 2 × 12 = 24` to give up (inclusive
  at both, so 12 acquires and 24 still holds), `REPLACE_POINTS = 1` (a rival replaces the
  held anchor only when strictly nearer by more than this, at exactly 1 the held anchor
  stays). The **first** anchor is the §31C two-pass choice itself (least distance, the
  group strictly less than one point further ranked by kind, then distance, then stored
  order); no bonus, no pairwise epsilon, no ranking by text. A replacement is the best
  by that ranking among the candidates nearer than the held one by more than one point and
  offered on their own. A scale that is not a positive finite number, or a non-finite
  pointer, holds nothing.
* **Saved vertex.** The moved vertex and every candidate within the document tolerance
  (`COINCIDENT_MM`, 1e-6 mm) of its position are not offered. The Midpoints and Intersections
  of its incident edges are offered as the press finds them and are frozen with the anchor:
  the ring stays where the anchor was while the edges move with the draft. The anchor creates
  no geometry, no constraint and no ID.
* **The gesture.** Directions and constants are §31D-1's (Orthogonal/Polar 45°, 8 points
  across, 16 along, 1 point of competition, measured to the guide, not to its origin). The
  point is where the pointer is in the document (the press point plus its movement), not a
  grab offset of the moved vertex; the guide starts at the anchor. Precedence: object →
  guide → grid → free; a guided point is never rounded to the grid; the anchor's own
  position is not an object target of its gesture (it is where the guide starts); a typed
  Length is a distance **from the anchor**; an object point is taken only if it is at that
  length within `OBJECT_MATCH_MM`; any other Length text keeps its text and, on an acquired
  guide, places nothing (an object cannot bypass it); off a guide the field is not used.
  The text of the other axis of an axis guide is the anchor's number in shortest decimal.
* **New.** The draft is rebuilt from the press-time strings at every preview: `before` plus
  one point when there is a target. A press that did not move has no target. Close contour,
  Create and the shared polygon check are untouched; what leaves the form is the typed
  polygon (a widget test compares the `NewDocument` with `PolygonExtrusion::new`).
* **History.** Hover, a provisional anchor, its replacement and the standing words take no
  Undo step and change no string, selection or dirty flag. A finished gesture that changed
  the strings is one step; Escape, lost focus and a vanished pointer restore the press-time
  strings with Undo and Redo as they were. Typed X/Y and Next X/Y bypass every snap.
* **Where the state lives.** The mode (`Canvas.anchors`) is the editor's setting beside
  Guides and travels with its tab. The provisional anchor (`Canvas.provisional`) is frame
  state: it is cleared when the pointer leaves, on `PointerGone`/focus loss, when Guides or
  the mode go off, when a tab's form is set aside (`Forms::park`) and when the document
  changes under the form (`Editor::finish_session_change`); `dismiss` resets everything.
* **Words.** The ring and the words name the real anchor and the length from it
  (`180° · Length 10 mm from Midpoint · edge 2→3 · adds vertex 5`, a conflict with a typed
  Length appended). The standing words of the mode are painted on the canvas under its
  caption (two short lines: the mode, the anchor and whether the gesture `adds vertex N` or
  `moves vertex K`, or why nothing is offered), so the form is exactly as tall as with
  Guides on and the mode off. Words are frame state: not stored, not compared.
* **Not offered:** under a modal or running job, through a window that covers the canvas,
  outside the visible canvas, over geometry that cannot be read, in a closed or full New
  drawing, in a saved Sketch without a selected vertex.

## Code

`sketch/anchor.rs` (pure: `track`, `Moves`, `Situation`, `describe`, `status`; no egui, no
state), the wiring in `sketch.rs` (`Canvas.anchors/provisional`, `VertexDrag.anchored`,
`Anchored`, `anchor_moves`, `track_anchor`, `anchor_situation`, `preview_anchored`, the row's
checkbox, `pointer_over`, `paint_standing`, `paint_provisional`), `object_snap.rs`
(`Candidate::source`, `drop_at` shared with `frozen`, `Window::contains` visible to
siblings), `guide.rs` (`Role::AddFrom/DragFrom` and their labels, `shown` visible),
`tabs.rs` (one call in `Forms::park`). Tests in `sketch/anchor.rs` and `sketch/anchor_tests.rs`
(`guide_tests.rs` only gained `pub(super)` on the helpers it shares). No dependency, schema,
kernel, solver, ABI, job or CLI change; the CLI reaches the same result with explicit
coordinates; there is no pointer CLI.

## Executed (author, local)

All runs sequential (`CARGO_BUILD_JOBS=1`), existing targets and pinned libraries; pressure
level 1 and swap 0 in every sample; no viewer, window, CUA or GPU test was started. Every
widget run (including each mutation) went through `tools/watch-viewer-memory.py`, limit
1536 MiB. Logs: the ignored `target/31h-check/`. These are the numbers of the final tree.

| Check | Result |
| --- | --- |
| New tests | 6 geometry (`sketch::anchor::tests`), 14 real-event canvas/editor (`sketch::anchor_tests`), 1 two-tab (`sessions::tests::tabs::drafts::the_anchor_mode_follows_its_tab_and_the_resting_anchor_follows_nobody`), 1 native session gate = 22 |
| True stub, whole viewer suite under the watchdog | 742 passed, 0 failed, 2 ignored; peak 141.6 MiB; exit 0; 211 skip lines in stderr (one is this slice's native gate, `skipped: the session gates need Open CASCADE`, which is therefore **not** a geometry pass there); 21 `FCAD_31H_*` markers printed |
| Stub proof | bridge `CMakeCache`: `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE`, `OpenCASCADE_DIR=…/absent-occt-true-stub`; build warning "Open CASCADE was not usable"; `otool -L` of the test executable: 0 × libTK, 0 × planegcs (native: 50 and 1) |
| Native (OCCT + PlaneGCS, release), whole suite under the watchdog | 742 passed, 0 failed, 2 ignored; peak 267.7 MiB; exit 0; 23 skip lines (stub/real-GUI/no-solver alternatives, none of this slice); 22 markers including `FCAD_31H_NATIVE_EXECUTED volume=3072.65625 triangles=12`; the §31C/§31D native markers also present |
| ci.yml step (the real `run:` body, now 46 exact gates: 25 of §31D-1 and 21 new), each test executable run under the watchdog instead of `cargo test` | true stub and native OCCT **without** solver: exit 0, 46 × `1 passed; 0 failed`, 21 `FCAD_31H_*` and 25 `FCAD_31D_*` markers, 0 skipped |
| runtime-layout.yml native step (the real body, macOS substitutions, both native gates through the watchdog) | exit 0; `$GITHUB_ENV` receives `FCAD_SKETCH_GUIDE_FBX` and `FCAD_SKETCH_ANCHOR_FBX`; both markers printed |
| occt-pin.yml marker line | `grep -Fxq 'FCAD_31H_NATIVE_EXECUTED volume=3072.65625 triangles=12'` passes on the native suite log |
| Native scenario (small saved profile) | the sample plate, Apply height 9.5, then through the widgets: Fit, Orthogonal, Length 5, From object, an ordinary press selects vertex 2, a press on the **shown** vertex 1 `(33, 15.5)` dragged south → vertex 2 `(33, 10.5)`; Apply vertices → document Undo/Redo → export before Save → Save → cold reopen. Against the peer CLI given those coordinates through `edit-sketch-copy`: SQL cells of the saved file equal the CLI copy; cold rebuild resolves every stored reference; STL and FBX of the unsaved export equal the CLI's, and the Undo steps equal the height-only and original exports; the source file is byte-identical until Save |
| Independent readers | STL parsed by a separate Python script (`python3 -I`, `target/31h-check/stl_measure.py`): 12 triangles, 684 bytes, bounds (-4.5, 3.25, 0)–(33, 15.5, 9.5), volume 3072.65625 mm³, every directed edge closed; FBX by pinned ufbx 0.23.0 strict through the real hook lines of `tools/check-fbx-complex.sh`: `checks=6 failures=0`, `FCAD_SKETCH_ANCHOR_UFBX_EXECUTED`; a copy truncated to 2000 bytes is refused (`checks=1 failures=2`) |
| fmt, `clippy --workspace --all-targets --all-features -D warnings` | pass (stub env) |
| actionlint on all workflows, shellcheck `tools/check-fbx-complex.sh`, `tools/check-actions-run-size.py` | pass; 210 blocks, largest 20 432 of 21 000 bytes (not this slice's block), packed argv 20 463 |
| Licence headers (503), export boundary, solver ownership, STEP corpus, planegcs pins, notice ownership, `git diff --check` | pass; no manifest, lock or deny change |
| Mixed (OCCT without solver) whole-suite run | **N/A, reasoned:** no kernel, solver, ABI or job boundary is touched (`check-solver-ownership` passes); the ci.yml step was run in that shape instead (46 gates). The whole viewer suite in that shape fails the solver-dependent tests as recorded for §31C, so it is not offered as a pass and was not run |

### Mutations (existing harness copy `target/31h-check/mutate.py`; all compiled and failed on executed assertions; files restored byte for byte; positive repeat green)

SHA-256 of `sketch.rs`, `anchor.rs`, `tabs.rs`, `anchor_tests.rs` before the first
mutation and after the last restoration: `b4bbb689…`, `d1461c3f…`, `1694b83e…`,
`c2ebab77…` (identical).

1. **The anchor follows the edited geometry** (a saved gesture measures from the moved
   vertex's live position): `a_saved_vertex_moves_from_another_anchor…` failed, `left
   ["40.5","40.25"]`, `right ["35","40"]`; the native gate failed on `[33.25, 7.999…]`
   against `[33.0, 10.5]`.
2. **Switch at exactly one point** (`>` to `>=`): the geometry gate failed for "held 5,
   rival 4, scale 1".
3. **The resting anchor is not forgotten when a tab's form is set aside** (the call in
   `Forms::park` removed): the two-tab gate failed, `left (true, true)`, `right (true,
   false)`.
4. **The press ignores the shown anchor:** the New gate failed on "the anchor is not
   selected" (`left Some(0)`, `right None`), the saved gate on the selection moving to the
   anchor (`left Some(4)`, `right Some(2)`).
5. **The hold ends at the aperture:** the geometry gate failed "held at 12.001".
6. **An unmoved press adds a point:** the New gate failed with a fourth point and history
   `(1,0)`.

### Found while building, and changed

* The first form had the standing line as a widget under the canvas. In the worst saved case
  the repository measures (the four-Fillet base against 768 points) the form then ended at
  767 of 768 points. The words are now painted on the canvas: the form is as tall as with
  Guides on and the mode off (754 in that case; 744 with Guides off).
* A packed frame whose click on a checkbox is followed by a canvas gesture registers only
  the control clicked *last* (egui hit-tests at the final pointer position, as recorded for
  §31C); the packed test therefore toggles after the gesture, and the toggle-before-press
  case is a separate frame, as a person does it.
* A window New drawing holds tab switching (`making_new`), so a tab-switch test with a
  drawing is unreachable; the two-tab gate covers the saved forms, and the drawing's anchor
  is cleared by `finish_session_change` and by closing.

### Design choices flagged for review

The mode is **off** by default and needs Guides (flipping that is one initialiser); the mode
stays remembered but unavailable while Guides are Off; with the mode on a press on another
vertex of a saved Sketch starts a gesture from it instead of selecting it (limitation above);
the moved vertex goes where the pointer would put a new point (no grab offset); an object
target never snaps to the gesture's own anchor; releasing outside the canvas ends the gesture
on its last preview as the §31D-1 drag does; the standing words are painted on the canvas;
the words `vertex N`, `edge a>b`, `edges a>b × c>d` use the canvas's 1-based numbers. The
constants (12, 24, 1 points) are the §31C ones, named in `anchor.rs`.

## Author handoff: not done, not claimed

At handoff, a window (the author ran none), a viewer, the CUA, any GPU or remote CI run of this diff,
Windows or Linux evidence were not available (independent review follows below).
Still out of scope: the circle centre (no circle is drawn here), quadrants, nearest,
tangency, apparent crossings, keyboard cycling of overlapping candidates, a Length typed
*during* a held gesture, a persistent dimension, angles and any constraint adapter, a hover
or pointer command in the CLI. §31C as a whole, §31D as a whole beyond this canvas, §30,
Milestone 5C, the product and the OOM investigation stay open.

Base CI (not this diff), read at the start and again before this handoff: for `853a6bf` the
`CI` run 38650712572, the planegcs pin 38650712579, both SBOMs and the notices completed
with success; `combined runtime layout` 38650712679 was still `in_progress` and is not
counted as evidence of anything. The PR #110 code passed 15/15 checks and its final docs head
7/7.

## Independent macOS window recipe (the author does not run it)

Inputs only, outside the checkout, made by `/private/tmp/ferrite-31h-window/make_inputs.py`
(SHA-256 `129e217f…693f`; it writes the editor's L and nothing else: no expected output,
no screenshot). Run it with the freshly built native CLI and the OCCT library path:
`python3 -I make_inputs.py <checkout>/target/release/ferritecad inputs` →
`inputs/L.json` (`e7172796…6ee6f65`) and `inputs/L.fcad` (`38e02d73…210c6`; 60 × 20 foot with
a 20-wide column to y = 40, height 10), `SHA256.txt`. Work on a copy; any viewer run only
through `tools/watch-viewer-memory.py --limit-mib 1536 --seconds <limit>`, pressure normal.

1. Open a copy of `L.fcad`; press **Edit Sketch …** for its Sketch; **Fit drawing**. The Guides
   row reads `Guides: Off`; **From object** is greyed. Choose **Orthogonal**: it becomes
   available. Type `15` into **Length mm** (keyboard only). Tick **From object**. The canvas
   says `From object: select the vertex to move first (press it), then press another shown
   point.`
2. Press and release vertex 3 `(60, 20)` (the first press is the old press): its row is
   highlighted, selection = vertex 3; the canvas now says `… hover an endpoint, midpoint or
   crossing, press it, drag and release; it moves vertex 3.`
3. **Hover.** Move to vertex 5 `(20, 40)`: a blue ring and `From Endpoint · vertex 5 at (20,
   40) mm` beside the pointer. Walk 20 points away: the ring stays (held up to 24). Go
   beyond 24: it goes. Hover the middle of the edge to vertex 6 `(10, 40)` (`Midpoint · edge
   5>6`), then approach vertex 5 again: the ring changes only when the other point is clearly
   nearer (more than one point). Nothing changes in the rows; **Undo draft** stays disabled;
   hovering the selected vertex 3 itself shows no ring.
4. **Start and finish.** Press on the ring at vertex 5 (the ring turns green, radius 8, the
   selection stays on vertex 3) and drag east, a few points off the line: a dashed ray, a
   circle at `(35, 40)`, the label `0° · Length 15 mm from Endpoint · vertex 5 · moves vertex
   3`, vertex 3 reads `X 35 · Y 40`. Move far past other points: the green ring does not
   move. Escape: vertex 3 is `(60, 20)` again, Undo/Redo as before. Repeat and release: one
   **Undo draft** restores `(60, 20)`, **Redo draft** returns `(35, 40)`. A press that does
   not move changes nothing.
5. Clear the Length box and repeat from a midpoint anchor; try a refused text (`abc`, `-3`):
   the gesture places nothing and the text stays. Turn **Object snap** and the grid on and
   compare (object → guide → grid → free). Switch Guides **Off**: the ring goes, the canvas
   says the mode is unavailable, and the checkbox stays ticked but greyed; switching Guides
   back on finds the mode on again. A job running or a window over the canvas: no ring.
6. Final state for the comparison: vertex 3 at exactly `(35, 40)` produced by the gesture
   (not by typing). **Save edited copy…** → Cancel (the draft stays) → **Apply vertices** →
   unsaved export (STL, FBX) → **Save**. Then, with the original `L.fcad`, the CLI: read the
   ids with `ferritecad inspect --json L.fcad` (`result.content_version`, `result.sketches[0]`
   `sketch_id` and `vertices[].curve_id`) and run `ferritecad edit-sketch-copy L.fcad --sketch
   <id> --expect-version <content_version> --request <json with the same curve ids and the
   vertices (0,0) (60,0) (35,40) (20,20) (20,40) (0,40)> -o cli.fcad --json`; compare all SQL
   cells, UUIDs and references, `validate`, `rebuild --cold`, `export-stl` and `export-fbx`
   bytes with the window's. An independent STL reader should find 20 triangles, bounds
   (0,0,0)–(60,40,10) and volume 17500 mm³ (the unchanged input is 16000 mm³: negative
   control); pinned ufbx must accept the FBX (`tools/check-fbx-complex.sh`'s hook lines).
7. New drawing (a separate, disposable window, cancelled before Quit): click four points of
   a zigzag whose first and last edges cross, roughly `(0,0)`, `(30,30)`, `(30,0)`, `(0,15)`
   (the crossing is near `(10, 10)`); choose Orthogonal and tick **From object**. Hover the
   crossing (`Intersection · edges 1>2 × 3>4` with its actual numbers), press, drag east: a
   point is added; one Undo removes it. A click away from every ring is the old click; after
   **Close contour** the canvas says `this drawing is closed`.
8. Two tabs with two L copies: the mode and its Length follow their tab; hovering an anchor
   in tab A and switching to B and back shows no ring until the pointer moves again.
   Optionally a 2x display. Observe the memory pressure and the exit status; do not call
   the OOM investigation closed.

## Manifest

`crates/ferritecad-app/src/sketch/anchor.rs` and `anchor_tests.rs` (new), `sketch.rs`,
`sketch/object_snap.rs`, `sketch/guide.rs`, `sketch/guide_tests.rs`, `tabs.rs`, `sessions.rs`
(its native vertex gate became `vertex_gate`, called by the old gate and by the new test),
`sessions/tests/tabs/drafts.rs`, `.github/workflows/{ci,occt-pin,runtime-layout}.yml`,
`tools/check-fbx-complex.sh`, `docs/implementation-plan.md`, this file. The diffstat is in the
handoff report.

## Independent review — lifecycle repair

A regression test reproduced an anchor returning on an idle redraw after
`finish_session_change` cleared it. The final hover query had acquired a new point
from the old pointer position, even though no new pointer event arrived. Acquisition
and replacement now belong to `PointerMoved`; idle frames only validate an existing
ring. A press confirms a previously acquired ring, including a move earlier in the
same frame. Escape also clears a provisional anchor; release, lost focus and form
parking cannot reacquire one on an idle frame. The regression covers document change,
parking, Escape and focus loss, and fails before the repair. An old cancellation
fixture now releases the physical mouse button after Escape before starting its next
hover/press gesture.

After repair: 20 anchor tests passed in native and true-stub builds, and the native
Apply/Undo/Redo/unsaved-export/Save gate passed with its required marker. Workspace
clippy (all targets/features, warnings denied) and formatting passed. Each test run
used a separate watchdog log under `target/31h-review/`, cap 1536 MiB. Window and remote
results will be recorded separately; these checks do not establish a window pass.

The window review also found that the bundled font renders the edge-direction arrow
as a missing-glyph square. Edge labels now use ASCII `>`; their kind and ordering
remain unchanged. The event assertions check the displayed labels.

## Independent macOS window verification

Two fresh arm64 bundles were staged from the reviewed source and run separately
under `tools/watch-viewer-memory.py`, cap 1536 MiB. The first covered the saved
Sketch workflow at `43e33fb`; the second checked the label repair and New at
`4450f39`. Inputs and outputs are private copies in
`/private/tmp/ferrite-31h-review/run`; no repository fixture was edited.

Observed in the saved L sketch: Guides Off disables From object; Orthogonal makes
it available; vertex 3 is selected before anchoring at vertex 5. The ring remains
at 20 logical points and disappears beyond 24; Escape clears the provisional
ring. Length 15 from endpoint (20,40) moves vertex 3 to (35,40). One draft Undo
restores (60,20), Redo restores (35,40). `abc` remains visible and refuses placement;
a midpoint anchor at (10,40) instead gives (25,40). Save edited copy → Cancel and
switching to another tab and back preserve the draft, mode and Length. Apply,
document Undo/Redo, unsaved STL/FBX export and Save all completed; the file remained
byte-identical to the source until Save and the title became clean afterwards.

The CLI edited the same source with the same six curve UUIDs and coordinates.
All SQL cells matched the saved GUI file, including the stored IDs and references
(no UUID normalisation); validate and cold rebuild passed. Both unsaved GUI exports
matched the CLI byte-for-byte: STL 1084 bytes / 20 triangles, FBX 5092 bytes.
An independent binary STL reader checked every directed edge and measured bounds
(0,0,0)–(60,40,10), volume 17500 mm³. The original is 16000 mm³. Pinned ufbx 0.23.0
strict reported 6 checks, 0 failures. Source SHA-256 remained
`38e02d7327b650dd34944a8d30cca512901f07e172371b4a0fd62b45147210c6`.
Comparison script: `target/31h-review/compare-window.py`; output: the run folder's
`comparison.json`. The native Save panels retained the extension while the stem
was replaced, so the test exports are named `gui-unsaved.stl.stl` and
`gui-unsaved.fbx.fbx`; their contents, not their names, were compared.

On the final bundle a four-point New zigzag offered the actual finite intersection
with the readable label `Intersection · edges 1>2 × 3>4`. An eastward gesture added
exactly one fifth vertex with the intersection's Y coordinate (22.17795238095238);
one Undo removed it. The temporary New form was cancelled without publication.
The CUA interface has no hover-only primitive: right-button clicks placed the pointer
for the provisional-ring observations, without a primary gesture; frame-by-frame
hover, focus loss, same-frame events, occlusion and exact thresholds remain covered
by the real egui event tests. One early midpoint probe used coordinates from before
a form-height change; it was undone and repeated against a fresh screenshot. It is
not evidence for midpoint snapping.

Both window processes exited normally: PID 48735, peak sampled footprint 192.111 MiB,
exit 0; PID 56434, peak 186.298 MiB, exit 0. All samples had pressure 1 and swap 0.
No app observation was requested after Quit. Separate logs:
`target/31h-review/window.jsonl`, `window-final.jsonl`. One attempted widget run
while the first viewer was alive was correctly refused by the watchdog (125);
after Quit its unique-log repeat passed 20/20. This is not a test failure or a
window pass. The OOM cause remains unknown. Windows/Linux window smoke, live
Escape during a held CUA drag and a separate 1x display were not exercised here.

After the label repair, the wider native `sketch::` regression suite passed
141/141 with no skipped messages, under its own memory watchdog
(`target/31h-review/sketch-regression.*`).

## Remote verification of the reviewed code

The exact base `853a6bf95a11430a6becfdbfcd12aaee3d015975` subsequently completed
27/27 check runs successfully; this is separate from the new code.
For code `4450f3992f1b4cb455f87833bf33e970419a3149`,
[CI 38661495053](https://github.com/gesriot/ferrite-cad/actions/runs/38661495053)
completed 7/7 successfully and
[planegcs pin 38661491415](https://github.com/gesriot/ferrite-cad/actions/runs/38661491415)
completed 4/4. The downloaded CI log was independently checked for all 21 new
exact event/geometry/tab test names and execution markers on each OS (63 executions).
The review regression is included in the blocked/idle-canvas gate. Native runtime
results are recorded separately below.

[Runtime layout 38661491440](https://github.com/gesriot/ferrite-cad/actions/runs/38661491440)
completed 4/4 successfully (Linux, macOS, Windows and the cross-platform comparison).
Thus the exact code head passed 15/15 checks. Full per-job logs downloaded directly
from the GitHub API confirm the exact native anchor test and its
`volume=3072.65625 triangles=12` marker on all three OS, with no skip in that gate.
Each platform also executed `FCAD_SKETCH_ANCHOR_UFBX_EXECUTED`, preceded by
`FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0`. The combined `gh run --log`
view omitted some completed Linux/macOS steps; it was not used to infer missing
execution. Audit scripts and results are under `target/31h-review/`.
The later documentation commit changes only this evidence and the implementation
plan; its checks are attached separately to PR #111.
