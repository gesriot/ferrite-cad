// SPDX-License-Identifier: MIT
//! Why the solver refused, in terms the caller can act on.
//!
//! Every variant names the caller's own point or constraint. None of them
//! carries a native pointer, a native tag, an equation index, a status code or
//! anything else that would only mean something inside planegcs: a caller
//! cannot act on those, and a message containing one invites somebody to
//! depend on it.

use crate::{ConstraintId, PointId};

/// Why there is no solver to call.
///
/// A distinct, typed answer rather than a skipped test or a panic. The point
/// of naming it is that the caller above must decide what to do — and that a
/// build without planegcs can never quietly become a build that solved the
/// sketch some other way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Unavailable {
    /// This build did not link planegcs, so there is nothing to ask.
    #[error(
        "this build did not link planegcs, so FerriteCAD has no sketch solver; build it with \
         tools/build-planegcs.sh and point FCAD_PLANEGCS_DIR at the result"
    )]
    NotLinked,
}

/// Which number was not a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NotFinite {
    #[error("point {0:?} has a coordinate that is not finite")]
    PointCoordinate(PointId),
    #[error("constraint {0:?} carries a value that is not finite")]
    ConstraintParameter(ConstraintId),
}

/// A refusal from the native solver itself.
///
/// Deliberately three named situations rather than the native status number.
/// The number is planegcs's, it is not stable across releases of it, and a
/// caller who matched on it would be depending on the library rather than on
/// this contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NativeFailure {
    #[error("the sketch solver could not build this system")]
    CouldNotBuild,
    #[error("the sketch solver could not analyse this system")]
    CouldNotDiagnose,
    #[error("the sketch solver refused the request")]
    Refused,
}

/// Everything the public boundary can say instead of an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SolverError {
    #[error(transparent)]
    Unavailable(#[from] Unavailable),

    /// A constraint refers to a point the sketch does not contain.
    #[error("constraint {constraint:?} refers to {point:?}, which is not in this sketch")]
    UnknownPoint {
        constraint: ConstraintId,
        point: PointId,
    },

    /// The same point identifier was used twice.
    #[error("{0:?} appears more than once in this sketch")]
    DuplicatePoint(PointId),

    /// The same constraint identifier was used twice.
    ///
    /// Refused rather than tolerated because a conflict is reported by
    /// identifier: two constraints sharing one would make the answer
    /// ambiguous exactly when it matters most.
    #[error("{0:?} appears more than once in this sketch")]
    DuplicateConstraint(ConstraintId),

    #[error(transparent)]
    NotFinite(#[from] NotFinite),

    /// A starting state that is not one position per point of the sketch.
    #[error("this sketch has {expected} point(s) and the starting state has {actual}")]
    StateSize { expected: usize, actual: usize },

    /// A starting state that names a point the sketch does not contain.
    #[error("the starting state names {0:?}, which is not in this sketch")]
    UnknownPointInState(PointId),

    #[error(transparent)]
    Native(#[from] NativeFailure),
}

impl SolverError {
    /// Whether this refusal is "there is no solver here" rather than
    /// "this sketch is wrong".
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}
