# §31I — A visible canvas for the Circle and Circle-with-hole forms

Status: implemented and independently reviewed, including a macOS window check and the
corrections below. Remote checks are recorded with the pull request. The sections up to
"Not in this slice" are the design note written before the
code (kept, with the one decision that changed marked); the results follow. Plan entry:
[§31I](implementation-plan.md). Base: `000c1f76f27f` (merge of
PR #112, tree `364d5f8d803e`). The numeric forms, their jobs and their CLI requests
already exist: [circle-sketch-extrude.md](circle-sketch-extrude.md),
[annular-sketch-extrude.md](annular-sketch-extrude.md), [edit-circle-copy.md](edit-circle-copy.md),
[edit-annular-copy.md](edit-annular-copy.md) and the session Apply of
[§30D](analytic-sketch-session-apply.md). This slice adds a view of their draft and
pointer gestures on it; nothing else.

## What a person can do

In **New → Circle** and **New → Circle with hole**, and in **Edit saved Circle** /
**Edit saved annulus**, a canvas (XY, mm, +X right, +Y up) sits above the number
fields. It shows the draft those fields hold: one analytic circle, or the bounding
circle and the bore with their common centre. Three handles are drawn and can be
dragged: the **centre**, the **outer radius** (on the bounding circle, east of the
centre) and the **bore radius** (on the bore, north of the centre). The numbers stay
the exact input; the canvas writes the same strings a person could type. Only the old
explicit buttons publish: **Create circle document** / **Create annular document**,
**Apply circle** / **Apply annulus**, **Save edited … copy…**. A pointer movement
starts no job, no rebuild and no file write.

## Who owns what

* **The draft strings** are the forms' own `CircleState` / `AnnulusState` in
  `sketch::Editor` (centre X/Y, radius or outer/inner radius, height). The canvas
  reads and writes those strings in place; it keeps no copy of the model and no
  accepted value. Requests, documents and the CLI keep analytic `Circle` curves with
  their UUIDs and references; the drawn circle is egui's screen polyline only.
* **The canvas state** (view: lower-left corner in mm and scale in logical points per
  mm; the gesture under way) is `sketch::analytic::Canvas`, one for the circle form
  and one for the annular form, held by the same `Editor`. New's Circle and Circle
  with hole therefore keep their own view and draft when the person switches
  Polygon/Circle/Circle with hole. Because it is part of the `Editor`, it goes with
  the tab's form (`Forms::park`/`restore`), with a New held for Quit, and with a
  published draft that is given back; `dismiss` resets it. Nothing is persisted.
* **The history** is the forms' existing bounded one (`circle_undo/redo/applied`,
  `annulus_undo/redo/applied`, 128 states). The saved forms already have it with
  **Confirm draft numbers**; the New forms gain the same stacks and **Undo draft** /
  **Redo draft** beside **Cancel draft**. No second history and no second draft
  state machine.
* **Holding the window.** `Editor::gesturing()` is also true while a canvas gesture is
  held, so the existing owners of tab switch, Open, New, Close and Quit refuse exactly
  as they do for a Line vertex drag.
* **Refusals stay where they are.** The canvas lives inside the forms: a constrained
  Circle or annulus, which the existing catalogue refuses, has no form and so no canvas.

## One gesture, one step; unconfirmed strings

A press on a handle freezes the role, the press point, the view, the centre and the
exact strings. Moves preview into the strings; nothing enters history. The release
(also outside the canvas) ends the gesture:

* strings equal to the press-time strings → nothing: no step, Redo kept;
* otherwise one step whose Undo returns **the exact strings that were in the fields at
  the press**, with their spelling (`12.50`, ` 3`, `1e1` stay as typed).

