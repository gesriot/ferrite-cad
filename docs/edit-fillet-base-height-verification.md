# §28C — verification record

[Contract and agent recipe](edit-fillet-base-height.md).

This records what was executed for this slice and where. It does not repeat
§28A's or §28B's evidence, nor the reviewer's §28B GUI and CI measurements.

## Independent review on macOS, 2026-09-27

Reviewed PR #64 at `9c670f6eb3c497121cb8406ece3af81bbd27df36`, including the
normal merge of the Apple Silicon-only policy from PR #63. No production-code
fix was needed. The review checked the shared Fillet frame, transactional
re-derivation, strict reference resolution, additive JSON fields, SQL write
allowlist, and preservation of existing runtime gates.

CI attribution is explicit: the native code and workflows are identical to
`f086e805fdb7138f9cc5fd669ff45acfee5b0c69`; the change from that commit to
`9c670f6` is this verification document only. On `f086e80`, CI run
[36339637257](https://github.com/gesriot/ferrite-cad/actions/runs/36339637257),
PlaneGCS pin run
[36339634624](https://github.com/gesriot/ferrite-cad/actions/runs/36339634624),
and runtime run
[36339634617](https://github.com/gesriot/ferrite-cad/actions/runs/36339634617)
all succeeded. The downloaded runtime logs contain all ten new distinct gate
names on each OS, with fourteen executions per OS including repeats in the
mixed and app steps; the recipe and height-reader markers are present. Each
OS log has 106 visible successful ufbx markers, with no failures; triangle
reader output redirected to files is not included in that marker count.
The seven ordinary CI jobs on the documentation head `9c670f6` also passed.
The documentation head did not trigger another native runtime workflow.

Local builds used the existing OCCT 8.0.1 and PlaneGCS installations and the
existing native target, one build at a time, with both native requirements
enabled. The CLI and viewer were rebuilt in release mode. Document/jobs/eval
reported 459 passes and one existing ignored benchmark; two stub-only refusal
tests explicitly reported N/A because a solver was present. The CLI Fillet,
edit-extrude and JSON suites reported 32 passes; app Fillet and edit worker
tests reported 4 + 11 passes, with no native skips. Workspace clippy with all
targets/features and `-D warnings`, fmt and diff whitespace checks passed.
The extracted public recipe completed with `FCAD_28C_RECIPE_OK`. This local
review did not repeat the large STEP campaign or rebuild native libraries.

A fresh bundle passed the arm64 check for all 53 Mach-O files, strict deep
code-sign verification, bundled CLI startup and viewer `--solver-info`.
One viewer, PID 25788, ran under the 1536 MiB watchdog. The actual window and
system Save panels demonstrated:

- opening the generated rounded plate and selecting its base Extrude;
- the saved Fillet UUID, corner and radius in the height form;
- zero height refused with Save disabled;
- entering 11.4375 mm, cancelling Save, and retaining that draft;
- saving `gui.fcad`, automatic Open with the new window title, and the taller
  rounded part visible in isometric view;
- exporting `gui.stl` and `gui.fbx` through the window, then normal Quit.

After the first system Save panel, CUA coordinate clicks failed with
`noWindowsAvailable`; the same viewer remained alive and responsive. Tab/Space
navigation completed the form and export actions, and accessibility actions
operated the system panels. A mistyped Save filename was corrected and checked
before publication. These automation failures were not application crashes.
After Quit, only the owned PID was checked; no app lookup relaunched it.

The extracted comparator consumed these actual GUI files (it does not create
them): `FCAD_28C_GUI_COMPARE_OK cells=129 height=11.4375`. GUI/CLI copies differ
only in the allowed timestamp; against the source, only the selected base
Extrude payload/hash and timestamp changed. The source hash stayed identical.
GUI and CLI STL/FBX bytes match exactly. The independent binary STL parser
measured 64 triangles, 3284 bytes, dimensions 37.5 × 12.25 × 11.4375 mm,
closed consistently oriented edges, and signed mesh volume 5240.133410 mm³
(analytic reference 5240.256600 mm³; tessellation is not exact B-Rep volume).
A freshly compiled pinned ufbx 0.23.0 reader reported six identity checks,
zero failures; its triangle output matched the STL under the documented
coordinate conversion: `FCAD_STL_FBX_MATCH triangles=64 worst_m=8.67e-19`.

Watchdog: peak footprint 209.96994 MiB, pressure normal throughout, swap 0
throughout, exit 0, no abort. This does not establish the cause or resolution
of the earlier OOM. Raw logs, fixtures, comparator output and measurements
were captured in `/private/tmp/ferrite-pr64-review` and copied to the review's
durable evidence directory before handoff.

## Where and how this was run

* **Base.** Freshly fetched `origin/main` at
  `30920eff1481a96187e09b4ea7ab43b61019708b`, the merge of PR #62. Its tree
  `f25861c…` equals the reviewed §28B head `5db8ba4`. The branch is
  `edit-fillet-base-height`, created from it; `edit-fillet-radius` had been
  deleted on the remote.
* **Merge-triggered CI on the base** is reported in [CI](#ci), separately
  from this change's runs.
* **Cloud container.** Linux x86_64, 4 CPUs, 15 GiB RAM, the session's
  OCCT 8.0.1 install and its existing native and stub targets, built one at
  a time. No PlaneGCS is linked here (`FERRITECAD_REQUIRE_PLANEGCS=0`); none
  is needed. OCCT and Boost were not rebuilt.
* **No window, GPU test or browser was launched.** The widget tests drive
  real egui widgets headlessly; that is not a window test.

## OCCT probe — measured before the policy was chosen

A temporary test (not committed) called the product's own
`GeometryKernel::fillet_edge` on the 37.5 × 12.25 mm plate at (−4.5, 3.25)
for r = 0.01, 3.0625 and 6.125 mm and heights from 1e-7 to 1e5 mm:

| height (mm) | result, for each r |
| --- | --- |
| 1e5, 1e3, 13.5, 6.75, 3.3125, 1, 0.5, 0.1, 0.01, 1e-3, 1e-4, 5e-5, 2e-5 | valid, 7 faces, `Cylinder{r}`, axis (0, 0, 1) r inward of the corner; volume against `(W·D − (1 − π/4)·r²)·h` within 2.5e-16 relative |
| 1e-5, 5e-6, 2e-6, 1e-6, 1e-7 | `fillet_edge` refused ("Open CASCADE could not round this edge") |

Plain extrusion succeeded at every one of these heights. So the edit adds no
height bound: the policy stays positive and finite, and the kernel's refusal
at 1e-5 mm and below reaches the user typed as `kernel`, with nothing
published (tested at 1e-6 mm).

## What changed

* **Document.**
  * `fillet_radius.rs`: `saved_fillet` (the §28B frame and seven-name check)
    is shared, and `fillet_over_plate` returns the one Fillet over the plate,
    `None` without one, or the frame's reason. Two messages were made neutral
    ("this slice edits a plate with one Fillet…", "…with a Fillet and no
    Cut"); no test pinned them.
  * `height_edit.rs`: `prepare_extrude_height` reads that frame when the
    history has a Fillet, accepts only its `previous`, and carries the
    `SavedFillet` in `PreparedExtrudeHeight::fillet`. The Cut-history branch
    is unchanged. `rederive` compares the whole value, Fillet included.
  * `document.rs`: `write_extrude_height` takes the in-place branch for a Cut
    history or a Fillet.
  * `edit.rs`: `ExtrudeChoice::fillet` (context), the base row's refusal
    lifted only on the supported frame, and `filleted` kept for a Fillet
    outside it with the frame's reason appended.
  * `fillet.rs`: `refuse_filleted`'s wording now names both allowed edits.
    Every other editor still calls it.
* **Jobs.** `requires_resolved_references` names the exemption for a height
  edit with neither a Cut history nor a Fillet.
* **CLI.** The additive `features[].fillet_base`.
* **App.** The base row's context line in the existing form.
* **Tools.** The Fillet block of `check-fbx-complex.sh` reads six more
  exports (`FCAD_FILLET_HEIGHT_UFBX_EXECUTED`) and now sets its own
  interpreter. Running that block alone failed on `$python: unbound
  variable`: since §28A it relied on a Revolve directory having been given as
  well, which CI always does and a local run need not.
* **Unchanged**, with an empty diff against the base: the kernel, the OCCT
  crate and bridge (C++ and CMake), topology, evaluation, `vendor/`,
  `tools/native/`, `sbom/` and `Cargo.lock`. No capability, schema, payload
  version, cache key, archive tag or command was added.

Before this record: 18 files changed against the base, 1865 insertions and
53 deletions; this record adds its own file.

## New and changed tests

* **Document** (`fillet_radius::tests`, kernel-free):
  * `the_base_height_changes_alone_and_interleaves_with_the_radius` —
    discovery (the base row editable with the Fillet as context, no Cut
    history, bad heights refused, h < r accepted); then 9.5, 0.40625 (< r),
    the same, a radius edit, 13.1875. After each: every SQL cell but the one
    row's payload/hash and the stamp, the Fillet row, every name, `validate`.
  * `the_height_writer_refuses_a_forged_or_stale_preparation` — a
    preparation without the Fillet (which would take the legacy writer and
    its weaker reference promise), another Fillet radius or UUID, a reversed
    plate, another profile, another name, the Fillet's row, another version,
    and a Fillet name deleted after preparation: each refused, nothing
    written.
  * `a_height_under_a_fillet_outside_the_frame_is_refused_with_its_reason` —
    bad heights, the Fillet's UUID, a foreign UUID, an extra Fillet name, a
    moved tip; the other editors still name the Fillet.
  * `fillet::tests::one_fillet_is_written_as_the_new_tip_and_a_forged_one_is_refused`
    changed: it asserted that the height editor refuses a filleted plate,
    which is what this slice reverses; it now asserts the height is
    available and the other editors still refuse.
* **CLI** (`crates/ferritecad-cli/tests/fillet.rs`, `mod height`):
  * `height_discovery_and_protocol_without_native` — `fillet_base` null on a
    plain plate and exact on a rounded one, `base_height_edit*` null,
    `edit_extrude.available`; the other editors naming the Fillet. Zero,
    negative, NaN, infinite heights, the Fillet's and the Sketch's UUID and a
    foreign UUID: their own kinds with a kernel, `unsupported` naming Open
    CASCADE without one; exit 7 on a lost refusal report; nothing written.
    An extra Fillet name: `available` false naming the Fillet and "more
    faces", and the command refused the same way.
  * `native_height_edits_keep_the_named_cylinder_and_every_identity` — three
    fixtures (CCW, CW from the upper right, CCW from the third corner), two
    corners; 6.75 → 11.4375 → 1.1875 mm (< r). For every copy: the result,
    the SQL allowlist, every name, the Fillet row via `fillets[]`,
    `validate`, a cold `rebuild` (10 of 10), cold/Miss/Hit equality, 7 faces,
    the B-Rep volume `(W·D − (1 − π/4)·r²)·h` to 1e-9, `Cylinder{r}` under the
    same `EdgeFilletFace` on a vertical axis at the same XY, the carried caps
    and sides as planes, the independently read STL (spanning 0..h, only the
    chosen corner rounded, the volume within the chords' bound) and a
    complete FBX.
  * `native_the_same_height_publishes_the_same_model` — identical payload,
    identical measurement; the text mode's "saved … feature …".
  * `native_a_changed_height_misses_in_place_and_interleaves_with_the_radius`
    — one cache under one path: Miss, Hit, then the edited copy at the same
    path: the base Extrude and the Fillet both miss (no event hits), the new
    exact volume is measured, then Hit, equal to cold. Then
    `edit-fillet-radius` on the height-edited copy, and `edit-extrude` on
    that, with the same UUIDs and the exact volume.
  * `native_height_refusals_races_cancellation_and_report_loss_are_atomic` —
    source, occupied and hard-link outputs; 1e-6 mm (`kernel`); cancellation
    before the job and at its last barrier; an output race; a stale source; a
    base name that resolves to nothing (discovery accepts the source, the
    copy is refused `topology`, "unresolved"); a lost report after
    publication (exit 7, the copy kept).
  * `native_a_filleted_copy_and_other_histories_are_refused_by_name` (§28A)
    changed the same way as the document test: `edit_extrude` is available,
    and `edit-extrude` on the Fillet's own UUID is refused.
* **App** (`edits::tests`):
  * `fillet_base_height_widgets_show_the_fillet_and_keep_the_draft` —
    kernel-free: the form opens, the row's context names the Fillet, bad
    drafts are refused, a Save Cancel runs nothing, the request is the base
    Extrude at 0.40625 mm, and a stale reply and a worker refusal keep the
    draft.
  * `native_fillet_base_height_widgets_worker_and_cli_publish_the_same_part`
    — widgets, the real worker and async Open (a failed Open restores the
    draft, the accepted scene has the new height under the same Fillet),
    then the peer `edit-extrude`: every SQL cell equal but the stamp, no UUID
    mapped, and byte-identical STL and FBX.
* **CI.**
  * **`ci.yml`**, stub: the discovery gate, the widget gate, the three
    document gates, and the recipe expecting `FCAD_28C_RECIPE_NO_KERNEL`.
  * **`runtime-layout.yml`**, the native Fillet step on three OSes: the five
    `height::` gates, the three document gates and the two app gates by exact
    name, failing on `skipped:`; six more exports for the FBX loop.
  * **No-solver step** (`--no-default-features`): two native height gates
    and the recipe expecting `FCAD_28C_RECIPE_OK`.
  * **Packed argv**: the FBX step greps `FCAD_FILLET_HEIGHT_UFBX_EXECUTED`.
  * The Windows TKBool/TKFillet rows are unchanged; no library set changed.

## Local results

Native (OCCT 8.0.1, no PlaneGCS, debug):

* `--test fillet`: 17 passed (7 §28A, 5 §28B, 5 §28C). Document `fillet`
  tests: 12 passed. App `fillets::` and `fillet_base_height`: 6 passed.
  `edit_extrude` 2 and `json_v1` 13 passed.
* `--no-default-features`: the four native `height::` gates passed.
* Recipe: `FCAD_28C_RECIPE_OK up=5240.133408/5240.256600
  down=544.057567/544.070357` (mesh volume against the exact B-Rep one).
* **FBX.** The whole `tools/check-fbx-complex.sh` was run as CI runs it, with
  the §28A, §28B and §28C Fillet artifacts (36 files):
  `FCAD_FILLET_UFBX_EXECUTED`, `FCAD_FILLET_RADIUS_UFBX_EXECUTED`,
  `FCAD_FILLET_HEIGHT_UFBX_EXECUTED`, 18 triangle joins, exit 0. The six
  height exports read `checks=6 failures=0` each and joined 64 triangles,
  worst 1.73e-18 m.
* **`validation_really_read_only_permissions`** passed under UID 65534
  (`setpriv --reuid=65534 --regid=65534 --clear-groups`); as root it fails,
  since root ignores the permission it tests.
* **Full workspace:** `cargo test --workspace --no-fail-fast` gave
  2115 passed, 19 failed and 2 ignored (§28B: 2105 passed; the 10 more are
  this slice's).
  * The 19 failures are exactly the set of the earlier baseline runs in this
    container, compared line by line: 18 tests that need a linked PlaneGCS
    while `FERRITECAD_REQUIRE_OCCT=1`, and the root-only
    `validation_really_read_only_permissions`, which passes under an
    ordinary UID (above).
  * None of them involves a Fillet or a height edit.
* `cargo fmt --all -- --check` is clean. `cargo clippy --workspace
  --all-targets -D warnings` is clean natively, and so is the stub target
  with `--all-features`.

Stub (no OCCT): `--test fillet` 17 passed, the native ones returning early
with `skipped:`; document 12 and app 6 passed; the recipe prints
`FCAD_28C_RECIPE_NO_KERNEL` with the typed `unsupported`.

### Mutations — local, executed, restored byte for byte

Each was applied by a script and compiled; the file was restored from `HEAD`
and its SHA-256 checked (`cache.rs` a7d4acbb…, `jobs/src/edit.rs`
8e480893…).

| Mutation | Caught by (executed) |
| --- | --- |
| M1 — stale cache: `eval.fillet.named` ignores the predecessor's key | `native_a_changed_height_misses_in_place…`: after the height changed, the Fillet answered `Hit` where `Miss` was due |
| M2 — the standalone exemption reaches a rounded plate: `requires_resolved_references` ignores the Fillet | `native_height_refusals…`: the copy with a dangling base name published (exit 0 where 2 was due) |

Under M1 the other four height gates passed: only a cache under one path can
serve an old solid. Under M2 the others passed: only a source with an
unresolved name tells the two rules apart. After restoring, the document
`fillet` (12), CLI `--test fillet` (17) and app (6) tests passed again.

## CI

The code, CI files and tools are those of `f086e80`. The head that adds this
section changes only this record, so it triggers ordinary CI and no native
runtime run (`docs/**` is outside `runtime-layout.yml`'s and
`planegcs-pin.yml`'s path filters).

* **The first head, `bdeee80`,** failed the stub step of ordinary CI
  ([run 36335714625](https://github.com/gesriot/ferrite-cad/actions/runs/36335714625)):
  `height::height_discovery_and_protocol_without_native` passed, but it chose
  its expected answers through `native()`, which prints `skipped:` in a build
  with no kernel, and the exact-name gate rightly fails on any `skipped:`.
  Reproduced locally (8 such lines); `b1e073a` decides the branch with
  `ferritecad_occt::is_available()` instead, and the gate, run exactly as the
  workflow runs it, passed. The native runtime runs on `bdeee80` and
  `b1e073a` were cancelled by the next push.
* **main `4b32ee3` (PR #63, macOS on Apple Silicon only) was merged in** as
  `f086e80`, an ordinary merge commit with no conflict. Four files were
  changed on both sides (`ci.yml`, `runtime-layout.yml`, the README, the
  plan); both groups of gates are kept. After the merge the Fillet tests (17
  CLI, 12 document, 6 app) passed again here; the architecture check refuses
  to run on this Linux host, as it should, and ran on macOS in CI.
* **Native runtime and packaging on `f086e80`**
  ([run 36339634617](https://github.com/gesriot/ferrite-cad/actions/runs/36339634617)):
  green on all three platforms and in the cross-platform comparison
  ([Linux](https://github.com/gesriot/ferrite-cad/actions/runs/36339634617/job/108677107335),
  [macOS](https://github.com/gesriot/ferrite-cad/actions/runs/36339634617/job/108677107114),
  [Windows](https://github.com/gesriot/ferrite-cad/actions/runs/36339634617/job/108677107248)).
  That covers the Fillet step with the five `height::`, three document and
  two app gates, the no-solver step with `FCAD_28C_RECIPE_OK`, the FBX step
  with `FCAD_FILLET_HEIGHT_UFBX_EXECUTED`, and #63's arm64 checks on macOS.
  Each of those steps exits 1 on a missing `ok`, a `skipped:` or a missing
  marker. The job logs were not read here: this container's proxy refuses
  the log download, so no count of gates is claimed from them.
* **Ordinary CI on `f086e80`**
  ([run 36339637257](https://github.com/gesriot/ferrite-cad/actions/runs/36339637257)):
  lint, supply-chain, sbom, notices and the tests on Linux, macOS and
  Windows succeeded, the new stub step included.
* **planegcs pin on `f086e80`**
  ([run 36339634624](https://github.com/gesriot/ferrite-cad/actions/runs/36339634624)):
  success on all three platforms.
* **Bases.** Every merge-triggered workflow on `30920ef` concluded success,
  including combined runtime layout
  ([run 36332420351](https://github.com/gesriot/ferrite-cad/actions/runs/36332420351)).
  On `4b32ee3` CI, planegcs pin, rust sbom, rust notices and product sbom
  succeeded, and so did combined runtime layout
  ([run 36339485744](https://github.com/gesriot/ferrite-cad/actions/runs/36339485744)).

## Limits

* Only the Blind height of the base under the one saved §28A Fillet, into a
  new copy. Not supported: the profile, the radius from this form, the
  edge, constraints or any other parameter; a second Fillet, cap edges, a
  Cut with a Fillet, a Revolve, an arbitrary plane, deleting or retargeting
  the Fillet; a preview, mouse picking, in-place Save. The 5C milestone is
  not complete.
* OCCT refuses a plate 1e-5 mm tall or less; that is its limit, measured,
  not a product policy.
* A mesh check is chord-bounded, not exact. The exact claims are the B-Rep's
  (volume, surface, radius, axis).
* The height is measured through the exact volume and the mesh's Z extent;
  the kernel API exposes no face bounds.
* No window, GPU or browser test was run here. The historical fillet-corpus
  OOM is still not explained.

## macOS fixtures and window scenario (for the reviewer's Mac)

Nothing here was run in this cloud container, which has no window system.
The generator and comparator use only the staged CLI. They were exercised
here with a second CLI copy standing in for the window's output, which says
nothing about the window: that passed over 129 cells, a tampered
`objects.name` cell was caught, and a missing `gui.stl` was refused without
anything being created.

### Fixture generator

It writes `rounded.fcad` (the plate with the §28A Fillet, r = 2.375 mm at
(33, 3.25)) and `facts.json` into a new directory:

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-fillet-base-height-verification.md").read_text(encoding="utf-8")
for mark, name in (("# FCAD_28C_GUI_FIXTURE\n", "ferrite-28c-fixture.py"),
                   ("# FCAD_28C_GUI_COMPARE\n", "ferrite-28c-compare.py")):
    Path(name).write_text(text.split(mark, 1)[1].split("\n```", 1)[0], encoding="utf-8")
EXTRACT
FERRITECAD=/path/to/ferritecad python3 ferrite-28c-fixture.py "$FCAD_28C_DIR"
```

```python
# FCAD_28C_GUI_FIXTURE
import hashlib, json, os, pathlib, subprocess, sys
cli = os.environ["FERRITECAD"]
out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "fillet-height-gui").resolve()
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
assert catalog["edit_extrude"]["available"] is True, catalog["edit_extrude"]
(base,) = [f for f in catalog["features"] if f["fillet_base"] is not None]
(out / "facts.json").write_text(json.dumps({
    "base_feature_id": base["feature_id"], "content_version": catalog["content_version"],
    "fillet_feature_id": base["fillet_base"]["fillet_feature_id"],
    "corner_mm": base["fillet_base"]["corner_mm"], "radius_mm": 2.375,
    "height_mm": H, "new_height_mm": 11.4375,
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
print("FCAD_28C_GUI_FIXTURE_OK", out)
```

### Window scenario

On the reviewer's Apple Silicon (arm64) Mac, run one `ferritecad-viewer`
against the staged OCCT under the 1536 MiB watchdog.

1. **Open.** Open `$FCAD_28C_DIR/rounded.fcad`. **Edit extrusion…** is
   enabled; **Fillet edge** is disabled with a hover text naming the Fillet.
2. **Form.** Press **Edit extrusion…** and choose the row
   "Extrude1 — … — 6.75 mm". It shows "Rounded by Fillet … at (33, 3.25),
   r 2.375 mm. The Fillet keeps its edge and radius; only the plate's height
   changes." and "Current distance: 6.75 mm".
3. **Invalid.** Enter `0`: "invalid input: extrude distance must be
   positive" appears and **Save new file…** is disabled.
4. **Draft.** Enter `11.4375`.
5. **Save Cancel.** **Save new file…** → **Cancel**. The draft stays.
6. **Save and Open.** **Save new file…** → `$FCAD_28C_DIR/gui.fcad`. The
   viewer opens the copy asynchronously: the plate is taller, and the corner
   at (33, 3.25) is still rounded with the same radius.
7. **Exports.** From the window, export **STL** and **FBX** of `gui.fcad`
   into `$FCAD_28C_DIR` as `gui.stl` and `gui.fbx`, at the default
   tessellation.
8. **Quit** the viewer normally, then run:

```sh
FERRITECAD=/path/to/ferritecad python3 ferrite-28c-compare.py "$FCAD_28C_DIR"
```

The compare step makes the peer copy with `edit-extrude` and the same
height, and exports it. It compares every table and every cell of `gui.fcad`
with the peer's, with no UUID mapping (nothing is minted), skipping only
`meta.modified_at`. It checks the SQL allowlist of `gui.fcad` against
`rounded.fcad`, the base row's height and Fillet context in `inspect`, and
that `gui.stl` and `gui.fbx` are byte-identical to the peer's. It never
creates or overwrites a `gui.*` file, and a missing one is a failure. It
prints `FCAD_28C_GUI_COMPARE_OK`.

```python
# FCAD_28C_GUI_COMPARE
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
    p = subprocess.run([cli, "edit-extrude", source, "--feature", facts["base_feature_id"],
                        "--distance-mm", str(facts["new_height_mm"]), "--expect-version",
                        facts["content_version"], "-o", peer, "--json"],
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

base = bytes.fromhex(facts["base_feature_id"].replace("-", ""))
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
# And against its source: only the base Extrude's payload and hash moved.
moved, _ = differing(source, gui)
assert {(t, c) for t, c, i in moved if t == "objects"} == {("objects", "payload"), ("objects", "payload_hash")}, moved
assert moved - {("objects", "payload", base), ("objects", "payload_hash", base)} <= {("meta", "modified_at", 1)}, moved
p = subprocess.run([cli, "inspect", gui, "--json"], capture_output=True, encoding="utf-8")
result = json.loads(p.stdout)["result"]
(row,) = [f for f in result["features"] if f["feature_id"] == facts["base_feature_id"]]
assert row["distance_mm"] == facts["new_height_mm"], row
assert row["fillet_base"]["fillet_feature_id"] == facts["fillet_feature_id"], row
assert row["fillet_base"]["radius_mm"] == facts["radius_mm"], row
assert row["fillet_base"]["corner_mm"] == facts["corner_mm"], row
for fmt in ("stl", "fbx"):
    assert (out / f"gui.{fmt}").read_bytes() == (out / f"peer.{fmt}").read_bytes(), fmt
print("FCAD_28C_GUI_COMPARE_OK", f"cells={compared}", f"height={row['distance_mm']}")
```
