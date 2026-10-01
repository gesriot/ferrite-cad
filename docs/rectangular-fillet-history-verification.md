# §28L — verification record

[The contract and recipe](rectangular-fillet-history.md).

## Base and scope

Base `main` = `97be2825496118198df803b80e3ebf7cbad2af84`, tree
`b67a15d104bb237c87b28a103cebde3805043995` (PR #75, §28K merged). The branch
is `rectangular-fillet-history`; §28K was not redone and `main` was not rolled
back. Post-merge runs of #75 are runs of `main`; the runs of this PR are told
apart from them below.

## Where and how this was run

Local, in the cloud container: Linux, Open CASCADE 8.0.1 and PlaneGCS built from
local inputs — **the local PlaneGCS is not pinned**; the pinned `planegcs pin`
workflow and the three-platform runtime layout are authoritative. macOS is
Apple Silicon (arm64) only. No window, GPU or Unity ran here. One heavy process
at a time, 2–4 jobs; OCCT, Boost and PlaneGCS were not rebuilt.

## What changed

One bounded typed reader of a 0–4 Fillet history (`saved_fillets`) is shared by
discovery, preparation, the transactional writers and the evaluator; the
payload, the archive and the persisted version did not change (none was
needed). See the contract for the policy, naming, SQL allowlists and the wire.

## New tests

- Document (`fillet_radius::tests::history`, 8): reader 0–4; exact bounds over
  24 orders; all 24 orders at policy level; the closing pair; duplicate, fifth,
  branch and foreign predecessor; height, rectangle and constraints under 4;
  every writer re-derives and refuses forgery and stale radii; the evaluator on
  solved Lines.
- CLI (`tests/fillet/history.rs`, 7): four corners in several orders (cold
  reopen, analytic volume `(W*D-(1-pi/4)*sum r^2)*h`, four cylinders by
  name/radius/axis, independent mesh, FBX/STL join, cache Miss/Hit); fourth
  Fillet refusals, races, cancellation and report loss; early/middle/last
  radii; the closing line; height/coordinates/constraints; the solved plate
  decides every radius; discovery without native.
- App (6): third/fourth Fillet, radius, height, Sketch and constraints forms
  name the history and keep Save/Cancel in reach with four Fillets; the worker
  publishes what the CLI publishes.
- Flipped, not weakened: the earlier "third refused" assertions now require
  the third corner to be offered.

## Local results

Full regression (document, jobs, eval, CLI, app, solver info; planegcs, one run):
1144 `passed`, 0 failed, apart from two tests that need a read-only file to be
unreadable and fail as root in this container
(`read_only_permissions_still_dump_when_the_file_can_be_read`,
`validation_really_read_only_permissions`; the same two fail on `main` here).
The twelve recipes 28A–28L ran against this build: all `FCAD_28*_RECIPE_OK`.
The 28L recipe prints
`four=3016.186559/3016.643957 early=… moved=… dimensioned=…`.

### Mutations — local, executed, restored

Each compiled, failed where named and was restored byte for byte, after which
the positive gates passed again:

1. The evaluator judges a Fillet against its predecessor only (not every
   earlier Fillet).
2. Creation judges the new radius against the last Fillet only.
3. The radius edit takes the nearest neighbour only.
4. Inherited origins lost in topology (origins of origins).
5. The origin names of earlier Fillets dropped.
6. The stale-radius guard of the writer disabled.

### Compatibility with the reader on `main`

`main` built with the same solver opens a 3- and a 4-Fillet document, lists
every row, validates it, and refuses rebuild, export and edits with the typed
`unsupported: this build rounds at most two corners of one plate, and this
document holds N Fillets`; it writes nothing and never builds a partial Body.
Old commands, request versions and JSON v1 fields/types/codes for 0, 1 and 2
Fillets are unchanged; a scalar was not turned into an array or null.

## CI

Implementation code head `4011211fccfb7888f36dc69c3d4c704b14339601`; later review changes are recorded below. Base `main` 97be2825496118198df803b80e3ebf7cbad2af84 (PR #75, §28K); its post-merge runs are separate from the runs below.

The first push, `06972e8`, failed in the Ubuntu and macOS stub step: the flipped §28G no-kernel check called the `native()` helper, which prints `skipped:` and so trips the exact-name no-skip gate, and the renamed document gate (`…a_third_is_refused` → `…a_third_is_offered`) found 0 tests. Both were real defects of the PR, fixed in `4011211`; before pushing, all twelve fillet steps of the stub job were run locally in the stub build (65 `test … ok`, 12 `FCAD_28*_RECIPE_NO_KERNEL`, exit 0).

All runs on `4011211` finished green:

- CI, run 36842858820 (`lint`, `test` on ubuntu, macos and windows, `sbom`, `notices`, `supply-chain`): https://github.com/gesriot/ferrite-cad/actions/runs/36842858820
- combined runtime layout, run 36842853292 (linux, macos, windows, comparison): https://github.com/gesriot/ferrite-cad/actions/runs/36842853292
- planegcs pin: its paths were not touched by the second push; the run on `06972e8` (36841756742) was green, and `06972e8..4011211` changed only a test file and the two workflows.

Limit of this record: the full job logs could not be downloaded here (the log archive host answered 403 to the session's egress policy), so the per-step `test … ok` and marker counts were **not** recounted from saved logs. What holds instead is that each of these steps fails its job unless the exact test name printed `ok` without `skipped:` (loops over named gates) and each marker (`FCAD_28L_RECIPE_OK`, `FCAD_28L_RECIPE_NO_SOLVER`, `FCAD_28L_RECIPE_NO_KERNEL`, `FCAD_FILLET_FOUR_UFBX_EXECUTED`) is grepped; a green job therefore executed them. That is an inference, not a count.

## Limits

Out of scope and unchanged: arbitrary edges, Chamfer, Cut+Fillet, new
constraints, in-place Save, live preview. Milestone 5C is not declared
complete. The local PlaneGCS is not the pinned one. No window ran here.
The memory exhaustion seen in earlier macOS runs is still unexplained.

## macOS fixture and window scenario (for the user's Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised with a second
CLI invocation standing in for the window, which says nothing about the window:
`FCAD_28L_GUI_COMPARE_OK cells=1554 triangles=272`; without the window's files
it stopped with `FCAD_28L_GUI_COMPARE_MISSING …` and created nothing; a
stand-in height of 7 mm instead of 9.5 mm, a changed byte in a refs row and a
changed hash outside the allowlist were each refused.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon Mac, with a fresh arm64 bundle, and do not set
`FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `two.fcad` — a free plate, 37.5 × 12.25 × 6.75 mm, drawn clockwise
from the upper right, offset and fractional — with Fillet 1 at (33, 3.25) and
Fillet 2 at (-4.5, 15.5), each r 6.12 mm, and `facts.json`.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/rectangular-fillet-history-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28L_GUI_FIXTURE\n", "ferrite-28l-fixture.py"),
                   ("# FCAD_28L_GUI_COMPARE\n", "ferrite-28l-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_28L_DIR="$PWD/fillet-four-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28l-fixture.py "$FCAD_28L_DIR"
```

```python
# FCAD_28L_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-four-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# A free plate, clockwise from the upper right, offset and fractional, no
# constraint. Corners C0..C3 are the stored starts of Segments 1..4: C0 (33,
# 15.5), C1 (33, 3.25), C2 (-4.5, 3.25), C3 (-4.5, 15.5). Segments 1 and 3 are
# the 12.25 mm sides (C0-C1 and C2-C3 share them); Segments 2 and 4 the 37.5 mm
# ones.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
H = 6.75
# Two opposite corners are rounded at 6.12 mm each (a corner takes up to 6.125):
# the third corner C2 then shares the short Segment 3 with Fillet 2, so its
# radius may be at most 12.25 - 6.12 - 0.01 = 6.12 mm; the fourth corner C0
# shares the short Segment 1 with Fillet 1, which closes the pair.
F1_AT, F2_AT, R1, R2 = [33.0, 3.25], [-4.5, 15.5], 6.12, 6.12
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H, "points_mm": PLATE}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
def fillet(source, corner, radius, dest):
    catalog = run("inspect", source, "--json")["result"]
    chosen = next(c for c in catalog["bodies"][0]["fillet_edge"]["target"]["candidates"]
                  if c["stored_corner_mm"] == corner)
    request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": radius}))
    run("fillet-edge-copy", source, "--body", catalog["bodies"][0]["body_id"], "--expect-version",
        catalog["content_version"], "--request", request, "-o", dest, "--json")
once = out / "once.fcad"
fillet(plate, F1_AT, R1, once)
source = out / "two.fcad"
fillet(once, F2_AT, R2, source)
for p in (request, plate, once):
    p.unlink()
catalog = run("inspect", source, "--json")["result"]
(base,) = catalog["features"]
fillets = catalog["fillets"]
assert [f["history_index"] for f in fillets] == [1, 2] and base["fillet_history"]["count"] == 2, fillets
(sketch,) = catalog["sketches"]
facts = {
    "body_id": catalog["bodies"][0]["body_id"], "base_feature_id": base["feature_id"],
    "sketch_id": sketch["sketch_id"], "fillet_ids": [f["feature_id"] for f in fillets],
    "vertices": [{"curve_id": v["curve_id"], "start_mm": v["start_mm"]} for v in sketch["vertices"]],
    "radii_mm": [R1, R2], "saved_corners_mm": [F1_AT, F2_AT], "height_mm": H,
    "content_version": catalog["content_version"],
    # The window's five publications, and the numbers each is judged by.
    "third": {"corner_mm": [-4.5, 3.25], "radius_mm": 4.5, "refused_radius_mm": 6.121},
    "fourth": {"corner_mm": [33.0, 15.5], "radius_mm": 3.5, "refused_radius_mm": 6.121},
    "early": {"fillet": "first", "radius_mm": 2.5},
    "taller": {"height_mm": 9.5},
    "rect_mm": [1.5, -2.25, 30.0, 12.75],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_28L_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_28L_DIR/../watch-28l.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

If system memory pressure aborts or stops the run, it does not count; start
again from a fresh fixture directory. **Do not address the viewer after Quit
through the automation**: it may relaunch the app outside the watchdog. If it
was relaunched, quit that instance too and read only the original PID's exit
and watchdog record. Type numbers exactly as written.

1. **Open** `$FCAD_28L_DIR/two.fcad` (asynchronously). The feature list shows
   Extrude -> Fillet 1 -> Fillet 2; **Fillet edge of …** is offered and lists
   exactly the two unrounded corners (-4.5, 3.25) and (33, 15.5).
2. **Third.** Choose the corner (-4.5, 3.25), radius `4.5` → **Apply fillet** → **Save fillet copy…** →
   `$FCAD_28L_DIR/gui-third.fcad`; it opens asynchronously. The history names
   Fillet 1, 2 and 3; only (33, 15.5) is offered.
3. **Fourth.** Choose (33, 15.5), radius `3.5` → **Apply fillet** → **Save fillet copy…** →
   `$FCAD_28L_DIR/gui-fourth.fcad`; it opens. No corner is left: the form says
   every corner is already rounded.
4. **Refusal by the second neighbour.** In `gui-third.fcad`, reopened from
   disk, choose (33, 15.5), radius `6.121` → **Apply fillet**: the
   refusal names Fillet 1 (the closing pair), not only the nearer neighbour.
   The draft is kept; Save is unavailable and no `gui-refused.fcad` is published.
5. **Recovery and Save Cancel.** Type `3.5`; **Apply fillet** →
   **Save fillet copy…** → **Cancel** in the file dialog: nothing starts
   and the draft stays. Then dismiss the draft without saving. The Fillet
   form has no whole-request Undo/Redo; exercise that in the Sketch step.
6. **Early radius.** In `gui-fourth.fcad` open **Edit radius** on Fillet 1 and
   type `2.5`; the form names Fillet 1 and every neighbour; **Apply radius** →
   **Save radius copy…** →
   `$FCAD_28L_DIR/gui-early.fcad`.
7. **Base height.** **Edit height** `9.5` → **Save** →
   `$FCAD_28L_DIR/gui-height.fcad`.
8. **Base size.** **Edit Sketch**, drag/type the four corners to the rectangle
   x `1.5`..`31.5`, y `-2.25`..`10.5` (x₀ 1.5, y₀ -2.25, 30 × 12.75);
   use the Sketch editor’s Undo/Redo and confirm the coordinates return →
   **Save** → `$FCAD_28L_DIR/gui-rect.fcad`; it opens.
9. **Exports.** Export `gui-rect.stl` and `gui-rect.fbx` of `gui-rect.fcad`
   into `$FCAD_28L_DIR`, at the default tessellation.
10. With four Fillets the history and forms must scroll inside a bounded
    height with **Save**/**Cancel** reachable.
11. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28l-compare.py "$FCAD_28L_DIR"
```

It first requires `gui-third.fcad`, `gui-fourth.fcad`, `gui-early.fcad`,
`gui-height.fcad`, `gui-rect.fcad`, `gui-rect.stl` and `gui-rect.fbx` (and
never creates them), refuses a `gui-refused.fcad`, checks the source's SHA-256,
each window edit's allowlist, makes the CLI peers (only if absent), maps only
the new Fillet UUID and its new names, compares every SQL cell except the stamp
(a payload hash only where no mapped UUID is inside the payload, and the
payload itself always), requires byte-equal STL/FBX, then reads the window's
STL itself: closed, oriented, the plate's bounds and the analytic volume.
It prints `FCAD_28L_GUI_COMPARE_OK cells=N triangles=M`.

```python
# FCAD_28L_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-third.fcad", "gui-fourth.fcad", "gui-early.fcad", "gui-height.fcad", "gui-rect.fcad",
       "gui-rect.stl", "gui-rect.fbx")
missing = [n for n in GUI if not (out / n).is_file()]
if missing:
    sys.exit(f"FCAD_28L_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first")
assert not (out / "gui-refused.fcad").exists(), "a refused Save published something"
source = out / "two.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
F1, F2 = facts["fillet_ids"]
SKETCH, BASE, BODY = facts["sketch_id"], facts["base_feature_id"], facts["body_id"]
H = facts["height_mm"]
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
def uuid_bytes(text):
    return bytes.fromhex(text.replace("-", ""))
def allowlist_row(before, after, row, columns=("payload", "payload_hash")):
    """Exactly one object row's payload and hash and the stamp moved."""
    rid = uuid_bytes(row)
    a, b = tables(before), tables(after)
    assert a.keys() == b.keys()
    for t in a:
        (ac, arows), (bc, brows) = a[t], b[t]
        assert ac == bc, t
        assert len(arows) == len(brows), t
        if t == "objects":
            k = ac.index("id")
            arows, brows = sorted(arows, key=lambda r: r[k]), sorted(brows, key=lambda r: r[k])
        for x, y in zip(arows, brows):
            for c, u, v in zip(ac, x, y):
                assert u == v or (t == "objects" and c in columns and x[ac.index("id")] == rid) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c} moved"
def allowlist_fillet(before, after, names, new_capabilities):
    """One new Fillet row, the Body row's payload and hash, `names` new names,
    two new dependency edges and the old tip edge gone, the stamp; nothing
    else moved."""
    bid = uuid_bytes(BODY)
    a, b = tables(before), tables(after)
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
                    assert u == v or (row[k] == bid and c in ("payload", "payload_hash")), f"objects.{c} moved"
        elif t in ("deps", "topology_refs"):
            kept = [r for r in arows if r in brows]
            lost = [r for r in arows if r not in brows]
            assert all(t == "deps" and bid in r for r in lost), (t, lost)
            assert len(brows) - len(kept) == (2 if t == "deps" else names), t
        elif t == "capabilities":
            assert set(arows) <= set(brows), "a capability changed"
            assert {r[0] for r in set(brows) - set(arows)} <= new_capabilities, "a new capability"
        elif t == "meta":
            for x, y in zip(arows, brows):
                assert all(u == v or c == "modified_at" for c, u, v in zip(ac, x, y)), "meta"
        else:
            assert arows == brows, t
def peer_fillet(src, corner, radius, dest):
    if dest.exists():
        return
    catalog = run("inspect", src, "--json")["result"]
    chosen = next(c for c in catalog["bodies"][0]["fillet_edge"]["target"]["candidates"]
                  if c["stored_corner_mm"] == corner)
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": radius}))
    run("fillet-edge-copy", src, "--body", BODY, "--expect-version", catalog["content_version"],
        "--request", request, "-o", dest, "--json")
    request.unlink()
def peer_radius(src, feature, radius, dest):
    if dest.exists():
        return
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "radius_mm": radius}))
    run("edit-fillet-radius", src, "--feature", feature, "--expect-version",
        run("inspect", src, "--json")["result"]["content_version"], "--request", request, "-o", dest, "--json")
    request.unlink()
def peer_height(src, height, dest):
    if dest.exists():
        return
    run("edit-extrude", src, "--feature", BASE, "--distance-mm", str(height), "--expect-version",
        run("inspect", src, "--json")["result"]["content_version"], "-o", dest, "--json")
def at(rect, p):
    x0, y0, w, d = rect
    sx = max(v["start_mm"][0] for v in facts["vertices"])
    sy = max(v["start_mm"][1] for v in facts["vertices"])
    return [x0 + w if p[0] == sx else x0, y0 + d if p[1] == sy else y0]
def peer_rect(src, rect, dest):
    if dest.exists():
        return
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "vertices": [
        {"curve_id": v["curve_id"], "start_mm": at(rect, v["start_mm"])} for v in facts["vertices"]]}))
    run("edit-sketch-copy", src, "--sketch", SKETCH, "--expect-version",
        run("inspect", src, "--json")["result"]["content_version"], "--request", request, "-o", dest, "--json")
    request.unlink()
# The window's chain, and the same five edits through the shipped CLI.
g = {n: out / f"gui-{n}.fcad" for n in ("third", "fourth", "early", "height", "rect")}
p = {n: out / f"peer-{n}.fcad" for n in g}
peer_fillet(source, facts["third"]["corner_mm"], facts["third"]["radius_mm"], p["third"])
peer_fillet(p["third"], facts["fourth"]["corner_mm"], facts["fourth"]["radius_mm"], p["fourth"])
peer_radius(p["fourth"], F1, facts["early"]["radius_mm"], p["early"])
peer_height(p["early"], facts["taller"]["height_mm"], p["height"])
peer_rect(p["height"], facts["rect_mm"], p["rect"])
# Allowlists on the window's own chain: each step moves exactly what it may.
allowlist_fillet(source, g["third"], 9, set())
allowlist_fillet(g["third"], g["fourth"], 10, set())
allowlist_row(g["fourth"], g["early"], F1)
allowlist_row(g["early"], g["height"], BASE)
allowlist_row(g["height"], g["rect"], SKETCH)
# What the window's documents say: the history in order, the radii as typed.
def history(path, count, radii):
    result = run("inspect", path, "--json")["result"]
    rows = result["fillets"]
    assert [r["history_index"] for r in rows] == list(range(1, count + 1)), rows
    assert [r["radius_mm"] for r in rows] == radii, rows
    for r in rows:
        assert r["radius_edit"]["available"] is True and len(r["radius_edit"]["neighbours"]) == count - 1, r
    base = result["features"][0]
    assert base["fillet_history"]["count"] == count and (base["fillet_base"] is not None) == (count <= 2)
    assert run("validate", path, "--json")["result"]["valid"] is True
    n = len(tables(path)["topology_refs"][1])
    assert f"{n} of {n} stored references resolved" in run("rebuild", path, "--cold")
    return rows
R1, R2 = facts["radii_mm"]
R3, R4, E1 = facts["third"]["radius_mm"], facts["fourth"]["radius_mm"], facts["early"]["radius_mm"]
rows3 = history(g["third"], 3, [R1, R2, R3])
rows4 = history(g["fourth"], 4, [R1, R2, R3, R4])
history(g["early"], 4, [E1, R2, R3, R4])
history(g["height"], 4, [E1, R2, R3, R4])
history(g["rect"], 4, [E1, R2, R3, R4])
assert rows3[0]["feature_id"] == F1 and rows3[1]["feature_id"] == F2
assert [r["feature_id"] for r in rows4[:3]] == [r["feature_id"] for r in rows3], "an earlier Fillet changed UUID"
# Every cell of every window document equals the CLI's, with only the UUIDs
# the operation minted mapped: the new Fillet, then each of its new names by
# what it means. Payloads, hashes (where no mapped UUID is inside), schema
# versions, references and dependencies all take part.
mapping = {}
def apply(cell):
    if isinstance(cell, bytes):
        for a, b in mapping.items():
            cell = cell.replace(a, b)
    return cell
def mint(gui, peer, before_gui, before_peer):
    """Match what `gui` minted on top of `before_gui` with what `peer` minted."""
    def new(path, before):
        a, b = tables(before), tables(path)
        old_ids = {r[a["objects"][0].index("id")] for r in a["objects"][1]}
        objs = [r for r in b["objects"][1] if r[b["objects"][0].index("id")] not in old_ids]
        assert len(objs) == 1, "one new object"
        fid = objs[0][b["objects"][0].index("id")]
        cols = b["topology_refs"][0]
        old_refs = {r[cols.index("id")] for r in a["topology_refs"][1]}
        refs = [r for r in b["topology_refs"][1] if r[cols.index("id")] not in old_refs]
        return fid, refs, cols
    gf, grefs, cols = new(gui, before_gui)
    pf, prefs, _ = new(peer, before_peer)
    assert len(grefs) == len(prefs)
    mapping[gf] = pf
    left = {tuple((c, apply(r[i])) for i, c in enumerate(cols) if c not in ("id","payload_hash")): r[cols.index("id")] for r in grefs}
    right = {tuple((c, r[i]) for i, c in enumerate(cols) if c not in ("id","payload_hash")): r[cols.index("id")] for r in prefs}
    assert left.keys() == right.keys(), "the new names differ in meaning"
    for k, u in left.items():
        mapping[u] = right[k]
def same(gui, peer):
    left, right = tables(gui), tables(peer)
    assert left.keys() == right.keys()
    cells = 0
    for t in left:
        (cols, lrows), (rc, rrows) = left[t], right[t]
        assert cols == rc, t
        assert len(lrows) == len(rrows), t
        mapped = sorted(([apply(c) for c in r] for r in lrows), key=repr)
        theirs = sorted((list(r) for r in rrows), key=repr)
        for x, y, raw in zip(mapped, theirs, sorted(lrows, key=lambda r: repr([apply(c) for c in r]))):
            for k, c in enumerate(cols):
                if t == "meta" and c == "modified_at":
                    continue
                if c == "payload_hash" and "payload" in cols:
                    # A hash covers the payload it hashes: equal wherever no
                    # minted UUID is inside, and the payload itself is equal.
                    payload = cols.index("payload")
                    assert x[payload] == y[payload], f"{t}.payload"
                    if apply(raw[payload]) == raw[payload] and apply(raw[cols.index("id")]) == raw[cols.index("id")]:
                        assert x[k] == y[k], f"{t}.payload_hash"
                    cells += 1
                    continue
                assert x[k] == y[k], f"{t}.{c}"
                cells += 1
    return cells
mint(g["third"], p["third"], source, source)
cells = same(g["third"], p["third"])
mint(g["fourth"], p["fourth"], g["third"], p["third"])
cells += same(g["fourth"], p["fourth"])
for n in ("early", "height", "rect"):
    cells += same(g[n], p[n])
# The exports of the window's last document are the CLI's, byte for byte, and
# the mesh is read here, independently of the B-Rep.
for fmt in ("stl", "fbx"):
    if not (out / f"peer-rect.{fmt}").exists():
        run(f"export-{fmt}", p["rect"], "-o", out / f"peer-rect.{fmt}", "--json")
    assert (out / f"gui-rect.{fmt}").read_bytes() == (out / f"peer-rect.{fmt}").read_bytes(), fmt
data = (out / "gui-rect.stl").read_bytes()
(count,) = struct.unpack_from("<I", data, 80)
assert len(data) == 84 + 50 * count
tri = [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)] for i in range(count)]
pts = [q for t in tri for q in t]
q = lambda v: tuple(round(x * 1e4) for x in v)
directed = {}
for a, b, c in tri:
    for u, v in ((a, b), (b, c), (c, a)):
        directed[(q(u), q(v))] = directed.get((q(u), q(v)), 0) + 1
assert all(n == 1 for n in directed.values()), "not one oriented surface"
assert all((v, u) in directed for u, v in directed), "the mesh is open"
six = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2])
          + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in tri)
x0, y0, w, d = facts["rect_mm"]
h = facts["taller"]["height_mm"]
xs, ys, zs = ([pt[k] for pt in pts] for k in range(3))
for got, want in ((min(xs), x0), (max(xs), x0 + w), (min(ys), y0), (max(ys), y0 + d), (min(zs), 0.0), (max(zs), h)):
    assert abs(got - want) < 1e-4, (got, want)
sx = max(v["start_mm"][0] for v in facts["vertices"])
sy = max(v["start_mm"][1] for v in facts["vertices"])
corner = lambda p_: [x0 + w if p_[0] == sx else x0, y0 + d if p_[1] == sy else y0]
rounded = [(corner(facts["saved_corners_mm"][0]), E1), (corner(facts["saved_corners_mm"][1]), R2),
           (corner(facts["third"]["corner_mm"]), R3), (corner(facts["fourth"]["corner_mm"]), R4)]
exact = (w * d - (1 - math.pi / 4) * sum(r * r for _, r in rounded)) * h
slack = sum(math.pi / 2 * r * 0.01 * h for _, r in rounded)
assert exact - slack - 1e-3 <= six / 6 <= exact + 1e-3, (six / 6, exact)
for c in ([x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]):
    near = any(abs(pt[0] - c[0]) < 1e-4 and abs(pt[1] - c[1]) < 1e-4 for pt in pts)
    assert near == all(c != at_ for at_, _ in rounded), c
for (cx, cy), r in rounded:
    ax = cx + (r if abs(cx - x0) < 1e-9 else -r)
    ay = cy + (r if abs(cy - y0) < 1e-9 else -r)
    wall = [pt for pt in pts if abs(pt[0] - cx) < r - 1e-4 and abs(pt[1] - cy) < r - 1e-4]
    assert len(wall) >= 4, (cx, cy)
    assert all(abs(math.hypot(pt[0] - ax, pt[1] - ay) - r) < 1e-3 for pt in wall), (cx, cy, r)
print("FCAD_28L_GUI_COMPARE_OK", f"cells={cells}", f"triangles={count}")
```


## Independent macOS review — 2026-10-01

Reviewed on Apple Silicon at `eed6745a733689cd5c21a4df6b88fa8bb680b56e`
(the review commit corrects stale reader references, not executable code).
Pinned OCCT 8.0.1 and PlaneGCS, the reused release target, one local heavy
process at a time. No native dependency was rebuilt. Fmt and workspace
clippy, all targets/features with `-D warnings`, passed.

Local tests: 660 actually executed, zero failures. The harness counted 664:
four explicitly reported no-solver-only cases were N/A in this native build;
one old timing benchmark was ignored. This covers document/jobs/eval, all
Fillet CLI tests and the app Fillet/constraints/Sketch/height workers.
The extracted 28L recipe passed with the fresh bundled CLI.

The full original runtime logs (36842853292) were downloaded and counted:
21 distinct new named tests, 22 executions per platform (the height widget
also runs in the earlier edit group), on Linux/macOS/Windows. Native recipe,
mixed/no-solver recipe and `FCAD_FILLET_FOUR_UFBX_EXECUTED` each occurred on all
three. This supplies the direct log audit unavailable in the cloud session.
Review runtime [36878507925](https://github.com/gesriot/ferrite-cad/actions/runs/36878507925)
and the final-head ordinary CI are separately required before merge.

A fresh ad-hoc signed arm64 bundle passed closure and solver loader probes
without DYLD variables or loader-failure overrides. The initial locked-screen
attempt was stopped with SIGTERM and does not count as window verification.
After unlock, owned PID 87805 ran under the 1536 MiB watchdog:

- Opened the two-Fillet fixture; created the third at (-4.5, 3.25), r4.5.
- Before the fourth publication, r6.121 at (33, 15.5) refused with the first
  Fillet UUID and the closing shared Line. The draft survived. Applied r3.5,
  cancelled the native Save dialog, then saved the retained draft as the
  fourth Fillet; creation was then disabled because all corners were rounded.
- Changed the first radius to 2.5, base height to 9.5, and stored clockwise
  Sketch coordinates to x 1.5..31.5, y -2.25..10.5. Sketch Undo/Redo restored
  the exact final coordinates. All five copies published and opened
  asynchronously; STL and FBX were exported from the last accepted scene.
- The four-Fillet constraints form exposed its history and reachable
  controls. Added a pending Vertical, opened and cancelled Save, observed
  the retained request, then explicitly dismissed it without publication.

The original recipe incorrectly promised whole-request Undo/Redo in the
Fillet form and referred to a Save before Apply. The contract and scenario
now describe the actual forms: Fillet/radius use Apply, while Sketch and
constraints have request history. No Fillet request-history feature was
added by this review.

The strict comparator ran on the real window files: 1554 cells; the source
was byte-identical, SQL allowlists passed and GUI/CLI STL and FBX were
byte-identical. An independently generated peer with height 7 instead of 9.5
was rejected at `objects.payload`. STL: 272 triangles, 13684 bytes, closed and
consistently oriented, bounds [1.5,31.5] × [-2.25,10.5] × [0,9.5] mm; signed
mesh volume 3477.608295 mm³ versus analytic 3478.390760 mm³, within the stated
facet band. FBX: 50024 bytes; pinned ufbx 0.23.0 strict, six checks and zero
failures, oriented triangle join with the independently parsed STL 272/272.

Peak footprint 204.282 MiB; pressure normal throughout, swap unchanged
(647299072 bytes), viewer exit 0, watchdog not triggered. After Quit only the
PID/log was inspected; no automation call relaunched the viewer. The earlier
OOM remains unexplained. Evidence, temporary models and screenshots were
kept outside the checkout in `ferrite-pr76-review`; no fixture was committed.
