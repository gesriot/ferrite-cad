# §31E — Diameter input through Radius

Status at the author handoff: implemented, **unstaged and uncommitted** on branch
`circle-diameter-input`; no window was run; the diff has no CI of its own. Base:
`9fbf719e86336c0e9d5c8909e045515e5efac487` (merge of PR #107; tree
`8361d6d30a395516ff42c52d99d0c57d2b9f8301`; HEAD = main = origin/main after a fresh
fetch, clean tree; the foreign worktree `a200` untouched). The plan entry is
[§31E](implementation-plan.md). This slice does **not** close §31C (circle centre),
§31D (hover anchors), §30, Milestone 5C, the product or the OOM investigation.

## What a person and an agent can do

A person who opens *Edit constraints…* on a saved Circle, or on a ring, picks a circle by
its row (the UUID is what is picked; the ring shows `Boundary circle · …` and
`Bore circle · …`), types a diameter in mm in **Diameter mm** and presses **Add diameter**.
The draft then holds the ordinary `Radius` constraint with `R = D/2` for that circle, shown
in *Pending additions* as `Radius 6.75 mm · Circle <UUID>`; the standing line beside the
box says `Stored as Radius = Diameter / 2`. Apply, Save copy, Undo, Redo, Remove and the
tab, job and refusal behaviour are exactly those of a typed radius, because the draft only
ever holds a Radius.

An agent sends the same thing to the existing command:

```json
{"rule":"diameter","curve_id":"<UUID from inspect>","diameter_mm":13.5}
```

in `add` of `edit-sketch-constraints-copy` request v1. It is converted to the same
`AddSketchConstraint::Circle` / `Radius`; the response and `inspect` still report
`{"kind":"radius","curve_id":…,"radius":6.75}`. Nothing remembers that a diameter was
typed: there is no Diameter constraint, no solver equation, no ABI/shim kind, no
capability, no schema, no new response version and no new command.

## Contract (decided, and tested)

* **One conversion.** `CircleRadiusMm::from_diameter_mm(D)` in `ferritecad-document`
  (`sketch_constraints.rs`) is the only place that computes `D/2`; the form and the
  command call it. `D` must be finite and strictly positive; the result is then
  **checked**: halving can lose precision when the resulting radius is subnormal,
  including for normal `D` near the normal/subnormal boundary. The smallest positive
  `D` halves to `0`. A `D` that is not
  exactly twice the radius it yields is refused (`circle diameter is too small to halve
  into a positive radius in mm`) instead of being stored as another circle. The existing
  `CircleRadiusMm::new`, the circle policy (`CircleExtrusion`, 1 000 000 mm bound),
  `validate_edits` and the solver checks are untouched and still judge the result: a
  `D` of 4 000 000 is refused as the radius 2 000 000 would be.
* **The row.** Under the Radius row, on its own line, so the Radius row is not widened or
  moved: `Diameter mm [box] [Add diameter]  Stored as Radius = Diameter / 2`. Its own box
  (`draft.diameter_mm`) beside the radius box: no box is ever read in two units, there is
  no unit switch, and the typed text is neither converted nor consumed by Add (as with
  Radius). Typing is not a history step; one accepted Add is exactly one. Text that is
  not a positive finite number, or that cannot be halved, is refused in words
  (`Enter a positive diameter in mm.` / the conversion's own sentence), stays in the box
  as typed, adds nothing and changes neither the picked circle nor the radius box. Add is
  disabled until a circle is picked. While a job runs the whole body is disabled and the
  Diameter box is also **non-interactive**: egui lets a focused box keep taking text even
  when its `Ui` is disabled (§31D), and a focused box was observed to; the older Radius,
  Line length and Fixed boxes now use the same guard after independent review (see below).
* **Request form.** `{"rule":"diameter","curve_id":UUID,"diameter_mm":N}`, additive in
  request v1, strict like its neighbours (`deny_unknown_fields`): `radius_mm` inside it,
  both numbers, any extra field, a missing `diameter_mm` or `curve_id`, a bad UUID, a
  string/null/boolean number, a duplicate key, `Diameter`/`diameter_mm` as the rule, an
  array in place of the object, `NaN`, `Infinity`, `1e999`, `0`, `-0.0`, negative numbers
  and the two subnormals are all `kind:"input"`, code 2, nothing written. A `diameter_mm`
  inside a `radius` addition is refused too. The old `radius` form and its output are
  unchanged.
* **One slot.** A circle has one Radius slot, however it was filled: a diameter beside a
  radius, two diameters, and a diameter while a stored Radius exists are refused by the
  document's own rule (`only one radius …`, a structural refusal, not a solver
  diagnosis). Replacing is the existing exact `remove <Radius UUID>` + `add`, atomic, in
  one request; in the form it is the row's Remove plus Add diameter, and each is an Undo
  step. Other UUIDs, the pin, the concentric pair, the stored coordinates, the stored
  references and foreign SQL data keep the existing allowlist.
