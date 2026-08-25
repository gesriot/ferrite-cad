# Third-party components

FerriteCAD's own code is MIT. That covers this repository and nothing else:
the components below keep their own licences, and their terms apply to anyone
who redistributes a build containing them.

Two rules follow from those terms and are enforced rather than remembered:

- **Copyleft libraries are linked dynamically, never compiled in.** The user
  must be able to replace them with their own build, which a shared library
  allows and a static one does not without further obligations this project
  does not take on.
- **Nothing under the GPL or AGPL enters the application process at all.**
  `deny.toml` lists the ones that have come up by name, so a refusal is a
  decision on record rather than an omission.

## Open CASCADE Technology

- **Licence:** LGPL-2.1 with the Open CASCADE exception
- **Used for:** the geometry kernel
- **Linking:** dynamic. The adapter is a static shim of FerriteCAD's own MIT
  code (`crates/ferritecad-occt-bridge`) that calls into OCCT's shared
  libraries; no OCCT object code is linked into a FerriteCAD binary.
- **Version:** pinned to V8_0_1, commit
  `b8f597c677811d1f9f4d8a97f5ae2825c0353a42`, source archive SHA-256
  `dba62b81078dd43cec23feba89432be301582341001edad1b93342ad8bda35ea`
- **Replacing it:** build OCCT from the pinned source (`docs/build-occt.md`)
  and put the resulting shared libraries where the application finds them.
- **Notices:** OCCT's own `LICENSE_LGPL_21.txt` and `OCCT_LGPL_EXCEPTION.txt`
  are recorded by the pin workflow and ship beside the libraries.

## Bundled typefaces

The interface embeds four fonts in the binary. A build without them draws no
text at all, so they are not optional the way a library sometimes is.

All four permit being bundled with an application, and all four require their
terms to travel with it. Full texts are committed verbatim in
[`licences/fonts/`](licences/fonts) and are copied from the crate that carries
the font files, `epaint_default_fonts` 0.36.1, so that what ships is what was
read rather than a link that may move.

| Font | Used for | Licence | Text |
| --- | --- | --- | --- |
| Hack Regular | the proportional and monospace faces | MIT (Hack), with DejaVu contributions in the public domain and Bitstream Vera's own permission notice | [`Hack-Regular.txt`](licences/fonts/Hack-Regular.txt) |
| Ubuntu Light | the lighter interface face | Ubuntu Font Licence 1.0 | [`UFL.txt`](licences/fonts/UFL.txt) |
| Noto Emoji Regular | monochrome emoji | SIL Open Font License 1.1 | [`OFL.txt`](licences/fonts/OFL.txt) |
| emoji-icon-font | interface glyphs | MIT | [`emoji-icon-font-mit-license.txt`](licences/fonts/emoji-icon-font-mit-license.txt) |

**What the two font licences require of a distributor.** Both allow the fonts
to be embedded in and shipped with a program, and neither imposes anything on
the program itself: the OFL is explicit that bundling does not make the
software subject to it, and the Ubuntu Font Licence is a font licence in the
same sense. What both require is that the licence and its copyright notice
accompany the font wherever it goes, which is why the texts above are in the
repository rather than referenced.

Neither font is modified here. The OFL's reserved-font-name condition and the
Ubuntu licence's renaming rule both bite on modification, and this project
embeds the files as published.

`deny.toml` grants `OFL-1.1` and `Ubuntu-font-1.0` as an exception only to
`epaint_default_fonts` 0.36.1. They are not in the global allow-list: another
crate under either licence, or even an upstream font update, must be reviewed
and carry its own terms before entering the graph.

## planegcs

- **Licence:** GNU Library General Public License, version 2 or (at your
  option) any later version
- **Used for:** FerriteCAD's sketch constraint solver, behind the product
  contract in `crates/ferritecad-sketch-solver`, and the second candidate in
  the comparison that chose it (`crates/ferritecad-solver-lab`, a client of
  that crate). Not part of any shipped application today.
- **Linking:** dynamic. The shim
  (`crates/ferritecad-sketch-solver/planegcs-bridge`) is FerriteCAD's own MIT
  code and holds no planegcs types; planegcs itself is a shared library built
  beside it and can be replaced. One crate owns that boundary, and
  `tools/check-solver-ownership.sh` keeps a second copy of it from appearing.
- **Source:** FreeCAD 1.0.1, `src/Mod/Sketcher/App/planegcs`, archive SHA-256
  `f62bc07c477544eff62b6ab0fc3bb63fa7f1e6f94763c51b0049507842d444f3`
- **Modifications:** none. The sources are used byte-identical. Four files
  beside them – `SketcherGlobal.h`, `FCConfig.h`, `Base/Console.h` and
  `provenance.cpp`, committed in `tools/planegcs/glue` – are FerriteCAD's own
  MIT build glue, written because FreeCAD's versions reach into Qt and its
  build system, and each says so in its first lines. The Windows export
  problem is solved in FerriteCAD's `SketcherGlobal.h` using the same
  `dllexport`/`dllimport` mechanism FreeCAD's own header uses, so no LGPL
  source is edited to make a DLL.
- **Replacing it:** `tools/build-planegcs.sh` fetches the pinned release and
  the pinned Eigen and Boost, verifies all three checksums before extracting
  anything, and builds the shared library. Beside it the script writes the
  complete corresponding source, FreeCAD's full `LICENSE` text, the Eigen
  source and both of the other licence texts, the provenance with its three
  checked digests, and `REPLACING.md`, which says how to rebuild or substitute
  the library and which files are FerriteCAD's. Point `FCAD_PLANEGCS_DIR` at
  your own build instead. Full statement in
  [`docs/build-planegcs.md`](docs/build-planegcs.md).
