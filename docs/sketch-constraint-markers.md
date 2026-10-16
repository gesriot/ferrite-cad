# §31N — Saved non-dimensional constraints as selectable markers on the accepted Sketch canvas

Status: implemented, awaiting independent review. The author's handoff is an unstaged/uncommitted diff on the branch
`sketch-constraint-markers`; the author ran no window, viewer, CUA or system dialog. Plan entry:
[§31N](implementation-plan.md). Base: `main` = `origin/main` = `6e791da1810a4dbc7bd87af3a9c81cf5e1636915` (merge of PR #117,
tree `21aaea4afdd486ae169ae36521b2844d06b0e4e3`, identical to its checked head `a4a6e74`; fetched again at the end, still the
same). The base's own runs of that merge SHA: `CI` 38897109655, `planegcs pin` 38897109640, `rust notices` 38897109570,
`rust sbom` 38897109689 and `product sbom` 38897109735 all `success`; `combined runtime layout` 38897109791 was `in_progress`
when read. That is the base, not a CI of this diff: **the CI of the unstaged diff has not run**, and nothing here substitutes
for it.

## Problem

§31L–§31M gave the accepted solved drawing of the **Sketch constraints** form labels for the saved
whole-Line lengths, a single Circle's radius and the saved directed angles. Every other saved rule —
Horizontal, Vertical, Fixed, EqualLength, Parallel, Perpendicular, Collinear and a single Circle's Fixed
centre — was still only a row of text in **Persisted constraints** with a *Remove* tick; a person could
not see on the drawing which Line or point such a row meant.

## Decision

A UI adapter over the existing rules: each of those saved rules gets a **marker** on the accepted drawing,
selected by an explicit **Rule** target; selecting it marks the rule's own stored references on the
solved geometry and its row in **Persisted constraints**; an explicit **Remove constraint** action
prepares its removal through the form's existing validated remove and history. No new constraint, document
field, schema, capability, FFI, solver equation, jobs operation, CLI request, stored marker position,
placement engine, dragging or preview. Not covered, as before: arcs, mixed profiles, an annulus, a form with
no accepted drawing (it keeps its rows, without markers), construction geometry, Coincident closure (never
a marker, never removable).

### Decision 1 — identity and selection of a marker

* A marker is its **rule**: `Mark { id: the saved rule's UUID, kind, refs }`. `kind` is its own enum of the
  seven families (not a curve kind, not a `Dimension` or a `Turn`). `refs` are the references **exactly as the
  rule stores them**: `Line(a, b)` for Horizontal/Vertical (two points of one Line, in the stored order),
  `Point(p)` for Fixed (a Line's Start/End, or the Circle's Center: a `SketchPointRef`), `Pair(a, b)` for
  EqualLength/Parallel/Perpendicular/Collinear (two `SketchSegmentRef`s as stored — never normalised, never
  re-ordered, never reduced to one curve UUID). Only whole Lines of the form's Sketch are marked; any other
  shape of a rule keeps its row and no marker.
* The marker list is read **once, from the form's own catalogue** (the same pinned reading as the rows, the
  lengths and the angles). A click on the canvas returns only the **rule UUID**; the form selects it only if
  its own reading holds that rule. Nothing is computed from coordinates.