* **Ring roles** are the saved ones. A diameter lands on the circle whose UUID is picked
  or named, never on the first or second row; a ring stored bore-first behaves like one
  stored boundary-first. Diameters given to the wrong circles cross the pair over and the
  rebuild refuses, exactly as crossed radii do.

## Code

`ferritecad-document/src/sketch_constraints.rs` (`from_diameter_mm` and its unit test),
`ferritecad-cli/src/edit_constraints.rs` (`Addition::Diameter`), `ferritecad-app/src/constraints.rs`
(`Draft.diameter_mm`, the row, a cfg(test) accessor `draft_diameter`), tests in
`constraints/tests/diameter.rs` (new), `sessions/tests/tabs/drafts.rs`,
`ferritecad-cli/tests/{circle,annular}_constraints.rs`. CI: gates added to the existing
`ci.yml` constraints step, the existing native circle step of `runtime-layout.yml`, one
marker line in `occt-pin.yml`, one pinned-ufbx hook in `tools/check-fbx-complex.sh`. No
dependency, manifest, lock, schema, kernel, solver, ABI or job change.

## Executed (author, local)

All builds and runs `CARGO_BUILD_JOBS=1`, the pinned libraries, no rebuild of OCCT or
PlaneGCS, no viewer, window, CUA or GPU run by the author (the whole viewer suites ran under
`tools/watch-viewer-memory.py`, 1536 MiB, 1500 s). Memory pressure level 1 and swap 0 before,
during and after; free disk 147 GiB; no own process left running. Logs: the ignored directory
`target/31e-check/`. The true-stub target `/private/tmp/ferrite-25j-stub-target` of earlier slices
had been removed, so it was rebuilt from scratch with the recipe of the earlier slices.

**True stub (no OCCT, no PlaneGCS)**

