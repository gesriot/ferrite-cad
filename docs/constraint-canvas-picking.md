# §31K — Select constraint targets in the accepted Sketch drawing

Status: implementation for independent review; the author ran no window, viewer, CUA or system
dialog. Plan entry: [§31K](implementation-plan.md). Base: `edbf6641149b` (merge of PR #114, tree
`b01c1638c4df`), branch `constraint-canvas-picking`, nothing staged or committed. Base CI: PR #114's head
`913c852` passed 15/15 checks and 3/3 workflows; the post-merge runs of `edbf664` (`CI` 38764653461,
`combined runtime layout` 38764653564, `planegcs pin`, `rust notices`, both SBOMs) were all `completed` /
`success` when read, at the start and again before the handoff. That is the base, not this diff: the CI of
this diff has not run.

## Problem

The **Sketch constraints** form chose its Lines, a Line's endpoint and the A/B pair from text lists of
UUIDs. The coordinates in those lists are the *stored approximation* the document keeps as the solver's
starting guess; once rules are persisted, the drawing the rebuild actually made is somewhere else. A
person could not see which Line a UUID was, nor what a pair or a pin referred to.

## Decision

A small **read-only canvas** in the same form shows the **accepted solved drawing** of the Sketch and a
click on it fills the fields the lists already fill. Nothing else changes: the same *Add …*, *Replace
length*, *Remove*, *Clear*, **Apply constraints** and **Save constraints copy…**, the same request, the
same shared job, the same CLI. No new constraint family, document field, schema or capability, no solver
or FFI change, no pointer API in the CLI, no dragging of geometry, no persistence of a view or a pick.

Covered: a **Line polygon** (including the profile of a Revolve and the bases under Fillet, Cut and
Chamfer histories) and **one analytic Circle**. Not covered, and stated as such in the form: two circles
(an annulus), arcs, points, mixed profiles, a Sketch the accepted rebuild has no drawing of.

## What is drawn, and where it comes from

The drawing is `SketchPresentation` — the curves, with their `StableEntityId`s, in the document's order,
at the coordinates **the rebuild built the profile from** (solved when the Sketch has constraints, the
stored ones when it has none) — produced by the same one rebuild as the picture
(`LoadedScene.sketch_presentations`, [analytic-sketch-canvas.md](analytic-sketch-canvas.md) and the
presentation module of `ferritecad-eval`). It is read through the type `ferritecad-scene` already
re-exports; the app's dev-dependency on `ferritecad-eval` is not promoted. Nothing is re-read from the
`.fcad`, no solver is asked, no arc or circle is rebuilt from a mesh.

Two states are told apart, in the form and on the canvas:

| Where | It is | Said as |
| --- | --- | --- |
| the canvas | the **accepted solved drawing** | *Canvas: solved drawing; pending rules not drawn.* under the header, *Accepted · solved* on the canvas |
| the lists (Segment, Circle, Stored length, Pin note) | the **stored inputs** | *Coordinates below are stored inputs, not the solved drawing.* (unchanged) |
| rules in *Pending additions* / *Remove* ticks | **not solved and not drawn** | the canvas does not move until **Apply constraints** is accepted and its scene arrives; never a live preview |

When the form has **no** accepted drawing — none was handed over, it is of another version, the rebuild
has none for this Sketch, its curves are not the stored ones, the profile is not a polygon or one circle,
or a coordinate cannot be drawn — the same line says *Accepted drawing unavailable: <why>.* (full sentence
on hover), there is no canvas, and every selector is exactly what it was. Stored geometry is never drawn
under the word *solved*.

## The boundary where drawing, catalogue and session revision agree

One reading and one rebuild produce `LoadedScene.edit_source` (the edit catalogue, whose `version` is the
`DocumentVersion` of that exact reading) and `LoadedScene.sketch_presentations`. The app keeps them
together through the arrival it already has:

* `prepare_arrival` turns the presentations into the GPU drawings as before and **also** builds
  `PreparedLoad.sketches`: an `Accepted` that holds the presentations **stamped with
  `edit_source.version`**. A load that fails (reading, conversion, upload) never reaches the commit.
* `present` binds the session change (`Tabs::bind`) and then `commit_scene`, in which
  `scene.edit_source` and `scene.sketches` are replaced in **one statement** with the picture. A tab
  shown again reloads its scene through the same route, so a hidden tab's drawing is never kept, only
  rebuilt for its version.
