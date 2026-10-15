# §31J — Zoom and pan on the Sketch canvases

Status: reviewed implementation. The independent macOS window check covered cursor zoom,
dragging at the changed scale, Fit, draft/tab ownership and publication of saved Line, Circle
and annular profiles. Native-window pan and pinch remain unobserved; their widget gates execute.
The original design and author evidence below are retained separately. Plan entry: [§31J](implementation-plan.md). Base: `b4286163045b` (merge of
PR #113, tree `e64d0c38ce0b`). The three canvases already exist: the Line polygon canvas
(New and every saved Sketch base: Extrude, Revolve, Fillet, Cut, Chamfer histories), the Circle
canvas and the Circle-with-hole canvas
([analytic-sketch-canvas.md](analytic-sketch-canvas.md)). This slice moves their **view** and
nothing else.

## Decision

**Gestures.** Each canvas reads what the window already gives egui:

| Input | Effect |
| --- | --- |
| mouse wheel, trackpad scroll (vertical part of any `MouseWheel`, any unit) | zoom about the cursor |
| trackpad pinch (`Event::Zoom`) | zoom about the cursor |
| hold the **middle** button and drag | pan |
| hold **Shift** and drag with the primary button | pan (for a trackpad); Shift + primary never edits |
| the existing **Fit drawing** / **Fit profile** (and the Line's **Reset view**) | the explicit way back |

The primary button alone keeps its meaning: add a point, move a vertex, move a handle.
Space is not used (it types into a focused number box); the secondary button is left
free. Horizontal scroll is ignored. A short hint is painted on the canvas itself (no
form height), and says "view locked" while an edit gesture holds it.

**Coordinates.** A view is the lower-left corner in mm and logical points per mm (the
existing `canvas_point` / `canvas_mm`). Events are egui logical points (egui-winit has
already divided OS physical pixels by `pixels_per_point`); picking radii, snap and guide
apertures stay in logical points, so zoom changes how many mm they cover and nothing
else. Zoom by factor `f` about screen point `a`: the document point under `a` before is
under `a` after (`minimum' = under(a) − (a − corner)/scale'`). Pan: the document point
under the press stays under the pointer (`minimum = grab − (p − corner)/scale`), so it
cannot drift whatever the event batching.

**Finite bounds.** Scale stays within **[1e-5, 1e8] points per mm**: the largest
drawable extent is 2·10⁶ mm (±10⁶ mm, the drawable bound), which the old Fit shows in
170–450 points (≥ 8.5·10⁻⁵ pt/mm), so 10⁻⁵ is below any Fit; the old Fit of a degenerate
profile reaches 4.5·10⁷ pt/mm (extent floor 10⁻⁵ mm), so 10⁸ is above any Fit, and one
point there is 10⁻⁸ mm, still ≫ the f64 spacing (1.2·10⁻¹⁰ mm) at 10⁶ mm. The corner
stays within ±10⁹ mm (arithmetic safety, not UX). One event multiplies the scale by at
most 8 or 1/8; a non-finite, zero or negative delta, factor or position changes nothing.
A step that would cross a limit stops at the limit with the cursor anchor kept; at the
limit the view is bit-identical (no jump). If preserving the cursor anchor would cross the arithmetic corner bound, the whole zoom step is ignored. Nonzero wheel movement on Start, Move and End is applied; Cancel carries no movement. Wheel: exp(points / 200), a line is 40 points.

**Who may navigate, and when.** Only the form that is enabled (no running job), has
nothing queued, has the window focus, and only with the pointer on the visible,
uncovered part of its canvas (the predicate a press uses). Over a panel, another
form, outside the canvas, while a job runs or a request waits: the event is not taken,
so the 3D camera/pick/hover decision stays where it is (`window_pointer_claimed`: a
floating form is a non-background layer, so wheel, pinch and presses over it never
reach the camera; a 3D drag begun in the viewport still continues across a form).

**Competition with editing.** An edit gesture freezes its view at the press (it
already copies it); while it is held, wheel, pinch, pan presses and Fit change nothing.
Within one frame events keep egui's order: a navigation event is decided where it
stands (held gesture → ignored) but takes effect **at the end of the frame**, and not at
all if an edit gesture is held by then; every press, move, release and click of the frame
is read against the view the person saw. A pan is itself a held gesture of the window
(`Editor::gesturing()`), so tab switch, Open, New, Close and Quit refuse as for a vertex
drag. While a pan is held no edit gesture starts; zoom is allowed. Escape restores the
view of the press; focus loss, `PointerGone`, a lost release, a release outside the
canvas, a form that becomes unavailable and a canvas that stops being drawn end it where it is.
A pan never edits.

**State and owner.** `sketch::navigation` (new) owns the arithmetic and the small
per-canvas state (the held pan, the last pointer). The Line `Canvas`, `circle_canvas`
and `annulus_canvas` hold their own, so each keeps its own view, and they travel with
the `Editor` (`park`/`restore`, New held for Quit, Open, Close, `dismiss`). The view is
not saved into the document. Navigation changes no string, selection, draft, history,
request, version or constraint, starts no job, and publishes nothing; Fit is never
applied by itself.

## What a person can do

On the Line polygon canvas (**Create sketch + Extrude…** → Line polygon, and **Edit saved
Sketch** for every supported base), the Circle canvas and the Circle-with-hole canvas (New and
saved), turn the wheel, scroll with two fingers or pinch to zoom about the cursor; hold the
middle button, or Shift with the left button, and drag to pan; press **Fit drawing** /
**Fit profile** (the Line also has **Reset view**) to see the whole drawing again. The lower right
of the canvas says "Wheel or pinch: zoom · Shift+drag or middle-drag: pan", "Panning · Esc puts
the view back", "View locked while you drag" (a vertex or handle is held) or "View locked while the
form is busy". Nothing else changes: not a coordinate, radius, height, selection, version,
constraint or history; no job, rebuild, file or Confirm. Typed text keeps its spelling (`12.50`),
valid or not.

## Who owns what

* **The view** of each canvas is that canvas's own: the Line `Canvas.minimum` / `.scale` (one
  canvas for New and every saved base), and `View` of `circle_canvas` and `annulus_canvas`. All three
  live in the `Editor`, so they go with the tab's form (`Forms::park`/`restore`), a New held for
  Quit, an Open over New and a given-back publication, and `dismiss` resets them. The document
  never sees a view; nothing is persisted.
* **`sketch::navigation`** (new, no draft, history, selection or request in it) owns the view
  arithmetic (`zoomed`, `placed`, the limits), `Navigation` (a held pan, the last pointer, the
  frame the canvas was last drawn in) and the painted hint. A canvas lends a `Surface` (its `Ui`,
  rectangle, visible part, whether the form is open and whether a gesture freezes the view) and a
  `View` for one event, in order, and learns whether the event belonged to navigation.
* **The window's ownership of gestures** is `Editor::gesturing()`: a held pan of any of the three
  views is in it, so tab switch, Open, New, Close and Quit refuse as for a vertex drag. It is not
  in `analytic_gesturing()`, which disables the forms' boxes and buttons for a held handle: a pan
  edits nothing, so Undo/Redo and the availability of Create/Apply/Save are what they were.

## Events of one frame

egui hands a frame's events in order and the canvas reads them in that order:

* a **press, move, release or click** is read against the view the person saw (the view at the start
  of the frame); an edit gesture copies that view at its press, as before;
* a **wheel, pinch or pan** event is decided where it stands: ignored if a vertex or handle is held
  at that point, or if the pointer is not on the visible, uncovered canvas of an open form, and
  otherwise applied to a working copy of the view; the copy replaces the view at the end of the
  frame, and is dropped if a gesture is held by then (a wheel before a press in the same frame);
* the pointer of a wheel or pinch is the last position an event of the frame gave, seeded from egui's
  own when the frame has no pointer event;
* a Shift press that becomes a pan is never an edit and its click (egui's click of a press and
  release without movement) places and selects nothing on the Line canvas; while a pan is held no
  press starts an edit; while an edit gesture is held no pan starts.

## Not in this slice

New CAD operations, a command engine, keyboard or double-click zoom, inertia, rotation, snap or
guide families, the §31C circle-centre object snap, guides on the analytic canvas, mixed profiles,
arcs, constraints, solver/FFI/schema/CLI pointer API, launch-journal records of frames or views.
§30, Milestone 5C, the product and the OOM investigation stay open.

## Code

* `crates/ferritecad-app/src/sketch/navigation.rs` (new): the above. Four pure tests.
* `crates/ferritecad-app/src/sketch.rs`: the Line `Canvas` holds a `Navigation` and a `pan_click`;
  `draw` takes `queued`, reads the navigation of each event first, keeps the working view and
  commits it before painting; `gesturing()` includes the three pans; `Editor::draw` ends the pan
  of a canvas that was not drawn in the frame before. Shared with the other canvases and unchanged:
  `canvas_point`/`canvas_mm`, `fit_view`, `paint_grid`, `pointer_over`, `PICK_POINTS`.
* `crates/ferritecad-app/src/sketch/analytic.rs`: `View` moved to `navigation` (its `fit` stays);
  `Canvas` holds a `Navigation`, reads it in the same ordered loop and commits the same way.
* Tests: `sketch/navigation_tests.rs` (12 real-event widget tests and the helpers the owner
  and native gates share), `sessions/tests/analytic.rs` (two native gates; the result checks of the
  §31I gates became two functions both call), `sessions/tests/tabs/drafts.rs` (a native two-tab
  gate) and a held pan in the five owner regressions (tab leave, Close, Quit with forms, Quit with an
  idle New, Open over New).
* Workflows: `ci.yml` gains one step that runs the 16 kernel-free `sketch::navigation` tests in one
  filtered run and checks every name and marker; `runtime-layout.yml` runs the two native gates in
  the §31I step; `occt-pin.yml` greps their markers and the two-tab marker; `tools/check-fbx-complex.sh`
  reads the two exports with the pinned reader beside the §30D and §31I ones. No dependency, schema,
  kernel, solver, job, CLI or journal change.

## Executed (author, local)

All sequential, `CARGO_BUILD_JOBS=1`, existing targets and pinned libraries; pressure 1 and swap 0 in
every watchdog sample; every widget, native and suite run of a test executable went through
`tools/watch-viewer-memory.py --limit-mib 1536` with a finite time limit and its own log; no viewer,
window, CUA or GPU test was started. Logs: ignored `target/sketch-navigation-check/` (the final tree's
are `final2-*` and `guard*/final2-*`; the first full pass of the earlier tree found the two defects
listed below, and its numbers are not used here).

| Check | Result |
| --- | --- |
| New tests | 4 pure (`sketch::navigation::tests`) + 12 real-event widget (`sketch::navigation_tests`) = 16 kernel-free, 2 native session gates, 1 native two-tab gate = 19; a held pan added to 5 existing owner regressions (tab leave, Close, Quit with forms, Quit with an idle New, Open over New) |
| True stub, the 16 tests, the real `ci.yml` step body (only its `cargo test` line replaced by the same filter on the built executable under the watchdog) | exit 0; `16 passed; 0 failed`; every name and all 16 `FCAD_31J_*` exact-line markers |
| True stub, whole viewer suite (`--nocapture`) | 788 passed, 0 failed, 4 ignored; exit 0; 16 `FCAD_31J_*` markers; 217 skip lines (§31I reported 214): the three new native gates are the added three, so they are **not** geometry passes there; no `FCAD_31J_NATIVE`/`TABS` marker in that log |
| Stub proof | bridge `CMakeCache` of the build: `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE`, `OpenCASCADE_DIR=/private/tmp/absent-occt-true-stub`; build warning "Open CASCADE was not usable"; `otool -L` of the fresh test executable: 0 × libTK, 0 × planegcs |
| Native (OCCT + PlaneGCS, release), whole viewer suite (`--nocapture`) | fresh test executable and peer CLI (rebuilt before the runs: cargo found it out of date) each import 50 libTK and libplanegcs; 788 passed, 0 failed, 4 ignored; exit 0; peak footprint 299.2 MiB; all 16 + the 2 native + the tab `FCAD_31J_*` markers; 23 skip lines (the count §31I reported; none of this slice's gates, which all printed their markers) |
| `occt-pin.yml` marker lines | all 11 `grep -Fxq` lines (the three new among them) pass on that whole-suite log |
| `runtime-layout.yml` extended §31I step (the real body, `matrix.name` = macos, `$PWD/vendor` made absolute, only its `cargo test` lines replaced by the built release executable under the watchdog) | exit 0; the two §31I and the two new gates `1 passed; 0 failed` each with their markers; `circle-navigated`, `annulus-navigated` STL/FBX written |
| Native scenario | stored Circle/annulus (12.5, −7.25), R 10.5, bore 4.75, height 15.25, made by the CLI. Through the widgets: zoomed in about the centre handle (two wheel lines), panned (20, 12) points, **Fit profile** (the opening view back, bit for bit), zoomed and panned again (−15, 10): that alone left every cell of the session's file and of the source, the source's bytes, the command line's STL/FBX of the source and the version the form was read from as they were, and every string, history and request untouched. Then at that scale: centre dragged (−31.25, +18.75) points, outer handle (−12.5, −9.75), bore handle (−7.25, +4.5); the canvas wrote centre (9.912367317491665, −8.802579609505), R 9.499316841861885, bore 4.418353706455293 (within 1e-3 mm of the computed drag over the navigated scale; §31I's numbers at the fitted scale differ); Apply on the real worker; the accepted model equals the CLI's `edit-circle`/`edit-annular` copy given those numbers in every SQL cell; every other table, UUID, name, reference and the height kept; export before Save equals the CLI's STL/FBX byte for byte and differs from the original; Undo gives the original's exports, Redo the edited; the source is byte-identical until Save; Save equals the CLI copy in every cell; a cold reopen resolves every reference; the wall under the boundary's saved reference is a cylinder of R and the one under the bore's of r (within 1e-9) |
| Independent readers | STL by a separate Python script (`stl_measure.py`): Circle 384 triangles, closed, every vertex at R about the dragged centre, volume 4320.168849 = 0.999301 of πR²h; annulus 656 triangles, closed, 1104 vertex uses at R and 864 at r, none elsewhere, 0.999512 of the analytic volume; negative control (the §31I numbers) fails with 1152 of 1152 vertex uses off. FBX by pinned ufbx 0.23.0 (reader rebuilt from the cached digests): `--identity` `checks=6 failures=0` for the four canvas exports, `--triangles` + `tools/fbx/stl-matches-fbx.py`: 376, 644, 384 and 656 triangles matched; a copy truncated to 2000 bytes is refused (`checks=1 failures=2`) |
| fmt, `clippy --workspace --all-targets --all-features -D warnings` | pass (true-stub env) |
| actionlint, shellcheck `tools/check-fbx-complex.sh`, `tools/check-actions-run-size.py` | pass; 214 blocks, largest 20 432 of 21 000 (not this slice's: the new step is small and the two gates were added to the existing §31I native step) |
| Licence headers (509), export boundary, solver ownership, notice ownership, PlaneGCS pins, STEP corpus, `git diff --check` | pass; no manifest, lock or deny change |
| Window recipe inputs and comparator | `make_inputs.py` wrote seven fresh inputs; `compare.py` was exercised on CLI-made stand-ins (not window outputs) for a Line polygon, a Circle and an annulus: `VERDICT: PASS`, and with `--negative` `NEGATIVE CONTROL HELD` |
| Mixed (OCCT without solver) | **N/A, reasoned:** no kernel, solver, ABI or job boundary is touched (`check-solver-ownership` passes) and the navigation needs neither; not run |

### Mutations (`target/sketch-navigation-check/mutate.py`; every one compiled and failed on an executed assertion of the gate listed; restored byte for byte; positive repeat green; final tree)

SHA-256 of `sketch.rs`, `sketch/analytic.rs` and `sketch/navigation.rs` before the first and after the
last: `7812282c…`, `0addf697…`, `35a0979e…` (identical at each restore).

1. **Zoom about the middle of the canvas instead of the cursor** (the directed one): the zoom gate
   failed (the point under the cursor moved).
2. **The scale changes during an edit drag** (the directed one: the working view replaces the real
   one while a vertex or handle is held, and the wheel is not held off): the frozen-gesture gate and
   the packed-frame gate failed.
3. The wheel is not held off inside a frame whose gesture is held: the packed-frame gate failed.
4. The working view replaces the real one while a press is held at the end of the frame: the
   packed-frame gate failed (the frozen-gesture gate does **not**: its wheel is held off by the other
   guard — the two guards have a gate each).
5. The pan's vertical sign: the pure pan test and the widget pan gate failed.
6. The wheel ignores cover and the outside of the canvas: the refusal gate failed.
7. A Shift pan's click places a point on the Line canvas: the no-movement pan gate failed (the first
   widget set missed this: its pans always moved, and egui only calls a press without movement a click).
8. A held pan does not hold the window: the pan gate failed.
9. A pan on a canvas that is no longer drawn is never ended: the profiles gate failed.
10. A pan survives the focus loss and a job starting under it: the pan-ends gate failed.

### Found while building, and changed

* **A pan was first put in `analytic_gesturing()`**, which also disables the forms' boxes and buttons
  (profile choice, Undo/Redo draft, Cancel) for a held handle; a pan edits nothing, so it now holds the
  window through `gesturing()` only and the availability of publication is what it was.
* **A pan could outlive its canvas.** Its buttons stay enabled, so a middle-button hold plus a left
  click on another profile left a pan on a canvas that is no longer drawn, and `gesturing()` refused
  Quit for good. `Editor::draw` now ends the pan of a canvas that was not drawn in the frame before.
* **An existing regression compares the whole `Editor` through `Debug`** (the returned New form after
  Undo/Redo): the navigation's frame bookkeeping (last pointer, frame number) made two equal editors
  differ. `Navigation`'s `Debug` shows only the held pan.
* **Two mutants survived the first set** (items 4 and 7 above) and got their own gates.
* The tab gate called `round` on the tab already shown; the local final-run driver reused a watchdog
  log directory name (the watchdog refuses an existing log and the step ended silently) and, run once
  while its mutation stage built, a second watchdog run was refused with rc 125, as the guard is meant
  to. None of these is in the repository.

### Design choices flagged for review

Wheel, two-finger scroll and pinch all **zoom** (a trackpad's two-finger scroll does not pan; its
horizontal part is ignored); pan is a held middle button or Shift + primary (Space types into a focused
box, the secondary button is left free); a reserved pan press on the canvas never edits, even when a queued request or an edit gesture prevents navigation; Escape restores the view of the press while every other end keeps the view reached; a pan holds
the window but disables no control, so typing during a pan is possible; zoom is allowed during a pan;
a navigation event takes effect at the end of the frame and a wheel before a press that is still held
at the end of that frame is dropped; the wheel's pointer is the last position an event of the frame
gave (seeded from egui's when the frame has none, so the first wheel of a frame that moves the pointer
after it, on a canvas not drawn before, can anchor at the position of the frame before); scale limits
[1e-5, 1e8] points per mm, corner ±1e9 mm, one event at most 8x, one e-fold per 200 points of scroll, a
line is 40 points, a page is the canvas height; the hint is 11-point text on a dark plate at the lower
right of the canvas (no form height) in four states; the Line's **Fit drawing** and **Reset view** and
the Circle/annulus **Fit profile** are disabled while a pan or a gesture is held, as they were during a
gesture.

## Author handoff: not done, not claimed

Not run by the author: a window, the viewer, the CUA, any GPU test, Windows or Linux, the CI of this
diff; no window pass is claimed. The conversion of OS physical pixels to egui points is egui-winit's
(0.36.1 `on_mouse_wheel`/`PinchGesture`, read, not executed: a winit `Window` cannot be made without a
display); the gates start from egui's logical events at 1x and 2x and from winit events through the
window's own routing. Not in scope and still open: the §31C circle-centre object snap (the centre is
drawn, it is no snap candidate on either canvas), snapping and guides on the analytic canvas,
quadrants, mixed Line/Circle profiles, arcs, constraints made from the canvas, a pointer command in
the CLI; keyboard or double-click zoom; §30, Milestone 5C, the product and the OOM investigation.

