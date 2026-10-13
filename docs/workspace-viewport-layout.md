# §31B — A separate 3D viewport and the window's working layout

## Contract decided before code

Base after a fresh fetch: HEAD = main = origin/main =
`4410bd6d2595bc01ec7d9e5620b40a551629e4b9` (merge of PR #104; §31A, review head
`5959cd4`), clean checkout. Branch `workspace-viewport-layout`; every change is
unstaged and uncommitted. §31A is not implemented again.

**The problem.** Model stood on the left, but the former wall of editor buttons, the
definitions, the inspector and the solve status were drawn straight into the root
interface, over a model that was still rendered into the whole window. The camera's
aspect ratio, the renderer's viewport and the pick frame all took the whole window
as the 3D view, so anything under a control was drawn *and* pickable, and a press on
a control was kept from the model only by egui's "wants the pointer" flag.

**The layout** is one production function, `workspace::compose`, run every frame by
`Live::draw` and run by the tests with real widgets:

| Region | Contents | Bound |
| --- | --- | --- |
| Top | file and view actions; the tabs | each wraps into at most two rows (56 / 66 pt) and scrolls beyond that; the tab strip is one row tall even with no tab, so the first document does not move the viewport |
| Left | **Model** tree (§31A, unchanged) | width 160…340 pt, never more than 28 % of the window; scrolls both ways |
| Right | **Actions** (questions needing an answer, outcomes, recovery, checkpoints, the Create/Add/Edit routes) and **Details** (definitions and visibility, topology inspector, sketch solve status) | width 220…480 pt, never more than 34 %; scrolls both ways |
| Bottom | the buttons that stop what is running (Cancel, Cancel creation, Cancel operation, Cancel export), progress, the activity line, unsaved mark, the document's last outcome | one fixed height (60 pt), scrolls |
| Middle | **the viewport** | whatever no panel covers |

Floating editor forms stay egui windows, in front of the viewport; their first
position is the viewport's top-left corner rather than the Model panel. Their IDs,
`in_tab_scope("tab-forms" | "tab-height", tab)`, are unchanged, and each right-panel
group has an explicit ID so one appearing does not renumber the fields below it. A
question that must be answered first (Close with an unfinished form, Quit) takes the
right panel; the other panels are drawn disabled, so the layout does not jump and
nothing else can be pressed. Every previous route, message and guard is reached from
the same owners and predicates; nothing was rewritten to fit a panel.

**One rectangle.** The viewport is not a panel. `compose` returns what is left in
logical points; `PixelRect::from_points` is the only conversion to physical pixels
(each edge rounded, clamped to the surface, empty if anything is not finite). The
rectangle is set on `ViewportInput` once per frame, *before* the model is drawn, and
then:

* the camera has exactly the rectangle's size (so Frame all/selected and the aspect
  ratio are the viewport's), and a change of size forgets a remembered magnification;
* the renderer draws into the rectangle only: the whole texture is cleared, the
  viewport and scissor are the rectangle, and a camera that disagrees with it, a
  rectangle outside the texture, or an empty one is refused/cleared rather than
  drawn stretched;
* a pick or hover frame is rendered at the camera's size, so its pixel `(x, y)` is
  the viewport's own pixel;
* a pointer arrives in window pixels and is translated by the reducer, once, with
  the origin removed. The pointer is kept in window pixels; pending picks and hovers
  are in the viewport's pixels and are **forgotten when the rectangle changes in
  origin or in size**, so the same numbers are never read in a different place;
* a press beside the viewport begins no gesture whoever claimed it; a drag that
  began inside keeps its button across a panel and does not pick where it ends;
  wheel, pinch and turn beside the viewport are ignored.