The saved forms distinguish typed text from the confirmed state (`*_applied`, set by
**Confirm draft numbers**, which keeps its meaning: it parses, rewrites the spelling,
and records one step only for a different number — compared as numbers since this
slice, because a gesture can leave a typed spelling in the confirmed state).
Decision for **typed but unconfirmed text when a drag starts**: at the release of a
changed gesture, if the
press-time strings differ from the confirmed state, those strings are first recorded
as their own step (exactly as typed, not rewritten — unlike Confirm), then the gesture
is the next step and becomes the confirmed state. So one **Undo draft** gives back the
pre-gesture strings exactly, a second gives the previously confirmed numbers; Redo
walks back. No Confirm is needed after a gesture. A no-op gesture records neither.
New forms use the same rule with the opening numbers as the first confirmed state.
Escape, window focus loss, `PointerGone`, a lost release or a form that becomes
unavailable cancel: the press-time strings come back and Undo/Redo are untouched.

## Handles and the pick rule

* Positions: centre `(cx, cy)`; outer `(cx + R, cy)`; bore `(cx, cy + r)`. A radius
  that is not positive draws no circle and offers no handle (there is nothing to
  grab); its number stays visible and refused by the form.
* Pick radius: **9 logical points**, the Line canvas's vertex radius (§25C), at any
  zoom and any pixels-per-point; only handles inside the visible part of the canvas.
* **Rule for crowded handles:** the least screen distance wins; an exact tie goes to
  **centre, then outer, then bore** (a centre move cannot change a size, so a
  collapsed circle cannot be made worse by an ambiguous press). Nothing is re-picked
  during a gesture; a new press is a new gesture.
* Centre: moves by the pointer delta divided by the frozen scale (the grab offset is
  kept: the centre does not jump under the cursor); an axis the pointer did not move
  keeps its string. Radius: the Euclidean length of (handle offset at the press +
  pointer delta) from the frozen centre — never an X projection; only that one field
  is written; the other radius and the centre keep their strings.
* Values are written as Rust's shortest exact decimal of the f64, like an unsnapped
  Line drag. No grid, object snap or guide applies on this canvas (§31C centre snap
  stays open).

## Input that cannot be drawn

