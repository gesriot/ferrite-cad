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
            Constraint::Collinear { a, b } => {
                let ends = |(from, to): (_, _)| (at(slot(from)), at(slot(to)));
                note(collinear_error(ends(a), ends(b)));
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

/// How far two segments are from lying on one line: the worst of
///
/// * the sine of the angle between them, which is dimensionless, and
/// * the distance of each end of each segment from the line through the
///   other, which is a length.
///
/// Both are needed. The sine alone is the cross product, which two parallel
/// segments on different lines also zero; the distances alone say little about
/// the direction of a short segment. Taking each segment's ends against the
/// other's line makes the answer the same whichever segment is called `a`.
/// Neither the direction nor the order of the ends enters, and the segments
/// need not touch. A segment with no length has no line and is infinitely far.
fn collinear_error(a: ((f64, f64), (f64, f64)), b: ((f64, f64), (f64, f64))) -> f64 {
    let run = |(from, to): ((f64, f64), (f64, f64))| (to.0 - from.0, to.1 - from.1);
    let cross = |u: (f64, f64), v: (f64, f64)| u.0 * v.1 - u.1 * v.0;
    let (ra, rb) = (run(a), run(b));
    let (la, lb) = (ra.0.hypot(ra.1), rb.0.hypot(rb.1));
    if la <= crate::MIN_ANGLE_VECTOR || lb <= crate::MIN_ANGLE_VECTOR {
        return f64::INFINITY;
    }
    // Signed distance of `p` from the line through `from` along `along`.
    let off = |p: (f64, f64), from: (f64, f64), along: (f64, f64), length: f64| {
        cross(along, (p.0 - from.0, p.1 - from.1)) / length
    };
    let measured = [
        cross(ra, rb) / (la * lb),
        off(a.0, b.0, rb, lb),
        off(a.1, b.0, rb, lb),
        off(b.0, a.0, ra, la),
        off(b.1, a.0, ra, la),
    ];
    // `f64::max` skips a NaN, so a state that is not a number would otherwise
    // read as the best answer there is.
    if measured.iter().any(|value| !value.is_finite()) {
        return f64::INFINITY;
    }
    measured
        .into_iter()
        .fold(0.0, |worst: f64, value| worst.max(value.abs()))
}

#[cfg(test)]
mod tests {
    use super::collinear_error;
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

    #[test]
    fn the_collinear_residual_holds_direction_and_offset_and_nothing_else() {
        let on_x = ((0.0, 0.0), (4.0, 0.0));
        let flip = |s: ((f64, f64), (f64, f64))| (s.1, s.0);
        // Disjoint, reversed, nested and touching segments of one line are
        // satisfied alike: neither overlap nor order is part of the relation.
        for other in [
            ((10.0, 0.0), (30.0, 0.0)),
            ((30.0, 0.0), (10.0, 0.0)),
            ((-9.0, 0.0), (-2.0, 0.0)),
            ((1.0, 0.0), (2.0, 0.0)),
            ((4.0, 0.0), (6.0, 0.0)),
        ] {
            for (a, b) in [(on_x, other), (other, on_x), (flip(on_x), other)] {
                assert_eq!(collinear_error(a, b), 0.0, "{a:?} {b:?}");
            }
        }
        // Parallel is not enough: the cross product is zero here and the
        // segments are a full 3 units apart, which is what the offset says.
        let parallel = ((10.0, 3.0), (30.0, 3.0));
        assert_eq!(collinear_error(on_x, parallel), 3.0);
        assert_eq!(collinear_error(parallel, on_x), 3.0);
        assert_eq!(collinear_error(on_x, flip(parallel)), 3.0);
        // A segment turned about a point of the line: its far end is 1 away
        // from it, whichever segment is called `a`.
        let turned = ((10.0, 0.0), (20.0, 1.0));
        assert_eq!(collinear_error(on_x, turned), 1.0);
        assert_eq!(collinear_error(turned, on_x), 1.0);
        // Among short segments the angle is what is wrong, and it is read as a
        // dimensionless sine (about 1e-3) rather than as the 1e-6 length that
        // the far end happens to be off the line.
        let small = ((0.0, 0.0), (1e-3, 0.0));
        let small_turned = ((2e-3, 0.0), (3e-3, 1e-6));
        let got = collinear_error(small, small_turned);
        assert!((got - 1e-3).abs() < 1e-6, "{got}");
        assert_eq!(got, collinear_error(small_turned, small), "symmetric");
        // An offset that is tiny against the lengths still counts as itself.
        let nudged = ((10.0, 1e-5), (30.0, 1e-5));
        assert!((collinear_error(on_x, nudged) - 1e-5).abs() < 1e-15);
        let nudged = ((10.0, 1e-7), (30.0, 1e-7));
        assert!(collinear_error(on_x, nudged) < crate::RESIDUAL_LIMIT);
        // A segment with no length has no line, and lies on every line.
        for (a, b) in [
            (on_x, ((9.0, 0.0), (9.0, 0.0))),
            (((9.0, 0.0), (9.0, 0.0)), on_x),
            (on_x, ((9.0, 0.0), (9.0 + 1e-10, 0.0))),
        ] {
            assert_eq!(collinear_error(a, b), f64::INFINITY, "{a:?} {b:?}");
        }
        // And a state that is not a number is not a satisfied one.
        for bad in [f64::NAN, f64::INFINITY] {
            assert_eq!(
                collinear_error(on_x, ((10.0, bad), (30.0, 0.0))),
                f64::INFINITY,
                "{bad}"
            );
        }
        println!("\nFCAD_31G_RESIDUAL_EXECUTED");
    }
}
