# §28F — verification record

[Contract and recipe](fillet-constrained-plate.md).

## Where and how this was run

* **Base.** Freshly fetched `origin/main` at
  `eafd925882d4a8d4c0eff7c437aa2a5dec043b65`, tree
  `4eb45d565f1f15c080578917f166931a28c8714c` (PR #69, merged; checked through
  the GitHub API). The branch `fillet-constrained-plate` was created from it.
  Merge-triggered CI on the base is in [CI](#ci), separately from this
  change's runs.
* **Cloud container.** Linux x86_64, 4 CPUs, 15 GiB RAM, the session's
  existing OCCT 8.0.1 install and native/stub targets, one build at a time,
  `CARGO_BUILD_JOBS=2`. OCCT, Boost and PlaneGCS were not rebuilt.
* **PlaneGCS: local, unpinned.** The proxy refuses the pinned FreeCAD archive
  (HTTP 403) and the pinned Boost archive, so the pinned library cannot be
  built here; no network restriction was worked around. The library used is
  the one an earlier slice (§27H) built from the FreeCAD 1.0.1 `planegcs`
  sources with Ubuntu's Eigen 3.4.0 and Boost 1.83, linked only through
  `FCAD_PLANEGCS_DIR`/`LD_LIBRARY_PATH`. **Every local solver result below is
  from that library; the named CI gates on Linux, macOS (Apple Silicon) and
  Windows with the pinned delivery are the authoritative solver evidence.**
* **No window, GPU test, browser or large STEP corpus was run.** The widget
  tests drive real egui widgets headlessly; that is not a window test.

## What changed

* `document`: `cut_edit::read_history` takes whether the base may carry the
  constraint editor's managed Line family; the new
  `saved_plate_for_fillet` reads the plate a Fillet would round with it
  allowed (only with no Cut; a Cut history stays unconstrained). The Fillet
  target reads its candidates from the stored Lines (`corners_of_lines`),
  records `constrained`, and applies the full stored bound only to an
  unconstrained plate (`SavedFilletTarget::check_radius`); a constrained one
  gets the value part (finite, ≥ 0.01 mm). `PreparedEdgeFillet` carries
  `profile_constrained`. The evaluator (`evaluable_fillet`) is unchanged: it
  already judged a constrained plate on the built Lines (§28E).
* `jobs`: the copy job's cold rebuild after the write also returns the new
  Fillet's corner, asked by the same `evaluable_fillet` of the Lines that very
  rebuild built the predecessor from (no second rebuild or solve);
  `AddedEdgeFillet` gains `built_corner` and `profile_constrained`.
* `cli`: `fillet_edge.target.profile_constrained`; candidates' part numbers
  nullable and `null` for a constrained plate; `stored_corner_mm` and
  `stored_adjacent_lengths_mm`; the result's `corner_mm` from the built plate,
  plus `adjacent_lengths_mm`, `profile_constrained`, `stored_corner_mm`.
* `app`: the **Fillet edge** form says the plate is dimensioned, labels the
  numbers as stored and the limit as the solved plate's; the §28E radius form
  no longer shows an `r ≤` read off stored sides for a constrained plate.

## New tests

CLI (`crates/ferritecad-cli/tests/fillet.rs`, `constraints::first`):

* `rounding_discovery_and_protocol_without_native` — kernel-free discovery on
  a plate dimensioned by the shipped preparation/writer; the unconstrained
  plate's fields unchanged; protocol by build (no kernel: `unsupported` «Open
  CASCADE»; OCCT without PlaneGCS: `unsupported` «constraint»; both: the
  100 mm request is refused `input` «too short» by the solve); malformed
  request `input`; radius 0.005 mm `input` once a kernel is there.
* `occt_without_solver_refuses_rounding_a_dimensioned_plate` — mixed gate;
  explicit `skipped:` where a solver is linked.
* `native_a_dimensioned_plate_is_rounded_at_its_solved_corner` — CCW, CW from
  the upper right, CCW from the third corner; three different corners;
  translated to (−9.25, −2.5), 41.125 × 15.625 mm; r 3.0625.