Base CI (not this diff), read at the start and again before the handoff: PR #113's final head
`a246687` passed 23/23 checks and 5/5 workflows. The post-merge runs of `b428616`: `CI` 38724617838,
`planegcs pin` 38724617809, `rust notices` 38724617857 and both SBOMs completed with success;
`combined runtime layout` 38724617810 was still `in_progress` and is not counted as evidence of
anything.

## Independent review (macOS arm64, 2026-10-15)

Three defects were reproduced with assertions before their production fixes:

1. Nonzero wheel deltas on `Start` and `End` were dropped. The existing zoom widget gate now
   exercises Start/Move/End on all six forms; Cancel carries no movement.
2. Zoom clamped the corner at its arithmetic bound, moving the point under the cursor by millions
   of mm. The existing limits gate now reaches that case. A zoom that cannot keep its anchor
   inside the bound changes nothing. Non-finite positions and empty canvas rectangles are refused.
3. A reserved pan press could fall through to a normal Line click while navigation was unavailable,
   including a queued Create and a focus transition. Reserved presses now stay out of the canvas's
   edit path even when no pan can start; other widgets still receive their normal events. The
   refusal gate compares the draft before and after all refused input and before and after queueing.
   Its visible-vertex probes exclude a vertex that has moved outside the canvas onto another button.

