# §28H — verification record

[Contract and recipe](edit-sequential-fillet-radii.md).

## Where and how this was run

* **Base.** `origin/main` at `649554395ae6d2275d523e3ce913fe9a14aa0963`
  (§28G, PR #71 squash-merged; its tree is the reviewed head `aac3c86`). The
  branch `edit-sequential-fillet-radii` was created from it with a clean tree.
  Its merge-triggered workflows are recorded under [CI](#ci), separately from
  this change's runs.
* **Cloud container.** Linux x86_64, the session's existing OCCT 8.0.1 install
  and native/stub targets, one heavy build at a time, `CARGO_BUILD_JOBS` 2–4.
  OCCT, Boost and PlaneGCS were not rebuilt.
* **PlaneGCS: local, unpinned.** Every local solver result below is from the
  library an earlier slice (§27H) built from FreeCAD 1.0.1 `planegcs` sources
  with Ubuntu's Eigen 3.4.0 and Boost 1.83 — not the pinned delivery. **The
  named CI gates on Linux, macOS (Apple Silicon) and Windows with the pinned
  delivery are the authoritative solver evidence.**
* **No window, GPU test, Unity, browser or large STEP corpus was run**, and
  `FCAD_ALLOW_LOADER_FAILURE_PROBES` was never set. The widget tests drive
  real egui widgets headlessly; that is not a window test. The window
  scenario below is for the user's Mac.

## What changed

* **document:** `read_history` takes the whole Fillet chain (each Fillet's
  `previous` is the one before it, the last is the Body tip);
  `saved_history_under_fillets`. `saved_fillet_for_radius` routes exactly two
  Fillets to `saved_sequential_fillet`, which re-reads the §28G class (no Cut,
  both edges on the base Extrude, Fillet 1 on the base, distinct corners,
  exactly the fifteen names by meaning) and returns the selected Fillet with
  its `history_index`, base and `NeighbourFillet`. `SavedFillet::check_radius`
  is the one validator: value only on a constrained plate; otherwise §28A's
  corner bound and `check_pair` in history order with the other radius as
  saved. `max_radius_mm` is `None` on a constrained plate, else the exact
  bound (`pair_bound` for Fillet 2, `pair_bound_of_first` — float stepping
  under the same predicate — for Fillet 1). `saved_fillet` (height, Sketch,
  constraints) still accepts exactly one Fillet; with two it now says only the
  radii can be edited. The writer is unchanged: it re-derives the preparation
  inside the transaction and writes one row's payload/hash and the stamp.
* **jobs / cli:** the result names `previous_feature_id` and
  `history_index`; discovery adds `fillets[].history_index` and
  `radius_edit.neighbour`.
* **app:** the button names the Fillet's place and radius; the form says which
  Fillet it edits, the other's radius and corner and the shared-side rule; a
  worker refusal is shown inside the form ("Could not save: …") as well as in
  the status line, and the draft is kept.
* No command, request version, payload version, capability, SQLite schema,
  archive format, solver, copier or geometric route was added.

## New tests

Document `fillet_radius::tests` (kernel-free):

* `either_sequential_radius_changes_alone_and_keeps_every_identity` — both
  Fillets up/down/equal, only the selected payload moves, every UUID and name
  kept, history indices and neighbours as stored.
* `the_pair_bound_is_exact_for_either_radius_and_absent_opposite` — r1 = 6,
  r2 = 6.12 on the 12.25 mm side: Fillet 1's bound is
  `pair_bound_of_first(12.25, 6.12)`, accepted, the next float refused, and
  the evaluator's own check agrees float for float; Fillet 2's is
  `min(12.25 − 6 − 0.01, 6.125)`; on an **8 mm** side beside r2 = 4 the bound
  is 3.9900000000000007, which the history-order predicate accepts and the
  same inequality solved for r1 would refuse (see M4); opposite corners: the
  corner bound alone.
* `the_writer_refuses_a_forged_sequential_radius_edit` — a forged radius past
  the pair bound, another predecessor, the other Fillet's edge, the other
  Fillet's row and a stale version are refused writing nothing; Fillet 1 at
  6.125 beside 6.12 is refused «flat» before any writer.

CLI `tests/fillet.rs`, module `sequential::radius`:

* `radius_discovery_and_protocol_without_native` — rows, indices,
  neighbours, bounds and the protocol per build (kernel: `input` for bad
  radii; no kernel: `unsupported` after parsing).
* `native_editing_either_radius_is_what_the_numbers_say` — CCW and CW
  windings, adjacent across 12.25 mm: Fillet 1 up/down, Fillet 2 up/equal to
  Fillet 1 (the two cylinders stay apart by UUID); opposite corners. Each copy:
  allowlist, refs unchanged, the other row byte-identical, validate, cold /
  Miss / Hit builds equal, OCCT faces by origin UUID and axis/radius, volume,
  independent STL and FBX.
* `native_the_pair_bound_is_exact_when_editing_the_first_radius` — the
  offered Fillet 1 bound publishes; the next float is refused `input`.
* `native_either_radius_edit_invalidates_exactly_its_suffix` — warm cache
  copied to the copy: editing Fillet 2 → Hit, Hit, Miss; Fillet 1 → Hit,
  Miss, Miss; then the cold build equals the warm one and a rerun is all Hit.
* `native_dimensioned_radius_edits_answer_to_the_solved_plate` — a
  dimensioned plate whose solved sides differ from the stored ones: an edit
  accepted by the stored numbers is refused by the solved pair, and the
  solved-room edits of both Fillets publish on the solved corners.
* `native_either_radius_refusals_cancellation_and_report_loss_are_atomic` —
  version, no-clobber and alias guards, cancellation, closed stdout and exit 7
  through the existing helpers.

App `fillets::tests`:

* `sequential_radius_widgets_name_the_fillet_and_keep_the_draft` — both
  buttons, the form texts, Fillet 1's exact pair bound (accepted; next float
  refused «flat»), exactly the widgets' request, a worker refusal shown in the
  form with the draft kept, Cancel; Fillet 2's own form (6.125 accepted, next
  float refused).
