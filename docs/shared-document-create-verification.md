# §24B creation verification

2026-09-07. Continued the existing `shared-document-create` work from
`baaa138d33648ca9eab7beb294500d5d36f40d45` (PR #13 merge). The incoming staged,
unstaged and untracked work was preserved before editing; no unrelated diff or
`.bak`/`.mutbak` files were removed. GitHub reports all 19 jobs on that base as
success. Final revision and remote runs are recorded in the PR.

## Failing first

A clean `git archive` of the base was built outside the checkout, with the new
CLI process test copied into it. The two regressions fail on their filesystem
assertions, after confirming exit 2 and the expected finite-value diagnostic:

| Command | Old result | Process-test failure |
| --- | --- | --- |
| `create bad.fcad --sample --size NaN 50 12` | Leaves empty `.fcad` | `the refused creation left a document behind` |
| `create bad.fcad --sample --size 60 40 NaN` | Leaves empty `.fcad` after transaction rollback | `a refused creation left a document` |

A separate CLI reproduction kept the failed destination and ran `inspect`:
exit 0, 0 objects, 0 dependencies, 0 topology references. Both regressions pass
on the shared route, including the assertion that the destination directory is
empty, not just the exit code.

## Boundaries and clients

`ferritecad_jobs::create_document` owns construction and publication. The sample
feature graph retains names, payloads, dependency roles, segment identities and
three stored cap/side references. Model dimensions remain millimetres; display
units do not change them. Width/depth zero and negative values remain accepted;
non-finite values and non-positive extrusion heights remain refused.

- `Document::write` rolls back the complete sample graph on a failure after
  objects have been inserted. The test inspects that still-open scratch database.
- After SQLite closes, progress delivery permits deterministic tests of a
  racing destination and cancellation through the actual `create_document`
  path. Atomic no-clobber preserves the racing file. Cancellation cleans the
  scratch directory, including the operation-owned SQLite sidecar names.
- The final cancellation check is immediately before publication. A cancellation
  reported after publication returns `CreatedDocument`; it does not remove or
  misreport the published file.
- The UI command tests exercise `ask_new`, form parsing, `start_new`, worker
  execution and `finish_create`. A separate real CLI process writes its peer
  document. Semantic comparison maps each object and sketch segment consistently
  wherever referenced, retaining full payloads and parent/dependency links. A
  negative control changes the side reference to another valid segment and must
  compare unequal.
- Native tests cold-rebuild both documents, require 3/3 references, compare STL
  triangles, compare FBX hierarchy/geometry after mapping native object identity,
  and require byte equality for UI/CLI FBX of each *same* saved document. The
  source document bytes are unchanged by these reads/exports.
- Window-state tests cover cancelled form/dialog, rejected creation, failed
  subsequent Open, stale Open, saved empty document vs no document, availability
  during load/export/replace confirmation, cancellation before/after publication,
  and shutdown cancelling and joining a worker while its scratch exists.

## Local checks

Rust 1.96.0, macOS arm64. Native inputs are the existing local OCCT 8.0.1 and
planegcs deliveries. No C++, FFI headers, native source pins or assets changed.
The only dependency change is jobs → document (an existing workspace crate).
The three Rust fragments, their digest references in the native inventory,
and the three product SBOMs were regenerated with the repository generators:
`generate-rust-sbom.sh`, `generate-native-inventory.sh`, `generate-product-sbom.sh`.
The first CI SBOM job identified stale Rust-fragment digests in the native
inventory; regenerating those references fixes that measured failure without
changing a native component, asset, pin or ownership map.

- `cargo fmt --all -- --check`, workspace all-target/all-feature clippy with
  `-D warnings`, and `git diff --check`.
- Document: 122; jobs: 29; UI: 102 tests passed.
- Real no-OCCT build: fresh target directory, CMake search excluding the installed
  Homebrew prefix; cache records `OpenCASCADE_DIR-NOTFOUND`. CLI create: 9 passed,
  zero skips. UI New: 17 harness passes comprising 16 executed tests and **one
  explicitly reported geometry skip**. The two window-coordination tests run
  separately without a kernel. No missing kernel is counted as executed geometry.
- Native viewer: 247; solver-info: 3; UI: 102 passed with
  `FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=1`, and
  `FERRITECAD_REQUIRE_GPU=1`, **zero skips**. GPU tests run outside the sandbox:
  the sandboxed Metal probe explicitly reported no graphics adapter. An earlier
  mixed local target also failed to locate `libTKernel.8.0.dylib`; the isolated
  native target supplies the actual OCCT/planegcs loader paths.
- Existing CLI native tests (including complex FBX and imported pixels) passed;
  the only failure in that intermediate combined run was an old UI test's copied
  toolbar coordinates. Those tests now locate the real rendered labels, and all
  102 UI tests pass. The final PR CI runs the complete workspace again.
- Export and solver ownership boundaries, licence headers, planegcs pin ownership,
  notice ownership and generated-inventory checks remain mandatory.

Ordinary CI builds both binaries before `cargo test --workspace`, so neither
client comparison relies on an unbuilt peer. OCCT pin already runs all CLI and
viewer tests; the new native creation gate is additionally required by name in
its log. Dispatching that workflow covers the changed Rust creation/loader/export
route on the three native platforms; it does not establish a new kernel pin.
The planegcs pin workflow's native inputs and solver integration did not change.

## Manual GUI smoke completed

2026-09-07, after the user unlocked the Mac. The earlier locked-screen attempt
provided no GUI evidence. The previously staged bundle also predated the final
build, so its preliminary plate/export observation was repeated using a fresh
bundle from `fd2efccf01aa513edc245f3ce171768f81ebd6d3`.

`cargo build -p ferritecad-app -p ferritecad-cli --all-features` completed against
the existing OCCT/planegcs inputs in the isolated native target. The repository's
`runtime-closure.sh` and `stage-runtime-layout.sh` prepared
`/private/tmp/Ferrite 24B final GUI/staged/FerriteCAD.app`. The staged viewer and
current build have the same Mach-O UUID
`A81D95D1-B55B-3F29-A346-DCBD4879E905` and identical `__text` dumps; staging only
relocates/signs the delivery. The GUI tool launched this bundle without arguments.
Native dialogs were operated through accessibility controls, and egui controls
through clicks located from current screenshots. The following are observed UI
actions and results, separate from the automated command tests above.

All outputs below are in
`/private/tmp/Ferrite 24B final GUI/results with spaces`, outside the checkout.

| GUI action | Observed result and filesystem check |
| --- | --- |
| Empty start | `No document`, Open/New available, Export FBX disabled. |
| New → Sample plate | Defaults 60/40/10 and mm labels visible; entered 83/47/13. |
| Choose where to save → system Save as `plate 83x47x13.fcad` | `Created` names the saved path; document name and Plate row appear; solid visible, including after clicking Iso. |
| Export FBX → system Save | `plate 83x47x13.fbx`: success, 4127 bytes, 1 node, 1 geometry, 1 material. |
| New → form Cancel | Form closes; accepted plate and view remain. No new file or scratch; existing hashes unchanged. |
| New → system Save named `cancelled dialog.fcad` → Cancel | Returns to the form with the plate still visible; named file absent, directory contents and existing hashes unchanged. |
| New empty → choose existing `plate 83x47x13.fcad` | macOS shows its standard Replace confirmation. After clicking Replace for this test file, FerriteCAD refuses with `already exists; choose a different file name`. Plate/view remain; document bytes unchanged; no scratch/sidecars. |
| Export after the cancellations and refusal | `after refusal.fbx` is byte-identical to the first plate export, confirming the accepted export source remains the plate. |
| New → Empty → Save as `empty document.fcad` | Saved filename and Created status appear; `No definitions`, empty viewport, Export FBX available. This is an accepted document. |
| Export the empty document | `empty document.fbx`: success, 1218 bytes, 0 nodes/geometries/materials. |
| Open → system dialog → existing plate | Plate filename, same Plate identity and visible solid return. `after reopen.fbx` is byte-identical to the initial plate export. |

The CLI **from that same staged bundle** subsequently inspected, validated and
cold-rebuilt both GUI-created documents. Both validate with zero warnings; the
plate rebuild evaluates 4 objects, builds 1 shape and resolves 3/3 stored refs;
the empty document has 0 objects/shapes/refs. CLI FBX exports are byte-identical
to the GUI exports of each saved document. The original plate document hash is
unchanged after cancellation, refused replacement, Open and export. No scratch
directory or SQLite/cache sidecar remains in the output directory.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| Plate `.fcad` | 86016 | `9b21521e9dfb66168f047b82bc512ab9e4263f0250ae0410c125f65a31c1648d` |
| Plate FBX, all four GUI/CLI exports | 4127 | `868d6f2ba35123c84d007e81d2f9056e536724c87fc43b5f08860c2574a1e2ee` |
| Empty `.fcad` | 86016 | `80f5f1fd6c70e28de2802a86a4aa2fc5fd6e0e2fc611a3aef30822b6146bf42a` |
| Empty FBX, GUI and CLI | 1218 | `63a08e22c5355facca0786ef460aaa8cc8e76c80d341bf2dd84539dd492922c9` |

The CLI commands and outcomes are retained locally in
`/private/tmp/ferrite-24b-final-gui-artifacts.log`. Screenshots and native-dialog
observations are in the task's UI-tool results. The required GUI smoke has no
remaining unavailable observation. Cancellation during the short-running worker
and stale-result races retain their deterministic automated coverage; they were
not simulated by GUI timing. This temporary local bundle does not establish
distribution signing, notarization or a new release-packaging claim.

## Remote verification and report follow-up

On the tested application revision `fd2efccf01aa513edc245f3ce171768f81ebd6d3`, all
7 workflows and 33 jobs succeeded, including Windows. The native creation gate
passed on all three platforms in OCCT run `34117820084`, with zero OCCT skips.
Each OCCT job reports 49 solver-test skips because that workflow does not link
planegcs; the separate linked-solver workflow passed. The combined local native
run passed all 579 relevant tests with zero skips.

This GUI follow-up changes documentation only. Applicable checks on its final
head and their links are recorded in PR #14. Native build inputs did not change,
so the native pin workflows are not dispatched again for this report update.

§24 remains partial: edits to existing models, an unsaved document, Save/Save As,
dirty state, arbitrary modelling, structured public results and stdin/batch are
outside §24B. The independent review requested before merging is recorded below.

## Independent review, 2026-09-07

Reviewed the transaction/publication boundary, worker cancellation and shutdown,
accepted-scene transitions, and the UI/CLI semantic and native comparison gates.
No blocking creation or scene-state defect was found. Review changes:

- CLI defaults now read `PlateSize::DEFAULT`, the same source used by the UI,
  instead of repeating the three numeric literals.
- The real CLI default-size regression now checks the stored Blind extrusion
  height as well as the profile corners. Temporarily changing only the CLI height
  default to 11 built successfully and failed the test on `the default height
  changed` (11 versus 10); the restored source passes.
- The capability map now distinguishes cancellation observed before publication
  from late cancellation, and documents the native macOS Replace prompt followed
  by the application's no-clobber refusal.

Independent local checks after the code correction: 512 tests passed (document
122, jobs 29, UI 102, viewer 247, solver-info 3, CLI create 9). OCCT 8.0.1,
planegcs and GPU were required; an uncaptured-output run confirmed zero skip
messages. A separate no-OCCT target records `OpenCASCADE_DIR-NOTFOUND` in its CMake
cache and passes all 9 CLI create tests. Formatting, workspace all-target/all-feature
clippy with `-D warnings`, export/solver ownership boundaries, licence headers
(292 files) and `git diff --check` passed. Review logs are outside the checkout in
`/tmp/ferrite-pr14-review`.

The reviewer independently launched the staged bundle named above; its viewer
and the locally rebuilt viewer both report Mach-O UUID
`A81D95D1-B55B-3F29-A346-DCBD4879E905`. Review changes do not modify viewer code.
Observed real GUI actions: empty start, New sample plate with default values
visible, entering 91/53/17 mm, native Save in a path with spaces, accepted visible
plate, Iso, and native FBX export. Form Cancel and native Save Cancel both preserve
the plate, directory contents and existing file hashes. The wider implementation
smoke above remains separate evidence; this review did not repeat every row.

The CLI from that same staged bundle inspected and validated the GUI-created
document, cold-rebuilt 4 objects / 1 shape / 3 of 3 refs, and exported FBX. Both
GUI and CLI FBX files are 4128 bytes with SHA-256
`5404e15e514f1ffb8c880ba023220e786ce9b88c84d072e2d8e4cfb9347715c3`.
The source document remains unchanged, SHA-256
`52ff6261ca2377fcb7213acc734087a8a4dd2fd798c84e2754464a2741ce988a`.
Outputs are in `/tmp/ferrite-pr14-review/UI results with spaces`. The updated
CLI executable is separately covered by the 9 process tests above.

Applicable CI on the review head and merge revision is recorded in PR #14. The
native pin runs on `fd2efccf01aa513edc245f3ce171768f81ebd6d3` remain evidence for
that revision, not newly dispatched runs on the review commit. No native input,
dependency edge or shared creation algorithm changed during this review.
