# §28A — verification record

[Contract and agent recipe](single-edge-fillet.md).

This records what was executed for this slice and where. It does not repeat
earlier slices' evidence.

## Where and how this was run

* **Base.** Freshly fetched `origin/main` at
  `226cb1612d070b32e236889c9f09d6a3f81fb08a`, the merge of PR #60. Its tree
  `e937b80…` equals the reviewed §27H head `926f82a`. The branch is
  `single-edge-fillet-copy`, created from a clean checkout.
* **Merge-triggered CI on the base**, as read while this slice was built:
  CI ([run 36307533376](https://github.com/gesriot/ferrite-cad/actions/runs/36307533376)),
  rust sbom, product sbom, planegcs pin and rust notices had concluded
  success; combined runtime layout
  ([run 36307533504](https://github.com/gesriot/ferrite-cad/actions/runs/36307533504))
  was still in progress then; it later concluded success (see [CI](#ci)).
* **Cloud container.** Linux x86_64, 4 CPUs, 15 GiB RAM, the session's
  OCCT 8.0.1 and its existing native and stub targets. No PlaneGCS is linked
  here (`FERRITECAD_REQUIRE_PLANEGCS=0`). None is needed: a Fillet asks no
  solver anything.
* **No window, GPU test or browser was launched.** The widget tests drive
  real egui widgets headlessly; that is not a window test.

## OCCT probe — measured before the policy was chosen

A C++ probe on OCCT 8.0.1 rounded one vertical edge of a
37.5 × 12.25 × 6.75 mm plate at (−4.5, 3.25) with `BRepFilletAPI_MakeFillet`.
Each result was checked with `BRepCheck_Analyzer`, and its volume was
compared with `W·D·H − (1 − π/4)·r²·H`:

| r (mm) | result |
| --- | --- |
| 12.25 (= the shorter side) and larger | `IsDone` false |
| 12.2 | valid; relative volume error 3e-15 |
| 0.01 … 6.125 | valid; relative error ≤ 5e-10 |
| 0.001 | 1.3e-7 |
| 1e-5 | 1.4e-3 |
| 1e-7 | "valid", removes 0 mm³ |

On success, `Generated(edge)` is exactly one cylindrical face with its axis
along Z, and `IsDeleted(edge)` is true.

The chosen class is `0.01 mm ≤ r ≤ ½ · min(adjacent Lines)`. It is well
inside both ends of the measured range and is never clamped.

## What changed

* **Document.** The changes are:
  * the `feature.fillet` kind with payload v1 and the `feature.fillet.v1`
    capability;
  * `SemanticRole::EdgeFilletFace`;
  * validator rules for `previous` and the edge's feature;
  * `fillet.rs`, which holds:
    * discovery, through the shared `saved_history` with zero Cuts;
    * the rectangle corners and the radius policy;
    * preparation;
    * the writer's re-derivation;
    * the evaluator's class check.

  The Cut, height, sketch, constraint, circle, annulus and Revolve readers
  all refuse a filleted part by naming the Fillet.
* **Kernel.** `FilletRequest`, `FilletResult` with its own `validate`,
  `GeometryKernel::fillet_edge` and `fillet_cache_key`.
* **OCCT bridge.** `fc_occt_fillet_edge` and `fc_occt_fillet_faces`, with the
  carried answers read back through the existing `fc_occt_cut_carried`.
* **Topology.** `record_fillet`, the resolver arm, and archive tag 18. The
  archive format stays v3.
* **Evaluation.** The cold and cached Fillet arm, and the `eval.fillet.named`
  key.
* **Jobs.** `fillet_edge_copy` through the shared `edit_object_copy`.
* **CLI.** `fillet-edge-copy`, `bodies[].fillet_edge` discovery, and a
  Fillet line in `rebuild`.
* **App.** The `fillets` form, and the worker and Open wiring beside the Cut
  form's.
* **Unchanged.** The SQLite schema, ADR 0004, and the corpus
  `fillet_all` probe.

## New tests and gates

* **CLI** (`crates/ferritecad-cli/tests/fillet.rs`):
  * `fillet_discovery_and_protocol_without_native` — the four candidates and
    their labels, the canonical pair, strict JSON v1, structural refusals,
    exit 7 on a lost report, and nothing written. The strict JSON cases are:
    * unknown and duplicate keys, and the escaped duplicate
      `"radius\u005fmm"` (a literal backslash in the fixture);
    * arrays where objects belong, three Lines, and one Line twice;
    * a string or infinite radius, an index, coordinates, a v2 request, and
      an oversized request.
  * `fillet_discovery_refuses_every_composition_outside_the_class_without_native`
    — reversed, symmetric, constrained, placed, two bodies, a trapezoid.
  * `native_two_corners_both_windings_and_starts_are_what_the_numbers_say` —
    a CCW plate, a CW plate from the upper right, and a CCW plate from the
    third corner. Each has two corners, at r = 2.375 and 3.0625, requested
    with the pair reversed. For every copy:
    * the result JSON, payload, tip, dependencies and seven new names;
    * the SQL allowlist and `validate`;
    * a cold `rebuild` (10 of 10 names), cold/Miss/Hit equality, 7 faces, and
      the volume to 1e-9;
    * `Cylinder{r}` about an axis r inward of the chosen corner;
    * carried caps and sides as planes, and base names still on the base
      shape;
    * the STL read independently: closed and oriented, the chosen corner
      without a vertex and the other three with one, vertices on the arc,
      the chord-bounded volume;
    * a complete FBX.
  * `native_changing_the_plate_invalidates_the_cached_fillet` — Miss, then Hit,
    then the plate made 9.5 mm tall under the document API: the Extrude and
    the Fillet both Miss, the volume is the new exact one, and cold and Hit
    agree.
  * `native_fillet_refusals_and_cancellation_are_atomic` — covers:
    * the source, occupied and hard-link outputs;
    * the radii 0, 0.009, D/2 + 1e-9, D and 100;
    * a stale version;
    * cancellation before start and at the last barrier;
    * a lost report after publication (exit 7, the file kept).
  * `native_a_filleted_copy_and_other_histories_are_refused_by_name` — every
    editor's discovery names the Fillet's UUID. A second Fillet, a Cut and
    `edit-extrude` are refused and the copy is unchanged. A Cut history, a
    Revolve, a circle and an annulus are refused by discovery.
  * `native_the_evaluator_refuses_a_saved_fillet_outside_the_class` — forged
    radius and non-adjacent joint refused by the evaluator; an edge of a
    Sketch refused by the validator first.
* **OCCT.** `tests/fillet_edge_occt.rs` covers the trait on both windings,
  all four corners, and every tracked face's outcome, plus the refusals and
  cancellation. The two `ffi::tests` check the header argument order, and
  the bridge on all four joints with bad arguments.
* **Kernel.** Request, result and cache-key unit tests.
* **Document.** Six `fillet::tests`: payload round trip and canonical joint,
  both windings and every start, the non-rectangles, the radius policy and
  edge meaning, the writer against forged preparations, and the validator.
* **Topology.** The codec round trip, the swapped joint and an unknown tag;
  the tag table; and `record_fillet` with silent and contradictory history.
* **App.** Two tests:
  * `fillets::tests::fillet_widgets_list_corners_refuse_like_the_document_and_keep_the_draft`
    — kernel-free. The form is opened through its own button. It covers:
    * the corner list;
    * refusals, and unapplied changes;
    * one press giving one request;
    * the draft surviving a cancelled Save, a running job, a stale reply
      and a worker refusal.
  * `fillets::tests::native_fillet_widgets_worker_and_cli_publish_the_same_part`
    — widgets, the real worker and async-Open draft retention, then the peer
    CLI. Every SQL table and cell is compared after mapping only the eight
    minted UUIDs (the Fillet and its seven names); a payload hash is checked
    through its payload. The STL and FBX bytes are equal.
* **CI.**
  * **`ci.yml`** gains a stub step. It runs the two discovery gates, the
    widget gate and two writer gates, and runs the recipe expecting
    `FCAD_28A_RECIPE_NO_KERNEL`.
  * **`runtime-layout.yml`** gains a native step on three OSes. It runs
    every test above by exact name, fails on `skipped:`, and exports the
    six copies for the FBX loop.
  * **The no-solver step** runs the two main native gates and the recipe
    with `--no-default-features`.
  * **`tools/check-fbx-complex.sh`** reads the six FBX files with pinned
    ufbx, joins their triangles with the STL, and prints
    `FCAD_FILLET_UFBX_EXECUTED`.

## Local results

Native (OCCT 8.0.1, no PlaneGCS, debug):

* `--test fillet`: 7 passed. `--test fillet_edge_occt`: 2 passed. The two
  `ffi` tests, the three kernel tests, the six document tests and the three
  topology tests all passed. The app `fillets::` tests: 2 passed.
* Recipe: `FCAD_28A_RECIPE_OK volume=3087.091072 exact=3087.195319 r=3.0625`.
* **FBX.** Pinned ufbx v0.23.0 was built earlier in this container from the
  pinned sources. It read the six exported copies, with `--identity` giving
  `checks=6 failures=0`. `stl-matches-fbx.py` matched their triangles:
  64 or 68 triangles, worst 1.73e-18 m.
* **Full workspace:** `cargo test --workspace --no-fail-fast` gave
  2095 passed, 19 failed and 2 ignored.
  * The 19 failures are exactly the set of the earlier §27D baseline run in
    this container, compared line by line: 18 tests that need a linked
    PlaneGCS while `FERRITECAD_REQUIRE_OCCT=1`, and the root-only
    `validation_really_read_only_permissions`.
  * None of them involves a Fillet.
* `cargo fmt --all -- --check` is clean. `cargo clippy --workspace
  --all-targets -D warnings` is clean natively, and so is the stub target
  with `--all-features` (the CI invocation).

Stub (no OCCT): `--test fillet` 7 passed, the 5 native ones printing
`skipped:`; the app `fillets::` tests 2 passed, the worker one skipped; the
recipe prints `FCAD_28A_RECIPE_NO_KERNEL` with the typed `unsupported`.

### Mutations — local, executed, restored byte for byte

Each mutation was applied by a script, compiled, and run against
`--test fillet`. The file was then restored from `HEAD`, and its SHA-256 was
checked (`cold.rs` e6639163…, `cache.rs` a7d4acbb…).

| Mutation | Caught by (executed) |
| --- | --- |
| M1 — wrong corner: the evaluator rounds the first *other* named sweep edge while the names still say the requested joint | `native_two_corners…`: "the axis [-2.125, 5.625, 0] is not r inward of [33.0, 3.25]" |
| M2 — stale predecessor cache: `eval.fillet.named` keyed by the predecessor's identity instead of its content key | `native_changing_the_plate…`: after the plate changed, the Extrude missed and the Fillet answered `Hit` |

After restoring, `--test fillet` (7), the app `fillets::` tests (2) and
`fillet_edge_occt` (2) passed again.

### The prior-main CLI (226cb16) on a filleted copy — local

The worktree `/home/user/base` was moved to `226cb16` (detached), and its
debug CLI was built natively into `/home/user/base-target`. This head made
`plate.fcad` and its filleted copy.

| On the filleted copy | prior main 226cb16 |
| --- | --- |
| `inspect --json` | ok; `edit_extrude` refused with `document_refusal` "it requires feature.fillet.v1, which this build does not implement" |
| `validate`, `rebuild --cold`, `export-stl` | refused: a reference "requires unsupported capabilities: feature.fillet.v1" |
| `cut-circular-copy`, `edit-extrude` | exit 2, "document cannot be edited: it requires feature.fillet.v1…", nothing written |
| `fillet-edge-copy` | unknown subcommand |
| the file | SHA-256 unchanged |

On the unfilleted `plate.fcad`, `inspect` is identical outside the new
`bodies[].fillet_edge`, and the STL and FBX are byte-identical between the
two builds.

## CI

The code and CI files are those of `a51c0d1`. Any later head changes only
this record, so it triggers ordinary CI but no native runtime run.

* **First native run, on `bbb907a`**
  ([run 36310418061](https://github.com/gesriot/ferrite-cad/actions/runs/36310418061)):
  * Linux and macOS were green, including the new Fillet step and the FBX
    loop.
  * Windows passed every functional step, the Fillet step included. It then
    failed "Require one owner for every staged file": `bin/TKBool.dll` and
    `bin/TKFillet.dll` were staged but owned by nobody.
    * Why: MSVC imports only what is referenced. With `fillet_edge` in the
      product, the Windows binaries now load TKFillet and its TKBool
      dependency. Linux and macOS already staged both.
    * The fix (`a51c0d1`) adds the two rows to
      `tools/native/staged-windows.tsv` and regenerates the native inventory
      and the product SBOMs with the repository's generators. Locally,
      `check-native-inventory.sh` passed 58 checks and
      `check-product-sbom.sh` passed 40.
* **Native runtime and packaging on `a51c0d1`**
  ([run 36314451887](https://github.com/gesriot/ferrite-cad/actions/runs/36314451887)):
  green on all three platforms, along with the cross-platform comparison
  and release-set job
  ([Linux](https://github.com/gesriot/ferrite-cad/actions/runs/36314451887/job/108606392492),
  [macOS](https://github.com/gesriot/ferrite-cad/actions/runs/36314451887/job/108606392506),
  [Windows](https://github.com/gesriot/ferrite-cad/actions/runs/36314451887/job/108606392398)).
  * **What the tool returned.** Only the last 20000 lines of each job log.
    They begin inside the native steps, after the no-solver step.
  * **In that tail, on all three OSes:**
    * `ok` for all 14 names searched, all seven CLI `fillet` gates among them;
    * the two `fillet_edge_occt` tests, the two `ffi` tests, the two app
      `fillets::` tests, and `record_fillet`;
    * `FCAD_FILLET_UFBX_EXECUTED` (pinned ufbx identity and the triangle
      join with the STL);
    * no `FAILED`, no panic, and no test output with `skipped:`.
  * **Outside that tail, and not read here:** the no-solver step's two
    Fillet gates and its `FCAD_28A_RECIPE_OK` line. That step is green; it
    exits 1 on a missing `ok`, a `skipped:` or a missing marker.
* **Ordinary CI on `a51c0d1`**
  ([run 36314453921](https://github.com/gesriot/ferrite-cad/actions/runs/36314453921)):
  success on all three OSes. On `958ee69`
  ([run 36311042342](https://github.com/gesriot/ferrite-cad/actions/runs/36311042342))
  the Ubuntu log showed the stub step's gates passing and
  `FCAD_28A_RECIPE_NO_KERNEL`.
* **Other workflows on `a51c0d1`:** product sbom
  ([run 36314453914](https://github.com/gesriot/ferrite-cad/actions/runs/36314453914))
  and rust sbom
  ([run 36314453903](https://github.com/gesriot/ferrite-cad/actions/runs/36314453903))
  succeeded. planegcs pin on `bbb907a`
  ([run 36310418040](https://github.com/gesriot/ferrite-cad/actions/runs/36310418040))
  succeeded.
* **Base.** Every merge-triggered workflow on `226cb16` concluded success.
  That includes combined runtime layout
  ([run 36307533504](https://github.com/gesriot/ferrite-cad/actions/runs/36307533504)),
  which finished after this slice began.

## Limits

* One vertical edge of one unconstrained axis-aligned rectangular plate,
  once. Not supported:
  * chains, cap edges, a variable radius, and a second Fillet;
  * editing the Fillet or anything under it;
  * Chamfer and Shell;
  * a preview or mouse picking;
  * in-place Save.
  The 5C milestone is not complete.
* The Fillet tip publishes no edge or vertex names; the base's edge names
  stay historical at the base.
* The radius bounds are a measured, conservative policy on OCCT 8.0.1. The
  kernel limit is the shorter side itself.
* A mesh check is chord-bounded, not exact. The exact claims are the B-Rep's
  (volume, surface, radius and axis).
* No window, GPU or browser test was run here. The historical fillet-corpus
  OOM is not declared fixed.

## macOS fixtures and window scenario (for the reviewer's Mac)

Nothing here was run in this cloud container, which has no window system.
The generator and comparator use only the staged CLI. They were exercised
here with a second CLI copy standing in for the window's output, which says
nothing about the window. A tampered `objects.name` cell was caught.

### Fixture generator

It writes `plate.fcad`, the peer request, and `facts.json` (Body, version,
corner, joint, radius and the source SHA-256) into a new directory:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/single-edge-fillet-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28A_GUI_FIXTURE\n", "ferrite-28a-fixture.py"),
                   ("# FCAD_28A_GUI_COMPARE\n", "ferrite-28a-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28a-fixture.py "$FCAD_28A_DIR"
```

```python
# FCAD_28A_GUI_FIXTURE
import json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
X0, Y0, W, D, H = -4.5, 3.25, 37.5, 12.25, 6.75
request = out / "create.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H,
    "points_mm": [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]}))
source = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", source, "--json")
request.unlink()
catalog = run("inspect", source, "--json")["result"]
body = catalog["bodies"][0]
chosen = next(c for c in body["fillet_edge"]["target"]["candidates"]
              if c["corner_mm"] == [X0 + W, Y0])
peer_request = out / "peer-request.json"
peer_request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"],
                                    "radius_mm": 2.375}))
(out / "facts.json").write_text(json.dumps({
    "body_id": body["body_id"], "content_version": catalog["content_version"],
    "corner_mm": chosen["corner_mm"], "joint": chosen["edge"]["joint"],
    "radius_mm": 2.375, "source_sha256": __import__("hashlib").sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28A_GUI_FIXTURE_OK", out)
```

### Window scenario

Run `ferritecad-viewer` against the staged OCCT under the usual watchdog.

1. **Open.** Open `$FCAD_28A_DIR/plate.fcad`. In **Saved objects**, the
   button **Fillet edge of Body — …** is enabled.
2. **Form.** Press it. The window "Fillet one vertical edge — new copy"
   lists four corners, each with both Line UUIDs, the side lengths and
   `r ≤ 6.125 mm`, and shows "Radius from 0.01 mm to 0.5 × the shorter
   adjacent side".
3. **Refusals.** **Apply fillet** with nothing chosen is refused ("choose one
   vertical edge"). Choose **Corner (33, 3.25)** and try the radius `7`: it
   is refused as too large, and no **Save** appears.
4. **Apply.** Enter the radius `2.375` and press **Apply fillet**. The
   window reads "Ready: round the edge at (33, 3.25) with r2.375 mm".
5. **Save Cancel.** **Save fillet copy…** → **Cancel**. The draft stays.
6. **Save and Open.** **Save fillet copy…** → `$FCAD_28A_DIR/gui.fcad`. The
   viewer opens the copy asynchronously. The corner at (33, 3.25) is
   rounded, and the other three are sharp.
7. **Exports.** From the window, export **STL** and **FBX** of `gui.fcad`
   into `$FCAD_28A_DIR` as `gui.stl` and `gui.fbx`, at the default
   tessellation.
8. **Refused editors.** With `gui.fcad` open, the **Fillet edge**, Cut and
   sketch buttons are disabled, and their hover text names the Fillet.
9. **Compare.** Quit the viewer normally, then run:

```sh
FERRITECAD=/path/to/ferritecad python3 ferrite-28a-compare.py "$FCAD_28A_DIR"
```

The compare step makes the peer copy with the same request and exports it.
It then compares every table and every cell of `gui.fcad` against the peer,
after mapping only the Fillet's UUID and its seven names by meaning. A
payload hash is compared through its payload, and `meta.modified_at` is
skipped. It requires `gui.stl` and `gui.fbx` to be byte-identical to the
peer's. It never creates or overwrites a `gui.*` file, and a missing one is
a failure. It prints `FCAD_28A_GUI_COMPARE_OK`.

```python
# FCAD_28A_GUI_COMPARE
import hashlib, json, os, pathlib, sqlite3, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
source, gui = out / "plate.fcad", out / "gui.fcad"
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["source_sha256"], "source changed"
for name in ("gui.fcad", "gui.stl", "gui.fbx"):
    assert (out / name).exists(), f"export {name} from the window first"
peer = out / "peer.fcad"
if not peer.exists():
    p = subprocess.run([cli, "fillet-edge-copy", source, "--body", facts["body_id"],
                        "--expect-version", facts["content_version"], "--request",
                        out / "peer-request.json", "-o", peer, "--json"],
                       capture_output=True, encoding="utf-8")
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
        try:
            cur = db.execute(f'SELECT rowid, * FROM "{t}" ORDER BY rowid')
            rows = cur.fetchall()
        except sqlite3.OperationalError:  # a WITHOUT ROWID table
            cur = db.execute(f'SELECT * FROM "{t}"')
            rows = cur.fetchall()
        out[t] = ([d[0] for d in cur.description], rows)
    db.close()
    return out

def minted(path):
    """The Fillet's id and its seven names, keyed by what each name means."""
    db = sqlite3.connect(f"{path.as_uri()}?mode=ro", uri=True)
    old = {r[0] for r in sqlite3.connect(f"{source.as_uri()}?mode=ro", uri=True)
           .execute("SELECT id FROM objects")}
    new = [r[0] for r in db.execute("SELECT id FROM objects") if r[0] not in old]
    assert len(new) == 1, new
    cols = [d[0] for d in db.execute("SELECT * FROM topology_refs").description]
    refs = [dict(zip(cols, r)) for r in db.execute("SELECT * FROM topology_refs")]
    db.close()
    mine = [r for r in refs if new[0] in r.values()]
    assert len(mine) == 7, len(mine)
    return new[0], mine

a_id, a_refs = minted(gui)
b_id, b_refs = minted(peer)
pairs = [(b_id, a_id)]
def meaning(r, fid):
    return repr(sorted((k, v.replace(fid, b"F") if isinstance(v, bytes) else v)
                       for k, v in r.items() if k not in ("id", "rowid")))
for r in b_refs:
    match = [m for m in a_refs if meaning(m, a_id) == meaning(r, b_id)]
    assert len(match) == 1, "a new name has no single counterpart"
    key = next(k for k in r if k == "id")
    pairs.append((r[key], match[0][key]))

def mapped(v):
    if isinstance(v, bytes):
        for x, y in pairs:
            v = v.replace(x, y)
    return v

left, right = tables(gui), tables(peer)
assert left.keys() == right.keys()
compared = 0
for t in left:
    (lc, lrows), (rc, rrows) = left[t], right[t]
    assert lc == rc and len(lrows) == len(rrows), t
    rrows = [tuple(map(mapped, r)) for r in rrows]
    if lc[0] != "rowid":  # no stored order: compare as sets, after mapping
        lrows, rrows = sorted(lrows, key=repr), sorted(rrows, key=repr)
    for lr, rr in zip(lrows, rrows):
        for c, x, y in zip(lc, lr, rr):
            compared += 1
            if t == "meta" and c == "modified_at":
                continue
            if t == "objects" and c == "payload_hash":
                payload = lr[lc.index("payload")]
                assert payload == rr[lc.index("payload")], "payload"
                continue
            assert x == y, f"{t}.{c} differs"
for fmt in ("stl", "fbx"):
    assert (out / f"gui.{fmt}").read_bytes() == (out / f"peer.{fmt}").read_bytes(), fmt
print("FCAD_28A_GUI_COMPARE_OK", f"cells={compared}", f"minted={len(pairs)}")
```
