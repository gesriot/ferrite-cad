// SPDX-License-Identifier: MIT
//! What a caller says to the sketch solver, and what it hears back.
//!
//! Everything here is stated in the caller's own terms. A point is whichever
//! point the caller called it; a constraint is whichever constraint the caller
//! numbered. planegcs has its own numbering for both — a parameter block, and
//! an integer tag per equation group — and neither ever reaches this file. A
//! diagnosis that named a native tag would be asking the person who drew the
//! sketch to know how the solver stored it.

/// A point in the sketch plane, named by whoever owns the sketch.
///
/// The value is the caller's and is never re-issued, re-based or compacted.
/// Sparse and non-sequential identifiers are ordinary: a sketch that has had
/// points deleted from it has exactly that shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointId(pub u64);

/// One constraint, named by whoever owns the sketch.
///
/// The same promise as [`PointId`], and the one that matters most: this is
/// what a conflict is reported in. planegcs blames its own tags, the solver
/// maps them back here, and a caller never learns that tags existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstraintId(pub u64);

/// Where a point is: on the way in as a starting guess, on the way out as an
/// answer.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Position {
    pub point: PointId,
    pub x: f64,
    pub y: f64,
}

impl Position {
    pub fn new(point: PointId, x: f64, y: f64) -> Self {
        Self { point, x, y }
    }
}

/// One relationship a solved sketch has to satisfy.
///
/// These eight are the ones the solver comparison measured, and nothing here
/// is wider than what was measured. Arcs, circles, tangency and symmetry are
/// what planegcs was chosen for and are not in this slice.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Constraint {
    /// Two points occupy the same place.
    Coincident { a: PointId, b: PointId },
    /// A point is pinned where it is told.
    Fixed { point: PointId, x: f64, y: f64 },
    /// The distance between two points.
    Distance {
        a: PointId,
        b: PointId,
        distance: f64,
    },
    /// Two points share a y coordinate.
    Horizontal { a: PointId, b: PointId },
    /// Two points share an x coordinate.
    Vertical { a: PointId, b: PointId },
    /// Two segments are the same length.
    EqualLength {
        a: (PointId, PointId),
        b: (PointId, PointId),
    },
    /// Two segments meet at a right angle.
    Perpendicular {
        a: (PointId, PointId),
        b: (PointId, PointId),
    },
    /// Two segments run in the same direction.
    Parallel {
        a: (PointId, PointId),
        b: (PointId, PointId),
    },
}

impl Constraint {
    /// Every point this constraint refers to, in the order it names them.
    ///
    /// One list, used by validation, by encoding and by residual evaluation
    /// alike, so that a constraint cannot be checked against one set of
    /// references and solved against another.
    pub(crate) fn points(&self) -> Vec<PointId> {
        match *self {
            Self::Fixed { point, .. } => vec![point],
            Self::Coincident { a, b }
            | Self::Distance { a, b, .. }
            | Self::Horizontal { a, b }
            | Self::Vertical { a, b } => vec![a, b],
            Self::EqualLength { a, b } | Self::Perpendicular { a, b } | Self::Parallel { a, b } => {
                vec![a.0, a.1, b.0, b.1]
            }
        }
    }

    /// The numbers this constraint carries, which must all be finite.
    pub(crate) fn parameters(&self) -> Vec<f64> {
        match *self {
            Self::Fixed { x, y, .. } => vec![x, y],
            Self::Distance { distance, .. } => vec![distance],
            _ => Vec::new(),
        }
    }
}

/// A sketch: points, where they start, and the rules between them.
///
/// Stored in the order the caller added them, which is the order this crate
/// hands to planegcs. That order is an implementation detail of the native
/// system and never appears in a result: a caller who adds the same
/// constraints in a different order gets the same identifiers back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sketch {
    points: Vec<Position>,
    constraints: Vec<(ConstraintId, Constraint)>,
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_point(&mut self, id: PointId, x: f64, y: f64) -> &mut Self {
        self.points.push(Position::new(id, x, y));
        self
    }

    pub fn add_constraint(&mut self, id: ConstraintId, constraint: Constraint) -> &mut Self {
        self.constraints.push((id, constraint));
        self
    }

    pub fn points(&self) -> &[Position] {
        &self.points
    }

    pub fn constraints(&self) -> &[(ConstraintId, Constraint)] {
        &self.constraints
    }
}

/// What the solver makes of a system's structure, before it is solved.
///
/// The three things planegcs actually reports about a system it has
/// diagnosed, and no fourth thing invented beside them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Diagnosis {
    degrees_of_freedom: usize,
    conflicting: Vec<ConstraintId>,
    redundant: Vec<ConstraintId>,
}

