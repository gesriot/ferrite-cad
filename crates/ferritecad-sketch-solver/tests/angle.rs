// SPDX-License-Identifier: MIT
//! The directed angle between two vectors, measured against the real library.
//!
//! The angle of every solved sketch is measured here from the solved points,
//! with `atan2(cross, dot)` written out again below, and never read back from
//! the request, the residual or the solver. A residual that agreed with itself
//! would pass a solver that lost the sign.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_sketch_solver as solver;
use ferritecad_sketch_solver::{Constraint, ConstraintId, Outcome, PointId, Sketch};
use std::f64::consts::PI;

/// Sparse identifiers on purpose: nothing may depend on position.
const P0: PointId = PointId(3);
const P1: PointId = PointId(91);
const P2: PointId = PointId(12);
const P3: PointId = PointId(400);

const ANGLE: ConstraintId = ConstraintId(70);
const OTHER: ConstraintId = ConstraintId(71);

fn native() -> bool {
    if solver::is_available() {
        true
    } else {
        ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
        false
    }
}

fn deg(angle: f64) -> f64 {
    angle.to_radians()
}

/// Two free segments: `a` is P0→P1 pinned in place along +X, `b` is P2→P3 and
/// starts `start_deg` degrees from +X with length 5.
fn two_segments(start_deg: f64) -> Sketch {
    let t = deg(start_deg);
    let mut sketch = Sketch::new();
    sketch
        .add_point(P0, 0.0, 0.0)
        .add_point(P1, 10.0, 0.0)
        .add_point(P2, 20.0, 5.0)
        .add_point(P3, 20.0 + 5.0 * t.cos(), 5.0 + 5.0 * t.sin());
    sketch
        .add_constraint(
            ConstraintId(1),
            Constraint::Fixed {
                point: P0,
                x: 0.0,
                y: 0.0,
            },
        )
        .add_constraint(
            ConstraintId(2),
            Constraint::Fixed {
                point: P1,
                x: 10.0,
                y: 0.0,
            },
        );
    sketch
}

fn with_angle(
    mut sketch: Sketch,
    a: (PointId, PointId),
    b: (PointId, PointId),
    angle: f64,
) -> Sketch {
    sketch.add_constraint(ANGLE, Constraint::DirectedAngle { a, b, angle });
    sketch
}

fn point(solution: &solver::Solution, id: PointId) -> (f64, f64) {
    let at = solution.position(id).expect("the point is in the answer");
    (at.x, at.y)
}

/// The counter-clockwise turn from `a` to `b` in degrees, from solved points.
fn measured(solution: &solver::Solution, a: (PointId, PointId), b: (PointId, PointId)) -> f64 {
    let (a0, a1) = (point(solution, a.0), point(solution, a.1));
    let (b0, b1) = (point(solution, b.0), point(solution, b.1));
    let (ax, ay) = (a1.0 - a0.0, a1.1 - a0.1);
    let (bx, by) = (b1.0 - b0.0, b1.1 - b0.1);
    (ax * by - ay * bx).atan2(ax * bx + ay * by).to_degrees()
}

fn length(solution: &solver::Solution, v: (PointId, PointId)) -> f64 {
    let (p, q) = (point(solution, v.0), point(solution, v.1));
    (q.0 - p.0).hypot(q.1 - p.1)
}

fn solved(sketch: &Sketch) -> solver::Solution {
    match solver::solve(sketch).expect("a solvable system") {
        Outcome::Solved(solution) => solution,
        other => panic!("expected a solution, got {other:?}"),
    }
}

const A: (PointId, PointId) = (P0, P1);
const B: (PointId, PointId) = (P2, P3);

/// The wrap that makes `-180` and `+180` one direction: measured modulo 360
/// into `(-180, 180]`, for comparing an answer with a request.
fn same_turn(measured: f64, asked: f64) -> bool {
    let d = (measured - asked).rem_euclid(360.0);
    d < 1e-6 || 360.0 - d < 1e-6
}

#[test]
fn a_positive_and_a_negative_angle_are_different_solutions() {
    if !native() {
        return;
    }
    for asked in [30.0f64, -30.0, 90.0, -90.0, 135.0, -135.0] {
        // Started at the opposite sign, so the solver has to cross over.
        let start = -asked.signum() * 20.0;
        let solution = solved(&with_angle(two_segments(start), A, B, deg(asked)));
        let got = measured(&solution, A, B);
        assert!(
            same_turn(got, asked),
            "asked {asked}° and the solved points measure {got}°"
        );
        assert!(
            !same_turn(got, -asked),
            "{asked}° was satisfied by its mirror image {got}°"
        );
        // The pinned vector did not move, and the other kept a direction.
        assert_eq!(point(&solution, P1), (10.0, 0.0));
        assert!(length(&solution, B) > 1.0, "the free vector collapsed");
    }
}

