// SPDX-License-Identifier: MIT
//! A bench for comparing sketch constraint solvers, before one is chosen.
//!
//! The last question stage 0 asks: can a sketch solver be relied on, and which
//! one. Nothing here is wired into a document or an interface, and nothing
//! should be until the answer is in — a solver chosen because it was already
//! integrated is a solver chosen for the wrong reason.
//!
//! # What is being compared
//!
//! A problem is stated in neutral terms — points, and constraints between them
//! — and every candidate is asked the same questions through one interface:
//! does it converge, how closely, how quickly, what does it say about a sketch
//! that is under- or over-constrained, and does it behave while a point is
//! being dragged. A candidate that cannot answer one of those is telling us
//! something.
//!
//! # Why the numerics are written out
//!
//! Taking them from a library would mean comparing solvers through whatever
//! that library makes convenient. Every choice here — the damping, the
//! tolerance a rank is judged against, how a step is accepted — is a choice
//! the comparison is about, so each one is visible.

mod corpus;
mod linalg;
mod lm;

pub use corpus::{Corpus, problem};
pub use linalg::Matrix;
pub use lm::{DoesNothing, LevenbergMarquardt};

use std::time::Duration;

/// A point in the sketch plane, by index into the unknown vector.
///
/// Each point occupies two unknowns, `2i` and `2i + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point(pub usize);

impl Point {
    pub fn x(self) -> usize {
        self.0 * 2
    }
    pub fn y(self) -> usize {
        self.0 * 2 + 1
    }
}

/// One relationship a solved sketch has to satisfy.
///
/// Deliberately a small set. These are what a rectangle, a slot and a bracket
/// are made of, and a solver that struggles on them will not be rescued by
/// having tangency as well.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Constraint {
    /// Two points occupy the same place. Two equations.
    Coincident { a: Point, b: Point },
    /// A point is pinned where it is. Two equations.
    Fixed { point: Point, x: f64, y: f64 },
    /// The distance between two points. One equation.
    Distance { a: Point, b: Point, distance: f64 },
    /// Two points share a y coordinate. One equation.
    Horizontal { a: Point, b: Point },
    /// Two points share an x coordinate. One equation.
    Vertical { a: Point, b: Point },
    /// Two segments are the same length. One equation.
    EqualLength {
        a: (Point, Point),
        b: (Point, Point),
    },
    /// Two segments meet at a right angle. One equation.
    Perpendicular {
        a: (Point, Point),
        b: (Point, Point),
    },
    /// Two segments run in the same direction. One equation.
    Parallel {
        a: (Point, Point),
        b: (Point, Point),
    },
}

impl Constraint {
    /// How many equations this constraint contributes.
    pub fn equations(&self) -> usize {
        match self {
            Self::Coincident { .. } | Self::Fixed { .. } => 2,
            _ => 1,
        }
    }

