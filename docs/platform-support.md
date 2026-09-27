# Platform support

FerriteCAD has three product targets. A failure on any of them blocks release;
macOS is not an optional build.

| Platform | Architecture | Rust target |
| --- | --- | --- |
| Windows | x86-64, MSVC | `x86_64-pc-windows-msvc` |
| Linux | x86-64, GNU | `x86_64-unknown-linux-gnu` |
| macOS | Apple Silicon, arm64 | `aarch64-apple-darwin` |

## macOS decision

macOS supports **Apple Silicon only**. Intel Macs, an x86-64 build running
under Rosetta, and universal (`arm64;x86_64`) deliveries are outside the
product contract. There is no Intel release artifact or Intel CI lane.
The CLI, viewer, OCCT, planegcs and FerriteCAD's native shims must agree on
arm64. An arm64 executable beside an Intel dylib is not a supported bundle.

Use a native arm64 Rust toolchain (`rustc -vV` reports
`host: aarch64-apple-darwin`). Native macOS CI verifies both `uname -m` and
the Rust host before building. OCCT is configured with
`-DCMAKE_OSX_ARCHITECTURES=arm64`; the product shim and planegcs CMake
projects default to this single architecture and reject other explicit
values. Cargo native adapters reject unsupported macOS targets even when
building without native libraries, rather than silently producing a stub.

Staging and the native checks of staged/extracted release bundles inspect
**every executable and every dylib** with `lipo -archs`, requiring exactly
`arm64`. This is inspection, not a plan to join architectures. The synthetic
`--no-execute` packager fixtures test archive structure; they do not prove
Mach-O architecture, signing or launchability.

The target lists in package manifests, notices, SBOMs and the release set
already name only `aarch64-apple-darwin`. This decision does not remove an
existing Intel artifact or change dependency versions.

## Separate release work

The minimum supported macOS version is **not yet established**. Apple Silicon
support is an architecture policy, not evidence that every macOS release on
that hardware runs the product. Measure the deployment target of the complete
native dependency closure and test the oldest claimed OS before advertising it.

Bundle-relative `@rpath` loading, signing nested code and notarisation remain
required for public distribution. Current workflow archives carry ad-hoc
signatures for local verification, not a Developer ID/notarisation claim.
See [OCCT build](build-occt.md), [planegcs build](build-planegcs.md) and
[runtime layout](runtime-layout.md).