HiDPI is the same rectangle at another scale: the layout is in points, so 2x gives
twice every edge. Zero size, minimise and a window too small for the panels give an
empty rectangle: nothing is drawn but the clear, the camera is not drawable, and
nothing is picked. A tab shown again keeps its own camera and is fitted to the
*present* rectangle (`Tabs::view_for` copies the window's region, not a size).

**State.** No new store, history, persistence, autosave, schema or dependency. The
Model selection by UUID is still separate from topology picks; tab switching,
Apply/Undo/Redo/Save, unfinished forms, Close/Quit, New and stale-action guards are
the existing owners'. Layout reads only borrowed presentation data (`workspace::View`
holds no open document, session or worker; the file name and the already-read
`ExtrudeEditSource` it receives are the ones the forms received before), so a frame
can neither rebuild nor read the document. Windows and Linux use the same code; macOS support remains arm64 only.

## Where it lives

* `ferritecad-viewport`: `PixelRect` (physical-pixel rectangle; `from_points`,
  `contains`, `local`, `window`).
* `ferritecad-ui`: `ViewportInput::{set_region, region}` (the region and its
  consequences; `resize(w, h)` stays as "the region is the whole window", used by the
  arithmetic's tests); `toolbar` is now `toolbar_actions` plus `activity_lines`, so the
  buttons and the status lines can stand in different panels. The stop buttons that
  exist only while something runs moved from the action row to the activity lines
  (the composition `toolbar` shows them under the row): a button that appeared
  in the row changed the row's height, and the viewport with it, each time an
  operation started.
* `ferritecad-viewport-gpu`: `draw_into` and `SurfaceFrame::draw_scene` take the
  region (viewport, scissor, refusals); `SurfaceFrame::size`;
  `Renderer::draw_window_image`, the readable twin of the window's pass, which exists
  so the region can be checked without a window.
* `ferritecad-app`: `workspace.rs` (`compose`, `region_of`); `Live::draw` lays the
  interface out first, sets the region, draws the model into it, then paints the
  interface; a resize only reconfigures the surface (the next frame finds the
  region); `Tabs::view_for` copies the window's region; `form_window` opens at the
  viewport's corner.

## Verification

Executed locally on macOS arm64 (this is the only supported macOS architecture;
native and stub test executables and the peer CLI were checked with `file`: arm64),
with serial builds (`CARGO_BUILD_JOBS=1`) in the existing common `target/` and the
existing `target/30u-stub`; no third large target and no foreign file, cache,
process or worktree was touched. Logs are in the ignored `target/31b-check/`.
Environments: `target/30t-review/env.sh` (native: vendor OCCT 8.0.1 and PlaneGCS,
`FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=1`) and
`target/30u-check/stub-env.sh` (true stub). The stub was proved, not assumed:
the bridge CMake cache holds `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE` and
`OpenCASCADE_DIR-NOTFOUND`, and `otool -L` of the stub test executable names no
`libTK*` or PlaneGCS library (`stub-proof.log`). The native executable links
`@rpath/libTK*`.

New gates, each inserted into an existing block or run, none duplicating a heavy
corpus:

| Gate | Where | Marker (printed after the last assertion) | Configuration |
| --- | --- | --- | --- |
| `sessions::tests::tabs::workspace::long_messages_and_long_lists_cannot_move_the_viewport_or_paint_over_it` | `ci.yml`, the existing "Keep Quit and New drafts and accepted Model navigation owners" block | `FCAD_31B_LAYOUT_BOUNDS_EXECUTED` | true stub + native |
| `…::the_same_layout_at_two_pixels_per_point_is_the_same_rectangle_twice_as_large` | same block | `FCAD_31B_SCALE_EXECUTED` | stub + native |
| `…::an_empty_window_and_an_empty_document_stay_controllable` | same block | `FCAD_31B_EMPTY_EXECUTED` | stub + native |
| `…::panels_keep_their_presses_and_the_viewport_keeps_its_own` | same block | `FCAD_31B_OWNERSHIP_EXECUTED` | stub + native |
| `…::dragging_a_panel_moves_the_viewport_and_what_was_asked_in_the_old_one_is_dropped` | same block | `FCAD_31B_REGION_MOVE_EXECUTED` | stub + native |
| `…::each_tab_keeps_its_camera_across_a_layout_change_and_is_fitted_to_the_present_viewport` | same block | `FCAD_31B_TABS_EXECUTED` | stub + native |
| `…::a_question_that_must_be_answered_first_leaves_the_layout_and_the_viewport_alone` | same block | `FCAD_31B_MODAL_EXECUTED` | stub + native |
| `…::a_form_keeps_its_focus_and_text_when_the_groups_above_it_come_and_go` | same block | `FCAD_31B_FORM_IDS_EXECUTED` | stub + native |
| `tests::a_native_part_is_framed_picked_and_drawn_only_inside_a_shifted_viewport` | `occt-pin.yml`, the existing list of gates of the viewer-suite run that already sets `FERRITECAD_REQUIRE_GPU=1` and `FERRITECAD_REQUIRE_OCCT=1` | `FCAD_31B_NATIVE_REGION_EXECUTED picks=6 scales=2 kinds=face,edge,corner` | native + device |
| `a_model_in_a_shifted_region_stays_in_it_and_is_picked_through_the_same_geometry`, `a_region_with_no_area_clears_and_a_region_that_disagrees_is_refused` (`ferritecad-viewport-gpu/tests/smoke.rs`) | the crate's ordinary tests (`cargo test --workspace`), which skip without an adapter, as every test in that file does | `FCAD_31B_GPU_REGION_EXECUTED`, `FCAD_31B_GPU_REFUSALS_EXECUTED` | device, kernel-free |
| `panels::tests::the_stop_buttons_stand_with_the_activity_and_never_resize_the_action_row`, `input::tests::a_click_is_expressed_in_the_pixels_of_the_picture_not_of_the_window`, `…::a_moved_region_forgets_what_was_asked_in_the_old_one_even_at_the_same_size`, `…::a_press_beside_the_picture_begins_nothing_even_if_nobody_claimed_it` | `ci.yml`, the existing `ferritecad-ui --lib` exact loop of the same block (`test … ... ok`) | – | headless |
| `region::tests` (5, `ferritecad-viewport`) and the other five `input::tests` | ordinary lib tests (`cargo test --workspace`) | – | headless |

What the layout gates do: every frame is `workspace::compose` with the window's
real owners (`Tabs`, `Sessions`, the forms, `ViewportInput`) and real egui
pointer input; the rectangle is `workspace::region_of`. Four hundred words in the
activity line, fifteen hundred in the outcome, four hundred definitions, sixteen
Cuts whose names, kinds and notes are paragraphs, and unbreakable 400-character
tokens in the Actions panel, with a reading, a creation, an operation and an
export all running, leave the viewport identical to the pixel to the quiet
layout, paint nothing over it, and keep every toolbar button wholly in view at 1024
points. At one and two pixels per point the pixel rectangle doubles exactly and a
pointer lands at the same place of the picture. A press on each of the top strip, the
Model panel, the Actions panel and the status strip begins no gesture and picks
nothing; the real **Open…** button takes its press; a click in the viewport asks at
the right pixel of the picture; a drag begun there crosses the Actions panel,
turns the model and does not pick where it ends. Dragging the two panel edges
by real egui events moves the rectangle 20 points at the same size, and the click
and hover that were waiting are dropped. Two tabs are turned differently, the panels
are resized, and each tab comes back with its own eye and target in the present
rectangle. During the Close/Quit question the rectangle does not move and a
disabled **Open…** takes no press. The open height form's field, focused by a real
press and typed into by a real keystroke, keeps its identity, focus and text while the
groups above it appear and go (its ID is `scrolled` + group + the unchanged
`in_tab_scope` part and tab, none of which depends on a sibling). The empty window and an empty document keep
**Open…**, **New…** and the Create routes and a Model panel that says so, and
the first tab appearing moves nothing.

The device-backed gates draw a known part through the window's own pass into a
texture larger than the picture: Frame all fits the whole extent inside the
rectangle by its aspect; every pixel outside the rectangle is the clear colour
even when the model is zoomed past its edges; every pixel inside is the pixel of
the pick frame; a face, an edge and a corner are found at the pixels the camera
projects them to, once the origin is removed and not before. The native gate does
this for the committed plate's named face, edge and corner (three OCCT scenes) at 1x
and 2x with a layout from `quiet_layout` (the production `compose`), a click
delivered in window pixels through the reducer, `selection_at` and the window pass
with the chosen thing marked and unmarked, which differs at the window pixel the
click was made at.

| Executed check | Result | Log |
| --- | --- | --- |
| Base | HEAD = main = origin/main = `4410bd6d`; six push workflows of that commit (`CI`, `combined runtime layout`, `planegcs pin`, `rust notices`, `rust sbom`, `product sbom`) all completed `success`; none still running. This is the base's CI, not CI of this diff | `base-ci.txt`, `remote-main.txt` |
| Real `ci.yml` blocks, true stub | the "Keep Quit and New drafts…" step: 22 exact gates (ten §30W, four §31A, eight §31B) executed through the extracted run body, each with its marker and `1 passed`, no `skipped:`; the "Exercise tab drafts and Close questions…" step, with its `ferritecad-ui --lib` exact loop now including the four new UI gates: 59 gates, no `skipped:` | `ci-stub-block.log`, `ci-stub-ui-step.log` |
| Real `runtime-layout.yml` native block (§30W + two §31A) | 3 gates executed, no skip | `runtime-native-block.log` |
| Native device gate, release, `FERRITECAD_REQUIRE_GPU=1`, under the watchdog | passed, marker printed | `watch-native-gpu-gate.*` |
| `ferritecad-viewer` whole suite, native release `--features planegcs`, `FERRITECAD_REQUIRE_GPU=1`, one thread, under the watchdog | 651 passed, 0 failed, 2 ignored; 23 explicit N/A returns (stub-only 9, no-solver/mixed 8, real-GUI-artifact opt-ins 6) and none for OCCT, PlaneGCS or the adapter | `watch-native-full-suite*` |
| `ferritecad-viewer` whole suite, true stub debug, one thread, under the watchdog | 651 passed, 0 failed, 2 ignored; 202 explicit native/solver returns (e.g. 52 "session gates need Open CASCADE", 37 "no sketch solver"): the stub checks layout and ownership, not geometry | `watch-stub-full-suite*` |
| `ferritecad-viewport-gpu` under the watchdog | `tests/smoke.rs` 101 passed, `tests/sketch.rs` 15, lib 17; no `skipped:` | `watch-gpu-*` |
| `ferritecad-ui --lib`, `ferritecad-viewport --lib` | 119 passed (was 110), 17 passed (was 12) | `ui-lib.log`, `viewport-lib.log` |
| `cargo fmt --all -- --check` | passed | `fmt-final.log` |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed on the final Rust source | `clippy-final.log` |
| Licence headers, export boundary, solver ownership | passed (487 tracked headers; the three new sources checked separately, all `MIT`) | `untracked-source.log` |
| actionlint, shellcheck, Actions run size, `git diff --check` | passed; the two changed run bodies were shellchecked with SC2194/SC2016 excluded for the matrix interpolation; 203 run blocks, largest 20432 bytes, packed argv 20463 < 21000 (unchanged) | `actionlint.log`, `shellcheck.log` |
| Dependencies, licences, SBOM, notices | N/A: no manifest, lock file or vendored file changed | – |
| GUI generator and comparator (unchanged from §31A) | inputs generated outside the checkout (`inputs_only=true`); `--compare` without window outputs refuses (`missing real GUI outputs`); a root inside the checkout is refused; no positive GUI output was manufactured | `gui-generator.log`, `gui-missing-refusal.log`, `gui-checkout-refusal.log` |

### Compiling mutations

Three directed compiling breakages, each caught by a gate, restored byte for byte
(SHA-256 of the file before and after are identical) and followed by the passing
positive gate. No mutation framework was added.

1. **Origin lost in the pick** (`ViewportInput` stored the pressed window point as
   the pick): `input::tests::a_click_is_expressed_in_the_pixels_of_the_picture_not_of_the_window`
   failed (`Some((250.0, 100.0))` ≠ `Some((50.0, 60.0))`) and the native gate failed
   with **face: the click is not at the pixel it was made at** (358, 265 ≠ 98, 218).
   `input.rs` SHA-256 `54ccb2581dfc5e64c39ea340c1f74b74baf2c7f3654e7e8ed0767324b05504bb`
   (`mutation-lost-origin.log`, `mutation-input-*.sha`).
2. **Wrong aspect** (the camera sized by the region's far corner, as if by the
   window): `the_camera_is_the_size_of_the_region_and_not_of_the_window` failed
   (`(800, 600)` ≠ `(600, 560)`) and the native gate failed with **the camera is not
   the region's size** (`mutation-wrong-aspect.log`, `mutation2-*.sha`).
3. **Viewport origin dropped in the renderer** (`set_viewport(0, 0, …)` with the
   scissor kept): the device gate failed with **window pixel 225,80 differs from the
   pick frame's** (`mutation-viewport-origin.log`, `mutation3-*.sha`; `renderer.rs`
   restored identically).

