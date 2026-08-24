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
- **Replacing it:** `tools/build-planegcs.sh` fetches the pinned release,
  verifies the checksum before extracting anything, and builds the shared
  library. Beside it the script writes the complete corresponding source,
  FreeCAD's full `LICENSE` text, the provenance with its checked digest, and
  `REPLACING.md`, which says how to rebuild or substitute the library and
  which files are FerriteCAD's. Point `FCAD_PLANEGCS_DIR` at your own build
  instead. Full statement in [`docs/build-planegcs.md`](docs/build-planegcs.md).
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

### Eigen and Boost

planegcs needs both at build time. Eigen is MPL-2.0 and Boost is under the
Boost Software License; both are permissive and both are header-only for what
planegcs uses, so neither adds an obligation beyond attribution. They are
found on the system rather than vendored.

## Rust dependencies

`cargo deny check licenses` runs in CI over the whole dependency tree against
the allow-list in `deny.toml`. A crate whose licence is not on that list fails
the build rather than arriving unnoticed.
