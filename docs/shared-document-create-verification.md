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

## Manual GUI observation unavailable

A temporary `.app` was prepared outside the checkout with the repository's
runtime-closure and staging tools, containing the built viewer, CLI and native
libraries. The available UI tool was asked to launch it with no arguments, but
reported that the Mac was locked and automatic unlock failed. The user was asked
to unlock it. **No New/Save dialog click or rendered model is claimed from that
attempt.**

Still unobserved manually: empty start → New plate with nondefault dimensions →
system Save in a path with spaces → model → FBX; New empty; form/save cancellation;
refusal of an existing destination; Open of an existing document afterwards.
Automated worker/state tests above are separate evidence and do not substitute
for those observations. No distribution-signing, notarization or new packaging
claim is made.

§24 remains partial: edits to existing models, an unsaved document, Save/Save As,
dirty state, arbitrary modelling, structured public results and stdin/batch are
outside §24B. The PR must remain unmerged for independent review.
