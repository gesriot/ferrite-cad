# §27G — verification record

[Contract and agent recipe](revolve-profile-constraints.md).

This records what was executed for this slice and where. It does not repeat
the earlier slices' evidence.

## Where and how this was run

* **Base.** `origin/main` at `5caaa1ca293cdffc3e8daf5490bb03281120adf3`,
  the merge of PR #58. Its tree `94bf726…` equals the reviewed §27F head
  `731fea9`. The branch is `revolve-profile-constraints`.
* **Cloud container.** Linux x86_64, 4 CPUs, 15 GiB RAM, using the
  session's installed OCCT 8.0.1 and its existing native/stub targets.
  * **PlaneGCS is not available here.** The proxy refuses the pinned
    FreeCAD and Boost archives (HTTP 403), so no solver was built. Every
    test that solves therefore runs only in the named CI gates
    (Linux/macOS/Windows, `FERRITECAD_REQUIRE_PLANEGCS=1`, exact name, no
    skip).
  * **What did run locally:** the discovery, request, writer, UI-widget,
    worker-refusal and OCCT-without-solver evidence.
* **No window, GPU test or browser was launched.** The widget tests drive
  real egui widgets headlessly; that is not a window test. The macOS window
  smoke is the reviewer's (see the last section).

## What changed

* **Domain** (`ferritecad-document`):
  * `sketch_edit::constraint_frame` gives the constraint editor the owning
    feature explicitly, as the existing `SketchProfileUse`. A Sketch that a
    Revolve turns is judged by the shared `revolve_document` frame, never by
    the Extrude frame.
  * Axis-closed Revolve profiles are refused by name.
  * `constrained_lines` checks the **stored** inputs against the owner's
    policy.
  * `ConstraintSketchChoice` gains `profile_use`; `height_mm` is now `None`
    for a Revolve.
  * `PreparedSketchConstraints` carries a private `profile_use` with an
    accessor, in place of the public `height_mm`.
  * `SketchProfileUse::check` is public, so a solved profile is judged by
    the same function as a stored one.
  * `revolve_choices` no longer refuses a profile merely for carrying
    constraints. This is what lets the §27E angle edit keep them.
* **Copy job** (`ferritecad-jobs`): the solved-profile check passes the
  solved Lines, with their UUIDs, through `SketchProfileUse::check`.
  * For an Extrude this is exactly the previous `PolygonExtrusion::new`.
  * For a Revolve it is `stated_revolution`: class, axis clearance and a
    simple polygon.
  * A circle profile still uses the extrusion height, which a Revolve never
    reaches.
* **CLI:**
  * `constraint_edit.profile_feature` is additive, reusing the
    `ProfileFeature` kinds.
  * The `edit-sketch-constraints-copy` request now also refuses arrays in
    place of the request or an addition.
* **UI:** one line in the existing Edit constraints window names the
  owning Revolve and its kept turn.
* **Evaluator, kernel, OCCT bridge, topology, solver and storage are
  unchanged.** `git diff 5caaa1c -- crates/ferritecad-eval
  crates/ferritecad-kernel crates/ferritecad-occt
  crates/ferritecad-occt-bridge crates/ferritecad-topology
  crates/ferritecad-sketch-solver` is empty. There is no new dependency,
  capability or migration.

## Results

Recorded after the local runs and the final-head CI; see the PR.

## macOS fixtures and window scenario (for the reviewer's Mac)

Nothing here was run in this cloud container: it has no window system and
no PlaneGCS. The generator uses only the staged CLI.

### Fixture generator

It writes to `$FCAD_27G_DIR`:
* `sector.fcad`: 137.5°, the fractional stepped profile with a bore;
* `bushing.fcad`: a full turn;
* the peer CLI copies of exactly what the window scenario builds:
  * `peer-rigid.fcad`: nine additions in widget order;
  * `peer-tall.fcad`: the wall height replaced by exact UUID;
* their STL and FBX exports.

