# §27C verification — solid full-turn Revolve closed on the axis

[Contract and recipe](axis-closed-revolve.md).

## Where and how this was run

* Base: `main` at `6f3b88d05bdda5ff9b34efff662d0fbe40c1d32c`, the merge of
  §27B. Its merge CI was green before work started.
* A Linux cloud container, with pinned OCCT 8.0.1 and no PlaneGCS. One build
  job at a time, on the existing native and stub targets. No GUI, viewer,
  GPU or pixel run was made here.
* The old reader is the real §27B CLI, built from a detached worktree of the
  base commit.

## Decisions

* **One stated fact.** The Revolve stores its axis Line UUID
  (`axis_segment`); the policy derives the class from the coordinates, and
  `stated_revolution` requires the two to agree in creation, discovery, edit
  preparation, the writer, and the evaluator. Neither side is trusted alone.
* **Separate layout and capability.** A Revolve with an axis Line is payload
  v2 and requires `feature.revolve.axis-closed.v1`. Hollow Revolves stay v1
  and byte-identical, so an old build keeps working on them and cannot take
  a solid part for one.
* **Exact zero.** "On the axis" is X == 0 in IEEE terms; −0 is stored as +0.
  Nothing is snapped: 0 < X ≤ 1e-6 mm is refused as near the axis.
* **The axis Line raises no face, checked twice.** The bridge requires the
  stated Line's sweep to make no face of the solid; `record_revolve`
  requires its history to be empty. Every other Line still needs exactly one
  face, with full coverage. No reference, seam, cap or placeholder is made
  for it, and none for a cone's apex.
* **Zero-area triangles.** OCCT meshes a cone's apex as several nodes at one
  point, and the triangles fanning into it have no area; STL export refused
  them. The bridge now drops every zero-area triangle. It does so for every
  shape, not only fresh revolutions, so that a cold build and an
  archive-restored one tessellate identically. The pinned export and pixel
  corpus below proves no earlier mesh had one.
* **JSON.** `full_turn_revolve` keeps meaning a profile off the axis, with
  its `axis_clearance_mm`. The new class gets its own kind,
  `full_turn_revolve_axis_closed`, with `axis_curve_id` and
  `off_axis_clearance_mm`, so an old client stops instead of misreading it.

## Native results (OCCT 8.0.1, no solver)

### New tests (12)

| Suite | Test |
|---|---|
| `ferritecad-cli --test revolve` | `axis::native_solid_revolutions_create_measure_name_cache_and_export` |
| | `axis::native_solid_revolution_edits_keep_the_axis_names_sql_and_cache` |
| | `axis::native_solid_revolution_refusals_are_atomic` |
| | `axis::solid_revolution_discovery_payload_and_writer_without_kernel` |
| | `axis::occt_without_solver_builds_and_edits_a_solid_revolve` |
| | `axis::native_solid_revolve_delivery_late_guards_and_cancellation` |
| `ferritecad-occt --test revolve_occt` | `native_axis_closed_revolutions_are_solid_and_name_every_line_but_the_axis` |
| | `native_axis_closed_requests_are_checked_not_trusted` |
| `ferritecad-kernel --lib` | `kernel::tests::a_stated_axis_line_keys_apart_and_must_be_a_profile_line` |
| `ferritecad-document --lib` | `polygon::tests::an_axis_closed_profile_is_one_named_line_on_the_axis` |
| | `polygon::tests::an_axis_closed_profile_refuses_every_other_way_of_meeting_the_axis` |
| `ferritecad-app` | `sketch::drag_tests::native_solid_revolve_widgets_drag_worker_and_cli_create_and_edit` |

All 12 pass natively. `ferritecad-cli --test revolve` passes 17 of 17
(6 §27A, 5 §27B, 6 §27C), and `revolve_occt` passes 5 of 5.

Changed expectations in older tests: a §27A request with one whole Line on
the axis is now a solid part, so the §27A/B "on the axis" refusals use an
isolated touch instead, and the §27B edit that put a bushing's inner wall on
the axis is now refused as a hollow → solid change.

### What the new gates prove

* **Geometry, independent of production helpers.** Each profile is checked
  against its own closed-form volume (πr²h, πr²h/3, and their sums), in
  every rotation of saved order and both windings, with fractional sizes and
  a Y shift. Kernel volumes agree to 1e-6 relative:

  | Part | Exact volume | Faces |
  |---|---|---|
  | Cylinder r10 h15 | 1500π | 3 (plane, cylinder, plane) |
  | Cone r10 h15 | 500π | 2 (plane, cone; no apex face) |
  | Stepped shaft | 860π | 5 |
  | Frustum (cylinder edited, top r5) | 875π | 3 |