A field that is empty, `-`, not a number, not finite (`NaN`, `inf`, `1e999`) or
beyond ±1e6 mm (the Line canvas's drawable bound) makes the canvas draw **no profile**
and no handle and paint which field and why (`Radius: "-" is not a number`). The old
profile is never shown as if it were the new numbers; the view is kept; no gesture can
start; the form's own message stays below as before. Numbers that can be drawn but
that the document refuses (radius ≤ 0, bore ≥ outer, a wall too thin) are drawn as
they are, in their roles (the bore stays the bore even when larger), never swapped or
clamped; the form shows the existing validation refusal and offers no
Create/Apply/Save. A gesture that leads there leaves that visible draft.

## View

The first frame with a drawable profile fits it (the bounding box of the larger
positive radius around the centre, the Line canvas's 30-point margins, centred);
**Fit profile** fits again. Typing, a gesture or a new frame never re-centre. The
canvas has a fixed size inside the floating form (it moves no workspace panel) and
reacts only to presses inside its visible part.

## Not in this slice

New CAD operations, mixed Line/Circle profiles, arcs, constraints (none is created),
document schema, CLI or job changes, height editing on the canvas, snapping or guides,
the §31C circle-centre object snap, launch-journal records of frames or drafts.
§30, Milestone 5C, the product and the OOM investigation stay open.

## Code

* `crates/ferritecad-app/src/sketch/analytic.rs` (new, the adapter): `Fields` (the form's
  strings lent for a frame), `Strings` (their exact text), `Finished` (a released gesture's
  own before/after strings), `Profile::read` (drawable or
  the named reasons), `Profile::handles`, `View::fit`, `pick` (the rule above), `Canvas`
  (view, held gesture: handle, press point, view, numbers and strings of the press),
  `Canvas::draw` (the frame's events in order: press, move, release, Escape,
  `WindowFocused(false)`, `PointerGone`, then the lost-release check, as the Line canvas
  does) and the painting (egui circles for the screen only). Three pure tests.
* `crates/ferritecad-app/src/sketch.rs`: the `Editor` holds `circle_canvas` and
  `annulus_canvas`; `gesturing()` includes them; `record_gestures` puts finished gestures
  into the existing bounded stacks; `begin()` makes New's opening numbers its first
  confirmed state; draft Undo/Redo also serve New's Circle/annular forms; the four forms
  draw the canvas first, keep their boxes non-interactive while a handle is held or a job
  runs, disable Create/Apply/Confirm/Save copy/Undo/Redo/Cancel/profile during a gesture
  and refuse a press while one of their requests is queued; Confirm compares numbers; a
  Circle/annulus form that outgrew its window asks egui for one more layout pass. Shared
  with the Line canvas, without changing its results: `canvas_point`/`canvas_mm` (the
  transform), `fit_view` (its fit), `paint_grid` and `PICK_POINTS` (9 points).
* Tests: `sketch/analytic_tests.rs` (11 real-event widget tests and helpers shared with
  the window tests), `sessions/tests/analytic.rs` (`canvas_gate`, `walls_by_reference`,
  two native gates), `sessions/tests/tabs/drafts.rs` (a native two-tab gate; a held
  handle in the tab-leave regression), and one held handle each in the existing Close,
  Quit-with-forms, Quit-with-idle-New and Open-over-New regressions.
* Workflows: `ci.yml` gains one step that runs the 14 kernel-free `sketch::analytic`
  tests in one filtered run and checks every name and marker; `runtime-layout.yml` gains
  one native step for the two canvas routes; `occt-pin.yml` greps their markers and the
  two-tab marker in its whole-suite log; `tools/check-fbx-complex.sh` reads the two saved
  FBX with the pinned reader beside the §30D ones. No new dependency, schema, kernel,
  solver, job, CLI operation or journal change.

## Executed (author, local)

All sequential, `CARGO_BUILD_JOBS=1`, existing targets and pinned libraries; pressure 1
and swap 0 in every watchdog sample; every widget, native and suite run of a test
executable went through `tools/watch-viewer-memory.py --limit-mib 1536` with a finite
time limit and its own log; no viewer, window, CUA or GPU test was started. Logs:
ignored `target/analytic-canvas-check/`.

| Check | Result |
| --- | --- |
| New tests | 3 pure (`sketch::analytic::tests`), 11 real-event widget (`sketch::analytic_tests`), 2 native session gates, 1 native two-tab gate = 17; 5 existing owner regressions extended (tab leave, Close, Quit with forms, Quit with idle New, Open over New) |
| True stub, the 14 widget/pure tests, the real `ci.yml` step body (only its `cargo test` line replaced by the same filter on the built executable under the watchdog) | exit 0; `14 passed; 0 failed`; every name and all 14 `FCAD_31I_*` exact-line markers |
| True stub, whole viewer suite (`--nocapture`) | 769 passed, 0 failed, 4 ignored; exit 0; 14 `FCAD_31I_*` markers; 214 skip lines, among them the three new native gates (`skipped: the session gates need Open CASCADE` ×2, `skipped: the picture of a ring in a tab needs OCCT`), which are therefore **not** geometry passes there; no `FCAD_31I_NATIVE`/`TABS` marker in that log |
| Stub proof | bridge `CMakeCache`: `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE`, `OpenCASCADE_DIR` absent; build warning "Open CASCADE was not usable"; `otool -L` of the fresh test executable: 0 × libTK, 0 × planegcs |
| Native (OCCT + PlaneGCS, release), whole viewer suite (`--nocapture`) | fresh test executable and peer CLI each import 50 libTK and libplanegcs; 769 passed, 0 failed, 4 ignored; exit 0; peak footprint 290.8 MiB on the final tree (276.3 MiB on the run before the last test addition); all 17 `FCAD_31I_*` markers; 23 skip lines (stub-only, no-solver-only and real-GUI-artifact alternatives, none of this slice); the §30D analytic gates and the five extended owner regressions pass in it |
| `occt-pin.yml` marker lines | the three new `grep -Fxq` lines (and the existing §31E one) pass on that whole-suite log |
| `runtime-layout.yml` new native step (the real body, `matrix.name` = macos, `$PWD/vendor` made absolute, only its `cargo test` lines replaced by the built release executable under the watchdog) | exit 0; both gates `1 passed; 0 failed` with their markers; `circle-canvas` and `annulus-canvas` STL/FBX written |
| `tools/check-fbx-complex.sh` analytic hook (its real `if … fi` block, the reader built here) | on the whole-suite artifacts: `checks=6 failures=0` and an STL↔FBX triangle match for all five (`circle-session`, `annulus-session`, `annulus-swapped-session`, `circle-canvas`, `annulus-canvas`); `FCAD_ANALYTIC_SESSION_UFBX_EXECUTED` |
| Native routes (exact, under the watchdog) | `FCAD_31I_NATIVE_CIRCLE_EXECUTED walls=1 sql=cli export=before_save undo=1 redo=1`, `FCAD_31I_NATIVE_ANNULUS_EXECUTED walls=2 …`, `FCAD_31I_TABS_EXECUTED`; 1 passed each, no skip |
| Native scenario | stored Circle/annulus (12.5, −7.25), R 10.5, bore 4.75, height 15.25, made by the CLI. Through the widgets: centre dragged (−31.25, +18.75) points, outer handle (−12.5, −9.75), bore handle (−7.25, +4.5); the canvas wrote centre (8.639705709107599, −9.56617657453544), R 9.036505702296449, bore 4.288671237958911; Apply circle/annulus on the real worker; the accepted model equals the CLI's `edit-circle`/`edit-annular` copy given those numbers in every SQL cell; every other table, every UUID, name, reference and the height kept; export before Save equals the CLI's STL/FBX byte for byte and differs from the original; Undo gives the original's exports, Redo the edited; the source is byte-identical until Save; Save equals the CLI copy in every cell; a cold reopen resolves every reference; the wall under the boundary's saved reference is a cylinder of R and the one under the bore's of r (within 1e-9) |
| Independent readers | STL by a separate Python script (`python3 -I target/analytic-canvas-check/stl_measure.py`): Circle 376 triangles, closed, every vertex at R about the dragged centre, volume 3909.345889 = 0.999271 of πR²h; annulus 644 triangles, closed, 1082 vertex uses at R and 850 at r, none elsewhere, volume 3029.495100 = 0.999498 of π(R−r)(R+r)h; negative control (the stored numbers) fails with 1932 of 1932 vertex uses off. FBX by pinned ufbx 0.23.0 (reader built from the cached digests): `--identity` `checks=6 failures=0` for both, `--triangles` + `tools/fbx/stl-matches-fbx.py`: 376 and 644 triangles matched; a copy truncated to 2000 bytes is refused (`checks=1 failures=2`). Two separate native runs give byte-identical STL; their FBX differ only in the three embedded UUIDs of each run's freshly created source |
| fmt, `clippy --workspace --all-targets --all-features -D warnings` | pass (true-stub env); one `type_complexity` in a test helper was fixed on the way |
| actionlint, shellcheck `tools/check-fbx-complex.sh`, `tools/check-actions-run-size.py` | pass; 213 blocks, largest 20 432 of 21 000 bytes (not this slice's; the canvas gates were moved to their own step when adding them to the §30D step would have made it 21 346) |
| Licence headers (507), export boundary, solver ownership, notice ownership, PlaneGCS pins, STEP corpus, `git diff --check` | pass; no manifest, lock or deny change |
| Mixed (OCCT without solver) | **N/A, reasoned:** no kernel, solver, ABI or job boundary is touched (`check-solver-ownership` passes) and the canvas needs neither; not run |

### Mutations (copy of the existing harness, `target/analytic-canvas-check/mutate.py`; every one compiled and failed on an executed assertion; restored byte for byte; positive repeat green)

SHA-256 of `sketch/analytic.rs` and `sketch.rs` before the first and after the last:
`a8b8bf8b…`, `ef1ecb58…` (identical).

1. **The centre jumps under the pointer** (the press measured from the handle's own point):
   the New Circle gate failed, `left ["4.764705882352941", "2.8823529411764706"]`, `right
   ["4.411764705882353", "2.6470588235294117"]`.
2. **The bore's handle writes the outer radius:** the New annulus gate failed (`outer:
   "5.228…", bore: Some("4")` against `outer: "11.504…", bore: Some("5.228…")`), and the saved
   gate (`outer "6.372…"` against `"10.5"`).
3. **Undo loses the typed string** (the press keeps parsed numbers): the unconfirmed-text
   gate failed, `centre ["12.5", …]` against the typed `["12.50", …]`.
4. **A held gesture's form takes text** (boxes left interactive): the blocked gate failed,
   `"1129"` against `"112"` — the focused box would have taken the key during the drag.

### Found while building, and changed

* **A taller form clipped a new line for a frame.** The existing §30D gate "the document's
  last outcome is readable inside the open form" failed: egui sizes an auto-sized window
  from its last frame, and the old forms were shorter than egui's default window, which
  left room. With the canvas, a line that appears (the session's outcome after a refused
  Apply) was cut off, and an idle window would keep it so. The Circle/annulus forms now ask
  egui for one more layout pass when their content outgrows the window but fits on the
  screen (`request_discard`, egui's own mechanism, which it already uses for every new
  `Grid`). The old gate passes unchanged.
* **Confirm after a gesture.** A gesture leaves the fields it did not move in their typed
  spelling and is the confirmed state, so Confirm's textual comparison would have made a
  step for a mere respelling; it now compares the numbers (no step for an equivalent
  spelling, as §30D states).
* **Focus.** A box made non-interactive while a handle is held also loses its keyboard
  focus (it no longer registers for focus); typing resumes after clicking the box again.
* A press beside the canvas inside the floating form may move the form (an egui window
  drag); tests read positions again after it.

### Design choices flagged for review

Outer handle east, bore handle north (they separate even when the radii are close);
tie order centre → outer → bore; a radius that is not positive has no handle; values are
written as the shortest exact decimal (like an unsnapped Line drag, no rounding to 0.001
as a Line click does); no grid, object snap or guide on this canvas; text typed and not
confirmed is recorded as its own step when a changed gesture ends (so two Undo steps lead
past such a drag); New forms gain **Undo draft**/**Redo draft** beside **Cancel draft**
(typed text alone is still no step there); the canvas is 510 × 230 points, fitted centred
with the Line canvas's scale rule; boxes are also non-interactive while a job runs (the
§31E trap, now closed for these forms); text that cannot be drawn draws no partial
profile; a profile with no positive radius is framed as 1 mm about its centre.

## Author handoff: not done, not claimed

Not run by the author: a window, the viewer, the CUA, any GPU test, Windows or Linux, and
the CI of this diff; no window pass is claimed. Not in scope and still open: the §31C
circle-centre object snap (the centre is drawn here, but it is no snap candidate on either
canvas), snapping and guides on this canvas, quadrants, mixed Line/Circle profiles, arcs,
constraints made from the canvas, a pointer command in the CLI; §30, Milestone 5C, the
product and the OOM investigation.

Base CI (not this diff), read at the start and again before the handoff: PR #112's final
head `93441db` passed 15/15 checks. The post-merge runs of `000c1f7` are separate: `CI`
38699080869, `planegcs pin` 38699080913, `rust notices` 38699080824 and both SBOMs completed
with success; `combined runtime layout` 38699080874 was still `in_progress` and is not
counted as evidence of anything.

## Independent macOS window recipe (the author does not run it)

Inputs only, outside the checkout, made by `/private/tmp/ferrite-31i-window/make_inputs.py`
(SHA-256 `634671de…76d5`): `python3 -I make_inputs.py <checkout>/target/release/ferritecad
<new dir>` (after `source target/launch-journal-check/native-env.sh`) writes `circle.json`,
`circle.fcad` (centre (12.5, −7.25), R 10.5, height 15.25), `annulus.json`, `annulus.fcad`
(the same with a bore of 4.75) and `SHA256.txt`; no expected window output and no
screenshot exist. `compare.py` (SHA-256 `620ed05e…0fc`) beside it compares what the window
saved and exported with the CLI. Work on copies; run the viewer only through
`tools/watch-viewer-memory.py --limit-mib 1536 --seconds <limit>`, pressure normal; logs
into `target/analytic-canvas-check/` of the checkout under review.

1. **New → Circle.** **Create sketch + Extrude…** → **Circle**: the canvas shows a blue
   circle of radius 10 about (0, 0), centred, with a cross at the centre and a square on the
   east side. Hover each: a ring and `Centre (0, 0) mm` / `Radius 10 mm`. Press a few points
   off the cross and drag diagonally: the circle follows the hand, the cross keeps its offset
   from the pointer, Center X/Y change, Radius does not. Release: one **Undo draft** puts the
   strings back exactly, **Redo draft** returns them. Drag the square diagonally: Radius is
   the distance from the centre (not the horizontal movement). Press and release without
   moving: nothing changes, Redo stays. Press, move, **Escape**: back as it was. Type `-` in
   Radius: the canvas says `Not drawn: Radius: "-" is not a number` and draws nothing, the
   form shows its own error; a drag starts nothing; type a number back. **Fit profile**.
2. **Switch profiles.** Choose **Circle with hole**: its own canvas (blue boundary, orange
   bore, square and diamond handles); drag the outer, then the bore, then the centre: each
   writes only its own fields. Go to **Line polygon** and back to **Circle** and **Circle
   with hole**: each kept its numbers, view and history. Drag the diamond outward past the
   blue circle: the bore is drawn larger, still orange; the form says the inner radius must
   be smaller and offers no Create; Undo recovers. **Create annular document**: a new
   Untitled tab holds exactly the shown numbers. **Save** opens a dialog: **Cancel** writes
   nothing and the tab stays unsaved; **Save** again to a new file outside the checkout.
3. **Saved Circle.** Open a copy of `circle.fcad`, **Edit circle …**. Type `12.50` in
   Center X (do not confirm), then drag the radius handle. **Undo draft** → Center X reads
   exactly `12.50` and Radius `10.5`; **Undo draft** again → the stored numbers; **Redo** twice.
   **Confirm draft numbers** is disabled right after a drag (the drag is confirmed). Press
   **Apply circle**: no dialog; the model changes, the title shows unsaved. Export STL and FBX
   **before Save** (keep them). Document Undo/Redo once each. **Save As…** → Cancel (nothing
   written), then **Save**.
4. **Saved annulus.** The same with a copy of `annulus.fcad` and **Edit annulus …**: drag the
   centre, the outer and the bore (each changes only its own field), **Apply annulus**, export
   before Save, **Save**.
5. **Two tabs.** Open two copies of `annulus.fcad`; open the form in each; drag in tab A, drag
   and **Fit profile** in tab B; switch several times: each tab shows its own view, numbers and
   draft Undo depth; Undo in A changes nothing in B. Optionally, hold a handle and press
   Cmd+Q: the window does not quit (the status asks to wait); the release ends the drag as
   one step.
6. **Refusal and recovery.** Type `-2` in Outer radius: no blue circle and no square, the
   form's refusal is shown, the bore and centre stay; Undo or type a number back.
7. **Comparison.** For each saved file: `python3 -I compare.py <checkout>/target/release/ferritecad
   <the input copy you started from> <the saved file> <the STL exported before Save> <the
   FBX exported before Save> <new work dir>` must print `VERDICT: PASS` (every SQL cell but
   `meta.modified_at`, the STL/FBX of the saved file against the CLI copy's, and the exports
   before Save against them); the same with `--negative` must print `NEGATIVE CONTROL HELD`.
   Read the FBX with the pinned reader (`tools/check-fbx-complex.sh` hook lines or the reader
   built from `tools/unity-fbx-smoke/scripts/`), and measure the STL independently (centre
   and radii from `inspect --json`). Optionally repeat step 1 on a 2x display. Record the
   watchdog's peak, pressure, swap and exit; do not call the OOM investigation closed.

## Independent review and window results

Three defects were reproduced by executed assertions and corrected:

* A released gesture followed by another held gesture in the same frame confirmed the
  latter's preview. Cancelling it then left a phantom confirmed radius and an extra Undo
  step. `Finished` now captures both endpoints at release; history never infers a completed
  edit from the fields at the end of the frame. The packed-events gate covers Circle and
  annulus with a second held gesture, cancellation and the next real edit.
* A move and press over a floating form could reach the camera before egui processed the
  new pointer position. The window now queries the painted areas using the latest OS
  position, converted from physical pixels to logical points. A drag that starts in the
  viewport still belongs to it across the form. The blocked-input gate exercises this
  routing for both forms at 1x and 2x; the final window run kept the 3D camera unchanged
  through the three annular handle drags.
* JSON request decoding rounded `11.017646992297319` one ULP differently from numeric
  field input. CLI `serde_json` now uses `float_roundtrip`; the actual Circle request DTO
  has a bit-exact regression. No version or field changed. Cargo.lock, package/edge
  inventories and notices remain unchanged; the SBOM and notice regeneration gates pass.

The final native viewer suite passes 769 tests (four existing ignored cases; conditional
returns remain separate from native evidence). All 17 new markers execute. CLI unit,
Circle/annular edit and JSON v1 suites pass 10 + 6 + 5 + 13 tests. Fmt, workspace clippy
with all targets/features and warnings denied, actionlint, shellcheck, Actions run-size,
licence and boundary checks pass. Logs are in ignored `target/analytic-canvas-review/`.

Three owned macOS bundle processes ran under the 1536 MiB watchdog and exited normally:
peaks 209.626, 242.298 and 204.204 MiB; pressure 1 and swap 0 throughout. The window
exercised saved Circle typed `12.50`, radius/centre drags, exact-string Undo, document
Undo/Redo, Apply, export before Save, Save As Cancel and Save; saved annulus centre and
both radii, an oversized bore refusal and recovery; New Circle gestures and invalid `-`;
profile switching and New annulus creation, Save Cancel and Save. The final bundle also
preserved a Circle form's fitted view and an annular form's unfitted view and numbers
across tab switches. Escape/focus-loss during a held handle and the full crowded-handle
matrix were verified by widget gates, not claimed as window observations.

Real saved Circle and annulus SQL cells equal the CLI copies except `meta.modified_at`;
their pre-Save STL/FBX bytes equal the saved and CLI exports. Original-number exports
differ, and input files remain byte-identical. Independent STL measurements found:

| Model | Triangles | Signed volume, mm³ | Mesh / analytic volume |
| --- | ---: | ---: | ---: |
| Saved Circle, R 9.101862939626594, h 15.25 | 376 | 3966.099765 | 0.999271 |
| Saved annulus, R 9.427973129520453, r 4.300467688151963, h 15.25 | 652 | 3370.822388 | 0.999514 |
| New annulus, R 10, r 4.5091218580557175, h 10 | 668 | 2501.707551 | 0.999548 |

All three meshes are closed and have every vertex on its expected radius about the
published centre. Pinned ufbx 0.23.0 reads each actual FBX with 6 checks and 0 failures;
its triangle stream matches that model's STL. Window artifacts are outside the checkout
in `/private/tmp/ferrite-31i-review/`. The handoff's separate temporary input/compare
scripts did not survive the reboot; the reviewer generated fresh inputs and compared
the actual window outputs independently. No window, Linux/Windows or mixed-build result
is inferred from the author's headless tests. The cause of the historical OOM remains open.

## Manifest

New: `crates/ferritecad-app/src/sketch/analytic.rs`,
`crates/ferritecad-app/src/sketch/analytic_tests.rs`, this file. Changed:
`crates/ferritecad-app/src/sketch.rs`, `crates/ferritecad-app/src/sessions/tests/analytic.rs`,
`crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs` and its `close_form.rs`,
`quit_forms.rs`, `quit_new.rs`, `open_over_new.rs`, `.github/workflows/{ci,occt-pin,runtime-layout}.yml`,
`tools/check-fbx-complex.sh`, `docs/implementation-plan.md`, `docs/cli-capabilities.md`.
The diffstat is in the handoff report.
Review corrections also touch `crates/ferritecad-app/src/main.rs`,
`crates/ferritecad-ui/src/input.rs`, `crates/ferritecad-cli/Cargo.toml` and
`crates/ferritecad-cli/src/edit_circle.rs`.