* `native_both_orders_publish_the_same_part` — constraints→Fillet vs
  Fillet→constraints.
* `native_edits_after_rounding_a_dimensioned_plate` — Replace length, radius,
  height.
* `native_the_radius_bound_is_the_solved_plates_in_both_directions`.
* `native_rounding_refusals_on_a_dimensioned_plate_are_atomic`.

Every published copy goes through one checker (`rounded`): result identities,
`corner_mm` = solved corner within 1e-9 mm and `stored_corner_mm` = stored,
the exact §28A allowlist (`only_the_fillet_was_added`) and the Sketch SQL row
byte-identical, every source name kept and exactly seven added, `fillets[]`
constrained with a `null` bound, the coordinate editor refusing; the solved
Line starts equal to the expected rectangle within 1e-9 mm and the solver's
DOF; `validate`; cold `rebuild` resolving every name; the B-Rep cold, through
a real cache Miss and a Hit (same solid): 7 faces, volume
`(W·D − (1 − π/4) r²) h` within 1e-9 relative, the named `edge fillet face`
a Cylinder of r with its axis r inward of the solved corner; the STL read
independently (closed, one orientation, extents of the solved plate, only
that corner rounded, arc vertices on the radius, chord-bounded volume); the
FBX complete.

Document: `fillet::tests::a_dimensioned_plate_is_a_target_judged_by_value_until_it_is_solved`
(candidates are the stored joints; 7 mm beyond the stored 6.125 mm passes the
value policy, 0.005 and NaN refused; a preparation made under the constrained
policy is refused by the writer, writing nothing, once the plate is
unconstrained again).

App: `fillets::tests::fillet_widgets_on_a_dimensioned_plate_label_stored_numbers_and_defer_the_bound`
(no kernel) and `fillets::tests::native_dimensioned_fillet_worker_and_cli_publish_the_same_part`
(7.5 mm refused by the worker, the draft kept and nothing written; 6.5 mm —
beyond the stored 6.125 mm, within the solved 7.125 mm — published by the
worker and by the CLI; SQL equal after matching the minted Fillet and name
UUIDs; STL and FBX byte-identical; async Open). The worker/CLI comparison was
moved into a shared helper used by the §28A test as well.

## Local results

Solver build (OCCT 8.0.1 + the unpinned PlaneGCS):

* `tests/fillet.rs`: 36 passed, 0 failed (29 existing + 7 new), debug and
  release. The new native tests all assert DOF 0 read from the solver's
  report, and solved Line starts within 1e-9 mm of the expected rectangle,
  which differs from the stored one by several millimetres (e.g. the stored
  corner (33, 3.25) at (35.25, 2.0) on the 41.25 × 15.5 mm plate).
* Radius in both directions: 7 mm (stored bound 6.125) published on the
  41.25 × 15.5 plate (solved bound 7.75); 5 mm (within the stored bound)
  refused `input` «too short … 8.5» on the 30 × 8.5 plate; a solved depth of
  exactly 2r (4.75 mm, r 2.375) published; 2r − 1e-7 refused.
* Both orders: every SQL table equal once the Fillet UUID, its seven name
  UUIDs and the constraint UUIDs (by stored position) are matched, rows
  compared as sets; the measured solids the same; the STL triangles the same;
  the solves the same.
* Solve-dependent refusals: a pin beyond the first Line's end with the sides
  H/V publishes as a constraint copy of the plain plate, and rounding it is
  refused `input` «moves a Line off its side»; one length without H/V
  publishes a slanted polygon, and rounding it is refused `unsupported`
  «rectangle». On the rounded result, a second length on the opposite side is
  refused `constraint` with its UUIDs. Stale version and occupied destination
  `input`; the occupied file is kept.
* Document 20 fillet unit tests; app 6 fillet tests (4 existing, 2 new).
* Recipe: `FCAD_28F_RECIPE_OK rounded=4244.559534/4244.801693
  replaced=3093.684526/3093.926693 radius5=3128.524036/3128.692190
  taller=4518.979163/4519.222052` (measured mesh / exact volume, mm³).