* **Names.** Every non-axis Line's reference resolves to its own face, whose
  surface (plane, cylinder with its radius, cone) and axis (the sketch Y
  axis) are checked through the Line's history. The axis Line has no
  reference and no history. An archive Hit resolves the same names; a stale
  sidecar is a Miss.
* **Mesh.** The tests' own STL reader checks closure and orientation, the
  volume band, radial and axial bounds, that each disc ending on the axis
  has the area of a full disc (no hole), and that a sloped Line ending on the
  axis ends in an apex vertex on it. FBX is read by the pinned ufbx reader.
* **Edits.** IDs, order, count, winding, axis Line and every SQL cell except
  the Sketch payload and its hash are preserved; the source hash never
  changes. Cylinder → frustum keeps the same Lines and names.
* **Refusals, each atomic and at a named stage:** crossing, isolated touch,
  several axis Lines, a collapsed or moved axis Line, near-axis vertices,
  self-intersection, hollow ↔ solid edits, reordered or foreign IDs, stale
  versions, aliases and occupied outputs, forged payloads (a v1 header over
  an axis Line, a stated Line the coordinates disagree with, prepared
  records the writer re-derives), cancellation and late delivery.
* **UI.** The real widgets create a cylinder by clicks, refuse an isolated
  touch and a crossing with Undo, publish through the creates worker (Save
  Cancel and a failed Open keep the draft), and match the peer CLI's
  semantics and STL bytes. Reopened, the editor names the axis Line; one
  1 mm-snapped drag makes a frustum with one Undo step; dragging an axis end
  off the axis is refused in preview and Escape cancels the gesture; making
  it hollow is refused. The worker's copy and the peer CLI's copy have equal
  objects, equal references (the source's, with none for the axis Line) and
  byte-equal STL and FBX.

### Regression runs

* **Workspace, native:** `cargo test --workspace --no-fail-fast`: 2031
  passed, 2 ignored, 19 failed. All 19 are N/A here:
  - 18 are PlaneGCS suites (7 app `constraints::tests::native_*`, 11 CLI in
    `annular_constraints`, `circle_constraints` and `edit_constraints`).
    With `FERRITECAD_REQUIRE_OCCT=1` and no solver linked, their guard fails
    by design; CI runs them with PlaneGCS.
  - `validate::validation_really_read_only_permissions` refuses to run as
    root. Run as uid 1001 (`fcad`), it passes.
* **Pinned corpus with the zero-area filter:** `export_stl` 9,
  `export_fbx` 9, `export_fbx_identity` 1, `export_fbx_complex` 3,
  `export_scene_complex` 2, `occurrence_identity_complex` 2,
  `complex_step_pixels` 3, `imported_step_pixels` 3, `import_step` 13,
  `shared_step_import` 3, `json_import` 3, and all of `ferritecad-occt`: all
  pass unchanged.

## Stub results (no OCCT)

The stub CLI links no OCCT or PlaneGCS library (`ldd` lists none).
* `axis::solid_revolution_discovery_payload_and_writer_without_kernel` runs
  and passes. In the stub it also proves that creating a solid profile and
  editing a solid document both exit 2 `unsupported`, with nothing written
  and the source unchanged.
* The five native `axis::` tests and the app test print `skipped: …` and
  return; they are counted as skips, not passes.

## The real §27B CLI

On a §27C cylinder (solid, v2), the base binary:
* refuses to create the same profile (exit 2, the old "not strictly on the
  positive radial side");
* `validate`: exit 0 with warning `object.unknown-type` (Revolve v2);
* `inspect`: exit 0, `revolves` empty, the Sketch not editable;
* `rebuild`, `export-stl`, `export-fbx`, `print-topology`: exit 2, "preserves
  but cannot rebuild", no output written;
* `edit-sketch-copy`: exit 2 `unsupported`, "document cannot be edited: it
  requires feature.revolve.axis-closed.v1"; no output, source hash unchanged.

On a hollow bushing, old and new agree exactly:
* `inspect --json` is identical except the two additive `revolves[]` fields
  (`closure: "radial_clear"`, `axis_curve_id: null`), for a bushing made by
  either build;
* STL and FBX bytes are identical;
* the old and new `edit-sketch-copy` copies of an old-made bushing have
  equal SQL cells, all of them;
* the payload stays v1 with capabilities `core.part.v1` and
  `feature.revolve.v1` only.

## Mutations

Each mutation was applied to the source, the named gates run, and the file
restored from a byte copy (`git diff` empty); the positive gates were then
re-run and passed.

| # | Mutation | Caught by |
|---|---|---|
| M1 | Creation also writes a `RevolveFace` reference for the axis Line | 5 `axis::` tests: publication refuses, the fake name resolves to no face |
| M2 | The evaluator states the next Line as the axis Line (swapped provenance) | 5 `axis::` tests: the bridge refuses, "vertex … ends the axis segment but is not on the axis" |
| M3 | `record_revolve` accepts a face filed under the axis Line | first **survived**; `native_axis_closed_requests_are_checked_not_trusted` now forges such a history and catches it |
| M4 | `stated_revolution` accepts any stated/derived mismatch | 4 CLI tests (2 `axis::`, 2 §27B `edit::`) and the app test |
| M5 | Revolve v2 no longer requires `feature.revolve.axis-closed.v1` | 2 `axis::` tests |

## Recipe

The `FCAD_27C_AGENT_RECIPE` block was extracted from the contract by its
marker and executed with the fresh CLI and the ufbx reader:
`FCAD_27C_RECIPE_OK 24`. The §27A and §27B recipes were re-run against the
same CLI: `FCAD_27A_RECIPE_OK 12` and `FCAD_27B_RECIPE_OK 6`. The §27A recipe
needed two fixes: its "touch" refusal now uses an isolated touch, and it
asserted the Sketch was not editable, which had been false since §27B (the
base binary fails the same assertion).

## Checks

* `cargo fmt --all --check`: ok.
* `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  ok (PlaneGCS itself is not linked here).
* `check-licence-headers.sh`, `check-export-boundary.sh`,
  `check-solver-ownership.sh`, `check-notice-ownership.sh`: pass.
* `check-native-inventory.sh`: 57 of 58 checks pass. The one failure is the
  missing `jsonschema-cli`, which fails the same way on the base; the
  inventory inputs are unchanged.
* The fixture generator below was executed: 4 CLI solids and their STL/FBX,
  every FBX read by ufbx with 6 checks and 0 failures. Mesh volumes (from
  the generator's STL; tessellated approximations, not B-Rep volumes):

  | File | Exact | STL | Triangles |
  |---|---|---|---|
  | `cylinder-cli` | 1500π = 4712.388980 | 4709.288976 | 396 |
  | `cone-cli` | 500π = 1570.796327 | 1568.986041 | 1334 |
  | `shaft-cli` | 860π = 2701.769682 | 2699.481665 | 704 |
  | `frustum-cli` | 875π = 2748.893572 | 2745.583862 | 1332 |

## Resources

* Disk: 85% of the allowance used; about 5.6 GiB free at the end.
* Memory: well below the 15 GiB, no swap.
* One build at a time; no other user's files or processes were touched.

## Limits

* The window scenario below was not run: no GUI in the cloud.
* The solver-backed suites run only in CI.
* The mixed OCCT/no-solver gate ran here in the dev profile, where PlaneGCS
  is not linked; CI runs it as `--release --no-default-features`.

## macOS fixtures and window scenario (for the reviewer's Mac)

### Fixture generator

`FERRITECAD` is the bundled CLI
(`FerriteCAD.app/Contents/MacOS/ferritecad`). The marked block writes the
three CLI-created solids the window's creations are compared against, a
cylinder source to edit, and the CLI-equivalent edited copy:

```sh
# FCAD_27C_MAC_FIXTURES
set -euo pipefail
: "${FERRITECAD:?set FERRITECAD to the bundled CLI}"
dir="${1:-$PWD/ferrite-27c-fixtures}"
mkdir -p "$dir" && cd "$dir"
make() {
  printf '{"request_version":1,"points_mm":%s,"axis":"sketch_y","angle":"full_turn"}' "$2" > "$1.json"
  "$FERRITECAD" create-sketch-revolve "$1.json" -o "$1.fcad" --json
}
make cylinder-cli '[[0,0],[10,0],[10,15],[0,15]]'
make cone-cli '[[0,0],[10,0],[0,15]]'
make shaft-cli '[[0,0],[10,0],[10,5],[6,5],[6,15],[0,15]]'
cp cylinder-cli.fcad cylinder-source.fcad
python3 - "$FERRITECAD" <<'PY'
import json, subprocess, sys
cli = sys.argv[1]
c = json.loads(subprocess.run([cli, "inspect", "cylinder-source.fcad", "--json"], check=True,
                              capture_output=True, text=True).stdout)["result"]
row = c["sketches"][0]
assert row["editable"] and row["profile_feature"]["kind"] == "full_turn_revolve_axis_closed"
target = [[0, 0], [10, 0], [5, 15], [0, 15]]
vertices = [{"curve_id": v["curve_id"], "start_mm": p} for v, p in zip(row["vertices"], target)]
with open("frustum-cli.json", "w") as f:
    json.dump({"request_version": 1, "vertices": vertices}, f)
subprocess.run([cli, "edit-sketch-copy", "cylinder-source.fcad", "--sketch", row["sketch_id"],
                "--expect-version", c["content_version"], "--request", "frustum-cli.json",
                "-o", "frustum-cli.fcad", "--json"], check=True)
PY
for f in cylinder-cli cone-cli shaft-cli frustum-cli; do
  "$FERRITECAD" export-stl "$f.fcad" -o "$f.stl" --json
  "$FERRITECAD" export-fbx "$f.fcad" -o "$f.fbx" --json
done
```

### Window scenario

Run exactly one viewer under the watchdog, on the fixture folder outside the
checkout. Give the watchdog a `--log` name that does not exist yet, and do
**not** redirect the shell's stdout or stderr to that name: the watchdog
creates its log exclusively, and a §27B review attempt failed before any
viewer started because shell redirection had already created it. If the
watchdog's own output must be kept, send it to a different file.

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$dir/watch-27c.jsonl" --limit-mib 1536 --seconds 1800 \
  -- /path/to/FerriteCAD.app/Contents/MacOS/ferritecad-viewer
```

After Quit, look only at that PID's exit and the log; do not query the
application's state in a way that could relaunch it.

1. **Draw a cylinder.** Click `Create sketch + Extrude…`, click the canvas
   at (0,0), (10,0), (10,15), (0,15), **Close contour**, and choose
   **Feature: Revolve 360°**.
   * The canvas labels the axis "axis (Y) at X = 0 · radius X →".
   * The text says to keep every point at X > 0 for a part with a hole, or
     to put exactly one whole edge on X = 0 for a solid part, and that the
     profile may not cross the axis or touch it at a single point. It does
     not say that every point needs X > 0.
   * `Create in new file…` is offered.
2. **Refuse single touches and crossings.** Set vertex 1 X to 4: a red
   refusal says the profile "touches the axis alone" and the Create button
   disappears. **Undo draft** restores 0. Set it to −1: the refusal says the
   profile would cross the axis. **Undo draft**.
3. **Cancel, then publish.** `Create in new file…` → **Cancel**: nothing is
   written and the draft stays. Then save as `cylinder-gui.fcad` in the
   fixture folder (Go to Folder, basename only). The solid opens: one closed
   cylinder, no bore.
4. **The cone and the stepped shaft.** In the same PID, repeat steps 1 and 3
   with (0,0), (10,0), (0,15) as `cone-gui.fcad`, and with (0,0), (10,0),
   (10,5), (6,5), (6,15), (0,15) as `shaft-gui.fcad`. The cone comes to a
   point on the axis.
5. **Export the creations.** Export STL for all three through the GUI as
   `<name>-gui.stl`, and FBX for the cylinder as `cylinder-gui.fbx`.
6. **Edit a saved solid.** Open `cylinder-source.fcad` and click **Edit
   Sketch Profile — <UUID>…**.
   * A line reads "Solid part: edge 4 (Line <UUID>) stays on the axis at
     X = 0; every other point stays at X > 0." The UUID is the one
     `inspect --json` gives as `profile_feature.axis_curve_id`.
   * There is no Feature switch or height field; Remove and adding points
     are disabled.
7. **Drag, with one Undo.** Choose Snap **1 mm** and drag vertex 3 from
   X = 10 to X = 5. The top of the part narrows into a frustum. One **Undo
   draft** restores the whole drag; **Redo draft** brings it back.
8. **Refuse leaving the axis.** Drag vertex 1 (on the axis) to the right:
   the preview is refused ("touches the axis alone") with no Save button.
   Press Escape before releasing: the gesture is cancelled and the draft is
   the frustum again. Then type X = 1 for both vertex 1 and vertex 4: the
   refusal says a part "cannot change between solid and hollow". **Undo
   draft** twice.
9. **Cancel, then publish the copy.** `Save edited copy…` → **Cancel**:
   nothing is written. Then save as `frustum-gui.fcad`; the copy opens.
   Export STL and FBX through the GUI as `frustum-gui.stl` and
   `frustum-gui.fbx`. Quit.
10. **Compare with the CLI.**
    * `cmp frustum-gui.stl frustum-cli.stl` and
      `cmp frustum-gui.fbx frustum-cli.fbx` must be identical with **no**
      normalization: both copies keep the source's UUIDs.
    * `cmp cylinder-gui.stl cylinder-cli.stl`, and the same for the cone and
      the shaft, must be identical. The created FBX files differ only in the
      UUIDs each creation mints, so compare only their sizes.
    * `"$FERRITECAD" print-topology <file>` must say 3 of 3 references
      resolved for the cylinder and the frustum, 2 of 2 for the cone, and
      5 of 5 for the shaft. None of them names the axis Line.

Expected memory: the same order as §27A/B (about 200 MiB peak), exit 0,
swap 0.
