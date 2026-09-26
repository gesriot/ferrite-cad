# §27F — verification record

[Contract](edit-partial-revolve-profile.md).

Each claim below is marked **local** (run in the cloud container), **CI**
(GitHub Actions on the exact PR head) or **N/A** (not run, with the
reason).

## Where and how this was run

* **Container.** A Linux cloud container with the pinned OCCT 8.0.1 already
  installed (not rebuilt) and no PlaneGCS.
* **Build.** One cargo build at a time (`-j2`), reusing the existing native
  and stub targets. Disk was checked between steps.
* **Base.** Branch `edit-partial-revolve-profile` from `origin/main` 9e1c5d5
  (the squash of #57).
* **No window.** No window, viewer or GPU was launched. The UI test drives
  the real egui widgets, a real drag and the real worker headlessly; it is
  not a window run. The window scenario at the end is for the reviewer's
  Mac.
* **Mixed build.** The CLI and app crates have no default features, so every
  native run here is OCCT without PlaneGCS. The full OCCT+PlaneGCS matrix is
  CI's.

## What changed

* **Domain** (`sketch_edit.rs`).
  * `SketchProfileUse::PartialRevolve { feature, body, axis_segment,
    degrees }` is new.
  * `revolve_frame`'s intent check now accepts `Partial` as well as
    `FullTurn`, still requiring the sketch Y axis and NewBody, inside the
    shared `revolve_document` frame.
  * The profile policy is `stated_revolution`, for both extents alike.
* **Writer and spine.** Unchanged: `replace_sketch_coordinates` →
  `write_sketch_geometry` → `edit_object_copy`.
* **JSON.** Two additive `profile_feature` kinds,
  `partial_turn_revolve` and `partial_turn_revolve_axis_closed`, each with
  `angle_deg`.
* **CLI.** The `edit-sketch-copy` request is strict about shape:
  * one decode of the original bytes, so duplicate keys, escaped ones
    included, are refused by the derive;
  * an extra shape check that the request and each vertex are JSON
    objects.
* **UI.**
  * A sector's draft is `Feature::RevolveAngle`, with the saved angle shown
    in a read-only field.
  * The texts name the saved θ° sector and point the angle to Edit Revolve
    angle.
  * Everything else is the existing editor and worker.
* **No schema change.** No payload, capability or SQL schema changed.

## Native results (OCCT 8.0.1, no solver) — local

### New and replaced tests

| Test | What it proves |
|---|---|
| `profile::native_partial_profile_edits_measure_caps_names_cache_sql_and_exports` | The edit's geometry, SQL, cache and exports (see below). |
| `profile::native_partial_profile_edit_delivery_late_guards_and_cancellation` | Every guard around publication (see below). |
| `profile::partial_profile_requests_refuse_duplicates_arrays_and_foreign_fields` | Discovery and request refusals in any build (see below). |
| `profile::occt_without_solver_edits_a_partial_revolve_profile` | The mixed gate: the SQL allowlist, the Revolve bytes, the kernel's measurement and the STL. |
| `revolve_angle_edit::tests::sector_profile_writer_rederives_and_refuses_forged_or_stale_sketches` | Without a kernel: the writer's own gate (see below). |
| `sketch::drag_tests::native_partial_revolve_profile_widgets_drag_worker_and_cli_edit_one_copy` | The real widgets, the worker and the peer CLI (see below). |
| `partial::native_saved_partial_revolve_refuses_coordinate_edits_outside_the_class` (replaces `…refuses_coordinate_editing_atomically`) | The class contract that replaces the §27D gate (see below). |

**`native_partial_profile_edits_measure_caps_names_cache_sql_and_exports`**
* **Profiles:** stepped with a bore, a solid cylinder and a solid cone; the
  sizes are fractional and shifted along Y. Each is saved at 137.5° and at
  220°, and edited A → B with radial and axial changes.
* **SQL:** only the Sketch row's `payload`/`payload_hash` differ. `meta` is
  untouched.
* **Kept:** the Revolve payload (byte-identical), the refs, payload v3/v4
  and the capabilities.
* **Discovery:** the vertices are B, and the stated angle is the source's.
* **The kernel's measurement of each copy:**
  * volume equals this file's Pappus(B) × θ/360 within 1e-9;
  * each face under its saved UUID: each Line's face is turned from exactly
    0° to θ, and each cap lies on its plane, faces out and covers the
    profile's area.
* **Cache:**
  * cold;
  * then the old profile's archive, copied under the copy's name, which
    must be a Miss;
  * then its own archive, a Hit.
* **Independent STL:** closed through T-junctions, outward, within the
  chord band, and inside [0°, θ].
* **B → A:** every SQL cell equals the source's, and the default STL is
  byte-identical.
* **Chains:** profile → angle and angle → profile on one source give the
  same cells (except `modified_at`), the same Sketch payload, the source's
  refs, the kernel volume, and byte-identical STL and FBX.

**`native_partial_profile_edit_delivery_late_guards_and_cancellation`**
* Occupied, source-itself and hard-link destinations: nothing written.
* Exit 7 with stdout closed, and with both streams closed: the copy stands
  and measures correctly.
* The mock kernel's rebuild failure: nothing written.
* Cancellation at 0.4: no output and no scratch.
* A publication race: the other writer's bytes are kept.
* The writer gate, reached directly:
  * a forged axis end off the axis (refused: "touches the axis alone");
  * a forged construction flag ("coordinate write");
  * a stale Sketch ("changed after").
* A late change of the source at 0.95: "source has changed".
* A stale version refused by process.

**`partial_profile_requests_refuse_duplicates_arrays_and_foreign_fields`**
* **Discovery without a kernel:** the new kind, `angle_deg` and
  `feature_id`.
* **Refused as `input` before any kernel, in any build:**
  * duplicate `request_version` (2 then 1), duplicate `vertices`,
    duplicate `curve_id`;
  * an escaped duplicate `start_mm`, an escaped duplicate
    `request_version`;
  * the request as an array, a vertex as an array;
  * `angle_deg` in the request, `extent` in the request, an unknown vertex
    field;
  * a string coordinate, 1e400;
  * no vertices, malformed JSON.
* **v2:** `unsupported`.
* **Native only:** a foreign UUID, crossing the axis, and hollow→solid are
  refused by name. On stub every well-formed request is `unsupported`.

**`sector_profile_writer_rederives_and_refuses_forged_or_stale_sketches`**
* It runs without a kernel.
* Discovery reports `PartialRevolve` with 90°.
* A forged hollow→solid payload is refused.
* A stale row is refused.
* An honest write changes the Sketch and not a byte of the Revolve row.

**`native_partial_revolve_profile_widgets_drag_worker_and_cli_edit_one_copy`**
* **Opening.** A 220° solid cylinder sector opens from its own `Edit
  Sketch` row, through real scrolling and pointer events.
* **The texts:** the header, "saved 220° sector", the read-only Angle
  "220", the axis-Line text, and the pointer to Edit Revolve angle.
* **Editing.**
  * One snapped drag is one Undo step. Undo and Redo go through the
    buttons.
  * Exact fields change the vertices; the angle stays "220".
  * An axis end dragged off the axis is refused in the preview, with no
    Save, and Escape cancels it.
* **The worker.** An occupied destination publishes nothing and keeps the
  draft. A refused Open restores the draft, and an accepted one clears it.
* **The peer CLI** with the same request:
  * **every** SQL cell is equal (`sql_facts`), and so are the semantics;
  * the refs are the source's, and the Revolve row equals the source's
    (220°);
  * the kernel volume is a frustum × 220/360 with 5 faces;
  * STL and FBX bytes are equal with no mapping;
  * the source is byte-identical.

**`native_saved_partial_revolve_refuses_coordinate_edits_outside_the_class`**
It replaces §27D's `native_saved_partial_revolve_refuses_coordinate_editing_atomically`
(the blanket "a sector refuses every coordinate edit"). Each of these
refusals writes nothing and leaves the source unchanged:
* a bored sector: hollow→solid, across the axis, winding;
* a solid sector: solid→hollow, and the axis moved to another Line.

### The old refusal assertions, replaced deliberately

The old contract was asserted in 5 places, and each is changed to the new
one:
* the discovery checks in `partial::native_partial_revolutions_measure_caps_names_cache_and_exports`
  and `partial::partial_revolution_payload_capabilities_and_discovery_without_kernel`;
* the Sketch check in `angle::native_revolve_angle_edits_measure_caps_names_cache_sql_and_exports`;
* the app test `native_revolve_angle_edit_widgets_worker_and_cli_publish_one_copy`,
  which asserted `begin_edit` false;
* the §27D gate itself.

In each, `editable: true` with the new kind, the vertices and the stated
angle now replace `editable: false` with the refusal. The §27D and §27E
recipes are updated the same way. The §27D recipe now checks that
hollow→solid is refused.

### Workspace — local

`cargo test --workspace --no-fail-fast` gives 2058 passed, 2 ignored and 19
failed:
* **2058 passed:** §27E's 2053 plus the 5 new tests compiled into that run.
  The domain test was added after the run had started; it passes on its own
  (see Stub and Checks).
* **The 19 failures** are the same N/A set as §27D/§27E:
  * 18 PlaneGCS suites (no solver in this container);
  * `validate::validation_really_read_only_permissions`, which cannot
    observe a read-only file as root. Re-run as uid 1001 with `setpriv`, it
    passes.

## Stub results (no OCCT) — local

Run with `CARGO_TARGET_DIR=/home/user/stub-target`, with OCCT and PlaneGCS
unset:
* The `revolve` test binary: 32 passed. The 23 native tests print `skipped:
  this build has no Open CASCADE`. `profile::partial_profile_requests_…`
  takes its stub branch: every well-formed request is `unsupported`, with
  nothing written.
* The domain test passes.
* The app test prints `skipped: no OCCT for the partial Revolve profile
  worker`.

The error priority is unchanged from §27B. Request shape, duplicates, size,
version and UTF-8 are judged identically in both builds, before the kernel.
UUIDs, the class and the profile are reached only by a native job.

## The real §27E CLI (9e1c5d5) on §27F copies — local

It was built from the merge commit in a separate worktree and target, and
run on the fixture generator's `*-cli.fcad` copies (stepped 137.5°,
cylinder 220°, cone 220°):
* `validate` reports 0 warnings, `rebuild --cold` passes, and
  `print-topology` resolves 8/8, 5/5 and 4/4.