### What the new gates found

* A vertical-only scroll area grows to the width of its widest content, and a
  resizable panel grows with it: one Model row with a long name widened the Model
  panel from 230 to 286.7 points and moved the viewport. Both side panels now
  scroll both ways inside the panel's own width (labels wrap at that width). The
  failing run was the intermediate state of `long_messages_…`.
* A status strip that grew with its message moved the viewport by 31 points when a
  long sentence appeared; the strip is now one fixed height.
* Opening the first document gave the window its first tab and took 32 points from
  the viewport after the model had been framed for it; the tab strip now keeps one
  row. Likewise the stop buttons appearing in the action row added a row (21
  points) at 1024 points while an operation ran; they are in the activity lines now,
  and the layout tests run the widest case (everything running) against the idle one
  and require an identical rectangle.
* The 1x and 2x layouts differed by 0.2 points until the side panel could no
  longer be sized by its content; they are equal now.

### Not run, and why

* **No window, viewer, bundle, CUA, osascript or screenshot** was run by the author;
  headless owner/widget tests and the offscreen device gates are not GUI evidence.
  The window scenario below is for the independent reviewer.
* Windows and Linux builds and windows were not run (no such host; the change uses
  no platform API: `set_viewport` and a scissor rectangle are core wgpu). Their CI
  is the base's, not this diff's.
