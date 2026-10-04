# §30E — verification

[Contract](revolve-angle-session-apply.md). Marker `FCAD_30E_REVOLVE_ANGLE_SESSION_APPLY`.
Base `064b105607d992d1fb1da152efa511ed741f8c61`, tree
`1eee8f3a55c3dcb74f2c9af0fb4691c2e3db1769`; `main == origin/main`, clean before
branching. PR #84 was MERGED on 2026-10-03; its final head `7589d8a` had 7/7 CI
success. Its earlier code/workflow head `5b08f342` had 15/15 checks. Neither is the
CI of this slice. Branch `revolve-angle-session-apply`; commits use the existing
`gesriot <gessman1618@gmail.com>` author and committer. No merge/auto-merge.

## Gates and comparisons

`sketch::tests::angle_apply` drives real egui widgets with `can_apply_angle`.
Its busy gate first submits a changed valid request in idle. It covers current
unconfirmed fields, draft confirmation/Undo/Redo, differently written no-op,
range refusals with retained text, actual dirty-session discovery, clean/dirty
copy availability, no/wrong form, no session, full turn, loading, session work,
height/New forms, running export, STL configuration and replacement question,
readable status inside the window clip and accepted-scene dismissal.

`sessions::tests::angle` uses the real workers and a real peer CLI, on offset
rectangles initially at 137.5°: bore 2.75 / radius 7.5, bottom −3.25, height 12.75;
axis-closed variant has a real final axis Line. Profile radius becomes 8.75;
the constrained variant adds H/V, Fixed and two distances through §30C, solves
height 13.75 while stored coordinates remain unchanged, then applies 212.25°.
It checks common Undo/Redo, Save, cold reopen and every ref (two caps required),
stale version refusal, invalid angle, no-op/Redo/file cleanup, cancellation,
stale answer, scene/GPU refusal, retained draft and branch after Undo/Save As.

Angle comparison excludes only `meta.modified_at`: no UUID is introduced.
Independent constraint peers map only newly added constraint UUIDs. The raw hash
is validated, only those UUID bytes are substituted, the normalized payload hash
is recomputed, and every other SQL cell stays in the comparison. No Sketch row,
payload or hash is dropped. Exports before Save match real CLI STL/FBX bytes.
B-Rep volume/angle and STL closure, winding, angle, radial limits, axial limits
and volume are measured independently. Analytical volume and mesh approximation
are reported separately. Pinned strict ufbx reads all three FBX artifacts and
joins their triangles with the STL; workflow markers make missing reading fail.

New exact-name/no-skip gates extend `ci.yml` and `runtime-layout.yml`; old gates
remain. The stub is an actual no-OCCT build: discovery/widgets run and the typed
worker refusal publishes nothing. Mixed OCCT/no-solver runs both unconstrained
branches, then refuses an actually constrained model without changing state.
Configuration-specific skips in the general suite are not geometry evidence.

## Executable local gates

On this Mac the existing `/private/tmp/ferrite-pr84-review/env.sh` was checked:
arm64 OCCT 8.0.1 and PlaneGCS from FreeCAD 1.0.1, with the FreeCAD, Eigen 3.4.0 and
Boost 1.91.0 archive digests matching `tools/planegcs/pin.env`; runtime solver
provenance agrees. Native inputs were not rebuilt. Builds use two jobs and run
sequentially. Existing native/stub targets are reused, never another checkout.

```sh
source /private/tmp/ferrite-pr84-review/env.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --release --features planegcs -p ferritecad-app sketch::tests::angle_apply -- --nocapture --test-threads=1
# The native workflow step below names the three geometry tests exactly and rejects skips.
# Extract its run block, replacing only the matrix platform expression with macos:
python3 - <<'PY'
from pathlib import Path
for file, label, output in [
 ('.github/workflows/ci.yml', 'Open, edit, Undo, Redo and Save one document without native geometry', 'stub-packed.sh'),
 ('.github/workflows/runtime-layout.yml', 'Edit, Undo, Redo, export and Save one open document through the native window state', 'native-packed.sh'),
 ('.github/workflows/runtime-layout.yml', 'Build circles with OCCT and refuse constraints without the solver', 'mixed-packed.sh')]:
 lines=Path(file).read_text().splitlines()
 i=next(i for i,l in enumerate(lines) if l.strip() == '- name: '+label)
 i=next(i for i in range(i+1,len(lines)) if lines[i].strip() == 'run: |')+1
 body=[]
 while i<len(lines) and (lines[i].startswith('          ') or not lines[i].strip()):
  body.append(lines[i][10:]); i+=1
 Path('/private/tmp/'+output).write_text('\n'.join(body).replace('${{ matrix.name }}','macos')+'\n')
PY
export RUNNER_TEMP=/private/tmp/ferrite-30e-gates
mkdir -p "$RUNNER_TEMP"
export GITHUB_ENV="$RUNNER_TEMP/github-env"
export FCAD_OCCT_LIB_DIR="$PWD/vendor/install/lib"
bash /private/tmp/native-packed.sh
FERRITECAD_REQUIRE_PLANEGCS=0 bash /private/tmp/mixed-packed.sh
# Restore the shipped CLI before native peers / GUI:
cargo build --release --features planegcs -p ferritecad-cli -p ferritecad-app
# Stub: same packed argv, in the existing stub target, no native discovery.
CARGO_TARGET_DIR=/private/tmp/ferrite-25j-stub-target OpenCASCADE_DIR=/private/tmp/absent-occt \
 FERRITECAD_REQUIRE_OCCT=0 FERRITECAD_REQUIRE_PLANEGCS=0 bash /private/tmp/stub-packed.sh
```

