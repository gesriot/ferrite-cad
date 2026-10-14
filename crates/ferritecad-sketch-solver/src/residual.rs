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

/// The largest single constraint residual at `state` and `radii`.
///
/// In the units of whichever constraint produced it: lengths for distances,
/// squared quantities for equal-length and perpendicularity. That is why the
/// limit it is judged against is a number and not a tolerance in millimetres.
pub(crate) fn worst(prepared: &Prepared, state: &[f64], radii: &[f64]) -> f64 {
    let at = |slot: usize| (state[slot], state[slot + 1]);
    let mut worst = 0.0f64;
    let mut note = |value: f64| worst = worst.max(value.abs());

    for constraint in &prepared.constraints {
        let slot = |point| prepared.slot_of(point);
        match *constraint {
            // Measured against the radius that came back, which is the whole
            // point: planegcs reports "minimised" for a system it could not
            // satisfy, and the number it was asked for is the one thing that
            // would agree with itself no matter what it returned.
            Constraint::Radius { circle, radius } => {
                note(radii[prepared.circle_slot(circle)] - radius);
            }
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
            Constraint::DirectedAngle { a, b, angle } => {
                let a = delta(state, slot(a.1), slot(a.0));
                let b = delta(state, slot(b.1), slot(b.0));
                note(directed_angle_error(a, b, angle));
            }
        }
    }
    worst
}

fn delta(state: &[f64], from: usize, to: usize) -> (f64, f64) {
    (state[from] - state[to], state[from + 1] - state[to + 1])
}

/// How far the turn from `a` to `b` is from `angle`, in radians, wrapped across
/// `±π` so that `-π` and `+π` — one direction measured from either side — agree.
///
/// Signed on purpose: `atan2(cross, dot)` is the counter-clockwise angle, so
/// `+30°` is `60°` away from a request for `-30°` and `0` is `π` away from `π`.
/// `acos(dot)` could not tell those apart. A vector with no direction is
/// infinitely far: `atan2(0, 0)` is `0`, and without this a collapsed segment
/// would satisfy a `0°` request.
fn directed_angle_error(a: (f64, f64), b: (f64, f64), angle: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    if a.0.hypot(a.1) <= crate::MIN_ANGLE_VECTOR || b.0.hypot(b.1) <= crate::MIN_ANGLE_VECTOR {
        return f64::INFINITY;
    }
    let measured = (a.0 * b.1 - a.1 * b.0).atan2(a.0 * b.0 + a.1 * b.1);
    let error = measured - angle;
    if error > PI {
        error - TAU
    } else if error < -PI {
        error + TAU
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::directed_angle_error as error;
    use std::f64::consts::PI;

    fn unit(degrees: f64) -> (f64, f64) {
        (degrees.to_radians().cos(), degrees.to_radians().sin())
    }

    #[test]
    fn the_residual_is_signed_wraps_across_the_boundary_and_refuses_a_vector_with_no_direction() {
        let x = (3.0, 0.0);
        // Signed: +30° holds for +30° and is a 60° miss for -30°.
        assert!(error(x, unit(30.0), 30f64.to_radians()).abs() < 1e-12);
        assert!(error(x, unit(-30.0), (-30f64).to_radians()).abs() < 1e-12);
        let miss = error(x, unit(30.0), (-30f64).to_radians());
        assert!((miss - 60f64.to_radians()).abs() < 1e-12, "{miss}");
        // 0° is codirectional and is a full half-turn from 180°, either way.
        assert!(error(x, (2.0, 0.0), 0.0).abs() < 1e-12);
        assert!((error(x, (2.0, 0.0), PI).abs() - PI).abs() < 1e-12);
        assert!((error(x, (-2.0, 0.0), 0.0).abs() - PI).abs() < 1e-12);
        // Opposite is +π whichever side of the axis rounding put the noise on:
        // atan2 returns -π for -0.0 and +π for +0.0.
        for noise in [1e-17, -1e-17, 0.0, -0.0] {
            assert!(error(x, (-2.0, noise), PI).abs() < 1e-12, "noise {noise}");
        }
        // Across ±180°: 170° measured against -170° is a 20° miss, not 340°.
        let near = error(x, unit(170.0), (-170f64).to_radians());
        assert!((near.abs() - 20f64.to_radians()).abs() < 1e-12, "{near}");
        let near = error(x, unit(-170.0), 170f64.to_radians());
        assert!((near.abs() - 20f64.to_radians()).abs() < 1e-12, "{near}");
        // A vector with no direction is infinitely far, even though
        // atan2(0, 0) = 0 would satisfy a 0° request.
        for (a, b) in [
            (x, (0.0, 0.0)),
            ((0.0, 0.0), x),
            (x, (1e-10, 0.0)),
            ((0.0, -1e-10), x),
        ] {
            assert_eq!(error(a, b, 0.0), f64::INFINITY, "{a:?} {b:?}");
        }
        println!("\nFCAD_31F_RESIDUAL_EXECUTED");
    }
}
