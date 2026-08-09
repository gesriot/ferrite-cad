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

## planegcs

- **Licence:** GNU Library General Public License, version 2 or (at your
  option) any later version
- **Used for:** a second candidate in the sketch solver comparison
  (`crates/ferritecad-solver-lab`). Not part of any shipped application today.
- **Linking:** dynamic. The shim
  (`crates/ferritecad-solver-lab/planegcs-bridge`) is FerriteCAD's own MIT code
  and holds no planegcs types; planegcs itself is a shared library built beside
  it and can be replaced.
- **Source:** FreeCAD 1.0.1, `src/Mod/Sketcher/App/planegcs`, archive SHA-256
  `f62bc07c477544eff62b6ab0fc3bb63fa7f1e6f94763c51b0049507842d444f3`
- **Modifications:** none. The sources are used byte-identical. Three headers
  beside them — `SketcherGlobal.h`, `FCConfig.h` and `Base/Console.h` — are
  FerriteCAD's own MIT build glue, written because FreeCAD's versions reach
  into Qt and its build system, and marked as such.
- **Replacing it:** `tools/build-planegcs.sh` fetches the pinned release,
  verifies the checksum before extracting anything, and builds the shared
  library. The build directory carries FreeCAD's complete `LICENSE` text
  beside the library. Point `FCAD_PLANEGCS_DIR` at your own build instead.
- **Off by default:** the `planegcs` cargo feature. Ordinary builds and CI do
  not compile or link it.
- **Platform coverage:** the linked lab path is currently implemented and
  locally exercised on macOS; the helper also supports Linux. It is not part
  of the three-platform pin workflow and has no native Windows build path yet.

### Eigen and Boost

planegcs needs both at build time. Eigen is MPL-2.0 and Boost is under the
Boost Software License; both are permissive and both are header-only for what
planegcs uses, so neither adds an obligation beyond attribution. They are
found on the system rather than vendored.

## Rust dependencies

`cargo deny check licenses` runs in CI over the whole dependency tree against
the allow-list in `deny.toml`. A crate whose licence is not on that list fails
the build rather than arriving unnoticed.
