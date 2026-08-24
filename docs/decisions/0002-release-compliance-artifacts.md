# 2. Release notices use cargo-about and the SBOM is CycloneDX JSON

**Status:** accepted. This is an engineering policy, not legal advice; the
first public binary release still requires the legal review already called
for by the project RFC.

## What was decided

FerriteCAD's release compliance has two generated artifacts with different
jobs:

- `cargo-about` generates the human-readable notices for the Rust dependency
  graph. Its version, configuration and Handlebars template are committed or
  pinned. The configuration is the licence election: its accepted licences
  are in priority order, with MIT before Apache-2.0, and no GPL, AGPL or LGPL
  alternative is accepted merely because it appears in an `OR` expression.
  A crate that cannot be resolved from checked local inputs needs a
  checksum-bound clarification; it may not be silently omitted.
- The machine-readable SBOM is CycloneDX 1.5 JSON. `cargo-cyclonedx` produces
  the Rust graph for each shipped binary and a repository-owned checked input
  supplies the native and asset components that Cargo cannot know about:
  Open CASCADE, planegcs, Eigen, Boost and the embedded fonts. The final BOM is
  deterministic, schema-valid and describes the exact staged package, not the
  whole development workspace and not a different platform's build.

`cargo-deny` remains the admission gate. It answers whether a dependency's
licence expression is allowed; it does not generate notices and is not an
SBOM. The notice generator must agree with its policy, but neither tool is a
substitute for the other.

## Native build inputs

The release path may not discover Eigen or Boost from a developer machine.
Their versions, source URLs and digests have one committed owner. The same
checked inputs are used on Linux, macOS and Windows and are recorded in the
planegcs provenance and the SBOM.

Eigen is MPL-2.0 code compiled into planegcs. A package carrying that library
also carries the MPL text, a notice telling the recipient how to obtain the
Source Code Form, and the exact digest-checked Eigen source used for the
build. A URL alone is not the reproducibility boundary of this project.

Boost's object-code exception means that a package containing only the
generated shared library does not acquire a source or notice obligation from
Boost. FerriteCAD still records the checked Boost version in provenance and
the SBOM, and includes the short Boost Software License text. This is a
deliberate inventory and review choice, not a claim that the exception
requires the text.

## Scope of the next slices

The compliance-input slice comes before the packager. It centralizes and
checks the Eigen and Boost inputs, makes the planegcs component artifact
self-describing, commits the `cargo-about` election and template, and proves a
CycloneDX generator can account for both Rust binaries plus every native and
asset component. It does not create a FerriteCAD release archive.

Only after that slice is green on all three product platforms may the
packager consume the measured runtime layout. A package is refused if notices
or the SBOM are missing, stale, non-deterministic, or disagree with the files
actually staged.
