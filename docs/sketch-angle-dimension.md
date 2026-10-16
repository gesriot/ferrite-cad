# §31M — A saved directed angle as a selectable, replaceable dimension on the accepted Sketch canvas

Status: reviewed implementation. The author handed over an unstaged/uncommitted diff and ran no
window, viewer, CUA or system dialog; independent review and its corrections are recorded below.
Plan entry: [§31M](implementation-plan.md). Base:
`763e082516a44f87480d22d6f17f464a009d1233` (merge of PR #116, tree
`dabb5ff50fc7a3e19060514297190d414404cbc5`, identical to its checked PR head `058b1d7`).
Independent review refreshed the exact base: **27/27 check runs successful**. Remote validation of
this change belongs to its own PR commits; base checks do not substitute for those results.
Branch during implementation/review: `sketch-angle-dimension`.

## Problem

§31L gave the accepted solved drawing of the **Sketch constraints** form a label for each saved whole-Line
length and a single Circle's radius, a **Dimension** target that selects one, and **Replace dimension**. A saved
**DirectedAngle** (§31F) between two Lines was still only a row of text in **Persisted constraints**:
`Directed angle −70° · <UUID>` with *Remove*, and replacing it meant ticking *Remove* and typing the pair, the
two directions and the angle again.

## Decision

The same accepted drawing carries a **label for each saved DirectedAngle**; the **Dimension** target selects
it; the one **Replace dimension** action prepares the replacement as the existing atomic remove/add of the
draft, **keeping both exact directed vectors of the saved rule**. A UI adapter over the existing family: no new
constraint, document field, schema, capability, FFI, solver equation, jobs operation or CLI request, no
coordinates written in place of the rule, no stored label position. Not covered, as before: every other
relation, an annulus, arcs, mixed profiles, a form with no accepted drawing (it keeps its rows and editor,
without labels), dragging a dimension.

### Decision 1 — a dimension selected by its rule UUID beside the curve selection

§31L had one selection, the curve, because a Length or a Radius belongs to exactly one curve. A DirectedAngle
belongs to **two** Lines, in an order, each with a direction. So the identity of a selected angle is **the
full UUID of its saved rule**, resolved in the form's own reading; it is not a curve, not the pair and not the
number.

* The form keeps its **curve selection** (`selected`) exactly as it was, and gains **one more field**:
  the UUID of the selected angle rule (`turn`). **At most one of the two is set.** Selecting an angle
  (its label under *Dimension*, or its row in the persisted list) sets the UUID and **clears the curve
  selection**: neither Line is "the" selection of an angle, and no curve and no end is invented for it.
  Setting the curve selection by any means (canvas under Object, Endpoint or Dimension on a Length/Radius
  label, the Lines list, a Length or Radius row) clears the angle. Nothing else moves.
* Both Lines of the selected angle are highlighted on the solved drawing with the direction of each vector
  (`from` → `to`, an arrowhead at the `to` end) and the letters **A** and **B**, in the selection blue and
  drawn on the other side of the Line from the pair-slot letters, so a saved angle is never mistaken for the
  **Pair A / Pair B** slots.
* The **pair slots and their `start>end` directions are never reassigned** by selecting or replacing an angle:
  they are the unapplied input for *adding* a new constraint, and stay as the person set them.
* Choosing publishes nothing, adds nothing, spends no Undo and leaves every typed text alone. The persisted
  row is selected and brought into view like a Length's. The editor is the one under the selected row.
* A label hit is reported as the **rule UUID** (`Click::Angle(rule)`); the form accepts it only if its own
  reading holds that rule. A rule the form does not hold selects nothing.

Length and Radius keep their model untouched (`Dimension`: one curve, millimetres); the angle is its own typed
sibling (`Turn`: the rule UUID, the two `DirectedLine`s and the `DirectedAngle`). Degrees are never put in
`target_mm`, a pair is never put in a curve field.

### Decision 2 — where the label of a pair stands

A pair has no single place, and an arc between two Lines would have to pick one of the two angular sectors and
so could show the smaller unsigned angle of a signed value. So **no arc, no intersection and no search**.

