// SPDX-License-Identifier: MIT
//! What the public boundary promises whether or not there is a solver behind
//! it.
//!
//! Every gate here runs in an ordinary build of this workspace — no feature,
//! no library, nothing fetched. That is deliberate: the refusals a caller
//! depends on must be this crate's own behaviour, not something planegcs
//! happens to do when it is present. A check that only runs on the machine
//! that built the library is a check that is not protecting the other two.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_sketch_solver as solver;
use ferritecad_sketch_solver::{
    Constraint, ConstraintId, NotFinite, PointId, Position, Sketch, SolverError,
};

const A: PointId = PointId(7);
const B: PointId = PointId(41);
const C: PointId = PointId(900);

/// Three points and two constraints, valid, with sparse identifiers.
fn valid() -> Sketch {
    let mut sketch = Sketch::new();
    sketch
        .add_point(A, 0.0, 0.0)
        .add_point(B, 30.0, 1.0)
        .add_point(C, 15.0, 20.0);
    sketch
        .add_constraint(
            ConstraintId(5),
            Constraint::Fixed {
                point: A,
                x: 0.0,
                y: 0.0,
            },
        )
        .add_constraint(
            ConstraintId(900_001),
            Constraint::Distance {
                a: A,
                b: B,
                distance: 30.0,
            },
        );
    sketch
}

#[test]
fn a_build_without_a_solver_answers_unavailable_rather_than_skipping() {
    // Meaningful in both configurations, so it can never quietly become a
    // test that ran and checked nothing. With a library it asserts there
    // really is one; without, it asserts the refusal is the typed one.
    let sketch = valid();
    match solver::solve(&sketch) {
        Ok(_) => assert!(
            solver::is_available(),
            "a sketch was solved by a build that says it has no solver"
        ),
        Err(error) => {
            assert!(
                !solver::is_available(),
                "an available solver refused a valid sketch: {error}"
            );
            assert_eq!(
                error,
                SolverError::Unavailable(solver::Unavailable::NotLinked),
                "an absent solver must be a typed Unavailable and nothing else"
            );
            assert!(error.is_unavailable());
        }
    }
    // The same answer from every entry point, so no route into the crate can
    // be the one that quietly does something else.
    assert_eq!(
        solver::diagnose(&sketch).is_err(),
        !solver::is_available(),
        "diagnose and solve must agree about whether there is a solver"
    );
    assert_eq!(
        solver::Drag::begin(&sketch, A).is_err(),
        !solver::is_available(),
        "a gesture and a solve must agree about whether there is a solver"
    );
}

#[test]
fn a_constraint_naming_a_point_the_sketch_does_not_have_is_refused() {
    let mut sketch = valid();
    sketch.add_constraint(
        ConstraintId(12),
        Constraint::Horizontal {
            a: A,
            b: PointId(4242),
        },
    );
    assert_eq!(
        solver::solve(&sketch).expect_err("the boundary must refuse this"),
        SolverError::UnknownPoint {
            constraint: ConstraintId(12),
            point: PointId(4242),
        },
        "a dangling reference must be named, not passed to the solver"
    );
}

#[test]
fn every_reference_of_a_multi_point_constraint_is_checked() {
    // The fourth reference of a four-reference constraint is the one a check
    // written for two points would miss.
    let missing = PointId(4242);
    for (slot, constraint) in [
        (
            0,
            Constraint::Parallel {
                a: (missing, B),
                b: (A, C),
            },
        ),
        (
            1,
            Constraint::Parallel {
                a: (A, missing),
                b: (B, C),
            },
        ),
        (
            2,
            Constraint::Parallel {
                a: (A, B),
                b: (missing, C),
            },
        ),
        (
            3,
            Constraint::Parallel {
                a: (A, B),
                b: (C, missing),
            },
        ),
    ] {
        let mut sketch = valid();
        sketch.add_constraint(ConstraintId(20), constraint);
        assert_eq!(
            solver::solve(&sketch).expect_err("the boundary must refuse this"),
            SolverError::UnknownPoint {
                constraint: ConstraintId(20),
                point: missing,
            },
            "reference {slot} of a four-point constraint was not checked"
        );
    }
}

#[test]
fn one_identifier_cannot_name_two_things() {
    let mut points = valid();
    points.add_point(B, 1.0, 1.0);
    assert_eq!(
        solver::solve(&points).expect_err("the boundary must refuse this"),
        SolverError::DuplicatePoint(B)
    );

    // Constraints matter more than points here: a conflict is reported by
    // identifier, so two constraints sharing one would make the answer
    // ambiguous exactly when somebody needs it.
    let mut constraints = valid();
    constraints.add_constraint(ConstraintId(5), Constraint::Horizontal { a: A, b: B });
    assert_eq!(
        solver::solve(&constraints).expect_err("the boundary must refuse this"),
        SolverError::DuplicateConstraint(ConstraintId(5))
    );
}