impl Diagnosis {
    pub(crate) fn new(
        degrees_of_freedom: usize,
        conflicting: Vec<ConstraintId>,
        redundant: Vec<ConstraintId>,
    ) -> Self {
        Self {
            degrees_of_freedom,
            conflicting,
            redundant,
        }
    }

    /// How much freedom is left. Zero means the sketch cannot move.
    pub fn degrees_of_freedom(&self) -> usize {
        self.degrees_of_freedom
    }

    /// Constraints that cannot all hold at once, in the caller's numbering.
    pub fn conflicting(&self) -> &[ConstraintId] {
        &self.conflicting
    }

    /// Constraints that say something already said, in the caller's numbering.
    ///
    /// Kept apart from [`Diagnosis::conflicting`] because they are different
    /// facts about a sketch and call for different answers: a redundant sketch
    /// still solves, and telling somebody to delete a constraint that is
    /// merely repeated is different advice from telling them their drawing is
    /// impossible.
    pub fn redundant(&self) -> &[ConstraintId] {
        &self.redundant
    }

    pub fn is_under_constrained(&self) -> bool {
        self.degrees_of_freedom > 0
    }
}

/// A solved sketch.
///
/// Only ever produced when every constraint is satisfied to within
/// [`crate::RESIDUAL_LIMIT`]. A sketch that is under-constrained or carries
/// redundant constraints still solves, and both facts travel with the answer
/// rather than withholding it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Solution {
    positions: Vec<Position>,
    degrees_of_freedom: usize,
    redundant: Vec<ConstraintId>,
    worst_residual: f64,
}

impl Solution {
    pub(crate) fn new(
        positions: Vec<Position>,
        degrees_of_freedom: usize,
        redundant: Vec<ConstraintId>,
        worst_residual: f64,
    ) -> Self {
        Self {
            positions,
            degrees_of_freedom,
            redundant,
            worst_residual,
        }
    }

    /// Where every point ended up, one entry per point of the sketch.
    pub fn positions(&self) -> &[Position] {
        &self.positions
    }

    pub fn position(&self, point: PointId) -> Option<Position> {
        self.positions.iter().copied().find(|p| p.point == point)
    }

    pub fn degrees_of_freedom(&self) -> usize {
        self.degrees_of_freedom
    }

    pub fn is_under_constrained(&self) -> bool {
        self.degrees_of_freedom > 0
    }

    pub fn redundant(&self) -> &[ConstraintId] {
        &self.redundant
    }

    /// The largest single constraint residual left, measured by this crate
    /// against the caller's own constraints rather than reported by planegcs.
    ///
    /// In the units of whichever constraint produced it: a distance residual
    /// is a length and an equal-length residual is an area, so this is a
    /// numeric quantity and not one physical measurement.
    pub fn worst_residual(&self) -> f64 {
        self.worst_residual
    }
}

/// What became of a sketch that was handed to the solver.
///
/// The distinctions are the ones the native boundary really draws. planegcs
/// separates a solution that zeroes the error function from one that only
/// minimises it, reports degrees of freedom, and names conflicting and
/// redundant constraints apart; those are the differences here. It does not
/// distinguish, say, a sketch that is impossible from one the solver merely
/// could not reach, so neither does this.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Outcome {
    /// Every constraint holds. Possibly under-constrained, possibly with
    /// redundant constraints; both are reported inside.
    Solved(Solution),
    /// Constraints that cannot all hold at once, named in the caller's
    /// numbering. No positions: a sketch that was refused is not a sketch
    /// that was half moved.
    Conflicting {
        constraints: Vec<ConstraintId>,
        redundant: Vec<ConstraintId>,
    },
    /// The system was built and solved, and the answer does not satisfy it.
    ///
    /// Carries no positions, for the same reason. A partially moved sketch
    /// published after a refusal is the one failure mode that turns a solver
    /// error into a wrong drawing.
    ///
    /// `worst_residual` is `None` when planegcs refused outright and left no
    /// state to measure. Reporting an infinity there would be quoting a
    /// measurement that was never taken.
    DidNotConverge { worst_residual: Option<f64> },
}

impl Outcome {
    pub fn solution(&self) -> Option<&Solution> {
        match self {
            Self::Solved(solution) => Some(solution),
            _ => None,
        }
    }

    pub fn is_solved(&self) -> bool {
        matches!(self, Self::Solved(_))
    }
}
