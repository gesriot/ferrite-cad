# Building planegcs

planegcs is FerriteCAD's sketch solver, chosen in
[ADR 0001](decisions/0001-sketch-solver.md). It is LGPL-2.0-or-later, so it is
built as a shared library that whoever receives a build can replace with their
own. Nothing of it is compiled into a FerriteCAD binary.

Since §21A-2b1 the FerriteCAD application can load it. Built with the feature
on, `ferritecad-viewer` links the shared library, and
`ferritecad-viewer --solver-info` answers with what that library says about
itself. What has not been done is packaging: there is no relocatable release
that carries planegcs, because laying one out has to solve Open CASCADE's
loader layout at the same time, and that is §21A-2b2. A run of the application
against a build tree finds the library through the loader's search path, which
is an unbundled run and is not evidence about a package.

§21A-2b2a has since measured what such a release would have to carry and run a
candidate layout with both build trees taken away; the numbers and the layout
are in [runtime-layout.md](runtime-layout.md). The packager itself is still to
come.

Nothing above the loading is integrated either. No document stores a
constraint, no feature reads one, and no interface draws one.

```
tools/build-planegcs.sh [output-directory]      # default ./vendor/planegcs
FCAD_PLANEGCS_DIR=<output> cargo test -p ferritecad-sketch-solver \
    --features planegcs -- --nocapture
FCAD_PLANEGCS_DIR=<output> cargo test -p ferritecad-solver-lab \
    --features planegcs -- --nocapture

# The application, and the question it can answer about itself. The library
# is found by the loader, so name <output> in LD_LIBRARY_PATH on Linux,
# DYLD_LIBRARY_PATH on macOS or PATH on Windows.
FCAD_PLANEGCS_DIR=<output> cargo build -p ferritecad-app \
    --bin ferritecad-viewer --features planegcs
target/debug/ferritecad-viewer --solver-info
```

On Windows, run it from a shell that has already seen `vcvars64.bat`, so cmake
finds MSVC's `cl.exe`.

## The pin

One file, [`tools/planegcs/pin.env`](../tools/planegcs/pin.env), owns the
version, the archive URL and the SHA-256 of all three native inputs:

| | Version | Archive | SHA-256 |
| --- | --- | --- | --- |
| planegcs | FreeCAD 1.0.1, `src/Mod/Sketcher/App/planegcs` | `https://github.com/FreeCAD/FreeCAD/archive/refs/tags/1.0.1.tar.gz` | `f62bc07c477544eff62b6ab0fc3bb63fa7f1e6f94763c51b0049507842d444f3` |
| Eigen | 3.4.0 | `https://gitlab.com/libeigen/eigen/-/archive/3.4.0/eigen-3.4.0.tar.gz` | `8586084f71f9bde545ee7fa6d00288b264a2b7ac3607b974e54d13e7162c1c72` |
| Boost | 1.91.0 | `https://archives.boost.io/release/1.91.0/source/boost_1_91_0.tar.gz` | `5734305f40a76c30f951c9abd409a45a2a19fb546efe4162119250bbe4d3a463` |

Every digest is checked before anything is extracted, not after, and all three
are checked before any of them is unpacked, so a wrong one stops the build with
nobody's bytes on disk. A cached archive is checked again on every run: an
archive fetched once and trusted thereafter is an archive somebody can replace
between the two runs.

The same file is read four times: by the build script that fetches the three
releases, by the provenance string compiled into the library, by the lab's
build script, which compiles in what the library is expected to answer, and by
[`tools/check-planegcs-delivery.sh`](../tools/check-planegcs-delivery.sh),
which checks a finished delivery against it. A version number kept in several
places drifts, and the copy that drifts is the one nobody is looking at, so
[`tools/check-planegcs-pins.sh`](../tools/check-planegcs-pins.sh) runs in
ordinary CI and fails if a second copy appears in anything that runs, if a
workflow installs Eigen or Boost from a package manager, or if the component
artifact stops carrying the source, licence and provenance files.

## Which files are whose

**planegcs, LGPL-2.0-or-later, used byte-identical.** Named in
[`tools/planegcs/CMakeLists.txt`](../tools/planegcs/CMakeLists.txt) rather than
globbed, because this list is the answer to "which files are planegcs" and a
glob answers "whatever was in the directory":