#[test]
fn a_coordinate_or_a_dimension_that_is_not_a_number_is_refused() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut sketch = Sketch::new();
        sketch.add_point(A, bad, 0.0).add_point(B, 1.0, 1.0);
        assert_eq!(
            solver::solve(&sketch).expect_err("the boundary must refuse this"),
            SolverError::NotFinite(NotFinite::PointCoordinate(A)),
            "a point at {bad} reached the solver"
        );

        let mut dimensioned = valid();
        dimensioned.add_constraint(
            ConstraintId(31),
            Constraint::Distance {
                a: A,
                b: B,
                distance: bad,
            },
        );
        assert_eq!(
            solver::solve(&dimensioned).expect_err("the boundary must refuse this"),
            SolverError::NotFinite(NotFinite::ConstraintParameter(ConstraintId(31))),
            "a distance of {bad} reached the solver"
        );

        let mut pinned = valid();
        pinned.add_constraint(
            ConstraintId(32),
            Constraint::Fixed {
                point: B,
                x: 1.0,
                y: bad,
            },
        );
        assert_eq!(
            solver::solve(&pinned).expect_err("the boundary must refuse this"),
            SolverError::NotFinite(NotFinite::ConstraintParameter(ConstraintId(32))),
            "a pin at {bad} reached the solver"
        );
    }
}

#[test]
fn a_starting_state_of_the_wrong_shape_is_refused() {
    let sketch = valid();
    let full: Vec<Position> = sketch.points().to_vec();

    assert_eq!(
        solver::solve_from(&sketch, &full[..2]).expect_err("the boundary must refuse this"),
        SolverError::StateSize {
            expected: 3,
            actual: 2
        }
    );

    let mut too_many = full.clone();
    too_many.push(Position::new(PointId(1), 0.0, 0.0));
    assert_eq!(
        solver::solve_from(&sketch, &too_many).expect_err("the boundary must refuse this"),
        SolverError::StateSize {
            expected: 3,
            actual: 4
        }
    );

    // Right length, wrong point: a state naming something the sketch does not
    // have would otherwise be written into whichever slot happened to be free.
    let mut stranger = full.clone();
    stranger[1] = Position::new(PointId(4242), 1.0, 1.0);
    assert_eq!(
        solver::solve_from(&sketch, &stranger).expect_err("the boundary must refuse this"),
        SolverError::UnknownPointInState(PointId(4242))
    );

    // Right length, one point named twice, so another is left unset.
    let mut twice = full;
    twice[1] = Position::new(A, 1.0, 1.0);
    assert_eq!(
        solver::solve_from(&sketch, &twice).expect_err("the boundary must refuse this"),
        SolverError::DuplicatePoint(A)
    );
}

#[test]
fn a_sketch_is_refused_before_the_solver_is_consulted() {
    // The ordering that makes every gate above a statement about this crate
    // rather than about planegcs: an invalid sketch is refused for being
    // invalid, in a build with no solver to refuse it for any other reason.
    let mut sketch = valid();
    sketch.add_constraint(
        ConstraintId(12),
        Constraint::Vertical {
            a: A,
            b: PointId(4242),
        },
    );
    let error = solver::solve(&sketch).expect_err("the boundary must refuse this");
    assert!(
        !error.is_unavailable(),
        "an invalid sketch was reported as a missing solver: {error}"
    );
}

#[test]
fn a_non_finite_drag_target_never_reaches_the_solver() {
    let sketch = valid();
    let Ok(mut gesture) = solver::Drag::begin(&sketch, B) else {
        // Without a library there is no gesture to move; the refusal to begin
        // one is checked above.
        return;
    };
    for bad in [f64::NAN, f64::INFINITY] {
        assert_eq!(
            gesture
                .move_to(bad, 0.0)
                .expect_err("the boundary must refuse this"),
            SolverError::NotFinite(NotFinite::PointCoordinate(B))
        );
        assert_eq!(
            gesture
                .move_to(0.0, bad)
                .expect_err("the boundary must refuse this"),
            SolverError::NotFinite(NotFinite::PointCoordinate(B))
        );
    }
}