| Check | Result |
| --- | --- |
| Stub proof | the bridge `CMakeCache` holds `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE` and `OpenCASCADE_DIR=/private/tmp/absent-occt-true-stub`; the build says "Open CASCADE was not usable"; `otool -L` of the viewer test executable, the two CLI test executables run here and the CLI binary: 0 × `libTK*`, 0 × planegcs (the native viewer test executable: 50 and 1). (The first fresh stub directory found Homebrew OCCT in `/opt/homebrew` and was reconfigured before any result was taken.) |
| Conversion, strict request, widgets (the real `ci.yml` block, extracted and replayed) | exit 0; `FCAD_31E_DIAMETER_CONVERSION_EXECUTED`, `FCAD_31E_CLI_STRICT_EXECUTED`, `FCAD_31E_WIDGETS_EXECUTED`, `FCAD_31E_REPLACE_EXECUTED`, `FCAD_31E_RING_EXECUTED`, each `1 passed; 0 failed`, no `skipped:` |
| Whole viewer suite under the watchdog | 707 passed, 0 failed, 2 ignored (702 + 5 new); 206 skip lines (204 before; the two new ones are this slice's kernel-only gates, the tab gate and the native end-to-end); peak footprint 50.8 MiB; exit 0; 3 `FCAD_31E_*` markers |
| `ferritecad-document`, all targets | 158 + 9 + 9 + 32 + 29 + 30 + 10 passed, 0 failed |
| `ferritecad-cli` `annular_constraints`, `circle_constraints`, `edit_constraints`, `edit_sketch` | 6, 7, 9, 5 passed, 0 failed; the native process gates return early with `skipped:` here |
| Geometry skips, listed separately | the native end-to-end gate (OCCT + PlaneGCS), the tab gate (OCCT), the two CLI native process gates; every other 31E gate ran |
| Widths measured by the widget gate | the Diameter row reaches 428.7 pt; the rest of the form reaches 598 pt, so the new row widens nothing |

**Native (OCCT + PlaneGCS, release)**

| Check | Result |
| --- | --- |
| Peer CLI | `cargo build --release --features planegcs -p ferritecad-cli` immediately before the worker gates (the replay of the real step builds it first) |
| `runtime-layout.yml` block (extracted with its own preamble and macOS substitutions, replayed) | exit 0; `FCAD_31E_CLI_CIRCLE_EXECUTED`, `FCAD_31E_CLI_ANNULUS_EXECUTED`, `FCAD_31E_NATIVE_EXECUTED radii=6.75,2.125 constraints=4 faces=4`, `FCAD_31E_TABS_EXECUTED`; each `1 passed; 0 failed`; `$GITHUB_ENV` receives `FCAD_DIAMETER_FBX` |
| End-to-end gate | the form of a ring **stored bore-first**, with foreign SQL data attached to the source (an attached source claim, an unknown table, extra capability rows): Bore row, 4.25; Boundary row, 13.5; Pair Circle A/B, Add Concentric; Fixed centre; Save copy → the window's worker (`spawn_constraint_edit`), DOF 0; the same four constraints as a peer CLI process with `diameter` requests. Compared: same objects, dependencies and topology references (and equal to the source's); stored curves unchanged (the guess is kept); the same four rules, matched by the rule they carry, each publication with its own four fresh constraint UUIDs; **every SQL cell equal** except the Sketch row's payload and hash and `meta.modified_at`; against the source every other table and object row literal, the foreign capability rows kept. Reopen + cold rebuild of both copies: the kernel's faces, `Cylinder{6.75}` under the Boundary's reference and `Cylinder{2.125}` under the Bore's, 4 faces, volume within 1e-6 of π(R−r)(R+r)h = 1934.288 mm³; `rebuild --cold`: `4 of 4 stored references resolved`. The exported STL, read by the test's own parser: closed, one oriented surface, rims on both solved radii about (−3.5, 4.25), volume between the two inscribed-prism bounds (a mesh is not offered as the analytic circle). Negative control: the same boxes with the diameters on the other circles cross the pair over; the worker refuses, no file, the form keeps its pick, text and four pending constraints |
| Pinned ufbx 0.23.0 (reader rebuilt from sources whose SHA-256 match the pin; the real hook block of `tools/check-fbx-complex.sh` replayed) | the 94 120-byte FBX of that gate: `checks=6 failures=0`, `FCAD_DIAMETER_UFBX_EXECUTED`; a truncated copy through the same hook: `checks=1 failures=2`, exit 1 |
| Whole viewer suite under the watchdog | 707 passed, 0 failed, 2 ignored; 33.0 s; peak footprint 298.2 MiB; pressure 1; swap 0; exit 0; 23 skip lines (as before, none of this slice); all five `FCAD_31E_*` markers; the `occt-pin.yml` exact-line grep `FCAD_31E_TABS_EXECUTED` passes on it |
| `ferritecad-cli` the four suites above, `ferritecad-document` | 5, 6, 9, 5 passed (the `occt_without_solver_*` gates are compiled out with the feature); document as in the stub; no `skipped:` in the three constraint suites |
| OCCT without solver (debug, `FERRITECAD_REQUIRE_PLANEGCS=0`, DYLD = OCCT only) | the tab gate prints its marker; the widget gates and the CLI strict gate pass (`unsupported` for the well-formed request); the end-to-end gate reports `skipped: the diameter gate needs planegcs` and passes. The older `native()` helper of the CLI process gates (and the older app gates that copy it) fail in that shape when `FERRITECAD_REQUIRE_OCCT=1`, as recorded for §31C; only the new end-to-end gate was written to skip where only the solver is missing |
| Agent recipe | `FCAD_31E_AGENT_RECIPE_OK` against the native CLI; the text below is byte-identical to the file that ran |

