# 2. Release notices use cargo-about and the SBOM is CycloneDX JSON

**Status:** accepted. This is an engineering policy, not legal advice; the
first public binary release still requires the legal review already called
for by the project RFC.

## What was decided

FerriteCAD's release compliance has two generated artifacts with different
jobs:

- `cargo-about` establishes the licences of the Rust dependency graph. Its
  version and configuration are pinned and committed. The configuration is the
  licence election: its accepted licences are in priority order, with MIT
  before Apache-2.0, and no GPL, AGPL or LGPL alternative is accepted merely
  because it appears in an `OR` expression. A crate that cannot be resolved
  from checked local inputs needs a checksum-bound clarification; it may not be
  silently omitted.
- The machine-readable SBOM is CycloneDX 1.5 JSON. `cargo-cyclonedx` produces
  the Rust graph for each shipped binary and a repository-owned checked input
  supplies the native and asset components that Cargo cannot know about:
  Open CASCADE, planegcs, Eigen, Boost and the embedded fonts. The final BOM is
  deterministic, schema-valid and describes the exact staged package, not the
  whole development workspace and not a different platform's build.

### How the notices are actually produced

Amended after §21A-2b2b0b1 measured it. This decision originally said the
notices were rendered from a committed Handlebars template. They are not, and
the reason is a property of the tool rather than a preference: `cargo-about`
resolves one root crate at a time and renders only its own model of that one
root. FerriteCAD ships two binaries, neither of which has a library target, so
no aggregate crate can name both, and a virtual workspace root makes every
workspace member a root, including the solver bench. So `cargo-about` is run
once per shipped binary in JSON mode and the union is taken by
`tools/generate-rust-notices.sh`, which also owns the document layout. The
policy, the tool pin and the licence payload stay committed, which is what the
original decision was protecting.

The union is taken over the full package identity: source, name and version. A
package reached from both binaries must agree, from both, on its declared
expression and on every licence text; disagreement is a refusal, not a choice
between them. Because the generator could agree with itself about the wrong
graph, the same graph is resolved independently by `cargo tree` and the two
must match exactly before anything is written.

### Packages that publish no licence text

Also amended after measurement. `cargo-about` answers a licence it recognised
no file for by substituting a canonical SPDX template whose copyright line is
still `Copyright (c) <year> <copyright holders>`, and it reports this only at
debug level. Such a template states the terms but carries no notice, and MIT
requires the notice. FerriteCAD refuses it, wherever it comes from: a publisher
that vendors an unfilled REUSE template beside its real licence produces the
same defect.

Where the publisher ships no file but the upstream repository has one, the text
is taken from that repository at the exact commit the crate records in its own
`.cargo_vcs_info.json`, and committed under `tools/notices/texts/` bound by
SHA-256. Fetching happens in a separate, explicitly networked command; ordinary
generation and the ordinary gate read the committed payload and never contact a
git host.

Where the publisher has published no licence text anywhere, the package goes on
a closed, exhaustive allowlist that records only what can be checked, keeps its
two evidence classes apart, and never claims a text was recovered. The list may
not grow without a decision, a networked gate refuses a row whose upstream has
since published a text, and the removal conditions are recorded with the slice
in the implementation plan.

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
requires the text. The rebuildable planegcs component artifact additionally
carries the checked Boost header tree because FerriteCAD's MIT shim compiles
against it. A later runtime-only product package may omit that build input;
the two artifacts make different promises and are checked separately.

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
