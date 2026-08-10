# Choosing a sketch solver

The last stage 0 gate. Nothing here is wired into a document or an interface,
and nothing should be until a candidate is chosen — a solver picked because it
was already integrated is a solver picked for the wrong reason.

## How the comparison is set up

A problem is stated in neutral terms: points, and constraints between them.
Every candidate answers the same questions through one `Solver` interface, so
the comparison is not shaped by what any one of them makes convenient. The
numerics are written out in the crate rather than taken from a library for the
same reason: the damping, the tolerance a rank is judged against and how a step
is accepted are all things the comparison is about.

The corpus is generated, so a problem can be grown to any size and a failure
names the parameters that produced it:

- **rectangle** — fully constrained, the smallest interesting case;
- **chain-n** — a row of rectangles each sharing an edge with the last;
- **polygon-n** — a closed polygon with every side length given;
- **bracket-n** — a staircase of arms, each square to and the same length as
  the last, with only the first dimensioned, so every arm depends on it;
- **underconstrained** — a rectangle with nothing pinned;
- **overconstrained** — a rectangle told twice that one side is horizontal.

## What the first candidate does

Levenberg–Marquardt on the normal equations. Damping is scaled by the diagonal
rather than added as `λI`, because a sketch mixes units — a distance residual
is millimetres and an equal-length residual is millimetres squared — and
uniform damping would quietly favour whichever happens to be larger.

One local release run, with the tests serialized, covered fifteen problems up
to 208 equations. These are example observations, not stable benchmark
numbers; the hardware profile and repeated-sample harness required for that do
not exist yet:

| problem | equations | unknowns | dof | redundant | iterations | worst residual | time |
|---|--:|--:|--:|--:|--:|--:|--:|
| rectangle | 8 | 8 | 0 | 0 | 2 | 2.0e-9 | 4 µs |
| chain-10 | 98 | 80 | 0 | 18 | 2 | 2.5e-7 | 340 µs |
| chain-21 | 208 | 168 | 0 | 40 | 3 | 4.1e-10 | 3.3 ms |
| polygon-32 | 35 | 64 | 29 | 0 | 8 | 2.8e-7 | 950 µs |
| bracket-48 | 98 | 98 | 1 | 1 | 9 | 2.4e-8 | 2.6 ms |
| bracket-100 | 202 | 202 | 1 | 1 | 11 | 1.4e-8 | 17.7 ms |

Everything cleared the common `1e-6` residual limit. It is a numeric comparison
limit, not a physical nanometre: distance residuals are in millimetres while
equal-length and dot/cross-product residuals are in mm². Re-solving from the
previous solution while a corner is pulled is much cheaper than a cold solve,
but the current test-only timing is not yet a UI latency measurement.

Two readings matter more than the totals:

- **Iteration count matters at least as much as size.** `chain-21` and
  `bracket-100` are nearly the same size, but the bracket needs many more
  iterations. Chained perpendicular and equal-length constraints are a harder
  family than a similarly sized rectangle chain.
- **The cost is superlinear.** Dense normal equations are O(unknowns³) per
  iteration. At 200 unknowns that is comfortable; a sketch ten times larger
  would not be, and the answer then is sparsity, not a faster machine.

Diagnosis comes from the Jacobian's rank at the starting state: the unpinned
rectangle reports two degrees of freedom, the repeated constraint reports one
redundant equation. That second one is the case a person most needs told
about, because such a sketch still solves and is still wrong to edit.

## The second candidate

planegcs, FreeCAD's solver: LGPL-2.0-or-later, built as a shared library by
`tools/build-planegcs.sh` from a pinned FreeCAD 1.0.1
whose checksum is verified before anything is extracted, linked dynamically
behind a shim of our own MIT code, and off by default behind a cargo feature.
The sources are used byte-identical; the only files added are three build-glue
headers, because FreeCAD's own versions reach into Qt and its build system.
The build output carries FreeCAD's complete licence text beside the library.
Recorded in THIRD_PARTY_LICENSES.md.