- `App/planegcs/Constraints.cpp`, `GCS.cpp`, `Geo.cpp`, `qp_eq.cpp`,
  `SubSystem.cpp`, and the headers beside them;
- `boost_graph_adjacency_list.hpp`, from the same release.

**FerriteCAD's own, MIT, in [`tools/planegcs/glue/`](../tools/planegcs/glue).**
Four small files, each saying so in its first lines, written because FreeCAD's
versions reach into Qt and its build system:

| File | Why |
| --- | --- |
| `SketcherGlobal.h` | the one export macro planegcs includes. FreeCAD's reaches `FCGlobal.h` and from there into Qt |
| `FCConfig.h` | FreeCAD's is a build-system product full of platform probes; planegcs needs none of it, only for the include to resolve |
| `Base/Console.h` | two printf-style calls, silent here, because writing to a terminal inside a timed region measures the terminal |
| `provenance.cpp` | the function that says which planegcs this is |

**FerriteCAD's own, MIT, in the product crate.**
[`crates/ferritecad-sketch-solver/planegcs-bridge/`](../crates/ferritecad-sketch-solver/planegcs-bridge)
is the flat C boundary and holds no planegcs types.

Nothing under `App/planegcs/` is edited. The Windows export problem is solved
in FerriteCAD's `SketcherGlobal.h`, using the same `dllexport`/`dllimport`
mechanism FreeCAD's own header uses, because every planegcs class the shim
touches already carries the macro upstream.

## Eigen and Boost

Both are needed as headers only at build time, and `tools/build-planegcs.sh`
fetches both itself, by the digests above and by nothing else.

| | Headers used | Where the build gets them |
| --- | --- | --- |
| Eigen | `Eigen/Core`, `Dense`, `OrderingMethods`, `QR`, `Sparse` | the pinned archive, unpacked into `sources/eigen-3.4.0/` inside the delivery and compiled from there |
| Boost | `boost/graph/adjacency_list.hpp`, `connected_components.hpp`, `graph_concepts.hpp`, `boost/math/constants/constants.hpp` | the pinned archive, unpacked into `build-inputs/boost/` beside the delivery |

**The shim is compiled against the same two.** The MIT bridge in
`ferritecad-sketch-solver` includes the same planegcs headers, so its build
script takes Eigen from `sources/eigen-3.4.0/` and Boost from
`build-inputs/boost/` inside whatever `FCAD_PLANEGCS_DIR` names. That is not
only a provenance question: `GCS.h` templates on Eigen types that cross the
shim's boundary into the shared library, and a shim built against a different
Eigen agrees with that library about the function names and nothing underneath
them. It is also why the Boost headers sit under a name a delivery keeps rather
than in the helper's work directory.

There is deliberately no environment variable that redirects either one, and no
system include directory is consulted. `FCAD_EIGEN_INCLUDE` and
`FCAD_BOOST_INCLUDE` are cmake arguments now and nothing else; the pin workflow
sets both to empty decoy directories over the production build, so a helper
that read them again would stop at configure time rather than deliver a library
compiled against something nobody recorded. For an experimental build against
your own headers, drive
[`tools/planegcs/CMakeLists.txt`](../tools/planegcs/CMakeLists.txt) directly.

**The Eigen tree the library is compiled from is the Eigen tree the delivery
carries.** It is unpacked straight into `sources/eigen-3.4.0/` and cmake is
pointed at that directory, rather than at a scratch copy that is later
duplicated into the output. cmake reports which include directories it was
configured with in `planegcs-build-info-Release.txt`, and the helper refuses a
build whose Eigen or Boost is not the one it laid out, so the file somebody
edits to make a platform compile cannot quietly change what a recipient is
given.

**What Eigen's licence is here, measured rather than assumed.** Eigen is
primarily MPL-2.0 and its own `COPYING.README` says some files are under BSD or
LGPL. The compile closure of the five planegcs translation units on macOS is
262 Eigen files: 253 carry the MPL-2.0 header, seven carry no per-file notice
and fall under the project's MPL-2.0 default,
`Core/arch/Default/BFloat16.h` is Apache-2.0 from TensorFlow and
`Core/util/MKL_support.h` is BSD-3-Clause from Intel. No LGPL file is in it.
Cross-checked with Eigen's own mechanism: the same build with
`-DEIGEN_MPL2_ONLY`, which is a compilation error on any LGPL include, compiles
clean. The whole source tree is delivered regardless, `COPYING.*` files and
all, so the obligation is discharged without the measurement having to be
right.