* FBX: `tools/check-fbx-complex.sh --release --features planegcs` exactly as
  CI runs it, with the Fillet artifacts of the release run: all six Fillet
  markers including the new `FCAD_FILLET_FIRST_UFBX_EXECUTED`; the six new
  files `first-ccw/cw/third/rounded/wide/bound` read by pinned ufbx 0.23.0
  (`checks=6 failures=0`), 64–96 triangles, STL↔FBX worst 6.94e-18 m.
* fmt and clippy (`-D warnings`, all targets) on document, jobs, cli and app
  with the solver features: clean.

Regression of the affected crates, solver build, debug (`--no-fail-fast`):
`ferritecad-document` (lib and integration tests), `ferritecad-jobs`,
`ferritecad-eval`, every `ferritecad-cli` test target except the heavy ones
listed under [Limits](#limits), the CLI binaries, and `ferritecad-app`:
1078 passed, 2 failed. The two are the permission tests below: as root they
fail identically on base `eafd925` (built from `git archive` in the
scratchpad), and as `nobody` both pass on this branch. No other difference.

OCCT without PlaneGCS (`--no-default-features`, `FERRITECAD_REQUIRE_PLANEGCS=0`,
the CI argv):

* gates `constraints::first::rounding_discovery_and_protocol_without_native`,
  `constraints::first::occt_without_solver_refuses_rounding_a_dimensioned_plate`
  and the two §28E ones: `test … ok`, no `skipped:`. The refusal: `unsupported:
  this sketch carries 11 constraint(s) and this build did not link planegcs, …`.
* Recipe: `FCAD_28F_RECIPE_NO_SOLVER` at the first constraint copy.
* The unconstrained route rounds a plate without a solver (7 faces) inside the
  mixed gate.

Stub (no OCCT; CMake configured through an explicit toolchain file with the
package registries off and the native prefixes ignored, `OpenCASCADE_DIR-NOTFOUND`
in the cache; the stub CLI imports 0 OCCT/PlaneGCS libraries):

* the three `ci.yml` gates (CLI discovery, app widgets, document writer):
  `test … ok`, no `skipped:`.
* Recipe: `FCAD_28F_RECIPE_NO_KERNEL`.
* `tests/fillet.rs` whole: 36 passed, of which 29 print `skipped: this build
  has no Open CASCADE` (geometry); 7 kernel-free tests executed.

### Mutations — local, executed, restored byte for byte

Each is a compiled change, run against the new tests, then the file restored
from a copy (`sha256 95521f08…` checked each time).

* **M1 — stored instead of solved.** `evaluable_fillet` handed the stored
  Lines to the solved check. Caught by 4 of 7:
  `native_a_dimensioned_plate_is_rounded_at_its_solved_corner` and
  `native_edits_after_rounding_…` (the published `corner_mm` is the stored
  corner (33, 3.25)/(33, 15.5), not the solved one),
  `native_the_radius_bound_is_the_solved_plates_in_both_directions` (7 mm
  refused by the stored bound), `native_rounding_refusals_…` (the flipped plate
  published, exit 0). `native_both_orders_…` passes under it, as it must: the
  mutation changes both routes alike.
* **M2 — the neighbouring joint in preparation.** Caught by 6 of 7, all by the
  writer's re-derivation («the prepared fillet does not describe the document
  it is being written to»), not by geometry.
* **M2b — the neighbouring joint, consistently.** Each constrained candidate
  carries its neighbour's joint in discovery, preparation and re-derivation
  alike. Caught by 5 of 7 by the geometry: the published `corner_mm` is
  another corner of the solved plate (e.g. (31.875, 13.125) instead of
  (31.875, −2.5)); `native_both_orders_…` by its name matching.
* **M3 — no value policy before the solve.** Equivalent at the CLI contract
  level: all 7 CLI tests pass, because the evaluator refuses the same radius
  with the same kind at the copy's rebuild. Caught by the executed document
  test («at least») and the widget test («0.005 was applied»), where no
  rebuild follows.

### Compatibility with the reader on `main` after #69

