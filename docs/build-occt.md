# Building Open CASCADE for FerriteCAD

**Status:** stage 0 recipe, not yet exercised by CI.

FerriteCAD links Open CASCADE **dynamically and only dynamically**. This is a
licence requirement, not a preference: OCCT is LGPL-2.1 with the Open CASCADE
exception, and dynamic linkage with a documented replacement path is what keeps
FerriteCAD's own code distributable under MIT. See
[`THIRD_PARTY_LICENSES.md`](../THIRD_PARTY_LICENSES.md).

## Pinning the version

The architecture RFC names OCCT 8.0. Treat that as the intent, not the fact:
**the pinned version is whichever tag actually builds and passes the smoke test
on all three platforms**, recorded here with its checksum before any code
depends on it.

Fill in once all three platforms have passed:

| Field | Value |
| --- | --- |
| Tag | _to be recorded_ |
| Commit | _to be recorded_ |
| Source archive SHA-256 | _to be recorded_ |
| CMake version used | _to be recorded_ |
| Compiler per platform | _to be recorded_ |

**The commit is the authoritative pin, not the archive checksum.** GitHub
generates tag archives on demand rather than storing them, and their bytes have
changed before when the underlying archive format changed. The SHA-256 records
one particular download and is worth verifying; a build script that must still
be reproducible in three years should clone the commit.

Until this table is filled in, no build script may download OCCT. An unpinned
third-party C++ archive is a supply-chain hole, and the plan's rule is that
versions and checksums are fixed before the dependency is used
(implementation-plan.md, 4.4).

### Producing the record

Run the **OCCT pin** workflow from the Actions tab with the candidate tag, e.g.
`V7_9_0`. It builds OCCT from source on Linux, Windows and macOS, runs
[`tools/occt-smoke`](../tools/occt-smoke) against each build, and prints a
filled-in version of the table above in the run summary.

Prefer it to a manual run on one machine. The deliverable here is not "it
compiled" but the record of *which* compiler and CMake produced that result, and
a workflow log states that where a person's recollection does not. It also
covers platforms you may not have to hand.

A tag is pinnable only when all three platforms pass. Two green platforms and
one that was never run is not a pin, and the workflow fails rather than
reporting a partial result as a success.

## Required OCCT modules

Only what the adapter actually calls, to keep the shipped library set small:

- `FoundationClasses` — collections, `Standard_Failure`, `Message_ProgressRange`
- `ModelingData` — `TopoDS`, `BRep`, `Geom`
- `ModelingAlgorithms` — booleans, fillet, chamfer, shell, `BRepTools_History`
- `Visualization` is **not** required: FerriteCAD renders through `wgpu` and
  takes only tessellation data from OCCT
- `DataExchange` — STEP through XDE

Optional third-party dependencies (Tcl/Tk, FreeType, VTK, Qt, OpenGL samples)
are all disabled.

## Common CMake configuration

```sh
cmake -S occt -B build \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_LIBRARY_TYPE=Shared \
  -DBUILD_MODULE_Draw=OFF \
  -DUSE_TK=OFF -DUSE_TCL=OFF -DUSE_FREETYPE=OFF -DUSE_VTK=OFF \
  -DBUILD_DOC_Overview=OFF \
  -DCMAKE_INSTALL_PREFIX=../vendor/install
cmake --build build --config Release --parallel
cmake --install build --config Release
```

Leave `ApplicationFramework` and `DataExchange` on (defaults) so XDE/STEPCAF
works. Visualization toolkits are still built by a default OCCT tree because
XCAF imports link against them; disable optional viewer backends with the
`USE_*` flags above rather than assuming the whole module can be omitted on
every platform.

`BUILD_LIBRARY_TYPE=Shared` is not optional. A static build would change the
licence position entirely and must not be used, even for a local experiment
that might later be copied into a release.

## Per-platform notes

### Linux

Toolchain: GCC 12 or newer, or Clang 15 or newer.

```sh
sudo apt install build-essential cmake ninja-build
```

Libraries install to `vendor/install/lib` as `libTK*.so`. At run time they are
found through `RPATH=$ORIGIN/../lib` baked into the executable, so the
application never depends on the user's `LD_LIBRARY_PATH`.

### macOS

Toolchain: Apple Clang from Xcode Command Line Tools.

```sh
brew install cmake ninja
```

Two things differ from Linux and both must be handled before the first release
build, not after:

- The install name of each `libTK*.dylib` must be rewritten to
  `@rpath/libTK*.dylib`, with `RPATH=@executable_path/../Frameworks` in the
  bundle. Without this the application only runs on the machine that built it.
- Universal binaries need `-DCMAKE_OSX_ARCHITECTURES="arm64;x86_64"`, or two
  separate builds joined with `lipo`. Decide which before the packaging work
  starts; the second option is usually less painful with OCCT.

Anything shipped must also be signed and notarised, which requires the
dynamic libraries to be signed individually.

### Windows

Toolchain: MSVC from Visual Studio 2022 Build Tools.

```powershell
cmake -S occt -B build -G "Visual Studio 17 2022" -A x64 <flags above>
```

`TK*.dll` files are placed beside the executable. The debug and release C
runtimes cannot be mixed, so a debug FerriteCAD build needs a debug OCCT build.

## Smoke test

Before any Rust code touches OCCT, a plain C++ program must demonstrate that
the library works and that our expectations of it hold. The implementation is
[`tools/occt-smoke/`](../tools/occt-smoke/) (CMake + C++17, shared OCCT only);
build and run instructions are in its README.

1. build a box;
2. extrude a planar profile into a solid;
3. run a boolean cut;
4. apply a fillet to a selected edge;
5. serialise and deserialise the B-Rep;
6. read a STEP file through XDE and confirm units, a name and a colour survive;
7. write a STEP file and read it back;
8. tessellate with an explicit deflection and count the triangles.

Step 6 is the one that most often fails quietly. It is the reason the smoke
test exists rather than an assumption in a design document.

For XDE (steps 6–7) the OCCT install must include DataExchange **and** the OCAF
document toolkits XCAF depends on (`ApplicationFramework`). Stock OCCT CMake
packages also list Visualization toolkits (`TKService`/`TKV3d`) as link
dependencies of XCAF even when no viewer is used — the smoke test’s
`find_package` accounts for that; the program itself never opens a viewer.

## FFI boundary

The Rust side never sees an OCCT type. The first adapter is a flat `extern "C"`
C++17 shim. Its surface is deliberately composed of DTOs plus opaque owned
handles, so it can later be implemented by an out-of-process worker without
changing `cad-kernel`:

- no OCCT handle, exception type or container appears in the bridge header;
- every wrapper entry point catches `Standard_Failure` and every other C++
  exception, translating it into a structured adapter error before returning a
  status code; every exported entry point is `noexcept`;
- every OCCT object crossing the boundary is an opaque pointer with an explicit
  destructor function;
- long operations take a cancellation token and a progress callback;
- tolerances are passed explicitly, never left to a kernel default.

This boundary is chosen over `cxx` because OCCT's API is built on `Handle<T>`,
exceptions and non-movable types; `cxx` would still need an opaque wrapper for
every meaningful operation. The C ABI makes ownership, errors, progress and
future IPC explicit rather than hiding them behind generated glue.

## Packaging

The OCCT notice, the full LGPL-2.1 text and instructions for replacing the
library must ship in every package. This is a release gate
(implementation-plan.md, 11), and it is the condition on which the dynamic
linkage argument rests.