    /// Writes this constraint's residuals, and their derivatives.
    ///
    /// `row` is where in the system this constraint's first equation sits.
    fn evaluate(&self, state: &[f64], row: usize, residuals: &mut [f64], jacobian: &mut Matrix) {
        let get = |index: usize| state[index];
        let mut equation = |offset: usize, value: f64, terms: &[(usize, f64)]| {
            residuals[row + offset] = value;
            for (column, derivative) in terms {
                jacobian.add(row + offset, *column, *derivative);
            }
        };

        match *self {
            Self::Coincident { a, b } => {
                equation(0, get(a.x()) - get(b.x()), &[(a.x(), 1.0), (b.x(), -1.0)]);
                equation(1, get(a.y()) - get(b.y()), &[(a.y(), 1.0), (b.y(), -1.0)]);
            }
            Self::Fixed { point, x, y } => {
                equation(0, get(point.x()) - x, &[(point.x(), 1.0)]);
                equation(1, get(point.y()) - y, &[(point.y(), 1.0)]);
            }
            Self::Distance { a, b, distance } => {
                let (dx, dy) = (get(a.x()) - get(b.x()), get(a.y()) - get(b.y()));
                let length = (dx * dx + dy * dy).sqrt();
                // Squared form near zero: the derivative of a length is not
                // defined when two points coincide, and a solver stepping
                // through that state should not meet an infinity.
                if length < 1e-9 {
                    equation(
                        0,
                        dx * dx + dy * dy - distance * distance,
                        &[
                            (a.x(), 2.0 * dx),
                            (b.x(), -2.0 * dx),
                            (a.y(), 2.0 * dy),
                            (b.y(), -2.0 * dy),
                        ],
                    );
                } else {
                    equation(
                        0,
                        length - distance,
                        &[
                            (a.x(), dx / length),
                            (b.x(), -dx / length),
                            (a.y(), dy / length),
                            (b.y(), -dy / length),
                        ],
                    );
                }
            }
            Self::Horizontal { a, b } => {
                equation(0, get(a.y()) - get(b.y()), &[(a.y(), 1.0), (b.y(), -1.0)]);
            }
            Self::Vertical { a, b } => {
                equation(0, get(a.x()) - get(b.x()), &[(a.x(), 1.0), (b.x(), -1.0)]);
            }
            Self::EqualLength { a, b } => {
                let (ax, ay) = (get(a.0.x()) - get(a.1.x()), get(a.0.y()) - get(a.1.y()));
                let (bx, by) = (get(b.0.x()) - get(b.1.x()), get(b.0.y()) - get(b.1.y()));
                equation(
                    0,
                    (ax * ax + ay * ay) - (bx * bx + by * by),
                    &[
                        (a.0.x(), 2.0 * ax),
                        (a.1.x(), -2.0 * ax),
                        (a.0.y(), 2.0 * ay),
                        (a.1.y(), -2.0 * ay),
                        (b.0.x(), -2.0 * bx),
                        (b.1.x(), 2.0 * bx),
                        (b.0.y(), -2.0 * by),
                        (b.1.y(), 2.0 * by),
                    ],
                );
            }
            Self::Perpendicular { a, b } => {
                let (ax, ay) = (get(a.1.x()) - get(a.0.x()), get(a.1.y()) - get(a.0.y()));
                let (bx, by) = (get(b.1.x()) - get(b.0.x()), get(b.1.y()) - get(b.0.y()));
                equation(
                    0,
                    ax * bx + ay * by,
                    &[
                        (a.1.x(), bx),
                        (a.0.x(), -bx),
                        (a.1.y(), by),
                        (a.0.y(), -by),
                        (b.1.x(), ax),
                        (b.0.x(), -ax),
                        (b.1.y(), ay),
                        (b.0.y(), -ay),
                    ],
                );
            }
            Self::Parallel { a, b } => {
                let (ax, ay) = (get(a.1.x()) - get(a.0.x()), get(a.1.y()) - get(a.0.y()));
                let (bx, by) = (get(b.1.x()) - get(b.0.x()), get(b.1.y()) - get(b.0.y()));
                equation(
                    0,
                    ax * by - ay * bx,
                    &[
                        (a.1.x(), by),
                        (a.0.x(), -by),
                        (a.1.y(), -bx),
                        (a.0.y(), bx),
                        (b.1.y(), ax),
                        (b.0.y(), -ax),
                        (b.1.x(), -ay),
                        (b.0.x(), ay),
                    ],
                );
            }
        }
    }
}

/// A sketch to be solved: points, their starting places, and the rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub name: String,
    /// Two values per point, x then y.
    pub start: Vec<f64>,
    pub constraints: Vec<Constraint>,
}

impl Problem {
    pub fn unknowns(&self) -> usize {
        self.start.len()
    }

    pub fn equations(&self) -> usize {
        self.constraints.iter().map(Constraint::equations).sum()
    }

    /// The residual vector and Jacobian at `state`.
    pub fn evaluate(&self, state: &[f64]) -> (Vec<f64>, Matrix) {
        let rows = self.equations();
        let mut residuals = vec![0.0; rows];
        let mut jacobian = Matrix::zeros(rows, self.unknowns());

        let mut row = 0;
        for constraint in &self.constraints {
            constraint.evaluate(state, row, &mut residuals, &mut jacobian);
            row += constraint.equations();
        }
        (residuals, jacobian)
    }

    /// What this sketch is, before anyone tries to solve it.
    ///
    /// Computed from the Jacobian's rank at the starting state. A rank below
    /// the number of unknowns leaves degrees of freedom; a rank below the
    /// number of equations means some constraints say what others already
    /// said, which is the case a person most needs told about because the
    /// sketch will still solve and will still be wrong to edit.
    pub fn diagnose(&self, tolerance: f64) -> Diagnosis {
        let (_, jacobian) = self.evaluate(&self.start);
        let rank = jacobian.rank(tolerance);
        Diagnosis {
            unknowns: self.unknowns(),
            equations: self.equations(),
            rank,
            degrees_of_freedom: self.unknowns().saturating_sub(rank),
            redundant: self.equations().saturating_sub(rank),
        }
    }
}

/// What a sketch's constraint system looks like structurally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnosis {
    pub unknowns: usize,
    pub equations: usize,
    pub rank: usize,
    /// How much freedom is left. Zero means fully constrained.
    pub degrees_of_freedom: usize,
    /// How many equations repeat something already said.
    pub redundant: usize,
}

impl Diagnosis {
    pub fn is_fully_constrained(&self) -> bool {
        self.degrees_of_freedom == 0 && self.redundant == 0
    }
}

/// What a candidate solver did with a problem.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub converged: bool,
    pub iterations: usize,
    /// The largest single residual left, in millimetres or mm².
    pub worst_residual: f64,
    pub elapsed: Duration,
    pub solution: Vec<f64>,
}

/// A candidate. Every one is asked the same questions the same way.
pub trait Solver {
    fn name(&self) -> &'static str;

    /// Solves from `start`, which may be the problem's own or a dragged state.
    fn solve(&self, problem: &Problem, start: &[f64]) -> Outcome;
}
