# §27A verification — full-turn Revolve / NewBody

[Contract and recipe](full-turn-revolve.md).

## Where and how this was run

* **Machine.** A Linux x86_64 cloud container (Linux 6.18, 16 GiB RAM).
* **Branch and base.** Branch `full-turn-revolve` from base `9d1a5f5`
  (tree `a8c01bdb`). All six workflows on `9d1a5f5` were green before
  implementation started.
* **Native stack.** The pinned OCCT 8.0.1 native install and the pinned ufbx
  reader. Neither OCCT nor PlaneGCS was rebuilt; only the bridge was
  recompiled.
* **Build settings.** `CARGO_BUILD_JOBS=1`, sequential builds.
* **No solver locally.** PlaneGCS cannot be built here: network policy denies
  its Boost archive host. So every local native run is OCCT with no solver.
  The solver-backed suite is covered only by the three-OS CI, and it was not
  re-run here.
* **No GUI.** The window scenario in the contract was not run in the cloud.
  Widget-level checks drive the real egui widgets and the create worker
  headlessly; no GUI pass is claimed.

## Changed and new files

* **Domain and document** (`ferritecad-document`):
  - `polygon.rs`: shared `simple_line_polygon`, `FullTurnRevolution`, and two
    tests;
  - `model.rs`: `Revolve`, `RevolveAxis`, `RevolveExtent`, and the
    `RevolveFace` role;
  - new `revolve.rs`: discovery;
  - `schema.rs`: `feature.revolve.v1` in `SUPPORTED_CAPABILITIES`;
  - `document.rs`: the capability required by each `RevolveFace` reference;
  - `validate.rs`: the Revolve's Profile edge;
  - `edit.rs`, `lib.rs`.
* **Kernel contract** (`ferritecad-kernel`):
  - `request.rs`: `RevolveRequest`;
  - `result.rs`: `RevolveResult`;
  - `kernel.rs`: `revolve()` and `revolve_cache_key`, with two tests;
  - `mock.rs`: an explicit refusal;
  - `handle.rs`: `FaceSurface::Cone`;
  - `lib.rs`.
* **Bridge** (`ferritecad-occt-bridge`):
  - `ferritecad_occt.h`: `fc_occt_revolve`, `fc_occt_revolve_faces`,
    `fc_occt_surface_axis`, `FC_OCCT_SURFACE_CONE`;
  - `bridge.cpp`: `BRepPrimAPI_MakeRevol` with checked sweep history. The
    extrude-only queries refuse revolved shapes.
* **Adapter** (`ferritecad-occt`):
  - `ffi.rs`: the header parity test is extended;
  - `kernel.rs`, `unavailable.rs`;
  - new `tests/revolve_occt.rs`.
* **Topology** (`ferritecad-topology`):
  - `map.rs`: `record_revolve`, `FeatureNames.revolved`;
  - `archive.rs`: `RevolvedFace`;
  - `codec.rs`: tag 15;
  - `resolve.rs`: the `RevolveFace` arm.
* **Evaluator** (`ferritecad-eval`):
  - `convert.rs`: `revolve_request`;
  - `cache.rs`: `revolve_archive_key`;
  - `cold.rs`: the Revolve arm;
  - `lib.rs`.
* **Create job** (`ferritecad-jobs`): `NewDocument::SketchRevolve` and
  `populate_revolve` in `create.rs`; `lib.rs`; `polygon.rs`.
* **CLI** (`ferritecad-cli`):
  - new `revolve.rs`: `create-sketch-revolve`;
  - `main.rs`;
  - `json.rs`: the operation and the `revolves` discovery;
  - `topology.rs`: `revolve face from segment …`;
  - new `tests/revolve.rs`.
* **App** (`ferritecad-app`):
  - `sketch.rs`: the Feature choice, the axis drawing, the draft and three
    tests;
  - `creates.rs`: reading back semantics.
* **CI and tools:** `.github/workflows/runtime-layout.yml` and
  `tools/check-fbx-complex.sh`.
* **Docs:** `README.md`, `docs/cli-capabilities.md`, `docs/cli-json-v1.md`,
  `docs/implementation-plan.md`, the new `docs/full-turn-revolve.md`, and this
  file.

No `Cargo.toml` or `Cargo.lock` changed, and no SQL schema changed. The Rust
notices, Rust SBOM, native inventory and product SBOM inputs are unchanged,
so none of them was regenerated.

## Native results (OCCT 8.0.1, no solver)

### Kernel: `ferritecad-occt --test revolve_occt`, 3 of 3

