# §29A — verification record

[The contract and recipe](rectangular-corner-chamfer.md).

## Base and scope

Base `main` = `ea4602e4197ca1abbf3f3f57b373282395ae7304`, tree
`cd2d70b3ae9e9f5ec153460e268a74f24b96a873` (PR #76, §28L merged). The branch is
`rectangular-corner-chamfer`; §28L was not redone and `main` was not rolled
back. Runs of #76 on `main` after the merge are told apart from this PR's runs.
Milestone 5C is **not** declared complete.

## Independent macOS review (2026-10-01)

The review reproduced a defect in the new planar-face diagnostic: the lower
cap of an annular extrusion returned an upward normal. Its centre of mass is
inside the bore, so an inside/outside probe there cannot determine the face's
orientation. The new regression failed on an executed assertion before the
fix. `face_plane` now finds the same face occurrence in its solid by identity,
uses the surface U/V cross product (including indirect placements), and applies
the occurrence's orientation. Both extrusion directions and named archive
round-trips are covered. This exact regression joins the existing runtime gate
on all three platforms. String allocation in the common Fillet/Chamfer bridge
entry is also inside its exception guard.

Local Apple Silicon review used the existing pinned OCCT/PlaneGCS and release
target, with sequential builds. The initial affected matrix executed 1036 tests;
four explicit no-solver N/A cases and one old ignored benchmark are separate.
After the fix, all three OCCT Chamfer tests, eight CLI Chamfer tests, four app
Chamfer tests, workspace clippy (all targets/features, warnings denied), and
fmt passed. Deliberate panics caught by the CLI measurement test are its
negative controls, not failed tests. Native libraries were not rebuilt.
Window smoke and remote CI for this review commit are still pending here.

## Where and how this was run

Local, in the cloud container: Linux, Open CASCADE 8.0.1 and PlaneGCS built from
local inputs — **the local PlaneGCS is not the pinned one**; the pinned
`planegcs pin` workflow and the three-platform runtime layout are
authoritative. macOS is Apple Silicon (arm64) only. No window, GPU or Unity ran
here. One heavy process at a time, 2–4 jobs; OCCT, Boost and PlaneGCS were not
rebuilt.

## What changed

One new feature, `feature.chamfer` (payload v1, capability
`feature.chamfer.v1`, role `EdgeChamferFace`, topology tag 21, cache keys
`kernel.chamfer_edge` / `eval.chamfer.named`), over the forward Blind/NewBody
Extrude of one rectangular plate, through the kernel trait, a real
`BRepFilletAPI_MakeChamfer` call in the shim and the same shared copy job as
the other edits. The archive FORMAT_VERSION and the SQLite schema did not
change (nothing needed them). The distance policy is one expression used by
discovery, preparation, the evaluator and both writers:
`min 0.001 mm`, `max = min(adjacent lengths) − 0.01 mm`, the maximum accepted
and the next representable value refused. Every older editor either refuses the
Chamfer by naming it (`refuse_chamfered`, called first by `refuse_filleted`)
or is untouched.

## New tests

- Document (`chamfer::tests`, 8): payload round-trip and refusals; exact
  policy at its bounds; every corner in both windings; the distance edit and
  forged/stale writers; creation re-derivation and forgery; every other editor
  refuses by name; a dimensioned plate refused with the constraint named; the
  evaluator judges the built Lines.
- Kernel (3), topology (2), OCCT (`chamfer_edge_occt`, 2): request/result
  checks, cache key by edge meaning and distance bits, own role and byte form,
  the selected-edge chamfer through the shim with every tracked face accounted
  for, and a chamfer that cannot be what it claims refused with nothing kept.
- CLI (`tests/fillet/chamfer.rs`, 8): every corner in three drawing orders
  (B-Rep volume `(W·D − d²/2)·h`, the plane's normal, position and area, the
  corner it is at, independent STL parse, FBX/STL join, cache Miss/Hit);
  the corner and the volume measured apart; exact bounds and cache by
  distance; the edit keeps every UUID and moves one row; refusals, races,
  cancellation and report loss; every other editor refuses by name; the
  evaluator refuses a forged Chamfer; discovery and the strict protocol
  without native.
- App (4): the forms list corners and refuse like the document, keep the draft
  and have whole-request Undo/Redo; the worker publishes what the CLI publishes.

## Local results

Full regression (document, jobs, eval, CLI, app, solver info; planegcs): all
passed apart from two tests that need an unreadable read-only file and fail as
root in this container (`read_only_permissions_still_dump_when_the_file_can_be_read`,
`validation_really_read_only_permissions`; they fail on `main` here too).
App: 376 passed. `cargo fmt --check` and workspace clippy
(`--all-targets --features planegcs -- -D warnings`) are clean.
The twelve recipes 28A–28L ran against this build: all `FCAD_28*_RECIPE_OK`.
Stub build (no Open CASCADE): the discovery/protocol, widget, document, kernel
and topology gates passed and the recipe printed `FCAD_29A_RECIPE_NO_KERNEL`.
OCCT without the solver (release, `--no-default-features`): the kernel-free
gate and two native gates passed and the recipe printed `FCAD_29A_RECIPE_OK`.
The extracted recipe prints `FCAD_29A_RECIPE_OK` with a kernel and
`FCAD_29A_RECIPE_NO_KERNEL` without one.
The FBX artifacts the CI loop reads (six Chamfer FBX/STL pairs) were produced
by the CLI tests and read by pinned ufbx (`FCAD_PRODUCTION_FBX_UFBX_EXECUTED
checks=6 failures=0`) and joined with the STL (`FCAD_STL_FBX_MATCH triangles=16`).

### Mutations — local, executed, restored

Each compiled, failed on an executed assertion and was restored byte for byte;
the positive gates then passed again:

1. The chosen corner mirrored to another corner: five CLI tests fail (the
   plane's position, the vertex columns).
2. The cache key ignores the distance: the distance-edit test fails
   (`Hit` where `Miss` is required).
3. The writer skips the distance bound: the document test fails
   (`write_chamfer_distance(&forged).is_err()`); the CLI gate stays green
   because the evaluator is the second line — recorded as layered defence.
4. The Chamfer face filed under the Fillet role: six CLI tests fail.

### Compatibility with the reader on `main`

`main` built with the same solver was run against a Chamfer document:
`inspect` lists every row and shows no Chamfer field; `validate`, `rebuild
--cold`, `export-stl`, `export-fbx` and `edit-extrude` exit 2 with
`unsupported … feature.chamfer.v1`; `fillet-edge-copy` refuses the unknown
field. The file's SHA-256 is unchanged; no partial plate is ever shown
(`FCAD_29A_COMPAT_OK`).

## CI

Pull request [#77](https://github.com/gesriot/ferrite-cad/pull/77), code head
`292ed48e06fdb9b03355e8b24ebf9e7904cc8ee8` (later commits change only this
record). These are the PR's runs, not post-merge runs of `main`.

- Ordinary CI [36918611868](https://github.com/gesriot/ferrite-cad/actions/runs/36918611868): success (includes the stub step with the Chamfer discovery/protocol, widget, document, kernel and topology gates and the `FCAD_29A_RECIPE_NO_KERNEL` recipe).
- Combined runtime layout [36918605041](https://github.com/gesriot/ferrite-cad/actions/runs/36918605041): success on Linux, macOS and Windows plus the three-platform comparison. Its steps ran: the OCCT-without-solver Chamfer recipe, the native Chamfer step (8 CLI, 2 OCCT, 3 kernel, 8 document, 2 topology, 4 app gates and the recipe) and the pinned-ufbx step with `FCAD_CHAMFER_UFBX_EXECUTED`.
- planegcs pin [36906833039](https://github.com/gesriot/ferrite-cad/actions/runs/36906833039): success on `1c78bb7`; later commits changed only workflows, a test file and docs.
- Two earlier pushes failed and were fixed, not retried: a runtime step over GitHub's 21 000-character limit (the workflow failed in 0 s with no jobs), then a gate that prints deliberate panics before its `ok`, then a `--features planegcs` the document crate does not have.

The full job logs were not downloaded (only the step conclusions and the tails of the failing logs were read), so per-step test counts are an inference from the gates, as stated under Limits.

## Limits

Out of scope and refused: user dimensional constraints on the plate, arbitrary
or cap edges, a second Chamfer, mixed Fillet/Chamfer chains, unequal distances
and angle, in-place Save and live preview. The local PlaneGCS is not the
pinned one. No window ran here. The memory exhaustion seen in earlier macOS
runs is still unexplained. The full CI job logs could not be downloaded here if
the log archive host answers 403; then per-step counts are an inference from
the gates (each fails its job unless its exact test printed `ok` without
`skipped:` and each marker was grepped).

## macOS fixture and window scenario (for the user's Mac)

Nothing here was run in this container, which has no window system. The
generator and comparator use only the CLI. They were exercised with a second CLI
invocation standing in for the window, which says nothing about the window:
`FCAD_29A_GUI_COMPARE_OK cells=256 triangles=16`; without the window's files
it stopped with `FCAD_29A_GUI_COMPARE_MISSING …` and created nothing; a
stand-in Chamfer at another corner, one at d 3 instead of 2.375, a changed
byte in a refs row and a changed object hash were each refused.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon Mac, with a fresh arm64 bundle, and do not set
`FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `plate.fcad` — a free plate, 37.5 × 12.25 × 6.75 mm, drawn clockwise
from the upper right, offset and fractional, no Fillet or Chamfer — and
`facts.json`.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/rectangular-corner-chamfer-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_29A_GUI_FIXTURE\n", "ferrite-29a-fixture.py"),
                   ("# FCAD_29A_GUI_COMPARE\n", "ferrite-29a-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_29A_DIR="$PWD/chamfer-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-29a-fixture.py "$FCAD_29A_DIR"
```

```python
# FCAD_29A_GUI_FIXTURE
import hashlib, json, math, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "chamfer-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
# A free plate, clockwise from the upper right, offset and fractional, no
# constraint and no Fillet or Chamfer: corners C0 (33, 15.5), C1 (33, 3.25),
# C2 (-4.5, 3.25), C3 (-4.5, 15.5); sides 12.25 mm (C0-C1, C2-C3) and 37.5 mm.
PLATE = [[33.0, 15.5], [33.0, 3.25], [-4.5, 3.25], [-4.5, 15.5]]
H = 6.75
CORNER, D1, D2 = [33.0, 3.25], 2.375, 4.5
MAX = min(12.25, 37.5) - 0.01
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H, "points_mm": PLATE}))
source = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", source, "--json")
request.unlink()
catalog = run("inspect", source, "--json")["result"]
(base,) = catalog["features"]
(sketch,) = catalog["sketches"]
row = catalog["bodies"][0]["chamfer_edge"]
assert row["available"] is True and catalog["chamfers"] == [], row
candidate = next(c for c in row["target"]["candidates"] if c["corner_mm"] == CORNER)
assert candidate["max_distance_mm"] == MAX, candidate
facts = {
    "body_id": catalog["bodies"][0]["body_id"], "base_feature_id": base["feature_id"],
    "sketch_id": sketch["sketch_id"], "height_mm": H, "plate_mm": [-4.5, 3.25, 37.5, 12.25],
    "vertices": [{"curve_id": v["curve_id"], "start_mm": v["start_mm"]} for v in sketch["vertices"]],
    "content_version": catalog["content_version"],
    # What the window is asked to do, and the numbers each step is judged by.
    "corner_mm": CORNER, "edge": candidate["edge"],
    "create_distance_mm": D1, "edit_distance_mm": D2,
    "max_distance_mm": MAX, "refused_distance_mm": math.nextafter(MAX, 1e9),
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
(out / "facts.json").write_text(json.dumps(facts))
print("FCAD_29A_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_29A_DIR/../watch-29a.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

If system memory pressure aborts or stops the run, it does not count; start
again from a fresh fixture directory. **Do not address the viewer after Quit
through the automation**: it may relaunch the app outside the watchdog. If it was
relaunched, quit that instance too and read only the original PID's exit and
watchdog record. Type numbers exactly as written.

1. **Open** `$FCAD_29A_DIR/plate.fcad` (asynchronously). **Chamfer edge of …**
   is offered and lists four corners: (33, 15.5), (33, 3.25), (-4.5, 3.25),
   (-4.5, 15.5), each with its two side lengths (12.25 × 37.5) and `d ≤ 12.24 mm`.
2. **Create.** Choose the corner (33, 3.25), distance `2.375` → **Apply chamfer**
   → **Save chamfer copy…** → `$FCAD_29A_DIR/gui-chamfer.fcad`; it opens
   asynchronously. The feature list shows Extrude → Chamfer; no further Chamfer
   is offered.
3. **Edit.** On the Chamfer open **Edit Chamfer distance…**; the form names the
   Chamfer, its Body and the edge. Type `4.5` and **Apply distance**.
4. **Exact refusal.** Type `12.240000000000002` (the largest accepted value,
   `12.24`, plus one representable step; it is also `refused_distance_mm` in
   `facts.json`) → **Apply distance**: refused with the bound named; the draft
   is kept; Save is unavailable and no `gui-refused.fcad` is published. Type
   `12.24` → accepted (do not save this).
5. **Undo/Redo and recovery.** The form's **Undo request** / **Redo request**
   step back and forward over the applied requests (each is a whole request, not
   a field edit). Return to `4.5` and **Apply distance**.
6. **Save Cancel.** **Save distance copy…** → **Cancel** in the file dialog:
   nothing starts and the draft stays.
7. **Publish.** **Save distance copy…** → `$FCAD_29A_DIR/gui-size.fcad`; it
   opens asynchronously.
8. **Exports.** Export `gui-size.stl` and `gui-size.fbx` of `gui-size.fcad`
   into `$FCAD_29A_DIR`, at the default tessellation.
9. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-29a-compare.py "$FCAD_29A_DIR"
```

It first requires `gui-chamfer.fcad`, `gui-size.fcad`, `gui-size.stl` and
`gui-size.fbx` (and never creates them), refuses a `gui-refused.fcad`, checks
the source's SHA-256, each window edit's allowlist (the creation: one new row,
the Body's payload and hash, seven names, two dependency edges; the edit: one
row), makes the CLI peers, maps only the new Chamfer UUID and its seven new
names by meaning, compares every SQL cell except the stamp (a payload hash only
where no mapped UUID is inside the payload, and the payload itself always),
requires the same Chamfer UUID before and after the edit, a cold rebuild that
resolves every stored name, byte-equal STL/FBX, then reads the window's STL
itself: closed, one winding, the plate's bounds, the volume `(W·D − d²/2)·h`,
the three untouched corners and the vertex columns of the cut at the chosen
corner, and the one flat: its plane, outward normal and area `d·√2·h`.
It prints `FCAD_29A_GUI_COMPARE_OK cells=N triangles=M`.

```python
# FCAD_29A_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-chamfer.fcad", "gui-size.fcad", "gui-size.stl", "gui-size.fbx")
missing = [n for n in GUI if not (out / n).is_file()]
if missing:
    sys.exit(f"FCAD_29A_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first")
assert not (out / "gui-refused.fcad").exists(), "a refused Save published something"
source = out / "plate.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
BODY = facts["body_id"]
D1, D2 = facts["create_distance_mm"], facts["edit_distance_mm"]
X0, Y0, W, D = facts["plate_mm"]
H = facts["height_mm"]
CORNER = facts["corner_mm"]
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
def allowlist_row(before, after, row):
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
                assert u == v or (t == "objects" and c in ("payload", "payload_hash") and x[ac.index("id")] == rid) \
                    or (t == "meta" and c == "modified_at"), f"{t}.{c} moved"
def allowlist_create(before, after):
    """One new Chamfer row, the Body row's payload and hash, seven new names,
    two new dependency edges and the old tip edge gone, the new capabilities,
    the stamp; nothing else moved."""
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
            assert len(brows) - len(kept) == (2 if t == "deps" else 7), t
        elif t == "capabilities":
            assert set(arows) <= set(brows), "a capability changed"
            new = {r[0] for r in set(brows) - set(arows)}
            assert new <= {"feature.chamfer.v1", "feature.predecessor.v1", "topology.origin-face.v1"}, new
            assert "feature.chamfer.v1" in new, "the Chamfer capability is declared"
        elif t == "meta":
            for x, y in zip(arows, brows):
                assert all(u == v or c == "modified_at" for c, u, v in zip(ac, x, y)), "meta"
        else:
            assert arows == brows, t
def peer_chamfer(src, dest):
    catalog = run("inspect", src, "--json")["result"]
    chosen = next(c for c in catalog["bodies"][0]["chamfer_edge"]["target"]["candidates"]
                  if c["corner_mm"] == CORNER)
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "distance_mm": D1}))
    run("chamfer-edge-copy", src, "--body", BODY, "--expect-version", catalog["content_version"],
        "--request", request, "-o", dest, "--json")
    request.unlink()
def peer_distance(src, feature, dest):
    request = out / "peer-request.json"
    request.write_text(json.dumps({"request_version": 1, "distance_mm": D2}))
    run("edit-chamfer-distance", src, "--feature", feature, "--expect-version",
        run("inspect", src, "--json")["result"]["content_version"], "--request", request, "-o", dest, "--json")
    request.unlink()
g = {"chamfer": out / "gui-chamfer.fcad", "size": out / "gui-size.fcad"}
p = {n: out / f"peer-{n}.fcad" for n in g}
for q in p.values():
    q.unlink(missing_ok=True)
peer_chamfer(source, p["chamfer"])
peer_distance(p["chamfer"], run("inspect", p["chamfer"], "--json")["result"]["chamfers"][0]["feature_id"], p["size"])
# What the window's documents say: one Chamfer at the chosen edge, the size as
# typed, the same feature UUID before and after the edit, names that resolve.
def chamfer_of(path, distance):
    result = run("inspect", path, "--json")["result"]
    (row,) = result["chamfers"]
    assert row["distance_mm"] == distance and row["corner_mm"] == CORNER, row
    assert row["edge"] == facts["edge"] or sorted(row["edge"]["joint"]) == sorted(facts["edge"]["joint"]), row
    assert row["distance_edit"]["available"] is True and row["distance_edit"]["max_distance_mm"] == facts["max_distance_mm"]
    assert result["bodies"][0]["chamfer_edge"]["available"] is False, "a second Chamfer is offered"
    assert run("validate", path, "--json")["result"]["valid"] is True
    n = len(tables(path)["topology_refs"][1])
    text = run("rebuild", path, "--cold")
    assert "tip Chamfer" in text and f"{n} of {n} stored references resolved" in text, text
    return row
first, second = chamfer_of(g["chamfer"], D1), chamfer_of(g["size"], D2)
assert first["feature_id"] == second["feature_id"], "the size edit changed the Chamfer UUID"
# Allowlists on the window's own chain: each step moves exactly what it may.
allowlist_create(source, g["chamfer"])
allowlist_row(g["chamfer"], g["size"], first["feature_id"])
assert [r[0] for r in tables(g["chamfer"])["topology_refs"][1]] == [r[0] for r in tables(g["size"])["topology_refs"][1]], \
    "a name changed UUID"
# Every cell of every window document equals the CLI's, with only the UUIDs the
# operation minted (the Chamfer and its new names, by meaning) mapped.
mapping = {}
def apply(cell):
    if isinstance(cell, bytes):
        for a, b in mapping.items():
            cell = cell.replace(a, b)
    return cell
def mint(gui, peer, before):
    def new(path):
        a, b = tables(before), tables(path)
        old_ids = {r[a["objects"][0].index("id")] for r in a["objects"][1]}
        objs = [r for r in b["objects"][1] if r[b["objects"][0].index("id")] not in old_ids]
        assert len(objs) == 1, "one new object"
        cols = b["topology_refs"][0]
        old_refs = {r[cols.index("id")] for r in a["topology_refs"][1]}
        return objs[0][b["objects"][0].index("id")], [r for r in b["topology_refs"][1] if r[cols.index("id")] not in old_refs], cols
    gf, grefs, cols = new(gui)
    pf, prefs, _ = new(peer)
    assert len(grefs) == len(prefs) == 7
    mapping[gf] = pf
    keep = lambda r, f: tuple((c, f(r[i])) for i, c in enumerate(cols) if c not in ("id", "payload_hash"))
    left = {keep(r, apply): r[cols.index("id")] for r in grefs}
    right = {keep(r, lambda c: c): r[cols.index("id")] for r in prefs}
    assert left.keys() == right.keys(), "the new names differ in meaning"
    for k, u in left.items():
        mapping[u] = right[k]
def same(gui, peer):
    left, right = tables(gui), tables(peer)
    assert left.keys() == right.keys()
    cells = 0
    for t in left:
        (cols, lrows), (rc, rrows) = left[t], right[t]
        assert cols == rc and len(lrows) == len(rrows), t
        order = sorted(lrows, key=lambda r: repr([apply(c) for c in r]))
        theirs = sorted((list(r) for r in rrows), key=repr)
        for raw, y in zip(order, theirs):
            x = [apply(c) for c in raw]
            for k, c in enumerate(cols):
                if t == "meta" and c == "modified_at":
                    continue
                if c == "payload_hash" and "payload" in cols:
                    payload = cols.index("payload")
                    assert x[payload] == y[payload], f"{t}.payload"
                    if apply(raw[payload]) == raw[payload] and apply(raw[cols.index("id")]) == raw[cols.index("id")]:
                        assert x[k] == y[k], f"{t}.payload_hash"
                    cells += 1
                    continue
                assert x[k] == y[k], f"{t}.{c}"
                cells += 1
    return cells
mint(g["chamfer"], p["chamfer"], source)
cells = same(g["chamfer"], p["chamfer"]) + same(g["size"], p["size"])
# The exports of the window's last document are the CLI's, byte for byte, and
# the mesh is read here, independently of the B-Rep.
for fmt in ("stl", "fbx"):
    run(f"export-{fmt}", p["size"], "-o", out / f"peer-size.{fmt}", "--json")
    assert (out / f"gui-size.{fmt}").read_bytes() == (out / f"peer-size.{fmt}").read_bytes(), fmt
data = (out / "gui-size.stl").read_bytes()
(count,) = struct.unpack_from("<I", data, 80)
assert len(data) == 84 + 50 * count
tri = [[struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for k in range(3)] for i in range(count)]
pts = [q for t in tri for q in t]
key = lambda v: tuple(round(x * 1e4) for x in v)
directed = {}
for a, b, c in tri:
    for u, v in ((a, b), (b, c), (c, a)):
        directed[(key(u), key(v))] = directed.get((key(u), key(v)), 0) + 1
assert all(n == 1 for n in directed.values()), "not one oriented surface"
assert all((v, u) in directed for u, v in directed), "the mesh is open"
six = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2])
          + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in tri)
xs, ys, zs = ([pt[k] for pt in pts] for k in range(3))
for got, want in ((min(xs), X0), (max(xs), X0 + W), (min(ys), Y0), (max(ys), Y0 + D), (min(zs), 0.0), (max(zs), H)):
    assert abs(got - want) < 1e-4, (got, want)
exact = (W * D - D2 * D2 / 2) * H
assert abs(six / 6 - exact) < 1e-3, (six / 6, exact)
# The chosen corner: the plate's own corner is gone, the cut's two new vertex
# columns are D2 along each adjacent side, and no other corner changed.
corners = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]
near = lambda c, z: any(abs(pt[0] - c[0]) < 1e-4 and abs(pt[1] - c[1]) < 1e-4 and abs(pt[2] - z) < 1e-4 for pt in pts)
for c in corners:
    for z in (0.0, H):
        assert near(c, z) == (c != CORNER), (c, z)
ix = -1.0 if abs(CORNER[0] - (X0 + W)) < 1e-9 else 1.0
iy = -1.0 if abs(CORNER[1] - (Y0 + D)) < 1e-9 else 1.0
for z in (0.0, H):
    assert near([CORNER[0] + ix * D2, CORNER[1]], z) and near([CORNER[0], CORNER[1] + iy * D2], z), z
# The one flat: every triangle on the plane ix*(x-cx) + iy*(y-cy) = D2, facing
# out of the plate, adding up to D2*sqrt(2)*H.
flat = 0.0
for a, b, c in tri:
    if all(abs(ix * (v[0] - CORNER[0]) + iy * (v[1] - CORNER[1]) - D2) < 1e-4 for v in (a, b, c)):
        u = [b[i] - a[i] for i in range(3)]
        w = [c[i] - a[i] for i in range(3)]
        n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]]
        length = math.sqrt(sum(x * x for x in n))
        assert (-ix * n[0] - iy * n[1]) / length / math.sqrt(2) > 1 - 1e-4, "the flat faces the plate"
        flat += length / 2
assert abs(flat - D2 * math.sqrt(2) * H) < 1e-3, flat
print("FCAD_29A_GUI_COMPARE_OK", f"cells={cells}", f"triangles={count}")
```

A wrong corner or distance in the window's document fails at the first assertion
about the saved Chamfer; a wrong geometry with a right document fails at the
mesh checks.