The first guarded regression run executed 16 tests: 14 passed, two failed (wheel phase and corner
anchor). After those fixes the stronger before/after refusal check failed on an executed assertion
with an added Line point. Final native results: 16/16 exact pure/widget tests, no skips, then the
whole viewer suite **788 passed, 0 failed, 4 existing ignored**. All 19 `FCAD_31J_*` markers executed;
23 unrelated conditional skip lines are recorded separately. Maximum sampled native-test footprint
was **289.627 MiB**, exit 0. Logs: ignored `target/sketch-navigation-review/`.

The independent true-stub rebuild also ran the full suite: **788 passed, 0 failed,
4 existing ignored**, all 16 pure/widget markers, and 217 separately counted conditional
native skips. None of the three new native markers was claimed there. The fresh CLI and
test executable have no OCCT or PlaneGCS imports; the bridge CMake cache explicitly disables
OpenCASCADE discovery, preventing an accidental Homebrew fallback. Both full-suite runs
were guarded, with pressure 1 and swap 0 throughout. Final workspace all-target/all-feature
clippy with `-D warnings`, fmt, actionlint, shellcheck and the Actions run-size check pass;
the largest run block is 20,432 bytes, within the 21,000-byte repository limit.

The fresh relocatable bundle was staged from the current release CLI/viewer and verified ad hoc.
It ran without DYLD search paths, under the unchanged 1536 MiB / 1800 s watchdog. One viewer
(PID 17831) exited normally after 1580.4 s: peak **208.939 MiB**, all pressure samples 1, swap 0.
Only owned test tabs were closed; completion was checked by the PID/guard, without another app query.
The UI helper's earlier soft cutoff stopped one cleanup query; cleanup finished before the original
watchdog deadline. The watchdog limits were never raised or bypassed.

