# §28D — verification record

[Contract and agent recipe](edit-fillet-base-sketch.md).

This records what was executed for this slice and where. It does not repeat
the evidence of §28A–§28C.

## Where and how this was run

* **Base.** Freshly fetched `origin/main` at
  `f3e228694370407ef62dbea2b41356503562db01`, the merge of PR #64 (§28C,
  implementation head `f086e80`). The branch is `edit-fillet-base-sketch`,
  created from it.
* **Merge-triggered CI on the base** is reported in [CI](#ci), separately
  from this change's runs.
* **Cloud container.** Linux x86_64, 4 CPUs, 15 GiB RAM, the session's
  existing OCCT 8.0.1 install and its existing native and stub targets, built
  one at a time. No PlaneGCS is linked here (`FERRITECAD_REQUIRE_PLANEGCS=0`);
  none is needed. OCCT and Boost were not rebuilt.
* **No window, GPU test or browser was launched.** The widget tests drive
  real egui widgets headlessly; that is not a window test.

## What changed

* `document`: `SketchChoice` carries the saved Fillet
  (`fillet: Option<SavedFillet>`). `coordinate_choice` reads the frame with
  `fillet_radius::fillet_over_plate`; on the Fillet's profile Sketch it
  offers the Blind-Extrude coordinate editor, on any other Sketch it refuses
  naming the Fillet, and on a Fillet outside the frame it refuses with the
  reason (`fillet::filleted_outside_frame`, also used by the Extrude
  editor's discovery). `validate_coordinates` checks the candidate with
  `SavedFillet::corner_on` (`rectangle_corners`, `corner_for`,
  `check_radius`, the same functions the Fillet uses) and with
  `keeps_every_side`. The writer, `write_sketch_geometry`, is unchanged: it
  re-derives through the same preparation inside its transaction.
* `cli`: `sketches[].fillet_base`, the object `features[].fillet_base`
  already carries. No command, argument, request or result changed.
* `app`: one context line in the existing Edit Sketch form; the form,
  canvas, Undo/Redo, Save and async Open are the existing ones.
* No capability, schema, payload version, cache key or archive tag.

## New and changed tests

* **Domain** (`fillet_radius::tests`, kernel-free):
  * `the_base_rectangle_moves_and_resizes_alone_and_interleaves` — moved,
    grown, the narrowest allowed (4.75 mm deep for r = 2.375), then height
    9.5 and radius 1.1875 and back: each write changes only the Sketch row,
    and the Fillet's edge, `previous`, radius and every name stay.
  * `a_candidate_rectangle_that_does_not_keep_the_rounded_corner_is_refused`
    — 4.74 mm deep and 4.7 mm wide ("too large", nothing clamped), Lines
    rotated onto other sides ("keep its side"), a trapezoid ("axis-aligned
    rectangle"), a reordered loop.
  * `the_sketch_writer_refuses_a_forged_or_stale_preparation` — a forged
    too-small payload, another plane, a construction Line, a reversed loop;
    stale: a 6 mm deep preparation after the radius was raised to 3.5 mm is
    refused inside the writer ("too large").
  * `a_sketch_under_a_fillet_outside_the_frame_is_refused_with_its_reason`.
  * Four earlier tests that asserted the base Sketch of a rounded plate is
    refused now assert it is editable with the Fillet as context; their
    constraint, circle and annulus refusals are kept.
* **CLI** (`tests/fillet.rs`, `mod sketch`):
  * `sketch_discovery_and_protocol_without_native` (kernel-free): discovery,
    `fillet_base` equal to the Extrude row's, the other editors refused by
    Fillet UUID; too small and sides swapped `input`, trapezoid
    `unsupported` (all `unsupported` "Open CASCADE" in a stub build); a
    malformed request `input` before any kernel; exit 7 on a lost report;
    nothing written; a Fillet outside the frame keeps the row refused.
  * `native_sketch_edits_move_the_rounded_corner_and_keep_every_identity` —
    CCW, CW from the upper right, and CCW from the third Line; two different
    corners; r = 3.0625. Grown and moved to `[-9.25, -2.5, 51.0, 19.625]`
    (x0, y0, W, D), then the edited copy shrunk to `[1.375, 4.0, 18.5,
    8.25]`. Each copy: exactly two SQL cells moved (the Sketch row's payload
    and hash), stored names equal, `validate`, cold `rebuild` with every
    stored reference resolved, cold = Miss = Hit, 7 faces, volume
    `(W·D − (1 − π/4)·r²)·h` to 1e-9 relative, the same `EdgeFilletFace` a
    cylinder of radius r on a vertical axis r inward of the moved corner, the
    four sides and two caps planar, the STL measured separately, FBX
    exported for the ufbx loop.
  * `native_the_same_rectangle_publishes_the_same_model` — zero cells moved,
    the same measurements.
  * `native_a_moved_rectangle_misses_in_place_and_interleaves_with_height_and_radius`
    — under one path the plate and the Fillet miss and neither returns the
    old part, then Hit; then height 9.75 → radius 4.5 → rectangle
    `[-6.0, 2.0, 22.25, 13.5]` on the same UUIDs.
  * `native_sketch_refusals_races_cancellation_and_report_loss_are_atomic` —
    the source, an occupied output and a hard-link alias as destinations,
    a stale version, cancellation, a race at the last barrier, exit 7.
* **App** (`sketch::tests`):
  * `fillet_base_sketch_widgets_show_the_corner_and_keep_the_draft`
    (kernel-free): the context line, "no side may be shorter than 4.75 mm";
    a too-small draft refused with "too large" and kept until Undo; typed
    move, Undo/Redo buttons, one request per press; a cancelled Save, a stale
    completion and a worker refusal all keep the draft.
  * `native_fillet_base_sketch_widgets_worker_and_cli_publish_the_same_part`
    — the widgets' request through the app's worker and the same edit through
    the shipped `edit-sketch-copy`: every SQL cell equal with no identifier
    mapped, byte-identical STL and FBX; a failed async Open restores the
    draft.

## Local results

Native (OCCT 8.0.1, no PlaneGCS, debug, `FERRITECAD_REQUIRE_OCCT=1`):

* `--test fillet`: 22 passed (7 §28A, 5 §28B, 5 §28C, 5 §28D), no
  `skipped:`. Document `fillet` tests: 16 passed. App: 8 passed.
* `--no-default-features`: the five `sketch::` gates, run one by one in the
  workflow's exact form, each `ok` with no `skipped:`.
* Recipe, extracted from the contract and run with the real CLI:
  `FCAD_28D_RECIPE_OK up=6747.662753/6747.735453
  down=1021.975239/1022.047953` (mesh volume against the exact B-Rep one).
* **FBX.** The whole `tools/check-fbx-complex.sh` was run as CI runs it, with
  the §28A–§28D Fillet artifacts (48 files, from
  `FCAD_FILLET_ARTIFACTS=<dir> cargo test -p ferritecad-cli --test fillet`):
  `FCAD_FILLET_UFBX_EXECUTED`, `FCAD_FILLET_RADIUS_UFBX_EXECUTED`,
  `FCAD_FILLET_HEIGHT_UFBX_EXECUTED`, `FCAD_FILLET_SKETCH_UFBX_EXECUTED`,
  exit 0, 5 min 52 s. The six new files (`sketch-{ccw,cw,third}-{up,down}`)
  each read `checks=6 failures=0` and joined their STL at 68 triangles,
  worst 6.94e-18 m.
* **`validation_really_read_only_permissions`** passed under UID 65534
  (`setpriv --reuid=65534 --regid=65534 --clear-groups`); as root it fails,
  since root ignores the permission it tests.
* **Full workspace:** `cargo test --workspace --no-fail-fast` gave
  2126 passed, 19 failed and 2 ignored (§28C: 2115 passed; the 11 more are
  this slice's: 4 domain, 5 CLI, 2 app).
  * The 19 failures are exactly the set of the earlier baseline runs in this
    container, compared line by line: 18 tests that need a linked PlaneGCS
    while `FERRITECAD_REQUIRE_OCCT=1`, and the root-only
    `validation_really_read_only_permissions`, which passes under an
    ordinary UID (above).
  * None of them involves a Fillet or a Sketch edit.
* `cargo fmt --all -- --check` and `git diff --check` are clean.
  `cargo clippy --workspace --all-targets -- -D warnings` is clean natively,
  and so is the stub target with `--all-features`.

Stub (no OCCT), proved by its build and not by an unset variable:

* In this container CMake also finds OCCT through the user package registry
  (`~/.cmake/packages/OpenCASCADE`), so the first stub configure here found
  `/home/user/native/occt-build`, failed to configure the bridge and built
  without a kernel — a stub, but not the clean case. The stub's
  `ferritecad-occt` build directory was removed and rebuilt with `HOME`
  pointing to an empty directory: its `bridge-build/CMakeCache.txt` now
  records `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`, and the build
  script emitted no `cfg(occt)` and no link lines.
* `ldd` on the freshly built stub `ferritecad`, the `fillet` test binary and
  the viewer's test binary: 0 `libTK*` and 0 PlaneGCS imports each (5 shared
  objects in all). The native `ferritecad` and `fillet` binaries import 26
  `libTK*` each.
* The `ci.yml` step was run exactly as written: the discovery gate, the
  widget gate and the four domain gates each `ok` with no `skipped:`; the
  extracted recipe printed `FCAD_28D_RECIPE_NO_KERNEL` with the typed
  `unsupported`.
* **Geometry skips, listed separately — not passes:** in the stub the four
  `sketch::native_*` CLI tests printed `skipped: this build has no Open
  CASCADE` and returned; the native app worker test likewise skips. They
  are N/A there and ran natively above.

### Mutations — local, executed, restored byte for byte

Each was applied by a script and compiled; the file was restored from `HEAD`
and its SHA-256 checked with `sha256sum -c` (`sketch_edit.rs` 3d65ffcd…,
`document.rs` d3689a9a…).

| Mutation | Caught by (executed) |
| --- | --- |
| M1 — `keeps_every_side` returns `Ok(())` at once | domain `a_candidate_rectangle_that_does_not_keep_the_rounded_corner_is_refused` (rotated sides accepted); CLI `sketch_discovery_and_protocol_without_native` (sides swapped: exit 0 where 2 was due) |
| M2 — the Sketch writer no longer compares the prepared payload with its re-derivation | domain `the_sketch_writer_refuses_a_forged_or_stale_preparation` ("another plane was written") |

After restoring, the document `fillet` (16), CLI `--test fillet` (22) and app
(8) tests passed again.

## CI

* **Base `f3e2286`, merge-triggered, checked separately:** CI
  ([run 36347302947](https://github.com/gesriot/ferrite-cad/actions/runs/36347302947)),
  planegcs pin
  ([run 36347302957](https://github.com/gesriot/ferrite-cad/actions/runs/36347302957)),
  rust sbom, rust notices and product sbom concluded success, and so did
  combined runtime layout
  ([run 36347302962](https://github.com/gesriot/ferrite-cad/actions/runs/36347302962))
  concluded success on all three platforms and in the cross-platform
  comparison, after this change was pushed.
* **Code head `f397a47`** (the code, CI files and tools of this change;
  the head that adds this paragraph changes only this record, and
  `docs/**` is outside `runtime-layout.yml`'s and `planegcs-pin.yml`'s path
  filters, so it triggers ordinary CI only):
  * native runtime and packaging
    ([run 36350147817](https://github.com/gesriot/ferrite-cad/actions/runs/36350147817)):
    green on
    [Linux](https://github.com/gesriot/ferrite-cad/actions/runs/36350147817/job/108707134979),
    [macOS](https://github.com/gesriot/ferrite-cad/actions/runs/36350147817/job/108707135080),
    [Windows](https://github.com/gesriot/ferrite-cad/actions/runs/36350147817/job/108707135151)
    and the
    [comparison](https://github.com/gesriot/ferrite-cad/actions/runs/36350147817/job/108719324019).
    That covers the no-solver step with the two `sketch::` gates and
    `FCAD_28D_RECIPE_OK`, the Fillet step with the five `sketch::` CLI,
    four document and two app gates, the FBX step with
    `FCAD_FILLET_SKETCH_UFBX_EXECUTED`, and the arm64 checks on macOS. Each
    of those steps exits 1 on a missing `ok`, a `skipped:` or a missing
    marker. The job logs were not read here: this container's proxy refuses
    the log download, so no count of gates is claimed from them.
  * ordinary CI
    ([run 36350162702](https://github.com/gesriot/ferrite-cad/actions/runs/36350162702)),
    the new stub step with `FCAD_28D_RECIPE_NO_KERNEL` included: success.
  * planegcs pin
    ([run 36350147847](https://github.com/gesriot/ferrite-cad/actions/runs/36350147847)):
    success.

## Limits

* Only the coordinates of the base rectangle under the one saved §28A
  Fillet, into a new copy, with every Line keeping its side. Not supported:
  an arbitrary quadrilateral, constraints, reordering or reversing the loop,
  a rotated plane, a second Fillet, Chamfer, a Cut with a Fillet, preview,
  face picking, in-place Save. The 5C milestone is not complete.
* A canvas drag of one vertex makes a non-rectangle, which is refused; a
  rectangle is moved by editing two vertices (typed, or two drags).
* A mesh check is chord-bounded, not exact. The exact claims are the B-Rep's
  (volume, surface, radius, axis).
* No window, GPU or browser test was run here. The workflows' job logs cannot
  be downloaded through this container's proxy, so no gate count is claimed
  from them.

## macOS fixtures and window scenario (for the reviewer's Mac)

Nothing here was run in this cloud container, which has no window system.
The generator and comparator use only the CLI. They were exercised here with
a second CLI copy standing in for the window's output, which says nothing
about the window: that passed over 129 cells
(`FCAD_28D_GUI_COMPARE_OK cells=129 corner=[36.5, 1.25]`), a tampered
`objects.name` cell was caught, and a missing `gui.stl` was refused without
anything being created.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, and do not
set `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `rounded.fcad` (the plate drawn clockwise from (33, 15.5), 37.5 ×
12.25 × 6.75 mm, with the §28A Fillet r = 2.375 mm at (33, 3.25)) and
`facts.json` into a new directory:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-fillet-base-sketch-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28D_GUI_FIXTURE\n", "ferrite-28d-fixture.py"),
                   ("# FCAD_28D_GUI_COMPARE\n", "ferrite-28d-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28d-fixture.py "$FCAD_28D_DIR"
```

```python
# FCAD_28D_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-sketch-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# Clockwise from the upper right, as the viewer's own widget tests draw it.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
CORNER, R, H = [33.0, 3.25], 2.375, 6.75
# The window's edit: the right side to x 36.5, the bottom to y 1.25.
NEW = [[36.5, 15.5], [36.5, 1.25], [-4.5, 1.25], [-4.5, 15.5]]
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H, "points_mm": PLATE}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
catalog = run("inspect", plate, "--json")["result"]
body = catalog["bodies"][0]
chosen = next(c for c in body["fillet_edge"]["target"]["candidates"] if c["corner_mm"] == CORNER)
request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": R}))
source = out / "rounded.fcad"
run("fillet-edge-copy", plate, "--body", body["body_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", source, "--json")
request.unlink()
plate.unlink()
catalog = run("inspect", source, "--json")["result"]
(sketch,) = [s for s in catalog["sketches"] if s["fillet_base"] is not None]
assert sketch["editable"] is True, sketch
assert [v["start_mm"] for v in sketch["vertices"]] == PLATE, sketch["vertices"]
(out / "facts.json").write_text(json.dumps({
    "sketch_id": sketch["sketch_id"], "content_version": catalog["content_version"],
    "curve_ids": [v["curve_id"] for v in sketch["vertices"]],
    "fillet_feature_id": sketch["fillet_base"]["fillet_feature_id"],
    "edge": sketch["fillet_base"]["edge"], "radius_mm": R, "height_mm": H,
    "corner_mm": CORNER, "saved_mm": PLATE, "new_mm": NEW, "new_corner_mm": [36.5, 1.25],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28D_GUI_FIXTURE_OK", out)
```

### Window scenario

On the reviewer's Apple Silicon (arm64) Mac, run one viewer under the
1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_28D_DIR/../watch-28d.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

1. **Open.** Open `$FCAD_28D_DIR/rounded.fcad`. **Edit Sketch Sketch1 —
   <UUID>…** is enabled (before §28D it was refused naming the Fillet).
2. **Form.** Press it. The form shows the four vertices (33, 15.5),
   (33, 3.25), (−4.5, 3.25), (−4.5, 15.5) and "Rounded by Fillet <UUID> at
   the corner of Lines <UUID> | <UUID>, r 2.375 mm. The Fillet keeps its
   corner and radius: every Line keeps its side, and no side may be shorter
   than 4.75 mm."
3. **Too small, exact refusal.** Change both `15.5` Y fields to `7`. Save is
   replaced by the red text "invalid input: a fillet of 2.375 mm at corner
   the joint of segments <UUID> and <UUID> is too large: this build rounds up
   to 0.5 × the shorter adjacent side (3.75 mm), which is 1.875 mm; nothing
   is clamped". The typed `7`s stay in the draft.
4. **Undo/Redo.** **Undo draft** twice: both Y fields are `15.5` again and
   **Save edited copy…** is back. **Redo draft** once shows one `7` and the
   refusal (the canvas is not a rectangle); **Undo draft** once more.
5. **Resize and move.** Change both `33` X fields to `36.5` and both `3.25`
   Y fields to `1.25`. Optionally drag one vertex on the canvas: the refusal
   names the axis-aligned rectangle; **Undo draft** removes the drag in one
   step.
6. **Save Cancel.** **Save edited copy…** → **Cancel**. The draft stays.
7. **Save and Open.** **Save edited copy…** → `$FCAD_28D_DIR/gui.fcad`. The
   viewer opens the copy asynchronously: the plate is 41 × 14.25 mm, and the
   rounded corner is now at (36.5, 1.25) with the same radius.
8. **Exports.** From the window, export **STL** and **FBX** of `gui.fcad`
   into `$FCAD_28D_DIR` as `gui.stl` and `gui.fbx`, at the default
   tessellation.
9. **Quit** the viewer normally; look only at that PID's exit and the
   watchdog log, then run:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28d-compare.py "$FCAD_28D_DIR"
```

The compare step makes the peer copy with `edit-sketch-copy` and the same
vertices, and exports it. It compares every table and every cell of
`gui.fcad` with the peer's, with no UUID mapping (nothing is minted),
allowing only `meta.modified_at`. It checks the SQL allowlist of `gui.fcad`
against `rounded.fcad` (exactly the Sketch row's payload and hash), the Line
UUIDs and new vertices, the Fillet's UUID, edge, radius and moved corner and
the unchanged height in `inspect`, and that `gui.stl` and `gui.fbx` are
byte-identical to the peer's. It never creates or overwrites a `gui.*` file,
and a missing one is a failure. It prints `FCAD_28D_GUI_COMPARE_OK`.

```python
# FCAD_28D_GUI_COMPARE
import hashlib, json, os, pathlib, sqlite3, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
source, gui = out / "rounded.fcad", out / "gui.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
for name in ("gui.fcad", "gui.stl", "gui.fbx"):
    assert (out / name).exists(), f"export {name} from the window first"
peer = out / "peer.fcad"
if not peer.exists():
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "vertices": [
        {"curve_id": c, "start_mm": p} for c, p in zip(facts["curve_ids"], facts["new_mm"])]}))
    p = subprocess.run([cli, "edit-sketch-copy", source, "--sketch", facts["sketch_id"],
                        "--expect-version", facts["content_version"], "--request", request,
                        "-o", peer, "--json"], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (p.stdout, p.stderr)
for fmt in ("stl", "fbx"):
    target = out / f"peer.{fmt}"
    if not target.exists():
        p = subprocess.run([cli, f"export-{fmt}", peer, "-o", target, "--json"],
                           capture_output=True, encoding="utf-8")
        assert p.returncode == 0, (p.stdout, p.stderr)

def tables(path):
    db = sqlite3.connect(f"{path.as_uri()}?mode=ro", uri=True)
    out = {}
    for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
        cur = db.execute(f'SELECT * FROM "{t}"')
        out[t] = ([d[0] for d in cur.description], sorted(cur.fetchall(), key=repr))
    db.close()
    return out

sketch = bytes.fromhex(facts["sketch_id"].replace("-", ""))
def differing(a, b):
    """Every cell that differs between two copies, as (table, column, id)."""
    left, right = tables(a), tables(b)
    assert left.keys() == right.keys()
    moved, compared = set(), 0
    for t in left:
        (lc, lrows), (rc, rrows) = left[t], right[t]
        assert lc == rc and len(lrows) == len(rrows), t
        if "id" in lc:
            k = lc.index("id")
            lrows, rrows = sorted(lrows, key=lambda r: r[k]), sorted(rrows, key=lambda r: r[k])
        for lr, rr in zip(lrows, rrows):
            for c, x, y in zip(lc, lr, rr):
                compared += 1
                if x != y:
                    moved.add((t, c, lr[lc.index("id")] if "id" in lc else None))
    return moved, compared

stamp = {("meta", "modified_at", 1)}
# The window's copy against the peer's: identical but for the time stamp.
moved, compared = differing(gui, peer)
assert moved <= stamp, moved
# And against its source: only the base Sketch's payload and hash moved.
moved, _ = differing(source, gui)
assert moved - stamp == {("objects", "payload", sketch), ("objects", "payload_hash", sketch)}, moved
p = subprocess.run([cli, "inspect", gui, "--json"], capture_output=True, encoding="utf-8")
result = json.loads(p.stdout)["result"]
(row,) = [s for s in result["sketches"] if s["sketch_id"] == facts["sketch_id"]]
assert [v["curve_id"] for v in row["vertices"]] == facts["curve_ids"], row
assert [v["start_mm"] for v in row["vertices"]] == facts["new_mm"], row
(fillet,) = result["fillets"]
assert fillet["feature_id"] == facts["fillet_feature_id"] and fillet["edge"] == facts["edge"], fillet
assert fillet["radius_mm"] == facts["radius_mm"], fillet
assert fillet["corner_mm"] == facts["new_corner_mm"], fillet
assert row["fillet_base"]["corner_mm"] == facts["new_corner_mm"], row
(feature,) = [f for f in result["features"] if f["fillet_base"] is not None]
assert feature["distance_mm"] == facts["height_mm"], feature
for fmt in ("stl", "fbx"):
    assert (out / f"gui.{fmt}").read_bytes() == (out / f"peer.{fmt}").read_bytes(), fmt
print("FCAD_28D_GUI_COMPARE_OK", f"cells={compared}", f"corner={fillet['corner_mm']}")
```
