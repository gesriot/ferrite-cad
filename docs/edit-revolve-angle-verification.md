# §27E — verification record

[Contract](edit-revolve-angle.md).

Each claim below is marked **local** (run in the cloud container), **CI**
(GitHub Actions on the exact PR head) or **N/A** (not run, with the
reason).

## Where and how this was run

* **Container.** A Linux cloud container with the pinned OCCT 8.0.1 already
  installed (not rebuilt) and no PlaneGCS.
* **Build.** One cargo build at a time (`-j2`), reusing the existing native
  and stub targets. Disk was checked between steps.
* **Base.** Branch `edit-revolve-angle` from `origin/main` 6a129a2 (the
  squash of #56).
* **No GUI.** No window, viewer or GPU was launched. The UI test below
  drives the real egui widgets and the real worker headlessly; it is not a
  window run. The window scenario at the end is for the reviewer's Mac.
* **Mixed build.** The CLI and app crates have no default features, so
  every native run here is the mixed configuration: OCCT without PlaneGCS.
  The full OCCT+PlaneGCS matrix is CI's.

## Decisions made before code

Recorded in the contract before implementation:

* **Class.** A standalone partial Revolve: payload v3 with a bore, or v4
  closed on the axis.
  * **The frame.** The same frame as the §27B profile edit (XY datum,
    Sketch, Revolve, Body, exactly three dependencies), refactored into one
    shared `revolve_document`, with each edit's own extent check. The
    profile edit's messages are unchanged.
  * **The profile.** The class is checked by the same `revolve_choices` /
    `stated_revolution` check every reader applies.
* **The angle.** `RevolveAngle::new` is the only policy (0.01°–359.99°
  inclusive, stored exactly). It is not repeated in the UI or the CLI.
* **The same angle.** Accepted, as a height edit to the same height is. The
  copy has a byte-identical Revolve payload; only `meta.modified_at` may
  differ. `content_version` includes `meta`, so it differs too.
* **The writer.** It updates `payload`/`payload_hash` of the one row
  (exactly one row changed) and stamps `modified_at`. It does not rebuild
  capabilities: the object set and payload versions cannot change.
* **The request.** `{"request_version":1,"angle_deg":N}`, and it must be a
  JSON object. An implementation finding: serde's derive also accepts
  `[1, 90]` for a struct. For this new command the request is parsed as a
  `Value` first and refused unless it is an object. Existing commands keep
  their behaviour; this is not widened to them.
* **Discovery.** Additive `revolves[].angle_edit {available, refusal,
  document_refusal, min_deg, max_deg}` from the same pinned reading.
  `angle_deg` is unchanged.
* **Schema.** No payload, capability or SQL schema change.

## Native results (OCCT 8.0.1, no solver) — local

### New tests (8)

| Crate | Test | What it proves |
|---|---|---|
| document | `revolve_angle_edit::tests::catalogue_names_each_refusal_and_prepares_only_the_angle` | Discovery, the named refusals and the prepared payload (see below). |
| document | `revolve_angle_edit::tests::writer_rederives_and_refuses_forged_or_stale_preparations` | The writer's own gate, reached directly (see below). |
| cli | `angle::native_revolve_angle_edits_measure_caps_names_cache_sql_and_exports` | The editing chain, geometry, SQL and cache (see below). |
| cli | `angle::revolve_angle_discovery_and_request_refusals_preserve_every_file` | Discovery and request refusals in any build (see below). |
| cli | `angle::native_revolve_angle_edit_refusals_preserve_every_file` | Refusals from real processes, each writing nothing and leaving the source unchanged (see below). |
| cli | `angle::native_revolve_angle_edit_delivery_late_guards_and_cancellation` | Delivery, late guards and cancellation (see below). |
| cli | `angle::occt_without_solver_edits_a_partial_revolve_angle` | The mixed gate: an edit with a kernel and no solver, with the allowlist, the kernel's measurement and the STL. |
| app | `sketch::tests::native_revolve_angle_edit_widgets_worker_and_cli_publish_one_copy` | The real widgets, the worker and the peer CLI (see below). |

**`catalogue_names_each_refusal_and_prepares_only_the_angle`**
* The catalogue row of a sector has no refusal, the saved angle and its
  Body.
* Eight bad angles are refused by both `validate_angle` and preparation.
* The prepared payload equals the stored one with only the extent replaced.
* A Sketch UUID says "not a Revolve", and an unknown UUID says "does not
  exist".
* A full turn is refused by name, with `degrees: None`.

**`writer_rederives_and_refuses_forged_or_stale_preparations`**
* A forged `axis_segment`, a forged row name and a forged version are each
  refused inside the transaction.
* A source changed after preparation is refused.
* The honest value writes exactly the angle.

**`native_revolve_angle_edits_measure_caps_names_cache_sql_and_exports`**
* **Profiles:** a stepped part with a bore, a solid cylinder and a solid
  cone, at fractional sizes and shifted along Y.
* **Chain:** each is created at 137.5° and edited 137.5 → 220 → 90 → 137.5,
  each copy made from the previous one (through 180° both ways, and
  A → B → A).
* **Each copy:**
  * the reply names the same feature and document, and the file it was made
    from is byte-identical;
  * SQL allowlist on every cell: only the Revolve row's
    `payload`/`payload_hash` and `meta.modified_at` differ, and exactly one
    `objects` row changes;
  * the topology refs are equal to the source's, and so are payload v3/v4
    and the capability rows;
  * `angle_deg` is the new angle, and the Sketch is refused, naming the new
    sector.
* **Kernel measurement:**
  * the B-Rep volume equals this file's own Pappus × θ/360 within 1e-9
    relative;
  * the face count is Lines (minus the axis Line) plus 2;
  * every name resolves to exactly one face. Each Line's face is turned
    from exactly 0° to θ. Each cap, under its unchanged UUID, lies on its
    plane (start z = 0, end at θ), faces outward, stays inside the profile
    and covers the profile's area within 1e-5.
* **Cache:**
  * cold, then a cached rebuild from the archive the chain has carried so
    far: a Miss for a new angle, then a Hit;
  * back at 137.5° the source's own archive answers (a Hit);
  * the 90° copy's archive alone, given to the 137.5° copy, is a Miss and
    then a Hit.
* **Independent STL:**
  * closed through T-junctions only, outward;
  * signed volume within the chord band of Pappus × θ/360;
  * every vertex inside the turned profile and inside [0°, θ];
  * both end faces on their planes, facing out, covering the profile's
    area.
* **A → B → A:** the Revolve payload is byte-identical to the created one,
  and the default STL is byte-identical to the source's.
* **The same angle:** allowlist with zero changed `objects` rows, and a
  byte-identical payload.
* **Range edges:** 0.01°, 359.99° and 100/3° are stored exactly and built.

**`revolve_angle_discovery_and_request_refusals_preserve_every_file`**
* The exact `angle_edit` object, and the full-turn refusal.
* Refused as `input`:
  * malformed JSON, empty input, an array;
  * a missing angle, a missing version;
  * a string, `null`, a list or an object as the angle, and an unknown
    nested field;
  * an unknown top-level field, an `extent`;
  * 1e400, a string version;
  * oversize input.
* v2 is `unsupported`.
* A non-UTF-8 output path is refused in JSON, and usage errors stay clap's
  text.
* **Out-of-range angles:** `input` with the domain's message on native, and
  `unsupported` (the missing kernel) on stub.

**`native_revolve_angle_edit_refusals_preserve_every_file`**
* An unknown UUID (`input`).
* Sketch and Body UUIDs ("not a Revolve").
* A full-turn document ("no angle to edit").
* A stale version.
* Outputs that are occupied (whose bytes are kept), the source itself, a
  symlink or a hard link to it.
* A moved plane, a profile that now touches the axis ("between hollow and
  solid").
* An unresolvable saved name, which the baseline rebuild refuses.
* It also checks a UTF-8 non-ASCII source and destination (Cyrillic and θ)
  that publishes.

**`native_revolve_angle_edit_delivery_late_guards_and_cancellation`**
* Exit 7 with stdout closed, and with both streams closed: the copy
  remains and measures correctly.
* A rebuild failure: the mock kernel cannot turn a profile, so the job
  refuses at the baseline and writes nothing.
* Cancellation at 0.4 writes nothing.
* A publication race: the destination is created at 0.95, and the other
  writer's bytes are kept.
* The source changed at 0.95 is refused as "source has changed".
* A preparation made from the old bytes is refused by the changed
  document's writer directly.
* Cancellation after publication keeps the copy.

**`native_revolve_angle_edit_widgets_worker_and_cli_publish_one_copy`**
* **Opening.** The source is created by the CLI (stepped, fractional,
  shifted, 137.5°). The Sketch cannot be opened for coordinates, with the
  §27D refusal. The sector opens from its own row, found by scrolling the
  real saved-object list and clicking with real pointer events.
* **The form.** The window title and the saved angle and class text are
  shown.
* **Refusals.** 360 is refused in the form, with no Save. Seven other bad
  inputs are refused.
* **Apply, Undo, Redo.** 220 is applied, then Undo returns 137.5 and Redo
  returns 220, through the real buttons. Applying the same number adds no
  checkpoint.
* **Save.** Save gives a request with the accepted version.
* **The worker.**
  * An occupied destination publishes nothing and keeps the draft.
  * A stale generation changes nothing.
  * A refused Open restores the draft and its history. An accepted one
    clears it.
* **The peer CLI** edits the same source with the same request:
  * the semantics, objects and refs are equal;
  * the refs are the source's;
  * the stored angle is 220°;
  * STL and FBX bytes are identical with no mapping;
  * the source is byte-identical.

### Existing tests and the refactor — local

* **The §27B refactor.** `revolve_frame` now calls the shared
  `revolve_document`, and every message is kept (checked by the §27B and
  §27D tests):
  * `edit::native_revolve_profile_edit_refusals_preserve_every_file` and
    the other `edit::` tests pass;
  * `partial::native_saved_partial_revolve_refuses_coordinate_editing_atomically`
    passes.
* **The workspace.** `cargo test --workspace --no-fail-fast` gives 2053
  passed, 2 ignored and 19 failed. §27D recorded 2045 passed; the
  difference is exactly the 8 new tests. The 19 failures are the same N/A
  set as §27D:
  * 18 PlaneGCS suites (the app's `constraints::tests::native_*` worker
    tests, and the CLI's `annular_constraints`, `circle_constraints` and
    `edit_constraints` native tests). This container has no solver, so they
    are CI's.
  * `validate::validation_really_read_only_permissions`, which cannot
    observe a read-only file as root. Re-run as uid 1001 through
    `setpriv`, it passes.

## Stub results (no OCCT) — local

`CARGO_TARGET_DIR=/home/user/stub-target`, with OCCT and PlaneGCS unset:
* the five `angle::` CLI tests pass. The four native ones print
  `skipped: this build has no Open CASCADE`, and the discovery/refusal test
  takes its stub branch: every well-formed request is `unsupported` with
  nothing written, whatever its angle;
* both document tests pass (they need no kernel);
* the app test prints `skipped: no OCCT for the Revolve angle edit worker`.

This is the error priority the contract records. Request shape, version,
size and UTF-8 are judged identically in both builds, before the kernel. The
angle, the class and the version are reached only by a native job.

## The real §27D CLI (6a129a2, built from the merge commit) — local

It was built in a separate worktree and target and run on the copies the
new build edited (stepped, cylinder, cone, 137.5° → 220°):
* `inspect --json` equals the new build's output once `angle_edit` is
  removed. The same holds for an unedited sector and a full turn.
* `validate` reports 0 warnings, `rebuild --cold` passes, and
  `print-topology` resolves 8/8, 5/5 and 4/4.
* Its `export-stl` and `export-fbx` output is byte-identical to the new
  build's.
* No file was written.

So a §27D reader opens, rebuilds and exports an angle-edited copy
unchanged. No payload, capability or SQL schema bump was needed.

## Checks — local

* `cargo fmt --all -- --check` is clean.
* `cargo clippy --workspace --all-targets -- -D warnings` is clean (native,
  no PlaneGCS).
* **Recipes, extracted from Markdown and executed with the pinned ufbx
  reader:**
  * §27E: `FCAD_27E_RECIPE_OK 9 15 fbx` (9 edited copies, 15 refusals);
  * §27A OK 12, §27B OK 6, §27C OK 24, §27D OK 48 fbx.
* **The macOS fixture generator** ran locally against the native CLI: 16
  JSON replies `ok:true`, exit 0.

## Mutations — local

Each was applied to the source, compiled, run and then restored. The
restore was checked with `grep`, and the suite was re-run green.

| # | Mutation | Compiles | Caught by (executed assertions) |
|---|---|---|---|
| M1 | The new angle is ignored: `prepare_revolve_angle` no longer replaces `extent` | yes | 2 document tests (the prepared payload differs from the expected one). 4 CLI tests: the allowlist expects exactly one changed Revolve row and finds none (`angle.rs:122`), and the delivery test's volume check at 220° fails (`partial.rs:187`). |
| M2 | The writer's re-derivation and version guard are bypassed: the check closure in `write_revolve_angle` becomes `\|_\| Ok(())` | yes | `writer_rederives_and_refuses_forged_or_stale_preparations` (the forged payload is written: `expect_err` fails). `native_revolve_angle_edit_delivery_late_guards_and_cancellation` (a preparation from another version is written: `expect_err` fails). |

Neither is an equivalent mutant: each changes published bytes or accepts a
write the contract forbids.

## FBX — local and CI

* **Local.** The three 220° copies (stepped, cylinder, cone) were exported
  at the default tessellation as STL and FBX by the angle test
  (`FCAD_REVOLVE_ANGLE_ARTIFACTS`). Then:
  * `test_stl_matches_fbx.py`: OK;
  * the pinned ufbx `--identity`: `checks=6 failures=0` for each;
  * `--triangles` joined with the STL by `stl-matches-fbx.py`:
    `FCAD_STL_FBX_MATCH` with 632, 244 and 658 triangles, worst 1.73e-18 m.
  * The new `check-fbx-complex.sh` block, extracted and run with only
    `reader`, `work`, `root` and `native` set, prints
    `FCAD_REVOLVE_ANGLE_UFBX_EXECUTED`. The STEP corpus was not duplicated
    locally.
* **CI.** The runtime-layout job sets `FCAD_REVOLVE_ANGLE_FBX_DIR` and
  requires `FCAD_REVOLVE_ANGLE_UFBX_EXECUTED` in the log.

## CI gates added (runtime layout, exact name, no skip)

* **Mixed, no solver:** `angle::occt_without_solver_edits_a_partial_revolve_angle`.
* **Revolve CLI (`--features planegcs`):** the four other `angle::` tests.
* **Domain:** the two `revolve_angle_edit::tests::` tests.
* **UI:** `sketch::tests::native_revolve_angle_edit_widgets_worker_and_cli_publish_one_copy`.
* **FBX:** the `revolve-angle-0..2` ufbx loop with the STL join.

Every gate name of the base workflow is still present (the set difference
against `origin/main` is empty), and exactly 8 names were added.
### CI on 56ef028 (the code head) — read from the jobs and their logs

* **All green on 56ef028:**
  * the CI workflow: lint, supply-chain, sbom, notices, and test on
    ubuntu, macos and windows;
  * planegcs pin (linux, macos, windows and the three-platform compare);
  * combined runtime layout (run 36229314207): linux, macos and windows,
    every step success.
* **How much of each log could be read.** The log tool returns the last
  5000 lines of each runtime job (Linux 8296 lines in total). Direct log
  download is refused by this container's proxy. So the early
  no-solver step is outside what could be read, and its result is taken
  from the job itself.
* **The mixed gate.** Step 18 ("Build circles with OCCT and refuse
  constraints without the solver") finished success on all three
  platforms. That step fails unless the exact `test <gate> ... ok` line is
  present and no `skipped:` appears, so
  `angle::occt_without_solver_edits_a_partial_revolve_angle` ran and passed
  there.
* **In the readable part of each of the three logs:**
  * the 7 other new names each have exactly one `test <name> ... ok`
    line;
  * `FCAD_REVOLVE_ANGLE_UFBX_EXECUTED` is present;
  * `FCAD_STL_FBX_MATCH` reports `triangles=632`, `244` and `658`, worst
    `1.73e-18` m, for the three angle-edited copies. The §27D sector joins
    beside them are unchanged (168, 984 or 980, and 498 triangles).
* **Totals.** A total per OS of distinct names and executions, as the
  previous slices recorded it (240 and 279), is not reproduced here,
  because the first part of each log is not readable. What is established
  is that every gate name of the base workflow is still in the workflow and
  8 were added. Each gate step refuses a missing or skipped name, and every
  gate step was green on all three platforms.
* **Base.** On `main` 6a129a2, every workflow finished success, including
  combined runtime layout (run 36226853287).

## Limits

* **The cone.** A cone sector's mesh has T-junctions along its first
  ruling, as recorded in §27D. It is checked as closed through T-junctions,
  never claimed as a strict 2-manifold.
* **Not in this slice:**
  * sector coordinates;
  * full↔partial;
  * axis or direction changes;
  * constraints on a Revolve profile;
  * booleans or several bodies;
  * live preview;
  * in-place Save.
* **An existing message.** The §27D message for an angle within 0.01° of a
  full turn prints the f64 margin `360 − 359.99` as `0.009999999999990905°`.
  The domain wording predates this slice and is unchanged here; the limit
  itself is exactly `RevolveAngle::MAX_DEGREES` = 359.99.
* **The stub build** cannot judge an angle: every well-formed request is
  refused as the missing kernel.
* **No window run here.** The macOS scenario below is for the reviewer's
  fresh bundle.
* **Local test gaps:**
  * the root-only permission test is N/A as root (it is run as an
    unprivileged UID when needed);
  * the PlaneGCS suites are N/A in this container. CI runs them.

## macOS fixtures and window scenario (for the reviewer's Mac)

### Fixture generator

`FERRITECAD` is the bundled CLI
(`FerriteCAD.app/Contents/MacOS/ferritecad`). The marked block writes:
* three 137.5° sources (stepped with a bore, solid cylinder and solid cone,
  at fractional sizes and shifted along Y);
* a full turn;
* the peer CLI's 220° copy of each source, with its STL and FBX;
* each source's STL and a byte copy of each source.

It was executed locally against the native CLI (see Checks).

```sh
# FCAD_27E_MAC_FIXTURES
set -euo pipefail
: "${FERRITECAD:?set FERRITECAD to the bundled CLI}"
dir="${1:-$PWD/ferrite-27e-fixtures}"
mkdir -p "$dir" && cd "$dir"
make() {
  printf '{"request_version":2,"points_mm":%s,"axis":"sketch_y","extent":%s}' "$2" "$3" > "$1.json"
  "$FERRITECAD" create-sketch-revolve "$1.json" -o "$1.fcad" --json
}
sector='{"kind":"angle","degrees":137.5}'
make stepped-source '[[4.25,1.5],[10.75,1.5],[10.75,6.5],[7.5,6.5],[7.5,16.25],[4.25,16.25]]' "$sector"
make cylinder-source '[[0,-2.5],[9.75,-2.5],[9.75,12.25],[0,12.25]]' "$sector"
make cone-source '[[0,0.5],[8.5,0.5],[0,13.75]]' "$sector"
make full-turn '[[4,0],[10,0],[10,15],[4,15]]' '{"kind":"full_turn"}'
printf '{"request_version":1,"angle_deg":220}' > angle-220.json
for f in stepped cylinder cone; do
  "$FERRITECAD" export-stl "$f-source.fcad" -o "$f-source.stl" --json
  set -- $("$FERRITECAD" inspect "$f-source.fcad" --json | python3 -c '
import json, sys
c = json.load(sys.stdin)["result"]
print(c["revolves"][0]["feature_id"], c["content_version"])')
  # The peer CLI edit of the same source, to be compared with the window's.
  "$FERRITECAD" edit-revolve-angle "$f-source.fcad" --feature "$1" --expect-version "$2" \
    --request angle-220.json -o "$f-cli.fcad" --json
  "$FERRITECAD" export-stl "$f-cli.fcad" -o "$f-cli.stl" --json
  "$FERRITECAD" export-fbx "$f-cli.fcad" -o "$f-cli.fbx" --json
  cp "$f-source.fcad" "$f-source.bytes"
done
```

Extract and run:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-revolve-angle-verification.md").read_text()
code = text.split("# FCAD_27E_MAC_FIXTURES\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27e-fixtures.sh").write_text(code)
EXTRACT
FERRITECAD=/path/to/FerriteCAD.app/Contents/MacOS/ferritecad bash ferrite-27e-fixtures.sh "$PWD/ferrite-27e-fixtures"
```

### Window scenario

Run exactly one viewer under the watchdog, on the fixture folder outside the
checkout. Give the watchdog a `--log` name that does not exist yet, and do
not redirect shell output to it.

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$dir/watch-27e.jsonl" --limit-mib 1536 --seconds 1800 \
  -- /path/to/FerriteCAD.app/Contents/MacOS/ferritecad-viewer
```

1. **Open the sector.** Open `stepped-source.fcad`. Among the saved-object
   actions:
   * `Edit Sketch Profile — <UUID>…` is disabled. Its hover text is
     "coordinate editing of a partial Revolve (137.5° sector) is not
     supported in this build; only a full-turn Revolve profile can be
     edited".
   * `Edit Revolve angle Revolve1 — <UUID>…` is enabled.
2. **The form.** Click it. A window titled "Edit Revolve angle — new copy"
   shows:
   * "Saved partial Revolve · angle only · right-handed about the sketch +Y
     axis";
   * the Revolve UUID and the path;
   * "Saved angle 137.5° · with a bore · profile, axis, direction and every
     UUID are retained";
   * an **Angle** field holding 137.5, with "° (0.01–359.99, a full turn is
     not an angle)".

   The main window's title and scene are unchanged.
3. **Refusals.** Type each of these in turn. Each shows a red refusal and
   no `Save edited Revolve copy…`:
   * 360: "state a full turn";
   * 0 and −45: "not positive";
   * 359.995: "a partial revolve turns at most 359.99°" (the message
     prints the f64 margin as `0.009999999999990905°`; see Limits);
   * 0.001: "below the 0.01° minimum";
   * `abc`.
4. **Apply, Undo, Redo.** Type **220** and click **Apply angle change**.
   **Undo draft** shows 137.5 and **Redo draft** shows 220.
5. **Cancel, then publish.**
   1. `Save edited Revolve copy…` → **Cancel**: nothing is written, and the
      draft and its Undo stay.
   2. Save again as `stepped-gui.fcad` (Go to Folder, basename only). While
      it saves, the title and scene stay those of `stepped-source.fcad`.
   3. The copy then opens: a 220° sector, now wider than a half turn, with
      two flat end faces, the first still in the sketch plane.
6. **Two more.** In the same PID, repeat steps 1, 2, 4 and 5 for
   `cylinder-source.fcad` → `cylinder-gui.fcad` and `cone-source.fcad` →
   `cone-gui.fcad`. The step 2 text says "solid, closed on the axis along
   Line <UUID>".
7. **Export through the GUI.** STL for all three as `<name>-gui.stl`, and
   FBX as `<name>-gui.fbx`.
8. **A full turn has no angle.** Open `full-turn.fcad`. `Edit Revolve angle
   Revolve1 — <UUID>…` is disabled, with the hover text "a full-turn
   Revolve has no angle to edit; changing between a full turn and a partial
   angle is not supported". Quit.
9. **Compare with the CLI, with no mapping.** An edited copy keeps every
   identity of its source, so the window's copy and the CLI's are the same
   model:
   * `cmp stepped-gui.stl stepped-cli.stl` and `cmp stepped-gui.fbx
     stepped-cli.fbx` must both be identical, and the same for the cylinder
     and the cone;
   * `cmp stepped-source.fcad stepped-source.bytes`, and the same for the
     other two: the sources are unchanged;
   * `"$FERRITECAD" inspect stepped-gui.fcad --json` reports `angle_deg`
     220 and `angle_edit.available` true;
   * `print-topology` resolves 8 of 8 for the stepped part, 5 of 5 for the
     cylinder and 4 of 4 for the cone.

Expected: exit 0, memory of the same order as §27A–D (about 200 MiB peak),
swap 0.