Observed in the window (Retina 2x):

- Saved Line: exact `12.50` survived zoom; small notch vertices separated; a drag at the enlarged
  scale made one draft Undo/Redo step; Fit was explicit. Apply, export before Save, document
  Undo/Redo, Save As Cancel and Save all ran. Cancel left the source byte-identical.
- Saved Circle: centre and radius drags, cursor-centred zoom, draft Undo/Redo, Fit, leaving the tab
  with its form and returning to the same view/numbers, Apply, export before Save and Save.
- Saved annulus: centre and both radius handles, zoom about each selected handle, Fit, an oversized
  hole as a visible refused draft and recovery by Undo, then Apply/export-before-Save/Save.
- A 0.02 mm bore of a 0.8 mm part became separately selectable after zoom. Drag changed the bore
  to 0.029698601320857266 mm and kept the centre and outer radius; Undo/Redo and Cancel published
  nothing. The source stayed byte-identical.
- New Line/Circle/annulus: independent zoomed views across profile changes, a new Line click at
  the changed scale, unchanged analytic numeric fields and inactive history after navigation alone.
  These New drafts were cancelled, not published.

Three real saved models and their exports made in the window before Save were compared with the
existing CLI given the saved numbers: all SQL cells except `meta.modified_at` and STL/FBX bytes
matched. Pristine inputs retained all seven recorded SHA-256 hashes. Independent binary STL:

| Window model | Triangles | Signed volume mm³ | Mesh / analytic |
| --- | ---: | ---: | ---: |
| Line | 24 | 6748.372104 | — |
| Circle | 404 | 5124.831519 | 0.999368 |
| Annulus | 684 | 4045.157390 | 0.999568 |

All three meshes are closed; circular vertices are on the stated radii. Pinned ufbx 0.23.0 read
all three actual FBX (`checks=6 failures=0` each); oriented triangles matched the STL. Substituting
pristine files for the saved results was refused in all three comparator controls. Inputs, saved
models and actual exports are outside the checkout in `/private/tmp/ferrite-31j-review/`; comparator
and reader logs and window captures are in the ignored review directory.

**Not observed in a native window:** Shift/middle pan, pinch, navigation attempted during a held
edit gesture, native focus-loss cancellation, 1x display, and the Revolve/Fillet-history recipe routes.
The current CUA driver did not produce Shift/middle drags with the attempted parameters; System
Events refused held-key injection (1002). These attempts are not pan passes. Real-widget gates
cover these navigation paths and the production window adapter at 1x/2x. No new test hook or model
operation was added for the driver. Windows/Linux windows, §31C centre snap, §30/5C, the product and
OOM investigation remain open. Base merge CI was independently confirmed complete: 27/27 checks,
6/6 workflows. CI of this diff is recorded separately in its PR after publication.

