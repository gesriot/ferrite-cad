# §31L — Dimension labels and explicit replacement on the accepted Sketch canvas

Status: implementation awaiting independent review. Plan entry: [§31L](implementation-plan.md). Base:
`3c853e6c38d08514bbbd0e39e511b4f49c389faf` (merge of PR #115, `main` = `origin/main`, tree
`790c50f1209139af4fd5ec52340d63dd812f67a6`). Branch: `sketch-dimension-labels`. At the author's handoff
nothing is staged or committed, and the author ran no window, viewer, CUA or system dialog.
The base is the reviewed §31K code (`0529b58`, 15/15 checks and 3/3 workflows) plus its docs-only commit
(`ed3b673`, 7/7 checks); the merge has the tree of `ed3b673`. At the start of this work the post-merge
`CI`, `planegcs pin`, `rust notices` and both SBOM runs of `3c853e6` were `completed` / `success`, and the
`combined runtime layout` run was still `in_progress`; it was not counted as a success then and, its tree and
workflows being those of the checked code, did not block this work (at the handoff it reads `completed` /
`success`). The CI of the unstaged diff has not run.

## Problem

The accepted canvas of §31K shows which Line or Circle the **Sketch constraints** form picks, but a person
could not see which *saved size* controls it. A saved whole-Line length or a Circle's radius was a row of
text in the **Persisted constraints** list, and replacing it meant ticking *Remove* on that row and adding a
new number — or typing into the length box and pressing *Replace length*, a Line-only route.

## Decision

The accepted solved drawing carries a **compact label for each saved dimension** of the form's Sketch: the
number the solver is asked to meet for a whole Line (its Distance between Start and End) or for a single
Circle (its Radius). A fifth explicit canvas target, **Dimension**, selects a label; the one **Replace
dimension** action prepares a replacement as one atomic remove/add in the draft. Nothing changes the model
except the existing **Apply constraints** / **Save constraints copy…**, which send the existing request
through the existing job and CLI. No new constraint family, document field, schema, capability, FFI, solver
equation, job, CLI request, stored position of a label, placement engine, dragging or pointer command.

Covered: a Line polygon (including the Revolve, Fillet, Cut and Chamfer bases §31K covers) and one analytic
Circle. A diameter is only another way to **type** that Radius (`R = D / 2`, §31E). Not covered, and left as
they were: DirectedAngle and every other relation, arcs, an annulus ("not for this profile"), mixed profiles,
a form with no accepted drawing (it keeps its lists, rows and editor, without labels).

## Where a label comes from, and what it says