* Mixed OCCT-without-solver was not run: layout does not change solver
  availability or execute constraints.
* No large STEP corpus, FBX/Unity or SBOM campaign: nothing they cover changed. A
  small imported STEP scene needs a native import that this slice does not touch;
  its viewport behaviour is the same path as an empty document's (no Body history,
  no rows), which is tested.
* The uncommitted diff has no CI of its own.

### Resources

Pressure was read as 1 before and after every long step; the watchdog aborts at any
other level (limit 1536 MiB footprint, 900–3000 s). Swap 0 MiB throughout, free disk
about 150 GiB. Sampled peak physical footprints (a 0.5 s sampler: a process that
lives less than a second may be sampled once or twice, so the short runs say little).
Final runs: native whole suite 273.7 MiB (227 samples, 120 s), the native device gate
168.2 MiB (7 samples, 4 s), stub whole suite 38.4 MiB (61 samples), `smoke.rs` 12.3 MiB,
`sketch.rs` and the viewport-gpu lib 2.4 MiB. The highest sample in any watched run
of this session was 291.8 MiB (an earlier native whole-suite run); the limit was 1536
MiB. Every watched run exited 0 and none was aborted. No cargo, rustc, viewer, watchdog or test
process of this session remained at the end.

**Deviation, stated plainly.** Before I applied the rule, eleven single-test device
invocations ran directly through `cargo test` without the watchdog: six of the
offscreen renderer gate (including the mutation runs) and five of the native gate.
Each lived about a second, drew into small offscreen textures and was followed by a
pressure reading of 1; no memory fault was seen, but they were not watched. Every
later device run, including all final evidence above, was under
`tools/watch-viewer-memory.py`. The reason for the earlier OOM remains unknown and
nothing here investigates it.