* **`native_full_turn_revolutions_are_what_pappus_and_their_lines_say`.**
  * **Profiles.** Bushing π(10²−4²)·15, stepped 750π and sloped 855π. Each
    was run as drawn, reversed, shifted by +2.625 mm in Y, and shifted and
    reversed: 12 solids.
  * **Volume.** OCCT's volume is within 1e-6 relative of Pappus.
  * **Faces.** There is one face per Line and no caps. Each face is found
    through its Line's own history entry and checked against that Line:
    - a radial Line gives a plane at its own y;
    - an axial Line gives a cylinder of its own radius, whose axis is the
      world Y axis;
    - a sloped Line gives a cone on the Y axis, with its mesh on the Line's
      own radius at every height;
    - every face is inside the Line's band and reaches all four quadrants.
* **`native_revolve_refuses_the_axis_other_kinds_and_cancellation`.** The
  bridge itself refuses a profile touching or crossing the axis (Input) and a
  circle (Unsupported). Cancelling before the build leaves no live shape.
* **`native_a_revolution_survives_the_named_archive_face_by_face`.** After an
  archive round trip, each slot is still its own Line's surface, and the
  volume is 750π.

### CLI: `ferritecad-cli --test revolve`, 6 of 6

* **`native_revolve_geometry_names_cache_reopen_and_exports`.**
  * **Geometry.** The same 12 variants go through `create-sketch-revolve`.
    Measured on reopen, the volume is within 1e-6 relative of Pappus.
  * **Names.** Every stored `RevolveFace` reference resolves to its own
    Line's face.
  * **STL, parsed independently.** Closed, oriented, inside the radial and
    axial band, a full turn, and a volume within the chord band.
  * **FBX.** Two files are written to `FCAD_REVOLVE_ARTIFACTS`.
  * **print-topology.** It names every reference
    `revolve face from segment <Line UUID>`, and 6 of 6 resolve.
  * **Cold and cache.** The cold build, Miss and Hit give equal volumes.
  * **Changed profile.** Widening the saved profile under the same sidecar
    gives a new Miss with a larger volume; then Hit, then an equal cold build.
* **`native_revolve_publication_delivery_and_cancellation`.** The
  publication envelope, and exit 7 when stdout fails after publication. A
  cancelled job leaves no destination and no temporary files.
* **`occt_without_solver_revolves_a_profile`.**
* **`revolve_request_refusals_and_usage_preserve_files`.** These requests are
  refused with exit 2, the request file is unchanged, and the directory
  gains nothing:
  - a missing request file;
  - `request_version` 2;
  - an extra `height_mm`;
  - no `axis` or no `angle`, `sketch_x`, `half_turn`, or `angle` given as
    the number 360;
  - a solid shaft on the axis, crossing the axis, or a vertex inside the
    1e-6 mm clearance;
  - a bow tie, two points, or a repeated closing point;
  - a coordinate over 1 000 000 mm;
  - a request over 65536 bytes, or text that is not JSON;
  - bad usage (no stdout), and an occupied output.

  Exit 7 applies when stdout fails after publication.
* **`revolve_alias_and_stub_kernel_refusals_preserve_storage`.** A
  destination that is an alias of the request is refused. A stub kernel
  publishes nothing.
* **`revolve_document_contract_and_discovery_without_kernel`.**
  * **Discovery.** It works without a kernel: a `revolves` entry, with
    `features` empty and the older editors refusing.
  * **Future capability.** A capability from a later build makes the document
    read-only, and its bytes are preserved.
  * **Missing capability.** A `RevolveFace` reference stripped of
    `feature.revolve.v1` is refused, and the refusal names the capability.

### Domain: `ferritecad-document --lib`, 91 passed, 1 ignored (pre-existing)

* **`a_full_turn_is_pappus_in_either_winding_and_any_axial_shift`.**
* **`a_full_turn_refuses_the_axis_and_whatever_the_polygon_refuses`.** A
  vertex at exactly `AXIS_CLEARANCE_MM` is refused and one at twice it is
  accepted. Mirrored profiles are refused, and so is everything the shared
  polygon validator refuses (including 257 vertices, which is covered only
  here, not at the CLI).

### Kernel crate: `ferritecad-kernel --lib`, 94 passed

* `kernel::tests::a_revolution_keys_apart_from_an_extrusion_and_by_its_profile`
* `kernel::tests::the_mock_refuses_a_revolution_instead_of_inventing_one`

### App: `ferritecad-app`

* **`sketch::tests::revolve_draft_widgets_name_the_axis_and_refuse_it`.** The
  real widgets choose Revolve 360°, and the canvas shows the axis text. A
  vertex on the axis is refused with the domain message. Undo/Redo of the
  feature choice follows from `feature` being part of the undoable `State`,
  but no test checks it; the window scenario does.
