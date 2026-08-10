# 1. planegcs is the sketch solver

**Status:** accepted, and reversible — see the last section.

## What was decided

FerriteCAD will use planegcs as its sketch constraint solver. The
Levenberg–Marquardt implementation in `crates/ferritecad-solver-lab` stays as
a reference: it is what the bench compares against, and what would be built on
if the decision were ever revisited.

## What the comparison found

Both candidates were asked the same questions through one interface, over a
generated corpus of up to 208 equations, with the drag measured as a drag —
one system set up and then nudged fifty times, phases reported apart.

| | Levenberg–Marquardt | planegcs |
|---|---|---|
| solves the corpus | yes | yes |
| residual on well-conditioned sketches | 1e-10 … 1e-15 | exact zero |
| drag p50 / p95 / max | 3 / 4 / 8 µs | 7 / 8 / 8 µs |
| drag setup, paid once | ~0 µs | 503 µs |
| refuses unsatisfiable sketches | yes | yes |
| names conflicting constraints | yes, from the Jacobian's rank | yes, natively |

Neither is close to a limit that matters. A drag step of 8 µs against a 16 ms
frame is not a difference a person can feel, and 503 µs of setup is paid once
per gesture.

## Why planegcs, given that

**Not speed, and not accuracy.** Both are adequate and neither wins outright:
the LM is twice as quick per drag step, planegcs is exact where the LM leaves
a picometre. Choosing on those numbers would be choosing on noise.

**What decides it is how much is already there.** The bench covers eight
constraint types on points. A sketcher needs arcs, circles, ellipses,
tangency, symmetry, equal radius, construction geometry, splines — planegcs
has them, tested against fifteen years of drawings that people actually made.
Writing that is not a slice, it is a project, and it is not the project this
one is.

**The licensing cost is already paid.** planegcs is LGPL, which this project
already handles for Open CASCADE: dynamic linking, notices, a replaceable
shared library, recorded in `THIRD_PARTY_LICENSES.md`. Adding a second
component on terms already met costs a build step, not a policy.

**What it costs.** A C++ build dependency (Eigen and Boost headers), a second
shared library to ship and to package on three platforms, and the same
obligation to let users replace it. The Windows and Linux paths for building
it are not yet exercised in CI — that is real, open work, not a reason against.

## What this does not decide

- **Nothing is integrated.** No document type, no interface, no feature reads
  a constraint yet. This says which solver the sketcher will be built on, not
  that it has been.
- **Sparsity is unresolved.** Both candidates are dense and cubic per
  iteration. At 200 unknowns that is comfortable; a sketch ten times larger
  would not be, and planegcs's own sparse paths have not been exercised here.
- **Conflict messages are a sketch of one.** Naming the constraints is done;
  saying it in a way a person acts on, in an interface, is not.

## Reversing it

The `Solver` interface is the whole commitment. A candidate is roughly two
hundred lines against it, and the corpus and measurements do not change when
one is added or removed — which is how planegcs was added in the first place.
If the LGPL obligation ever becomes unacceptable, or planegcs proves wrong in
a way this corpus could not show, the reference implementation is still there
and still passing.
