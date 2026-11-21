// SPDX-License-Identifier: MIT
//! The drawing's rules stay hard while the pointer supplies a least-squares target.
#![allow(
    clippy::panic,
    reason = "an unexpected solver outcome must fail the witness"
)]
use ferritecad_sketch_solver::{
    self as solver, Constraint, ConstraintId, Drag, Outcome, PointId, Sketch,
};

const A: PointId = PointId(5);
const B: PointId = PointId(17);

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(solver::is_available(), "PlaneGCS")])
}

fn point(fixed: bool) -> Sketch {
    let mut sketch = Sketch::new();
    sketch.add_point(A, 0., 0.);
    if fixed {
        sketch.add_constraint(
            ConstraintId(1),
            Constraint::Fixed {
                point: A,
                x: 0.,
                y: 0.,
            },
        );
    }
    sketch
}

fn sample(drag: &mut Drag, target: (f64, f64)) -> solver::Solution {
    let out = drag.move_to(target.0, target.1).expect("native sample");
    let Outcome::Solved(solution) = out else {
        panic!("unreachable sample: {out:?}");
    };
    let pos = solution.position(drag.point()).expect("dragged point");
    eprintln!(
        "Drag sample: freedom={} target=({},{}) position=({:.12},{:.12})",
        solution.degrees_of_freedom(),
        target.0,
        target.1,
        pos.x,
        pos.y,
    );
    solution
}

fn check(solution: &solver::Solution, id: PointId, x: f64, y: f64) {
    let at = solution.position(id).expect("named point in solution");
    assert!(
        (at.x - x).abs() <= 1e-6 && (at.y - y).abs() <= 1e-6,
        "{at:?}, expected ({x}, {y})"
    );
}

#[test]
fn a_horizontal_degree_of_freedom_follows_the_reachable_coordinate() {
    if !native() {
        return;
    }
    let mut s = point(true);
    s.add_point(B, 10., 0.)
        .add_constraint(ConstraintId(2), Constraint::Horizontal { a: A, b: B });
    let mut drag = Drag::begin(&s, B).expect("start the gesture");
    for target in [(12., 0.), (14., 3.), (8., -2.), (15., 1.), (16., 0.)] {
        let answer = sample(&mut drag, target);
        assert_eq!(answer.degrees_of_freedom(), 1);
        check(&answer, A, 0., 0.);
        check(&answer, B, target.0, 0.);
    }
}

#[test]
fn a_fixed_point_stays_at_its_position_for_every_soft_target() {
    if !native() {
        return;
    }
    let mut drag = Drag::begin(&point(true), A).expect("start even on a fixed point");
    assert!(drag.initial_conflict().is_none());
    for target in [(2., 3.), (-4., 1.)] {
        let answer = sample(&mut drag, target);
        assert_eq!(answer.degrees_of_freedom(), 0);
        assert!(answer.redundant().is_empty());
        check(&answer, A, 0., 0.);
    }
}

#[test]
fn a_point_with_two_degrees_of_freedom_reaches_each_target() {
    if !native() {
        return;
    }
    let mut drag = Drag::begin(&point(false), A).expect("start free point");
    for (x, y) in [(2., 3.), (-4., 1.)] {
        let answer = sample(&mut drag, (x, y));
        assert_eq!(answer.degrees_of_freedom(), 2);
        check(&answer, A, x, y);
    }
}

#[test]
fn a_drag_preserves_the_sketches_conflicting_and_redundant_constraint_ids() {
    if !native() {
        return;
    }
    for x in [0., 5.] {
        let mut sketch = point(true);
        sketch.add_constraint(ConstraintId(32), Constraint::Fixed { point: A, x, y: 0. });
        let initial = solver::solve(&sketch).expect("ordinary diagnosis");
        let mut drag = Drag::begin(&sketch, A).expect("drag diagnosis");
        let sample = drag.move_to(9., 2.).expect("diagnosed sample");
        match (initial, sample) {
            (Outcome::Solved(a), Outcome::Solved(b)) => {
                assert!(!a.redundant().is_empty());
                assert_eq!(a.redundant(), b.redundant());
                assert_eq!(a.degrees_of_freedom(), b.degrees_of_freedom());
                assert!(drag.initial_conflict().is_none());
            }
            (a @ Outcome::Conflicting { .. }, b) => {
                assert_eq!(a, b);
                assert_eq!(drag.initial_conflict(), Some(a));
            }
            outcomes => panic!("different diagnoses {outcomes:?}"),
        }
    }
}