## Independent macOS window scenario (the author does not run it)

Inputs and every real window output live outside the checkout. The generator and
the comparator are §31A's, unchanged (`tools/model-tree-navigation-gui.py`), so no
new comparator exists and nothing positive was generated here. Use a fresh arm64
bundle built through the existing staging route, pressure 1, a non-login Bash, the
existing watchdog and separate state:

```sh
source target/30t-review/env.sh
ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ferrite-31b-window.XXXXXX")/run"
export FERRITECAD="$APP/Contents/MacOS/ferritecad"
python3 tools/model-tree-navigation-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery" FERRITECAD_TABS_DIR="$ROOT/tabs"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/work/cuts.fcad"
```

1. **Layout.** The window opens with: file and view actions and the tab strip along
   the top, **Model** on the left, **Actions** and **Details** on the right, a status
   strip along the bottom, and the model only in the middle. Nothing is drawn over
   the model but a form window you opened. Resize the window larger and smaller,
   drag the Model panel's edge and the Actions panel's edge, and (if a second
   display of another scale is attached) move the window to it; minimise and
   restore. The model keeps its direction, fills the middle, and never goes under a
   panel; the buttons wrap before they scroll.
2. **Camera.** **Frame all (A)**: the whole part is inside the middle and centred in
   it. Orbit, pan and wheel-zoom with the pointer inside; the zoom goes towards the
   pointer. Press on a panel (including its empty parts and the status strip) and
   drag: the model does not move. Start a drag in the middle and carry it across the
   Actions panel: it keeps turning, and the release does not select anything.
