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

Measured in release, fifteen problems up to 208 equations:

| problem | equations | unknowns | dof | redundant | iterations | worst residual | time |
|---|--:|--:|--:|--:|--:|--:|--:|
| rectangle | 8 | 8 | 0 | 0 | 3 | 7.1e-15 | 7 µs |
| chain-10 | 98 | 80 | 0 | 18 | 3 | 2.1e-11 | 523 µs |
| chain-21 | 208 | 168 | 0 | 40 | 3 | 4.1e-10 | 3.7 ms |
| polygon-32 | 35 | 64 | 29 | 0 | 9 | 2.0e-10 | 935 µs |
| bracket-48 | 98 | 98 | 1 | 1 | 14 | 2.3e-12 | 3.7 ms |
| bracket-100 | 202 | 202 | 1 | 1 | 14 | 9.9e-11 | 22 ms |

Everything converged. **A drag step costs about a microsecond**: re-solving
from the previous solution while a corner is pulled is nothing like solving
from scratch, and that is the number interactivity depends on.

Two readings matter more than the totals:

- **Iteration count, not size, dominates.** `chain-21` (208 equations) solves
  in 3.7 ms and `bracket-100` (202 equations) takes 22 ms, because the bracket
  needs fourteen iterations to the chain's three. Chained perpendicular and
  equal-length constraints are what make a sketch expensive, not how many
  constraints there are.
- **The cost is superlinear.** Dense normal equations are O(unknowns³) per
  iteration. At 200 unknowns that is comfortable; a sketch ten times larger
  would not be, and the answer then is sparsity, not a faster machine.

Diagnosis comes from the Jacobian's rank at the starting state: the unpinned
rectangle reports two degrees of freedom, the repeated constraint reports one
redundant equation. That second one is the case a person most needs told
about, because such a sketch still solves and is still wrong to edit.

## What is not done

**planegcs is not wired in.** FreeCAD's solver is LGPL-2.1 and this project is
MIT with a statically linked shim, so vendoring it is a licensing decision
before it is an engineering one, and not one to make silently in a prototype.
The `Solver` interface exists so it can be added as a second candidate without
the corpus or the measurements changing.

Until then the comparison has one candidate, which means these numbers say
"a solver this project could own performs like this" and not yet "this is the
better of two".