The helper currently builds on macOS and Linux; the linked path has been
exercised locally on macOS. Unlike OCCT, it is not in the three-platform pin
workflow and has no native Windows build path yet.

```
FCAD_PLANEGCS_DIR=<dir> cargo test --release -p ferritecad-solver-lab \
    --features planegcs -- --nocapture --test-threads=1
```

### What the comparison shows

| problem | equations | LM | planegcs |
|---|--:|--:|--:|
| rectangle | 8 | 2.0e-9, 4 µs | 0, 164 µs |
| chain-10 | 98 | 2.5e-7, 340 µs | 0, 3.2 ms |
| chain-21 | 208 | 4.1e-10, 3.3 ms | 0, 17.7 ms |
| polygon-32 | 35 | 2.8e-7, 950 µs | 3.6e-15, 857 µs |
| bracket-48 | 98 | 2.4e-8, 2.6 ms | 3.5e-11, 2.3 ms |
| bracket-100 | 202 | 1.4e-8, 17.7 ms | 3.8e-10, 13.7 ms |

Both clear the same neutral residual limit on the whole corpus, and both
diagnose the under- and over-constrained cases the same way.

- **planegcs often leaves the smaller neutral residual**, reaching exact zero
  on several well-conditioned sketches where the LM stops after clearing the
  common limit. The extra digits do not decide the product choice at this
  scale.
- **The current times do not rank speed fairly.** planegcs necessarily runs
  its rank diagnosis while `System::initSolution()` prepares a solve; the
  local LM path does not. These are useful end-to-end smoke measurements, but
  not execution-equivalent timings. A speed decision needs phase-separated,
  repeated measurements on a recorded hardware profile.
- **The measurement is only as good as the corpus.** An earlier version of the
  bracket started its staircase ninety degrees from where its own constraints
  said the first arm went. The LM recovered from that and planegcs did not,
  which looked like a finding about solvers and was a finding about the
  corpus. It is fixed, and worth remembering: a comparison that flatters one
  candidate is usually measuring its own setup.

### Dragging, measured as a drag

A gesture is one system set up and then nudged, not fifty unrelated solves.
Measuring it the other way charged planegcs for a setup that includes a
diagnosis it always performs, against a solve that had none. Fifty steps, in
release:

| candidate | setup | diagnose | p50 | p95 | max | worst residual |
|---|--:|--:|--:|--:|--:|--:|
| Levenberg–Marquardt | ~0 µs | 2 µs | 3 µs | 4 µs | 8 µs | 3.0e-11 |
| planegcs | 503 µs | 32 µs | 7 µs | 8 µs | 8 µs | 0 |

Both are far inside a frame. The distribution matters more than the mean — a
drag that is usually fast and occasionally not feels broken — and neither has
a tail worth worrying about at this size.

### Sketches with no answer

Three of them: a triangle whose sides are 10, 10 and 40; one edge told it is
both 60 and 70 long; two segments told to be both parallel and perpendicular.
Both candidates refuse all three. That is the property that matters most: a
solver that says yes to a drawing the geometry cannot produce costs somebody a
part, where a refusal costs them a correction.

### Naming what is wrong

Counting is not enough. "This sketch is over-constrained" leaves a person to
find the offending line, and on a real sketch they will not. Both candidates
now name constraints — planegcs natively through its conflicting and redundant
tags, the LM from the rows its elimination could not use — and the bench turns
that into a sentence:

```
this constraint says nothing new: 0 to 1 is horizontal
```

Which is a start and not a finish: it names the constraint, it does not yet
name the *sketch entity* a person drew, because there is no sketch yet to name
it from.

### The decision

planegcs, for the reasons in [decisions/0001-sketch-solver.md](decisions/0001-sketch-solver.md).
Not because it is faster or more accurate — neither is decisive — but because
a sketcher needs arcs, tangency, symmetry and splines, and it already has
them.

### What remains open

Both candidates are dense and cubic per iteration, and sparsity is untouched.
The planegcs build is exercised on macOS only; Linux is supported by the
helper script and Windows is not attempted. And nothing here is integrated
with a document or an interface, which is deliberate: this was a comparison,
and it is over.
