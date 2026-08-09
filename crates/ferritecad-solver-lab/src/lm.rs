// SPDX-License-Identifier: MIT
//! A Levenberg–Marquardt solver, small enough to read.
//!
//! The first candidate, and the one that answers whether a sketch solver is
//! something this project could reasonably own. Damped Gauss–Newton on the
//! normal equations: at each step it solves `(JᵀJ + λ diag(JᵀJ)) s = -Jᵀr`,
//! takes the step if the residual fell and raises the damping if it did not.
//!
//! The damping is scaled by the diagonal rather than added as `λI` because a
//! sketch mixes units — a length residual is millimetres and an equal-length
//! residual is millimetres squared — and uniform damping would quietly favour
//! whichever happens to be larger.

use std::time::Instant;

use crate::linalg::{norm, solve_spd};
use crate::{COMPARISON_RESIDUAL_LIMIT, Outcome, Problem, Solver};

#[derive(Debug, Clone, Copy)]
pub struct LevenbergMarquardt {
    pub max_iterations: usize,
    /// A step is good enough when no single residual exceeds this.
    pub tolerance: f64,
}

impl Default for LevenbergMarquardt {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            // The comparison's common numerical boundary. Residuals have the
            // units of their constraints, so this must not be described as
            // one physical distance.
            tolerance: COMPARISON_RESIDUAL_LIMIT,
        }
    }
}

fn worst(residuals: &[f64]) -> f64 {
    residuals
        .iter()
        .fold(0.0f64, |worst, value| worst.max(value.abs()))
}

impl Solver for LevenbergMarquardt {
    fn name(&self) -> &'static str {
        "levenberg-marquardt"
    }

    fn solve(&self, problem: &Problem, start: &[f64]) -> Outcome {
        let began = Instant::now();
        let mut state = start.to_vec();
        let (mut residuals, mut jacobian) = problem.evaluate(&state);
        let mut damping = 1e-6;
        let mut iterations = 0;

        for iteration in 0..self.max_iterations {
            if worst(&residuals) <= self.tolerance {
                return Outcome {
                    converged: true,
                    iterations: Some(iteration),
                    worst_residual: worst(&residuals),
                    elapsed: began.elapsed(),
                    solution: state,
                };
            }
            iterations = iteration + 1;

            let normal = jacobian.transpose_times_self();
            let gradient = jacobian.transpose_times(&residuals);
            let before = norm(&residuals);

            // Raise the damping until the system is solvable and the step
            // actually helps. A sketch's Jacobian is rank-deficient whenever
            // the sketch is under-constrained, which is the normal case while
            // somebody is still drawing, so this is the path most steps take.
            let mut accepted = false;
            for _ in 0..24 {
                let mut damped = normal.clone();
                for i in 0..damped.rows {
                    let diagonal = damped.at(i, i);
                    // A free unknown has a zero diagonal; give it something to
                    // hold on to so the step is merely small, not infinite.
                    damped.set(i, i, diagonal + damping * diagonal.max(1.0));
                }

                let Some(step) =
                    solve_spd(&damped, &gradient.iter().map(|g| -g).collect::<Vec<_>>())
                else {
                    damping *= 10.0;
                    continue;
                };

                let candidate: Vec<f64> = state
                    .iter()
                    .zip(&step)
                    .map(|(value, delta)| value + delta)
                    .collect();
                let (trial_residuals, trial_jacobian) = problem.evaluate(&candidate);

                if norm(&trial_residuals) < before {
                    state = candidate;
                    residuals = trial_residuals;
                    jacobian = trial_jacobian;
                    damping = (damping / 3.0).max(1e-12);
                    accepted = true;
                    break;
                }
                damping *= 10.0;
            }

            if !accepted {
                // No amount of damping improved on where we are. That is an
                // answer: this is as close as this solver gets from here.
                break;
            }
        }

        Outcome {
            converged: worst(&residuals) <= self.tolerance,
            iterations: Some(iterations),
            worst_residual: worst(&residuals),
            elapsed: began.elapsed(),
            solution: state,
        }
    }
}

/// A no-op candidate, so the bench proves it can tell a solver from a failure.
///
/// Every measurement below is comparative, and a bench that reported success
/// for something that does nothing would not be measuring anything.
#[derive(Debug, Clone, Copy, Default)]
pub struct DoesNothing;

impl Solver for DoesNothing {
    fn name(&self) -> &'static str {
        "does-nothing"
    }

    fn solve(&self, problem: &Problem, start: &[f64]) -> Outcome {
        let began = Instant::now();
        let (residuals, _) = problem.evaluate(start);
        Outcome {
            converged: false,
            iterations: Some(0),
            worst_residual: worst(&residuals),
            elapsed: began.elapsed(),
            solution: start.to_vec(),
        }
    }
}