3. **Geometry.** Hover and click a face, an edge and a corner of the plate: the mark
   is under the pointer, and **Details** names what was chosen (Model selection
   stays separate). Do it again after dragging a panel edge and after a resize: the
   mark is still under the pointer.
4. **The existing flow** (as §31A step 1–2): in **Model** select the first Cut →
   **Edit Cut…** → radius 1.625 → Confirm draft numbers → **Apply cut**; export
   `unsaved.stl` and `unsaved.fbx` into `$ROOT` before Save; **Undo** → export
   `undo.stl`; **Redo**; **Save**. Every previous route (New, Create, Add, Edit,
   recovery, checkpoints, Save, Export) must be reachable from the new places.
5. **Tabs.** Before applying the Cut in step 4, **Open…** `work/copy.fcad`.
   Orbit it differently, resize a panel, switch back to cuts: each tab's view is its
   own and fits the present middle; the Cut's typed form survives the switch.
   After step 4, open `work/fillets.fcad`, `work/chamfer.fcad` and
   `work/revolve.fcad` in that order. Open and cancel their existing editors,
   including the Revolve Sketch editors, then return to cuts. The comparator
   expects all five tabs, in order `cuts, fillets, chamfer, revolve, copy`.
6. **Quit** normally and require the watchdog's own PID to exit; record the peak,
   pressure, swap and exit apart from comparator evidence. Then
   `python3 tools/model-tree-navigation-gui.py --compare "$ROOT"` must print
   `FCAD_31A_GUI_COMPARE_OK all_SQL_cells=true controls=3`. The geometry and layout
   steps (1–3, 5) are observed and recorded by the reviewer; the comparator proves
   only the files.

### Coordinate assumptions this slice changes

* Everything that took the **whole window as the 3D view**: a recipe or log that
  clicks "the middle of the window", "the plate at (x, y)" or a fixed offset from the
  window's corner now lands on a panel or a different part. The middle of the
  window is no longer the middle of the model.
* Everything that expects **controls over the model**: the toolbar, tab row,
  Create/Edit buttons, definitions and inspector are no longer stacked in the root
  interface under the toolbar (README's "under the toolbar" for `Export` and
  `Open failed` was corrected; the tab row of `document-tabs.md` is still directly
  beneath the toolbar buttons). §31A's step 3 ("existing definitions, Frame/Hide/Isolate
  and topology inspector remain reachable") now means the top strip and **Details**.