* The form gets **one more selection field**, `mark`. **At most one of `selected` (a curve), `turn` (an
  angle) and `mark` is set**: selecting a marker (its label under *Rule*, or its row) clears the other two;
  choosing a curve or an angle by any means clears the marker. The selection never touches the **Pair A/B
  slots**, the directions of the new directed angle, the Pin end, any typed text, the pending request or
  its history. Consequence: after a marker is chosen the *Add …* buttons that need a selected Line are
  disabled until a Line is chosen again (as §31M's angle).
* Highlight = the stored references: the curve(s) the rule names in the selection blue, a Fixed rule's
  **Start/End or Center** ringed and told, a Pair marks **both** Lines (the first stored point of each
  reference as a dot, the second as a ring, so a reversed `SegmentRef` is shown as stored).

### Decision 2 — explicit picking mode and a finite, deterministic place

* A sixth explicit target, **Rule**, after **Dimension** (greyed when the form has no marker). **Dimension**
  keeps exactly its §31L–§31M meaning and reaches only numeric labels. Under Rule only markers are reached and
  the numeric labels are pictures; under every other target markers are pictures. Paint order follows the
  target (the active family on top), so *the area that is picked is the area that is visibly drawn*.
* A marker's place is a pure function of its stored references and the view, with no search: a Horizontal or
  Vertical at the middle of its Line on the side **opposite** to the Line's length label; a Fixed Line end just
  beyond that end (away from the Line), a Circle's Fixed centre to the lower right of the centre; a Pair at the
  middle of the segment joining the middles of the two Lines (as §31M's angle). The box is then slid into
  the band the canvas's edge words leave; a marker whose anchor is off the canvas, or whose Line or Circle
  cannot be drawn, is **absent** (not clamped). Boxes may overlap: the marker of the rule stored **first** is
  on top and is the one a point reaches, never decided by UUID or hash; the persisted rows reach every rule.
* The label is ASCII only (`H`, `V`, `Fix`, `Eq`, `Par`, `Perp`, `Col`; a prepared removal reads
  `H -> removed`); the hover names the kind and the rule (`Horizontal · saved rule <12 hex> · Line …`).
  A marker has its own look (tinted box, not the numeric labels' black one); a prepared removal is amber.

## What is highlighted when a rule is selected

| Rule | Marked on the solved drawing | Told |
| --- | --- | --- |
| Horizontal / Vertical | its Line in the selection blue; a dot at the first stored point of the rule, a ring at the second | `Selected rule <12 hex> · Horizontal` |
| Fixed (Line) | its Line; a disc and a ring at the **Start** or **End** the rule pins | the word `Start` or `End` |
| Fixed (Circle) | the circle; a disc and a ring at its **centre** | the word `centre` |
| EqualLength, Parallel, Perpendicular, Collinear | **both** Lines; for each stored segment a dot at its `from` point and a ring at its `to` point, so a reversed segment shows as stored | the hover: `Parallel · saved rule <12 hex> · Line … end>start · Line … start>end` |

The row of the rule in **Persisted constraints** is highlighted and brought into view with the editor under it. The editor
is one line, `Remove constraint` and a sentence (*No change pending.* / *Pending: this constraint is removed; the drawing
keeps it until Apply constraints.*), inside the 150-point list: it adds no height wherever that list is full.

## Remove constraint

The one explicit action `Remove constraint` prepares the removal of the **selected** rule through the form's existing
road and nothing else:

| Case | What happens |
| --- | --- |
| a rule not yet pending removal | its UUID joins the request's removals: **one** `History::change`; every other pending addition and removal, every typed text, the pair slots and their directions, the selection are as they were |
| the same press again | the same request: no step, no duplicate, no refusal, and a Redo that exists is not wiped |
| Undo / Redo / *Clear pending changes* | return exactly the request (Clear is itself one step that Undo takes back) |
| a request the document's validator refuses (a pair that would hold two answers, a count over the limit, …) | the validator's sentence in the refusal line; the exact text, the selection and the history unchanged |
| a running job, or a request of the form waiting to be taken | the button takes nothing |
| a closure link (`Coincident closure`) | has no marker and no row to select; a request that removes one is refused by the document's own rule, not by a list of ours |

The row's own **Remove** tick makes the same removal and remains; the two agree. `validate_edits` judges the whole request,
so relation occupancy is honoured: a Parallel removed in the same request frees its pair for a Perpendicular, and ticking
the removal off again leaves a draft that the validator refuses as a whole. Apply constraints / Save constraints copy…
send the existing request through the existing job; the CLI is given the same UUIDs (`edit-sketch-constraints-copy`,
request v1 `remove`).

## Layout and the 768-point budget

Markers are drawn on the canvas (height 0). The sixth target button first widened the row of targets by 36 points; the Lines
list beside the canvas then wrapped its 336-point `Segment` rows in the narrower column and **two Revolve forms (five-segment
sector, six-segment shaft) grew by 17 points (620.5 → 637.5, 635.5 → 652.5)** while every older gate passed (they assert
`<= 768`). The row now sets its words in the canvas's own 11 points (`RichText::size`; setting the Button text style did not
take effect), with a 2-point gap and 3-point padding: **269 points of words against 292 before**, and every measurement of the
§31K–§31M gates is identical to a frozen baseline executable (`SAME_MEASUREMENTS`). `the_target_row_keeps_five_targets_…`
asserts the two Revolve bottoms against 620.5 and 635.5. Measured on the 988 × 768 gate screen (bottom of **Save
constraints copy…** with a rule selected and its removal prepared / with a long text in a box, a refusal and the unsaved note;
the fully loaded 64-gon has no room for a refusal line, as in §31K–§31M):

| Form | Bottom |
| --- | --- |
| plate holding seven rules, two lengths and an angle | 653.5 / 686.5 |
| 64-gon with every closure link, a length, two rules and 29 pending Horizontals | 670.5 / 754.5 |
| five-segment sector / six-segment shaft (§31K worst, with the Rule target) | 620.5 / 635.5 |

The widest text ends at 671.1 points (as in §31L–§31M). Every button, the editor, the markers and the canvas are whole inside
the screen and their clips (`…the_markers_the_editor_and_the_actions_stay_reachable_at_768_points_in_the_worst_forms`).

## Executed gates (author, local)

Every test executable ran under `tools/watch-viewer-memory.py --limit-mib 1536` with a finite time limit and its own log
(`target/constraint-marker-check/guard/`), `CARGO_BUILD_JOBS=1`, `--offline`, sequential builds, the existing native
(`target/release`) and true-stub (`/private/tmp/ferrite-25j-stub-target/release`) targets, pinned libraries; the pressure was 1
before every run. No viewer, window, CUA or GPU test was started (the viewer executable's tests are headless; the
`NSOpenPanel` lines in the whole-suite stderr are the existing `null_panel_constructor…` test). The frozen tree of the final runs
is `1e90fa37ba6b1e10` (sha256 of the sha256s of every modified or untracked `.rs`/`.yml`/`.sh`/`.py` file); it was identical
before and after every run, the mutation campaign and the positive repeat that followed it.

| Check | Result |
| --- | --- |
| New tests | 24: 5 pure (`sketch::marker_tests`), 14 form (`constraints::tests::marker`, real egui events; 1x and 2x for markers, references, view, blocked input, circle and removal), 3 tab (`sessions::tests::tabs::drafts::marker_form`) = **22 kernel-free**, and 2 native (`sessions::tests::native_a_saved_equal_length_…`, `native_a_saved_fixed_centre_…`). Written with the code; the failing-first evidence is the mutation campaign below and the red first runs of my own fixtures (see "Found while building") |
| Old filters untouched | `--list` under the watchdog: baseline executable 870 tests, new 894 = +24. `filter-audit.py` reads the 104 viewer `cargo test` commands of every workflow (66 literal filters) with libtest's substring rule: only the three filters of the new step grew (0→5, 0→14, 0→3); **no older filter selects any new test**. The §31K/§31L/§31M groups: 73 = 30 + 22 + 21 passed, and every `FORM_*`/`NATIVE_*` measurement line equals the frozen baseline executable's (diff empty) |
| True stub, whole viewer suite (`--nocapture`) | **890 passed, 0 failed, 4 ignored** (baseline 866 + 24); 22 `FCAD_31N_*`, 21 `FCAD_31M_*`, 22 `FCAD_31L_*`, 30 `FCAD_31K_*` markers; 224 skip lines (222 before + the 2 native gates, which skip in a stub and are **not** geometry passes there); peak 62.5 MiB |
| Stub proof | bridge `CMakeCache.txt`: `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE:UNINITIALIZED=TRUE`, `OpenCASCADE_DIR:UNINITIALIZED=…/absent-occt`; `otool -L` of the fresh test executable **and** of the fresh stub peer CLI: 0 × libTK, 0 × planegcs |
| Native (OCCT + PlaneGCS, release), whole viewer suite | the fresh test executable and the peer CLI each import 50 libTK and libplanegcs; **890 passed, 0 failed, 4 ignored**; 26 `FCAD_31N_*` lines (the 22 + a `…_REQUEST` and an `…_EXECUTED` per native gate), 23 skip lines (the count before this slice: none of this slice's gates); all 30 `grep -Fxq` marker lines of `occt-pin.yml` present; peak 315.2 MiB |
| Native scenario, rectangle (`write_marked_rectangle`) | the Sketch stores the parallelogram `(-6,-3) (6,-2) (7,5) (-5,4)` and the rules Line 1 start fixed at the origin, Line 1 horizontal and 60 mm, Line 2 vertical and 30 mm, then **EqualLength** and **Parallel** of Lines 1 and 3 (EqualLength stored first). The accepted `SketchPresentation` is `(0,0) (60,0) (60,30) (0,30)`; the form's drawing equals it curve for curve and the stored guess is more than 5 mm from it at every Line start; the solver reports **0 degrees of freedom** on a cold rebuild. Five markers `Eq Fix H Par V` and two labels; `Eq` and `Par` stand on the middle of the solved rectangle. A click there under Object takes nothing; under **Rule** it takes the EqualLength, the Parallel beneath is reached by its row; Lines 1 and 3 are marked (two dots, two rings). **Remove constraint** gives one removal of the EqualLength UUID and no addition, equal to the request made by the form's own road (the Remove tick of the row). Then through the real worker: **Apply** into an unsaved session after a height step, export before Save, Undo, Redo, Save and a cold rebuild with every saved reference resolving, each against the peer CLI given the same UUID: every SQL cell equal (`same_model_with_explicit_new_ids`: raw payload hash validated, no new UUID to map, all cells compared, the Sketch row included), STL/FBX byte-identical. The cold rebuild's drawing is `(0,0) (60,0) (60,30) (-5,30)` — Line 3 now **65 mm**, the free corner exactly where the Sketch stored it —, the solver reports **1 degree of freedom**, its form holds `Fix H Par V` with the Parallel under the UUID it had; the saved STL, read by `read_stl_of` (not the writer), has volume ½(60+65)·30·9.5 within 1e-4 and its base vertices are the four above. **Negative control:** the pristine source and its own STL given to the same reader fail it (`Line 3 is 60 mm long, not the 65 …`), caught by the gate, which then prints `negative=held` |
| Native scenario, cylinder (`write_guessed_circle`) | the Sketch stores the circle (4.5, 0) r 1.2 and keeps Radius 10 and Fixed centre (0, 0): the accepted presentation is (0, 0) r 10, DOF 0 on a cold rebuild, one label `R 10 mm` and one marker `Fix` lower right of the **solved** centre (not of the stored one). Under Rule the marker takes the rule and rings the solved centre (`centre`); **Remove constraint** gives one removal and equals the Remove tick's request. Same Apply / export before Save / Undo / Redo / Save / cold reopen against the CLI; the saved presentation is (4.5, 0) r 10, the solver reports **2 degrees of freedom**, the cold form has no marker and keeps the Radius under its UUID, and every rim vertex of the saved STL is on the circle (4.5, 0) r 10 within 1e-3, the mesh inscribed (≥ 97 % of πr²h). Negative control held |
| Real `ci.yml` and `runtime-layout.yml` step bodies (new, and the §31M, §31L, §31K ones) | each body cut by indentation (`matrix.name` = macos, `$PWD/vendor` made absolute), only its `cargo test` lines replaced by the already-built executable under the watchdog (the test launch is the one adaptation; `cargo test` itself was not run): all eight `STEP_BODY_PASSED`; the new CI step: `22 passed; 0 failed`, every name and all 22 `FCAD_31N_*` exact-line markers; the new runtime step: both gates `1 passed; 0 failed` with their exact markers and the four exports (`constraint-markers-rectangle`, `-circle` × `.stl`, `.fbx`) written |
| Pinned reader (the real `FCAD_ANALYTIC_SESSION_FBX_DIR` loop of `tools/check-fbx-complex.sh`, cut by its own `if`/`fi`, narrowed to the seven names the four steps write, ufbx 0.23.0 rebuilt from the cached digests) | `checks=6 failures=0` for all seven; `FCAD_STL_FBX_MATCH` triangles 12, 364, 12, 364, 12, **12** (rectangle), **396** (circle); a copy of the new FBX truncated to 2 000 bytes: `checks=1 failures=2` |
| Window comparator (`/private/tmp/ferrite-31n-window/`) | exercised only on **command-line stand-ins, not window outputs**: `compare.py` `VERDICT: PASS` for the rectangle (ring `(0,0) (60,0) (60,30) (-5,30)`, DOF 1) and the cylinder (centre (4.5, 0), r 10, DOF 2); `--negative` prints `NEGATIVE CONTROL HELD` for both |
| fmt, `clippy --workspace --all-targets --all-features -D warnings` | pass (true-stub environment) |
| actionlint (all workflows), shellcheck `tools/check-fbx-complex.sh`, `tools/check-actions-run-size.py` | pass; 222 blocks, largest 20 432 of 21 000, packed argv at most 20 463 (the new `ci.yml` and `runtime-layout.yml` steps are their own small blocks) |
| Licence headers, export boundary, solver ownership, notice ownership, PlaneGCS pins, STEP corpus, `git diff --check` | pass; no manifest, lock or deny change |
| Mixed (OCCT without solver) | **N/A, reasoned:** no kernel, solver, ABI, schema, job or CLI boundary is touched (`check-solver-ownership` and `check-export-boundary` pass; the diff is app-crate UI code, tests, workflow lines, a check-script name list and docs); the two native gates assert `FERRITECAD_REQUIRE_PLANEGCS` separately and skip without a solver; no new capability, so no new refusal shape to measure; not run |
| Mutations (`target/constraint-marker-check/mutate.py`; 16 mutants, 20 gate runs, 2 of them on the native executable) | every one compiled and failed with `test <gate> ... FAILED` and a `panicked at` line of the gate it names (exit 101; never a compile failure, a skip or zero tests); the four mutated files (`sketch/marker.rs`, `sketch/accepted.rs`, `constraints.rs`, `tabs.rs`) were restored byte for byte (sha256 identical for each, frozen-tree hash identical), then the whole positive sequence was repeated on the restored tree. *Class 1, directed — the wrong rule or the wrong references:* (1) a click selects the **neighbouring rule** — the markers gate and the native rectangle gate; (2) a pair marks **only its first Line** — the references gate and the native rectangle gate; (3) a **reversed segment is normalised** — the saved gate and the references gate; (4) a Fixed End is marked as Start — the references gate. *Class 2, directed — the atomic removal, its history or the preservation of other pending changes:* (5) the removal **replaces** the other pending removals — the remove gate; (6) it **drops the pending additions** — the remove gate; (7) a repeat **appends a duplicate** — the remove gate; (8) it **bypasses `validate_edits`** — the refused gate; (9) it is **no history step** — the remove gate. *New reasons:* (10) overlap decided by the last stored rule — the hit gate and the overlap gate; (11) markers always above the numeric labels — the overlap gate; (12) selecting a rule keeps a curve selected — the sync gate; (13) a curve picked in a list keeps the rule — the sync gate; (14) a marker reached under every target — the markers gate; (15) a Fixed Line end on the wrong end — the place gate; (16) a held form keeps the markers of another version — the stale tab gate. Logs: `target/constraint-marker-check/mutations/`, `M-mutate.log` |
| Resources | 96 watchdog logs over the whole work: peak footprint **315.159 MiB** (native whole suite 283–315 MiB, stub 63–67 MiB), pressure 1 in all 338 samples, swap 0, none aborted; the 27 non-zero exits are the 20 mutant gate runs above (exit 101) and 7 red iterations of my own (the stale-`rules` and flake runs, the strict-`Eq`/`Par` rectangle assertion, the first fixtures). Every test executable (group runs, `--list`, both whole suites, every step body, every mutant gate) ran under `tools/watch-viewer-memory.py --limit-mib 1536` with a finite limit and its own log; `CARGO_BUILD_JOBS=1`; no target other than the existing native and true-stub ones; OCCT and PlaneGCS were not rebuilt (the native build is the app crate and the peer CLI only, about 2 minutes). No viewer, window, CUA or GPU test was started; no other session's process was touched |

### Found while building, and changed

* **The sixth target button silently took 17 points from two Revolve forms** (see "Layout and the 768-point budget"): every older gate
  passed, because they assert `<= 768`. It was found only because the three older groups were run on a frozen **baseline** executable
  and on the new one and their `FORM_*` lines diffed (sector 620.5 → 637.5, shaft 635.5 → 652.5). Setting `TextStyle::Button` on the
  row had no effect on `Button::selectable`; `RichText::size` did. The diff is now empty and the form gate asserts the two bottoms.
* **Two pair rules on the same two Lines share one anchor**, so my first fixture's click on `Par` always took `Eq`; the fixture now
  keeps each pair at its own place and the same-place overlaps have their own `crossing` drawing and the rectangle of the native gate.
* **`scroll_to_row` only scrolls down and where a row stands depends on how the rows above it wrapped, which depends on the random
  UUIDs' glyph widths.** The first whole-suite run failed once (`row never became visible: Parallel`) in a test that had passed in
  every group run. The tests now scroll the persisted list to its top first (`Rig::scroll_list_to_top`), and the new group was run 13
  times in a row; the whole suites have passed since.
* **A stale variable of my own test**: a second `Rig::markers()` is a second document with other UUIDs; the rules read from the first
  rig selected "another rule" in the second. Recomputed.
* **The first native run failed on my own assertion** that `Eq` and `Par` are one rectangle (they share a centre, not a width); the
  rest of that gate, and the cylinder gate from its first run, passed.
* **rustfmt** reformatted only the files of this slice; two clippy findings in test code (`needless_range_loop`, `useless_format`) were fixed.
* **Test-code visibility only**, in older files: `fn band`/`whole_line`/`within` of `sketch/dimension.rs` became `pub(super)` and
  `upper_side` was extracted from `place` (same arithmetic; the `sketch::dimension_tests` pass unchanged); the exhaustive
  `match` of `Rig::target` in `constraints/tests/canvas.rs` got one arm for `Target::Rule`. No assertion or body of an older test changed.
* **Deviations from the stated resource rules, disclosed:** the pinned-reader negative control wrote a 2 000-byte truncated FBX to
  `/tmp` for a moment (removed); an earlier whole run (`F1`) was stopped by me (SIGINT, own PIDs only, one orphan `rustc` killed by PID)
  when `cargo fmt --check` and the flake above showed it stale, and its logs are not evidence; the first stub and native builds were
  run on the unmodified tree to freeze baseline executables.


## Design choices flagged for review

* **Rule is an explicit sixth target** (not an automatic pick under Object): markers never intercept a Line pick, at the cost of
  one button; **Dimension** stays what it was.
* **Exclusive selection.** A rule names Lines or a point, so selecting it clears the curve and the angle selection (as §31M's
  angle did); the *Add …* buttons that need a Line are disabled until a Line is chosen again.
* **Markers for all seven families only on a Line polygon or one Circle**; an annulus, a mixed profile and a form with no
  drawing have none (their rows are as before). Without a drawing a marker row still selects and the editor still prepares
  the removal (as §31M's angle rows).
* **Place.** One fixed rule per family, no search: opposite pairs of a rectangle share one middle of middles, so their markers
  overlap exactly and the saved order decides; the persisted rows reach every rule.
* **Paint order follows the target** (the reached family on top) so the picked area is the visible area; the marker tint is its own.
* **ASCII words** (`H`, `V`, `Fix`, `Eq`, `Par`, `Perp`, `Col`, `-> removed`); `has_glyphs` of the canvas font is asserted for every
  one. The longer names live in the hover and the row.
* **Remove constraint is idempotent and stays enabled** while the rule is already pending removal (its sentence says so); the row's
  Remove tick and Undo take it back.
* **Native fixture.** The removed rule is an EqualLength of a fully constrained rectangle, not a Horizontal or Vertical: the freed
  corner then stays *exactly* where the Sketch stored it (−5, 30), while the solver picks an angle for a freed orientation.

## Not covered

Arcs, mixed profiles, the annulus on this canvas, new constraint families (tangency, symmetry), construction geometry, dragging
a marker, a preview of the removal, a stored marker position or placement engine, a pointer command in the CLI, a new capability,
schema or ABI, persistence of the target or of the selection.

## Not run by the author

The author ran no window, the viewer, CUA, a system dialog, a GPU test, Windows/Linux or the remote CI of this diff. **No
author window pass is claimed**, and no expected GUI result is written down in place of a window. Still open, and not closed by
any headless pass: this slice's own window pass; the §31J/§31K/§31L/§31M native-window pan, pinch and held-input cases (a native
pinch, a held middle button during a tab switch, an external window covering the canvas); Windows and Linux windows; arcs, mixed
profiles, the annulus on this canvas; tangency and symmetry; dragging; the product, Milestone 5C, §30 and the OOM investigation
(not declared fixed or reproduced). The headless 1x/2x checks use synthetic drawings (named so in the test files); the real
presentation is read only by the two native gates.

## Reviewer recipe

`/private/tmp/ferrite-31n-window/` (made by `make_inputs.py`; copies of the generator, the recipe, the comparator and the stand-in
maker are in the ignored `target/constraint-marker-check/window-recipe/`): `inputs/` holds `rectangle.fcad`, `rectangle-b.fcad` (a
byte copy), `cylinder.fcad`, `kinds.fcad`, `overlap.fcad`, `many.fcad`, `plain.fcad`, `annulus.fcad` and `SHA256.txt`; **no expected
output or screenshot exists**. `RECIPE.md` there has the exact scenario with the controls by their names (**Rule**, **Remove
constraint**, **Apply constraints**, …). `compare.py` checks the saved file and the exports against the command line given the same
request: the UUIDs (every kept one still there, every removed one gone), every SQL cell, the complete Sketch row and CBOR payload
(the real hash of each file validated first, only fresh constraint UUIDs mapped), byte-identical STL/FBX, an independent STL reader
(closed oriented mesh; `--expect-ring` for the polygon, `--expect-centre`/`--expect-radius` for the cylinder), the solver's degrees
of freedom for the very request (`--expect-dof`); `--negative` swaps the pristine input for the saved file and must print
`NEGATIVE CONTROL HELD`. The author exercised it only on command-line stand-ins (not window outputs), for both kinds.


## Independent review on macOS arm64, 2026-10-16

The implementation was reviewed without changing its product code. Fresh release CLI/viewer and
viewer test executables were built from the working tree. Native and genuine stub viewer suites each
passed **890 tests, 0 failures, 4 existing ignored subprocess entry points**. The native run printed all
24 new final execution markers; its 23 conditional skip messages and the stub's 224 messages were
counted separately, not as native execution. The stub was proved by its CMakeCache and zero OCCT or
PlaneGCS imports. Fmt, workspace clippy with all targets/features and `-D warnings`, actionlint,
shellcheck, licence, solver/export boundaries, notice ownership, pin and Actions size checks passed
(222 blocks, largest 20432 bytes, packed argv 20463 < 21000).

A fresh relocatable bundle, viewer UUID `F6DDC09C-A148-310B-8312-9203D2FB5EC6`, ran once under the
1536 MiB watchdog, PID 22785. The actual native window showed:

- the saved EqualLength chosen by its marker at the shared Eq/Par location, the two exact referenced
  Lines highlighted and the original Pair A/B choices/direction retained; preparation, draft Undo/Redo,
  repeated Remove and a cancelled system Save retained the selection and request;
- Apply into an unsaved session, actual system STL/FBX export before Save, document Undo/Redo and Save;
  the reopened drawing was the trapezoid, with no Eq and the original Parallel retained;
- zoom hiding off-canvas markers and Fit restoring the drawing and the selected Parallel;
- a single Circle's Fixed marker selecting its centre, mutually exclusive Radius selection under
  Dimension, removal of the Fixed through Apply, exports before Save, document Undo/Redo, Save and
  reopening with no Fixed marker, Rule disabled and two solver-reported degrees of freedom;
- Collinear selecting both saved Lines on the seven-family fixture; on the overlap fixture, the first
  Parallel selected by the shared marker, and the second Parallel selected through its persisted row,
  with the other two Lines highlighted.

The actual saved window files and before-Save exports passed the independent comparator against the
CLI given the two UUIDs observed in the window. Every SQL cell, whole Sketch payload/CBOR/hash and
kept UUID/reference agreed; STL and FBX agreed byte-for-byte. The independently read polygon ring was
`(0,0) (60,0) (60,30) (-5,30)` with one degree of freedom; the cylinder's rim centre was `(4.5,0)`,
radius 10 mm, with two degrees of freedom. Both pristine-substitution negative controls failed as
required. Pinned ufbx and the independent STL join passed on all 14 native analytic artifact pairs
and both actual window exports (16 strict reads/joins, zero failures). All eight pristine input hashes
were unchanged. Logs, private comparator outputs and build records are in ignored
`target/constraint-marker-review/`; private window models are outside the checkout.

The viewer quit normally, exit 0, peak footprint **200.079 MiB**, pressure 1 throughout and swap 0.
Native/stub test peaks were 269.548/156.438 MiB, also normal exits with pressure 1 and swap 0. No
post-Quit app/AX query was made; the CUA handle was dropped and termination checked through the
watchdog/PID only. The first Linux PR CI debug suite found an additional test-harness defect: the new font-only
check dropped an unapplied egui `TexturesDelta`. Release builds disable that egui assertion. The
check now explicitly clears its unused texture upload, as a headless consumer must; glyph assertions
are unchanged. A fresh debug build executed all 22 new kernel-free cases successfully under the
watchdog (no skips), followed by fmt and app clippy. The original failed/cancelled CI runs are not
counted as passes; the corrected head requires its own full CI. The first comparator shell wrapper failed before launching its guard because
macOS Bash treats an empty array under `set -u` as unbound; the corrected Python argv driver ran all
four comparisons successfully. The failed wrapper is not counted as a comparison pass.

This is a pass for the window flows listed above, not the entire recipe. Native pan/pinch,
held-input/external-cover cases, physical 1x, the 48-gon window, identical-UUID dual-tab window,
window-only structural-refusal and busy cases, and unsupported-profile window checks were not
observed here; their headless coverage remains separate. Windows/Linux windows, the broader product,
Milestone 5C, §30 and OOM remain open. Remote CI results belong to the subsequently published exact
head and are checked separately before merge.