- **Off by default:** the `planegcs` cargo feature. Ordinary builds and CI do
  not compile or link it, and say the candidate is unavailable rather than
  fetching anything.
- **Platform coverage:** built and exercised through the real shared library
  on Linux, macOS and Windows by the `planegcs pin` workflow, which requires
  `FERRITECAD_REQUIRE_PLANEGCS=1` so a run cannot pass by skipping, and which
  runs both the product solver's own gates and the bench. Windows produces
  `planegcs.dll` with `planegcs.lib` as linker metadata; a static planegcs is
  refused by the build definition on every platform.
- **Not shipped:** no FerriteCAD release packages planegcs today. With the
  `planegcs` feature enabled, `ferritecad-viewer` does load the shared library
  and reports its own provenance through `--solver-info`; an ordinary build
  still compiles without it and answers a typed "unavailable". The combined
  runtime layout has been measured on all three platforms, but the packager is
  still §21A-2b2b.

### Eigen

- **Licence:** MPL-2.0. A handful of the files planegcs compiles are third
  party and more permissive: `Core/arch/Default/BFloat16.h` is Apache-2.0 and
  `Core/util/MKL_support.h` is BSD-3-Clause. No LGPL Eigen file is in the
  compile closure, which is measured rather than assumed and cross-checked
  against Eigen's own `EIGEN_MPL2_ONLY` guard.
- **Used for:** the dense and sparse linear algebra planegcs solves with.
  Header-only, and therefore compiled into the shared library rather than
  linked beside it.
- **Version:** 3.4.0, pinned with its archive URL and SHA-256 in
  `tools/planegcs/pin.env`.
- **Source:** because Eigen's code is inside the library, the exact source is
  part of the delivery. `tools/build-planegcs.sh` unpacks the checked archive
  into `sources/eigen-3.4.0/`, compiles from that directory, and copies its
  `COPYING.MPL2` out as `LICENSE-Eigen-MPL-2.0.txt`. A URL alone is not the
  reproducibility boundary of this project.
- **Enforced:** `tools/check-planegcs-delivery.sh` requires the delivered
  source to be byte-identical to the checked archive and the MPL text to be
  that source's own.

### Boost

- **Licence:** Boost Software License 1.0. Every one of the 1098 Boost files
  in the compile closure references it, which is measured rather than assumed.
- **Used for:** the graph and math headers planegcs includes. Header-only for
  everything used here, so nothing of Boost is linked.
- **Version:** 1.91.0, pinned with its archive URL and SHA-256 in
  `tools/planegcs/pin.env`, and taken from the official `archives.boost.io`
  release whose digest its publisher records beside it.
- **Source:** the component artifact carries the checked `boost/` header tree
  under `build-inputs/boost/`, because FerriteCAD's MIT shim must compile
  against the same headers as the shared library. The full Boost development
  source is not needed. A future runtime-only product package may omit these
  headers under the licence's object-code exception; the version, digest and
  `LICENSE-Boost-BSL-1.0.txt` remain part of the inventory either way.

Neither is discovered from a machine. The release path fetches both by digest,
consults no system include directory, and has no environment variable that
redirects either one; `tools/check-planegcs-pins.sh` fails ordinary CI if a
workflow installs either from a package manager or holds a second copy of a
pinned value. The accepted policy and the boundary between notices and SBOM
are recorded in
[`docs/decisions/0002-release-compliance-artifacts.md`](docs/decisions/0002-release-compliance-artifacts.md),
which lists the SBOM as the remaining work before a package can carry any of
this. The Rust notices exist and are described below.

## Rust dependencies

Two separate things, and neither substitutes for the other.

`cargo deny check licenses` runs in CI over the whole dependency tree against
the allow-list in `deny.toml`. A crate whose licence is not on that list fails
the build rather than arriving unnoticed. That is an admission check: it writes
no notice, and a package assembled from a green run of it would carry none.

The notices themselves are in [`licences/rust/`](licences/rust), one file per
product target, each the union of the two shipped binaries: `ferritecad-viewer`
with the `planegcs` feature and `ferritecad`. They list every third-party
package linked into either one, with its version, its registry checksum, the
licence FerriteCAD elects under the priority order in
[`tools/notices/about.toml`](tools/notices/about.toml), and that licence's
text. Dev-dependencies are excluded, so the solver bench and the fixtures are
not in them.

They are generated and must not be edited: `tools/check-rust-notices.sh`
regenerates every target twice and refuses any difference, and checks the
result against `Cargo.lock` and an independently resolved dependency graph.
Where a publisher ships no licence text in the crate, the text comes from the
upstream repository at the commit the crate records, committed under
[`tools/notices/texts/`](tools/notices/texts) and bound by SHA-256, so ordinary
builds and gates never contact a git host. Where a publisher has published no
text anywhere, the package is on a closed allowlist that says exactly that and
claims nothing more.

Native components are not in these files. Open CASCADE, planegcs, Eigen, Boost
and the fonts are described above, and the machine-readable inventory that
covers all of them together is the SBOM recorded in
[`docs/decisions/0002-release-compliance-artifacts.md`](docs/decisions/0002-release-compliance-artifacts.md),
which is not built yet.