* **Model** was at x ≈ 260 pt wide; it is 230 pt by default, and **Actions** is a
  second panel on the right (300 pt) that older recipes do not know.
* Older verification reports that record CUA screen-coordinate clicks
  (`edit-chamfer-base-sketch-verification.md`, `edit-fillet-base-height-verification.md`,
  `partial-angle-revolve-verification.md`, `sketch-session-apply-verification.md`)
  are historical operator notes and were not rewritten; the recipes that name
  buttons by their labels (`tools/*-gui.py`, which contain no coordinates) stay valid.
  A Retina display doubles pixels, not points: coordinates are physical pixels in the
  window and logical points in the layout, converted once.

## Limits at the author handoff

§30, Milestone 5C, the product and the earlier OOM investigation remain open. This
is a layout slice: there is no new CAD operation, schema, dependency, autosave,
history, docking framework or second state store, and no STEP assembly explorer.
Exact CI of this diff, Windows and Linux window behaviour and the real window are
not claimed. The next slice was not started.

## Manifest at the author handoff

Base `4410bd6d2595bc01ec7d9e5620b40a551629e4b9`, branch `workspace-viewport-layout`,
nothing staged (`git diff --cached` is empty), nothing committed. Full status,
per-file diffstat and SHA-256 of every changed or new source (this file excepted,
since it carries the manifest) are `status-final.txt`, `diffstat-all.txt` and
`source-hashes-final.txt` in `target/31b-check/`.

| Modified (tracked) | + | − |
| --- | ---: | ---: |
| `.github/workflows/ci.yml` | 14 | 2 |
| `.github/workflows/occt-pin.yml` | 7 | 1 |
| `README.md` | 12 | 3 |
| `crates/ferritecad-app/src/main.rs` | 323 | 190 |
| `crates/ferritecad-app/src/sessions/tests/tabs.rs` | 2 | 0 |
| `crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs` | 2 | 2 |
| `crates/ferritecad-app/src/sessions/tests/tabs/drafts/model_tree.rs` | 13 | 57 |
| `crates/ferritecad-app/src/tabs.rs` | 4 | 2 |
| `crates/ferritecad-ui/src/input.rs` | 291 | 23 |
| `crates/ferritecad-ui/src/lib.rs` | 3 | 2 |
| `crates/ferritecad-ui/src/panels.rs` | 123 | 22 |
| `crates/ferritecad-viewport-gpu/src/lib.rs` | 1 | 1 |
| `crates/ferritecad-viewport-gpu/src/renderer.rs` | 169 | 15 |
| `crates/ferritecad-viewport-gpu/src/surface.rs` | 31 | 5 |
| `crates/ferritecad-viewport-gpu/tests/smoke.rs` | 260 | 2 |
| `crates/ferritecad-viewport/src/lib.rs` | 2 | 0 |
| `docs/implementation-plan.md` | 22 | 0 |
| **17 files** | **1279** | **327** |

| New (untracked) | lines |
| --- | ---: |
| `crates/ferritecad-app/src/sessions/tests/tabs/workspace.rs` | 856 |
| `crates/ferritecad-app/src/workspace.rs` | 490 |
| `crates/ferritecad-viewport/src/region.rs` | 200 |
| `docs/workspace-viewport-layout.md` | 337 (this file, as written) |

No Cargo manifest, lock file, vendored source, schema, CLI or jobs file is in the
diff. No tool file is changed: the §31A generator and comparator are reused as they
are.



## Independent review — 2026-10-13

The first real window exposed an initial framing regression: a fast load could
frame the part while the camera had zero dimensions, before the narrower central
viewport was known. The first image clipped the part horizontally; Frame all
corrected it. The window now completes initial framing once, on the first usable
layout. Later panel/window resizing and minimisation retain the camera view.
The existing empty-window gate covers this lifecycle, and the native device gate
now starts with an accepted load before layout and requires correct initial
framing at both scales.

The native viewport/device gate is also required by the pull-request runtime
workflow on all three platforms. Its previous placement only in the manually
triggered OCCT pin workflow did not enforce the new coordinate path on a PR.
The exact execution marker, one-test summary and refusal of skips are retained.