**Both**

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` (stub env) | pass (clippy found two `cloned_ref_to_slice_refs` in the new CLI tests; fixed) |
| `actionlint` on all workflows; `tools/check-actions-run-size.py` | pass; 206 blocks, largest still 20 432 of 21 000 bytes (not a block of this slice; the circle step is 4 010 bytes, the `ci.yml` step 3 273) |
| Licence headers (494), export boundary, solver ownership, planegcs pins, notice ownership, STEP corpus checksums, `git diff --check` | pass; no manifest, lock or deny change |
| Mixed (OCCT without solver) whole-suite run | **not run, reasoned:** no kernel, solver, ABI or job boundary was touched (`check-solver-ownership` passes). The kernel-only and kernel-free gates of this slice were run in that shape instead (above) |
| Full STEP corpus, GPU, window, Windows/Linux, remote CI of this diff | **not run** |

### Mutations (all compiled and failed on executed assertions; files restored byte for byte)

SHA-256 of `sketch_constraints.rs` and `constraints.rs` before the first mutation and after the
last restoration: `9778310b…` and `fa0ecd20…` (identical).

1. **Lost `/2`** (`from_diameter_mm` ends `Self::new(diameter)`). True stub: the conversion unit test
   failed at `D 1.35e1`, left 13.5, right 6.75; three of the four widget gates failed on the
   request they build (`Radius(13.5/2)` was `Radius(13.5)`; the fourth is the native one and skipped).
   Native: the circle CLI process gate failed on the response rule (`radius 13.5` for `6.75`), the
   ring CLI gate on the radii by curve (`4.25, 13.5` for `2.125, 6.75`), and the end-to-end gate on the
   request the boxes build; the first run of that gate had selected zero tests (a filter that was not
   the full name) and was repeated with the full name before it was counted.
2. **Wrong addressee in the ring** (Add diameter writes to the first painted circle instead of the
   picked one: `circles[0].0`). True stub: the ring widget gate failed (`a circle may hold only one
   radius`, the second diameter had gone to the first row's circle); the single-circle gates passed, as
   they should. Native: the end-to-end gate failed on its request (a Concentric where the Boundary's
   Radius was expected).
3. **Control for the frozen box** (`.interactive(!running)` removed): the widget gate failed at
   `a focused box kept taking text while a job ran`, `"207"` for `"20"`.

After each restoration the positive gates above were repeated.

### Found while testing

* Clippy: two `cloned_ref_to_slice_refs` in the new CLI tests (fixed).
* The same egui trap that §31D met is **already present** in the older Radius box: a scratch gate
  (removed) typed `20` into it, then fed a `Text` event while a job ran, and it became `207`. The Line
  length and Fixed X/Y boxes are built the same way and were not measured one by one. Not changed here
  (outside this slice); recorded for review. The new Diameter box is non-interactive while a job runs.
* Zsh, not the product: `set -- $spec` does not split, `rm -f a.*` without a match aborts an `&&` chain,
  and a test marker printed without a leading newline cannot be matched as an exact line, hence
  `println!("\nFCAD_31E_…")`.

### Design choices flagged for review

* The conversion also refuses a `D` that does not halve exactly (the smallest subnormals); the task
  asked for the zero case, this is the same check stated as "R = D/2 exactly".
* The Diameter box is not cleared or converted after Add (like the Radius box), so a second Add of the
  same text is refused by the occupied slot rather than silently repeated.
* The row is a second line under the Radius row rather than inside it, because the Radius row already
  carries two buttons; the explanatory text sits on the new row.
* The Diameter box is frozen with `.interactive(!running)`; the older boxes were left alone.
* A diameter of 4 000 000 mm is refused by the circle policy, not by the conversion.

### Base CI (not this diff)

For the merge commit `9fbf719`, read at the start and again at the end of the work: `CI` 38516647070
(all seven jobs), planegcs pin, both SBOMs, notices and `combined runtime layout` 38516647115 (linux,
macos, windows and the comparison) all completed with success. At the start `combined runtime layout`
was still `in_progress`; it was not counted then. Nothing here is a result of a CI run of this diff.

## Not done, not claimed

No window, GPU, Windows or Linux run, no remote CI of this diff, no reviewer sign-off. Hover-anchor
acquisition (§31D), the circle-centre snap (§31C), a Diameter constraint of its own, expressions, and
every other constraint adapter. §31C, §31D, §30, Milestone 5C, the product and the OOM investigation
stay open.

## Agent recipe (executable)

Needs the native `ferritecad` (OCCT and planegcs) in `FERRITECAD`; run it with
`python3 -I`. It prints `FCAD_31E_AGENT_RECIPE_OK` after its last assertion.

```python
# FCAD_31E_AGENT_RECIPE
import json, os, pathlib, subprocess, tempfile
cli = os.environ["FERRITECAD"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-diameter-"))

def run(args, code=0):
    p = subprocess.run([cli, *args], capture_output=True, text=True)
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def edit(source, remove, add, out, code=0):
    """One request v1 against the version this very inspect reported."""
    seen = run(["inspect", str(source), "--json"])["result"]
    (root / "request.json").write_text(
        json.dumps({"request_version": 1, "remove": remove, "add": add}))
    return run(["edit-sketch-constraints-copy", str(source),
                "--sketch", seen["sketches"][0]["sketch_id"],
                "--expect-version", seen["content_version"],
                "--request", str(root / "request.json"), "-o", str(out), "--json"], code)

# 1. A source, and the circle's UUID from discovery (a ring offers two, with roles).
(root / "create.json").write_text(json.dumps(
    {"schema_version": 1, "center_mm": [12.0, -7.0], "radius_mm": 10.0, "height_mm": 15.0}))
source = root / "cylinder.fcad"
assert run(["create-circle-extrude", str(root / "create.json"), "-o", str(source), "--json"])["ok"]
circle = run(["inspect", str(source), "--json"])["result"]["sketches"][0][
    "constraint_edit"]["circles"][0]["curve_id"]

# 2. A diameter is stored as the ordinary Radius D/2; the answer still says radius.
sized = root / "sized.fcad"
done = edit(source, [], [{"rule": "diameter", "curve_id": circle, "diameter_mm": 13.5}], sized)
added = done["result"]["added_constraints"][0]
assert added["rule"] == {"kind": "radius", "curve_id": circle, "radius": 6.75}, added
assert done["result"]["solve"]["degrees_of_freedom"] == 2  # a sized circle still moves

# 3. Strict: nothing is guessed, nothing is published.
for bad in ({"rule": "diameter", "curve_id": circle, "radius_mm": 6.75},
            {"rule": "diameter", "curve_id": circle, "diameter_mm": 13.5, "radius_mm": 6.75},
            {"rule": "diameter", "curve_id": circle},
            {"rule": "diameter", "curve_id": circle, "diameter_mm": 0},
            {"rule": "diameter", "curve_id": circle, "diameter_mm": "13.5"}):
    refused = edit(source, [], [bad], root / "never.fcad", code=2)
    assert refused["error"]["kind"] == "input", refused
assert not (root / "never.fcad").exists()

# 4. One circle, one Radius slot: another diameter (or a radius) is refused, and
#    replacing is the existing exact remove + add, in one request.
edit(sized, [], [{"rule": "diameter", "curve_id": circle, "diameter_mm": 9.0}],
     root / "never.fcad", code=2)
assert not (root / "never.fcad").exists()
wider = root / "wider.fcad"
edit(sized, [added["constraint_id"]],
     [{"rule": "diameter", "curve_id": circle, "diameter_mm": 17.0}], wider)
kept = run(["inspect", str(wider), "--json"])["result"]["sketches"][0][
    "constraint_edit"]["constraints"]
assert [c["rule"]["radius"] for c in kept] == [8.5]

# 5. A cold rebuild resolves every stored name on the solved cylinder.
rebuilt = run(["rebuild", str(wider), "--cold"])
assert "3 of 3 stored references resolved" in rebuilt, rebuilt
print("FCAD_31E_AGENT_RECIPE_OK")
```

## Independent macOS window recipe (the author does not run it)

Inputs only, made with the peer CLI and **no** expected output:
`/private/tmp/ferrite-31e-window/inputs/` holds `cylinder.fcad` (one Circle, centre
(12, -7), R 10, height 15), `ring.fcad` (Boundary R 10, Bore r 4, same centre and height)
and `SHA256.txt` (their `shasum -a 256` and that of the two request files that made them).
Work on **copies**; run any viewer only through `tools/watch-viewer-memory.py --limit-mib 1536`
with a time limit and with the memory pressure normal.

1. Open a copy of `cylinder.fcad`; press **Edit constraints …**. Under the `Radius mm … Add
   radius … Add Fixed centre` row there is a second row `Diameter mm [box] Add diameter` and
   `Stored as Radius = Diameter / 2`. The form is not wider than before and every older button
   (Undo, Redo, Clear pending changes, Fixed X/Y, Save/Apply) is still there.
2. Press **Add diameter** with no circle picked: nothing happens. Pick the `Circle · …` row. Type
   `abc`, `0`, `-4`, `13,5`, `1e999`: a red sentence, the text stays as typed, nothing pending, Undo
   stays disabled. Type `13.5`: Undo is still disabled (typing is not a step). Press **Add diameter**:
   *Pending additions* shows `Radius 6.75 mm · Circle <UUID>` and Undo is enabled for one step;
   Undo and Redo move it; neither starts a job.
3. Type `9` and press **Add diameter**, then `5` in **Radius mm** and **Add radius**: both say one
   circle holds only one radius. **Apply constraints** (the document becomes unsaved), then reopen
   the form: the stored row `Radius 6.75 mm · <UUID>` is there. Replace it: its **Remove** box, a new
   diameter `17`, **Add diameter**: pending `Radius 8.5 mm`, the stored row marked for removal;
   Undo twice, Redo twice; **Apply constraints**; document Undo/Redo; Save As; STL/FBX export.
4. Open a copy of `ring.fcad`. Pick the `Bore circle` row, `4.25`, **Add diameter**; pick the
   `Boundary circle` row, `13.5`, **Add diameter** (Radius 2.125 on the Bore's UUID, 6.75 on the
   Boundary's). Pair Circle A = Boundary, B = Bore, **Add Concentric**; Fixed X `-3.5`, Y `4.25`,
   **Add Fixed centre**; **Apply constraints**. Negative control: the same with the diameters given
   to the other circles (Bore 13.5, Boundary 4.25) is refused by Apply, the form keeps its text, pick
   and pending list.
5. Tabs: type `21` in **Diameter mm** with the circle picked, open the second copy in a new tab, come
   back: the text and the pick are as they were; the other tab's form is its own.
6. After Quit, compare the saved file with a peer CLI run of the same numbers
   (`{"rule":"diameter",…}` from the UUIDs `inspect --json` reports): every SQL cell equal except the
   Sketch row's payload and hash (the new constraint UUIDs) and `modified_at`; `validate`,
   `rebuild --cold` (`4 of 4 stored references resolved` for the ring), STL volume
   (ring 1934.288 mm³ analytic, the mesh a little under or over by its chord error; cylinder
   with D 13.5: 2147.082 mm³, with D 17: 3404.701 mm³) and the pinned ufbx reader (`checks=6
   failures=0`). Negative control: the unchanged input ring is 3958.407 mm³ and its bytes differ.
7. Record memory pressure and the exit status. Do not call the OOM investigation closed.

## Manifest

Modified: `.github/workflows/{ci,occt-pin,runtime-layout}.yml`, `README.md`,
`crates/ferritecad-app/src/constraints.rs`, `crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs`,
`crates/ferritecad-cli/src/edit_constraints.rs`, `crates/ferritecad-cli/tests/{annular,circle}_constraints.rs`,
`crates/ferritecad-document/src/sketch_constraints.rs`, `docs/{annular-circle-constraints,circle-radius-constraints,cli-capabilities,cli-json-v1,implementation-plan}.md`,
`tools/check-fbx-complex.sh`. New (untracked): `crates/ferritecad-app/src/constraints/tests/diameter.rs`,
this file. The full diffstat is in the handoff report.

## Independent review

The author-reported focused-input problem was reproduced on Radius, Line length and
both Fixed coordinates in both Line and Circle forms: typing `7` while a job was
running changed `20` to `207`. The compiled regression executed one test and failed
on those values. All four older fields now use `interactive(!running)`, as Diameter
already did. The test also checks that editing resumes after the job and no text
change creates a constraint-history step; it is required by the existing CI block.
An initial test attempt used the wrong Line-length label and is not counted as the
failing-first evidence.

The conversion documentation now describes the result's subnormal boundary, including
a normal input whose half rounds. Adjacent normal boundary values are covered by the
conversion test. The existing exact-halving policy is preserved.

Review logs live in ignored `target/31e-review/`. Further verification is recorded
below when complete; author checks above remain separate.

Independent local checks of the reviewed diff: 41 native constraint-editor tests,
one native tab-retention test, 5 annular and 6 Circle CLI process tests, and the
conversion unit test all passed with required OCCT/PlaneGCS and no skips. True stub:
four widget tests, the conversion and strict CLI gates executed; the fifth widget
harness entry is the explicitly skipped native worker. Formatting, workspace clippy
(all targets/features, warnings denied), actionlint, shellcheck, licence and boundary
checks passed; the Actions maximum is still 20 432 bytes (limit 21 000).

The fresh bundle was built from the reviewed code and its ad-hoc signature verified.
The independent macOS arm64 window check completed on 2026-10-14, using private
copies in `/private/tmp/ferrite-31e-review-window.5rcfmgh5/` and one viewer under
the 1536 MiB watchdog. Observed in the actual window:

- No selection disables Add diameter. Invalid text remained visible with an error
  and no history change. Typing 13.5 made no step; Add made Radius 6.75 and one
  Undo/Redo step. Cancel in the system Save-copy dialog preserved the draft.
- A second diameter and an ordinary Radius both refused the occupied slot. Apply,
  exact Remove plus diameter 17, two draft Undo/Redo steps and document Undo/Redo
  produced the accepted radius 8.5. STL/FBX were exported before Save As.
- The ring's Bore received diameter 4.25 and Boundary 13.5, followed by Concentric
  (A = Bore, B = Boundary) and Fixed centre (-3.5, 4.25) on Boundary. Switching to
  the cylinder tab and back retained the selection, fields and four pending rules.
  Apply yielded DOF 0 with no redundant constraints; Save As and exports succeeded.
- Replacing only the Bore's Radius with diameter 20 refused Apply because the hole
  would exceed its boundary. The accepted model stayed saved; the draft retained
  the selected Bore, text 20, exact removal and pending Radius 10. Cancel dismissed it.

The script `target/31e-review/compare-window.py` compared these actual files with
independent CLI requests against the same source UUIDs. Every SQL cell was equal
except `objects.payload` and `objects.payload_hash` of the Sketch. Substituting
only the newly allocated constraint UUIDs made the payload bytes equal too;
`modified_at` happened to be equal in this run and required no allowance. The stored
curves, other objects, document ID, dependencies and references matched exactly.
Both original inputs and working source copies retained their SHA-256.

`validate` and cold rebuild passed on GUI and CLI copies (3/3 cylinder and 4/4 ring
references). The pre-Save GUI STL and FBX matched both the independent-request CLI
exports and fresh exports from the saved GUI files **byte for byte**. An independent
binary-STL parser checked byte length, closed directed edges, height 15, radii about
the intended centre, and mesh volume separately from the analytic reference:

| Actual window output | STL triangles / bytes | Mesh volume, mm³ | Analytic reference, mm³ | FBX bytes |
| --- | --- | --- | --- | --- |
| Cylinder D17 | 364 / 18 284 | 3402.054915 | 3404.701038 | 67 264 |
| Ring D13.5 / D4.25 | 512 / 25 684 | 1932.849057 | 1934.288414 | 94 120 |

Both FBX files passed pinned ufbx 0.23.0 (`checks=6 failures=0` each). Mesh radii
were within 1e-5 mm of 8.5 and of 6.75/2.125 respectively; chordal meshes were not
treated as exact cylinders. Logs: `window-comparison.log`, `gui.jsonl` and the
private output directory. Viewer PID 20884 exited normally, code 0, after 1102 s;
sampled peak footprint 206.329 MiB, pressure 1 throughout, swap 0, no watchdog abort.
No CUA query was made against the app after Quit.

Limits: this window run used the boundary-first ring; the bore-first case remains
covered by the native worker test. Only one malformed string was entered in the
window; the complete malformed-number matrix and input freeze during a job remain
automated widget/process checks. Multicharacter CUA typing initially dropped letters
and a key-name period followed the active layout; verified per-character text input
was used for the final numbers. These input-delivery attempts are not product failures.
Windows/Linux window tests were not run. Remote CI is reported separately in PR #108;
it was still running when this evidence was committed. OOM remains unresolved.