* A version that draws the same model (§30N: only the checkpoints differ) is accepted by
  `keep_picture` → `retarget_scene`, which keeps the picture, its GPU drawings **and these drawings**,
  and moves their stamp with `edit_source` in the same statement.
* A form is **handed** a drawing by `Editor::begin_with(path, source, &scene.sketches, sketch)` — the
  window's Edit-constraints button and the model-tree route both call it with both parts of the one
  scene — only if the stamp **equals `source.version`** (same document identity *and* content hash), the
  Sketch is in the rebuild's drawings, and its curves are the form's stored curves one for one in order
  and kind (`Drawing::of`). Otherwise `Drawing::Unavailable(why)`. Not the path, not the Sketch UUID alone.
* The **draft owns what it was handed**: the `Drawing` and the canvas state live in the constraints
  `Draft` beside its `version`, `choice` and picks, so a hidden tab, a form given back (Close, Quit,
  Open over New) and a published-but-not-yet-accepted draft carry their own drawing and cannot be paired
  with another one. A draft that returns **held** on a version that is no longer current
  (`Forms::restore`, `hold_stale_draft`) has its drawing retired (*another version*); its picks and
  lists are kept, Apply stays refused as before.
* Apply / Undo / Redo end every form about the replaced picture (`finish_session_change`) and the next
  arrival carries the next drawing; Save does not change the model. A failed or stale arrival leaves
  the scene, its catalogue and its drawings as they were (`commit_scene` returns before it assigns).

## What a click does

Four **explicit targets**, chosen with buttons (greyed where the form cannot use them); the click's
position never chooses between them:

| Target | A click on … | Fills (the same fields the lists and buttons fill) |
| --- | --- | --- |
| **Object** | a Line anywhere along it, or a Circle (its circle or its centre) | `selected` = that curve |
| **Endpoint** | a Line's end, or a Circle's centre | `selected` and, for a Line, the Pin end `Start` / `End` — the **stored** Line's Start/End, never the left or right of the screen; a Circle's centre selects the same circle |
| **Pair A** / **Pair B** | a Line or Circle (a single Circle has no pair: both are disabled) | `pair_a` / `pair_b` only; the selection and the directed angle's `vector_a_from` / `vector_b_from` are untouched, so choosing again never turns a vector round |

* The **pick radius** is the existing `PICK_POINTS` = 9 **logical** points (egui's unit, so 1x and 2x
  displays give the same pick; not physical pixels, not mm). The nearest candidate wins; an exact tie
  goes to the **curve stored first**, then Start before End, the circle before its centre. A Line's
  candidate is its distance to the whole segment; a Circle's are its ring and its centre (the Endpoint
  target takes only the centre). Where Lines meet at a joint the first stored Line takes an Endpoint
  click — to pin another Line's end choose that Line in the list and press Pin Start/End; the
  hover words name what a click would take before it is made.
* The hit must lie on the **visible, uncovered** part of the canvas. A zero-length Line, a non-positive
  or non-finite Circle and any non-finite number are never picked and mint no ID; a coordinate beyond
  the drawable bound makes the whole drawing unavailable.
* A **click** is a primary press and release within 6 points of each other (egui's own tolerance) with
  nothing between that cancels it: movement beyond it, Escape, a lost focus, a vanished pointer, a lost
  release. A Shift or middle press is a pan's and never a click, even when it does not move.
* It adds no constraint, writes no X/Y, length, radius or angle, does not touch the request history (no
  Undo spent), the refusal line or the typed text, takes no request and publishes no file.
* The highlight and the words agree with the lists: the selection in blue, A in orange and B in teal with
  their letters and an arrowhead for the way each Line runs as the directed angle's vector, the pin end
  with its `Start`/`End`, a Circle's centre ringed when the last pick was its centre; the top edge says
  `Selected: Line <12 hex>[ · Start|End]`, `Selected: Circle … · circle|centre`, or in a pair target
  `A: Line … start>end · B: Line … start>end`. A pick moves the Segment row into view.

## View, ownership and where it acts

Zoom about the cursor (wheel, two-finger scroll, pinch), pan (middle button, or Shift + primary), and
**Fit** reuse §31J's bounded view arithmetic and `Navigation` unchanged ([sketch-canvas-navigation.md](
sketch-canvas-navigation.md)): scale in [1e-5, 1e8] points/mm, at most 8x an event, nothing non-finite
changes the view, an event is read against the view the person saw and a wheel before a press that is still
held at the end of the frame is dropped. The view, the held pan, the held press, the target and the circle
part belong to the **form** (`Draft`), so to its tab; nothing is saved, nothing is in request history,
nothing reaches the file. A pan is a held gesture of the window (`Editor::gesturing()`: tab switch, Open,
New, Close and Quit refuse as for a vertex drag) and a canvas that stops being drawn ends it. Only a form
that is enabled, focused and has no request waiting takes a click, a wheel or a pan, and only on the
visible uncovered canvas; busy, queued, covered, outside, unfocused and (for a held draft) stale do
nothing. A floating form still claims wheel, pinch and presses from the 3D camera
(`window_pointer_claimed`) and a 3D drag begun in the viewport still crosses it.

