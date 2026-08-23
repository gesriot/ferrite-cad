<!-- SPDX-License-Identifier: MIT -->
# The runtime closure of a FerriteCAD release

**Status:** measured on Linux, macOS and Windows by the
[combined runtime layout](../.github/workflows/runtime-layout.yml) workflow in
[run 32666664382](https://github.com/gesriot/ferrite-cad/actions/runs/32666664382),
which is where every number below comes from. A candidate layout has been chosen
and started from a clean environment on all three. Nothing here is a release, and
no packager exists yet: that is section 21A-2b2b.

## Why this needed its own slice

Two proofs about running FerriteCAD already existed, and neither of them was
this one.

The [OCCT pin](build-occt.md) builds pinned Open CASCADE and exercises it, but
every downstream executable it launches finds the kernel through a build tree
loader environment: `LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH` or `PATH`.

The [planegcs pin](build-planegcs.md) builds the real `ferritecad-viewer`
against planegcs and shows that the application imports it, finds it through the
loader and is stopped by the loader when it is taken away. That workflow links
no Open CASCADE at all.

So no run had ever produced a product executable carrying both pinned
components and then tried to move it out of both build trees. Until one did,
the layout of a release was not a thing anybody knew, and writing a packager
would have meant guessing at it.

## The failure this started from

On `64b7d95`, with the viewer built against pinned Open CASCADE and pinned
planegcs at the same time, copying the executable alone into an empty directory
and clearing the loader environment gives a real process failure and not a
missing build script:

```
$ ferritecad-viewer --solver-info
dyld[43782]: Library not loaded: @rpath/libplanegcs.dylib
  Referenced from: .../staging/ferritecad-viewer
  Reason: no LC_RPATH's found
exit 134
```

The reason is in the two build scripts. Both `ferritecad-occt` and
`ferritecad-sketch-solver` emit `cargo::rustc-link-arg=-Wl,-rpath,...`, and
cargo applies a link argument to the emitting package's own targets and does
not propagate it to a dependent. `ferritecad-viewer` lives in
`ferritecad-app`, so it carries **no run path at all**.

The command line tool is the more interesting half. Copied the same way, it
runs. Not because it is relocatable, but because the Open CASCADE this machine
had installed records absolute install names, so the copy still resolves to the
directory it was built against. A staged binary that works only on the machine
that built it and one that is genuinely relocatable are indistinguishable until
that directory is taken away, which is why the workflow renames it rather than
trusting the exit code.

## What the closure actually holds

Measured by [`tools/runtime-closure.sh`](../tools/runtime-closure.sh), which
walks the graph with the platform's own inspector, resolves each edge the way
the loader would, and classifies every member.

Against pinned Open CASCADE 8.0.1 and pinned planegcs, on all three platforms:

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Open CASCADE toolkits | 26 (66 178 968 bytes) | 50 (71 888 848 bytes) | 23 (36 045 312 bytes) |
| planegcs | 1 (813 848 bytes) | 1 (636 384 bytes) | 1 (495 616 bytes) |
| Unaccounted for | 0 | 0 | 0 |
| Viewer must ship | 27 files, 66 992 816 bytes | 51 files, 72 525 232 bytes | 24 files, 36 540 928 bytes |
| Command line tool must ship | 26 files, 66 178 968 bytes | 50 files, 71 888 848 bytes | 23 files, 36 045 312 bytes |
| Whole staged layout | 29 files, 101 537 685 bytes | 53 files, 92 084 448 bytes | 26 files, 56 749 568 bytes |

The executables are 21 678 408 and 5 277 312 bytes on Linux, 14 668 192 and
4 465 088 on macOS, 16 098 304 and 4 110 336 on Windows. Neither carries a run
path on any platform, which is the finding the whole slice started from.

The spread between the platforms is not noise. macOS has to ship every one of
the fifty toolkits and the other two ship roughly half, for the reason given
below, and that alone is thirty-five megabytes of difference.

Three findings are worth stating separately.

**Nothing unaccounted for, and that is a property of the pin rather than of
luck.** The same measurement against the Homebrew Open CASCADE 7.9.3 on the same
machine reports four unaccounted-for libraries: `libfreetype`, `libpng16`,
`libtbb` and `libtbbmalloc`, 1 270 112 bytes in total. The pin configures
`USE_FREETYPE=OFF`, `USE_TCL=OFF`, `USE_TK=OFF` and `USE_VTK=OFF`, and that is
why the product closure has none of them. No Qt and no FreeCAD runtime appears
in either closure.

**Whether the closure has a transitive half at all is a platform difference,
and it was measured rather than assumed.** On macOS every shipped library is
named directly by both executables: the transitive half holds only two system
frameworks, `IOKit` and `OpenGL`, reached through other system frameworks. That
is a consequence of `ferritecad-occt`'s build script handing the linker every
toolkit the bridge reports, and of Mach-O recording all of them whether or not a
symbol is used from each. Linux splits the same closure thirteen direct and
thirteen transitive, and Windows eleven and twelve, because `--as-needed` and the
import table drop what is not used. That is also why those two ship about half as
many toolkits.

The clean environment check therefore hides a transitive toolkit where one
exists and a directly named one where none does, and records which it was:
`reach=transitive` on Linux and Windows, `reach=direct-only` on macOS. A platform
that quietly lost its transitive half would show up in the comparison rather than
passing as equivalent.

**The versioned name is the one that has to be carried.** Pinned Open CASCADE
installs a chain, `libTKernel.dylib` to `libTKernel.8.0.dylib` to
`libTKernel.8.0.1.dylib`, and every install name in the closure points at the
middle one. A package therefore has to hold `libTKernel.8.0.dylib` as a real
file. The stager copies each closure member under the name that was referenced
and follows symlinks while doing it, so the layout carries no link chain for an
archiver to lose.

## Absolute paths present today

The pinned Open CASCADE install already gives every `libTK*.dylib` an
`@rpath` install name, and the planegcs delivery does the same, so on macOS
neither component carries an absolute path of its own. Neither carries an
`LC_RPATH` either. What is absent is the run path on the product executables,
and supplying it is the whole of the macOS rewriting.

The Homebrew build named above is the counterexample: it records absolute
install names under `/opt/homebrew`, and a staged copy against it resolves back
into the build machine.

## System libraries, which must not be copied

Classified by where they resolve rather than by a list of names, because a name
list is a list somebody has to remember to extend. On macOS that means the
frameworks and dylibs under `/usr/lib` and `/System/Library`, which have no file
on disk at all: they live in the dyld shared cache, so asking whether the file
exists would report the entire operating system as missing, and why the macOS
system total below is seventeen files of zero bytes. On Linux it means
what resolves under `/lib`, `/lib64`, `/usr/lib` or `/usr/lib64`, plus the
virtual DSO and the program interpreter. On Windows it means System32, SysWOW64,
WinSxS and the API set names the loader redirects, for which no file exists
anywhere.

Open CASCADE and planegcs are matched by name **before** location, so a
distribution that installed a toolkit into a system directory is still reported
as Open CASCADE and still counted against the licence obligations.

## Licence obligations the closure carries

Both components stay shared and replaceable, which is what the licence position
already rested on and what the workflow now checks against the artefacts rather
than against the CMake flags. Nothing in the closure requires modifying LGPL
source.

Open CASCADE is LGPL 2.1 with the Open CASCADE exception, and
`THIRD_PARTY_LICENSES.md` already makes shipping `LICENSE_LGPL_21.txt` and
`OCCT_LGPL_EXCEPTION.txt` a release gate. planegcs is LGPL 2.0 or later, and the
delivery beside it already carries the licence text, the complete corresponding
source and the replacement instructions. A package built from this closure
therefore inherits obligations that are already recorded and adds none: the
measurement found no third library to account for.

## The candidate layout

Built by [`tools/stage-runtime-layout.sh`](../tools/stage-runtime-layout.sh)
into the runner's temporary directory and started by
[`tools/check-staged-layout.sh`](../tools/check-staged-layout.sh). It is a
measurement staging directory. It carries no version, no archive, no signature
anyone should rely on and no installer, and it is deleted with the runner.

**Linux.** `bin/` for the executables and `lib/` for the libraries. The
executables look through `$ORIGIN/../lib` and the shipped libraries look through
`$ORIGIN`, so a toolkit finds its neighbours relative to itself rather than
relative to whoever loaded it. `patchelf --set-rpath` replaces rather than
appends, so an absolute `RUNPATH` left by the build tree is gone rather than
merely outvoted.

**macOS.** `FerriteCAD.app/Contents/MacOS` and
`FerriteCAD.app/Contents/Frameworks`. The executables get
`LC_RPATH=@executable_path/../Frameworks` and the libraries get
`@loader_path`; every shipped library's install name is rewritten to
`@rpath/<file>`, and any run path the build tree left is deleted first.

Editing load commands invalidates a Mach-O signature, and arm64 refuses to start
an image whose signature does not match, so each staged file is re-signed ad hoc
afterwards. That is measured and recorded here because it is a fact about
running the layout at all. It is not a notarisation claim and not a distribution
signature, and this slice makes neither.

The candidate bundle has no `Info.plist`. It is started by path, which is what
the measurement needs; whether a released bundle needs one is a packaging
question and belongs to 21A-2b2b.

**Windows.** `bin/`, with the executables and the DLLs beside them, which is the
only layout the loader resolves without an environment variable. There is no run
path on this platform. `planegcs.lib` is linker metadata and is not part of the
runtime layout: it holds no planegcs implementation and must not be shipped as
if it were one.

## How the check refuses to pass for the wrong reason

The clean environment smoke asserts rather than arranges. It refuses to run when
`LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH` or `DYLD_FALLBACK_LIBRARY_PATH` is set,
because emptying them itself would make the check pass for a caller that had
left them pointing at the build tree. It refuses when a directory it was told is
gone still exists. It greps every staged file for the names of those
directories. On Windows it refuses when any `PATH` entry holds product runtime
libraries.

Then it runs both halves of the product, because each says nothing about the
other. `--solver-info` says a great deal about planegcs and nothing whatever
about Open CASCADE, so the command line tool rebuilds a copy of the committed
plate fixture and the report has to name `occt` as the kernel and to have built
a shape. No new viewer command was added to measure Open CASCADE; the existing
`rebuild --cold` already crosses that boundary, and it writes nothing, which is
checked.

Finally it takes libraries away. The staged planegcs is hidden and the viewer
must fail to start; a required Open CASCADE toolkit is hidden and the command
line tool must fail to start; both are put back and both must pass again. A
gate that only ever saw the broken state cannot tell a package that stopped
working from one that never did.

## What this does not give

There is no packager, no archive, no installer, no signing or notarisation
claim, and no release artifact. The staged directories live inside a runner's
temporary space and are not a delivery. Nothing about constraints, schema,
payload, evaluation, rebuild or sketch UI changed, and no fallback solver
exists. 21A-2b2b is the packager, and it is meant to be written against these
numbers rather than against a guess.