* `native_sequential_radius_worker_and_cli_publish_the_same_part` — the app's
  worker and the shipped CLI: every SQL cell equal but `meta.modified_at`,
  STL and FBX byte-equal.

## Local results

Solver build (OCCT 8.0.1 + the unpinned PlaneGCS), debug:

* `tests/fillet.rs`: REGRESSION_FILLET; `fillet_radius::` 16 passed;
  app `fillets::` 10 passed.
* Every new gate executed by its packed argv (`cargo test … "$gate" --
  --exact --nocapture --test-threads=1`, `test $gate ... ok`, no `skipped:`):
  the `ci.yml` step extracted from the workflow file and run as a script in
  the stub build (1 CLI, 1 app, 3 document gates and the recipe marker), the
  runtime-layout mixed lines extracted and run against the OCCT-without-solver
  release build, and the native CLI/document/app gates.
* Recipes §28A–§28H against the solver CLI: all eight print
  `FCAD_28x_RECIPE_OK`. §28H:
  `FCAD_28H_RECIPE_OK f2_up=3038.078618/3038.355417
  f1_bound=6.12:2991.862982/2992.271179 both_down=3064.995944/3065.189619`
  (measured mesh / exact volume, mm³; Fillet 1's offered bound beside 6.12 is
  the float printed `6.12`).
* FBX: `tools/check-fbx-complex.sh --features planegcs` as CI runs it, over the
  artifacts of the whole `tests/fillet.rs` run: every Fillet marker including
  the new `FCAD_FILLET_RADII_UFBX_EXECUTED`; the six files
  `radius-ccw-f1-up`, `radius-ccw-f2-up`, `radius-cw-f1-down`,
  `radius-opposite-f2`, `radius-bound`, `radius-dimensioned-f1` read by
  pinned ufbx 0.23.0 (`checks=6 failures=0` each) and joined with their STL
  (`FCAD_STL_FBX_MATCH`); 52 joins in all.
* fmt, workspace clippy (`--all-targets --all-features -D warnings`) and
  `git diff --check`: clean.

REGRESSION_SUMMARY

OCCT without PlaneGCS (`--no-default-features`,
`FERRITECAD_REQUIRE_PLANEGCS=0`, release, the CI argv):

MIXED_SUMMARY

Stub (no OCCT; CMake through an explicit toolchain file, native prefixes
ignored):

STUB_SUMMARY

### Mutations — local, executed, restored byte for byte

Each is a compiled change run against the new tests, then the file restored
from a copy and checked by `sha256sum -c`; the positive checks were rerun
after restoring (the regression above ran on the restored tree).