## Layout and the 768-point budget

The canvas is a fixed **268 × 112 logical points** plus one row of buttons, and stands **beside** the
Lines list (and the Circle row) in the height that list already took; with no drawing the form is laid out
as before, row for row. The status row costs no height (`interact_size.y = 0` in that one row; the first
attempt, a plain `ui.horizontal`, cost 4 points and failed the old reachability gate by 2.5 points). The
Lines column wraps its rows and scrolls inside its 150 points as ever. Measured bottoms of *Save
constraints copy…* on the 988 × 768 gate screen, without / with the canvas:

| Form | Without | With |
| --- | --- | --- |
| plate (4 Lines) | 517.5 | 569.5 |
| plate with H, V and both lengths persisted, pending rules, a refusal | 665.5 | 717.5 |
| 64-gon | 635.5 | 635.5 |
| 64-gon with every closure link, a stored length, 32 pending rules (the old gate's worst case) | 767.5 | 767.5 |
| six-segment shaft closed on the axis (owner text) | 625.5 | 635.5 |
| five-segment sector | 589.5 | 620.5 |
| single Circle | 358.5 | 470.5 |

(`constraints::tests::canvas::the_form_keeps_its_canvas_its_modes_and_its_actions_reachable_at_768_points`
also requires every button, the canvas itself and the Apply/Cancel/Undo/Redo/Clear row to be whole inside
the screen, with and without the *unsaved* note and a refusal line.) A form that already filled the screen
before this slice with a long Fillet history, many persisted rules and many pending ones can still overflow
exactly as it did; the canvas adds none to a full Lines list, 52 points to a four-Line plate, and 112 to
the (short) single-Circle form, which ends at 470 of its 768 points.

## Executed gates (author, local; all sequential, `CARGO_BUILD_JOBS=1`, existing targets, pinned libraries)

Every test executable ran under `tools/watch-viewer-memory.py --limit-mib 1536` with a finite time limit
and its own log (`target/constraint-canvas-check/guard/`); pressure 1 in every sample; no viewer, window,
CUA or GPU test was started.

| Check | Result |
| --- | --- |
| New tests | 32: 10 pure (`sketch::accepted_tests`), 3 scene (`accepted_scene_tests`, real rebuilds through the mock kernel), 14 form (`constraints::tests::canvas`), 3 tab (`sessions::tests::tabs::drafts::canvas_form`) = 30 kernel-free, and 2 native (`sessions::tests::native_rules_chosen_on_the_canvas_of_a_constrained_rectangle_…`, `native_a_radius_and_a_fixed_centre_chosen_on_the_canvas_of_a_solved_circle_…`). The tests were written with the code, not before it: the failing-first evidence is the 21 compiling mutants below and the defects the gates themselves found (see *Found while building*). |
| True stub, the real new `ci.yml` step body (only its `cargo test` line replaced by the same four filters on the built executable under the watchdog) | exit 0; `30 passed; 0 failed`; every name and all 30 `FCAD_31K_*` exact-line markers |
| True stub, whole viewer suite (`--nocapture`) | 820 passed, 0 failed, 4 ignored (31J: 788; +32); exit 0; 30 `FCAD_31K_*` markers; 219 skip lines (31J: 217): the two added are the two native gates, which skip in a stub and are **not** geometry passes there; no `FCAD_31K_NATIVE_*` marker in that log |
| Stub proof | bridge `CMakeCache.txt` of the build (`…/ferritecad-occt-bd6e1e14777ad9eb/out/bridge-build`): `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE:UNINITIALIZED=TRUE`, `OpenCASCADE_DIR:UNINITIALIZED=/private/tmp/ferrite-launch-journal-review/absent-occt`; build warning "Open CASCADE was not usable"; `otool -L` of the fresh test executable **and** of a fresh stub peer CLI (`/private/tmp/ferrite-25j-stub-target/debug/ferritecad`): 0 × libTK, 0 × planegcs |
| Native (OCCT + PlaneGCS, release), whole viewer suite (`--nocapture`) | the fresh test executable and the peer CLI each import 50 libTK and libplanegcs; 820 passed, 0 failed, 4 ignored; exit 0; 34 `FCAD_31K_*` lines (30 + the two native `…_REQUEST` and two `…_EXECUTED` ones); 23 skip lines (the count §31J reported; none of this slice's gates); peak footprint 274.8 MiB |
| Native scenario, rectangle | the Sketch stores the 12 × 8 mm parallelogram `(-6,-3) (6,-2) (7,5) (-5,4)` and the rules Line 1 horizontal and 60, Line 2 vertical and 30, Line 3 horizontal. The accepted `SketchPresentation` reads `(-29.689,-14.088) (30.311,-14.088) (30.311,15.912) (-5,15.912)`: Line 1 is horizontal and 60 mm, Line 2 vertical and 30 mm, Line 3 horizontal; the stored guess is 27.151 mm from it at the farthest end; the form's drawing equals the presentation curve for curve (UUID and numbers); each stored vertex and Line middle is on the canvas and near **nothing** of the solved drawing in Object, Pair and Endpoint targets, and a click there selects nothing. Rules chosen only by clicks — Pair A = Line 4, Pair B = Line 1 → Perpendicular; Line 1 selected → Replace length 70; Endpoint at Line 2's End → Pin End at (80, 45) — equal the lists' own request (`EditSketchConstraintsRequest` edits, version, Sketch, source); then, through the real worker: Apply, export before Save, Undo, Redo, Save and a cold rebuild with every saved reference resolving, each against the peer CLI given the same UUIDs; SQL cells equal after masking only the three freshly minted constraint UUIDs (`same_model_with_explicit_new_ids`); STL/FBX byte-identical. The saved Sketch's own rebuild presents `(10,15) (80,15) (80,45) (10,45)`, and the saved STL, read by `read_stl` (not the writer), has volume 70·30·9.5 within 1e-4 and bounds `(10,15,0)..(80,45,9.5)` |
| Native scenario, circle | the Sketch stores the circle (4.5, 0) r 1.2 and keeps Radius 10 and Fixed centre (0, 0); the accepted presentation is (0, 0) r 10; stored ring and centre are on the canvas and pick nothing. The ring (`Part::Ring`) and the centre (`Part::Centre`, Endpoint target) are the **same** curve UUID; the persisted Radius and Fixed centre are ticked **Remove** and Radius 8.5 / Fixed centre (2.5, -1.25) added; the request equals the lists'; Apply / export before Save / Undo / Redo / Save equal the CLI as above; the saved rebuild presents (2.5, -1.25) r 8.5 and every rim vertex of the saved STL is on it within 1e-3, the mesh inscribed (volume ≤ πr²h, ≥ 97 %) |
| Real `runtime-layout.yml` step (body cut by indentation, `matrix.name` = macos, `$PWD/vendor` made absolute, only its `cargo test` lines replaced by the built release executable under the watchdog) | exit 0: both gates `1 passed; 0 failed` with their exact markers; the four exports (`constraint-canvas-rectangle`, `-circle` × `.stl`, `.fbx`) written |
| Pinned reader (the real `FCAD_ANALYTIC_SESSION_FBX_DIR` hook block of `tools/check-fbx-complex.sh`, cut by its own `if`/`fi`, loop narrowed to the two new names, ufbx 0.23.0 built from the cached digests) | `checks=6 failures=0` for both; `FCAD_STL_FBX_MATCH triangles=12` and `triangles=364`; a copy of the FBX truncated to 2000 bytes is refused (`checks=1 failures=2`) |
| `occt-pin.yml` marker lines | the five new `grep -Fxq` lines pass on the native whole-suite log |
| `planegcs-pin.yml` count filters | still exactly 39 and 4 tests on the native executable |
| Mutations (`target/constraint-canvas-check/mutate.py`; 21 mutants, 24 gates; every one compiled and failed with `test <gate> ... FAILED` and a `panicked at` line of the gate named; restored byte for byte, sha256 identical; the five mutated files identical to before the run) | the **two directed ones**: (1) stored numbers instead of the accepted solved drawing — in `Accepted::drawing`, failed by both native gates; and one step later in `Drawing::of`, failed by `a_drawing_is_shown_only_for_the_curves_the_form_stores_in_the_order_it_stores_them`; (2) the neighbouring curve's UUID — failed by `the_nearest_part_within_nine_logical_points_wins_and_the_boundary_is_exact` and `a_click_fills_the_fields_the_lists_fill_for_each_explicit_target_and_only_them`. And one gate each for: pick radius 10 points; a tie to the later curve; an end named by the screen side (`an_end_is_start_or_end_by_the_stored_line_and_never_by_the_side_of_the_screen` and `a_reversed_line_is_picked_by_its_start_and_end_not_by_the_screen_side`); a hit off the visible canvas; a zero Line pickable; a Pair click also selecting; an End click not setting the pin end; a drag that comes back being a click; a queued form taking clicks; a covered canvas taking clicks; a held pan not holding the window; a pan on a vanished canvas holding it; a held form keeping a drawing of another version; a kept picture leaving its drawing behind; the scene not committing its drawing; a form handed another version's drawing; a missing drawing not stated; the picked row not brought into view. The positive repeat (30-test group by the real `ci.yml` step; stub and native whole suites; real runtime-layout step) followed on the restored tree: all green as above |
| fmt, `clippy --workspace --all-targets --all-features -D warnings` | pass (true-stub environment); `cargo fmt --all -- --check` clean |
| actionlint (all workflows), shellcheck `tools/check-fbx-complex.sh`, `tools/check-actions-run-size.py` | pass; 216 blocks, largest 20 432 of 21 000 (the new `ci.yml` and `runtime-layout.yml` steps are their own small blocks; nothing was added to a large one) |
| Licence headers (511), export boundary, solver ownership, notice ownership, PlaneGCS pins, STEP corpus, `git diff --check` | pass; no manifest, lock or deny change |
| Window recipe inputs and comparator | `make_inputs.py` wrote six inputs (hashes in `SHA256.txt` beside them); `compare.py` was exercised on command-line stand-ins (not window outputs) for a rectangle and a cylinder: `VERDICT: PASS`, and with `--negative` `NEGATIVE CONTROL HELD` |
| Mixed (OCCT without solver) | **N/A, reasoned:** no kernel, solver, ABI or job boundary is touched (`check-solver-ownership` passes); the two native gates assert `FERRITECAD_REQUIRE_PLANEGCS` separately and skip without a solver; not run |
| Resources | every test executable ran under `tools/watch-viewer-memory.py --limit-mib 1536` with a finite time limit and its own log: 113 logs, peak footprint 274.8 MiB, pressure 1 in all 781 samples, swap 0, none aborted; the non-zero exits are the intended failures of mutants and of the author's own iterations (and one mutant run that aborted on a missing DYLD path, see below). No viewer, window, CUA or GPU test was started; the author's own processes were gone at the end |

### Found while building, and changed

* **The status row cost height.** A plain `ui.horizontal` for the two statements (stored inputs / canvas
  or unavailable) is 4 points taller than the label it replaced; the old reachability gate, with
  0.5 points to spare (767.5), failed by 2.5. The row now has `interact_size.y = 0`.
* **A `#[cfg(test)] mod` near the top of `main.rs` broke `tools/check-export-boundary.sh`** (its
  `shipped` view of `main.rs` ends at the first `#[cfg(test)]`, so four statements about the accepted
  document were "missing"). The test module is declared at the end of the file, like `mod tests`.
* **The first native fixture proved nothing about stored-vs-solved.** A guess stored near its solved
  place has Line middles ON the solved Lines (the solver flattens a segment about its midpoint and keeps
  the translation near the guess); the gate's own assertion that every stored middle is far from the
  solved drawing failed on Line 2. The native fixtures now store a small shape about the origin and solve
  it to a large one around the same place, and a circle is stored inside its solved ring.
* **The first form test of a reversed Line aimed at a vertical Line**, for which "name the end by the
  screen side" is the same as the stored name: the mutant survived. It now aims at a horizontal Line
  stored right to left.
* **Endpoint at a joint.** Lines that meet tie exactly and the first stored takes the click, so a
  contour 0 → 1 → 2 → 3 gives Line 0's Start and the later Lines' Ends. This is the contract, stated
  above and in the plan; the way to pin another Line's end is its list row and Pin Start/End.
* `scroll_to_me` animates and never completes without a clock (`scroll_to_me_animation` with
  `ScrollAnimation::none()` is used); a window that is still fading in paints strokes at ~0.86 of their
  colour (the tests compare colours at the strength drawn); a disabled button is told by its text colour,
  not its `opacity_factor`.
* The mutation harness first captured the native environment through `/usr/bin/env`, which drops
  `DYLD_*` (exit 134 on the first native mutant); it now captures it with `python3`.
* `Editor::begin` (no drawing handed) is test-only now: both production routes call `begin_with`.

### Design choices flagged for review

The canvas is a fixed 268 × 112 points beside the Lines list (it can be small for a big drawing; zoom,
pan and Fit exist); the four targets are buttons, never inferred from the click; the first stored Line
wins at a joint and nothing cycles; Pair A/B are disabled for a single Circle; a pair pick never touches
the vector directions or the selection; a pick scrolls the selected Segment row into view; the pick
radius, tie rule and `PICK_POINTS` are the existing ones; the drawing of a held draft is retired rather
than kept as "the drawing of an older version"; an annulus keeps the selectors only, with the word
*not for this profile*; the status words are the ones in the table above.


## Not done, not claimed

A window, the viewer, the CUA, Windows or Linux, the CI of this diff. No window pass is claimed: the
reproducible scenario and inputs are in the reviewer recipe below. Also open: arcs, mixed profiles, the
annulus on this canvas, new constraint families, dragging geometry, cycling through coincident joints,
a pointer command in the CLI, the §31J native-window pan/pinch and held-input cancellation observations,
§30, Milestone 5C, the product and the OOM investigation.

## Reviewer recipe

`/private/tmp/ferrite-31k-window/inputs` (made by `make_inputs.py`; copies of the scripts, the recipe and
the comparator are kept in the ignored `target/constraint-canvas-check/window-recipe/`): `SHA256.txt`
there lists the six inputs; no expected output or screenshot exists. The recipe asks the reviewer to
observe, in a real window at 2x (and 1x if available): the two states and the unavailable sentence; clicks
on stored positions taking nothing; each target and its words; pair letters/arrowheads and vectors that
do not turn; typed text and request history untouched; pending rules not drawn until Apply; Apply,
export before Save, Undo/Redo and Save compared by `compare.py` (every SQL cell but the sketch row,
fresh constraint UUIDs masked once and nothing else, byte-identical STL/FBX, an independent STL reader,
a negative control); zoom, pan, Fit and held-pan refusal; two tabs; the Circle's ring and centre; the
scrolling Lines list; the places it must not act. The author checked `compare.py` and its negative control
on command-line stand-ins for both kinds, not on window outputs.

## Independent review

The review found an input-ownership defect: a primary press could select a Line when released after
focus had been lost, after another layer covered the canvas, or just outside the canvas within the
six-point click tolerance. The existing blocked-input gate was extended with those transitions at 1x
and 2x, including cover → uncover before release. Before the fix that one executed test failed on
`a release after losing focus=true selected` (exit 101). The canvas now cancels the held press as soon
as its surface stops accepting input, and checks the release position's visibility and ownership too.
Picking still changes neither request history nor typed fields.

On the fixed code the independent native whole-viewer run passed 820 tests, with 4 existing ignored
benchmarks and all 32 new gate markers. Its 23 conditional skip lines belong to earlier configuration
cases, not these native gates. The watchdog recorded exit 0, peak 274.110 MiB, pressure 1 and swap 0.
The true-stub repeat and native-window scenario are pending at publication; their results will be
recorded in the associated pull request. The author's headless results above are not a window pass.
