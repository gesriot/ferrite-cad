# Third-party licences

FerriteCAD's own code is MIT (see [LICENSE](LICENSE)). Every authored source and
build file says so itself through an `SPDX-License-Identifier: MIT` header; Cargo
manifests use the structured `license` field. `tools/check-licence-headers.sh`
enforces this in CI, because a source file copied out of the repository carries
no licence at all unless it declares one.

This file records every third-party component shipped with, or linked into, a
FerriteCAD binary.

It is maintained by hand and verified in CI by `cargo deny check`. A dependency
is added only with a licence, an owner and a stated reason
(implementation-plan.md, 2).

## Native components

| Component | Version | Licence | Linkage | Notes |
| --- | --- | --- | --- | --- |
| Open CASCADE Technology | pinned in `docs/build-occt.md` | LGPL-2.1 with the Open CASCADE exception | dynamic only | Never statically linked. The shipped notice must tell users how to obtain and replace the library. |

The OCCT notice, the full LGPL-2.1 text and replacement instructions must be
present in every distributed package. This is a release gate, not a
post-release fix (implementation-plan.md, 11).

### OCCT is LGPL-2.1 *only*

Verified against the OCCT README and representative source headers at the
currently pinned V8.0.1 commit, including
`src/FoundationClasses/TKernel/Standard/Standard_DefineException.hxx`. They
state LGPL version 2.1 with the Open CASCADE exception and separately offer
commercial terms.

There is no "or (at your option) any later version". That phrase appears only in
the boilerplate at the end of `LICENSE_LGPL_21.txt`, which is the FSF's template
for authors rather than the grant OCCT actually made.

This costs FerriteCAD nothing today: MIT combines with LGPL-2.1 without
difficulty. It matters for one future decision. Relicensing FerriteCAD under
GPL-3.0 would raise a real compatibility question, because the usual route from
LGPL-2.1 to GPL-3.0 runs through the "or later" clause up to LGPL-3.0, and that
route is closed here. A copyleft move would need legal advice first, not
afterwards — record this before anyone assumes it is a formality.

## Rust dependencies

Generated per release from the locked dependency graph:

```sh
cargo deny check licenses
cargo tree --edges normal --prefix none --format '{p} {l}' | sort -u
```

The generated listing is attached to each release artifact. The allow-list of
acceptable licences lives in [`deny.toml`](deny.toml).

## Deliberately excluded

| Component | Licence | Reason |
| --- | --- | --- |
| SolveSpace `slvs` sketch solver | GPL-3.0 | Incompatible with the MIT distribution policy; denied in `deny.toml`. |

`planegcs` (FreeCAD's sketch solver, LGPL-2.1) is the only mature open
alternative and is a candidate for the stage 0 solver comparison. If it is
adopted it must be linked dynamically and recorded in the native table above,
under the same conditions as OCCT.

## Test corpus

Test models must have a clear authorship status. No third-party commercial
parts enter this repository. The corpus is limited to procedurally generated
geometry, models authored for this project, and openly licensed datasets whose
licence is recorded in `tests/corpus/PROVENANCE.md`.
