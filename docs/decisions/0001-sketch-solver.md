# 1. planegcs is the sketch solver

**Status:** accepted, and reversible – see the last section.

## What was decided

FerriteCAD will use planegcs as its sketch constraint solver. The
Levenberg–Marquardt implementation in `crates/ferritecad-solver-lab` stays as
a reference: it is what the bench compares against, and what would be built on
if the decision were ever revisited.

## What the comparison found

Both candidates were asked the same questions through one interface, over a
generated corpus of up to 208 equations. Dragging updates one target fifty
times without rebuilding planegcs's constraint graph; one-time setup,
diagnosis and per-step work are reported apart. The two implementations do not
have identical starting-state semantics: the LM starts from the previous
solution, while planegcs reuses the gesture-start reference captured by
`initSolution()`.

| | Levenberg–Marquardt | planegcs |
|---|---|---|
| solves the corpus | yes | yes |
| clears the neutral `1e-6` residual gate | yes | yes |
| local release drag samples | low single-digit µs | single-digit to low tens of µs |
| setup and diagnosis | reported separately | reported separately |
| refuses unsatisfiable sketches | yes | yes |
| names conflicting constraints | yes, from the Jacobian's rank | yes, natively |

Neither approached a frame budget in the checked local runs. Exact numbers are
not treated as stable benchmarks: there is no recorded hardware profile,
repeated-sample harness or cold/warm protocol yet, and consecutive processes
show measurable first-run variation.

## Why planegcs, given that

**Not speed, and not accuracy.** Both clear the same neutral residual gate and
both are comfortably fast on this corpus. Choosing between their local timing
samples or extra unused digits would be choosing on noise.

**What decides it is how much is already there.** The bench covers eight
constraint types on points. A sketcher needs arcs, circles, ellipses,
tangency, symmetry, equal radius, construction geometry, splines – planegcs
has them, tested against fifteen years of drawings that people actually made.
Writing that is not a slice, it is a project, and it is not the project this
one is.

**The licensing machinery is familiar, but the obligation is separate.** The
project already has infrastructure for a dynamically linked, replaceable
LGPL component because of Open CASCADE. planegcs is LGPL-2.0-or-later without
the OCCT exception, so its notices, licence text, relinking path and source
offer still have to be reviewed and packaged on their own; this is not erased
by having solved a similar problem once.

**What it costs.** A C++ build dependency (Eigen and Boost headers), a second
shared library to ship and to package on three platforms, and the same
obligation to let users replace it.

That cost is now paid rather than estimated, in
[run 32643458969](https://github.com/gesriot/ferrite-cad/actions/runs/32643458969):
all three platforms build the pinned planegcs from a digest-checked archive
and run the lab through the real shared library, Windows included, producing a
DLL and an import library and never a static copy. The recipe, the
file-by-file ownership and the replacement path are in
[build-planegcs.md](../build-planegcs.md). What
remains open is packaging it into a FerriteCAD release, which is a different
question from being able to build and replace it.

## What this does not decide

- **Nothing is integrated.** No document type, no interface, no feature reads
  a constraint yet. This says which solver the sketcher will be built on, not
  that it has been. Being buildable on three platforms does not change that:
  the application does not load planegcs and does not ship it.
- **Sparsity is unresolved.** The local LM is dense and cubic per iteration.
  planegcs's diagnosis and solver paths have not been characterized at larger
  scale, and its sparse options have not been exercised here.
- **Conflict messages are a sketch of one.** Naming the constraints is done;
  saying it in a way a person acts on, in an interface, is not.

## Reversing it

The `Solver` interface is the architectural seam, not the whole cost: the
chosen path also has a C ABI, feature mapping, a build and a package. The
neutral corpus and reference LM remain independent of those pieces, so a
replacement can be measured against the same gate. If the LGPL obligation
becomes unacceptable, or planegcs proves wrong in a way this corpus could not
show, the reference implementation is still there and still passing.
