# §28I — verification record

[Contract and recipe](edit-two-fillet-base-height.md).

## Where and how this was run

* **Base.** `origin/main` at `38064fb409551eef21dc75be3bbfb99e2377bbc1`
  (§28H, PR #72 squash-merged). Its tree `49ea848` is the tree of PR #72's
  final head `971ccc9`. The branch `edit-two-fillet-base-height` was created
  from it with a clean tree. The base's merge-triggered runs are recorded
  under [CI](#ci), separately from this change's.
* **Cloud container.** Linux x86_64, the session's existing OCCT 8.0.1 install
  and native/stub targets, one heavy build at a time, `CARGO_BUILD_JOBS` 2–4.
  OCCT, Boost and PlaneGCS were not rebuilt.
* **PlaneGCS: local, unpinned.** Every local solver result below is from the
  library an earlier slice (§27H) built from FreeCAD 1.0.1 `planegcs` sources
  with Ubuntu's Eigen 3.4.0 and Boost 1.83 — not the pinned delivery. **The
  named CI gates on Linux, macOS (Apple Silicon) and Windows with the pinned
  delivery are the authoritative solver evidence.** macOS Intel is not
  supported.
* **No window, GPU test, Unity, browser or large STEP corpus was run**, and
  `FCAD_ALLOW_LOADER_FAILURE_PROBES` was never set. The widget tests drive
  real egui widgets headlessly; that is not a window test. The window
  scenario below is for the user's Mac. Memory/OOM is not considered fixed.

## What changed

* **document:** `fillet_radius::fillets_over_plate` — the reading the base
  height edit alone uses: one Fillet is exactly `fillet_over_plate` (§28C);
  two Fillets are read by `saved_sequential_fillet`, the reader §28H's
  radius edit uses, once for each, and returned in history order.
  `PreparedExtrudeHeight` and `ExtrudeChoice` gain `second_fillet`: Fillet 2,
  whose `previous` is Fillet 1; `fillet` stays the Fillet on the base.
  `rederive` compares the whole preparation, both Fillets included. The
  writer is unchanged: the history branch updates the base row's
  payload/hash and stamps `modified_at`. `fillet_over_plate` — read by the
  Sketch, constraint and add-Fillet editors — is unchanged and still admits
  exactly one Fillet; its two-Fillet refusal and `refuse_filleted` now say
  that the radii and the height can be edited.
* **cli:** `features[].fillet_base.second_fillet` (additive; `null` for one
  Fillet and on `sketches[]`).
* **app:** the Edit extrusion row shows "History: Extrude … -> Fillet 1 … ->
  Fillet 2 …" with both corners and radii (ASCII arrows); the Fillet form's
  §28G text says the radii and the height can be edited.
* No command, request format, payload version, capability, SQLite schema,
  archive format, copier, solver or geometric route was added.

## New tests

Document `fillet_radius::tests` (kernel-free):

* `the_base_height_under_two_fillets_changes_alone_and_keeps_both` — adjacent
  (12.25 mm shared side) and opposite pairs; heights 11.5, 2.25 and 2.25
  again (equal): only the base row changes (none for the equal one), both
  Fillet rows, radii, `previous`, every name and dependency stay; the
  preparation carries Fillet 1 on the base and Fillet 2 on Fillet 1's
  result; either Fillet, the Sketch and a foreign UUID are refused, and 0,
  −1, NaN, ∞ are `input`; the Sketch rows' refusals name the height.
* `the_height_writer_refuses_a_forged_two_fillet_preparation` — Fillet 2
  dropped, its radius altered, it placed on the base, the two swapped, both
  dropped (the legacy writer), Fillet 2's row as the feature, another
  version; and a preparation made stale by a later radius edit of Fillet 2:
  all refused, nothing written.

CLI `tests/fillet.rs`, module `sequential::height`:

* `height_discovery_and_protocol_without_native` — the exact
  `fillet_base` JSON with `second_fillet`, the Sketch still refused naming
  the height, and the protocol per build (kernel: `input` for bad heights,
  `unsupported` for either Fillet or the Sketch, `input` for a foreign UUID;
  no kernel: `unsupported` «Open CASCADE» before any other check), nothing
  written.
* `native_heights_under_both_fillets_are_what_the_numbers_say` — the offset
  fractional plate (−4.5, 3.25, 37.5 × 12.25): CCW adjacent r 2.375 / 3.0625
  raised to 11.5 then lowered to 2.25; CW (from the upper right) adjacent r
  4.8125 / 1.1875 kept at 6.75 then lowered to 3.375; CCW opposite r 2 / 5.5
  raised to 9.125. Each copy: result, allowlist, refs, both Fillet rows,
  validate, cold rebuild of every name; the B-Rep cold, Miss and Hit equal,
  8 faces, the analytic volume at the new height, Fillet 2's cylinder, Fillet
  1's cylinder as carried into the final Body and Fillet 1's own, each by its
  UUID with its radius and axis; the independent STL (bounds including the
  height, no vertex at a rounded corner, arc walls); FBX.
* `native_a_new_height_misses_the_whole_chain_and_radii_stay_editable` — the
  source's warm cache copied to the raised copy (it keeps the document id):
  Extrude, Fillet 1 and Fillet 2 each Miss, the result equals the cold
  build, a rerun is all Hit; then Fillet 1's and Fillet 2's radii are edited
  on the raised copy, each measured at the new height.
* `native_dimensioned_height_keeps_the_stored_sketch_and_the_solved_plate`
  — a plate stored 37.5 × 12.25 and solved 30.5 × 8 at (2.5, −1.75): raised
  to 13.25 and lowered to 4.5; the stored Sketch row is byte-identical, the
  solved starts are the expected ones, both Fillets at the solved corners.
* `native_height_refusals_cancellation_and_report_loss_are_atomic` — a
  height OCCT cannot round (1e-6 mm → `kernel`), the source as output, an
  occupied output, a hard-link alias, a stale version: nothing published and
  no scratch; cancelled at the last barrier through the shared job: nothing;
  a closed report pipe: published and exit 7.

App:

* `edits::tests::two_fillet_base_height_widgets_show_both_fillets_and_keep_the_draft`
  — the history text naming both Fillets, exact input, Save Cancel, the
  widgets' request, a worker refusal keeping the draft.
* `edits::tests::native_two_fillet_base_height_worker_and_cli_publish_the_same_part`
  — the app's worker and the shipped CLI: every SQL cell equal but
  `meta.modified_at`, STL and FBX byte-identical; the accepted scene keeps
  both Fillets at the new height; a failed Open restores the draft.

Updated: §28G/§28H tests that asserted the height editor refused a
two-Fillet history now assert it is offered, with `second_fillet` naming
Fillet 1 as its predecessor.

## Local results

Solver build (OCCT 8.0.1 + the unpinned PlaneGCS), debug:

* `tests/fillet.rs`: 54 passed — 52 executed, 2 printing `skipped: the mixed
  gate needs a build without PlaneGCS` (N/A here, counted apart); the five
  `sequential::height` tests executed. Document `fillet_radius::` 18 passed
  (2 new); app: the two new tests executed.
* Recipes §28A–§28I against the solver CLI: all nine print
  `FCAD_28x_RECIPE_OK`. §28I: `FCAD_28I_RECIPE_OK
  up=5245.444012/5245.745482 down=1026.282524/1026.341507
  f1_after=1020.263403/1020.343554` (measured mesh / exact volume, mm³, at
  11.5 mm, 2.25 mm, and 2.25 mm after Fillet 1 → 4.25 mm).
* FBX: `tools/check-fbx-complex.sh --features planegcs` as CI runs it, over
  the artifacts of the whole `tests/fillet.rs` run: every Fillet marker
  including the new `FCAD_FILLET_TWO_HEIGHT_UFBX_EXECUTED`; the four files
  `height-ccw-adjacent-0`, `height-cw-adjacent-1`, `height-ccw-opposite-0`,
  `height-dimensioned` read by pinned ufbx 0.23.0 (`checks=6 failures=0`
  each) and joined with their STL (120, 120, 136, 120 triangles, worst
  6.94e-18 m); 56 joins in all.
* fmt, workspace clippy (`--all-targets --all-features -D warnings`) and
  `git diff --check`: clean.

Regression of the affected crates, solver build, debug (`--no-fail-fast`):
`ferritecad-document`, `-jobs`, `-eval` (lib and integration tests), every
`ferritecad-cli` test target except the heavy ones listed under
[Limits](#limits), the CLI binaries, and `ferritecad-app`: **1100 passed,
2 failed, 1 ignored** on the harness (build-specific N/A inside those
counts; the Fillet ones are counted above). The two failures are the
root-only permission tests (`dump_graph`'s
`read_only_permissions_still_dump_when_the_file_can_be_read`, `validate`'s
`validation_really_read_only_permissions`), which fail under this
container's root on base too; run as `nobody` (their own test binaries, a
temporary HOME/TMPDIR, the solver library copied there) both pass. The
ignored one is the pre-existing timing test. Topology, kernel and OCCT
crates are unchanged and were not rerun.

OCCT without PlaneGCS (`--no-default-features`,
`FERRITECAD_REQUIRE_PLANEGCS=0`, release):

* the §28I lines of the runtime-layout step, extracted from the workflow file
  and run as a script: the discovery gate `test … ok`, `FCAD_28I_RECIPE_OK`
  with the same volumes;
* the whole `sequential::height` module: 4 executed and passed; the
  dimensioned test printed `skipped: constrained geometry requires PlaneGCS`
  (N/A here);
* all nine recipes: §28A–D, §28G–I `…_OK`, §28E/F `…_NO_SOLVER`.

Stub (no OCCT; CMake through an explicit toolchain file, native prefixes
ignored):

* the new `ci.yml` step extracted from the workflow file and run as a
  script: four gates `test … ok`, no `skipped:`, and
  `FCAD_28I_RECIPE_NO_KERNEL`;
* `tests/fillet.rs` 54: 10 executed, 44 N/A (`skipped: this build has no
  Open CASCADE`); the app harness 362 passed, its native tests N/A; document
  lib 124 passed, 1 ignored;
* all nine recipes print `FCAD_28x_RECIPE_NO_KERNEL`.

### Mutations — local, executed, restored byte for byte

Each is a compiled change run against the new tests, then the file restored
from a copy and checked by `sha256sum -c`; the regression above ran on the
restored tree.

* **M1 — stale suffix after a height change**: the Fillet archive key no
  longer depends on the predecessor's key (`cold.rs`). Caught by
  `native_a_new_height_misses_the_whole_chain_and_radii_stay_editable`:
  after the height edit Fillet 1 answers Hit where Miss is required, i.e. the
  final Body would be the old height's. The other four pass (cold builds do
  not use the key).
* **M2 — writer re-derivation bypassed**: `write_extrude_height`'s check
  closure returns `Ok(())`. Caught by
  `the_height_writer_refuses_a_forged_two_fillet_preparation` and §28C's
  `the_height_writer_refuses_a_forged_or_stale_preparation`.
* **M3 — Fillet 2 left out of the re-derivation**: `rederive` copies the
  prepared `second_fillet` over the checked one before comparing. Caught by
  `the_height_writer_refuses_a_forged_two_fillet_preparation`; §28C's
  one-Fillet test passes, as it must.

### Compatibility with the reader on `main`

`38064fb`, extracted with `git archive` and built (release, PlaneGCS) in the
scratchpad (`FCAD_28I_COMPAT_OK`):

* A copy this build raised to 11.5 mm: the old reader's `inspect` shows the
  new distance, no `second_fillet`, the base row not editable and
  `edit_extrude.available: false`; `validate` valid; cold `rebuild` resolves
  18 of 18 names; its STL **and** FBX are byte-identical to this build's; its
  `edit-extrude` refuses `unsupported` («this slice edits a plate with one
  Fillet, and this document holds 2 …») without output; the file's SHA-256 is
  unchanged. §28I writes nothing an old reader cannot read.
* A two-Fillet document written by the old build: this build discovers
  `second_fillet`, lowers it to 3.375 mm, the source unchanged; both readers
  validate the copy and it rebuilds 18 of 18.

## CI

* **Base `38064fb`, merge-triggered, checked separately** (read through the
  API): CI ([36677625746](https://github.com/gesriot/ferrite-cad/actions/runs/36677625746)),
  planegcs pin ([36677625733](https://github.com/gesriot/ferrite-cad/actions/runs/36677625733)),
  rust notices, product sbom and rust sbom concluded **success**; the combined
  runtime layout ([36677625739](https://github.com/gesriot/ferrite-cad/actions/runs/36677625739))
  was still **in progress** when this was written and is not counted as a
  success here.
* This change's runs are recorded in a separate commit once they finish.

## Limits

* The class: exactly §28G's two Fillets on one rectangular plate,
  unconstrained or carrying the managed Line family. Not supported: a third
  Fillet, arbitrary edges, Cut with Fillet, Chamfer, editing the Sketch or
  constraints of a two-Fillet history (UI or CLI), in-place Save.
* The height policy is §28C's: the domain adds no bound; a height the kernel
  cannot round is refused at the rebuild.
* Local solver evidence is from an unpinned PlaneGCS.
* A mesh check is chord-bounded; exact claims are the B-Rep's.
* Not run here: any window, GPU, Unity, browser or STEP-corpus test, and the
  heavy CLI targets `complex_step_pixels`, `export_scene_complex`,
  `occurrence_identity_complex`, `imported_step_pixels`,
  `fillet_shell_corpus`, `export_fbx_identity`, `shared_step_import`,
  `import_step`, `export_fbx_complex` (CI runs them).

## macOS fixture and window scenario (for the user's Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised here with a
second CLI invocation standing in for the window, which says nothing about
the window: `FCAD_28I_GUI_COMPARE_OK cells=382`; without the window's files
the comparator stopped with `FCAD_28I_GUI_COMPARE_MISSING …` and created
nothing; a tampered `objects.name` of Fillet 2 in `gui-down.fcad` was caught
(`AssertionError: objects.name`); a stray `never.fcad` was caught. The CLI
refused the scenario's 1e-6 mm height on this fixture as `kernel` («Open
CASCADE could not round this edge at radius 2.000000 …»), writing nothing.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon (arm64) Mac, and do not set `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `twice.fcad` — §28G's plate drawn clockwise from (33, 15.5),
37.5 × 12.25 × 6.75 mm as stored, dimensioned by the shipped command to
41 × 14.25 mm with Segment 1's start fixed at (36.5, 17.75) (stored and
solved XY differ), and rounded by `fillet-edge-copy` twice: Fillet 1 at the
stored corner (33, 3.25) (solved (36.5, 3.5)) with r 2 mm, Fillet 2 at
(33, 15.5) (solved (36.5, 17.75)) with r 5.5 mm — and `facts.json`:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-two-fillet-base-height-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28I_GUI_FIXTURE\n", "ferrite-28i-fixture.py"),
                   ("# FCAD_28I_GUI_COMPARE\n", "ferrite-28i-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_28I_DIR="$PWD/fillet-two-height-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28i-fixture.py "$FCAD_28I_DIR"
```

```python
# FCAD_28I_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-two-height-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# §28G's plate: clockwise from the upper right, dimensioned by the shipped
# command to 41 x 14.25 mm with Segment 1's start fixed at (36.5, 17.75).
# The stored corners (33, 3.25) and (33, 15.5) solve to (36.5, 3.5) and
# (36.5, 17.75) and share the solved 14.25 mm Line (12.25 mm as stored).
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
FIRST, SECOND, H = [33.0, 3.25], [33.0, 15.5], 6.75
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H, "points_mm": PLATE}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
catalog = run("inspect", plate, "--json")["result"]
(sketch,) = catalog["sketches"]
lines = [c["curve_id"] for c in sketch["constraint_edit"]["curves"]]
add = [{"curve_id": lines[0], "rule": "vertical"}, {"curve_id": lines[1], "rule": "horizontal"},
       {"curve_id": lines[2], "rule": "vertical"}, {"curve_id": lines[3], "rule": "horizontal"},
       {"curve_id": lines[0], "rule": "fixed", "at": "start", "x_mm": 36.5, "y_mm": 17.75},
       {"curve_id": lines[1], "rule": "distance", "distance_mm": 41.0},
       {"curve_id": lines[0], "rule": "distance", "distance_mm": 14.25}]
request.write_text(json.dumps({"request_version": 1, "remove": [], "add": add}))
dimensioned = out / "dimensioned.fcad"
run("edit-sketch-constraints-copy", plate, "--sketch", sketch["sketch_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", dimensioned, "--json")
def fillet(source, corner, radius, dest):
    catalog = run("inspect", source, "--json")["result"]
    chosen = next(c for c in catalog["bodies"][0]["fillet_edge"]["target"]["candidates"]
                  if c["stored_corner_mm"] == corner)
    request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": radius}))
    run("fillet-edge-copy", source, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
        catalog["content_version"], "--request", request, "-o", dest, "--json")
# Fillet 1 r 2 mm, then Fillet 2 r 5.5 mm beside it: 5.5 <= 14.25 - 2 - 0.01.
once = out / "once.fcad"
fillet(dimensioned, FIRST, 2.0, once)
source = out / "twice.fcad"
fillet(once, SECOND, 5.5, source)
for p in (request, plate, dimensioned, once):
    p.unlink()
catalog = run("inspect", source, "--json")["result"]
(base,) = catalog["features"]
assert base["editable"] is True and base["distance_mm"] == H, base
ctx = base["fillet_base"]
two = ctx["second_fillet"]
assert ctx["profile_constrained"] is True and two["previous_feature_id"] == ctx["fillet_feature_id"], ctx
assert catalog["sketches"][0]["editable"] is False, "the Sketch editors keep refusing"
(out / "facts.json").write_text(json.dumps({
    "base_feature_id": base["feature_id"], "content_version": catalog["content_version"],
    "first_fillet_id": ctx["fillet_feature_id"], "second_fillet_id": two["fillet_feature_id"],
    "radii_mm": [2.0, 5.5], "height_mm": H, "refused_height_mm": 1e-6,
    "up_height_mm": 12.25, "down_height_mm": 3.5, "gui_rect_mm": [-4.5, 3.5, 41.0, 14.25],
    "gui_corners_mm": [[36.5, 3.5], [36.5, 17.75]],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28I_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_28I_DIR/../watch-28i.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

If system memory pressure aborts or stops the run, it does not count; start
again from a fresh fixture directory. If the automation relaunches the viewer
after it has quit, quit that instance too and read only the original PID's
exit and watchdog record.

1. **Open** `$FCAD_28I_DIR/twice.fcad` (asynchronously). **Edit extrusion…**
   is enabled; the Sketch-coordinate and constraint editors of the base
   Sketch are refused, their reasons saying that with two Fillets only the
   radii and the height can be edited; **Fillet edge of …** is refused
   ("third").
2. **The base row.** Select "Extrude1 — <base_feature_id> — 6.75 mm". The
   form reads "History: Extrude <base> -> Fillet 1 <first_fillet_id> at (33,
   3.25), r 2 mm -> Fillet 2 <second_fillet_id> at (33, 15.5), r 5.5 mm. Both
   Fillets keep their edges and radii; only the plate's height changes." (the
   corners are the stored ones).
3. **Exact input.** Distance `0`: refused "positive"; nothing starts.
4. **Save Cancel.** Distance `12.25`, **Save new file…** → **Cancel** in the
   file dialog: nothing starts; the draft stays `12.25`.
5. **Worker refusal keeps the draft.** Distance `0.000001`, **Save new
   file…** → `$FCAD_28I_DIR/never.fcad`. The job is refused (`kernel`: Open
   CASCADE could not round the edge); the form stays with `0.000001`; the
   status line says why. `never.fcad` does not exist.
6. **Recovery, publish, async Open.** Distance `12.25`, **Save new file…** →
   `$FCAD_28I_DIR/gui-up.fcad`. The viewer opens the copy asynchronously; the
   base row now reads 12.25 mm and the same two Fillets.
7. **Lower it.** In the opened copy, distance `3.5`, **Save new file…** →
   `$FCAD_28I_DIR/gui-down.fcad`; it opens asynchronously: a 41 × 14.25 ×
   3.5 mm plate whose corners (36.5, 3.5) and (36.5, 17.75) are rounded by 2
   and 5.5 mm, nothing else. There is no Undo/Redo for this edit.
8. **Exports.** From the window export `gui-down.stl` and `gui-down.fbx` of
   `gui-down.fcad` into `$FCAD_28I_DIR`, at the default tessellation.
9. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28i-compare.py "$FCAD_28I_DIR"
```

It first requires `gui-up.fcad`, `gui-down.fcad`, `gui-down.stl` and
`gui-down.fbx` (and never creates them), refuses a `never.fcad`, checks the
source's SHA-256, the allowlist of each window edit (the base row's
payload/hash and the stamp only), the distances and both Fillets as the
catalogue describes them, makes the CLI peer (`peer-up.fcad`,
`peer-down.fcad` and its exports, only if absent), compares every SQL cell
but `meta.modified_at` — this edit mints no UUID — and requires byte-equal
STL/FBX, then reads the window's STL itself: the solved rectangle at 3.5 mm,
exactly the two solved corners rounded, each wall at its own radius about
its own axis. It prints `FCAD_28I_GUI_COMPARE_OK cells=N`.

```python
# FCAD_28I_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-up.fcad", "gui-down.fcad", "gui-down.stl", "gui-down.fbx")
missing = [n for n in GUI if not (out / n).is_file()]
if missing:
    sys.exit(f"FCAD_28I_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first")
assert not (out / "never.fcad").exists(), "the refused Save published something"
source = out / "twice.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout
def tables(path):
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    got = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        got[t] = ([d[0] for d in cur.description], sorted(cur.fetchall(), key=repr))
    db.close()
    return got
def allowlist(before, after, feature):
    """Only the base Extrude's payload/payload_hash and meta.modified_at moved."""
    fid = bytes.fromhex(feature.replace("-", ""))
    a, b = tables(before), tables(after)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc and len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                assert u == v or (t == "objects" and c in ("payload", "payload_hash")
                                  and x[ac.index("id")] == fid) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c}"
def same(gui, peer):
    """Every SQL cell equal but the stamp: this edit mints nothing."""
    left, right = tables(gui), tables(peer)
    assert left.keys() == right.keys()
    cells = 0
    for t in left:
        (cols, lrows), (_, rrows) = left[t], right[t]
        keep = [k for k, c in enumerate(cols) if not (t == "meta" and c == "modified_at")]
        norm = lambda rows: sorted((tuple(r[k] for k in keep) for r in rows), key=repr)
        assert norm(lrows) == norm(rrows), t
        cells += len(keep) * len(lrows)
    return cells
def described(path):
    (row,) = run("inspect", path, "--json")["result"]["features"]
    ctx = row["fillet_base"]
    return (row["feature_id"], row["distance_mm"], ctx["fillet_feature_id"], ctx["radius_mm"],
            ctx["second_fillet"]["fillet_feature_id"], ctx["second_fillet"]["previous_feature_id"],
            ctx["second_fillet"]["radius_mm"])
BASE, F1, F2 = facts["base_feature_id"], facts["first_fillet_id"], facts["second_fillet_id"]
r1, r2 = facts["radii_mm"]
up, down = facts["up_height_mm"], facts["down_height_mm"]
# The window's copies against their sources, and what they say.
allowlist(source, out / "gui-up.fcad", BASE)
allowlist(out / "gui-up.fcad", out / "gui-down.fcad", BASE)
assert described(out / "gui-up.fcad") == (BASE, up, F1, r1, F2, F1, r2)
assert described(out / "gui-down.fcad") == (BASE, down, F1, r1, F2, F1, r2)
# The same two edits through the shipped CLI, and its exports.
def peer(src, h, dest):
    if not dest.exists():
        version = run("inspect", src, "--json")["result"]["content_version"]
        run("edit-extrude", src, "--feature", BASE, "--distance-mm", h, "--expect-version",
            version, "-o", dest, "--json")
peer(source, up, out / "peer-up.fcad")
peer(out / "peer-up.fcad", down, out / "peer-down.fcad")
for fmt in ("stl", "fbx"):
    if not (out / f"peer-down.{fmt}").exists():
        run(f"export-{fmt}", out / "peer-down.fcad", "-o", out / f"peer-down.{fmt}", "--json")
cells = same(out / "gui-up.fcad", out / "peer-up.fcad") + same(out / "gui-down.fcad", out / "peer-down.fcad")
for fmt in ("stl", "fbx"):
    assert (out / f"gui-down.{fmt}").read_bytes() == (out / f"peer-down.{fmt}").read_bytes(), fmt
# The window's mesh, read here: the solved plate at the new height with
# exactly its two solved corners rounded, each wall at its own radius.
data = (out / "gui-down.stl").read_bytes()
(count,) = struct.unpack_from("<I", data, 80)
assert len(data) == 84 + 50 * count
pts = [struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for i in range(count) for k in range(3)]
x0, y0, w, d = facts["gui_rect_mm"]
xs, ys, zs = ([p[k] for p in pts] for k in range(3))
for got, want in ((min(xs), x0), (max(xs), x0 + w), (min(ys), y0), (max(ys), y0 + d),
                  (min(zs), 0.0), (max(zs), down)):
    assert abs(got - want) < 1e-4, (got, want)
rounded = [(c, r) for c, r in zip(facts["gui_corners_mm"], (r1, r2))]
for c in ([x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]):
    near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 for p in pts)
    assert near == all(c != at for at, _ in rounded), c
for (cx, cy), r in rounded:
    ax = cx + (r if abs(cx - x0) < 1e-9 else -r)
    ay = cy + (r if abs(cy - y0) < 1e-9 else -r)
    wall = [p for p in pts if abs(p[0] - cx) < r - 1e-4 and abs(p[1] - cy) < r - 1e-4]
    assert len(wall) >= 4, (cx, cy)
    assert all(abs(math.hypot(p[0] - ax, p[1] - ay) - r) < 1e-3 for p in wall), (cx, cy, r)
print("FCAD_28I_GUI_COMPARE_OK", f"cells={cells}")
```
