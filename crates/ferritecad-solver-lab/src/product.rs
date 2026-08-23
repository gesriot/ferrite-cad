// SPDX-License-Identifier: MIT
//! The bench, asking the product solver the questions it asks every candidate.
//!
//! This is the whole of the lab's relationship with planegcs. There is no
//! shim here, no build script, no C ABI and no constraint mapping: all of
//! those live in `ferritecad-sketch-solver`, which owns them, and the lab
//! reaches planegcs only by being one of its callers. That is the point of the
//! arrangement — a bench that held its own copy of the boundary would be
//! measuring a second implementation and reporting it as the product's.
//!
//! What stays here is the neutral corpus and the reference Levenberg–Marquardt
//! implementation. Both are deliberately independent of the product path, so a
//! replacement for planegcs can be measured against the same gate.

use std::time::Instant;

use ferritecad_sketch_solver as product;

use crate::{Blame, Constraint, Diagnosis, Drag, DragTimings, Outcome, Point, Problem, Solver};

/// The lab's neutral problem, in the product solver's own terms.
///
/// Point `i` of the bench becomes `PointId(i)` and constraint `i` becomes
/// `ConstraintId(i)`. That is a translation for the bench's convenience and
/// nothing more: the product contract never assumes an identifier equals a
/// position, and `caller_ids_survive_a_permuted_store` in its own gates is
/// what holds it to that.
fn as_sketch(problem: &Problem, start: &[f64]) -> product::Sketch {
    let mut sketch = product::Sketch::new();
    for index in 0..start.len() / 2 {
        sketch.add_point(
            product::PointId(index as u64),
            start[index * 2],
            start[index * 2 + 1],
        );
    }
    for (index, constraint) in problem.constraints.iter().enumerate() {
        sketch.add_constraint(product::ConstraintId(index as u64), translate(*constraint));
    }
    sketch
}

fn point(p: Point) -> product::PointId {
    product::PointId(p.0 as u64)
}

fn translate(constraint: Constraint) -> product::Constraint {
    match constraint {
        Constraint::Coincident { a, b } => product::Constraint::Coincident {
            a: point(a),
            b: point(b),
        },
        Constraint::Fixed { point: p, x, y } => product::Constraint::Fixed {
            point: point(p),
            x,
            y,
        },
        Constraint::Distance { a, b, distance } => product::Constraint::Distance {
            a: point(a),
            b: point(b),
            distance,
        },
        Constraint::Horizontal { a, b } => product::Constraint::Horizontal {
            a: point(a),
            b: point(b),
        },
        Constraint::Vertical { a, b } => product::Constraint::Vertical {
            a: point(a),
            b: point(b),
        },
        Constraint::EqualLength { a, b } => product::Constraint::EqualLength {
            a: (point(a.0), point(a.1)),
            b: (point(b.0), point(b.1)),
        },
        Constraint::Perpendicular { a, b } => product::Constraint::Perpendicular {
            a: (point(a.0), point(a.1)),
            b: (point(b.0), point(b.1)),
        },
        Constraint::Parallel { a, b } => product::Constraint::Parallel {
            a: (point(a.0), point(a.1)),
            b: (point(b.0), point(b.1)),
        },
    }
}

/// Positions back in the bench's flat vector, by identifier rather than by
/// arrival order.
fn as_state(positions: &[product::Position], unknowns: usize) -> Vec<f64> {
    let mut state = vec![0.0; unknowns];
    for position in positions {
        let index = position.point.0 as usize;
        state[index * 2] = position.x;
        state[index * 2 + 1] = position.y;
    }
    state
}

fn as_indices(ids: &[product::ConstraintId]) -> Vec<usize> {
    ids.iter().map(|id| id.0 as usize).collect()
}

/// planegcs, reached the way the application will reach it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Planegcs;

impl Planegcs {
    /// What the product solver makes of the system, without solving it.
    pub fn diagnose(&self, problem: &Problem) -> Option<(Diagnosis, bool, bool)> {
        let sketch = as_sketch(problem, &problem.start);
        let found = product::diagnose(&sketch).ok()?;
        let diagnosis = Diagnosis {
            unknowns: problem.unknowns(),
            equations: problem.equations(),
            rank: problem
                .unknowns()
                .saturating_sub(found.degrees_of_freedom()),
            degrees_of_freedom: found.degrees_of_freedom(),
            redundant: usize::from(!found.redundant().is_empty()),
        };
        Some((
            diagnosis,
            !found.conflicting().is_empty(),
            !found.redundant().is_empty(),
        ))
    }
}