#[test]
fn zero_is_codirectional_and_one_hundred_eighty_is_opposite_and_neither_is_the_other() {
    if !native() {
        return;
    }
    // From the opposite arrangement each time: 0° must turn the vector round
    // and 180° must not leave it where `acos(dot)` of a parallel pair would.
    let toward = solved(&with_angle(two_segments(175.0), A, B, 0.0));
    assert!(
        same_turn(measured(&toward, A, B), 0.0),
        "0° is not codirectional"
    );
    let away = solved(&with_angle(two_segments(5.0), A, B, PI));
    let got = measured(&away, A, B);
    assert!(
        got.abs() > 179.999,
        "180° must be opposite, the solved points measure {got}°"
    );
    // The same pair of vectors under the other answer is a conflict, not a
    // sketch that "satisfies both".
    let both = with_angle(two_segments(5.0), A, B, 0.0);
    let both = {
        let mut both = both;
        both.add_constraint(
            OTHER,
            Constraint::DirectedAngle {
                a: A,
                b: B,
                angle: PI,
            },
        );
        both
    };
    match solver::solve(&both).expect("a diagnosable system") {
        Outcome::Conflicting { constraints, .. } => {
            assert_eq!(constraints, vec![ANGLE, OTHER], "both reasons are named");
        }
        other => panic!("0° and 180° on one pair must conflict, got {other:?}"),
    }
}

#[test]
fn the_turn_across_plus_and_minus_one_hundred_eighty_is_the_short_way() {
    if !native() {
        return;
    }
    // 20° of travel across the boundary, in both directions.
    for (start, asked) in [
        (-170.0, 170.0),
        (170.0, -170.0),
        (-179.0, 179.0),
        (179.0, -179.0),
    ] {
        let solution = solved(&with_angle(two_segments(start), A, B, deg(asked)));
        let got = measured(&solution, A, B);
        assert!(
            same_turn(got, asked),
            "start {start}°, asked {asked}°, got {got}°"
        );
        assert!(
            !same_turn(got, -asked),
            "start {start}°: mirrored answer {got}°"
        );
    }
}

#[test]
fn a_swapped_or_turned_vector_is_the_same_relation_with_the_documented_angle() {
    if !native() {
        return;
    }
    // The relation 30° from A to B, said four ways. Each is solved on its own
    // and every answer is measured the same, independent way.
    let reversed_a = (P1, P0);
    let reversed_b = (P3, P2);
    for (name, a, b, asked) in [
        ("A to B", A, B, 30.0),
        ("B to A (swapped, negated)", B, A, -30.0),
        ("A turned (+180 normalised)", reversed_a, B, -150.0),
        ("B turned (+180 normalised)", A, reversed_b, -150.0),
        ("both turned (unchanged)", reversed_a, reversed_b, 30.0),
    ] {
        let solution = solved(&with_angle(two_segments(-40.0), a, b, deg(asked)));
        let canonical = measured(&solution, A, B);
        assert!(
            same_turn(canonical, 30.0),
            "{name}: the solved points measure {canonical}° from A to B, not 30°"
        );
        assert!(same_turn(measured(&solution, a, b), asked), "{name}");
    }
    // 180° turned once is 0°, and turned back is 180° again.
    let opposite = solved(&with_angle(two_segments(-40.0), A, B, PI));
    assert!(measured(&opposite, A, B).abs() > 179.999);
    let reversed = solved(&with_angle(two_segments(-40.0), reversed_a, B, 0.0));
    assert!(
        measured(&reversed, A, B).abs() > 179.999,
        "180° turned once is 0°"
    );
}

#[test]
fn parallel_still_accepts_both_directions_and_does_not_invent_one() {
    if !native() {
        return;
    }
    for start in [5.0, 175.0] {
        let mut sketch = two_segments(start);
        sketch.add_constraint(ANGLE, Constraint::Parallel { a: A, b: B });
        let solution = solved(&sketch);
        let got = measured(&solution, A, B);
        let cross_free = got.abs() < 1e-6 || (got.abs() - 180.0).abs() < 1e-6;
        assert!(cross_free, "start {start}° left Parallel at {got}°");
        // It stays on the side it started on: Parallel has no direction.
        assert_eq!(
            got.abs() < 90.0,
            start < 90.0,
            "start {start}° flipped to {got}°"
        );
    }
}

#[test]
fn degrees_of_freedom_come_from_the_solver_and_one_angle_removes_exactly_one() {
    if !native() {
        return;
    }
    let free = solver::diagnose(&two_segments(10.0)).expect("diagnosable");
    let angled =
        solver::diagnose(&with_angle(two_segments(10.0), A, B, deg(40.0))).expect("diagnosable");
    assert_eq!(
        angled.degrees_of_freedom() + 1,
        free.degrees_of_freedom(),
        "{free:?} -> {angled:?}"
    );
    assert!(angled.conflicting().is_empty() && angled.redundant().is_empty());
}

