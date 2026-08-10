// SPDX-License-Identifier: MIT
//! Dragging, measured the same way for every candidate.
//!
//! A drag is not a sequence of unrelated solves. The sketch is set up once and
//! then nudged, which is both how a person uses it and the only fair way to
//! compare: building the system inside every timed solve measured planegcs's
//! setup — which includes a diagnosis it always performs — against a solve
//! that had none.
//!
//! So the phases are separated. Setup and diagnosis are paid once and reported
//! once; what a person feels is the per-step solve, reported as a distribution
//! rather than a mean, because a drag that is usually fast and occasionally
//! not is a drag that feels broken.

use std::time::{Duration, Instant};

use crate::{Constraint, LevenbergMarquardt, Point, Problem, Solver};

/// What a drag cost, split into what is paid once and what is paid per step.
#[derive(Debug, Clone, PartialEq)]
pub struct DragTimings {
    pub candidate: &'static str,
    pub setup: Duration,
    pub diagnose: Duration,
    /// One entry per step, in order.
    pub steps: Vec<Duration>,
    /// Every candidate call reported a completed solve.
    pub all_steps_converged: bool,
    pub worst_residual: f64,
    /// Where the dragged point actually ended up, against where it was put.
    pub worst_follow_error: f64,
}

impl DragTimings {
    fn percentile(&self, fraction: f64) -> Duration {
        if self.steps.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = self.steps.clone();
        sorted.sort_unstable();
        let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
        sorted[index]
    }

    pub fn p50(&self) -> Duration {
        self.percentile(0.50)
    }

    pub fn p95(&self) -> Duration {
        self.percentile(0.95)
    }

    pub fn max(&self) -> Duration {
        self.steps.iter().copied().max().unwrap_or(Duration::ZERO)
    }

    /// One line, in microseconds, for the comparison table.
    pub fn line(&self) -> String {
        format!(
            "  {:<20} setup {:>7} us  diagnose {:>7} us  \
             p50 {:>6} us  p95 {:>6} us  max {:>6} us  worst {:>10.3e}",
            self.candidate,
            self.setup.as_micros(),
            self.diagnose.as_micros(),
            self.p50().as_micros(),
            self.p95().as_micros(),
            self.max().as_micros(),
            self.worst_residual
        )
    }
}

/// Where a dragged point is put at each step.
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    /// The point being pulled.
    pub point: Point,
    pub targets: Vec<(f64, f64)>,
}

impl Drag {
    /// A pull along a diagonal, in `steps` even nudges.
    pub fn diagonal(point: Point, steps: usize) -> Self {
        Self {
            point,
            targets: (1..=steps)
                .map(|step| (step as f64 * 1.5, step as f64 * 0.75))
                .collect(),
        }
    }
}

/// Drags with the Levenberg–Marquardt candidate.
///
/// Its equivalent of a persistent system is re-solving from the previous
/// solution: there is nothing to keep between steps but the state, which is
/// itself a finding about how much simpler it is.
pub fn drag_with_lm(problem: &Problem, drag: &Drag) -> DragTimings {
    let solver = LevenbergMarquardt::default();

    let began = Instant::now();
    let mut dragged = problem.clone();
    dragged.constraints.push(Constraint::Fixed {
        point: drag.point,
        x: problem.start[drag.point.x()],
        y: problem.start[drag.point.y()],
    });
    let pin = dragged.constraints.len() - 1;
    let mut setup = began.elapsed();

    let began = Instant::now();
    let _ = dragged.diagnose(1e-9);
    let diagnose = began.elapsed();

    let began = Instant::now();
    let initial = solver.solve(&dragged, &dragged.start);
    setup += began.elapsed();
    let mut all_steps_converged = initial.converged;
    let mut state = initial.solution;
    let mut steps = Vec::with_capacity(drag.targets.len());
    let mut worst_residual: f64 = 0.0;
    let mut worst_follow: f64 = 0.0;

    for (x, y) in &drag.targets {
        dragged.constraints[pin] = Constraint::Fixed {
            point: drag.point,
            x: *x,
            y: *y,
        };

        let began = Instant::now();
        let outcome = solver.solve(&dragged, &state);
        steps.push(began.elapsed());

        all_steps_converged &= outcome.converged;
        state = outcome.solution;
        worst_residual = worst_residual.max(outcome.worst_residual);
        worst_follow = worst_follow
            .max((state[drag.point.x()] - x).abs())
            .max((state[drag.point.y()] - y).abs());
    }

    DragTimings {
        candidate: solver.name(),
        setup,
        diagnose,
        steps,
        all_steps_converged,
        worst_residual,
        worst_follow_error: worst_follow,
    }
}
