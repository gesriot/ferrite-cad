# Building planegcs

planegcs is FerriteCAD's sketch solver, chosen in
[ADR 0001](decisions/0001-sketch-solver.md). It is LGPL-2.0-or-later, so it is
built as a shared library that whoever receives a build can replace with their
own. Nothing of it is compiled into a FerriteCAD binary.

Nothing in the FerriteCAD application loads it yet. This page is about the
component, the crate that owns it and the bench that measures it, not about a
release.

```
tools/build-planegcs.sh [output-directory]      # default ./vendor/planegcs
FCAD_PLANEGCS_DIR=<output> cargo test -p ferritecad-sketch-solver \
    --features planegcs -- --nocapture
FCAD_PLANEGCS_DIR=<output> cargo test -p ferritecad-solver-lab \
    --features planegcs -- --nocapture
```

On Windows, run it from a shell that has already seen `vcvars64.bat`, so cmake
finds MSVC's `cl.exe`.

## The pin

One file, [`tools/planegcs/pin.env`](../tools/planegcs/pin.env):

| | |
| --- | --- |
| source | FreeCAD 1.0.1, `src/Mod/Sketcher/App/planegcs` |
| archive | `https://github.com/FreeCAD/FreeCAD/archive/refs/tags/1.0.1.tar.gz` |
| SHA-256 | `f62bc07c477544eff62b6ab0fc3bb63fa7f1e6f94763c51b0049507842d444f3` |

The digest is checked before anything is extracted, not after. The same file
is read three times: by the build script that fetches the release, by the
provenance string compiled into the library, and by the lab's build script,
which compiles in what the library is expected to answer. A version number
kept in three places drifts, and the copy that drifts is the one nobody is
looking at.

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

Both are needed as headers only, at build time, and neither is redistributed
here. Eigen is MPL-2.0 and Boost is under the Boost Software Licence.

| | Headers used | Where the build finds them |
| --- | --- | --- |
| Eigen | `Eigen/Core`, `Dense`, `OrderingMethods`, `QR`, `Sparse` | `FCAD_EIGEN_INCLUDE`, else `/opt/homebrew/include/eigen3`, `/usr/local/include/eigen3`, `/usr/include/eigen3` |
| Boost | `boost/graph/adjacency_list.hpp`, `connected_components.hpp`, `graph_concepts.hpp`, `boost/math/constants/constants.hpp` | `FCAD_BOOST_INCLUDE`, else `/opt/homebrew/include`, `/usr/local/include`, `/usr/include` |

The pin workflow supplies a pinned Eigen 3.4.0 by digest to all three
platforms, so that a disagreement between them is about the platform. Boost
comes from each platform's package manager and the version it brought is
recorded in the run. The script itself accepts whatever a machine has: planegcs
1.0.1 builds against Eigen 3.4 and Eigen 5, and that was measured rather than
assumed.

## What is produced, per platform

| Platform | Library | Import library | Compiler in [run 32644085475](https://github.com/gesriot/ferrite-cad/actions/runs/32644085475) |
| --- | --- | --- | --- |
| Linux | `libplanegcs.so` | not applicable | GNU C++, ubuntu-latest X64 |
| macOS | `libplanegcs.dylib`, install name `@rpath/libplanegcs.dylib` | not applicable | AppleClang, macos-latest ARM64 |
| Windows | `planegcs.dll` | `planegcs.lib` | MSVC 19.51.36256, windows-latest X64 |

That run also recorded Eigen 3.4.0 by digest on all three, Boost 1_91 from
vcpkg on Windows and from the platform packages elsewhere, and the three
platforms agreeing over 43 semantic facts.

The import library is linker metadata. It carries no planegcs implementation;
what it does is let somebody relink against a library they replaced. Windows
produces a DLL and never a static copy: `add_library(planegcs SHARED ...)` is
checked afterwards against the target's actual type, and the build script
refuses a build that is not a shared library. Both refusals exist because this
file is what somebody edits when a platform will not link, and the licence
position is the thing that quietly gives way.

Beside the library the script writes the complete corresponding source
(`tree/`), FreeCAD's full `LICENSE`, a `PROVENANCE.txt` recording the release,
the checked digest, the platform and the compiler, and a `REPLACING.md`
generated from
[`tools/planegcs/DELIVERY.md.in`](../tools/planegcs/DELIVERY.md.in).

## Who owns what

One crate owns the whole boundary:
[`ferritecad-sketch-solver`](../crates/ferritecad-sketch-solver). It holds the
contract a caller states a sketch in, the Rust FFI, the MIT bridge, the
build-time detection and required mode, and the lifetime of the native session.

`ferritecad-solver-lab` is a *client* of it. It keeps the neutral corpus and
the reference Levenberg–Marquardt implementation, and it reaches planegcs only
by calling the product crate — no shim, no build script, no C ABI, no
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
  So the bench, which is such a dependent, finds the library the way a shipped
  application will — through the loader's search path. The pin workflow sets
  `LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH` or `PATH` accordingly, on all three
  platforms rather than on Windows alone.
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
answers a typed `Unavailable` — not a skipped test, not a panic, and never a
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
