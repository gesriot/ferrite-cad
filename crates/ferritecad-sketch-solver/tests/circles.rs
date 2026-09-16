// SPDX-License-Identifier: MIT
//! What the solver makes of a circle, measured against the real library.
//!
//! A circle is three unknowns — a centre that is an ordinary point of the
//! sketch, and a radius that is a scalar of its own — and every number here is
//! read back from planegcs rather than written down in advance. The degrees of
//! freedom in particular: a gate that asserted a remembered 3, 2, 0 would pass
//! against a build that had stopped declaring the radius at all.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_sketch_solver as solver;
use ferritecad_sketch_solver::{
    Circle, CircleId, Constraint, ConstraintId, NotFinite, PointId, Position, Sketch, SolverError,
};

/// Sparse and non-sequential on purpose: nothing may depend on an identifier
/// equalling its own storage position.
const CENTER: PointId = PointId(11);
const SECOND_CENTER: PointId = PointId(4_002);
const HOLE: CircleId = CircleId(77);
const SECOND: CircleId = CircleId(5);
const RADIUS: ConstraintId = ConstraintId(31);
const PIN: ConstraintId = ConstraintId(999);

fn free_circle() -> Sketch {
    let mut sketch = Sketch::new();
    sketch.add_point(CENTER, 12.0, -7.0);
    sketch.add_circle(HOLE, CENTER, 10.0);
    sketch
}

/// Whether there is a library to measure, saying so when there is not.
fn native() -> bool {
    if solver::is_available() {
        true
    } else {
        assert!(
            !solver::is_required(),
            "FERRITECAD_REQUIRE_PLANEGCS=1 and no library: this gate may not skip"
        );
        eprintln!("skipped: this build has no planegcs");
        false
    }
}

#[test]
fn a_free_circle_has_three_degrees_of_freedom_and_each_constraint_removes_its_own() {
    if !native() {
        return;
    }
    // Read from the library every time, never from a remembered number: a
    // build that stopped declaring the radius an unknown would still answer 2
    // for the radius case if this compared against a constant written here.
    let dofs = |sketch: &Sketch| {
        solver::diagnose(sketch)
            .expect("a diagnosable system")
            .degrees_of_freedom()
    };

    let free = free_circle();
    assert_eq!(dofs(&free), 3, "a centre is two and a radius is one");

    let mut sized = free_circle();
    sized.add_constraint(
        RADIUS,
        Constraint::Radius {
            circle: HOLE,
            radius: 6.75,
        },
    );
    assert_eq!(dofs(&sized), 2, "a radius removes exactly its own unknown");

    let mut pinned = free_circle();
    pinned.add_constraint(
        PIN,
        Constraint::Fixed {
            point: CENTER,
            x: -3.5,
            y: 4.25,
        },
    );
    assert_eq!(dofs(&pinned), 1, "pinning the centre removes two");

    let mut both = free_circle();
    both.add_constraint(
        RADIUS,
        Constraint::Radius {
            circle: HOLE,
            radius: 6.75,
        },
    )
    .add_constraint(
        PIN,
        Constraint::Fixed {
            point: CENTER,
            x: -3.5,
            y: 4.25,
        },
    );
    assert_eq!(dofs(&both), 0, "a sized, pinned circle cannot move");

    // And the freedom comes back when a constraint is taken away, measured
    // again rather than assumed to be the number it was before.
    assert_eq!(dofs(&sized), 2);
    assert_eq!(dofs(&pinned), 1);
    assert_eq!(dofs(&free), 3);
}