* Its STL and FBX bytes equal the new build's.
* No file was written.
* Its `inspect` is equal to the new build's except for `sketches`: the old
  reader reports the sector's Sketch as not editable. The new one reports it
  as editable, with the new kind.

## Checks — local

* `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets
  -- -D warnings` are clean (native, no PlaneGCS).
* **Affected suites after the mutations were restored:**
  * `revolve` 32/32 and `edit_sketch` 5/5;
  * `ferritecad-document` all passed (1 ignored, as before);
  * the app's `revolve` 8/8 and `drag_tests` 19/19.
* **The mixed build** (`--no-default-features`) of `profile::` passed 4/4.
* **Recipes, extracted from Markdown and executed with pinned ufbx:**
  * §27F `FCAD_27F_RECIPE_OK 12 8 fbx`;
  * the updated §27D OK 48 fbx and §27E OK 9 15 fbx;
  * §27A OK 12, §27B OK 6, §27C OK 24.
* **The macOS fixture generator** below ran locally: 15 replies `ok:true`,
  exit 0.

## Mutations — local

Each was applied, compiled, run, and restored byte for byte (checked with
`cmp` against a saved copy). The affected suites were then re-run green.

| # | Mutation | Compiles | Caught by (executed assertions) |
|---|---|---|---|
| M1 | Stale profile archive: `RevolveRequest::feed` no longer feeds the profile into the cache key | yes | `profile::native_partial_profile_edits_…`: the old profile's archive answers a Hit where the edited profile must be a Miss (`partial.rs:166`, `left: [Hit] right: [Miss]`). |
| M2 | Writer guard bypassed: the check closure of `write_sketch_geometry` becomes `\|_\| Ok(())` | yes | `sector_profile_writer_…`: the forged hollow→solid payload is written (`expect_err("forged class")` fails). `native_partial_profile_edit_delivery_…`: the forged axis end is written (`expect_err("forged")` fails). |