* **M1 — suffix: Fillet 2 keyed by the base, not by Fillet 1**
  (`cold.rs`: the fillet archive key takes the edge producer's key instead of
  the predecessor's). Caught by
  `native_either_radius_edit_invalidates_exactly_its_suffix`: after Fillet 1
  is edited, Fillet 2 answers Hit where Miss is required (a stale Fillet 2
  over the old Fillet 1 in the final Body). The other five pass: cold builds
  do not use the key.
* **M2 — writer guard compares identity only** (`rederive`: `checked.feature.id
  != prepared.feature.id` instead of the whole preparation). Caught by
  `the_writer_refuses_a_forged_sequential_radius_edit` and §28B's
  `the_writer_refuses_a_forged_or_stale_preparation`.
* **M3 — no pair check in preparation** (`check_radius` ignores the
  neighbour). Caught by `the_pair_bound_is_exact_for_either_radius_and_absent_opposite`
  and the writer test. The CLI tests pass under M3 — the copy's rebuild
  refuses the same radii atomically — which is the defence in depth the
  contract states, not evidence for M3.
* **M4 — the pair check solved for r1 when editing Fillet 1**
  (`check_pair(neighbour, r2, self, r1)`, i.e. `r1 ≤ L − r2 − 0.01`). First
  run: **survived** every test, because on the 12.25 mm fixtures the two
  forms agree at every float reachable under §28A's bound (checked
  exhaustively for r2 ∈ [6.115, 6.125] in 1e-6 steps). A search found that
  they differ on an 8 mm side (L = 8, r2 = 4 → offered r1 3.9900000000000007);
  that case was added to the domain test, and M4 rerun is caught there
  (`validate(max1)` refused). Counted only after the added case.

### Compatibility with the reader on `main`

`6495543`, extracted with `git archive` and built (release, PlaneGCS) in the
scratchpad (`FCAD_28H_COMPAT_OK`):

* A copy this build edited (Fillet 1 → 4.8125): the old reader's `inspect`
  lists both Fillets with their radii (no `history_index`, radius edits
  unavailable), `validate` valid, cold `rebuild` resolves 18 of 18 names; its
  STL is byte-identical to this build's; its `edit-fillet-radius` refuses
  `unsupported` («this slice edits a plate with one Fillet, and this document
  holds 2 …») without output; the file's SHA-256 is unchanged. §28H writes
  nothing an old reader cannot read: no payload version or capability moved.
* A §28G document written by the old build: this build discovers indices 1
  and 2, edits Fillet 2 (result `history_index` 2, previous radius 3.0625),
  the source unchanged, and a cold rebuild resolves 18 of 18.
* §28G's recipe and GUI comparator asserted that a two-Fillet copy offers no
  radius edit; the recipe now asserts both are offered, and the §28G
  comparator accepts either answer by build (`history_index` present or not),
  so it still judges a §28G bundle.

## CI

CI_SECTION

## Limits

* The class: exactly §28G's two Fillets, unconstrained or carrying the managed
  Line family. Not supported: a third Fillet, editing the height, Sketch or
  constraints of a two-Fillet history, retargeting a corner, Cut with Fillet,
  Chamfer, picking, preview, in-place Save.
* Discovery cannot bound a constrained plate; the copy's rebuild does, before
  publication.
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
the window: `FCAD_28H_GUI_COMPARE_OK cells=382`; without the window's files
the comparator stopped with `FCAD_28H_GUI_COMPARE_MISSING …` and created
nothing; a tampered `objects.name` of Fillet 2 in `gui-f2.fcad` was caught
(`AssertionError: objects.name`); a stray `never.fcad` was caught.

Use the bundled CLI, `FerriteCAD.app/Contents/MacOS/ferritecad`, on an Apple
Silicon (arm64) Mac, and do not set `FCAD_ALLOW_LOADER_FAILURE_PROBES`.

### Fixture generator

