// SPDX-License-Identifier: MIT
//! A sketch checked and laid out for the native solver, and the table that
//! brings the caller's identifiers back.
//!
//! Everything the public boundary refuses is refused here, which is before any
//! FFI call is made: a sketch that names a point it does not contain, that
//! uses one identifier twice, that carries a coordinate or a dimension which
//! is not a number, or that is given a starting state of the wrong shape.
//! Sending any of those across a C boundary and hoping the other side notices
//! would make the check a property of planegcs rather than of this contract.
//!
//! The other half of this file is the identifier table. planegcs is told about
//! constraints by position and blames them by tag; the caller knows neither.
//! `caller_ids` is the only route between the two, and it is a lookup rather
//! than arithmetic on purpose — an identifier that happened to equal its own
//! position would otherwise hide every mistake in this direction.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Constraint, ConstraintId, NotFinite, PointId, Position, Sketch, SolverError};

pub(crate) struct Prepared {
    /// Points in storage order; the native parameter block is two doubles per
    /// entry, in this order.
    point_ids: Vec<PointId>,
    index_of: BTreeMap<PointId, usize>,
    /// Two values per point, x then y.
    pub(crate) state: Vec<f64>,
    /// Constraints in storage order, as the caller stated them.
    pub(crate) constraints: Vec<Constraint>,
    /// The caller's identifier for each stored constraint, by storage
    /// position. Shorter than `constraints` when a gesture has appended a pin
    /// of its own, which is not the caller's and has no identifier.
    caller_ids: Vec<ConstraintId>,
}

/// A `Debug` that cannot leak the native layout.
///
/// Written out rather than derived because the derived one would print the
/// parameter block and the storage order, and something printed is something
/// somebody will read a meaning into. What is true of this type from outside
/// is how big the sketch is.
impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("points", &self.point_ids.len())
            .field("constraints", &self.constraints.len())
            .finish()
    }
}

impl Prepared {
    /// Checks a sketch and lays it out, optionally from a supplied state.
    pub(crate) fn new(sketch: &Sketch, start: Option<&[Position]>) -> Result<Self, SolverError> {
        let mut index_of = BTreeMap::new();
        let mut point_ids = Vec::with_capacity(sketch.points().len());
        for (index, position) in sketch.points().iter().enumerate() {
            if index_of.insert(position.point, index).is_some() {
                return Err(SolverError::DuplicatePoint(position.point));
            }
            point_ids.push(position.point);
        }

        let mut state = vec![0.0; point_ids.len() * 2];
        let positions = match start {
            None => sketch.points(),
            Some(given) => {
                if given.len() != point_ids.len() {
                    return Err(SolverError::StateSize {
                        expected: point_ids.len(),
                        actual: given.len(),
                    });
                }
                given
            }
        };
        // Written by identity rather than by position, so a state given in a
        // different order still lands on the right point, and one that names a
        // point twice is caught by the seen-set rather than by silently
        // overwriting the first.
        let mut seen = BTreeSet::new();
        for position in positions {
            let Some(&index) = index_of.get(&position.point) else {
                return Err(SolverError::UnknownPointInState(position.point));
            };
            if !seen.insert(position.point) {
                return Err(SolverError::DuplicatePoint(position.point));
            }
            if !position.x.is_finite() || !position.y.is_finite() {
                return Err(NotFinite::PointCoordinate(position.point).into());
            }
            state[index * 2] = position.x;
            state[index * 2 + 1] = position.y;
        }

        let mut caller_ids = Vec::with_capacity(sketch.constraints().len());
        let mut constraints = Vec::with_capacity(sketch.constraints().len());
        let mut named = BTreeSet::new();
        for &(id, constraint) in sketch.constraints() {
            if !named.insert(id) {
                return Err(SolverError::DuplicateConstraint(id));
            }
            for point in constraint.points() {
                if !index_of.contains_key(&point) {
                    return Err(SolverError::UnknownPoint {
                        constraint: id,
                        point,
                    });
                }
            }
            if constraint.parameters().iter().any(|v| !v.is_finite()) {
                return Err(NotFinite::ConstraintParameter(id).into());
            }
            caller_ids.push(id);
            constraints.push(constraint);
        }

        Ok(Self {
            point_ids,
            index_of,
            state,
            constraints,
            caller_ids,
        })
    }

    pub(crate) fn points(&self) -> usize {
        self.point_ids.len()
    }

    /// Where a point's x sits in the parameter block. Its y is the next slot.
    pub(crate) fn slot_of(&self, point: PointId) -> usize {
        // Every point named by a stored constraint was checked to exist when
        // this was built, and nothing adds constraints afterwards except
        // `pin`, which names a point it was given from this same table.
        self.index_of
            .get(&point)
            .copied()
            .expect("every stored constraint names a point this table contains")
            * 2
    }

    /// The caller's identifier for a stored constraint, if it has one.
    ///
    /// A gesture's own pin is stored past the end of `caller_ids` and answers
    /// `None`: it is this crate's constraint, not the caller's, and naming it
    /// in a diagnosis would invent an identifier the caller never issued.
    pub(crate) fn caller_id(&self, stored: usize) -> Option<ConstraintId> {
        self.caller_ids.get(stored).copied()
    }

    /// Appends a pin holding `point` where the state currently has it, and
    /// answers where it was stored.
    ///
    /// What a drag is: one more constraint in the system, whose target moves.
    pub(crate) fn pin(&mut self, point: PointId) -> Result<usize, SolverError> {
        let Some(&index) = self.index_of.get(&point) else {
            return Err(SolverError::UnknownPointInState(point));
        };
        self.constraints.push(Constraint::Fixed {
            point,
            x: self.state[index * 2],
            y: self.state[index * 2 + 1],
        });
        Ok(self.constraints.len() - 1)
    }

    /// Moves a stored pin's target, so the copy a result is judged against
    /// says the same thing as the native system does.
    pub(crate) fn move_pin(&mut self, stored: usize, x: f64, y: f64) {
        if let Some(Constraint::Fixed {
            x: target_x,
            y: target_y,
            ..
        }) = self.constraints.get_mut(stored)
        {
            *target_x = x;
            *target_y = y;
        }
    }

    /// The state, as positions in the caller's own terms.
    pub(crate) fn positions(&self, state: &[f64]) -> Vec<Position> {
        self.point_ids
            .iter()
            .enumerate()
            .map(|(index, &point)| Position::new(point, state[index * 2], state[index * 2 + 1]))
            .collect()
    }
}
