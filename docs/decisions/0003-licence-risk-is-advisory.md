# 3. Unresolved licence questions are recorded but do not block delivery

**Status:** accepted by the project owner. This is a product-development
decision and an explicit risk acceptance, not legal advice or a conclusion
that the unresolved questions are harmless.

## Decision

FerriteCAD will continue development, SBOM generation, packaging and release
work when third-party licence evidence is incomplete or legally ambiguous.
Those conditions are recorded as known licence risks, but they are not merge,
packaging or release gates.

In particular, the eleven macOS Rust packages for which the published crate
does not carry a publisher-supplied licence text remain in the generated
notice inventory. The uncertainty described by the `objc2` project about
bindings derived from Apple SDKs is also acknowledged and deliberately not
resolved by FerriteCAD. Neither condition blocks the macOS viewer.

The project will not contact upstream maintainers about these questions and
will not proactively investigate them further. They may be reconsidered only
by a later explicit decision of the project owner or in response to a concrete
distribution requirement or claim.

## What remains checked

This decision does not authorize false or invented licence data. FerriteCAD
still:

- inventories the exact dependency graph and records unresolved evidence as
  unresolved;
- carries publisher-supplied texts and the native-component material already
  assembled by the project;
- refuses stale, internally inconsistent or fabricated notice and SBOM data;
- keeps security advisories, dependency bans and source policy as ordinary CI
  gates.

`cargo-deny` licence findings are advisory. A generated notice may say
`KNOWN LICENCE RISK`, but that wording is informational and does not stop a
package. The compatibility option
`tools/check-rust-notices.sh --release-ready` now checks that the risk is
represented accurately; it is not legal clearance.

## Consequences

ADR 0002 still owns the notice and CycloneDX formats, reproducibility and
inventory rules. This decision supersedes only its statements that unresolved
licence evidence blocks CycloneDX work, packaging or release, and supersedes
the earlier requirement for a legal review before the first public binary
release.

The next release-engineering slice may therefore proceed to the CycloneDX
SBOM for all three targets, including macOS. Notices and the SBOM must describe
the staged product honestly, but they do not promise that every licensing
question has been resolved.
