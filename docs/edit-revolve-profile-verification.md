# §27B verification — edit a saved full-turn Revolve profile

[Contract and recipe](edit-revolve-profile.md).

## Where and how this was run

* **Machine.** A Linux x86_64 cloud container (Linux 6.18, 16 GiB RAM).
* **Branch and base.** Branch `edit-revolve-profile` from base `5843f27`
  (tree `aa151264`, the merge of PR #53). All six workflows on `5843f27` were
  green before implementation started, `combined runtime layout` included.
* **Native stack.** The pinned OCCT 8.0.1 install and the pinned ufbx reader
  were reused. Neither OCCT nor PlaneGCS was rebuilt, and no bridge or
  `Cargo` input changed.
* **Build settings.** `CARGO_BUILD_JOBS=1`, sequential builds, and only the
  existing native and stub targets.
* **No solver locally.** PlaneGCS cannot be built here: network policy denies
  its Boost archive host. Every local native run is OCCT with no solver, and
  the solver-backed suites run only in the three-OS CI.
* **No GUI.** The window scenario below was not run in the cloud. The
  widget-level test drives the real egui widgets and the edit worker
  headlessly; no GUI pass is claimed.

## Changed and new files

`git diff --stat 5843f27`: 16 files, about 2,160 insertions and 60
deletions, plus this file.

* **Domain, one owner** (`ferritecad-document`):
  - `sketch_edit.rs`:
    - `SketchProfileUse` (`BlindExtrude`/`FullTurnRevolve`) replaces
      `SketchChoice.height_mm`;
    - `revolve_frame` beside the unchanged Extrude `frame`, which now
      delegates to `extrude_frame`;
    - the policy is chosen by kind (`PolygonExtrusion`/`FullTurnRevolution`);
    - `validate_coordinates` returns the checked points.
  - `lib.rs`: the export.
* **CLI/JSON** (`ferritecad-cli`):
  - `src/json.rs`: the additive `sketches[].profile_feature`;
  - `tests/revolve.rs`: the `edit` module is included, and the §27A contract
    test now expects the row to be editable;
  - new `tests/revolve/edit.rs`.
* **UI** (`ferritecad-app`):
  - `sketch.rs`:
    - `begin_edit` takes the profile's kind from the choice;
    - a Revolve draft shows the axis and the saved full turn, and no height
      field;
    - a new widget/worker/CLI test.
  - `sketch/drag_tests.rs`: the explicit profile kind.
* **CI and tools:**
  - `.github/workflows/runtime-layout.yml`: exact gates, the no-solver gate
    and the edit FBX directory;
  - `tools/check-fbx-complex.sh`: a ufbx loop over `revolve-edit-0/1`.
* **Docs:**
  - `README.md`, `docs/cli-capabilities.md`, `docs/cli-json-v1.md`,
    `docs/edit-sketch-copy.md`, `docs/full-turn-revolve.md`,
    `docs/implementation-plan.md`;
  - new `docs/edit-revolve-profile.md`;
  - this file.

`EditSketchRequest`, `edit_sketch_copy`, `edit_object_copy`,
`Document::write_sketch_geometry`, `edit-sketch-copy` and its request v1 are
unchanged. The new behaviour enters only through `coordinate_choice`, which
discovery, preparation and the writer's re-derivation all call.

## Native results (OCCT 8.0.1, no solver)

### `ferritecad-cli --test revolve`, 11 of 11 (6 §27A + 5 §27B)

#### `edit::native_revolve_profile_edits_keep_names_measure_cache_sql_and_exports`

**Cases.** Eight edits: four cases, each in both windings.

| Case | Profile before → after | Volume before → after |
|---|---|---|
| Smaller bushing | [4,0],[10,0],[10,15],[4,15] → [3,2.5],[8,2.5],[8,12.5],[3,12.5] | 1260π → 550π |
| Bushing to cone | the outer wall's top end moves to (7, 15); that Line's face turns from a cylinder into a cone | 1260π → 855π |
| Stepped | → [5,1],[12,1],[12,4],[8,4],[8,14],[5,14] | 750π → 747π |
| Sloped | → [3,−1.25],[9,−1.25],[6,13.75],[3,13.75], a fractional Y shift | 855π → 720π |

**What is checked for each case:**
* **Volumes.** Expected volumes are written out from closed-form annulus and
  frustum formulas. The production Pappus helper is not used as the
  reference.
* **Discovery.** The row is `editable`, with `profile_feature.kind =
  full_turn_revolve`, the Revolve's `feature_id` and the Body's `body_id`,
  and `features: []`.
* **Result.** The reply names the destination, `document_id` and
  `sketch_id`. The source bytes are unchanged.
* **SQL.** Every cell of every table is compared, source against copy. Only
  the Sketch row's `payload`/`payload_hash` differ; every other row,
  `meta.modified_at`, capabilities, dependencies and topology refs are
  identical, and nothing is added.
* **After the edit:**
  - the vertices are exactly the requested ones under the saved UUIDs;
  - `bodies` and the Revolve entry are unchanged, apart from the segment
    coordinates;
  - `content_version` changes.
* **B-Rep.** The cold volume is within 1e-6 relative of the analytic volume.
  One face per Line. Every stored `RevolveFace` resolves under its old UUID
  to one face, with its kind checked against the Line's current
  coordinates:
  - an annular plane at its y;
  - a cylinder of its radius on the Y axis;
  - a cone on the Y axis at the Line's radius at every height.

  Every face goes all the way round, and every face has exactly one name.
  In the cone case the one Line whose kind changes is named: cylinder
  before, cone after, under the same reference.
* **Cache.** The source's warmed sidecar, copied beside the copy, records a
  **Miss** and returns the new geometry. The next rebuild is a **Hit** with
  the same volume. The cold build, the Miss and the Hit agree.
* **print-topology.** It names every Line UUID; n of n references resolve.
* **STL.** The independent binary parser finds a closed, consistently
  oriented mesh within the turned profile and the radial/axial bounds. Its
  volume is within the chord band of the analytic value. The mesh is not
  treated as the exact B-Rep.
* **FBX.** Two copies (`bushing-to-cone`, `stepped`) are exported to
  `FCAD_REVOLVE_EDIT_ARTIFACTS`. The pinned ufbx read them locally through
  the same loop CI runs: `checks=6 failures=0` each, and
  `FCAD_REVOLVE_EDIT_UFBX_EXECUTED`.
* **Also covered.** A second edit of an edited copy (annulus 6/2.5 × 20).
  An Extrude Sketch reports `{"kind":"blind_extrude","feature_id":…,
  "height_mm":10.0}`, and `revolves` is `[]`.

#### `edit::native_revolve_profile_edit_refusals_preserve_every_file`

Each refusal exits 2. The source bytes are unchanged and the directory
listing is identical.

* **Geometry:**
  - axis contact and axis crossing;
  - a vertex inside the 1e-6 mm clearance;
  - self-intersection;
  - zero area;
  - a coordinate beyond 1e6 mm;
  - a winding change.

  Messages are checked where the policy names them ("positive radial
  side", "winding").
* **Identities.** Reordered, duplicate, foreign and missing curve UUIDs are
  refused with "every saved curve UUID exactly once".
* **Version and target.**
  - a stale version: "source has changed";
  - an unknown Sketch: "does not exist";
  - an occupied destination: "already exists", and its bytes are kept;
  - the source itself, a symlink to it or a hard link to it as the
    destination.
* **Structure.** Constraints, a transformed plane and an extra object each
  refuse at the job with the structural message.

#### `edit::native_revolve_profile_edit_delivery_late_guards_and_cancellation`

* **Exit 7.** With stdout closed, alone and together with stderr, the edit
  exits 7 and the publication stands: it is measured at 747π.
* **Shared job with `OcctKernel`:**
  - cancelled at progress ≥ 0.4, after the baseline check and before the
    write: `Cancellation`, no copy and no scratch;
  - the source renamed at progress ≥ 0.95 (asserted to have run): the late
    version guard refuses, with no output and no scratch;
  - cancellation at progress 1.0: published anyway, measured, and with the
    same curve UUIDs.

#### `edit::occt_without_solver_edits_a_revolve_profile`

Mixed OCCT/no-solver, run with `FERRITECAD_REQUIRE_PLANEGCS=0`: ok.

#### `edit::revolve_profile_discovery_protocol_and_writer_without_kernel`

This test also runs in the stub build.
* **Discovery.** The row is `editable` with the saved vertices and
  `profile_feature` (axis, extent, clearance). The Revolve keeps
  `profile.available` and stays out of `features`. The constraint, circle
  and annulus editors, `cut_history_v3` and `edit_extrude` refuse.
* **Structures outside the class** refuse in discovery, with `vertices` and
  `profile_feature` null.
* **Protocol refusals** exit 2 with nothing written:
  - not JSON, an unknown field, `request_version` 2 or null;
  - a text coordinate;
  - over 65536 bytes;
  - a non-UTF-8 output path under `--json`.
* **Preparation** refuses axis contact, axis crossing, self-intersection,
  degeneracy, a winding change, and reordered, missing, duplicate or
  foreign UUIDs.
* **Writer forgeries** of the prepared payload each leave the file
  byte-identical:
  - a consistent profile on the axis;
  - an end moved without its start;
  - a constraint slipped in;
  - a construction Line.

  The honest prepared payload is accepted by the same writer.

### App: `sketch::tests::native_revolve_profile_edit_widgets_worker_and_cli_publish_one_copy`

* **Setup.** A §27A bushing is loaded through `ferritecad_scene::snapshot_of`
  (the real async-load reading).
* **Opening the editor.** `begin_edit` gives a Revolve draft with no height.
  The frame shows the axis label, the full-turn line and the Revolve header;
  there is no Blind height field and no Extrude/Revolve switch.
* **Editing through real fields.** X 4.5→3.5 (twice) and 10.5→8.5 (twice)
  through the real fields; Y is set on the draft and recorded.
* **Refusal and history.** X = −0.5 is refused as "not strictly on the
  positive radial side", and the refusal is painted. Undo, Redo, Undo run
  through the real buttons.
* **Worker.** `Save edited copy…` sends the saved UUIDs in saved order.
  - A stale worker reply changes nothing.
  - An occupied destination publishes nothing and keeps the draft.
  - The publication succeeds.
  - A failed async Open restores the Revolve draft; an accepted one retires
    it.
* **Peer CLI.** The same request through `edit-sketch-copy` gives equal
  `read_semantics`, meta, objects and refs. The refs equal the source's: no
  new names.
* **Result.** The kernel volume is π(8.5² − 3.5²)·10 within 1e-6. The
  **STL and FBX bytes of the UI and CLI copies are identical**, with no
  normalization, because both keep the same identities.

### Regression runs

* **CLI (native):**

  | Suite | Result |
  |---|---|
  | `edit_sketch` | 5 of 5 |
  | `edit_circle` | 5 of 5 |
  | `edit_annular` | 6 of 6 |
  | `edit_extrude` | 2 of 2 |
  | `circular_cut` (Cut-base coordinate edits included) | 42 of 42 |
  | `edit_circular_cut` | 8 of 8 |
  | `json_v1` | 13 of 13 |
  | `create` | 9 of 9 |
  | `sketch_extrude` | 4 of 4 |
  | `print_topology` | 8 of 8 |
  | `rebuild` | 9 of 9 |

* **Library crates:** document (91 + 1 existing ignored), jobs (53), eval
  and topology all pass.
* **App:** 324 passed, including the §25C drag tests with the new explicit
  profile kind.
* **Needs PlaneGCS (N/A here):** the 7 `constraints::tests::native_*` app
  tests and 6 `edit_constraints` CLI tests fail on their own "requires
  PlaneGCS" assertion. That is the known-unsupported solver suite, unrelated
  to this change; CI runs it.
* **Permission test.** `validation_really_read_only_permissions` refuses to
  run as root by design. Run under the unprivileged uid 1001 (`fcad`) with
  its own `TMPDIR`: ok.

## Stub results (no OCCT)

* **Tests.** The `revolve` binary: 11 passed. The 5 kernel-free tests,
  including the §27B discovery/protocol/writer test, run; the 6 native ones
  print `skipped:`.
* **Linking.** `ldd` on the stub `ferritecad` shows no `libTK*` or
  `planegcs`.
* **Discovery.** Stub `inspect --json` on a §27A document reports the row
  `editable` with `full_turn_revolve` and 4 vertices.
* **Actual order of refusals.**
  - Request parsing (version, JSON, size, UTF-8 paths) happens before the
    kernel. A request v2 is refused as "unsupported … request_version".
  - The command then opens the kernel before the job. So in the stub, a
    stale version or an unsupported structure is reported as "no Open
    CASCADE" (exit 2, nothing written), not as a stale-version refusal.
  - The stale and structural refusals are verified natively above.

## Mutations

Each mutation was applied by a script that kept a backup, then restored. The
file's SHA-256 was checked, and all 7 positive gates re-ran green.

| # | Mutation | Killed by |
|---|---|---|
| 1 | `replace_sketch_coordinates` ignores the new coordinates, so the copy keeps the old geometry | `edit::native_revolve_profile_edits_…`, assertion "exactly the Sketch row changes" (0 changed rows) |
| 2 | The writer skips its re-derived payload comparison (`checked.payload != prepared.payload && false`) | `edit::revolve_profile_discovery_protocol_and_writer_without_kernel`: the forged "end moved without its start" was accepted and `expect_err` failed |

## Recipe

The `FCAD_27B_AGENT_RECIPE` block was extracted from the contract exactly as
documented, and run with the fresh debug CLI and `read_production`. It
printed `FCAD_27B_RECIPE_OK 6`:
* bushing and sloped, in both windings, moved;
* the bushing's cylinder turned into a cone, in both windings;
* ufbx `checks=6 failures=0` for each copy;
* axis, stale, reordered and occupied refusals, with the source unchanged.

## Checks

* `cargo fmt --all -- --check`: ok.
* `cargo clippy --workspace --all-targets -- -D warnings`: ok. It was run
  without `--all-features` because planegcs is not buildable here; CI runs
  the full form.
* These scripts pass: `check-licence-headers.sh`, `check-export-boundary.sh`,
  `check-solver-ownership.sh`, `check-notice-ownership.sh` and
  `check-planegcs-pins.sh`.
* `runtime-layout.yml` parses. The new ufbx loop was executed locally on
  real FBX output.
* The native inventory, SBOM and notices inputs are unchanged, so nothing
  was regenerated.

## Resources

* Disk: 75% of the allowance used (9.7 GiB free).
* Memory stayed far below 16 GiB, with no swap.
* One build at a time.

## Limits

* The window scenario below was not run.
* Solver-backed suites run only in CI.
* Local clippy did not use `--all-features`.

## macOS fixtures and window scenario (for the reviewer's Mac)

### Fixture generator

`FERRITECAD` is the bundled CLI
(`FerriteCAD.app/Contents/MacOS/ferritecad`). The marked block creates the
two sources and the CLI-equivalent copies to compare against:

```sh
# FCAD_27B_MAC_FIXTURES
set -euo pipefail
: "${FERRITECAD:?set FERRITECAD to the bundled CLI}"
dir="${1:-$PWD/ferrite-27b-fixtures}"
mkdir -p "$dir" && cd "$dir"
printf '%s' '{"request_version":1,"points_mm":[[4,0],[10,0],[10,15],[4,15]],"axis":"sketch_y","angle":"full_turn"}' > bushing.json
"$FERRITECAD" create-sketch-revolve bushing.json -o bushing.fcad --json
cp bushing.fcad bushing-cone-source.fcad
python3 - "$FERRITECAD" <<'PY'
import json, subprocess, sys
cli = sys.argv[1]
for source, target, out in (
    ("bushing.fcad", [[3, 2.5], [8, 2.5], [8, 12.5], [3, 12.5]], "bushing-cli.fcad"),
    ("bushing-cone-source.fcad", [[4, 0], [10, 0], [7, 15], [4, 15]], "cone-cli.fcad"),
):
    c = json.loads(subprocess.run([cli, "inspect", source, "--json"], check=True,
                                  capture_output=True, text=True).stdout)["result"]
    row = c["sketches"][0]
    assert row["editable"] and row["profile_feature"]["kind"] == "full_turn_revolve"
    vertices = [{"curve_id": v["curve_id"], "start_mm": p} for v, p in zip(row["vertices"], target)]
    with open(out + ".json", "w") as f:
        json.dump({"request_version": 1, "vertices": vertices}, f)
    subprocess.run([cli, "edit-sketch-copy", source, "--sketch", row["sketch_id"],
                    "--expect-version", c["content_version"], "--request", out + ".json",
                    "-o", out, "--json"], check=True)
PY
for f in bushing-cli cone-cli; do
  "$FERRITECAD" export-stl "$f.fcad" -o "$f.stl" --json
  "$FERRITECAD" export-fbx "$f.fcad" -o "$f.fbx" --json
done
```

### Window scenario

Run one viewer under the 1536 MiB watchdog. After Quit, check only the PID
and the log; do not query the application's accessibility state.

1. **Open the editor.** Open `bushing.fcad` and click **Edit Sketch Profile —
   <UUID>…**.
   * The header reads "XY · mm · Line polygon · Revolve 360° about the
     sketch Y axis · NewBody".
   * The next lines say X is the radius, and "Edit exact coordinates. Curve
     IDs, order, closure and the saved full turn about Y are retained."
   * The canvas shows the orange axis at X = 0 labelled
     "axis (Y) · radius X > 0 →".
   * "Revolve: one full turn (360°) about the sketch Y axis, through X = 0."
     replaces the height field.
   * There is no Feature switch; Remove and adding points are disabled.
2. **Enter the new profile.** Set the four vertices to (3, 2.5), (8, 2.5),
   (8, 12.5), (3, 12.5) in the numeric fields. `Save edited copy…` is
   enabled.
3. **Refuse the axis.** Set vertex 1 X to 0.
   * A red refusal says "not strictly on the positive radial side", and the
     Save button disappears.
   * **Undo draft** restores 3; **Redo draft** brings back 0; **Undo draft**
     again.
4. **Cancel the save.** `Save edited copy…` → **Cancel**. Nothing is
   written, and the draft stays.
5. **Publish.** `Save edited copy…` → choose the fixture folder with Go to
   Folder, and type only the basename `bushing-gui.fcad`. The copy opens:
   the part is thinner, shorter and moved up by 2.5 mm.
6. **Export.** Export STL and FBX through the GUI as `bushing-gui.stl` and
   `bushing-gui.fbx`. Quit.
7. **Compare with the CLI copy.** `cmp bushing-gui.stl bushing-cli.stl` and
   `cmp bushing-gui.fbx bushing-cli.fbx` must both be identical with **no**
   normalization: both copies keep the source's UUIDs.
   `"$FERRITECAD" print-topology bushing-gui.fcad` names 4 of 4 references,
   each `revolve face from segment <UUID>`.
8. **The cone case.** Repeat steps 1, 2, 5 and 6 on
   `bushing-cone-source.fcad`, changing only vertex 3 from (10, 15) to
   (7, 15), and save as `cone-gui.fcad`. The outer wall becomes a cone.
   Compare with `cone-cli.stl` and `cone-cli.fbx` byte for byte.

Expected memory: the same order as §27A (about 207 MiB peak), exit 0,
swap 0.