`eafd925`, extracted with `git archive` and built with PlaneGCS in the
scratchpad, against a document this change wrote (plate → dimensioned →
Fillet r 7 mm): its `validate` says valid; its cold `rebuild` resolves 10 of 10
names with the Fillet as tip; its `inspect` lists the Fillet with
`profile_constrained: true` and an available radius edit; its STL **and** FBX
of that document are byte-identical to this build's; its
`edit-fillet-radius` edits it. Its `fillet-edge-copy` on the dimensioned plate
still refuses, `unsupported: this slice edits an unconstrained part`, and its
discovery says `available: false`. No new stored semantics, so no capability.

## CI

* **Base `eafd925`, merge-triggered, checked separately:** CI
  ([run 36480009948](https://github.com/gesriot/ferrite-cad/actions/runs/36480009948)),
  combined runtime layout
  ([36480009935](https://github.com/gesriot/ferrite-cad/actions/runs/36480009935)),
  planegcs pin
  ([36480010010](https://github.com/gesriot/ferrite-cad/actions/runs/36480010010)),
  product sbom, rust sbom and rust notices: all concluded success (workflow
  runs read through the API; the check-run count itself was not recounted).
* **This change, code head `f402aef`** (first and only CI round, no fix was
  needed): CI
  ([run 36606963811](https://github.com/gesriot/ferrite-cad/actions/runs/36606963811)),
  planegcs pin
  ([36606919303](https://github.com/gesriot/ferrite-cad/actions/runs/36606919303))
  and the combined runtime layout
  ([36606919110](https://github.com/gesriot/ferrite-cad/actions/runs/36606919110):
  Linux, macOS on Apple Silicon, Windows, and the comparison of the three)
  concluded success. That includes the new stub step, the mixed step with
  the two no-solver gates and `FCAD_28F_RECIPE_NO_SOLVER`, the Fillet step
  with the six native CLI gates, the document and app gates and
  `FCAD_28F_RECIPE_OK` against the pinned PlaneGCS, and the FBX campaign with
  `FCAD_FILLET_FIRST_UFBX_EXECUTED`. The job logs cannot be downloaded
  through this container's proxy, so the step conclusions are what is
  claimed, not gate counts read from logs.
* This documentation-only commit changes no input of the runtime layout or
  the pin; CI runs on it.

## Limits

* The class: one Fillet over one XY axis-aligned rectangular Blind/NewBody
  plate whose Sketch is unconstrained or carries the constraint editor's
  managed Line family. Not supported: an arbitrary quadrilateral, a second
  Fillet, Cut with Fillet, Chamfer, another or rotated plane, new constraint
  families, picking, live preview, in-place Save. The 5C milestone is not
  closed.
* Discovery cannot say whether a constrained plate will carry a radius; only
  the copy's rebuild can, and it does before anything is published.
* Local solver evidence is from an unpinned PlaneGCS; the pinned one runs
  only in CI.
* A mesh check is chord-bounded; exact claims are the B-Rep's.
* Not run here: any window, GPU, browser or STEP-corpus test, and the heavy
  CLI targets `complex_step_pixels`, `export_scene_complex`,
  `occurrence_identity_complex`, `imported_step_pixels`,
  `fillet_shell_corpus`, `export_fbx_identity`, `shared_step_import`,
  `import_step` (CI runs them); `export_fbx_complex` ran only inside the FBX
  checker. The historical fillet-corpus OOM is not explained.
* Two permission tests refuse privileged chmod as evidence and fail under
  this container's root by design: `dump_graph`'s
  `read_only_permissions_still_dump_when_the_file_can_be_read` (PR #68) and
  `validate`'s `validation_really_read_only_permissions`. Neither is counted
  from the root run. Run as `nobody` (`runuser -u nobody`, their own test
  binaries, a temporary HOME/TMPDIR, the solver library copied there for the
  run) both pass.

## macOS fixtures and window scenario (for the reviewer's Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised here with a
second CLI copy standing in for the window, which says nothing about the
window: `FCAD_28F_GUI_COMPARE_OK cells=115`; a tampered `objects.name` of the
Fillet in `gui.fcad` was caught (`AssertionError: objects`); a missing
`gui.stl` was refused without anything being created.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon (arm64) Mac, and do not set `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `dimensioned.fcad` — the plate drawn clockwise from (33, 15.5),
37.5 × 12.25 × 6.75 mm, dimensioned by the shipped command to 41 × 14.25 mm
with Segment 1's start fixed at (36.5, 17.75) — and `facts.json`:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/fillet-constrained-plate-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28F_GUI_FIXTURE\n", "ferrite-28f-fixture.py"),
                   ("# FCAD_28F_GUI_COMPARE\n", "ferrite-28f-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28f-fixture.py "$FCAD_28F_DIR"
```

```python
# FCAD_28F_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-first-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# Clockwise from the upper right, as the viewer's own widget tests draw it:
# Segment 1 runs -Y, 2 runs -X, 3 runs +Y, 4 runs +X.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
STORED, H = [33.0, 3.25], 6.75
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H, "points_mm": PLATE}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
catalog = run("inspect", plate, "--json")["result"]
(sketch,) = catalog["sketches"]
lines = [c["curve_id"] for c in sketch["constraint_edit"]["curves"]]
assert [c["start_mm"] for c in sketch["constraint_edit"]["curves"]] == PLATE
# Dimensioned by the shipped command: V, H, V, H on Segments 1..4; Segment
# 1's start fixed at (36.5, 17.75); Segment 2 = 41 mm; Segment 1 = 14.25 mm.
# The stored corner (33, 3.25) solves to (36.5, 3.5); the stored sides allow
# r <= 6.125 mm there, the solved ones r <= 7.125 mm.
add = [{"curve_id": lines[0], "rule": "vertical"},
       {"curve_id": lines[1], "rule": "horizontal"},
       {"curve_id": lines[2], "rule": "vertical"},
       {"curve_id": lines[3], "rule": "horizontal"},
       {"curve_id": lines[0], "rule": "fixed", "at": "start", "x_mm": 36.5, "y_mm": 17.75},
       {"curve_id": lines[1], "rule": "distance", "distance_mm": 41.0},
       {"curve_id": lines[0], "rule": "distance", "distance_mm": 14.25}]
request.write_text(json.dumps({"request_version": 1, "remove": [], "add": add}))
source = out / "dimensioned.fcad"
run("edit-sketch-constraints-copy", plate, "--sketch", sketch["sketch_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", source, "--json")
request.unlink()
plate.unlink()
catalog = run("inspect", source, "--json")["result"]
body = catalog["bodies"][0]
target = body["fillet_edge"]["target"]
assert body["fillet_edge"]["available"] is True and target["profile_constrained"] is True, body
chosen = next(c for c in target["candidates"] if c["stored_corner_mm"] == STORED)
assert chosen["corner_mm"] is None and chosen["max_radius_mm"] is None, chosen
(out / "facts.json").write_text(json.dumps({
    "body_id": body["body_id"], "body_name": body.get("name"),
    "content_version": catalog["content_version"], "sketch_id": sketch["sketch_id"],
    "lines": lines, "edge": chosen["edge"], "stored_corner_mm": STORED,
    "refused_radius_mm": 7.5, "radius_mm": 6.5, "height_mm": H,
    "gui_rect_mm": [-4.5, 3.5, 41.0, 14.25], "gui_corner_mm": [36.5, 3.5],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28F_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_28F_DIR/../watch-28f.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

1. **Open** `$FCAD_28F_DIR/dimensioned.fcad` (asynchronously, as always).
   **Fillet edge of <Body> — <UUID>…** is enabled (before §28F it was refused
   for a constrained profile).
2. **Form.** Press it. It says "This plate's Sketch carries constraints. The
   corners and sides below are the stored drawing, the solver's starting
   guess. …", and lists four corners as "Lines <UUID> | <UUID> — stored corner
   (x, y), stored sides a × b mm; r limit from the solved plate". No `r ≤`
   number is shown.
3. **Invalid radius.** Choose "Lines … — stored corner (33, 3.25)". Radius
   `0.005`, **Apply fillet**: refused "… at least 0.01 mm …"; nothing
   applied; the selection stays.
4. **Save Cancel.** Radius `7.5`, **Apply fillet** (accepted: the stored
   drawing's 6.125 mm does not decide it). "Ready: round the edge between
   Lines … with r7.5 mm; the radius is checked on the solved plate when
   saved". **Save fillet copy…** → **Cancel**: nothing starts; the draft
   stays.
5. **Refusal.** **Save fillet copy…** → `$FCAD_28F_DIR/never.fcad`. The job
   is refused; the form comes back with its draft; the status line reads
   "Could not save edited model: invalid input: as its constraints solve it,
   the rounded plate has sides of 14.25 and 41 mm at the rounded corner, too
   short for the saved radius: a fillet of 7.5 mm … is too large: … 7.125 mm;
   nothing is clamped". `never.fcad` does not exist.
6. **Publish.** Radius `6.5`, **Apply fillet**, **Save fillet copy…** →
   `$FCAD_28F_DIR/gui.fcad`. The viewer opens the copy asynchronously: a
   41 × 14.25 mm plate whose corner at (36.5, 3.5) is rounded — not the stored
   (33, 3.25).
7. **Exports.** From the window export `gui.stl` and `gui.fbx` of `gui.fcad`
   into `$FCAD_28F_DIR`, at the default tessellation.
8. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28f-compare.py "$FCAD_28F_DIR"
```

The comparator never creates or overwrites a `gui*` file; a missing one, or an
existing `never.fcad`, is a failure. It checks `gui.fcad` against its source
by §28A's allowlist (the Sketch row byte for byte), makes the peer copy with
`fillet-edge-copy` from the same request and exports it, compares every
table of `gui.fcad` with the peer's once the Fillet and its seven name UUIDs
are matched (only the stamp and the hashes over minted UUIDs excepted, their
payloads compared), requires byte-identical STL and FBX, and reads the solved
extents and the rounded corner from the window's STL; it prints
`FCAD_28F_GUI_COMPARE_OK`.

```python
# FCAD_28F_GUI_COMPARE
import hashlib, json, os, pathlib, sqlite3, struct, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
source = out / "dimensioned.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
for name in ("gui.fcad", "gui.stl", "gui.fbx"):
    assert (out / name).exists(), f"produce {name} from the window first"
assert not (out / "never.fcad").exists(), "the refused Save published something"

def run(*args, code=0):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == code, (args, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def tables(path):
    db = sqlite3.connect(f"{path.as_uri()}?mode=ro", uri=True)
    result = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        result[t] = ([d[0] for d in cur.description], cur.fetchall())
    db.close()
    return result

FILLET_CAPABILITIES = {"feature.fillet.v1", "topology.origin-face.v1", "feature.predecessor.v1"}
def fillet_allowlist(src, copy, body):
    """§28A's allowlist against the source: the Body row's payload/hash, one
    new object, +2/-1 edges, seven names, Fillet capabilities if missing and
    the stamp. The Sketch row, constraints and all, is the same bytes."""
    bid = bytes.fromhex(body.replace("-", ""))
    a, b = tables(src), tables(copy)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        if t == "objects":
            k = ac.index("id")
            mine = {r[k]: r for r in brows}
            assert len(brows) == len(arows) + 1, "one new object"
            for row in arows:
                for c, u, v in zip(ac, row, mine[row[k]]):
                    assert u == v or (row[k] == bid and c in ("payload", "payload_hash")), f"objects.{c}"
        elif t in ("deps", "topology_refs"):
            lost = [r for r in arows if r not in brows]
            assert all(t == "deps" and bid in r for r in lost), (t, lost)
            assert len(brows) - (len(arows) - len(lost)) == (2 if t == "deps" else 7), t
        elif t == "capabilities":
            assert set(arows) <= set(brows)
            assert {r[0] for r in set(brows) - set(arows)} <= FILLET_CAPABILITIES
        elif t == "meta":
            for x, y in zip(arows, brows):
                assert all(u == v or c == "modified_at" for c, u, v in zip(ac, x, y)), "meta"
        else:
            assert sorted(arows, key=repr) == sorted(brows, key=repr), t

def fillet_of(path):
    (f,) = run("inspect", path, "--json")["result"]["fillets"]
    return f

def same_publication(gui, peer):
    """Every SQL cell equal once the Fillet's UUID and its seven names, the
    identifiers this operation minted, are matched; the stamp and the hashes
    over minted UUIDs (compared through their payloads) excepted."""
    g, p = fillet_of(gui)["feature_id"], fillet_of(peer)["feature_id"]
    gb, pb = bytes.fromhex(g.replace("-", "")), bytes.fromhex(p.replace("-", ""))
    pairs = [(pb, gb)]
    def mapped(v):
        if isinstance(v, bytes):
            for x, y in pairs:
                v = v.replace(x, y)
        return v
    left, right = tables(gui), tables(peer)
    (rc, rrows), (lc, lrows) = right["topology_refs"], left["topology_refs"]
    i, o, pl = rc.index("id"), rc.index("owner_id"), rc.index("payload")
    mine = {(r[o], r[pl]): r[i] for r in lrows if r[o] == gb}
    for r in rrows:
        if r[o] == pb:
            pairs.append((r[i], mine[(gb, mapped(r[pl]))]))
    assert len(pairs) == 8, pairs
    cells = 0
    for t in left:
        (cols, lrows), (_, rrows) = left[t], right[t]
        keep = [k for k, c in enumerate(cols)
                if not (t == "meta" and c == "modified_at") and c != "payload_hash"]
        norm = lambda rows, f: sorted((tuple(f(r[k]) for k in keep) for r in rows), key=repr)
        assert norm(lrows, lambda v: v) == norm(rrows, mapped), t
        cells += len(keep) * len(lrows)
    return cells

def stl(path):
    data = path.read_bytes()
    (count,) = struct.unpack_from("<I", data, 80)
    return [struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k)
            for i in range(count) for k in range(3)]

# The window's copy against its source.
fillet_allowlist(source, out / "gui.fcad", facts["body_id"])
f = fillet_of(out / "gui.fcad")
assert f["edge"] == facts["edge"] and f["radius_mm"] == facts["radius_mm"], f
assert f["profile_constrained"] is True and f["radius_edit"]["max_radius_mm"] is None, f
# The same request through the shipped CLI, and its exports.
peer = out / "peer.fcad"
if not peer.exists():
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "edge": {
        "feature_id": facts["edge"]["feature_id"], "joint": facts["edge"]["joint"][::-1]},
        "radius_mm": facts["radius_mm"]}))
    done = run("fillet-edge-copy", source, "--body", facts["body_id"], "--expect-version",
               facts["content_version"], "--request", request, "-o", peer, "--json")["result"]
    assert done["stored_corner_mm"] == facts["stored_corner_mm"], done
    assert all(abs(u - v) < 1e-9 for u, v in zip(done["corner_mm"], facts["gui_corner_mm"])), done
for fmt in ("stl", "fbx"):
    if not (out / f"peer.{fmt}").exists():
        run(f"export-{fmt}", peer, "-o", out / f"peer.{fmt}", "--json")
cells = same_publication(out / "gui.fcad", peer)
for fmt in ("stl", "fbx"):
    assert (out / f"gui.{fmt}").read_bytes() == (out / f"peer.{fmt}").read_bytes(), fmt
# The window's mesh is the solved plate, rounded at the solved corner only.
x0, y0, w, d = facts["gui_rect_mm"]
pts = stl(out / "gui.stl")
xs, ys, zs = ([p[k] for p in pts] for k in range(3))
for got, want in ((min(xs), x0), (max(xs), x0 + w), (min(ys), y0), (max(ys), y0 + d),
                  (min(zs), 0.0), (max(zs), facts["height_mm"])):
    assert abs(got - want) < 1e-4, (got, want)
for c in ([x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]):
    near = any(abs(p[0] - c[0]) < 1e-4 and abs(p[1] - c[1]) < 1e-4 for p in pts)
    assert near == (c != facts["gui_corner_mm"]), c
print("FCAD_28F_GUI_COMPARE_OK", f"cells={cells}")
```