A form is opened (`Editor::begin_with`) with its catalogue (`choice.stored`, from the edit facts) and the
accepted drawing **of the same version** (§31K's stamp check). The form reads its dimensions **once, from that
catalogue** (`dimension::saved`): in the order of the curves, the whole-Line Distance (Start and End of one
Line, in either stored order) of each Line with a valid length, or the Radius of the one Circle with a valid
radius. The draft owns that list as it owns the drawing, so a hidden tab, a form given back and a held draft
carry their own, and nothing is looked up again from the file, the solver or the mesh.

* The **full UUID of the saved rule** is the label's identity (`Part::Label(rule)`); the curve and the kind
  are references to it. Selecting a label sets the form's selection to the curve **only if this form's own
  reading holds that rule on that curve**; a rule the form does not hold selects nothing.
* A label stands where the **solved** curve is: beside the middle of a Line (on its upper side, to the right
  of a vertical Line, whichever way the Line is stored) or outside a Circle's ring toward the upper right, a
  gap of three points from it. The box is then slid into the band of the canvas that the words on its edges
  leave. A label whose anchor is not on the canvas is absent (not clamped onto an edge): the list is the way
  to it. Nothing is stored about a position and there is no search for a free place.

| Where | It is | Said as |
| --- | --- | --- |
| a label | the **target** the saved rule holds (neither the length of the stored starting coordinates nor a measured size of the drawing) | `60 mm`, `R 10 mm` (the first nine characters of the exact number, else four decimals and `…`, else an exponent) |
| the lists (Segment, Circle) | the **stored starting coordinates** | unchanged: *Coordinates below are stored inputs, not the solved drawing.* |
| a label with a replacement prepared | the target, then what is prepared in its place; the drawing and the accepted size are unchanged until Apply | `60 → 70 mm` in amber; `60 mm → removed` if the rule is only ticked *Remove* |

There is no preview: the strokes never move before the next accepted scene, and nothing is drawn for a
prepared number.

## The Dimension target

Four targets of §31K are unchanged; **Dimension** is a fifth button (greyed when the form has no label).
Labels are drawn under every target but they are **pictures** except under Dimension: under Object a click
on a label's place takes the Line within nine points as ever, and a label stands farther from its Line than
that. Under Dimension only labels are picked.

* The **hit area is the drawn box**: its edges are in, a hair outside is not, and the nine-point radius of the
  curves plays no part. The point must be on the visible canvas and not covered (`pointer_over`).
* **Overlaps** are resolved by the order of the curves in the drawing, never by the UUIDs or a hash: the
  label of the curve stored first is drawn on top and is the one a point in the overlap reaches. The
  persisted rows are the fallback that reaches either.
* The click rules of §31K are unchanged and include the review fix: a press is cancelled the moment its
  surface stops accepting input (focus, a cover, a busy or waiting form), restoring availability before the
  release does not revive it, a release outside the canvas does not select even within the click distance,
  and a Shift or middle press is a pan's.
* Zoom, pan and **Fit** are §31J's arithmetic, unchanged: labels are placed from the view the person sees
  (the view of the start of the frame for an event), so a label moves with its curve, and Fit gives back the
  opening place exactly.

## One selection

There is no second "selected dimension". The form's one selection (the curve) is set by a label, by the
Lines list, by the canvas under any target and by a click on a **dimension row of the Persisted constraints
list** (now a selectable row: `Line length 60 mm · <UUID>`, `Radius 10 mm · <UUID>`; the *Remove* tick is
unchanged). The dimension of the selected curve is the dimension of the selection: at most one per curve
(a Line has one length slot, a Circle one radius slot), so the label, the row and the editor cannot disagree.
Choosing a label publishes nothing, adds nothing, spends no Undo and leaves every typed text alone. The row
and its editor are brought into view one frame after the selection moves (the list cannot scroll past a row it
has not yet measured).

## Replace dimension

Under the selected dimension's row — whose text already says the UUID and the accepted target — the editor is
one line: `New length [number] mm [Replace dimension]` for a Line; `Radius | Diameter` toggles,
`[number] mm [Replace dimension]` for a Circle; then a sentence saying what the typed number means and what is
pending (*Pending: 70 mm in place of 60 mm; the drawing keeps the accepted size until Apply constraints.*).

| Case | What happens |
| --- | --- |
| a number different from the saved one | the saved rule is ticked removed and **one** addition of the same family on the same curve takes its place: one `History::change`, one Undo step |
| another number for the same dimension | **that** addition is updated where it stands (still one pair); the draft changes by one step |
| the number already pending | nothing (no step) |
| the saved number | only this replacement is taken back: the saved rule is no longer removed (its UUID is kept), and every other pending change stays; an empty draft offers nothing to Apply or Save |
| an invalid number, or a draft the document's validator refuses | the refusal line, **the exact text typed, the selection and the history unchanged** |
| a running job, or a request of the form waiting to be taken | the box and the button take nothing (the box is not interactive, so a focused box stops taking text) |

* The Line semantics are those of **Replace length**: one function, `replace_stored`, now serves it and the
  Circle's radius (the part that really is common: remove + in-place addition, or the take-back); the old
  button and its tests are untouched and give the same request for the same number.
* A Circle's number is parsed as the Radius it is: `CircleRadiusMm::new` for **Radius**,
  `CircleRadiusMm::from_diameter_mm` (`D / 2`, refusing a `D` that does not halve exactly) for **Diameter**;
  the typed text is never converted when the toggle changes, and a sentence shows the converted value. "The
  saved number" is the **exact** equality of the validated types (`LineLengthMm`, `CircleRadiusMm`) — no
  tolerance: the next representable number is a replacement.
* Stored centre, radius and Line coordinates are never touched to satisfy a size.
* The prepared draft is the form's: it goes with its tab, Close/Quit give it back, a draft held on another
  version loses its drawing (so its labels) and keeps its rows, its text and its request (Apply stays refused).

## Layout and the 768-point budget

The label work is drawn on the canvas (height 0). The editor lives **inside the 150-point persisted-rules
list**, so it adds no height wherever that list is full. Measured on the 988 × 768 gate screen (Save
constraints copy… bottom, with a dimension selected / with a long text in the box, a prepared replacement, a
refusal and the unsaved note):

| Form | Bottom |
| --- | --- |
| plate holding two lengths | 681.5 / 714.5 |
| 64-gon with every closure link, a stored length and 32 pending rules (no refusal line, as §31K's) | 698.5 / **767.5** (the §31K figure) |
| single Circle with a Radius | 505.5 / 538.5 |

The widest text of the forms ends at 671.1 points (the Dimension button does not widen the canvas column
enough to move it); every button, the editor, the labels and the canvas are whole inside the screen and their
clips (`constraints::tests::dimension::the_labels_the_editor_and_the_actions_stay_reachable_at_768_points_in_the_worst_forms`).
§31K's own gate gives the same bottoms as before (517.5, 569.5, 665.5, 717.5, 635.5, 767.5, 620.5, 470.5).

## Executed gates (author, local)

Every test executable ran under `tools/watch-viewer-memory.py --limit-mib 1536` with a finite time limit and
its own log (`target/dimension-labels-check/guard/`, the superseded runs in `prev1/`, `prev2/`); `CARGO_BUILD_JOBS=1`,
the existing native and true-stub targets, pinned libraries, sequential builds. No viewer, window, CUA or
GPU test was started. The frozen tree of the final run is `039899beedece850` (sha256 of the sha256s of every
modified or untracked `.rs`/`.yml`/`.sh`/`.py`); it was identical after the mutations.

| Check | Result |
| --- | --- |
| New tests | 24: 5 pure (`sketch::dimension_tests`), 14 form (`constraints::tests::dimension`, real egui events at 1x and 2x), 3 tab (`sessions::tests::tabs::drafts::dimension_form`) = **22 kernel-free**, and 2 native (`sessions::tests::native_a_saved_length_…`, `native_a_saved_radius_…`). Written with the code, not before it: the failing-first evidence is the 18 compiling mutants below and the three gaps the mutation harness found in my own gates |
| True stub, the real new `ci.yml` step body (only its `cargo test` line replaced by the same three filters on the built executable under the watchdog) | exit 0; `22 passed; 0 failed`; every name and all 22 `FCAD_31L_*` exact-line markers; repeated on the restored tree after the mutations: the same |
| True stub, whole viewer suite (`--nocapture`) | 844 passed, 0 failed, 4 ignored (§31K: 820); 22 `FCAD_31L_*` and the 30 `FCAD_31K_*` markers; 221 skip lines (§31K: 219): the two added are the two native gates, which skip in a stub and are **not** geometry passes there; peak 162.1 MiB; repeated: 844/0/4 |
| Stub proof | bridge `CMakeCache.txt`: `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE:UNINITIALIZED=TRUE`, `OpenCASCADE_DIR:UNINITIALIZED=…/absent-occt`; build warning "Open CASCADE was not usable"; `otool -L` of the fresh test executable **and** of a fresh stub peer CLI: 0 × libTK, 0 × planegcs |
| Native (OCCT + PlaneGCS, release), whole viewer suite | the fresh test executable and the peer CLI each import 50 libTK and libplanegcs; 844 passed, 0 failed, 4 ignored; 26 `FCAD_31L_*` lines (the 22 + a `…_REQUEST` and an `…_EXECUTED` per native gate) and 34 `FCAD_31K_*`; 23 skip lines (§31K's count; none of this slice's gates); peak 270.2 MiB; repeated: 844/0/4, peak 275.7 MiB |
| Native scenario, rectangle (`write_dimensioned_rectangle`) | the Sketch stores the parallelogram `(-6,-3) (6,-2) (7,5) (-5,4)` (Line lengths 12.042 and 7.071 mm) and the rules Lines 1 and 3 horizontal, Lines 2 and 4 vertical, Line 1 60, Line 2 30. The accepted `SketchPresentation` is the 60 × 30 mm rectangle, and the form's drawing equals it curve for curve (UUID and numbers). The form holds exactly two labels, `60 mm` and `30 mm` (none says 12 or 7), each within 5 points of its **solved** Line and placed at that Line's middle. A click on `60 mm`'s place under Object takes nothing; under **Dimension** it selects Line 1; `70` typed and **Replace dimension** pressed give one removal (the 60 mm rule's UUID) and one Distance addition, the label reads `60 → 70 mm`, the drawing is exactly the rebuild's; the request equals the one the Lines list and Replace length make (`EditSketchConstraintsRequest` edits, version, Sketch, source). Then through the real worker: **Apply** into an unsaved session after a height step, export before Save, Undo, Redo, Save and a cold rebuild with every saved reference resolving, each against the peer CLI given the same request: every SQL cell equal after the one mapping of the freshly minted constraint UUID (`same_model_with_explicit_new_ids`: raw payload hash validated, UUID bytes mapped once, all cells compared, the Sketch row included), STL/FBX byte-identical. The cold rebuild's drawing is a closed 70 × 30 rectangle with Line 1 horizontal and Line 2 vertical; its form holds `70 mm` under a new UUID and `30 mm` under the UUID it had; the saved STL, read by `read_stl` (not the writer), has volume 70·30·9.5 within 1e-4 and extents 70 × 30 × 9.5 |
| Native scenario, circle (`write_guessed_circle`) | the Sketch stores the circle (4.5, 0) r 1.2 and keeps Radius 10 and Fixed centre (0, 0); the accepted presentation is (0, 0) r 10; the one label is `R 10 mm`, just outside the **solved** ring. Under Dimension the label selects the circle; **Diameter** `17` and Replace give one removal (the Radius rule) and `Radius(8.5)`, equal to the request made by Remove + Add diameter; the Fixed centre is untouched. Same Apply / export / Undo / Redo / Save / cold reopen against the CLI; the cold form holds `R 8.5 mm` under a new UUID and the Fixed centre under its own; the saved presentation is (0, 0) r 8.5 and every rim vertex of the STL is on it within 1e-3, the mesh inscribed (≥ 97 % of πr²h) |
| Real `runtime-layout.yml` step (body cut by indentation, `matrix.name` = macos, `$PWD/vendor` made absolute, only its `cargo test` lines replaced by the built release executable under the watchdog) | exit 0, both gates `1 passed; 0 failed` with their exact markers; the four exports (`dimension-labels-rectangle`, `-circle` × `.stl`, `.fbx`) written; repeated after the mutations: the same |
| Pinned reader (the real `FCAD_ANALYTIC_SESSION_FBX_DIR` hook block of `tools/check-fbx-complex.sh`, cut by its own `if`/`fi`, loop narrowed to the two new names, ufbx 0.23.0 from the cached digests) | `checks=6 failures=0` for both; `FCAD_STL_FBX_MATCH triangles=12` and `triangles=364` |
| `occt-pin.yml` marker lines | the five new `grep -Fxq` lines pass on the native whole-suite log |
| Mutations (`target/dimension-labels-check/mutate.py`; 18 mutants; every one compiled and failed with `test <gate> ... FAILED` and a `panicked at` line of the gate named; restored byte for byte, sha256 identical; the four mutated files identical to before the run) | the **directed** ones: (1) **another constraint's UUID** carried by a dimension — failed by the label gate and by the native rectangle gate; (1b) the replacement made on the **neighbouring Line** — failed by the replace gate and the native rectangle gate; (2) **broken replace/revert**: going back drops every other pending change (replace gate); going back leaves the saved rule removed, so its UUID is lost (replace, semantics and circle gates); replacing again appends a second pair (replace and circle gates). And one gate each for: a near-equal number taking the replacement back; a diameter taken as a radius (circle gate and native circle gate); the hit area padded by two points; the last of two overlapping labels winning; a label leaving the band of the canvas; a label placed by the way its Line is stored; the pending number replacing the target in the label; labels taking clicks under every target; a persisted row not selecting; the editor not brought into view; the box taking text while a request waits; a release outside the canvas taking a label; a held form keeping the labels of another version |
| fmt, `clippy --workspace --all-targets --all-features -D warnings` | pass (true-stub environment) |
| actionlint (all workflows), shellcheck `tools/check-fbx-complex.sh`, `tools/check-actions-run-size.py` | pass; 218 blocks, largest 20 432 of 21 000, packed argv at most 20 463 (the new `ci.yml` and `runtime-layout.yml` steps are their own small blocks; nothing was added to a large one) |
| Licence headers (516), export boundary, solver ownership, notice ownership, PlaneGCS pins, STEP corpus, `git diff --check` | pass; no manifest, lock or deny change |
| Window recipe inputs and comparator | `make_inputs.py` wrote seven inputs (hashes in `SHA256.txt` beside them); `compare.py` was exercised on command-line stand-ins (not window outputs) for a rectangle and a cylinder: `VERDICT: PASS` (rectangle extents 70 × 30 mm, volume 21000; cylinder r 8.5) and, with `--negative`, `NEGATIVE CONTROL HELD` |
| Mixed (OCCT without solver) | **N/A, reasoned:** no kernel, solver, ABI, schema, job or CLI boundary is touched (`check-solver-ownership` and `check-export-boundary` pass; the diff is app-crate UI code, tests, workflow lines and docs); the two native gates assert `FERRITECAD_REQUIRE_PLANEGCS` separately and skip without a solver; not run |
| Resources | 133 watchdog logs: peak footprint 282.5 MiB, pressure 1 in all 1 188 samples, swap 0; every log of the final runs exited 0 and none aborted (the non-zero exits are the intended failures of mutants and of the author's own iterations). One other session's `cargo run --release` was compiling during part of the work; it was left alone and did not move the pressure. No viewer, window, CUA or GPU test was started |

### Found while building, and changed

* **The editor could not be scrolled into view in the frame its row appeared**: egui clamps a scroll target to the
  content it measured the frame before, and moved the list 4 points of 22. The scroll is requested one frame after
  the selection moves (a flag in the draft); tests settle four frames after a selection.
* **The old count gate was a substring filter.** `ci.yml`'s §31K step requires `30 passed` for
  `sessions::tests::tabs::drafts::canvas_form` and `constraints::tests::canvas`; tests added to those modules would have
  broken it, so §31L's live in modules of their own and in a step of their own.
* **The true stub cannot rebuild a document that holds a persisted rule** (no solver). The tab gates there use a
  named synthetic hook (`Editor::persist_lengths_for_tests`) to give the form's catalogue the rules a persisted Line
  set holds; the real documents are in the native gates.
* **The mutation harness found three gaps, two of them in my own gates.** (a) A mutant that reverts to an empty
  draft survived the semantics gate, which only reverts a lone replacement: its gate list said more than the gate
  does, and now names the replace gate only. (b) A mutant that made every target pick labels survived the label gate,
  because the form itself ignores a label hit under another target; the gate now also requires that under Object,
  Endpoint and Pair a click on a Line that has a label takes the Line. (c) A mutant that never brought the editor into
  view survived the sync gate; a gate of its own (a 64-gon) now requires the row and the editor whole inside the
  clip after a label is chosen, with nobody scrolling.
* **The fully loaded 64-gon has no room for a refusal line** (767.5 of 768 points, as in §31K), so its worst case
  here carries a long text and the unsaved note but no refusal; every other form is tested with all of them.
* **`Apply constraints` exists only while the draft validates**, so every gate that needs it prepares a real
  replacement first.


## Design choices flagged for review

* **Dimension is an explicit fifth target.** Labels are always drawn and never intercept Object, Endpoint or
  Pair; this costs one button and keeps the targets honest. A label can be covered by another label or by
  nothing else; the persisted rows reach every dimension.
* **One selection, derived dimension.** Instead of a second "selected dimension" state the form uses the one
  selected curve, because a curve has at most one dimension; a persisted row is therefore a way to select its
  curve. Consequence: selecting a Line with a saved length by any means shows that dimension's editor.
* **The editor sits in the persisted list** (bounded at 150 points) instead of a new row, because the 768-point
  worst case has no spare height. The cost: for a very long list the editor is brought into view by scrolling,
  one frame after the selection moves, and a layout change can leave it a point or two clipped until the list
  is scrolled.
* **Labels are shortened, not rounded**: nine characters of the exact number, else four decimals and `…`;
  the row and the editor sentence give every digit.
* **A label is absent, not clamped, when its anchor is off the canvas** (zoomed far in); Fit returns it.
* **The typed text is kept across selections** (like the other boxes); the line next to the box always names
  the rule and the target that a press would replace.
* The old **Replace length** stays beside the new action (same request for the same number).

## Not run by the author

The author ran no window, the viewer, CUA, a system dialog, a GPU test, Windows/Linux or the remote CI of
this diff. No author window pass is claimed. Still open, and not closed by any headless pass: the §31J/§31K
native-window pan, pinch and held-input cases (a native pinch, a held middle button during a tab switch, an
external window covering the canvas); Windows and Linux windows; arcs, mixed profiles, the annulus on this
canvas, DirectedAngle and the other relations as labels; dragging; the product, Milestone 5C, §30 and the OOM
investigation (not declared fixed or reproduced).

## Reviewer recipe

`/private/tmp/ferrite-31l-window/` (made by `make_inputs.py`; copies of the generator, the recipe, the
comparator and the command-line stand-in maker are in the ignored `target/dimension-labels-check/window-recipe/`):
`inputs/` holds `rectangle.fcad`, `rectangle-b.fcad` (a byte copy), `cylinder.fcad`, `overlap.fcad`,
`many.fcad`, `plain.fcad`, `annulus.fcad` and `SHA256.txt`; **no expected output or screenshot exists**.
`RECIPE.md` there has the exact scenario with the controls by their names (**Dimension**, **Replace
dimension**, **Radius**/**Diameter**, **Apply constraints**, …). `compare.py` checks the saved file and the
exports against the command line given the same request: the UUIDs (every kept one still there, every
removed one gone, the right number of new ones), every SQL cell, the complete Sketch row and CBOR payload
(the real hash of each file validated first, only the fresh constraint UUIDs mapped), byte-identical
STL/FBX, and an independent STL reader (closed oriented mesh, real extents or radius); `--negative` swaps the
pristine input for the saved file and must print `NEGATIVE CONTROL HELD`. The author exercised it only on
command-line stand-ins (not window outputs), for both kinds.