Neither mutant is equivalent: M1 serves the wrong geometry from the cache,
and M2 publishes a class change the contract forbids.

## FBX — local and CI

* **Local.** `FCAD_REVOLVE_PROFILE_ARTIFACTS` exports the three 220° edited
  copies (stepped, cylinder, cone) as default STL and FBX. The new
  `check-fbx-complex.sh` block was extracted and run with only `reader`,
  `work`, `root` and `native` set:
  * ufbx `--identity` reports `checks=6 failures=0` ×3;
  * the `--triangles` join gives `FCAD_STL_FBX_MATCH` with 640, 220 and 686
    triangles, worst 1.73e-18 m;
  * it prints `FCAD_REVOLVE_PROFILE_UFBX_EXECUTED`.

  The STEP corpus was not duplicated locally.
* **CI.** The runtime-layout job sets `FCAD_REVOLVE_PROFILE_FBX_DIR` and
  requires `FCAD_REVOLVE_PROFILE_UFBX_EXECUTED`.

## CI gates (runtime layout, exact name, no skip)

* **Replaced (1):**
  `partial::native_saved_partial_revolve_refuses_coordinate_editing_atomically`
  → `partial::native_saved_partial_revolve_refuses_coordinate_edits_outside_the_class`.
  The prohibitions outside the slice stay: class changes, the axis Line,
  the frame (§27B `edit::` tests unchanged).