It also checks that `gui-rigid.fcad` and `gui-tall.fcad`, when present, are
the same model as their peers. Only new constraint UUIDs may differ. STL and
FBX must be byte-identical.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/revolve-profile-constraints-verification.md").read_text(encoding="utf-8")
code = text.split("# FCAD_27G_MAC_FIXTURES\n", 1)[1].split("\n```", 1)[0]
Path("ferrite-27g-fixtures.py").write_text(code, encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/staged/ferritecad FCAD_27G_DIR=/tmp/fcad-27g python3 ferrite-27g-fixtures.py
# … run the window scenario, saving gui-rigid.fcad and gui-tall.fcad there …
FERRITECAD=/path/to/staged/ferritecad FCAD_27G_DIR=/tmp/fcad-27g python3 ferrite-27g-fixtures.py compare
```

```python
# FCAD_27G_MAC_FIXTURES
import json, os, pathlib, sqlite3, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(os.environ["FCAD_27G_DIR"])
out.mkdir(parents=True, exist_ok=True)

def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def inspect(path):
    return run("inspect", path, "--json")["result"]

def constrain(source, edits, target):
    catalog = inspect(source)
    request = target.with_suffix(".json")
    request.write_text(json.dumps({"request_version": 1, **edits}))
    return run("edit-sketch-constraints-copy", source, "--sketch",
               catalog["sketches"][0]["sketch_id"], "--expect-version",
               catalog["content_version"], "--request", request, "-o", target, "--json")

def exports(path):
    for op, suffix in (("export-stl", ".stl"), ("export-fbx", ".fbx")):
        run(op, path, "-o", path.with_suffix(suffix), "--json")

def model(path):
    """Every SQL cell, and the Sketch row as discovery reports it."""
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
    cells = {t: sorted(db.execute(f'SELECT * FROM "{t}"').fetchall(), key=repr)
             for (t,) in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    db.close()
    catalog = inspect(path)["sketches"][0]
    return cells, catalog

if sys.argv[1:] != ["compare"]:
    for name, body in (
        ("sector", {"request_version": 2, "points_mm": [[2.75, -3.25], [7.5, -3.25],
                    [7.5, 2.125], [4.25, 9.5], [2.75, 9.5]], "axis": "sketch_y",
                    "extent": {"kind": "angle", "degrees": 137.5}}),
        ("bushing", {"request_version": 1, "points_mm": [[4.25, -1.5], [10.5, -1.5],
                     [10.5, 13.75], [4.25, 13.75]], "axis": "sketch_y", "angle": "full_turn"})):
        request = out / f"{name}-create.json"
        request.write_text(json.dumps(body))
        target = out / f"{name}.fcad"
        if not target.exists():
            run("create-sketch-revolve", request, "-o", target, "--json")
    sector = out / "sector.fcad"
    curves = [c["curve_id"] for c in
              inspect(sector)["sketches"][0]["constraint_edit"]["curves"]]
    line = lambda i, rule, **k: {"curve_id": curves[i], "rule": rule, **k}
    rigid = [line(0, "horizontal"), line(1, "vertical"), line(3, "horizontal"),
             line(4, "vertical"), line(0, "fixed", at="start", x_mm=2.75, y_mm=-3.25),
             line(0, "distance", distance_mm=4.75), line(1, "distance", distance_mm=5.375),
             line(3, "distance", distance_mm=1.5), line(4, "distance", distance_mm=12.75)]
    peer_rigid = out / "peer-rigid.fcad"
    if not peer_rigid.exists():
        constrain(sector, {"remove": [], "add": rigid}, peer_rigid)
    exports(peer_rigid)
    print("sector and bushing written; peer-rigid published; run the window scenario")
else:
    for gui, peer in (("gui-rigid", "peer-rigid"), ("gui-tall", "peer-tall")):
        gui, peer = out / f"{gui}.fcad", out / f"{peer}.fcad"
        if peer.stem == "peer-tall" and not peer.exists():
            # The same replacement, from the GUI's own rigid copy, so the
            # removed UUID is the one the window removed.
            source = out / "gui-rigid.fcad"
            catalog = inspect(source)["sketches"][0]["constraint_edit"]
            wall = next(c["constraint_id"] for c in catalog["constraints"]
                        if c["rule"]["kind"] == "distance" and c["rule"]["distance"] == 5.375)
            constrain(source, {"remove": [wall], "add": [{"curve_id": catalog["curves"][1]["curve_id"],
                               "rule": "distance", "distance_mm": 7.0}]}, peer)
            exports(peer)
        exports(gui)
        (a, x), (b, y) = model(gui), model(peer)
        assert a.keys() == b.keys()
        for table in a:
            if table != "objects":
                assert a[table] == b[table], table
        assert x["constraint_edit"]["curves"] == y["constraint_edit"]["curves"], "stored inputs"
        rules = lambda c: [r["rule"] for r in c["constraint_edit"]["constraints"]]
        assert rules(x) == rules(y), "the same rules in the same order"
        for suffix in (".stl", ".fbx"):
            assert gui.with_suffix(suffix).read_bytes() == peer.with_suffix(suffix).read_bytes(), suffix
        print(f"{gui.name} == {peer.name}: SQL outside the Sketch payload, rules, STL and FBX bytes")
```

The compare step builds `peer-tall.fcad` from the GUI's own `gui-rigid.fcad`,
so the removed UUID is the one the window removed. The two rigid copies
differ only in new UUIDs. That shows in the Sketch payload/hash and nowhere
else: every other SQL cell, the ordered rules and the export bytes are
compared exactly.

### Window scenario

Run `ferritecad-viewer` built with `--features planegcs` against the staged
PlaneGCS/OCCT, under the usual watchdog.

1. **Open.** Open `sector.fcad`. In **Saved objects**, choose **Edit
   constraints Profile — …**. The window reads "Profile of Revolve …:
   137.5° about the sketch Y axis. The turn and the axis are kept; the
   solved profile must keep its bore clear of the axis." No height is shown.
2. **Dimension.** Add, in this order:
   * **Segment 1** → **Add Horizontal**;
   * **Segment 2** → **Add Vertical**;
   * **Segment 4** → **Add Horizontal**;
   * **Segment 5** → **Add Vertical**;
   * **Segment 1** → **Pin Start**, Fixed X `2.75`, Fixed Y `-3.25` →
     **Add Fixed point**;
   * lengths, each with its own **Add length**: Segment 1 `4.75`,
     Segment 2 `5.375`, Segment 4 `1.5`, Segment 5 `12.75`.

   The pending list shows nine entries.
3. **Undo/Redo.** **Undo** removes the last length; **Redo** restores it.
4. **Refusal.** **Segment 1** → **Add Horizontal** again is refused ("only
   one H/V"). The pending list and Undo are unchanged.
5. **Save Cancel.** **Save constraints copy…** → **Cancel** in the file
   dialog. The draft and its history stay.
6. **Save and Open.** **Save constraints copy…** → `gui-rigid.fcad`. The
   viewer opens the copy asynchronously. The Body looks unchanged, because
   the rigid dimensions equal the stored coordinates.
7. **Replace length.** On `gui-rigid.fcad`, **Edit constraints** →
   **Segment 2**, `7` → **Replace length**. The pending list shows one
   Remove and one length. **Save constraints copy…** → `gui-tall.fcad`. The
   outer wall grows by 1.625 mm and the cone above it moves up.
8. **Refusal and draft recovery.** On `gui-tall.fcad`, **Edit
   constraints**:
   * **Remove** on the Fixed row;
   * **Segment 1** → **Pin Start**, X `-1`, Y `-3.25` → **Add Fixed point**;
   * **Save constraints copy…** → `gui-cross.fcad`.

   The save is refused with the axis reason, nothing is written and the
   draft is still open. **Cancel constraints draft**.
9. **Exports.** Export **STL** and **FBX** of `gui-tall.fcad` into
   `$FCAD_27G_DIR` as `gui-tall.stl` and `gui-tall.fbx`, and the same for
   `gui-rigid`.
10. **Compare.** Quit the viewer normally, then run the generator's
    `compare` step.