#[test]
fn nothing_a_caller_can_read_names_the_native_side() {
    // Not a style rule. A native pointer, tag or equation index printed into a
    // log is something somebody will read a meaning into, and none of them
    // means anything outside one particular build of planegcs.
    let sketch = valid();
    let rendered = vec![
        format!("{:?}", solver::solve(&sketch)),
        format!("{:?}", solver::diagnose(&sketch)),
        format!("{:?}", solver::Drag::begin(&sketch, A)),
        format!("{}", SolverError::Native(solver::NativeFailure::Refused)),
        format!(
            "{}",
            SolverError::Unavailable(solver::Unavailable::NotLinked)
        ),
        format!("{sketch:?}"),
    ];

    for text in rendered {
        assert!(!text.contains("0x"), "something printed an address: {text}");
        for leak in ["raw:", "tag", "ordinal", "libplanegcs", "planegcs.dll"] {
            assert!(
                !text.contains(leak),
                "the word {leak:?} reached a caller-visible rendering: {text}"
            );
        }
    }
}

#[test]
fn the_residual_limit_is_the_one_the_comparison_was_made_against() {
    // The lab's neutral gate and this crate's acceptance limit are one number.
    // Two would drift, and the one that drifted would be the one nobody was
    // looking at.
    assert_eq!(solver::RESIDUAL_LIMIT, 1e-6);
}

/// Four points: `A→B` along +X, `C→D` up, plus a coincident `E` for a vector
/// with no length.
fn angled(angle: f64, a: (PointId, PointId), b: (PointId, PointId)) -> Sketch {
    let mut sketch = Sketch::new();
    sketch
        .add_point(PointId(1), 0.0, 0.0)
        .add_point(PointId(2), 10.0, 0.0)
        .add_point(PointId(3), 4.0, 4.0)
        .add_point(PointId(4), 4.0, 9.0)
        .add_point(PointId(5), 0.0, 0.0);
    sketch.add_constraint(ConstraintId(8), Constraint::DirectedAngle { a, b, angle });
    sketch
}

#[test]
fn a_directed_angle_outside_the_canonical_range_is_refused_not_wrapped() {
    use std::f64::consts::PI;
    let (a, b) = ((PointId(1), PointId(2)), (PointId(3), PointId(4)));
    for bad in [
        -PI,
        -PI - 1e-12,
        PI + 1e-12,
        270f64.to_radians(),
        2.0 * PI,
        -4.0,
    ] {
        assert_eq!(
            solver::solve(&angled(bad, a, b)).expect_err("outside (-pi, pi]"),
            SolverError::AngleOutOfRange(ConstraintId(8)),
            "{bad} rad"
        );
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            solver::diagnose(&angled(bad, a, b)).expect_err("not a number"),
            SolverError::NotFinite(NotFinite::ConstraintParameter(ConstraintId(8)))
        );
    }
    // The canonical bounds are not an out-of-range error; with no library the
    // next answer is the typed Unavailable, and with one a diagnosis.
    for fine in [0.0, -0.0, PI, -PI + 1e-9, 1.0] {
        match solver::diagnose(&angled(fine, a, b)) {
            Ok(_) => assert!(solver::is_available()),
            Err(error) => assert!(error.is_unavailable(), "{fine} rad: {error}"),
        }
    }
    println!("\nFCAD_31F_SOLVER_RANGE_EXECUTED");
}

#[test]
fn a_directed_angle_against_a_vector_with_no_direction_is_refused_before_native() {
    let degenerate = |sketch: Sketch| {
        assert_eq!(
            solver::solve(&sketch).expect_err("a vector with no direction"),
            SolverError::DegenerateVector(ConstraintId(8))
        );
    };
    let line = (PointId(1), PointId(2));
    let up = (PointId(3), PointId(4));
    // One point twice, in either vector.
    degenerate(angled(0.5, (PointId(1), PointId(1)), up));
    degenerate(angled(0.5, line, (PointId(4), PointId(4))));
    // Two points that sit on each other.
    degenerate(angled(0.5, (PointId(1), PointId(5)), up));
    degenerate(angled(0.5, line, (PointId(5), PointId(1))));
    println!("\nFCAD_31F_SOLVER_DEGENERATE_EXECUTED");
}

#[test]
fn every_point_of_a_directed_angle_is_checked_against_the_sketch() {
    let missing = PointId(4242);
    for (slot, a, b) in [
        (0, (missing, PointId(2)), (PointId(3), PointId(4))),
        (1, (PointId(1), missing), (PointId(3), PointId(4))),
        (2, (PointId(1), PointId(2)), (missing, PointId(4))),
        (3, (PointId(1), PointId(2)), (PointId(3), missing)),
    ] {
        assert_eq!(
            solver::solve(&angled(0.5, a, b)).expect_err("a dangling reference"),
            SolverError::UnknownPoint {
                constraint: ConstraintId(8),
                point: missing,
            },
            "reference {slot}"
        );
    }
    println!("\nFCAD_31F_SOLVER_REFERENCES_EXECUTED");
}
