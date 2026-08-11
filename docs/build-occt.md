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

All eight smoke-test steps passed on all three. That historical run used the
since-removed Pro/ENGINEER fixture of unrecorded origin: steps 6 and 7 confirmed
that its name `AS1_PE_ASM`, colour `RGB(0,1,0)` and unit declaration
`length_unit_mm=1` survived a STEP round trip through XDE. The current smoke
input is FerriteCAD's synthetic, provenance-recorded
`fixtures/step/canonical/02-flat-assembly.step`; it passed the same gates on all
three platforms in [pin run 31410527797](https://github.com/gesriot/ferrite-cad/actions/runs/31410527797).
Tessellation produced 284 triangles on every platform, unchanged from
`V8_0_0`. `LICENSE_LGPL_21.txt` and
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

- The pinned 8.0.1 install already gives each `libTK*.dylib` an
  `@rpath/libTK*.dylib` install name, as verified from the adapter executable
  in pin run
  [31273458848](https://github.com/gesriot/ferrite-cad/actions/runs/31273458848).
  The app bundle must still provide `LC_RPATH=@executable_path/../Frameworks`,
  place the libraries there, and verify the result from a clean environment.
  A future OCCT package that
  reintroduces absolute install names must be rewritten during packaging.
- `ferritecad-occt` adds the raw install directory as an RPATH to its own test
  executables, but Cargo does not propagate that package-local link argument
  to downstream binaries. The pin workflow therefore sets
  `DYLD_LIBRARY_PATH` while its process-level CLI gates launch the unbundled
  debug executable. This only makes the pinned build tree runnable during the
  test; it is not a substitute for the bundle-relative RPATH above.
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

STEP output is not byte-deterministic merely because its header is fixed.
`STEPCAFControl_Writer` may emit independent colour/style entities in a
different order: a flat two-colour assembly produced two byte layouts across
six processes on OCCT 7.9.3, and OCCT 8.0.1 differed between platforms in
[pin run 31402276003](https://github.com/gesriot/ferrite-cad/actions/runs/31402276003).
Both layouts read back into the same names, assembly tree, placements, colours,
units and geometry. Consequently `tools/build-step-corpus` compares a sorted
semantic manifest; the SHA-256 of a committed STEP file is an integrity check
for that artifact, not a promise that regeneration reproduces its encoding.

## What Open CASCADE actually does, measured

Two questions the adapter had to answer empirically rather than from
documentation. Both were measured on OCCT 7.9.3 and are asserted by tests in
`crates/ferritecad-occt`.

**`BRepBuilderAPI_MakeWire` replaces the edges it welds.** Building four edges,
adding them to a wire and then asking `BRepTools_History::Generated(edge)`
which face each raised returns a face for the *first* edge and nothing for the
rest: MakeWire re-creates the others in order to share vertices between
neighbours, so the edges we built are no longer in the shape. Creating the
corner vertices first and building each edge between two of them leaves nothing
to weld, every edge keeps its identity, and history is complete. A naming layer
built on the naive version would have silently lost three quarters of its
references.

**`BRepPrimAPI_MakePrism` never polls the progress indicator.** A
`Message_ProgressIndicator` whose `UserBreak` returns true is called zero times
during a prism, and the build completes normally. Cancellation for extrusion is
therefore checked between steps — before the profile is built and before the
sweep — and not inside them. The indicator is installed anyway, for the
algorithms that do poll it, but no claim is made that a long extrusion can be
interrupted.

`FirstShape()` and `LastShape()` return a `TopoDS_Face` for a prism, which is
why the adapter reports the caps directly rather than exploring for them.

## The shim is static; Open CASCADE is dynamic

`crates/ferritecad-occt-bridge` builds as a **static** library and is linked
into the Rust binary. The dynamic-linking policy is about Open CASCADE, which
stays dynamic behind it; the shim is FerriteCAD's own MIT code and shipping it
inside the executable raises no licence question.

Static was not the first choice, and the reasons it became one are worth
keeping. A shared shim exports nothing on Windows without
`__declspec(dllexport)`, so the Rust link failed with `LNK2019: unresolved
external symbol fc_occt_*` on a build that had just compiled cleanly. Fixing
that leaves a second problem: Windows has no RPATH, so the shim's own DLL then
has to be findable at run time as well as Open CASCADE's. A static shim has
neither problem on any platform.

Two consequences for the Rust build script. It must name the Open CASCADE
toolkits itself, so CMake writes the resolved list into a file rather than
leaving the Rust side to guess which toolkits a given build provides. And a
static C++ library dragged into a Rust link needs the C++ runtime named
explicitly — `c++` on macOS, `stdc++` on Linux — because rustc assumes only a C
one; MSVC picks its runtime up from the object files.

Dynamic linkage is verified in the pin workflow against the **test executable**,
not the shim: the shim is inside it, and it is the executable that names Open
CASCADE at run time.

## What a cached B-Rep does and does not restore

Shapes are serialised with `BinTools`, Open CASCADE's binary B-Rep format,
wrapped in a four-byte magic, a format version, the payload length and a
BLAKE3-256 payload digest of FerriteCAD's own. The length rejects appended and
truncated data; the digest rejects same-length corruption before untrusted
bytes reach `BinTools`. The format and kernel versions move independently: the
kernel identity changes when Open CASCADE or the bridge build changes, while
the blob format version changes when FerriteCAD changes what it stores around
the kernel's bytes. A blob is refused unless both agree and its framing is
internally consistent.

Triangulation is deliberately not written into the blob. A tessellation belongs
to its own cache key at its own deflection, and bundling one here would tie two
independent results together.

**A decoded shape carries geometry and nothing else.** The format stores a
shape, not the history of the operations that produced it, so a restored shape
has no side faces and no caps. The bridge refuses those queries on a decoded
shape rather than answering with an empty list, which a naming layer would read
as "this feature produced nothing".

That is why a warm-cache rebuild is not yet wired to the evaluator. Reusing a
cached solid is only safe once the topology mapping is stored beside it and
restored with it; until then a cache hit would produce correct geometry with no
names, which is worse than a slower rebuild.

The cache key includes a full BLAKE3-256 digest of the bridge's own sources,
target and configured C++ toolchain, not the crate version. The code, compiler
and flags that produce the geometry change independently from releases, so
keying on the crate version would go on serving results computed by a build
that no longer exists. Comment-only edits invalidate the cache too; that costs
a rebuild, where the alternative costs a wrong answer served quickly.

This v2 framing, the build identity and 25 adapter tests against real geometry
were verified with pinned OCCT 8.0.1 on Linux, Windows and macOS in
[run 31275991427](https://github.com/gesriot/ferrite-cad/actions/runs/31275991427).

## A fillet can report success and hand back nonsense

The most important measurement in this project so far. On a 60 x 40 x 10
plate, `BRepFilletAPI_MakeFillet` over every edge behaves like this on 7.9.3:

| radius | `IsDone()` | `BRepCheck_Analyzer` | volume |
|-------:|:-----------|:---------------------|-------:|
| 4.0    | true       | valid                | 22 575 |
| 4.9    | true       | valid                | 21 890 |
| 5.0    | **false**  | —                    | —      |
| 5.1    | **true**   | **invalid**          | **25 815** |
| 6.0    | **true**   | **invalid**          | **25 088** |
| 20.0   | false      | —                    | —      |

Half the plate's thickness is 5 mm, so 5.0 failing is correct. What matters is
the band above it: the builder reports success and returns a shape that fails
the analyser and encloses more material than the 24 000 mm³ block it was cut
from. Rounding a convex edge removes material. Those shapes are not poor
answers, they are not answers, and a caller that trusted `IsDone()` would put
one in a document.

So the adapter checks every result with `BRepCheck_Analyzer` and refuses what
fails, issuing no handle. `IsDone()` alone is not a contract.

**Confirmed on the pinned kernel.** The table above was measured on 7.9.3, and
the pin workflow now records which path each refusal took. On OCCT 8.0.1, on
Linux, Windows and macOS alike:

```
radius 5.0: Open CASCADE could not round every edge of this shape
radius 5.1: produced a shape Open CASCADE reports as invalid; it is refused
radius 6.0: produced a shape Open CASCADE reports as invalid; it is refused
```

The band where the builder claims success and produces an invalid solid is
still there on the version this project ships against, so the check is
load-bearing rather than a precaution against an older release.

## What the corpus measured

Twenty procedural parts — blocks of varying proportion, L-shapes, and outlines
whose corners are arcs — swept from well inside each part's nominal limit to
well past it, on every platform in the pin workflow.

**Fillets never silently misbehaved once the check above was in place.** Every
part had at least one successful radius, and every other request was an
explicit refusal rather than an invalid handle. The nominal half-dimension is
a sweep scale, not a universal boundary: nineteen parts stopped below it, but
`rounded-tall` still rounded at the full nominal value. Two readings are worth
keeping:

- An L-shape's limit is set by its narrowest arm, not by its bounding box.
  `ell-thick` (50 x 50 x 40, 20 mm cut) rounded to 6.7 mm where its bounding
  box suggests 20 mm.
- `rounded-tall` rounded at its full nominal limit, so the nominal figure is a
  sweep scale, not a prediction.

**Shells are robust on sharp-cornered parts and weaker on cylindrical ones.**
Blocks and L-shapes hollowed to walls well past their nominal limit — the real
constraint is the wall against the part's thickness. Parts with small corner
radii are where `BRepOffsetAPI_MakeThickSolid` struggles: `rounded-tight`
(30 x 30 with 2 mm corners) hollowed only to 1.3 mm, and `rounded-tall`
(30 x 30 x 60 with 6 mm corners) to 5 mm, both refusing ten of twelve steps.
That is the shape of the risk for a shell feature: not sharp geometry, but
small-radius cylindrical faces.

Both operations were deterministic on all twenty parts: two identical fillet
requests and two identical shell requests gave identical face counts and
volumes to within 1e-9. These are separate assertions; repeating only the
fillet would say nothing about the offset algorithm behind shell.

The pin workflow runs the adapter tests with captured output enabled and keeps
`adapter-test-output.txt` beside each platform's pin report. Besides the sweep
tables, the output records whether the 5.0/5.1/6.0 plate boundary was refused
by the builder itself or by FerriteCAD's validity check. A green test alone
proves the policy but cannot distinguish those mechanisms.

## What identifies an imported definition

Durable selection into an imported assembly — "this bolt, the one I picked" —
needs a key that names something in the file rather than something in the
reader. Four candidates were ruled out before measuring: a `TDF_Label` entry is
a position in a document Open CASCADE built this run, a name is neither unique
nor always present, a position in the definition list is exactly what
`PersistedScene::bind` already refuses to trust, and geometry is a guess
wearing a number.

`tools/step-key-probe` measures what is left, on both halves of the corpus, and
the pin workflow runs it on all three platforms. Two things it distinguishes
that are easy to confuse: `#12` is an identifier the file wrote, while `(#12)`
is where the entity sits in the model this run. Open CASCADE prints them
similarly and only the first is an identity.

**PRODUCT_DEFINITION is present, unique and stable on every file that produces
a scene except the one built to collide identifiers.** The two files Open
CASCADE refuses outright have no scene and so nothing to key, which is the
right answer rather than a gap. The identifiers are identical on Linux, Windows
and macOS, and identical when the same bytes are read a second time in a second
reader.

**The route to it is typed, not a graph search.** The chain runs up and then
down, and every transition checks both the concrete STEP type and the field
that refers back to the entity just visited. A hop-limited walk over arbitrary
`Sharings` would still be a graph crawl, and could still arrive at a
neighbouring part:

    MANIFOLD_SOLID_BREP
      <- shared by  ADVANCED_BREP_SHAPE_REPRESENTATION
      <- shared by  SHAPE_DEFINITION_REPRESENTATION
      -> refers to  PRODUCT_DEFINITION_SHAPE
      -> refers to  PRODUCT_DEFINITION

The assembly half is equally strict: only
`NEXT_ASSEMBLY_USAGE_OCCURRENCE` is accepted, not its generic
`PRODUCT_DEFINITION_RELATIONSHIP` base class.

**An assembly has no geometry, so it has no entity of its own.** The shape
entity is available for only four of the ten files' definitions — every leaf
part has one and no assembly does. An assembly is instead named by the
occurrences that put its components inside it: `NEXT_ASSEMBLY_USAGE_OCCURRENCE`
relates the assembly to each component, so the parent every component agrees on
identifies it. This is why `shape entity` is measured and reported but cannot
be the key: it disappears exactly on the nodes a user is most likely to select.

Two findings came out of getting this wrong first, and both are recorded
because they are the shape of the risk rather than incidents:

- One assembly may contain the same part several times, and each placement is
  its own occurrence. Collecting parents without collapsing duplicates made a
  correct nested assembly look unidentifiable.
- Two assemblies with identical component sets cannot be told apart from their
  components alone. The probe reports no key in that case rather than choosing,
  and the corpus does not currently contain such a file.

**What a collided identifier does, measured.**
`06-duplicate-product-definition.step` exists because the corpus previously
could not answer this: `05-duplicate-entity-id.step` duplicates a shape
representation, and duplicates it identically, so nothing about it is
ambiguous. The new file writes `#31` twice with different contents — the second
carrying another part's formation and context — while every reference in the
file still resolves.

Open CASCADE reads it (`RetDone`), says `F: Ident defined SEVERAL TIMES : #31`
at load, transfers it successfully, and produces the same three nodes with the
same volumes as the undamaged assembly. **The geometry is untouched; what is
lost is an identity.** The two parts keep their keys, and the *assembly* loses
its own: its components' occurrences name the other `#31`, so the parent every
component agrees on is empty and the probe reports no key rather than choosing
between them.

That is the failure mode to design for, and it is not the obvious one. A
collided identifier does not produce two definitions sharing a key, which a
uniqueness check would catch. It produces a node with **no** key, in a file that
imports and looks entirely normal. An importer must therefore check that every
definition has a key, not only that the keys it has are distinct — and the load
diagnostic is available to explain why, which is more than
`04-corrupted-number.step` offers.

Uniqueness across arbitrary vendor files remains a measurement on this corpus
rather than a property of the format, so both checks belong at read time.

The workflow's denominator is independent of the key probe: the earlier
diagnostic probe counts files that transferred, and the key gate requires the
same number of files with definitions. Otherwise a regression that silently
dropped one file could still report a smaller but apparently perfect `N/N`.

## What meshing does, and does not, do

Three things about `BRepMesh_IncrementalMesh` that the adapter depends on,
measured on 7.9.3.

**It polls the progress indicator.** A six-faced box asked for 32 user-break
checks. This is the opposite of `BRepPrimAPI_MakePrism`, which polls zero
times, so tessellation can be stopped partway rather than only between
operations. Worth knowing before assuming the whole family behaves alike.

**The triangulation carries no normals.** `Poly_Triangulation::HasNormals()`
is false for every face of a freshly meshed prism, so the adapter computes
them: a node's normal is the average of the triangles meeting at it inside its
own face, which is exact for a plane and smooth across a cylinder.

**Orientation is not applied for you.** Five of a prism's six faces come back
`TopAbs_REVERSED`, and the top cap has a non-identity location. A caller that
took the stored winding at face value would light most of the solid inside out
and put one face in the wrong place. The adapter swaps the winding and negates
the normals of reversed faces, and transforms every node by its face's
location.

## A tessellation must not inherit the previous tessellation

Open CASCADE keeps triangulation on the shape. That is not only an internal
cache: it changes the answer. On a half-cylinder, a coarse request made after a
fine one reused the fine mesh and returned 632 triangles, while the same coarse
request on a fresh shape returned 28. Since tessellation parameters participate
in FerriteCAD's mesh cache key, serving those two different answers under one
key would be a correctness bug.

The bridge therefore calls `BRepTools::Clean` before every meshing pass. The
two calls of the caller-owned-buffer protocol each mesh from a clean shape, and
a permanent curved-profile regression compares `fine → coarse` with `coarse on
a fresh shape`, including positions, normals and indices.

The bridge also cleans after copying the mesh into caller-owned vectors. Raw
OCCT behaviour without that cleanup is surprising: even though `BinTools::Write`
is asked for no triangles and no normals, a prism's payload before and after
meshing has the same length and a different checksum. A mesh-free
`BRepBuilderAPI_Copy` did not help; with `copyMesh` off it produced a payload
twice the size and was still unstable. Cleaning the original shape does: a
regression test now requires byte-identical B-Rep before and after drawing, so
transient rendering state cannot leak into persistence.

## A shape-set index is not a name

The obvious way to point at a face inside a cached B-Rep is `BinTools_ShapeSet`:
it hands out an index per shape, `Index()` looks one up and `Shape()` gives it
back. It does not work, and the way it fails is silent.

`Index()` strips the location. The top cap of a prism is the bottom cap with a
translation — the two share a `TShape` — so both resolve to the same index.
Measured on OCCT 7.9.3: the located form returns 0, meaning not found, and the
form with its location stripped returns exactly the bottom cap's index. A
reference to the top face would have resolved to the bottom one, which is the
retargeting the whole naming design exists to prevent.

What the bridge does instead is write the wanted sub-shapes down. An archive is
a compound built deliberately — the shape first, then each named sub-shape in
the order asked for — and a *slot* is a position in that list. It is an
internal index of a blob we wrote, not a traversal index of geometry, and it
means nothing outside the blob it came with. After a round trip each slot
returns a face that is `IsSame` a face of the restored solid, with the same
area, and six names come back as six distinct faces.

An archive carries a different magic from a plain blob. Reading one as the
other would hand back the compound instead of the solid, or find no sub-shape
table, and both are wrong quietly.

## Open CASCADE 8.0 changed things the adapter uses

The bridge compiles against 7.9 and 8.0 alike, because a contributor's
installed Open CASCADE is not necessarily the pinned one. Three changes in 8.0
required care, all found by compiling the bridge against the pinned headers:

- `Standard_Failure` was reparented from `Standard_Transient` to
  `std::exception`. `DynamicType()` is gone and `GetMessageString()` is
  deprecated in favour of `what()`. The bridge selects on `OCC_VERSION_HEX`.
  Note the ordering consequence: `Standard_Failure` is now caught by a
  `catch (const std::exception&)`, so the more specific handler must come
  first — it does.
- `Standard_Boolean`, `Standard_True` and `Standard_False` are deprecated in
  favour of `bool`, `true` and `false`, which are the same types on 7.9.
- `TopTools_ListOfShape` moved to a deprecated alias header;
  `NCollection_List<TopoDS_Shape>` is the spelling that works on both.

Checking this does not need a full build. Extracting the pinned source archive
and running `clang++ -fsyntax-only` with every header directory on the include
path answers the question in a minute, where a pin workflow run costs the best
part of an hour. `OCC_VERSION_HEX` has to be defined for that check —
`Standard_Version.hxx` is generated by CMake and absent from the source tree,
so a stub supplying the pinned version numbers is needed, and an empty stub
silently selects the wrong branch.

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