impl Solver for Planegcs {
    fn name(&self) -> &'static str {
        "planegcs-dogleg"
    }

    fn solve(&self, problem: &Problem, start: &[f64]) -> Outcome {
        let began = Instant::now();
        let sketch = as_sketch(problem, start);
        let answer = product::solve(&sketch);
        let elapsed = began.elapsed();

        // A refusal publishes no positions, so there is no solved state to
        // report and the starting one is what the sketch still is. The bench
        // measures the residual itself either way, exactly as it does for the
        // reference implementation.
        let state = match &answer {
            Ok(product::Outcome::Solved(solution)) => {
                as_state(solution.positions(), problem.unknowns())
            }
            _ => start.to_vec(),
        };
        let (residuals, _) = problem.evaluate(&state);
        let worst = residuals
            .iter()
            .fold(0.0f64, |worst, value| worst.max(value.abs()));

        Outcome {
            converged: matches!(answer, Ok(product::Outcome::Solved(_)))
                && worst <= crate::COMPARISON_RESIDUAL_LIMIT,
            // The product contract reports no iteration count. planegcs does
            // not expose one through this path, and inventing one would be the
            // bench describing the solver rather than measuring it.
            iterations: None,
            worst_residual: worst,
            elapsed,
            solution: state,
        }
    }
}

/// Which constraints the product solver blames, in the bench's numbering.
pub fn blame_with_planegcs(problem: &Problem) -> Option<Blame> {
    let sketch = as_sketch(problem, &problem.start);
    let found = product::diagnose(&sketch).ok()?;
    let mut constraints = as_indices(found.conflicting());
    constraints.extend(as_indices(found.redundant()));
    constraints.sort_unstable();
    constraints.dedup();
    Some(Blame { constraints })
}

/// Drags with the product solver, which holds one native system throughout.
pub fn drag_with_planegcs(problem: &Problem, drag: &Drag) -> Option<DragTimings> {
    let began = Instant::now();
    let sketch = as_sketch(problem, &problem.start);
    let mut gesture = product::Drag::begin(&sketch, point(drag.point)).ok()?;
    let setup = began.elapsed();

    // The product contract diagnoses while the gesture is being set up, which
    // is what planegcs does too: there is no separate diagnosis to time.
    let diagnose = std::time::Duration::ZERO;

    // The pin the gesture holds is the product crate's, not the bench's, so
    // the bench keeps its own copy to judge the result against.
    let mut dragged = problem.clone();
    dragged.constraints.push(Constraint::Fixed {
        point: drag.point,
        x: problem.start[drag.point.x()],
        y: problem.start[drag.point.y()],
    });
    let pin = dragged.constraints.len() - 1;

    let mut steps = Vec::with_capacity(drag.targets.len());
    let mut worst_residual: f64 = 0.0;
    let mut worst_follow: f64 = 0.0;
    let mut all_steps_converged = true;

    for (x, y) in &drag.targets {
        let began = Instant::now();
        let answer = gesture.move_to(*x, *y).ok()?;
        steps.push(began.elapsed());

        let product::Outcome::Solved(solution) = &answer else {
            all_steps_converged = false;
            continue;
        };
        let state = as_state(solution.positions(), problem.unknowns());

        dragged.constraints[pin] = Constraint::Fixed {
            point: drag.point,
            x: *x,
            y: *y,
        };
        let (residuals, _) = dragged.evaluate(&state);
        worst_residual = worst_residual.max(
            residuals
                .iter()
                .fold(0.0f64, |worst, value| worst.max(value.abs())),
        );
        worst_follow = worst_follow
            .max((state[drag.point.x()] - x).abs())
            .max((state[drag.point.y()] - y).abs());
    }

    Some(DragTimings {
        candidate: "planegcs-dogleg",
        setup,
        diagnose,
        steps,
        all_steps_converged,
        worst_residual,
        worst_follow_error: worst_follow,
    })
}