**Boost 1.91.0 was checked before it was pinned.** The official
`archives.boost.io` release archive matches the SHA-256 that the publisher's
own `boost_1_91_0.tar.gz.json` records. Its `boost/` tree holds every header
planegcs reaches, pre-generated: nothing in the release distribution has to be
produced by Boost's build system first, `boost/version.hpp` included, which is
what makes one archive usable unchanged on all three platforms. The compile
closure is 1098 Boost files and every one of them references the Boost Software
License; none is under anything else. The longest relative path in the header
tree is 89 characters, which is why unpacking it on Windows does not run into
`MAX_PATH`.

## What is produced, per platform

| Platform | Library | Import library | Compiler in [run 32644085475](https://github.com/gesriot/ferrite-cad/actions/runs/32644085475) |
| --- | --- | --- | --- |
| Linux | `libplanegcs.so` | not applicable | GNU C++, ubuntu-latest X64 |
| macOS | `libplanegcs.dylib`, install name `@rpath/libplanegcs.dylib` | not applicable | AppleClang, macos-latest ARM64 |
| Windows | `planegcs.dll` | `planegcs.lib` | MSVC 19.51.36256, windows-latest X64 |

That run also recorded Eigen 3.4.0 by digest on all three, Boost 1_91 from
vcpkg on Windows and from the platform packages elsewhere, and the three
platforms agreeing over 43 semantic facts. Since §21A-2b2b0a Eigen and Boost
both come from the pin instead, and the summary the three platforms are
compared on carries the version and digest of all three inputs, so a platform
that built against something else is a difference rather than a detail in a
log.

The import library is linker metadata. It carries no planegcs implementation;
what it does is let somebody relink against a library they replaced. Windows
produces a DLL and never a static copy: `add_library(planegcs SHARED ...)` is
checked afterwards against the target's actual type, and the build script
refuses a build that is not a shared library. Both refusals exist because this
file is what somebody edits when a platform will not link, and the licence
position is the thing that quietly gives way.

Beside the library the script writes:

- `tree/`, the complete corresponding FreeCAD source, and FreeCAD's full
  `LICENSE` as `LICENSE-FreeCAD-LGPL-2.0-or-later.txt`;
- `sources/eigen-3.4.0/`, the exact Eigen source the library was compiled
  against, and that source's `COPYING.MPL2` as `LICENSE-Eigen-MPL-2.0.txt`;
- `build-inputs/boost/`, the exact Boost header tree used by the library and
  FerriteCAD's MIT shim, plus `LICENSE-Boost-BSL-1.0.txt` from the checked
  archive. A future runtime-only product package may omit these build headers
  under Boost's object-code exception; the rebuildable component artifact may
  not, because its documented `FCAD_PLANEGCS_DIR` command consumes them;
- `PROVENANCE.txt`, recording the version, archive URL and checked digest of
  all three inputs beside the platform and the compiler;
- `REPLACING.md`, generated from
  [`tools/planegcs/DELIVERY.md.in`](../tools/planegcs/DELIVERY.md.in).

[`tools/check-planegcs-delivery.sh`](../tools/check-planegcs-delivery.sh) is
what says that is true of a finished directory rather than of this paragraph.
It checks the three archives the helper left behind again, re-extracts Eigen
and Boost from their own archives, requires the delivered trees to be
byte-identical to those inputs, requires the MPL and Boost texts to belong to
those trees, and requires `PROVENANCE.txt` and `REPLACING.md` to name the
version, URL and digest of every input. Both pin workflows run it. The archives
are build-time evidence and are not uploaded; the checked Eigen and Boost trees
are the component artifact's rebuild inputs.

What is still missing before this output can be consumed by a FerriteCAD
packager is not native: the notices for the Rust dependency graph and the SBOM,
recorded in
[`ADR 0002`](decisions/0002-release-compliance-artifacts.md).

## Who owns what

One crate owns the whole boundary:
[`ferritecad-sketch-solver`](../crates/ferritecad-sketch-solver). It holds the
contract a caller states a sketch in, the Rust FFI, the MIT bridge, the
build-time detection and required mode, and the lifetime of the native session.

`ferritecad-solver-lab` is a *client* of it. It keeps the neutral corpus and
the reference Levenberg–Marquardt implementation, and it reaches planegcs only
by calling the product crate: no shim, no build script, no C ABI, no
constraint mapping of its own.

The direction matters both ways, and
[`tools/check-solver-ownership.sh`](../tools/check-solver-ownership.sh) checks
it on every ordinary CI run. A bench holding its own copy of the boundary would
be measuring a second implementation and reporting it as the product's; a
product able to reach into the bench could be handed the reference solver's
answer and nothing in the numbers would say so.

## How the crates link to it

The boundary is a flat C ABI, declared in `planegcs_shim.h`. Nothing of
planegcs's API reaches Rust.

- The shim is compiled as a **static** library and ends up inside the Rust test
  binary. That is not a licensing compromise: the shim is FerriteCAD's MIT
  code. A shared shim would need its own `dllexport` on Windows and its own
  DLL to be findable at run time, for nothing.
- planegcs stays **dynamic** beside it. The build script emits
  `rustc-link-lib=dylib=planegcs`, plus an `-Wl,-rpath` to the build directory
  on Linux and macOS.
- That run path reaches the **product crate's own** binaries and not a
  dependent's: cargo propagates a link *library* to the crates above but not a
  link *argument*, and the build script belongs to `ferritecad-sketch-solver`.
  The bench and `ferritecad-app` are both such dependents, and both find the
  library through the loader's search path. The pin workflow sets
  `LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH` or `PATH` accordingly, on all three
  platforms rather than on Windows alone. For the application that is an
  unbundled run, and the pin workflow requires it to carry no run path into the
  build tree, so what is being shown is the loader environment and not
  something a package would inherit.
- The library, not the shim, answers `fc_planegcs_provenance()`. A string
  compiled into the shim would go on saying "FreeCAD 1.0.1" beside a library
  built from anything at all.
- The shim counts the crossings, per thread. Three claims are otherwise
  unobservable: that a result attributed to planegcs came from planegcs rather
  than from arithmetic of our own, that a gesture used one native system rather
  than rebuilding it every sample, and that every session that was created was
  released exactly once. All three return the same coordinates either way, so
  `fc_gcs_native_solves`, `fc_gcs_native_sessions` and
  `fc_gcs_native_live_sessions` are what make them checkable.
- The application reaches all of this through `ferritecad-sketch-solver` and
  through nothing else. `ferritecad-app` has no build script, no `extern "C"`,
  no constraint mapping and no copy of the contract; its `planegcs` feature is
  forwarded to the solver crate rather than deciding anything, and
  `tools/check-solver-ownership.sh` fails ordinary CI if any of that comes back
  or if the application gains a dependency on the bench.
- There is **one** way to solve through the shim. The earlier one-shot
  `fc_gcs_solve` is gone: it built a system and mapped every constraint a
  second time, and two copies of that mapping is two places for it to be
  wrong.

## Off by default, and loudly

The `planegcs` cargo feature is off. With it on and no library present, the
build script warns and the lab reports the candidate as unavailable; ordinary
workspace CI runs `--all-features` on machines that have never built planegcs
and stays green, without reaching the network.

`FERRITECAD_REQUIRE_PLANEGCS=1` turns every reason to skip into a failure: no
feature, no library, no import library, an unloadable library. The
[pin workflow](../.github/workflows/planegcs-pin.yml) sets it, because a run
whose job is to prove planegcs works cannot pass by not having it.

Without a library the product crate still compiles, and every entry point
answers a typed `Unavailable`: not a skipped test, not a panic, and never a
quiet substitution of some other arithmetic. What it does *not* stop doing is
checking the sketch: an unknown point reference, a repeated identifier, a
coordinate that is not a number or a starting state of the wrong shape are
refused for what they are, in a build that has no solver to refuse them for any
other reason. Those gates therefore run in ordinary CI on all three platforms,
where there is no library at all.

## Replacing it

That is what the shared library is for, and the instructions ship with each
build in `REPLACING.md`. In short: rebuild from `tree/` with the same cmake
definition, or build any planegcs you like, put it where the old one was under
the same name, and rerun. On Windows keep the import library beside it so a
relink has something to read.