* **Added (6):**
  * mixed: `profile::occt_without_solver_edits_a_partial_revolve_profile`;
  * CLI: three `profile::` tests;
  * domain: `revolve_angle_edit::tests::sector_profile_writer_…`;
  * UI: `sketch::drag_tests::native_partial_revolve_profile_…`.
* **Totals.** No other base gate name was removed (the set difference
  against `origin/main` is exactly the one replacement). The base's 248
  distinct names and 287 executions per OS become 254 and 293.
* **ufbx reads per OS.** 86 become 92: 3 printed identity reads plus 3
  redirected triangle reads, that is 83 printed and 9 redirected.

### CI on e6ce4d3 (the code head), read from the jobs and their logs

* **All green on e6ce4d3:**
  * the CI workflow: lint, supply-chain, sbom, notices, and test on
    ubuntu, macos and windows;
  * planegcs pin (three platforms, and their compare);
  * combined runtime layout (run 36244076477): linux, macos and windows,
    every step success, including step 18 (no solver) and step 44 (FBX and
    ufbx).
* **How much of each log could be read.** The log tool returns only the
  last 5000 lines of each runtime job, and direct download is refused by
  this container's proxy. So the early no-solver step (18) is outside what
  could be read.
* **The mixed gate.** Step 18 fails unless the exact `test <gate> ... ok`
  line is present and no `skipped:` appears. It finished success on all
  three platforms, so
  `profile::occt_without_solver_edits_a_partial_revolve_profile` ran and
  passed there.
* **In the readable part of each of the three logs:**
  * the replacement gate and the five other new names each have exactly one
    `test <name> ... ok` line;
  * the replaced name
    `partial::native_saved_partial_revolve_refuses_coordinate_editing_atomically`
    appears nowhere;
  * `FCAD_REVOLVE_PROFILE_UFBX_EXECUTED` is present;
  * `FCAD_STL_FBX_MATCH` reports `triangles=640`, `220` and `686` for the
    three profile-edited copies. The earlier sector and angle joins beside
    them are unchanged: 168, 984 or 980, 498, 632, 244 and 658.
* **Totals.** Per-OS totals of distinct names and executions, and of ufbx
  reads, cannot be recounted from the logs, because the first part of each
  is not readable. From the workflow: 254 names and 293 executions (was
  248 and 287), and 92 ufbx reads (was 86). Every gate step was green on
  all three platforms.