## Independent macOS window recipe (the author does not run it)

Inputs only, outside the checkout, made by `make_inputs.py` (copies of both scripts are kept in
the ignored `target/sketch-navigation-check/window-recipe/`): after
`source target/launch-journal-check/native-env.sh`, `python3 -I make_inputs.py
<checkout>/target/release/ferritecad <new, absent dir>` writes `line.fcad` (a Line polygon
with a V notch of two 0.42 mm segments, vertices 0.3 mm apart), `revolve.fcad` (the same kind of
notch on a full-turn Revolve), `rounded.fcad` (a plate with a Fillet in its history), `circle.fcad`
(centre (12.5, −7.25), R 10.5, height 15.25), `annulus.fcad` (the same with a bore of 4.75),
`annulus-b.fcad` (its byte copy), `tiny-annulus.fcad` (centre (3, 2), outer 0.8, bore 0.02) and
`SHA256.txt`. No expected window output and no screenshot exist. `compare.py` (same folder)
compares what the window saved and exported with the command line. Work on copies; run the
viewer only through `tools/watch-viewer-memory.py --limit-mib 1536 --seconds <limit>` with
pressure normal; logs into `target/sketch-navigation-check/` of the checkout under review.

Gestures on every canvas: wheel or trackpad scroll (vertical) and pinch zoom about the cursor;
**middle-button drag** or **Shift + left-button drag** pans; **Fit drawing** (Line) / **Fit profile**
(Circle, annulus) and the Line's **Reset view** are the way back. The words on the canvas's lower
right say so, and say "View locked while you drag" while a vertex or handle is held.

