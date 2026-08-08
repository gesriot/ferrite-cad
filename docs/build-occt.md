# Building Open CASCADE for FerriteCAD

**Status:** pinned and verified on Linux, Windows and macOS.

FerriteCAD's engineering policy is to link Open CASCADE **dynamically and only
dynamically**. OCCT is LGPL-2.1 with the Open CASCADE exception. LGPL-2.1 also
describes compliance paths for other forms of linking, but FerriteCAD does not
support those distribution and relinking obligations. Shared libraries with a
documented replacement path are the one path this project builds and tests. See
[`THIRD_PARTY_LICENSES.md`](../THIRD_PARTY_LICENSES.md).

## Pinning the version

The architecture RFC names OCCT 8.0 as the intent. The pin below is the fact:
the tag that actually built and passed the smoke test on all three platforms.

| Field | Value |
| --- | --- |
| Tag | `V8_0_1` |
| Commit | `b8f597c677811d1f9f4d8a97f5ae2825c0353a42` |
| Source archive | `https://github.com/Open-Cascade-SAS/OCCT/archive/b8f597c677811d1f9f4d8a97f5ae2825c0353a42.tar.gz` |
| Source archive SHA-256 | `dba62b81078dd43cec23feba89432be301582341001edad1b93342ad8bda35ea` |
| Source archive size | 45 131 814 bytes |
| Reported version | `OCC_VERSION_COMPLETE=8.0.1` |
| Verified by | [run 31254884697](https://github.com/gesriot/ferrite-cad/actions/runs/31254884697), 2026-08-08 |

`V8_0_1` is an annotated tag: the ref itself names tag object
`c5605924864829ce8c1e1477f976ffb3880538a8`, which peels to the commit above.
The lightweight tag `V8.0.1` names the same commit. The pin records the commit,
so which of the two names was typed does not matter.

Toolchain each platform actually used, as recorded by that run:

| Platform | Runner image | CMake | Compiler |
| --- | --- | --- | --- |
| Linux | `ubuntu24 20260720.247.2` (x64) | 3.31.6 | GNU 13.3.0 |
| Windows | `win25-vs2026 20260803.193.1` (x64) | 4.4.2 | MSVC 19.51.36252.0, toolset 14.51.36231 |
| macOS | `macos26 20260728.0273.1` (arm64) | 4.4.0 | AppleClang 21.0.0.21000101 |

All eight smoke-test steps passed on all three. Steps 6 and 7 — the ones that
matter and the ones that fail quietly — confirmed that a STEP round trip through
XDE preserves the shape name `AS1_PE_ASM`, the colour `RGB(0,1,0)` and the unit
declaration `length_unit_mm=1`. Tessellation produced 284 triangles on every
platform, unchanged from `V8_0_0`. `LICENSE_LGPL_21.txt` and
`OCCT_LGPL_EXCEPTION.txt` were present in all three source trees; their absence
now fails the run rather than warning.

`V8_0_1` is 40 commits ahead of `V8_0_0` and carries fixes to the STEP writer,
to fillet and chamfer, and to several crash and null-dereference paths
([comparison](https://github.com/Open-Cascade-SAS/OCCT/compare/V8_0_0...V8_0_1)).
The smoke test exercises too little to show any of that; it is evidence the
version builds and keeps STEP metadata intact, not evidence the fixes work. The
reason to prefer it is that those areas are ones FerriteCAD will lean on, and
adopting the patch release before any Rust code depends on the kernel is
cheaper than moving later.

**The commit is the authoritative pin, not the archive checksum.** GitHub
generates tag archives on demand rather than storing them, and their bytes have
changed before when the underlying archive format changed. The checksum above
is of the *commit* archive, which is what the workflow downloads; a checksum
taken from `.../archive/refs/tags/V8_0_1.tar.gz`, or from the `.zip`, will
differ without anything being wrong. A build script that must still be
reproducible in three years should clone the commit.

The previous pin, `V8_0_0` / `d3056ef80c9668f395da40f5fd7be186cae4501f`, also
passed all three platforms in
[run 31227437597](https://github.com/gesriot/ferrite-cad/actions/runs/31227437597).

Re-running the pin workflow for a new tag is how this table changes. It should
not be edited by hand from a single machine: a pin is a claim about three
platforms.

### Producing the record

Run the **OCCT pin** workflow from the Actions tab with the candidate tag, e.g.
`V8_0_1`. It resolves that tag once, downloads the source snapshot by the
resulting immutable commit, builds OCCT on Linux, Windows and macOS, runs
[`tools/occt-smoke`](../tools/occt-smoke) against each build, and prints a
filled-in version of the table above in the run summary.

Commit-based download is deliberate. Downloading by tag after recording its
commit leaves a race in which a moved tag can make the table name one commit
while the runners compile another.

Prefer it to a manual run on one machine. The deliverable here is not "it
compiled" but the record of *which* compiler and CMake produced that result, and
a workflow log states that where a person's recollection does not. It also
covers platforms you may not have to hand.

A tag is pinnable only when all three platforms pass. Two green platforms and
one that was never run is not a pin, and the workflow fails rather than
reporting a partial result as a success.

Build-cache reuse is optional. Cached installs carry the CMake version,
configured compiler and original runner identity that produced them; a reused
build reports that stored provenance rather than incorrectly attributing the
binary to the current runner. The cache key also includes the resolved commit,
platform and runner architecture.

## Required OCCT modules

The adapter directly calls only this subset:

- `FoundationClasses` — collections, `Standard_Failure`, `Message_ProgressRange`
- `ModelingData` — `TopoDS`, `BRep`, `Geom`
- `ModelingAlgorithms` — booleans, fillet, chamfer, shell, `BRepTools_History`
- `Visualization` has no direct FerriteCAD API use: rendering goes through
  `wgpu`, and the adapter takes only tessellation data from OCCT
- `DataExchange` — STEP through XDE

That does not make the Visualization module optional in the stock OCCT build.
XCAF/DataExchange toolkits link to Visualization toolkits such as `TKService`,
so they remain transitive build and run-time dependencies even though
FerriteCAD never opens an OCCT viewer.

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

`BUILD_LIBRARY_TYPE=Shared` is not optional under FerriteCAD's distribution
policy. A static build would require a different compliance, packaging and
testing path that this project does not support; do not let one enter a release
by way of a local experiment.

## Per-platform notes

### Linux

Toolchain: GCC 12 or newer, or Clang 15 or newer.

```sh
sudo apt install build-essential cmake ninja-build \
  libx11-dev libxext-dev libxmu-dev libxi-dev \
  libgl1-mesa-dev libglu1-mesa-dev
```

The X11 and GL headers are not optional, despite FerriteCAD rendering through
`wgpu` and never opening an OCCT viewer. XCAF links against the Visualization
toolkits, so `TKService` is compiled whatever the `USE_*` flags say, and it
includes `X11/Xlib.h`. Without these packages the build fails partway through
with `fatal error: X11/Xlib.h: No such file or directory` — around fifteen
minutes in, after 1700 other objects have compiled. macOS and Windows use their
native windowing headers and need nothing extra.

Libraries install to `vendor/install/lib` as `libTK*.so`. The raw install used
by the pin workflow currently needs `LD_LIBRARY_PATH` for its smoke test:
modern linkers emit non-transitive `RUNPATH`, while OCCT toolkits load other
OCCT toolkits that the executable does not name directly.

A distributed application must not depend on the user's environment. The
packaging stage must give every shipped OCCT library an `$ORIGIN`-relative
search path (or apply an equivalent, verified loader layout) and test the
result from a clean environment. That release property is not established by
the pin workflow alone.

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

Toolchain: the MSVC C++ toolset, from Visual Studio Build Tools or any Visual
Studio edition that includes it. **Do not name a Visual Studio generator.**

`-G "Visual Studio 17 2022"` fails with *"could not find any instance of Visual
Studio"* on machines that have moved past VS 2022, which now includes both
current developer installs and the GitHub runner image. A generator that names
one Visual Studio release has to be edited on every release, and CMake cannot
target a Visual Studio it does not yet know about.

Use Ninja instead. It names no version, so the build follows whichever MSVC is
installed, and it is single-config, so `--config` disappears:

```bat
:: from a developer prompt, or after calling vcvars64.bat
cmake -S occt -B build -G Ninja ^
  -DCMAKE_C_COMPILER=cl -DCMAKE_CXX_COMPILER=cl ^
  <flags above>
cmake --build build --parallel
cmake --install build
```

`cl` is selected explicitly because a MinGW toolchain earlier on `PATH` will
otherwise be picked ahead of MSVC once the generator no longer implies a
compiler. This is not hypothetical: it happens on machines with MinGW installed
and on the CI runners.

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
(implementation-plan.md, 11), and it is part of the project's chosen
shared-library compliance path.