The window recipe was corrected to include all five tabs required by the existing
comparator. Verification results below describe reviewer runs separately from
the author's local runs above.

### Review checks and real window

Code/workflow commit: `4e80206fc016846020a1c5b203212c6248cf8a08`.
The local native environment is `target/30t-review/env.sh`; the genuine stub uses
`target/30u-stub` and `target/30u-check/stub-env.sh`. Reviewer logs are in ignored
`target/31b-review/`.

- All eight §31B workspace gates executed in both native and true stub, with their
  execution markers and no skips (`layout-after-fix.log`, `stub-final.log`).
- UI and viewport unit suites: 119 and 17 passed. The native device gate passed
  after the initial-frame fix: six face/edge/corner picks at two scale factors.
  It ran under the memory watchdog (`gpu-fixed.stdout`, `gpu-fixed.jsonl`).
- Fresh release CLI/viewer, fmt, workspace clippy with all targets/features and
  `-D warnings`, actionlint, Actions run-size, licence and boundary checks passed.

The fresh relocatable bundle in `target/31b-review/gui-fixed/FerriteCAD.app` used
only private fixtures under `/private/tmp/ferrite-31b-review-window/final`.
Observed in the macOS window: the initial part fits without pressing Frame all;
window zoom and restoration keep the view inside the panels; Iso/Top and Frame
all work; a face selection appears in Details. Model selects the first Cut and
opens its editor. The typed radius survives switching to another tab and back.
Confirm draft numbers → Apply cut changes the accepted model; STL and FBX export
before Save. Undo restores the original model; Redo and Save accept and persist
the changed model. All five fixture tabs were opened, with their distinct views;
Model opened the existing Fillet, Chamfer and Revolve editors, then their forms
were cancelled and the Cut tab restored.

The unchanged §31A file comparator executed against actual window outputs:
`FCAD_31A_GUI_COMPARE_OK all_SQL_cells=true controls=3`. It compared saved SQL,
UUIDs and refs, the unsaved STL/FBX against the CLI operation and the Undo STL
against the original model, and rejected all three negative controls. Pinned
ufbx 0.23.0 read the actual unsaved FBX: 532 triangles, six checks, zero failures
(`gui-compare.log`, `gui-ufbx.log`). The comparator does not prove window layout.

The window work used two runs of the fixed build. PID 17217 produced unsaved
exports and the Undo export, but reached the 2400-second watchdog time limit
(exit -15, explicitly not a completed window pass). Its sampled peak was
212.970 MiB. PID 26221 repeated the edit from the unchanged source, performed
Undo/Redo/Save and completed the five-tab scenario; normal Quit returned 0 with
sampled peak 207.782 MiB. Both runs had pressure 1 and swap 0. The initial
regression probe (PID 5288) also quit normally, peak 189.798 MiB. No exited viewer
was queried through CUA, and no test viewer remains running. Logs:
`window-watch.jsonl`, `window-final.jsonl`, `window-complete.jsonl`.

Operator limitation: a full path entered in the native Save name field became
a colon-separated filename in the working directory. The three actual GUI
exports were renamed to the comparator's expected paths without modifying their
bytes. No positive GUI output was substituted with a CLI-generated file.

Live panel-border dragging, orbit/pan across a panel, minimise/restore and GUI
edge/corner picking were not established by the window run. They are not claimed
as manual passes; deterministic input/layout tests and the offscreen device gate
provide their corresponding automated coverage. Windows/Linux windows and a
second physical display scale were not tested locally. The old OOM remains
unexplained; no memory cap was exceeded during these review runs.

Remote CI is tracked in [PR #105](https://github.com/gesriot/ferrite-cad/pull/105)
on the exact code/workflow commit above. The initial Windows PlaneGCS job failed
with a locked compiler metadata file (`os error 32`); rerunning that failed job
passed, including the three-platform comparison. The runtime workflow checks the
new GPU marker on Linux, macOS and Windows; a later documentation-only commit
must not be mistaken for execution of that native matrix.