* The label stands at the **middle of the segment that joins the middles of the two solved Lines** (screen
  space), centred, then slid into the band of the canvas that the words on its edges leave (§31L's rule).
  That point exists for every pair of live Lines: 0° and 180°, parallel, nearly parallel, collinear, crossing
  at their middles and overlapping Lines all give a finite place, and none of them needs the Lines to meet.
* The label is **absent**, not clamped onto an edge, when that point is not on the canvas, or when either
  Line cannot be drawn. The persisted rows reach every angle.
* The words are `A>B −70°`: the order (A, then B), the **accepted target of the saved rule**, never an angle
  measured on the stored guess or on the picture, and the unit. A prepared replacement is told beside it
  (`A>B −70 -> 110°`; `A>B −70° -> removed`) and nothing moves. The number is the saved value written with the
  same editable degree spelling as the persisted row, but never in a way that reads as another value: a value that rounds to
  `0` or to `±180` at four decimals is written with all its digits (`0.0000000001`, `179.9999999`), and a
  value that does not read back is said so in the editor line (below).
* The directions of A and B are told by the highlight and the arrows when the angle is selected or hovered,
  by the hover caption (`A: Line 3f2c1a start>end · B: Line 91ab end>start`) and by the persisted row (which
  already said them); the label's order `A>B` pairs with those.
* Overlaps are decided by a **saved order** and never by UUID or hash: the labels of curves (Lines' lengths,
  the Circle's radius) as §31L has them, then the angle labels in the order of their rules in the Sketch. The
  first is drawn on top and is the one a point in the overlap reaches; the persisted rows reach each of them.
* Zoom, pan and **Fit** are §31J's arithmetic, so a label moves with its Lines, and Fit gives the opening
  place back. The hit area is the drawn box; a click reaches a label only under **Dimension**.

## What the number is

The accepted semantics are §31F's and are not reinterpreted: two ordered vectors (the stored end references),
a positive turn counter-clockwise from A to B with Y up, `(−180°, 180°]`, `−180` typed is stored `+180`,
`0` is the same direction, anything outside `[−180, 180]` is refused and never wrapped. The unit boundary is
`DirectedAngle::from_degrees` / `degrees()` / `radians()`; there is no `acos`, no absolute value, no wrap of
this slice's own and no tolerance anywhere, including the return to the saved value, which is the exact equality
of the validated `DirectedAngle`. A zero vector cannot be spelt (`DirectedLine` needs two different ends) and a
Line that cannot be drawn has no label. Swapping A and B negates the number and reversing one vector adds 180°
(with the canonical `+180`); that is the rule's own business, and an existing Parallel is not changed by it.

Replace dimension, for a saved angle, behaves as it does for a length: a different number prepares **one**
removal of the saved rule and **one** `DirectedAngle` addition on the same two exact `DirectedLine`s (one
`History::change`); another number updates that addition where it stands; the number already pending changes
nothing; the exactly saved number takes back only this replacement (the saved rule keeps its UUID, every other
pending change stays); an empty request offers nothing to Apply or Save; text that is not a number in range,
a refused draft, a running job and a waiting request leave the exact text, the selection and the history alone.
The relation slot's occupancy and the document validator decide what is allowed; nothing is bypassed.

## The number: display, typed degrees, `DirectedAngle`

The numeric contract is unchanged: typed degrees still cross `DirectedAngle::from_degrees`, and
`DirectedAngle::degrees()` and CLI/JSON output are untouched. The UI's `editable_degrees` prefers that
existing display. When it would not convert back to the exact saved radians, it tries at most four
adjacent `f64` degree values on each side and accepts a candidate only after the existing converter
returns **the identical `DirectedAngle`**. This is a bounded presentation search, not a tolerance or
another degrees/radians conversion. The persisted row, hover caption, pending description and exact
canvas-label spelling use it consistently.

The author found 18 failures among 1 596 probes ±k·1e-10 and ±(180 − k·1e-10), k = 1…399.
Independent review first added an exact-roundtrip assertion: the original implementation compiled,
executed the named test and failed (exit 101, 18 gaps). The UI correction lets every one of those
probes type back exactly, including 7.9e-9°, while the document and CLI remain unchanged. The real
widget test types the displayed spellings, checks that no pending edit/history step appears, and
reverts a prepared replacement of the formerly problematic angle without replacing its saved UUID.

Not every arbitrary stored radian value has a degree input that maps to it. For example, canonical
0.17453292519943298 radians has none in the checked neighbourhood. If no exact input is found the
UI retains the original display and explicitly says that typing it changes the angle; Undo or
Clear pending changes remains the way back. The pure gate covers this fallback separately.

The short canvas annotation may use an ellipsis for other non-boundary values. Where four decimals
would collapse a distinct angle to 0 or ±180 (1e-10°, 179.9999999°, −179.9999999°, 0.0000123456°), it
uses all digits. Selection and the persisted row expose the exact editable spelling.

The new pending-angle annotation uses the ASCII arrow `->`, because the bundled canvas font
rendered the former arrow as a missing-glyph box in the native window. Existing numeric semantics
and the saved signed target are unchanged.

## Layout and the 768-point budget

The label work is drawn on the canvas (height 0). The editor lives **inside the 150-point persisted-rules list**, so it
adds no height wherever that list is full. Measured on the 988 × 768 gate screen (bottom of **Save constraints copy…**
with an angle selected / with a long text, a prepared replacement, a refusal and the unsaved note; the fully loaded
64-gon has no room for a refusal line, as in §31K/§31L):

| Form | Bottom |
| --- | --- |
| plate holding two lengths and three angles | 683.5 / 716.5 |
| 64-gon with every closure link, a stored length, two angles and 32 pending Horizontals | 700.5 / 754.5 |

The widest text ends at 671.1 points (as in §31L). Every button, the editor, the labels and the canvas are whole inside the
screen and their clips (`…the_angle_labels_the_editor_and_the_actions_stay_reachable_at_768_points_in_the_worst_forms`).
§31L's own gate gives the same bottoms as before (681.5/714.5, 698.5/**767.5**, 505.5/538.5).

## Executed gates (author, local)

Every test executable ran under `tools/watch-viewer-memory.py --limit-mib 1536` with a finite time limit and its own log
(`target/angle-dimension-check/guard/`), `CARGO_BUILD_JOBS=1`, the existing native and true-stub targets, pinned
libraries, sequential builds. No viewer, window, CUA or GPU test was started. The frozen tree of the final runs is
`4d8b6ecaf003a63e` (sha256 of the sha256s of every modified or untracked `.rs`/`.yml`/`.sh`/`.py`); it was identical before and after the mutation campaign and after the whole positive sequence repeated on the restored tree. Four complete sequences (fmt, lints, repo checks, clippy, true-stub groups and whole suite, the real ci.yml bodies, native build, whole suite, real runtime bodies, pinned reader) ran on the sources as they now stand or on trees that differ from them only in what "Found while building" lists; the last one (post-mutation repeat) is the evidence below.

| Check | Result |
| --- | --- |
| New tests | 22: 5 pure (`sketch::turn_label_tests`), 13 form (`constraints::tests::turn_label`, real egui events; 1x and 2x for labels, replacement, view and blocked input), 3 tab (`sessions::tests::tabs::drafts::turn_form`) = **21 kernel-free**, and 1 native (`sessions::tests::native_a_saved_angle_…`). Written with the code; the failing-first evidence is the mutation campaign below and the red first runs of my own fixtures (list under "Found while building") |
| Old filters untouched | through the watchdog, `--list`: §31L filter 22, §31K filter 30, §31M filter 21, **zero** §31M names matched by any older substring filter (`sketch::dimension_tests`, `constraints::tests::dimension`, `…drafts::dimension_form`, `sketch::accepted_tests`, `accepted_scene_tests`, `constraints::tests::canvas`, `…drafts::canvas_form`, `sketch::analytic`, `sketch::navigation`); the old 52 tests (22 + 30) pass after the change |
| True stub, the real new `ci.yml` step body (only its `cargo test` line replaced by the same three filters on the built executable under the watchdog) | `STEP_BODY_PASSED`; `21 passed; 0 failed`; every name and all 21 `FCAD_31M_*` exact-line markers; the real **old** §31L step body: 22 passed, unchanged |
| True stub, whole viewer suite (`--nocapture`) | **866 passed, 0 failed, 4 ignored** (§31L: 844); 21 `FCAD_31M_*`, 22 `FCAD_31L_*`, 30 `FCAD_31K_*` markers; 222 skip lines (§31L: 221): the one added is the native gate, which skips in a stub and is **not** a geometry pass there |
| Stub proof | bridge `CMakeCache.txt`: `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE:UNINITIALIZED=TRUE`, `OpenCASCADE_DIR:UNINITIALIZED=…/absent-occt`; build warning "Open CASCADE was not usable"; `otool -L` of the fresh test executable **and** of a fresh stub peer CLI: 0 × libTK, 0 × planegcs |
| Native (OCCT + PlaneGCS, release), whole viewer suite | the fresh test executable and the peer CLI each import 50 libTK and libplanegcs; **866 passed, 0 failed, 4 ignored**; 23 `FCAD_31M_*` lines (the 21 + `…_NATIVE_ANGLE_REQUEST` and `…_NATIVE_ANGLE_EXECUTED`), 26 `FCAD_31L_*`, 34 `FCAD_31K_*`; 23 skip lines (§31L's count; none of this slice's gates); all 25 marker lines of `occt-pin.yml` present |
| Native scenario (`write_angled_quad`) | the Sketch stores the parallelogram `(-6,-3) (6,-2) (7,5) (-5,4)` (it turns ±77.106°) and the rules Line 1 start fixed at the origin, Line 1 horizontal and 50 mm, Line 2 30 mm, Line 3 40 mm, **R_a** Line 1 start>end → Line 2 start>end **+100°**, **R_b** Line 2 **end>start** → Line 3 start>end **−100°**. The accepted `SketchPresentation` is `(0,0) (50,0) (44.7906,29.5442) (4.7906,29.5442)`; the form's drawing equals it curve for curve and measures +100° and −100° by `atan2(cross, dot)` of the very vectors the rules name (the stored guess does not). Five labels: `A>B 100°`, `A>B -100°`, `50 mm`, `30 mm`, `40 mm` (none says 77.1). A click on the `-100°` label under Object takes nothing; under **Dimension** it selects R_b, marks Lines 2 and 3 with the arrows (Line 2 toward its Start) and `-130` typed + **Replace dimension** gives one removal of R_b and one `DirectedAngle` on Line 2 end>start / Line 3 start>end of −130°, equal to the request made by the form's own road (Remove tick, pair, the A direction turned, the degrees box, Add directed angle). Then through the real worker: **Apply** into an unsaved session after a height step, export before Save, Undo, Redo, Save and a cold rebuild with every saved reference resolving, each against the peer CLI given the same request: every SQL cell equal after the one mapping of the freshly minted constraint UUID (`same_model_with_explicit_new_ids`: raw payload hash validated, UUID bytes mapped once, all cells compared, the Sketch row included), STL/FBX byte-identical. The cold rebuild's drawing turns +100° and **−130°** between the same vectors, keeps the three lengths and the anchor; its form holds `A>B -130°` under a new UUID and `A>B 100°` under the UUID it had; the saved STL, read by `read_stl_of` (not the writer), has volume = polygon area × height within 1e-4 and its base vertices are the drawing's four. **Negative control:** the pristine source and its own STL given to the same reader fail it (`the saved drawing turns 100 and -100.00000000000335`), caught by the gate, which then prints `negative=held` |
| Real `runtime-layout.yml` step body (new; body cut by indentation, `matrix.name` = macos, `$PWD/vendor` made absolute, only its `cargo test` line replaced by the built release executable under the watchdog) | `STEP_BODY_PASSED`: `1 passed; 0 failed` with its exact marker; `angle-dimension-quad.stl`/`.fbx` written; the real old §31L step body (two gates): `STEP_BODY_PASSED` |
| Pinned reader (the real `FCAD_ANALYTIC_SESSION_FBX_DIR` hook block of `tools/check-fbx-complex.sh`, cut by its own `if`/`fi`, loop narrowed to the new and the two §31L names, ufbx 0.23.0 rebuilt from the cached digests) | `checks=6 failures=0` for each; `FCAD_STL_FBX_MATCH triangles=12` (new) and `12`, `364` (§31L); a truncated copy of the new FBX: `checks=1 failures=2` |
| Comparator and window recipe (`/private/tmp/ferrite-31m-window/`; copies in the ignored `target/angle-dimension-check/window-recipe/`) | `make_inputs.py` wrote seven inputs (`quad`, `quad-b` = byte copy, `halfturn`, `overlap`, `many`, `plain`, `annulus`; every rule set solved, DOF 0 where stated); `compare.py` exercised only on **command-line stand-ins, not window outputs**: `VERDICT: PASS` for `quad` (−100 → −130, both saved angle rules measured by atan2 on the saved STL ring: +100° and −130°) and for `halfturn` (179.9999999 → 170); `--negative` prints `NEGATIVE CONTROL HELD` for both; a stand-in whose sign was flipped (+130 for −130) prints `VERDICT: FAIL` (Sketch row, STL, FBX and the expected angle differ) |
| fmt, `clippy --workspace --all-targets --all-features -D warnings` | pass (true-stub environment) |
| actionlint (all workflows), shellcheck `tools/check-fbx-complex.sh`, `tools/check-actions-run-size.py` | pass; 220 blocks, largest 20 432 of 21 000, packed argv at most 20 463 (the new `ci.yml` and `runtime-layout.yml` steps are their own small blocks; nothing was added to a large one) |
| Licence headers (520), export boundary, solver ownership, notice ownership, PlaneGCS pins, STEP corpus, `git diff --check` | pass; no manifest, lock or deny change |
| Mixed (OCCT without solver) | **N/A, reasoned:** no kernel, solver, ABI, schema, job or CLI boundary is touched (`check-solver-ownership` and `check-export-boundary` pass; the diff is app-crate UI code, tests, workflow lines and docs); the native gate asserts `FERRITECAD_REQUIRE_PLANEGCS` separately and skips without a solver; no new capability, so no new refusal shape to measure; not run |
| Mutations (`target/angle-dimension-check/mutate.py`) | **16 mutants, 26 gate runs (3 mutants also on the native executable): every one compiled and failed with `test <gate> ... FAILED` and a `panicked at` line of the gate it names** (exit 101, never a compile failure or zero tests); the four mutated files were restored byte for byte (sha256 identical for each, frozen-tree hash identical), then the whole positive sequence was repeated on the restored tree (native executable rebuilt). *Class 1, sign / direction / UUID lost:* (1) the typed sign dropped (`degrees.abs()`) — replace gate and native gate; (2) the replacement's first vector run the other way — replace gate and native gate; (3) an angle carries another rule's UUID — saved gate and label gate. *Class 2, atomic replace / revert broken:* (4) the replacement without the removal of the saved rule — replace gate and native gate; (5) the revert leaves the saved rule removed — replace gate; (6) the revert drops every other pending change — replace gate; (7) a second replacement appends a second pair instead of updating in place — replace gate. *New reasons:* (8) overlap decided by UUID, not the saved order — overlap gate and hit gate; (9) selecting an angle keeps a curve selected — label gate and sync gate; (10) a curve picked on the canvas or in a list keeps the angle — label gate and sync gate (this mutant first *survived* in a campaign on an earlier tree: see "Found while building"); (11) the arrow ignores where the vector starts — label gate; (12) a tolerance on the saved angle — semantics gate and number gate; (13) the label rounds 1e-10° and 179.9999999° to the boundary — text gate and number gate; (14) the editor never says the display does not read back — number gate; (15) a pair label on the first Line's middle only — place gate and label gate; (16) a held form keeps the labels of another version — tab gate. Logs: `target/angle-dimension-check/mutations/`, `M-mutate.log`. |
| Resources | final tree (runs C, the campaign, D): 44 watchdog logs, peak footprint **291.955 MiB**, pressure 1 in all 397 samples, swap 0, none aborted; the 26 non-zero exits are exactly the 26 mutant gate runs above (exit 101), every other log exited 0. All 106 logs of the whole work (including the stopped campaigns and my own iterations): peak 314.034 MiB, pressure 1, swap 0, none aborted. Every test executable (the widget groups, both whole suites, every step body, every mutant gate, the `--list` proofs after the first three) ran under `tools/watch-viewer-memory.py --limit-mib 1536`; `CARGO_BUILD_JOBS=1`; no other target than the existing native and true-stub ones; OCCT and PlaneGCS were not rebuilt (the native build is the app crate and the peer CLI only, 1m40s); the other sessions' processes were left alone. No viewer, window, CUA or GPU test was started |

### Found while building, and changed

* **A natural drawing makes labels overlap.** On the four-Line plate the label of (Line 1, Line 2) overlaps the length label `30 mm`, so a hit-box test at its corners reached the length: the exactness and edge gates use a `spread` drawing (four short Lines at the corners of a wide box) and a pair whose label has no neighbour. The overlap gate uses the opposite, `crossing` drawing (every Line through one point).
* **The validator wants every closure link of a Line set that keeps such a rule**, so the synthetic tab hook (`Editor::persist_angles_for_tests`, named synthetic; the true stub cannot rebuild a document that holds a persisted rule) adds them, as the Length hook does. The real documents of the form gates are written with `prepare_sketch_constraints` (kernel-free) and read with `ExtrudeEditSource::read`.
* **UUIDv7 ascend in mint order.** To prove "saved order, never UUID" the fixture writes R2, R0, R1 and stores R0, R1, R2: neither ascending nor descending by UUID; the pure gate sorts descending.
* **The native fixture first used ±80°**, which is within 5° of the ±77.1° the stored guess measures; it now uses ±100° (replacement −130°). The first native run failed on that assertion (executed, not a skip), as intended.
* **`Canvas::show` had 8 parameters** (clippy `too_many_arguments`, `-D warnings`): the dimensions the canvas labels travel as one `Labelled { sizes, turns }`.
* **Test-code visibility only** was widened in two §31L test files (`constraints/tests/dimension.rs`: 24 helpers, fields and a fixture `pub(super)`; `sessions/tests/tabs/drafts/dimension_form.rs`: 8 helpers `pub(super)`) so the new modules extend the existing harness instead of copying it. No assertion or body of an old test changed; verified by diff (every added line in the first file contains `pub(super)`; the only other added lines in the second are the four lines rustfmt wrapped one longer signature into).
* **Two cleanups and one more assertion after the first full run** (all evidence below repeated on the final tree): the two label painters share `label_look`, the persisted-row `match` is on `(dimension, turn)`, and the top edge's words under Object/Endpoint for a selected angle are asserted.
* **The mutation harness found dead code in my own change.** A campaign on a tree that cleared the angle twice after a canvas pick (once in the click loop, once for every source after the Lines list in the same frame) stopped at its 10th mutant: deleting the first clearing changed nothing, so that mutant *survived* (`exit=0`). The redundant block was removed (one normalisation remains, with a comment saying why the angle's own arms set `selected` to none) and the mutant was redefined to delete the one that remains, which both the label gate and the sync gate must catch. Its logs: `target/angle-dimension-check/third-campaign-surviving-mutant/` (its first ten mutants all failed on executed assertions of their own names, as the final campaign's do).
* **Two earlier mutation campaigns were stopped by me and are not evidence.** The first (SIGINT, about 90 seconds in, during its first native mutant) because its harness copy still wrote its logs into the §31L check directory; its stray logs were moved to `target/angle-dimension-check/interrupted-first-campaign/`. The second ran on the tree before one more assertion was added (the top edge says `Selected: angle …` under Object): I stopped it with SIGINT after about three minutes, the harness did not leave its native build, my `pkill` of that cargo then made the harness's native gate of the second mutant report exit −15 (a refusal by the harness's own rule, not a product result); the four mutated files were byte-identical after each stop (sha256 and the frozen-tree hash), an orphan `rustc` of mine was killed by PID, and its logs are in `target/angle-dimension-check/interrupted-second-campaign/` (its completed gates — the lost sign on the stub and the native executable, the lost direction on the stub — all failed on executed assertions of their own names). The campaign below is a complete, uninterrupted run on the final tree.
* **The first run of the real runtime step bodies "failed"** inside my driver because the watchdog refuses to overwrite the log of my earlier manual run (stale `rt-step-1`); the driver now gives each run its own log directory and the bodies passed. Not counted as a product failure and not hidden.

## Design choices flagged for review

* **Exclusive selection.** Selecting an angle clears the curve selection (and the reverse), instead of keeping a curve selected beside it: an angle belongs to two Lines, and two editors for two dimensions would otherwise be on screen. Consequence: after choosing an angle the Add buttons that need a selected Line are disabled until a Line is chosen again; the pair slots, their directions and every typed text are kept.
* **No arc and no leader lines.** The pair label stands at the middle of the middles; the Lines it relates are told by its order `A>B`, the hover caption and, when it is selected or hovered, the marks and arrows. An unselected label does not draw a connection to its Lines.
* **Label order.** The labels of curves are above the labels of pairs; within each, the saved order of the rules. Not by UUID, not by hash.
* **The UI number uses the existing converter**, with the bounded exact-roundtrip presentation search above; no tolerance changes the saved value.
* **The editor is in the persisted list** under the selected row, with its own degrees box (`dimension-deg`); the millimetre box is never read in degrees.
* **Arrows** of a selected angle are filled triangles in the selection blue four fifths along each vector, drawn from the `from` end toward the `to` end, with the letters **A**/**B** below the Line — apart from the orange/green pair-slot marks above it.
* **`DirectedAngle::degrees()` is not changed**: the exact-roundtrip search is confined to the UI and leaves JSON/CLI output alone.

## Not run by the author

The author ran no window, the viewer, CUA, a system dialog, a GPU test, Windows/Linux or the remote CI of this diff. **No author window pass is claimed**, and no expected GUI result is written down in place of a window. Still open, and not closed by any headless pass: this slice's own window pass; the §31J/§31K/§31L native-window pan, pinch and held-input cases (a native pinch, a held middle button during a tab switch, an external window covering the canvas); Windows and Linux windows; arcs, mixed profiles, the annulus on this canvas, the other relations as labels; dragging; the product, Milestone 5C, §30 and the OOM investigation (not declared fixed or reproduced).

Deviations from the stated resource rules, disclosed: the stub test executable was started three times with `--list` directly (not under the watchdog) before the same listing was repeated under it; the pinned-reader negative control wrote a 2 000-byte truncated FBX to `/tmp` for a moment and a second one to `/private/tmp/ferrite-31m-trunc/` (both removed); the interrupted first campaign above.

## Reviewer recipe

`/private/tmp/ferrite-31m-window/` (made by `make_inputs.py`; copies of the generator, the recipe, the comparator and the stand-in maker are in the ignored `target/angle-dimension-check/window-recipe/`): `inputs/` holds `quad.fcad`, `quad-b.fcad` (a byte copy), `halfturn.fcad`, `overlap.fcad`, `many.fcad`, `plain.fcad`, `annulus.fcad` and `SHA256.txt`; **no expected output or screenshot exists**. `RECIPE.md` there has the exact scenario with the controls by their names (**Dimension**, **Replace dimension**, `New angle`, **Apply constraints**, …). `compare.py` checks the saved file and the exports against the command line given the same request: the UUIDs (every kept one still there, every removed one gone, the right number of new ones), every SQL cell, the complete Sketch row and CBOR payload (the real hash of each file validated first, only the fresh constraint UUIDs mapped), byte-identical STL/FBX, an independent STL reader, and **every saved directed-angle rule measured on the saved solid by `atan2(cross, dot)` between the very vectors it names, in the direction it names them, within 5e-3° (float32 STL)**; `--negative` swaps the pristine input for the saved file and must print `NEGATIVE CONTROL HELD`. The author exercised it only on command-line stand-ins (not window outputs).

## Independent local review

Two presentation defects were corrected without changing the document, solver, jobs, CLI or JSON:

* Exact editable degrees: the failing-first gate compiled, ran one test and failed with 18 gaps
  among 1 596 values (exit 101). The bounded UI-only search described above closes all those gaps;
  the widget gate types the displayed number and reverts a replacement of 7.9e-9° exactly. The
  separate unreachable-radian probe still gives an honest warning.
* The new pending-angle arrow rendered as a missing-glyph box in the native window. It now uses
  ASCII `->`; the final bundle was rebuilt and the corrected annotation was observed in a second
  guarded native process. Existing Length/Radius behavior is unchanged.

Final source checks on macOS arm64: viewer native **866 passed / 0 failed / 4 ignored**, true stub
**866 / 0 / 4**, workspace clippy all targets/features `-D warnings`, fmt, actionlint, licence,
export/solver/notice boundaries and Actions run-size. There are 23 native-suite conditional skip
messages and 222 stub messages (including the native-only angle case); none is counted as native
geometry or as execution of the 21 mandatory kernel-free cases. Stub CMake explicitly disables
OCCT discovery and both fresh test executable and CLI import no OCCT/PlaneGCS. New CI run bodies
were also executed locally with the same filters, exact names, counts, markers and no-skip tests;
only `cargo test` was replaced by the already-built executable under the memory guard. Native
runtime case and artifact checks passed. The earlier §31L group remains exactly 22 cases.

Window review used private inputs outside the checkout, the freshly staged bundle and only the
owned watchdog PIDs. PID **27640** (Mach-O UUID `E84C48A7-0370-38B3-B474-1699BF23FCE1`, after the
number correction, before the ASCII-only label change) exercised:

* Select the saved −100° label, correct two solved vectors/arrows, keep separately chosen Pair A
  (Line 4) and Pair B (Line 1, reversed); prepare −130°, draft Undo/Redo, exact return to −100°,
  invalid `abc` with text/selection retained, Save constraints copy Cancel with draft retained.
* Apply −130°, export STL/FBX **before Save**, document Undo/Redo, Save the private model; reopen
  the form showing +100° and −130° under the retained/new rule UUIDs. Wheel zoom moved the drawing
  and labels off-canvas; Fit visibly restored the opening drawing and labels.
* On the real overlap fixture the first stored +180° rule wins the coincident label; its other
  rule remains selectable by the persisted row and highlights the other two Lines. Typing −180°
  for +180° prepares no change. On the 48-gon, choosing 7.5° reveals its distant row/editor without
  manual scrolling; a typed `8` and the selected rule survive a tab round trip.

All seven pristine input hashes remained unchanged. The actual saved model and before-Save
exports were compared with the existing CLI given the same directed endpoints and −130° request:
all SQL cells, the complete CBOR Sketch payload and validated hash agree after mapping only the
fresh constraint UUID and excluding modified_at; source is intact; saved/unsaved/CLI STL and FBX
are byte-identical. An independent STL reader found a closed 12-triangle prism, z = {0,10} mm,
extents 50 × 49.5442 × 10 mm, volume **16982.324464 mm³**, and turns **+100° / −130°** measured by
atan2 on the exact rule-directed Lines. Substituting the pristine source failed the comparison.
Pinned ufbx 0.23.0 read the GUI FBX (6 checks, 0 failures), and its 12 triangles matched the STL
with zero worst distance. All **12** final native analytic artifact pairs were read and matched
through that same small-reader path; no extra large STEP campaign was run locally.

Final bundle UUID `2BFE3C13-B296-3A3C-A70B-A1DF2A1E8532`, PID **39074**, reopened the saved −130°
model, selected the same rule, prepared −110°, and visibly showed `A>B -130 -> -110°` with no
missing glyph; it then cancelled that disposable draft and quit. Both window processes exited
normally (0), with sampled peaks **214.954 / 193.142 MiB**, pressure 1 throughout, swap 0 and no
watchdog abort. After Quit completion was checked only by PID/guard, never by querying the app.
The final full native/stub suites also exited 0 under the guard, peaks **287.126 / 156.048 MiB**.

Evidence is local and ignored: `target/angle-dimension-review/` (`number-failing-first`,
`final-native-suite`, `final-stub-suite`, `final-ci-*`, `final-runtime-step`, `compare-positive`,
`compare-negative`, `final-ufbx`, `gui`, `gui-final` logs); private window inputs/work are in
`/private/tmp/ferrite-31m-review/`. Source, tests and workflow remain reviewable in the PR; temporary
logs/models are not committed. Native-window pan/pinch, held input/external covering, the near-half-turn
window case, a physical 1x display, same-UUID dual-tab window interaction and Windows/Linux windows
remain unobserved here (their headless cases are distinct evidence). No product/§30/5C or OOM
completion is claimed.