#[test]
fn a_solved_circle_carries_the_radius_and_centre_it_was_asked_for() {
    if !native() {
        return;
    }
    let before = solver::native_solves();
    let mut sketch = free_circle();
    sketch
        .add_constraint(
            RADIUS,
            Constraint::Radius {
                circle: HOLE,
                radius: 6.75,
            },
        )
        .add_constraint(
            PIN,
            Constraint::Fixed {
                point: CENTER,
                x: -3.5,
                y: 4.25,
            },
        );

    let outcome = solver::solve(&sketch).expect("a solvable circle");
    let solution = outcome.solution().expect("solved");
    assert_eq!(solution.degrees_of_freedom(), 0);
    assert!(solution.redundant().is_empty());
    assert!(solution.worst_residual() <= solver::RESIDUAL_LIMIT);

    let circle = solution.circle(HOLE).expect("the circle it was given");
    assert_eq!(circle.center, CENTER);
    assert!((circle.radius - 6.75).abs() <= 1e-9, "{}", circle.radius);
    // The centre is the same answer read the other way, not a second one.
    let at = solution.position(CENTER).expect("the centre is a point");
    assert!((at.x + 3.5).abs() <= 1e-9 && (at.y - 4.25).abs() <= 1e-9);
    assert_eq!(solution.circles().len(), 1);

    assert!(
        solver::native_solves() > before,
        "the answer did not cross into planegcs"
    );
    assert_eq!(solver::native_live_sessions(), 0, "a session leaked");
}

#[test]
fn two_circles_answer_for_themselves_whichever_order_they_were_added_in() {
    if !native() {
        return;
    }
    // Two independent circles, stated both ways round. An answer that came
    // back right only because the constrained one happened to be stored first
    // would pass one of these and fail the other.
    for swapped in [false, true] {
        let mut sketch = Sketch::new();
        let add = |sketch: &mut Sketch, which: bool| {
            if which {
                sketch.add_point(CENTER, 12.0, -7.0);
                sketch.add_circle(HOLE, CENTER, 10.0);
            } else {
                sketch.add_point(SECOND_CENTER, 40.0, 40.0);
                sketch.add_circle(SECOND, SECOND_CENTER, 2.0);
            }
        };
        add(&mut sketch, swapped);
        add(&mut sketch, !swapped);
        // Only one of the two is sized, and only it may move.
        sketch.add_constraint(
            RADIUS,
            Constraint::Radius {
                circle: SECOND,
                radius: 8.125,
            },
        );

        assert_eq!(
            solver::diagnose(&sketch)
                .expect("diagnosable")
                .degrees_of_freedom(),
            5,
            "six unknowns less one radius (swapped={swapped})"
        );
        let outcome = solver::solve(&sketch).expect("solvable");
        let solution = outcome.solution().expect("solved");
        let sized = solution.circle(SECOND).expect("the sized circle");
        assert!(
            (sized.radius - 8.125).abs() <= 1e-9,
            "swapped={swapped}: {}",
            sized.radius
        );
        let untouched = solution.circle(HOLE).expect("the free circle");
        assert!(
            (untouched.radius - 10.0).abs() <= 1e-9,
            "swapped={swapped}: an unconstrained radius moved to {}",
            untouched.radius
        );
        let at = solution.position(CENTER).expect("its centre");
        assert!(
            (at.x - 12.0).abs() <= 1e-9 && (at.y + 7.0).abs() <= 1e-9,
            "swapped={swapped}: an unconstrained centre moved to {at:?}"
        );
    }
}

#[test]
fn two_different_radii_on_one_circle_conflict_and_two_equal_ones_are_redundant() {
    if !native() {
        return;
    }
    let other = ConstraintId(32);

    let mut impossible = free_circle();
    impossible
        .add_constraint(
            RADIUS,
            Constraint::Radius {
                circle: HOLE,
                radius: 6.75,
            },
        )
        .add_constraint(
            other,
            Constraint::Radius {
                circle: HOLE,
                radius: 8.125,
            },
        );
    let outcome = solver::solve(&impossible).expect("a diagnosable system");
    let solver::Outcome::Conflicting { constraints, .. } = &outcome else {
        panic!("two different radii on one circle are not both satisfiable: {outcome:?}")
    };
    assert!(
        constraints.contains(&RADIUS) || constraints.contains(&other),
        "the conflict names neither radius: {constraints:?}"
    );
    // Whatever it blamed, it blamed the caller's own numbering and nothing
    // else: a native tag leaking through would not be one of these two.
    for blamed in constraints {
        assert!(
            *blamed == RADIUS || *blamed == other,
            "{blamed:?} is not a constraint this sketch stated"
        );
    }

    let mut repeated = free_circle();
    repeated
        .add_constraint(
            RADIUS,
            Constraint::Radius {
                circle: HOLE,
                radius: 6.75,
            },
        )
        .add_constraint(
            other,
            Constraint::Radius {
                circle: HOLE,
                radius: 6.75,
            },
        );
    let outcome = solver::solve(&repeated).expect("a solvable system");
    let solution = outcome
        .solution()
        .expect("saying one thing twice still solves");
    assert!(
        !solution.redundant().is_empty(),
        "a repeated radius is redundant, not invisible"
    );
    for blamed in solution.redundant() {
        assert!(*blamed == RADIUS || *blamed == other, "{blamed:?}");
    }
    assert!((solution.circle(HOLE).expect("circle").radius - 6.75).abs() <= 1e-9);
}

