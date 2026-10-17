# Contributing to FerriteCAD

Read this file and `docs/decisions/0006-generic-modelling-core.md` before
starting a task.

## Roles

The maintainer sets direction, reviews every pull request and merges it. A
contributor implements one task on one branch and opens one pull request into
`main`. Contributors never merge, never push to `main` and never rewrite
published history.

## Direction

FerriteCAD is moving from narrow, whitelisted feature slices to a generic
parametric modelling core (ADR 0006). Until that move is finished these rules
override anything older in `docs/`:

1. **No whitelists.** Do not add code that recognises a particular document
   shape ("a rectangular plate", "at most N cuts", "the exact six-object
   history") to decide whether an operation is allowed. An operation applies to
   whatever geometry it is given; whether it can be built is decided by the
   kernel when it runs. Do not write analytic pre-checks of whether a feature
   or its dependents will still fit: the rebuild decides.
2. **The model always builds.** An edit is accepted only if it leaves no
   feature failing that was not failing before; the refusal names every
   feature that would fail and why. A document that no longer builds on its
   own (after a kernel update, say) still opens, with its failing features
   marked, so that it can be repaired.
3. **Never retarget silently.** A reference that does not resolve, or resolves
   to more than one entity, fails the feature that uses it and names the
   reference. It never picks a neighbour.
4. **Model numbers are `Expression`s in internal units**: millimetres and
   radians. Other units exist only at the UI and CLI edge.
5. **One domain path for UI and CLI.** Validation, document changes, rebuild
   and publication live in library crates. `ferritecad-app` and
   `ferritecad-cli` parse input and present output and hold no modelling rules.
6. **The file format is unstable.** Change it whenever a task needs to. Write
   no migrations, compatibility code, version checks or messages for older
   documents. Regenerate the fixtures that change.
7. **Delete what a generic path replaces** in the same pull request: the old
   per-feature functions, their tests, CLI commands and docs. List the
   deletions in the pull request.

## Code

- Rust 2024, toolchain pinned in `rust-toolchain.toml`. `unsafe` stays inside
  the FFI crates; `unwrap()` is denied by lint.
- Library errors use `CadError` from `ferritecad-types`; keep the source and
  never flatten an error into a string to pass it on. Binaries and tests may
  use `anyhow`. `expect()` only for internal invariants, with the invariant as
  its message.
- Prefer concrete types and free functions. Add a trait or generic parameter
  only when a second implementation exists now, or to invert a dependency for
  tests (`GeometryKernel` and its mock are the model).
- Items are `pub(crate)` unless they are part of a crate's API.
- Do not grow `crates/ferritecad-app/src/main.rs`; new window code goes into a
  module.
- Comments say why, briefly. No essays in doc comments, and no slice numbers
  (§NN) in new code.
- Do not optimise without a measurement; put the numbers in the pull request.

## Tests

- Tests are ordinary `cargo test` tests. Do not add per-test workflow steps,
  lists of test names, `*_EXECUTED` markers or exact pass counts.
- A test that needs Open CASCADE, PlaneGCS or a GPU may skip without it only
  when the matching `FERRITECAD_REQUIRE_OCCT`, `FERRITECAD_REQUIRE_PLANEGCS`
  or `FERRITECAD_REQUIRE_GPU` is not `1`; when it is `1` the test fails instead.
- Pin behaviour with a few strong tests: the main path, the failure paths and
  the invariants the task names. Do not write mutation harnesses, peer-CLI
  byte comparisons, window recipes or `*-verification.md` documents unless the
  task asks for them.
- When behaviour changes on purpose, change or delete the tests that encoded
  the old behaviour. Never keep both.
- GUI and GPU runs on a developer Mac go through
  `tools/watch-viewer-memory.py`; the project has an unexplained memory
  incident in its history.

## Build and run

Without Open CASCADE (stub kernel; geometry tests skip):

    cargo build --workspace --all-targets
    cargo test --workspace
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings

Native, with Open CASCADE 8.0.1 and PlaneGCS built as in `docs/build-occt.md`
and `docs/build-planegcs.md`:

    export OpenCASCADE_DIR=<occt install>/lib/cmake/opencascade
    export FCAD_PLANEGCS_DIR=<planegcs build>
    export FERRITECAD_REQUIRE_OCCT=1 FERRITECAD_REQUIRE_PLANEGCS=1
    # macOS only:
    export DYLD_LIBRARY_PATH=<occt install>/lib:<planegcs build>
    cargo test --workspace --all-features

On the maintainer's Mac the two live under `vendor/install` and
`vendor/planegcs`. macOS drops `DYLD_*` when it starts a protected binary
(`/bin/bash`, `env`, `nice`): run native tests from the shell that exported
the variables.

CI runs the stub suite and the native suite on Linux, macOS and Windows. A
pull request is merged only when every workflow is green on all three.

## Pull requests

- Branch from the current `origin/main`, with the branch name the task gives.
- Keep it reviewable: if the non-test diff passes about 3,000 lines, stop and
  propose a split in the pull request.
- Commits and pull request text in English, imperative mood. They describe the
  change only: no tool names, session links or co-author trailers.
- Description sections: **What and why**, **Design decisions**, **Deleted**,
  **Verification** (exact commands and results; say what was native, what was
  not, and what was checked in the GUI), **Known gaps**.
- If the task and the code disagree, or a rule here blocks the task, say so in
  the pull request instead of working around it.