* **`sketch::tests::native_revolve_draft_worker_and_cli_publish_equivalent_models`.**
  - Save Cancel writes nothing, and a taken path is refused.
  - The shared worker publishes the draft.
  - A failed async Open restores the draft.
  - A CLI peer built from the same points gives equal semantics and the same
    STL bytes; the FBX files are the same length.

### Mixed and regression runs

* **Mixed.** The OCCT/no-solver gate `occt_without_solver_revolves_a_profile`
  was run exactly with `FERRITECAD_REQUIRE_PLANEGCS=0`: ok.
* **CLI regression (native):**

  | Suite | Result |
  |---|---|
  | `create` | 9 of 9 |
  | `sketch_extrude` | 4 of 4 |
  | `edit_sketch` | 5 of 5 |
  | `edit_extrude` | 2 of 2 |
  | `circular_cut` | 42 of 42 |
  | `edit_circular_cut` | 8 of 8 |
  | `json_v1` | 13 of 13 |
  | `json_import` | 3 of 3 |
  | `rebuild` | 9 of 9 |
  | `export_stl` | 9 of 9 |
  | `export_fbx` | 9 of 9 |
  | `print_topology` | 8 of 8 |
  | `validate` | 3 of 4 |

  The fourth `validate` test is described in the next item.
* **Permission test.** `validation_really_read_only_permissions` refuses to
  run as root, by design. Re-run under the unprivileged uid 1001 (`fcad`) with
  its own `TMPDIR`: ok.
* **Library crates:** document, topology, kernel, eval, jobs and occt all
  pass. That includes the ffi header parity test, extended to
  `fc_occt_revolve`'s parameter order and `FC_OCCT_SURFACE_CONE`.
* **App:** 323 passed. The 7 `constraints::tests::native_*` fail by their
  own assertion that a solver is present: they are the known-unsupported
  PlaneGCS suite, since this host's env requires a solver it cannot build.
  They pass in CI.

## Stub results (no OCCT)

* The `revolve` binary: 3 kernel-free tests pass and the 3 native tests print
  `skipped:`.
* `ldd` on the stub `ferritecad` shows no `libTK*`.
* `create-sketch-revolve` exits 2 with an `unsupported` envelope and creates
  no file.

## Old reader (9d224d7 CLI) on a Revolve document

* `validate`, `rebuild --cold` and `export-stl` refuse with exit 2, naming
  `feature.revolve.v1`.
* `inspect --json` exits 0 with `features: []`, and every editor refuses.
* The document's SHA-256 is unchanged afterwards.

## Recipe

The `FCAD_27A_AGENT_RECIPE` block was extracted from the contract exactly as
it documents, and run with the fresh debug CLI and
`FCAD_UFBX_READER=read_production`. It printed `FCAD_27A_RECIPE_OK 12`:
* 12 profiles checked;
* 3 FBX files read by ufbx, each with `checks=6 failures=0`;
* 5 refusals, and an occupied output left byte-identical.

## Mutations

Each mutation was applied by a script that kept a backup, then restored. The
file's SHA-256 was checked, and the positive tests re-ran green afterwards:
all 15 CI gate names run exactly, `fail=0`.

| # | Mutation | Killed by |
|---|---|---|
| 1 | `required_capabilities_of` stops requiring `feature.revolve.v1` for `RevolveFace` | `revolve_document_contract_and_discovery_without_kernel`: the stripped reference is accepted, and `expect_err` fails |
| 2 | `revolve_cache_key` ignores the request (profile, axis, turn) | `kernel::tests::a_revolution_keys_apart_…` (assertion at kernel.rs:296), and `native_revolve_geometry_…`, where the changed profile gets `Hit` instead of `Miss` |
| 3 | The bridge turns by π instead of 2π (bridge rebuilt) | `native_full_turn_…` and `native_a_revolution_survives_…`: the bridge's own history check refuses, because the solid has 6 faces but the Lines raised 4 |

## Checks

* `cargo fmt --all -- --check`: ok.
* `cargo clippy --workspace --all-targets -- -D warnings`: ok. It was run
  without `--all-features`, since planegcs is not buildable here; CI runs
  the full form.
* These scripts pass: `check-licence-headers.sh`, `check-export-boundary.sh`,
  `check-solver-ownership.sh`, `check-notice-ownership.sh` and
  `check-planegcs-pins.sh`.
* `runtime-layout.yml` parses as YAML.

## Resources

* Disk: 74% of the allowance used at the end (9.9 GiB free).
* Peak RSS stayed well under 16 GiB, and there was no swap.
* One sequential build at a time throughout.

## Limits

* The GUI window scenario was not run; it is written out for macOS in the
  contract.
* Locally, the solver-backed suites run only in CI.
* The CI evidence for the exact head is recorded in the PR once its runs
  complete.