/// P0→P1→P2→P3→P0, a quadrilateral, with the first edge pinned.
fn quad() -> Sketch {
    let mut sketch = Sketch::new();
    sketch
        .add_point(P0, 0.0, 0.0)
        .add_point(P1, 10.0, 1.0)
        .add_point(P2, 11.0, 9.0)
        .add_point(P3, -1.0, 8.0);
    sketch
        .add_constraint(
            ConstraintId(1),
            Constraint::Fixed {
                point: P0,
                x: 0.0,
                y: 0.0,
            },
        )
        .add_constraint(
            ConstraintId(2),
            Constraint::Fixed {
                point: P1,
                x: 10.0,
                y: 0.0,
            },
        );
    sketch
}

#[test]
fn a_fourth_right_angle_of_a_closed_contour_is_redundant_and_a_wrong_one_conflicts() {
    if !native() {
        return;
    }
    let edges = [(P0, P1), (P1, P2), (P2, P3), (P3, P0)];
    let turn = |sketch: &mut Sketch, from: usize, id: u64, angle: f64| {
        sketch.add_constraint(
            ConstraintId(100 + id),
            Constraint::DirectedAngle {
                a: edges[from],
                b: edges[(from + 1) % 4],
                angle,
            },
        );
    };
    let mut three = quad();
    for (i, id) in [(0, 0), (1, 1), (2, 2)] {
        turn(&mut three, i, id, deg(90.0));
    }
    let base = solver::diagnose(&three).expect("diagnosable");
    assert!(
        base.conflicting().is_empty() && base.redundant().is_empty(),
        "{base:?}"
    );

    // Three turns of +90° with the first edge pinned: the fourth is implied.
    let mut four = three.clone();
    turn(&mut four, 3, 3, deg(90.0));
    let diagnosis = solver::diagnose(&four).expect("diagnosable");
    assert_eq!(diagnosis.redundant().len(), 1, "{diagnosis:?}");
    assert!(diagnosis.conflicting().is_empty());
    assert!(
        (100..104).contains(&diagnosis.redundant()[0].0),
        "the redundancy is named by the caller's own id"
    );
    let solution = solved(&four);
    for (i, edge) in edges.iter().enumerate() {
        let next = edges[(i + 1) % 4];
        assert!(
            same_turn(measured(&solution, *edge, next), 90.0),
            "corner {i}"
        );
    }
    assert_eq!(solution.redundant(), diagnosis.redundant());

    // The same fourth corner turned the other way cannot hold with the rest.
    let mut wrong = three;
    turn(&mut wrong, 3, 3, deg(-90.0));
    match solver::solve(&wrong).expect("a diagnosable system") {
        Outcome::Conflicting { constraints, .. } => {
            assert!(!constraints.is_empty());
            assert!(
                constraints.iter().all(|id| (100..104).contains(&id.0)),
                "{constraints:?}"
            );
        }
        other => panic!("a contradictory contour must conflict, got {other:?}"),
    }
}

#[test]
fn a_vector_the_solver_collapses_is_never_a_satisfied_angle() {
    if !native() {
        return;
    }
    // Asked for 0°, and every point of the free vector is told to coincide:
    // `atan2(0, 0)` is 0, which is the one way to "satisfy" it.
    let mut sketch = two_segments(15.0);
    sketch
        .add_constraint(
            ANGLE,
            Constraint::DirectedAngle {
                a: A,
                b: B,
                angle: 0.0,
            },
        )
        .add_constraint(OTHER, Constraint::Coincident { a: P2, b: P3 });
    // planegcs reports success after pulling the vector to a point; the
    // measured residual is what refuses it, as infinitely far from any angle.
    match solver::solve(&sketch).expect("a diagnosable system") {
        Outcome::Solved(solution) => panic!(
            "a collapsed vector was accepted: length {}",
            length(&solution, B)
        ),
        Outcome::DidNotConverge { worst_residual } => {
            assert_eq!(worst_residual, Some(f64::INFINITY));
        }
        other => panic!("expected a refused answer with no direction, got {other:?}"),
    }
}

#[test]
fn a_drag_keeps_the_angle_while_its_end_follows_the_pointer() {
    if !native() {
        return;
    }
    // The drag path holds one native system and nudges a pin. The angle is a
    // constraint of that system like any other, so the dragged vector keeps
    // its turn while its end follows the pointer.
    let sketch = with_angle(two_segments(-40.0), A, B, deg(60.0));
    let mut drag = solver::Drag::begin(&sketch, P3).expect("a draggable system");
    for target in [(25.0, 12.0), (28.0, 20.0), (15.0, 30.0)] {
        let outcome = drag.move_to(target.0, target.1).expect("a solved sample");
        let Outcome::Solved(solution) = outcome else {
            panic!("a drag sample did not solve: {outcome:?}");
        };
        assert!(
            same_turn(measured(&solution, A, B), 60.0),
            "target {target:?}"
        );
    }
}
