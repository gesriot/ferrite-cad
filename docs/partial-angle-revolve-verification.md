# §27D — verification record

[Contract](partial-angle-revolve.md).

Each claim below is marked **local** (run in the cloud container),
**CI** (GitHub Actions on the exact PR head) or **N/A** (not run, with the
reason).

## Where and how this was run

* **Container.** A Linux cloud container with the pinned OCCT 8.0.1 already
  installed (not rebuilt) and no PlaneGCS.
* **Build.** One cargo build at a time (`-j2`), on the existing native and
  stub targets, with the disk checked between steps.
* **Base.** Branch `partial-angle-revolve` from `origin/main` f45df99 (the
  squash of #55).
* **No GUI.** No window, viewer or GPU was launched. The UI tests below
  drive the real egui widgets headlessly and are not a GUI run. The window
  scenario at the end is for the reviewer's Mac.
* **Mixed build.** The CLI and app crates have no default features, so
  every native run here is the mixed configuration: OCCT without PlaneGCS.
  The full OCCT+PlaneGCS matrix is CI's.

## Decisions made before code

* **The angle.**
  - A finite number of **degrees**, stored exactly as given.
  - Accepted from 0.01° to 359.99°, both inclusive.
  - Right-handed about the sketch +Y axis, starting at the profile: (x, y)
    sweeps to (x·cos φ, y, −x·sin φ).
  - Exactly 360° is refused ("state a full turn"), never rewritten. Nothing
    is reduced modulo 360.
* **Why those limits.** They come from a probe of the kernel and of float
  precision, recorded in the contract.
  - OCCT builds valid sectors with the exact volume from 1e-6° to
    359.999999°, including at a radius of 1e6 mm.
  - The single-precision mesh loses whole faces at 1e-6° and keeps every
    face at 0.01° and at 359.99°.
  - 0.01° is eight orders of magnitude above `Precision::Angular`.
* **Layouts.** Revolve payload v3 is a sector with a bore; v4 is a sector
  closed on the axis. A new capability, `feature.revolve.partial.v1`.
  Full turns stay v1/v2, byte-identical. No SQL schema change.
* **Caps.**
  - A new role, `RevolveCap { side }`. It is not `ExtrudeCap`, and each
    role resolves only against its own feature kind.
  - The two caps are read in the bridge from
    `BRepSweep_Revol::FirstShape/LastShape` of the profile face, checked
    against the solid, and registered as the solid's own instances. The
    solid's end cap is REVERSED relative to `LastShape`.
  - Archive tags 16 and 17.
* **ABI.** `fc_occt_revolve` is unchanged. The new entry points are
  `fc_occt_revolve_partial(double angle_degrees)` and
  `fc_occt_revolve_caps`.
* **Request v2.** `extent: {"kind":"full_turn"} | {"kind":"angle","degrees":N}`,
  strict at every level. `FullTurn {}` is a struct variant so an extra
  field cannot be ignored. Request v1 is unchanged.
* **Editing.** A saved sector is not coordinate-editable in this slice. The
  refusal is named in edit preparation, which discovery and the writer
  share. Angle editing is out of scope.
* **UI.** One more Feature option, **Revolve angle**, with an **Angle °**
  field. It is implemented that way rather than as a sub-choice under
  Revolve, so the full-turn option keeps its label, text and tests.
* **Mesh (found while testing).**
  - A cone sector's mesh has collinear slivers along its first ruling.
    §27C's float-zero-area filter drops some of them, leaving T-junctions.
  - The independent STL checks split unmatched edges at vertices lying on
    them, then require every piece to meet its reverse.
  - A real hole still fails, and full-turn meshes are unchanged.

## Native results (OCCT 8.0.1, no solver) — local

### New tests (13)

| Test | What it shows |
|---|---|
| `revolve_occt::native_partial_revolutions_name_two_caps_from_the_sweep_history` | Cylinder, bushing and cone, both windings, at 90/180/270/137.5°: kernel volume = full × θ/360 (1e-9); one start and one end face, planar, distinct, not a Line's face; outward normals (0,0,1) and (−sin θ,0,−cos θ) from the mesh winding; area = profile area; `record_revolve` accepts a sector and refuses it as a full turn or with a missing cap; the caps survive the named archive. |
| `ffi::tests::the_partial_entry_refuses_what_is_not_a_sector_and_a_full_turn_has_no_caps` | The bridge entry refuses 0, −0, −90, 360, 720, NaN, ±∞ as input; a 90° bushing has 6 faces and the exact volume; a full turn answers no caps (unsupported). |
| `partial::native_partial_revolutions_measure_caps_names_cache_and_exports` | 32 CLI sectors plus 0.01°, 359.99° and 100/3°. Checked for each: discovery, capabilities, payload v3/v4, cold/Miss/Hit rebuilds, every name, `print-topology`, and the independent STL. |
| `partial::partial_revolution_requests_refuse_every_other_turn_atomically` | 14 bad extents and 6 bad request shapes → exit 2, `input`, nothing written; in a stub build a valid sector → `unsupported`. |
| `partial::native_partial_revolve_guards_delivery_and_cancellation` | Occupied output and request aliases refused, bytes kept; closed stdout → exit 7 with an intact sector; a cancelled job publishes nothing and leaves no scratch. |
| `partial::native_saved_partial_revolve_refuses_coordinate_editing_atomically` | `edit-sketch-copy` → exit 2 `unsupported` naming a partial Revolve; nothing written; source bytes unchanged. |
| `partial::partial_revolution_payload_capabilities_and_discovery_without_kernel` | v3/v4 written without a kernel: capability rows, a stored extent, a cache key that moves with the angle, `inspect` discovery; an envelope forged to the full-turn layout is refused ("does not match what it holds"). |
| `partial::occt_without_solver_turns_a_partial_revolve` | The mixed OCCT/no-solver gate. |
| `model::tests::a_partial_angle_is_a_validated_extent_with_its_own_layout` | Angle bounds; layouts 1–4; capabilities; readable versions; the payload round trip; stored 360/0/720 refused on decode; the end faces' meanings differ from each other and from `ExtrudeCap`. |
| `kernel::tests::a_partial_turn_keys_by_its_angle_and_leaves_the_full_turn_key` | Key by angle bits, full-turn key unchanged, `PartialTurn` bounds. |
| `result::tests::a_revolution_reports_no_caps_or_one_distinct_face_for_each` | `RevolveResult::validate` cap rules. |
| `codec::tests::a_sectors_end_faces_keep_their_own_tags` | Archive round trip with tags 16/17, never read back as extrusion caps. |
| `sketch::tests::partial_revolve_draft_widgets_state_the_angle_and_refuse_others` + `native_partial_revolve_draft_worker_and_cli_publish_equivalent_models` | Real widgets: the Revolve angle choice, header, axis, invalid angles refused with the Create button hidden, Undo/Redo through typed angles, switching features keeps the angle. The real worker: Save Cancel, an occupied destination, publish, Open. Compared with the CLI: STL bytes equal, FBX equal in full after mapping the two Body UUIDs (3 occurrences each). |

The independent STL check inside the CLI tests requires:
* closed and oriented (T-junctions excepted as above);
* a positive signed volume within the chord band × θ/360;
* every vertex inside the turned profile;
* every vertex with r > 1e-3 mm inside [0°, θ] (1e-3° tolerance), with
  both ends reached;
* each end face, found by plane, outward normal and in-plane side,
  covering the profile's area to 1e-4.

A full turn fails this check: it has no end faces and reaches every angle.

### Regression — local

* `cargo test --workspace` (native, dev, `-j2`): 2045 passed, 2 ignored,
  19 failed. Those 19 are exactly the ones §27C records as N/A here:
  - 18 are PlaneGCS suites: 7 app `constraints::tests::native_*`, and 11 CLI
    tests in `annular_constraints`, `circle_constraints` and
    `edit_constraints`;
  - 1 is `validate::validation_really_read_only_permissions`, which cannot
    hold as root. It passed when rerun as uid 1001.

* Existing Revolve suites: `revolve` CLI 23/23 (17 earlier plus 6 new),
  and `revolve_occt` 6/6.
* Real recipes, extracted from the docs and executed on the new CLI:
  - §27A `FCAD_27A_RECIPE_OK 12`;
  - §27B `FCAD_27B_RECIPE_OK 6`;
  - §27C `FCAD_27C_RECIPE_OK 24`;
  - §27D `FCAD_27D_RECIPE_OK 48`, with the pinned ufbx reader and again
    without it.
* `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets
  -- -D warnings` are clean natively. CI runs clippy with `--all-features`.

## Stub results (no OCCT) — local

* `revolve` CLI tests, `RUSTFLAGS="-D warnings"`: 23 passed. Every native
  test printed `skipped: this build has no Open CASCADE`, including the 4
  new native ones. The two kernel-free §27D tests ran and passed.
* App: `partial_revolve_draft_widgets_state_the_angle_and_refuse_others`
  passed; the worker test skipped with its reason. No warnings.
* The stub CLI on a valid v2 sector: exit 2 `unsupported` ("this build has
  no Open CASCADE"), nothing written.
* `ldd` lists no OCCT or PlaneGCS library.

## The real §27C CLI (f45df99, built from the merge commit) — local

On sector documents made by this build (v3 with a bore, v4 a cone):
* `inspect --json` exits 0. `revolves` is empty: the object is kept
  verbatim and never read as a full turn. The Sketch is not editable, and
  the document refusal says "it requires feature.revolve.partial.v1, which
  this build does not implement".
* `validate`, `rebuild --cold`, `export-stl`, `export-fbx`,
  `print-topology` and `edit-sketch-copy` exit 2 `unsupported`, naming the
  capability. No output file is written, and every source's SHA-256 is
  unchanged.

On full-turn documents made by this build (v1 with a bore; a v2 solid made
through request v2 `full_turn`):
* `inspect --json` is identical once the additive `angle_deg` (null) is
  removed.
* STL and FBX bytes are identical.
* The same `edit-sketch-copy` gives identical SQL in every table.

## Checks — local

* The macOS fixture generator below, extracted and run against the native
  CLI:
  - 3 sectors with their STL and FBX;
  - `print-topology` 8/8, 5/5, 4/4;
  - a second, independently created stepped sector gives identical STL
    bytes and `FCAD_27D_FBX_SAME`, while its raw FBX differs (UUIDs).
* Resources: disk stayed at or above 6.7 GB free; the pinned OCCT was not
  rebuilt.
* Not applicable to creation:
  - a stale version, because creation has no source version;
  - a source/destination race, because a create has no source document.

  The shared job's no-clobber, alias, exit 7 and cancellation paths are
  tested above.

## Mutations — local

Each mutant compiled and failed real assertions. The source was restored by
exact reverse replacement, and `git diff` was empty afterwards.

| Mutant | Caught by |
|---|---|
| M1 **full turn instead of partial**: request v2's angle validated, then published as a full turn | 4 CLI tests: discovery `"full_turn"` ≠ `partial_turn`; volume 4712.39 ≠ 2356.19; 1570.80 ≠ 599.96; the saved "sector" accepted an edit (exit 0 ≠ 2) |
| M2 **swapped cap identities** in the bridge (`LastShape` as start, `FirstShape` as end) | 3 CLI tests ("off the Start plane", "outside the profile on the Start face") and the kernel test ("[−1,0,0] is not [0,0,1]") |
| M3 **reversed sweep** (negative angle in the bridge) | 3 CLI tests ("ends at 356.4°, not 90") |

The comparer `tools/fbx/stl-matches-fbx.py` was also shown to reject one
FBX triangle with reversed winding, and one vertex moved by 1 µm (exit 1).

## FBX — local and CI

* **Reader.** The pinned ufbx reader (`read_production.c`, compiled with
  clang `-Wall -Wextra -Werror`) gains `--triangles`. It prints every
  mesh-instance triangle in world space, in file order and winding.
* **Local run.** The three CI artifacts (bushing 90°, cone 270°, fractional
  137.5°) all read with `--identity` `checks=6 failures=0`, and match their
  default-tessellation STL:
  - 168 triangles, worst 1.7e-18 m;
  - 984 triangles, worst 1.7e-18 m;
  - 498 triangles, worst 8.7e-19 m.
* **CI.** The same loop (`FCAD_REVOLVE_PARTIAL_UFBX_EXECUTED`) runs in the
  runtime workflow on Linux, macOS and Windows.

## CI gates added (runtime layout, exact name, no skip)

* **Mixed (`--no-default-features`):**
  `partial::occt_without_solver_turns_a_partial_revolve`.
* **Kernel:** `native_partial_revolutions_name_two_caps_from_the_sweep_history`.
* **CLI:** the five other `partial::` tests.
* **Kernel lib:**
  - `kernel::tests::a_partial_turn_keys_by_its_angle_and_leaves_the_full_turn_key`;
  - `result::tests::a_revolution_reports_no_caps_or_one_distinct_face_for_each`.
* **Document lib:**
  `model::tests::a_partial_angle_is_a_validated_extent_with_its_own_layout`.
* **UI:**
  - `sketch::tests::partial_revolve_draft_widgets_state_the_angle_and_refuse_others`;
  - `sketch::tests::native_partial_revolve_draft_worker_and_cli_publish_equivalent_models`.
* **OCCT lib:**
  `ffi::tests::the_partial_entry_refuses_what_is_not_a_sector_and_a_full_turn_has_no_caps`.
* **Topology lib:** `codec::tests::a_sectors_end_faces_keep_their_own_tags`.
* **FBX loop:** 3 partial FBX files with `--identity` and
  `--triangles` + STL join.

Every earlier gate is kept.

CI results on the exact PR head are reported in the pull request, not in
this file: editing this file would change the head being reported.

## Files changed

* `.github/workflows/runtime-layout.yml`
* `crates/ferritecad-app/src/sketch.rs`
* `crates/ferritecad-cli/src/json.rs`
* `crates/ferritecad-cli/src/revolve.rs`
* `crates/ferritecad-cli/src/topology.rs`
* `crates/ferritecad-cli/tests/revolve.rs`
* `crates/ferritecad-cli/tests/revolve/partial.rs`
* `crates/ferritecad-document/src/document.rs`
* `crates/ferritecad-document/src/lib.rs`
* `crates/ferritecad-document/src/model.rs`
* `crates/ferritecad-document/src/schema.rs`
* `crates/ferritecad-document/src/sketch_edit.rs`
* `crates/ferritecad-eval/src/cold.rs`
* `crates/ferritecad-eval/src/convert.rs`
* `crates/ferritecad-jobs/src/create.rs`
* `crates/ferritecad-jobs/src/lib.rs`
* `crates/ferritecad-jobs/src/polygon.rs`
* `crates/ferritecad-kernel/src/kernel.rs`
* `crates/ferritecad-kernel/src/lib.rs`
* `crates/ferritecad-kernel/src/request.rs`
* `crates/ferritecad-kernel/src/result.rs`
* `crates/ferritecad-occt-bridge/include/ferritecad_occt.h`
* `crates/ferritecad-occt-bridge/src/bridge.cpp`
* `crates/ferritecad-occt/src/ffi.rs`
* `crates/ferritecad-occt/src/kernel.rs`
* `crates/ferritecad-occt/tests/revolve_occt.rs`
* `crates/ferritecad-topology/src/archive.rs`
* `crates/ferritecad-topology/src/codec.rs`
* `crates/ferritecad-topology/src/map.rs`
* `crates/ferritecad-topology/src/resolve.rs`
* `docs/partial-angle-revolve-verification.md`
* `docs/partial-angle-revolve.md`
* `tools/check-fbx-complex.sh`
* `tools/fbx/stl-matches-fbx.py`
* `tools/unity-fbx-smoke/scripts/read_production.c`

## Limits

* **No GUI.** No window was run. The headless widget tests are not a GUI
  smoke; the reviewer's Mac scenario below is.
* **No PlaneGCS here.** PlaneGCS suites are N/A locally; CI runs them.
* **Not in this slice:**
  - angle editing, and coordinate editing of a saved sector (refused);
  - other axes, mirrored sweeps, Arc/Circle profiles;
  - booleans, several bodies, live preview, in-place Save.
* **Meshes near the limits.** A sector near the axis clearance at a tiny
  angle can keep a valid B-Rep yet lose a face's triangles in float. Export
  then refuses atomically (the §27C rule).
* **Cone sector meshes contain T-junctions** (see Decisions).
  Geometrically closed; a consumer that requires edge-manifold input may
  need to weld them.

## macOS fixtures and window scenario (for the reviewer's Mac)

### Fixture generator

`FERRITECAD` is the bundled CLI
(`FerriteCAD.app/Contents/MacOS/ferritecad`). The marked block writes three
CLI sectors, with their STL and FBX, for the window's creations to be
compared against, plus `same-fbx.py`. That script compares two
independently created FBX files in full, after mapping only each document's
own Body UUID. It was executed locally against the native CLI (see
Checks).

```sh
# FCAD_27D_MAC_FIXTURES
set -euo pipefail
: "${FERRITECAD:?set FERRITECAD to the bundled CLI}"
dir="${1:-$PWD/ferrite-27d-fixtures}"
mkdir -p "$dir" && cd "$dir"
make() {
  printf '{"request_version":2,"points_mm":%s,"axis":"sketch_y","extent":{"kind":"angle","degrees":%s}}' \
    "$2" "$3" > "$1.json"
  "$FERRITECAD" create-sketch-revolve "$1.json" -o "$1.fcad" --json
}
make stepped-cli '[[4,0],[10,0],[10,5],[7,5],[7,15],[4,15]]' 137.5
make cylinder-cli '[[0,0],[10,0],[10,15],[0,15]]' 180
make cone-cli '[[0,0],[10,0],[0,15]]' 270
for f in stepped-cli cylinder-cli cone-cli; do
  "$FERRITECAD" export-stl "$f.fcad" -o "$f.stl" --json
  "$FERRITECAD" export-fbx "$f.fcad" -o "$f.fbx" --json
done
cat > same-fbx.py <<'PY'
"""Compare two FBX files of independently created documents in full, after
mapping only each document's own Body UUID (3 occurrences each)."""
import json, subprocess, sys
cli, (a_doc, a_fbx), (b_doc, b_fbx) = sys.argv[1], sys.argv[2:4], sys.argv[4:6]
def mapped(doc, fbx):
    c = json.loads(subprocess.run([cli, "inspect", doc, "--json"], check=True,
                                  capture_output=True, text=True).stdout)["result"]
    [body] = [b["body_id"] for b in c["bodies"]]
    text = open(fbx, encoding="ascii").read()
    assert text.count(body) == 3, (fbx, text.count(body))
    return text.replace(body, "same-body")
assert mapped(a_doc, a_fbx) == mapped(b_doc, b_fbx), "FBX differs beyond the Body identity"
print("FCAD_27D_FBX_SAME", a_fbx, b_fbx)
PY
```

Extract and run:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/partial-angle-revolve-verification.md").read_text()
code = text.split("# FCAD_27D_MAC_FIXTURES\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27d-fixtures.sh").write_text(code)
EXTRACT
FERRITECAD=/path/to/FerriteCAD.app/Contents/MacOS/ferritecad bash ferrite-27d-fixtures.sh "$PWD/ferrite-27d-fixtures"
```

### Window scenario

Run exactly one viewer under the watchdog, on the fixture folder outside the
checkout. Give the watchdog a `--log` name that does not exist yet and do
not redirect shell output to it.

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$dir/watch-27d.jsonl" --limit-mib 1536 --seconds 1800 \
  -- /path/to/FerriteCAD.app/Contents/MacOS/ferritecad-viewer
```

1. **Draw and choose the angle.** Click `Create sketch + Extrude…`, enter
   (4,0), (10,0), (10,5), (7,5), (7,15), (4,15), **Close contour**, and
   choose **Feature: Revolve angle**.
   * The header reads "XY · mm · Line polygon · Revolve through an angle
     about the sketch Y axis · NewBody".
   * The canvas labels the axis "axis (Y) at X = 0 · radius X →".
   * An **Angle °** field shows 90.
   * The text names 0.01°–359.99° and says that 360° is Revolve 360°.
2. **Refuse other angles.** Type each of these in turn; each shows a red
   refusal in place of `Create in new file…`:
   * 360: "state a full turn instead";
   * 0 and −45: "not positive";
   * 359.995: "within 0.01° of a full turn";
   * 0.001: "below the 0.01° minimum";
   * `abc`.
3. **Undo/Redo.** **Undo draft** steps back through the typed angles to 90;
   **Redo draft** brings back 360. Undo again, then type **137.5**.
   Switching to **Revolve 360°** and back keeps 137.5.
4. **Cancel, then publish.** `Create in new file…` → **Cancel**: nothing is
   written and the draft stays. Then save as `stepped-gui.fcad` in the
   fixture folder (Go to Folder, basename only). It opens: a 137.5° sector
   of the stepped bushing, two flat end faces, the first in the sketch
   plane.
5. **Two more.** In the same PID, repeat steps 1, 3 and 4:
   * (0,0), (10,0), (10,15), (0,15) at 180 as `cylinder-gui.fcad`: half a
     solid cylinder;
   * (0,0), (10,0), (0,15) at 270 as `cone-gui.fcad`: three quarters of a
     cone.
6. **Export through the GUI.** STL for all three as `<name>-gui.stl`, and
   FBX for all three as `<name>-gui.fbx`.
7. **Editing is refused.** Open `stepped-gui.fcad`. The button `Edit Sketch
   Profile — <UUID>…` is shown disabled. Hovering it shows the refusal
   `inspect --json` reports: "coordinate editing of a partial Revolve
   (137.5° sector) is not supported in this build; only a full-turn Revolve
   profile can be edited". Quit.
8. **Compare with the CLI.**
   * `cmp stepped-gui.stl stepped-cli.stl`, and the same for the cylinder
     and the cone, must be identical.
   * `python3 same-fbx.py "$FERRITECAD" stepped-gui.fcad stepped-gui.fbx
     stepped-cli.fcad stepped-cli.fbx` prints `FCAD_27D_FBX_SAME`; the
     same for the cylinder and the cone. Raw `cmp` of these FBX files
     differs by design: independently created documents mint different
     UUIDs, and equal sizes prove nothing.
   * Exporting a GUI-created document through the CLI must give bytes
     identical to the GUI export (`cmp`), with no mapping.
   * `"$FERRITECAD" print-topology stepped-gui.fcad` says 8 of 8 references
     resolved, including `revolve start cap` and `revolve end cap`;
     cylinder 5 of 5; cone 4 of 4. None names an axis Line.

Expected: exit 0, memory of the same order as §27A–C (about 200 MiB peak),
swap 0.

## Independent macOS review — 2026-09-25

Reviewed PR #56 at `992f2800d38a6ae3f72f7ad02015f66fa470a883` against
`f45df99d52a59fecc46e02c7f5d1f5984bd817fb`. No product geometry, persistence or
UI implementation fix was needed. Review corrected the proof and its routing:

* The STL/FBX join accepted nine `nan` coordinates as a perfect match
  (`exit 0`, `worst_m=0`). It now rejects non-finite positions on either side,
  empty/truncated STL and missing, repeated or inconsistent reader reports.
  An executable test covers two positive oriented matches and eleven negative
  cases, including winding and coordinate changes. It runs in the existing
  FBX campaign. The real sector files still match.
* `tools/fbx/**` now triggers the runtime workflow, since that helper is an
  input to its proof. Grouped environment writes also clear actionlint's
  SC2129 finding. README, the shared CLI contract, capabilities and the plan
  now describe the partial turn rather than leaving discovery to a new page.

Local sequential native builds reused the installed pinned OCCT/PlaneGCS and
existing target. Core packages: 643 harness passes, of which two are explicit
no-solver-only N/A; one older timing benchmark ignored. OCCT: 140; selected
CLI suites: 119; sketch app: 36; edit app: 9. Thus **945 executed native tests**,
no failures and no native geometry skips. Fmt, workspace clippy all targets and
features with `-D warnings`, licence headers, export boundary, shellcheck,
actionlint and whitespace checks passed. A genuine stub (`NOTFOUND`, no native
imports) executed seven protocol/discovery tests and explicitly skipped 16
geometry tests. Mixed OCCT/no-solver executed the exact partial-turn gate;
imports were checked and the native CLI restored afterward.

The Markdown recipe executed all 48 cases with the rebuilt pinned ufbx reader.
Six local sector STL/FBX pairs passed the strengthened triangle join. The real
§27C bundled CLI could inspect both partial document classes read-only, but
validate/rebuild/STL/FBX refused (exit 2, including the new cap capability).
Sources stayed byte-identical and no outputs appeared. Full-turn discovery
matched after removing only the additive null `angle_deg`; old/new CLI STL and
FBX exports of the same full-turn document were byte-identical.

Initial exact-head CI was independently read from GitHub logs, including the
manual runtime run `36213931917` which the PR's short rollup omitted. On each
of Linux/macOS/Windows: **240 distinct required gate names, 279 executions**,
no required skips, 77 printed successful ufbx summaries plus three redirected
triangle-reader runs proved by their successful joins (80 reads total).
These are results for the incoming head; CI of the review commit must finish
before merge and is reported in the PR.

### Observed window coverage and interruptions

The fresh relocated, ad-hoc-signed bundle passed strict signature and loader
checks without DYLD variables. An initial owned viewer was stopped by the
watchdog at **216.282 MiB** because system pressure became elevated (level 2),
not because the 1536 MiB process cap was reached; swap remained zero. That run
is not a GUI pass. After native checks and pressure returning to normal, a
second guarded viewer was used.

Observed in that window: exact six-point stepped profile; Revolve angle;
visible refusals for 0, 360 and nonnumeric input; angle Undo/Redo; preservation
of 137.5 when switching full/partial mode; Save Cancel preserving the draft;
successful publication and async Open of the 137.5-degree stepped sector.
The saved GUI-created model, exported by the bundled CLI, has byte-equal STL
and identity-normalized FBX against the independent CLI creation, and all
**8/8 refs** resolve, including both named caps. This is not a claim that the
GUI export buttons were exercised.

CUA then repeatedly returned `noWindowsAvailable` for coordinate clicks while
its AX tree still named the live window; reconnecting did not restore clicks.
The owned viewer was closed through its native close button and exit 0 was
verified by PID, without querying the app afterward. Peak **196.939 MiB**,
pressure normal and swap zero throughout that second run. Remaining window
steps are both GUI export buttons, cylinder/cone creation and the refusal
hover. They remain unverified here; headless checks do not substitute for
these observations. Any later completion is recorded in the PR review log.
The cause of the older user OOM is not established by these runs.

Review logs, readback scripts, temporary models and both watchdog transcripts:
`/private/tmp/ferrite-27d-review/` (also preserved with the task's review
artifacts). No upstream native library rebuild, large local STEP campaign,
foreign worktree operation or cache deletion was performed.