/// Everything the boundary refuses about a circle, in every build.
///
/// None of this reaches planegcs: a reference that names nothing, a radius
/// that is not a length and an identifier used twice are this contract's to
/// refuse, and sending them across a C boundary to be noticed there would make
/// the check a property of the library.
#[test]
fn a_circle_the_contract_cannot_state_is_refused_before_any_library_is_asked() {
    let elsewhere = PointId(123);
    let unknown = CircleId(4);

    let mut no_center = Sketch::new();
    no_center.add_circle(HOLE, elsewhere, 10.0);
    assert!(matches!(
        solver::diagnose(&no_center),
        Err(SolverError::UnknownCenter { circle, point })
            if circle == HOLE && point == elsewhere
    ));

    let mut twice = free_circle();
    twice.add_circle(HOLE, CENTER, 4.0);
    assert!(matches!(
        solver::diagnose(&twice),
        Err(SolverError::DuplicateCircle(HOLE))
    ));

    let mut names_nothing = free_circle();
    names_nothing.add_constraint(
        RADIUS,
        Constraint::Radius {
            circle: unknown,
            radius: 5.0,
        },
    );
    assert!(matches!(
        solver::diagnose(&names_nothing),
        Err(SolverError::UnknownCircle { constraint, circle })
            if constraint == RADIUS && circle == unknown
    ));

    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut sketch = Sketch::new();
        sketch.add_point(CENTER, 0.0, 0.0);
        sketch.add_circle(HOLE, CENTER, bad);
        assert!(
            matches!(
                solver::diagnose(&sketch),
                Err(SolverError::NotFinite(NotFinite::CircleRadius(HOLE)))
            ),
            "a stored radius of {bad} was accepted"
        );

        let mut asked = free_circle();
        asked.add_constraint(
            RADIUS,
            Constraint::Radius {
                circle: HOLE,
                radius: bad,
            },
        );
        assert!(
            matches!(
                solver::diagnose(&asked),
                Err(SolverError::NotFinite(NotFinite::ConstraintParameter(
                    RADIUS
                )))
            ),
            "a requested radius of {bad} was accepted"
        );
    }

    // A starting state still has to be one position per point, and a circle
    // does not add one of its own beyond its centre.
    let sketch = free_circle();
    assert!(matches!(
        solver::solve_from(&sketch, &[]),
        Err(SolverError::StateSize {
            expected: 1,
            actual: 0
        })
    ));
    let start = [Position::new(CENTER, 1.0, 2.0)];
    // Everything above is this contract's own refusal and runs in every build.
    // What is left needs a library, and asking for it is not this gate
    // skipping: `is_available` rather than `native`, so no build reports a skip
    // for a check that already did its work.
    if solver::is_available() {
        let outcome = solver::solve_from(&sketch, &start).expect("solvable");
        let solution = outcome.solution().expect("no constraints still solves");
        assert_eq!(solution.degrees_of_freedom(), 3);
        // Started from the state the caller supplied, radius from the sketch.
        assert_eq!(
            solution.circle(HOLE).map(|c| c.radius),
            Some(10.0),
            "the radius guess is the sketch's"
        );
    }
}

/// The `Circle` value a caller builds is the one the contract stores.
#[test]
fn a_circle_keeps_the_identity_and_centre_it_was_given() {
    let circle = Circle::new(HOLE, CENTER, 10.0);
    assert_eq!(circle.circle, HOLE);
    assert_eq!(circle.center, CENTER);
    let sketch = free_circle();
    assert_eq!(sketch.circles(), [circle]);
    assert_eq!(
        sketch.points().len(),
        1,
        "a circle adds no point of its own"
    );
}