## Directed mutations

M30E-1 removes only the new angle step's `check_form_version(expected)` call.
The native radial gate must fail its executed stale-refusal assertion.
M30E-2 replaces `!self.running()` with `!self.busy()` in `Creates::can_apply_angle`.
The current-fields widget gate must fail its executed own-form availability
assertion. Both must compile. Save/restore the source bytes, verify their SHA-256
identities, then rerun positive gates; no reset/rebase or universal mutation system.

## Real macOS window artifacts

Use a fresh staged arm64 bundle under a single owned watchdog:

```sh
FERRITECAD="$APP/Contents/MacOS/ferritecad" \
 python3 tools/revolve-angle-session-gui.py /private/tmp/ferrite-30e-gui
python3 tools/watch-viewer-memory.py --log /private/tmp/ferrite-30e-watch.jsonl \
 --limit-mib 1536 -- "$APP/Contents/MacOS/ferritecad-viewer"
```

Only `source/` and `work/` inputs are generated. No generator or comparator
creates any missing window output. All outputs below belong in the fixture root.

1. Open `work/radial.fcad`. Edit saved Sketch: outer two X coordinates 7.5 → 8.75;
   Apply vertices. Open angle on the dirty document, type 212.25; Confirm draft
   numbers → Undo draft → Redo draft. Document Undo waits while the form is open.
   Apply angle with no dialog; form closes. Before Save capture the actual user's
   `work/radial.fcad` as `radial-after-apply.fcad` (it must still be the source).
   Export `radial-unsaved.stl` and `.fbx`. Document Undo: export `radial-undo.stl`;
   Undo again: original; Redo twice: changed angle. Save; capture actual file as
   `radial-saved.fcad`. Undo angle, edit angle to 198.125, Apply; Redo is absent.
   Save As `radial-branch.fcad`; capture previous `work/radial.fcad` as
   `radial-after-saveas.fcad` (byte-equal to the earlier Save).
2. Open `work/axis.fcad`. Change outer two X coordinates to 8.75; Apply vertices.
   Open angle on the dirty document; Apply 212.25 **without** draft confirmation.
   Export `axis-unsaved.stl` and `.fbx`; Save; capture actual `axis-saved.fcad`.
3. Open `work/constrained.fcad`. Edit constraints: Segment 2 length 12.75 → 13.75;
   Apply constraints. Open angle, type 360: visible refusal and retained field;
   capture actual user's file as `constrained-after-refusal.fcad`. Type 212.25;
   Apply without confirmation, export `constrained-unsaved.stl` and `.fbx`;
   Undo/Redo angle and constraint steps; Save; capture `constrained-saved.fcad`.
4. Quit. After Quit inspect only the owned PID/watchdog; never `getApp` or AX.

Compare only those real artifacts, then run the pinned reader:

```sh
source /private/tmp/ferrite-pr84-review/env.sh
python3 tools/revolve-angle-session-gui.py --compare /private/tmp/ferrite-30e-gui
# read_production is compiled from the checked ufbx pin by tools/check-fbx-complex.sh:
for name in radial axis constrained; do
 "$READER" --identity "/private/tmp/ferrite-30e-gui/$name-unsaved.fbx"
 "$READER" --triangles "/private/tmp/ferrite-30e-gui/$name-unsaved.fbx" > "/private/tmp/$name-triangles.txt"
 python3 tools/fbx/stl-matches-fbx.py "/private/tmp/ferrite-30e-gui/$name-unsaved.stl" "/private/tmp/$name-triangles.txt"
done
```

Seven comparator controls must reject: missing output, original in place of
Save, wrong branch, disk-original export, wrong mesh angle, changed Sketch metadata
and corrupt payload hash. A missing artifact fails immediately; a CLI stand-in is
never a window check. A locked screen or unavailable CUA leaves this scenario
unverified, with independent headless/native work continuing.

## Execution record

Local and remote outcomes are recorded below after execution, separately from the
base CI. §30/5C/product completion is not claimed.

Initial local positive evidence (macOS arm64, release, pinned native components):
all five new widget tests and the three real geometry chains passed. The extracted
native workflow block then passed all 18 exact gates, including §30A–D regressions,
with no skips. Native measurements at 212.25°:

| Model | Analytical / B-Rep mm³ | Independent STL approximation mm³ |
|---|---:|---:|
| radial-clear | 1629.499930770 | 1628.557896850 |
| axis-closed | 1808.095484777 | 1806.728696300 |
| constrained | 1757.303846909 | 1756.287927976 |

M30E-1 and M30E-2 both compiled and failed on their intended executed assertions.
Production source bytes were restored exactly (SHA-256 checked); positive native
and widget gates ran again. fmt, workspace/all-targets/all-features clippy with
`-D warnings`, export boundary, solver ownership, licence headers, PlaneGCS pins,
workflow YAML syntax and `git diff --check` passed. Measurements go to artifacts
rather than interleaving with the harness's exact success lines; executing the
packed workflow caught that formatting issue and its corrected block passed.

Logs/artifacts for this run: `/private/tmp/ferrite-30e/`; the generator refused a
checkout destination and generated only temporary input fixtures. Mixed, stub,
strict-reader, GUI and published-head CI evidence will be appended when completed.