1. **Hint and the cursor anchor** (each canvas). Hover a recognisable spot (a vertex, the
   notch, a handle) and turn the wheel up: the spot stays under the pointer while the drawing
   grows; down: it shrinks about the same spot. Pinch does the same. Zoom in until the grid
   label reads very small steps, then far out: the view stays a view (no jump, no blank
   freeze); at the end of the range further turns change nothing.
2. **Line polygon, saved Extrude** (`line.fcad`, **Edit saved Sketch**). Type `12.50` into a
   coordinate box without confirming. Zoom and pan (Shift + drag, then middle drag): the box still
   reads exactly `12.50`, **Undo draft**/**Redo draft** and the Apply/Save buttons look as before,
   and no job starts. A Shift press and release on empty canvas or on a vertex, without moving,
   places and selects nothing; Escape during a pan puts the view back. Near the notch the three
   vertices overlap at the opening view: zoom in until each can be picked apart (the 9-point ring
   follows the pointer, not the zoom). Turn on **Object snap**, zoom to make the 0.42 mm segments
   long, drag the notch tip toward the middle of a segment: **Midpoint** is named; at the opening
   zoom the same drag resolves to an endpoint.
3. **Held drag freezes the view.** Hold a vertex (do not release) and turn the wheel, pinch, press
   the middle button and move: nothing changes; the hint says "View locked while you drag".
   Release: one **Undo draft** step. Hold again, press Escape, release: the press-time strings.
4. **Fit** returns the whole contour at the canvas's fixed 510 × 250 points. Typing or dragging
   never fits by itself.
5. **Revolve** (`revolve.fcad`, the axis line follows the view) and the **Fillet history base**
   (`rounded.fcad`): zoom, pan, Fit, one vertex drag, **Apply**, export STL/FBX **before Save**,
   document Undo/Redo, **Save As…** → Cancel (nothing written), **Save**.
6. **New → Line polygon**: click three points, zoom and pan between clicks (the next click lands
   where the pointer is), **Create new document**: exactly the typed/clicked strings.
7. **Circle** (`circle.fcad`) and **Annulus** (`annulus.fcad`, **Edit …**): zoom in about the
   centre, pan, **Fit profile** (back to the opening view), zoom again, drag the centre and radius
   handles at that scale: the strings are the drag in points over the new scale. **Apply**,
   export before Save, **Save**. `tiny-annulus.fcad`: at Fit the bore handle sits on the centre
   (the cross and diamond coincide); zoom in about the centre until they part, then drag the
   diamond. **New → Circle with hole**: type a bore of `0.01` and do the same.
8. **Profiles and tabs.** In New, switch Line polygon / Circle / Circle with hole after zooming each:
   each keeps its own view. Open `annulus.fcad` and `annulus-b.fcad` in two tabs, open the form in
   each, zoom A, pan B, **Fit profile** in B, switch several times: each tab shows its own view.
   Hold a pan and try to switch tabs or press Cmd+Q: the window refuses (the status asks to wait);
   after the release it does not.
9. **Where it must not act.** Move another form over part of a canvas: the wheel over the covered
   part zooms nothing there. Wheel over the canvas must not zoom the 3D view; wheel over the 3D
   viewport beside the form still does; a 3D drag begun in the viewport still crosses the form.
   (A running job and a queued request disable navigation too; they are widget-gated, not
   window-observable.)
10. **Comparison.** For each saved file: `python3 -I compare.py <checkout>/target/release/ferritecad
    <line|circle|annulus> <the pristine input> <the saved file> <the STL exported before Save>
    <the FBX exported before Save> <new work dir>` must print `VERDICT: PASS` (every SQL cell but
    `meta.modified_at`, saved/command-line/before-Save STL and FBX byte-identical, an independent
    reader of the STL); the same with `--negative` must print `NEGATIVE CONTROL HELD`. Read the FBX
    with the pinned reader (`tools/check-fbx-complex.sh` hook lines or the reader built from
    `tools/unity-fbx-smoke/scripts/`). Optionally repeat step 1 and 7 on a 1x display. Record the
    watchdog's peak, pressure, swap and exit; do not call the OOM investigation closed.

## Manifest

New: `crates/ferritecad-app/src/sketch/navigation.rs`, `crates/ferritecad-app/src/sketch/navigation_tests.rs`,
this file. Changed: `crates/ferritecad-app/src/sketch.rs`, `crates/ferritecad-app/src/sketch/analytic.rs`,
`crates/ferritecad-app/src/sketch/analytic_tests.rs` (`canvas_rect` visible to the new tests),
`crates/ferritecad-app/src/sessions/tests/analytic.rs`, `crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs`
and its `close_form.rs`, `quit_forms.rs`, `quit_new.rs`, `open_over_new.rs`,
`.github/workflows/{ci,occt-pin,runtime-layout}.yml`, `tools/check-fbx-complex.sh`,
`docs/implementation-plan.md`, `docs/analytic-sketch-canvas.md`, `docs/sketch-vertex-drag.md`.
The diffstat is in the handoff report.