It writes `twice.fcad` — §28G's plate drawn clockwise from (33, 15.5),
37.5 × 12.25 × 6.75 mm as stored, dimensioned by the shipped command to
41 × 14.25 mm with Segment 1's start fixed at (36.5, 17.75), and rounded by
`fillet-edge-copy` twice: Fillet 1 at the stored corner (33, 3.25) (solved
(36.5, 3.5)) with r 2 mm, Fillet 2 at (33, 15.5) (solved (36.5, 17.75)) with
r 7.12 mm — and `facts.json`:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-sequential-fillet-radii-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28H_GUI_FIXTURE\n", "ferrite-28h-fixture.py"),
                   ("# FCAD_28H_GUI_COMPARE\n", "ferrite-28h-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
APP=/path/to/FerriteCAD.app
export FCAD_28H_DIR="$PWD/fillet-radii-gui"
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28h-fixture.py "$FCAD_28H_DIR"
```

```python
# FCAD_28H_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-radii-gui").resolve()
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
# Fillet 1 r 2 mm, then Fillet 2 r 7.12 mm beside it: 7.12 <= 14.25 - 2 - 0.01.
once = out / "once.fcad"
fillet(dimensioned, FIRST, 2.0, once)
source = out / "twice.fcad"
fillet(once, SECOND, 7.12, source)
for p in (request, plate, dimensioned, once):
    p.unlink()
catalog = run("inspect", source, "--json")["result"]
by = {f["history_index"]: f for f in catalog["fillets"]}
assert sorted(by) == [1, 2], catalog["fillets"]
for i in (1, 2):
    edit = by[i]["radius_edit"]
    assert edit["available"] is True and edit["max_radius_mm"] is None, edit
    assert edit["neighbour"]["stored_shared_length_mm"] == 12.25, edit
assert catalog["bodies"][0]["fillet_edge"]["available"] is False, "no third Fillet"
(out / "facts.json").write_text(json.dumps({
    "body_id": catalog["bodies"][0]["body_id"], "content_version": catalog["content_version"],
    "first_fillet_id": by[1]["feature_id"], "second_fillet_id": by[2]["feature_id"],
    "radii_mm": [2.0, 7.12], "refused_first_radius_mm": 7.125, "first_radius_mm": 7.1,
    "second_radius_mm": 3.5, "height_mm": H, "gui_rect_mm": [-4.5, 3.5, 41.0, 14.25],
    "gui_corners_mm": [[36.5, 3.5], [36.5, 17.75]],
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28H_GUI_FIXTURE_OK", out)
```

### Window scenario

One viewer under the 1536 MiB watchdog, without DYLD variables:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log "$FCAD_28H_DIR/../watch-28h.jsonl" --limit-mib 1536 --seconds 1800 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

1. **Open** `$FCAD_28H_DIR/twice.fcad` (asynchronously). Two enabled rows:
   **Edit Fillet radius Fillet — <first_fillet_id> (Fillet 1 of 2, r2 mm)…**
   and **… — <second_fillet_id> (Fillet 2 of 2, r7.12 mm)…**. **Fillet edge
   of …** is refused ("third"); the height, Sketch and constraint editors of
   the base refuse, their reasons saying that with two Fillets only the radii
   can be edited.
2. **Fillet 1's form.** It reads "Editing Fillet 1 of 2: Extrude <UUID> →
   Fillet <first> → Fillet <second>. The other Fillet (<second>) keeps
   r7.12 mm at (33, 15.5).", "Shares Line <UUID> (12.25 mm as stored) with the
   other Fillet: the two radii must leave at least 0.01 mm of it flat." and
   the constrained-plate notice (no upper bound is offered: the stored drawing
   does not decide it).
3. **Value refusal.** Radius `0.005`, **Apply radius**: refused, naming
   0.01 mm; nothing applied.
4. **Save Cancel.** Radius `7.125`, **Apply radius** (accepted as a value).
   **Save radius copy…** → **Cancel** in the file dialog: nothing starts; the
   draft stays.
5. **Wrong pair bound keeps the draft, in the form.** **Save radius copy…** →
   `$FCAD_28H_DIR/never.fcad`. The job is refused; the form stays open with
   `7.125` and shows, **inside the form**, "Could not save: invalid input: as
   the plate is built, fillets of 7.125 mm and 7.12 mm at the two ends of Line
   <UUID> (14.25 mm) would leave 0.004999999999999893 mm of it flat; …"; the
   status line says the same. `never.fcad` does not exist.
6. **Recovery, publish, async Open.** Radius `7.1`, **Apply radius**, **Save
   radius copy…** → `$FCAD_28H_DIR/gui-f1.fcad`. The viewer opens the copy
   asynchronously; the rows now read r7.1 mm and r7.12 mm.
7. **Fillet 2's form.** Press the Fillet 2 row: "Editing Fillet 2 of 2 …
   The other Fillet (<first>) keeps r7.1 mm at (33, 3.25)." Radius `3.5`,
   **Apply radius**, **Save radius copy…** → `$FCAD_28H_DIR/gui-f2.fcad`;
   it opens asynchronously; the top view shows the 41 × 14.25 mm plate with
   (36.5, 3.5) rounded by 7.1 mm and (36.5, 17.75) by 3.5 mm, nothing else.
   There is no Undo/Redo for this edit.
8. **Exports.** From the window export `gui-f2.stl` and `gui-f2.fbx` of
   `gui-f2.fcad` into `$FCAD_28H_DIR`, at the default tessellation.
9. **Quit** normally; read only that PID's exit and the watchdog log; then:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 ferrite-28h-compare.py "$FCAD_28H_DIR"
```

It first requires `gui-f1.fcad`, `gui-f2.fcad`, `gui-f2.stl` and
`gui-f2.fbx` (and never creates them), refuses a `never.fcad`, checks the
source's SHA-256, the allowlist of each window edit and the radii by
`history_index`, makes the CLI peer (`peer-f1.fcad`, `peer-f2.fcad` and its
exports, only if absent), compares every SQL cell but `meta.modified_at` —
this edit mints no UUID, so nothing is mapped — and requires byte-equal
STL/FBX, then reads the window's STL itself: the solved rectangle and height,
exactly the two solved corners rounded, each wall at its own radius about its
own axis. It prints `FCAD_28H_GUI_COMPARE_OK cells=N`.

```python
# FCAD_28H_GUI_COMPARE
import hashlib, json, math, os, pathlib, sqlite3, struct, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1]).resolve()
facts = json.loads((out / "facts.json").read_text())
# The window's files come first; this script never makes them.
GUI = ("gui-f1.fcad", "gui-f2.fcad", "gui-f2.stl", "gui-f2.fbx")
missing = [n for n in GUI if not (out / n).is_file()]
if missing:
    sys.exit(f"FCAD_28H_GUI_COMPARE_MISSING {' '.join(missing)}: run the window scenario first")
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
    """Only that Fillet's payload/payload_hash and meta.modified_at moved."""
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
def radii(path):
    rows = run("inspect", path, "--json")["result"]["fillets"]
    return {f["history_index"]: (f["feature_id"], f["radius_mm"]) for f in rows}
F1, F2 = facts["first_fillet_id"], facts["second_fillet_id"]
r1, r2 = facts["first_radius_mm"], facts["second_radius_mm"]
# The window's copies against their sources, and what they say.
allowlist(source, out / "gui-f1.fcad", F1)
allowlist(out / "gui-f1.fcad", out / "gui-f2.fcad", F2)
assert radii(out / "gui-f1.fcad") == {1: (F1, r1), 2: (F2, facts["radii_mm"][1])}
assert radii(out / "gui-f2.fcad") == {1: (F1, r1), 2: (F2, r2)}
# The same two edits through the shipped CLI, and its exports.
request = out / "peer-request.json"
def peer(src, feature, r, dest):
    if not dest.exists():
        request.write_text(json.dumps({"request_version": 1, "radius_mm": r}))
        version = run("inspect", src, "--json")["result"]["content_version"]
        run("edit-fillet-radius", src, "--feature", feature, "--expect-version", version,
            "--request", request, "-o", dest, "--json")
peer(source, F1, r1, out / "peer-f1.fcad")
peer(out / "peer-f1.fcad", F2, r2, out / "peer-f2.fcad")
for fmt in ("stl", "fbx"):
    if not (out / f"peer-f2.{fmt}").exists():
        run(f"export-{fmt}", out / "peer-f2.fcad", "-o", out / f"peer-f2.{fmt}", "--json")
cells = same(out / "gui-f1.fcad", out / "peer-f1.fcad") + same(out / "gui-f2.fcad", out / "peer-f2.fcad")
for fmt in ("stl", "fbx"):
    assert (out / f"gui-f2.{fmt}").read_bytes() == (out / f"peer-f2.{fmt}").read_bytes(), fmt
# The window's mesh, read here: the solved plate with exactly its two solved
# corners rounded, each wall at its own radius about its own axis.
data = (out / "gui-f2.stl").read_bytes()
(count,) = struct.unpack_from("<I", data, 80)
assert len(data) == 84 + 50 * count
pts = [struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * k) for i in range(count) for k in range(3)]
x0, y0, w, d = facts["gui_rect_mm"]
xs, ys, zs = ([p[k] for p in pts] for k in range(3))
for got, want in ((min(xs), x0), (max(xs), x0 + w), (min(ys), y0), (max(ys), y0 + d),
                  (min(zs), 0.0), (max(zs), facts["height_mm"])):
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
print("FCAD_28H_GUI_COMPARE_OK", f"cells={cells}")
```
