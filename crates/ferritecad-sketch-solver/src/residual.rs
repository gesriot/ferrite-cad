// SPDX-License-Identifier: MIT
//! How far a state is from satisfying the caller's constraints.
//!
//! Measurement, not solving. There is no step, no damping and no Jacobian
//! here: this evaluates the constraints once at one state and reports the
//! largest violation. The product path never substitutes arithmetic of its own
//! for planegcs, and this is not that — it is the check that what planegcs
//! handed back is what the caller asked for.
//!
//! It exists because the native boundary distinguishes a solution that zeroes
//! the error function from one that only minimises it, and the second is what
//! an impossible sketch produces. Believing the status alone would report the
//! 10-10-40 triangle as solved, which is the one failure that turns a solver
//! problem into a wrong drawing.

use crate::Constraint;
use crate::prepared::Prepared;

/// The largest single constraint residual at `state`.
///
/// In the units of whichever constraint produced it: lengths for distances,
/// squared quantities for equal-length and perpendicularity. That is why the
/// limit it is judged against is a number and not a tolerance in millimetres.
pub(crate) fn worst(prepared: &Prepared, state: &[f64]) -> f64 {
    let at = |slot: usize| (state[slot], state[slot + 1]);
    let mut worst = 0.0f64;
    let mut note = |value: f64| worst = worst.max(value.abs());

    for constraint in &prepared.constraints {
        let slot = |point| prepared.slot_of(point);
        match *constraint {
            Constraint::Coincident { a, b } => {
                let ((ax, ay), (bx, by)) = (at(slot(a)), at(slot(b)));
                note(ax - bx);
                note(ay - by);
            }
            Constraint::Fixed { point, x, y } => {
                let (px, py) = at(slot(point));
                note(px - x);
                note(py - y);
            }
            Constraint::Distance { a, b, distance } => {
                let ((ax, ay), (bx, by)) = (at(slot(a)), at(slot(b)));
                let (dx, dy) = (ax - bx, ay - by);
                note((dx * dx + dy * dy).sqrt() - distance);
            }
            Constraint::Horizontal { a, b } => {
                note(at(slot(a)).1 - at(slot(b)).1);
            }
            Constraint::Vertical { a, b } => {
                note(at(slot(a)).0 - at(slot(b)).0);
            }
            Constraint::EqualLength { a, b } => {
                let (ax, ay) = delta(state, slot(a.0), slot(a.1));
                let (bx, by) = delta(state, slot(b.0), slot(b.1));
                note((ax * ax + ay * ay) - (bx * bx + by * by));
            }
            Constraint::Perpendicular { a, b } => {
                let (ax, ay) = delta(state, slot(a.1), slot(a.0));
                let (bx, by) = delta(state, slot(b.1), slot(b.0));
                note(ax * bx + ay * by);
            }
            Constraint::Parallel { a, b } => {
                let (ax, ay) = delta(state, slot(a.1), slot(a.0));
                let (bx, by) = delta(state, slot(b.1), slot(b.0));
                note(ax * by - ay * bx);
            }
        }
    }
    worst
}

fn delta(state: &[f64], from: usize, to: usize) -> (f64, f64) {
    (state[from] - state[to], state[from + 1] - state[to + 1])
}
