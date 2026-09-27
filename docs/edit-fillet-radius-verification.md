# §28B — verification record

[Contract and agent recipe](edit-fillet-radius.md).

This records what was executed for this slice and where. It does not repeat
§28A's evidence.

## Where and how this was run

* **Base.** Freshly fetched `origin/main` at
  `8062ccb2e1f9304516cc39ed32128dc49daca752`, the merge of PR #61. Its tree
  `6403d99…` equals the reviewed §28A head `2115c6f`. The branch is
  `edit-fillet-radius`, created from it.
* **Merge-triggered CI on the base.** Every push run on `8062ccb` concluded
  success: CI
  ([run 36320735820](https://github.com/gesriot/ferrite-cad/actions/runs/36320735820)),
  combined runtime layout
  ([run 36320735808](https://github.com/gesriot/ferrite-cad/actions/runs/36320735808)),
  planegcs pin
  ([run 36320735846](https://github.com/gesriot/ferrite-cad/actions/runs/36320735846)),
  rust notices
  ([run 36320735810](https://github.com/gesriot/ferrite-cad/actions/runs/36320735810)),
  rust sbom
  ([run 36320735818](https://github.com/gesriot/ferrite-cad/actions/runs/36320735818))
  and product sbom
  ([run 36320735860](https://github.com/gesriot/ferrite-cad/actions/runs/36320735860)).
  Some were still in progress when this slice began.
* **Cloud container.** Linux x86_64, 4 CPUs, 15 GiB RAM, the session's
  OCCT 8.0.1 and its existing native and stub targets, built one at a time.
  No PlaneGCS is linked here (`FERRITECAD_REQUIRE_PLANEGCS=0`); none is
  needed.
* **No window, GPU test or browser was launched.** The widget tests drive
  real egui widgets headlessly; that is not a window test.

## What changed

* **Document.**
  * `cut_edit::read_history` is the one frame reader. It now takes an
    optional Fillet: asked about the history *under* it, it expects the
    Body's tip to be that Fillet, starts the chain at its `previous`,
    expects its Predecessor edge and covers its row. The Cut editors and
    §28A call it exactly as before; `saved_history_under_fillet` is the
    new entry. `fillet_references` and `same_meaning` became `pub(crate)`
    so the seven §28A names are checked by the same code that mints them.
  * `fillet_radius.rs`: `saved_fillet` (the frame, zero Cuts, the corner,
    the stored radius under §28A's policy, exactly the seven names),
    `fillet_radius_choices` for discovery, `prepare_fillet_radius`, and
    `rederive`, which the writer runs.
  * `Document::write_fillet_radius`: one checked transaction that
    re-derives the whole prepared value, updates exactly one row's
    `payload`/`payload_hash`, and stamps `modified_at`.
  * `refuse_filleted` now points at `edit-fillet-radius` for the one thing
    that can be edited.
* **Jobs.** `CopyWrite::FilletRadius` and `edit_fillet_radius_copy` through
  the existing `edit_object_copy`.
* **CLI.** `edit-fillet-radius` with strict request v1, and the additive
  top-level `fillets[]` in `inspect --json`.
* **App.** **Edit Fillet radius** buttons, the radius form, and its worker
  and async-Open wiring beside the §28A form's.
* **Unchanged**, with an empty diff against the base: the kernel, the OCCT
  crate and bridge (C++ and CMake), topology, evaluation, `vendor/`,
  `tools/native/` and `Cargo.lock`. No capability, schema, cache key or
  archive tag was added.

## New tests and gates

* **Document** (`fillet_radius::tests`, kernel-free):
  * `only_the_radius_changes_and_repeated_or_equal_edits_keep_every_identity`
    — every SQL cell before and after three edits (up, the same, down);
    only the Fillet's `payload`/`payload_hash` and `meta.modified_at` move.
  * `the_writer_refuses_a_forged_or_stale_preparation` — another edge,
    another predecessor, another name, an out-of-policy radius and another
    version are refused by the writer and write nothing; so is a
    preparation made before the document changed.
  * `a_fillet_outside_the_frame_is_refused_with_its_reason`.
* **CLI** (`crates/ferritecad-cli/tests/fillet.rs`, `mod radius`):
  * `radius_discovery_and_protocol_without_native` — discovery on a plain
    and a filleted plate; strict JSON (unknown and duplicate keys, the
    escaped duplicate `"radius\u005fmm"` with a literal backslash, an
    array, a string, missing and infinite radius, a retargeting key, an
    oversized request, v2); radius 0, negative, below the floor, past the
    bound and at the kernel's limit; the base Extrude and a foreign UUID as
    `--feature`; a stale version; exit 7 on a lost refusal report; nothing
    written. A Fillet with an eighth name is refused by discovery ("more
    faces") while its stored radius is still reported.
  * `native_radius_edits_move_the_named_cylinder_and_keep_every_identity` —
    three fixtures (CCW, CW from the upper right, CCW from the third
    corner), two corners each; 2.375 → 4.8125 → 1.1875 mm. For every copy:
    the result, the full SQL allowlist, every topology reference unchanged,
    `validate`, a cold `rebuild` (10 of 10 names), cold/Miss/Hit equality,
    7 faces, the analytic volume `W·D·H − (1−π/4)·r²·H`, `Cylinder{r}`
    under the same `EdgeFilletFace` reference with its axis moved by exactly
    the change of radius along both sides, the carried planes, the STL and
    a complete FBX.
  * `native_the_same_radius_publishes_the_same_model` — the same payload
    and hash; at most `modified_at` differs.
  * `native_a_changed_radius_misses_in_place_and_the_plate_is_reused` —
    one cache under one path: Miss, Hit, then the edited copy put at the
    same path; the Extrude hits, the Fillet misses, the new cylinder and
    volume are measured, and then both hit and agree with cold.
  * `native_radius_refusals_races_cancellation_and_report_loss_are_atomic`
    — the source, an alias and an occupied output; out-of-policy radii;
    cancellation before the job and at its last barrier (no shape kept);
    another process taking the output at the last barrier (its file kept);
    a stale source; a lost report after publication (exit 7, the copy
    kept with the new radius).
* **App** (`fillets::tests`):
  * `fillet_radius_widgets_show_the_saved_edge_and_keep_the_draft` —
    kernel-free: the form opened through its own button, the edge, bounds
    and refusals, one request per press, and the draft kept on a cancelled
    Save, a running job, a stale reply and a worker refusal.
  * `native_fillet_radius_widgets_worker_and_cli_publish_the_same_part` —
    widgets, the real worker and async Open, then the peer CLI: every SQL
    cell equal except `modified_at` (no UUID mapping: nothing is minted),
    and byte-identical STL and FBX.
* **CI.**
  * **`ci.yml`**, stub step: the discovery gate, the widget gate, the three
    document gates and the recipe expecting `FCAD_28B_RECIPE_NO_KERNEL`.
  * **`runtime-layout.yml`**, native Fillet step on three OSes: the five
    `radius::` gates, the two app gates and the three document gates by
    exact name, failing on `skipped:`. The radius edit writes six more
    exports (`radius-{ccw,cw,third}-{up,down}`) into the Fillet artifact
    directory.
  * **No-solver step** (`--no-default-features`): the two main native
    radius gates and the recipe expecting `FCAD_28B_RECIPE_OK`.
  * **`tools/check-fbx-complex.sh`** reads the six new FBX files with pinned
    ufbx, joins their triangles with the STL, and prints
    `FCAD_FILLET_RADIUS_UFBX_EXECUTED`, which the packed-argv step greps.
  * The Windows TKBool/TKFillet ownership rows from §28A are kept; no
    library set changed.

## Local results

Native (OCCT 8.0.1, no PlaneGCS, debug):

* `--test fillet`: 12 passed (the 7 of §28A and the 5 new). The three
  `fillet_radius` document tests and the four app `fillets::` tests passed.
* The same four native `radius::` gates also passed with
  `--no-default-features`. `ldd target/debug/ferritecad` lists `libTKFillet`
  and `libTKBool` and no PlaneGCS library.
* Recipe: `FCAD_28B_RECIPE_OK up=3067.076540/3067.232319
  down=3098.700654/3098.738551` (the mesh volume against the exact one).
* **FBX.** Pinned ufbx v0.23.0, built earlier in this container from the
  pinned sources, read the six radius exports: `--identity` gave
  `checks=6 failures=0` for each, and `stl-matches-fbx.py` matched their
  triangles (84 up, 48 down; worst 6.94e-18 m).
* **Full workspace:** `cargo test --workspace --no-fail-fast` gave
  2105 passed, 19 failed and 2 ignored (§28A: 2095 passed; the 10 more are
  this slice's).
  * The 19 failures are exactly the set of the earlier baseline runs in
    this container, compared line by line: 18 tests that need a linked
    PlaneGCS while `FERRITECAD_REQUIRE_OCCT=1`, and the root-only
    `validation_really_read_only_permissions`.
  * None of them involves a Fillet.
* `cargo fmt --all -- --check` is clean. `cargo clippy --workspace
  --all-targets -D warnings` is clean natively, and so is the stub target
  with `--all-features`.

Stub (no OCCT): `--test fillet` 12 passed, the native ones printing
`skipped:`; the app `fillets::` tests 4 passed, the two native ones
skipped; the recipe prints `FCAD_28B_RECIPE_NO_KERNEL` with the typed
`unsupported`.

### Mutations — local, executed, restored byte for byte

Each mutation was applied by a script and compiled. The file was then
restored from `HEAD`, and its SHA-256 was checked (`fillet_radius.rs`
a88326b7…).

| Mutation | Caught by (executed) |
| --- | --- |
| M1 — the new radius ignored: `prepare_fillet_radius` keeps the stored radius | three CLI gates: `native_radius_edits…` (result `radius_mm` 2.375 where 4.8125 was asked), `native_a_changed_radius_misses…` (the Fillet answered `Hit` where `Miss` was due), `native_radius_refusals…` (the lost-report copy still had 2.0) |
| M2 — the transaction guard removed: `rederive` returns `Ok(())` | `the_writer_refuses_a_forged_or_stale_preparation`: "an out-of-policy radius was written" (the first forged preparation in its list) |

Under M2 the CLI gates still passed: no CLI route can hand the writer a
forged preparation, which is why the guard is tested in the document crate.
After restoring, the document (3), CLI `--test fillet` (12) and app
`fillets::` (4) tests passed again.

## CI

The code, CI files and tools are those of `7df0d96`. The head that adds this
section changes only this record, so it triggers ordinary CI and no native
runtime run (`docs/**` is outside `runtime-layout.yml`'s and
`planegcs-pin.yml`'s path filters); the native tree is the one measured below.

* **Native runtime and packaging on `7df0d96`**
  ([run 36326313381](https://github.com/gesriot/ferrite-cad/actions/runs/36326313381)):
  green on all three platforms, along with the cross-platform comparison
  ([Linux](https://github.com/gesriot/ferrite-cad/actions/runs/36326313381/job/108639649013),
  [macOS](https://github.com/gesriot/ferrite-cad/actions/runs/36326313381/job/108639649146),
  [Windows](https://github.com/gesriot/ferrite-cad/actions/runs/36326313381/job/108639649144)).
  That covers the no-solver step (two radius gates and
  `FCAD_28B_RECIPE_OK`), the native Fillet step (the five `radius::`, three
  `fillet_radius::` and two app radius gates by exact name) and the FBX step
  (`FCAD_FILLET_RADIUS_UFBX_EXECUTED`). Each of those steps exits 1 on a
  missing `ok`, a `skipped:` or a missing marker. The job logs themselves
  were not read here: this container's proxy refuses the log download.
  Windows passed "Require one owner for every staged file" with the §28A
  TKBool/TKFillet rows unchanged.
* **Ordinary CI on `7df0d96`**
  ([run 36326331384](https://github.com/gesriot/ferrite-cad/actions/runs/36326331384)):
  lint, supply-chain, sbom, notices and the tests on Linux, macOS and
  Windows succeeded, including the new stub step with the recipe expecting
  `FCAD_28B_RECIPE_NO_KERNEL`.
* **planegcs pin on `7df0d96`**
  ([run 36326313378](https://github.com/gesriot/ferrite-cad/actions/runs/36326313378)):
  success on all three platforms.
* rust sbom, rust notices and product sbom are not triggered by this
  change's paths on a pull request.

## Limits

* Only the radius of the one saved §28A Fillet, into a new copy. Not
  supported: a second Fillet, another edge, radius 0 as deletion, cap
  edges, chains, a variable radius, Chamfer, editing the Sketch or height
  under a Fillet, a preview, mouse picking and in-place Save. The 5C
  milestone is not complete.
* The radius bounds are §28A's measured policy on OCCT 8.0.1.
* A mesh check is chord-bounded, not exact. The exact claims are the B-Rep's
  (volume, surface, radius and axis).
* No window, GPU or browser test was run here. The §28A review on the
  reviewer's Mac compared 149 SQL cells with a viewer peak of 197.55 MiB;
  nothing like it was run for this slice. The historical fillet-corpus OOM
  is still not explained.

## macOS fixtures and window scenario (for the reviewer's Mac)

Nothing here was run in this cloud container, which has no window system.
The generator and comparator use only the staged CLI. They were exercised
here with a second CLI copy standing in for the window's output, which says
nothing about the window: that passed with 129 cells, and a tampered
`objects.name` cell was caught.

### Fixture generator

It writes `rounded.fcad` (a plate with the §28A Fillet, r = 2.375 mm at
(33, 3.25)), the peer request and `facts.json` into a new directory:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-fillet-radius-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28B_GUI_FIXTURE\n", "ferrite-28b-fixture.py"),
                   ("# FCAD_28B_GUI_COMPARE\n", "ferrite-28b-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28b-fixture.py "$FCAD_28B_DIR"
```

```python
# FCAD_28B_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-radius-gui").resolve()
out.mkdir(parents=True, exist_ok=False)
def run(*args):
    p = subprocess.run([cli, *map(str, args)], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return json.loads(p.stdout)
X0, Y0, W, D, H = -4.5, 3.25, 37.5, 12.25, 6.75
request = out / "request.json"
request.write_text(json.dumps({"request_version": 1, "height_mm": H,
    "points_mm": [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]]}))
plate = out / "plate.fcad"
run("create-sketch-extrude", request, "-o", plate, "--json")
catalog = run("inspect", plate, "--json")["result"]
body = catalog["bodies"][0]
chosen = next(c for c in body["fillet_edge"]["target"]["candidates"]
              if c["corner_mm"] == [X0 + W, Y0])
request.write_text(json.dumps({"request_version": 1, "edge": chosen["edge"], "radius_mm": 2.375}))
source = out / "rounded.fcad"
run("fillet-edge-copy", plate, "--body", body["body_id"], "--expect-version",
    catalog["content_version"], "--request", request, "-o", source, "--json")
request.unlink()
plate.unlink()
catalog = run("inspect", source, "--json")["result"]
(row,) = catalog["fillets"]
assert row["radius_edit"]["available"] is True, row
(out / "peer-request.json").write_text(json.dumps({"request_version": 1, "radius_mm": 4.8125}))
(out / "facts.json").write_text(json.dumps({
    "feature_id": row["feature_id"], "content_version": catalog["content_version"],
    "corner_mm": row["corner_mm"], "joint": row["edge"]["joint"],
    "radius_mm": 2.375, "new_radius_mm": 4.8125,
    "max_radius_mm": row["radius_edit"]["max_radius_mm"],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28B_GUI_FIXTURE_OK", out)
```

### Window scenario

Run `ferritecad-viewer` against the staged OCCT under the usual watchdog.

1. **Open.** Open `$FCAD_28B_DIR/rounded.fcad`. In **Saved objects**, the
   button **Edit Fillet radius Fillet — …** is enabled, and **Fillet edge**
   is disabled with a hover text naming the Fillet.
2. **Form.** Press it. The window "Edit Fillet radius — new copy" says the
   edge cannot be changed here, shows "Edge: Corner (33, 3.25) — Lines … |
   …; sides 37.5 × 12.25 mm; r ≤ 6.125 mm" and "Saved radius 2.375 mm; from
   0.01 mm to 6.125 mm here."
3. **Refusal.** Enter `7` and press **Apply radius**: it is refused with the
   numbers, and no **Save** appears.
4. **Apply.** Enter `4.8125` and press **Apply radius**. The window reads
   "Ready: radius 2.375 mm -> 4.8125 mm at (33, 3.25)".
5. **Save Cancel.** **Save radius copy…** → **Cancel**. The draft stays.
6. **Save and Open.** **Save radius copy…** → `$FCAD_28B_DIR/gui.fcad`. The
   viewer opens the copy asynchronously; the corner at (33, 3.25) has the
   larger round and the other three are sharp.
7. **Exports.** From the window, export **STL** and **FBX** of `gui.fcad`
   into `$FCAD_28B_DIR` as `gui.stl` and `gui.fbx`, at the default
   tessellation.
8. **Compare.** Quit the viewer normally, then run:

```sh
FERRITECAD=/path/to/ferritecad python3 ferrite-28b-compare.py "$FCAD_28B_DIR"
```

The compare step makes the peer copy with the same request and exports it.
It compares every table and every cell of `gui.fcad` with the peer's, with
no UUID mapping (nothing is minted), skipping only `meta.modified_at`. It
checks the SQL allowlist of `gui.fcad` against `rounded.fcad`, the Fillet's
UUID, edge, corner and new radius in `inspect`, and that `gui.stl` and
`gui.fbx` are byte-identical to the peer's. It never creates or overwrites a
`gui.*` file, and a missing one is a failure. It prints
`FCAD_28B_GUI_COMPARE_OK`.

```python
# FCAD_28B_GUI_COMPARE
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
    p = subprocess.run([cli, "edit-fillet-radius", source, "--feature", facts["feature_id"],
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
        cur = db.execute(f'SELECT * FROM "{t}"')
        out[t] = ([d[0] for d in cur.description], sorted(cur.fetchall(), key=repr))
    db.close()
    return out

fid = bytes.fromhex(facts["feature_id"].replace("-", ""))
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

# The window's copy against the peer's: identical but for the time stamp.
moved, compared = differing(gui, peer)
assert moved <= {("meta", "modified_at", 1)}, moved
# And against its source: the allowlist, and the radius really moved.
moved, _ = differing(source, gui)
assert {(t, c) for t, c, i in moved if t == "objects"} == {("objects", "payload"), ("objects", "payload_hash")}, moved
assert all(i == fid for t, c, i in moved if t == "objects"), moved
assert moved - {("objects", "payload", fid), ("objects", "payload_hash", fid)} <= {("meta", "modified_at", 1)}, moved
p = subprocess.run([cli, "inspect", gui, "--json"], capture_output=True, encoding="utf-8")
(row,) = json.loads(p.stdout)["result"]["fillets"]
assert row["feature_id"] == facts["feature_id"] and row["radius_mm"] == facts["new_radius_mm"], row
assert row["edge"]["joint"] == facts["joint"] and row["corner_mm"] == facts["corner_mm"], row
for fmt in ("stl", "fbx"):
    assert (out / f"gui.{fmt}").read_bytes() == (out / f"peer.{fmt}").read_bytes(), fmt
print("FCAD_28B_GUI_COMPARE_OK", f"cells={compared}", f"radius={row['radius_mm']}")
```