* **Base.** On `main` 9e1c5d5 (merge CI, a separate run from this PR's),
  every workflow finished success, including combined runtime layout (run
  36241981002).

## Limits

* **The cone.** A cone sector's mesh has T-junctions (§27D). It is checked
  as closed through T-junctions, never as a strict 2-manifold.
* **Out of scope:**
  * full↔partial;
  * other axes and operations;
  * constraints on a Revolve profile;
  * Circle/Arc;
  * booleans and several bodies;
  * live preview;
  * in-place Save.
* **The stub build** cannot judge UUIDs, class or coordinates: every
  well-formed request is refused as the missing kernel.
* **No window run here.** OOM is not declared fixed.

## macOS fixtures and window scenario (for the reviewer's Mac)

### Fixture generator

`FERRITECAD` is the bundled CLI
(`FerriteCAD.app/Contents/MacOS/ferritecad`). The marked block writes:
* three sources: a stepped part with a bore at 137.5°, and a solid cylinder
  and a solid cone at 220°, at fractional sizes and shifted along Y;
* each source's STL and a byte copy of it;
* the peer CLI's copy of each, edited to exactly the coordinates the
  scenario types, with its STL and FBX.

It was executed locally (see Checks).

```sh
# FCAD_27F_MAC_FIXTURES
set -euo pipefail
: "${FERRITECAD:?set FERRITECAD to the bundled CLI}"
dir="${1:-$PWD/ferrite-27f-fixtures}"
mkdir -p "$dir" && cd "$dir"
make() {
  printf '{"request_version":2,"points_mm":%s,"axis":"sketch_y","extent":{"kind":"angle","degrees":%s}}' \
    "$2" "$3" > "$1.json"
  "$FERRITECAD" create-sketch-revolve "$1.json" -o "$1.fcad" --json
}
make stepped-source '[[4.25,1.5],[10.75,1.5],[10.75,6.5],[7.5,6.5],[7.5,16.25],[4.25,16.25]]' 137.5
make cylinder-source '[[0,-2.5],[9.75,-2.5],[9.75,12.25],[0,12.25]]' 220
make cone-source '[[0,0.5],[8.5,0.5],[0,13.75]]' 220
# The peer CLI edit of each source, to the coordinates the window scenario types.
peer() {
  set -- "$1" "$2" $("$FERRITECAD" inspect "$1-source.fcad" --json | python3 -c '
import json, sys
c = json.load(sys.stdin)["result"]
s = c["sketches"][0]
print(s["sketch_id"], c["content_version"], *[v["curve_id"] for v in s["vertices"]])')
  name=$1 points=$2 sketch=$3 version=$4
  shift 4
  python3 - "$points" "$@" > "$name-edit.json" <<'PY'
import json, sys
points, ids = json.loads(sys.argv[1]), sys.argv[2:]
print(json.dumps({"request_version": 1,
                  "vertices": [{"curve_id": i, "start_mm": p} for i, p in zip(ids, points)]}))
PY
  "$FERRITECAD" edit-sketch-copy "$name-source.fcad" --sketch "$sketch" --expect-version "$version" \
    --request "$name-edit.json" -o "$name-cli.fcad" --json
  "$FERRITECAD" export-stl "$name-cli.fcad" -o "$name-cli.stl" --json
  "$FERRITECAD" export-fbx "$name-cli.fcad" -o "$name-cli.fbx" --json
  "$FERRITECAD" export-stl "$name-source.fcad" -o "$name-source.stl" --json
  cp "$name-source.fcad" "$name-source.bytes"
}
peer stepped '[[3.75,0.5],[11.25,0.5],[11.25,7.25],[8.125,7.25],[8.125,18.5],[3.75,18.5]]'
peer cylinder '[[0,-3.75],[7.625,-3.75],[7.625,14.5],[0,14.5]]'
peer cone '[[0,-1.25],[11.5,-1.25],[0,9.875]]'
```

Extract and run:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-partial-revolve-profile-verification.md").read_text()
code = text.split("# FCAD_27F_MAC_FIXTURES\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27f-fixtures.sh").write_text(code)
EXTRACT
FERRITECAD=/path/to/FerriteCAD.app/Contents/MacOS/ferritecad bash ferrite-27f-fixtures.sh "$PWD/ferrite-27f-fixtures"
```

### Window scenario

Run exactly one viewer under the watchdog, on the fixture folder outside the
checkout. Give the watchdog a `--log` name that does not exist yet, and do
not redirect shell output to it.

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$dir/watch-27f.jsonl" --limit-mib 1536 --seconds 1800 \
  -- /path/to/FerriteCAD.app/Contents/MacOS/ferritecad-viewer
```

1. **Open the sector.** Open `stepped-source.fcad`. Among the saved-object
   actions, `Edit Sketch Profile — <UUID>…` is **enabled**. Click it. Check:
   * the header reads "XY · mm · Line polygon · Revolve through an angle
     about the sketch Y axis · NewBody";
   * the text says "…the saved 137.5° sector about Y are retained";
   * the **Angle °** field shows 137.5 and cannot be edited;
   * there is no Feature switch;
   * "Part with a bore: every point stays at X > 0."
2. **Drag and Undo.** Choose Snap `1 mm`. Drag one vertex by a few
   millimetres. It is one Undo step: **Undo draft** puts it back, **Redo
   draft** moves it again. Then **Undo draft** once more, so the draft is
   the saved profile.
3. **Refusal.** Type `0` into the first vertex's X. A red refusal reads
   "vertex 1 touches the axis alone…", and `Save edited copy…` is hidden.
   Undo it.
4. **Exact coordinates.** Type the vertices (X, Y) in order: (3.75, 0.5),
   (11.25, 0.5), (11.25, 7.25), (8.125, 7.25), (8.125, 18.5), (3.75, 18.5).
5. **Cancel, then publish.**
   1. `Save edited copy…` → **Cancel**: nothing is written, and the draft
      and its Undo stay.
   2. Save again as `stepped-gui.fcad` (Go to Folder, basename only). While
      it saves, the title and the scene stay those of the source.
   3. The copy then opens: a wider, taller stepped sector, still 137.5°,
      with two flat end faces.
6. **Two more.** In the same PID:
   * `cylinder-source.fcad` → (0, −3.75), (7.625, −3.75), (7.625, 14.5),
     (0, 14.5), saved as `cylinder-gui.fcad`. The text names the solid part
     and edge 4 on the axis, and the Angle shows 220.
   * `cone-source.fcad` → (0, −1.25), (11.5, −1.25), (0, 9.875), saved as
     `cone-gui.fcad`. Dragging the axis end (vertex 1) off X = 0 is refused
     in the preview.
7. **Export through the GUI.** STL `<name>-gui.stl` and FBX
   `<name>-gui.fbx` for all three. Quit.
8. **Compare, with no mapping.** An edited copy keeps every identity, so:
   * `cmp stepped-gui.stl stepped-cli.stl` and `cmp stepped-gui.fbx
     stepped-cli.fbx` must both be identical, and the same for the cylinder
     and the cone;
   * `cmp <name>-source.fcad <name>-source.bytes`: the sources are
     unchanged;
   * `"$FERRITECAD" inspect stepped-gui.fcad --json` reports `angle_deg`
     137.5, and the Sketch row has kind `partial_turn_revolve`;
   * `print-topology` resolves 8/8 for the stepped part, 5/5 for the
     cylinder and 4/4 for the cone.

Expected: exit 0, memory of the same order as §27A–E (about 220 MiB peak),
swap 0.

## Independent review correction (2026-09-26)

The two process inputs labelled escaped duplicate keys actually used literal
keys. They now contain JSON Unicode escapes for the first character of
`start_mm` and `request_version`; strict decoding still refuses both. The
exact process gate passed on native macOS after the correction. Production
behavior is unchanged. The shared frame comment now describes both supported
turn extents. Final review-head CI is separate from the incoming-head results
above; its evidence is recorded in the PR review.
