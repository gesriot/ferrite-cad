// SPDX-License-Identifier: MIT
//! What the product sketch solver does when there is a real planegcs behind
//! it.
//!
//! These are the gates the pin workflow runs with `FERRITECAD_REQUIRE_PLANEGCS=1`
//! on all three platforms. Under that variable nothing here may skip: a run
//! whose job is to prove the product solver works cannot pass by not having
//! one.
//!
//! The counters these gates lean on are not decoration. Two of the claims made
//! here are invisible in the geometry — that an answer credited to planegcs
//! came from planegcs rather than from arithmetic of our own, and that a
//! gesture built one native system rather than fifty — and a substituted
//! solver returns the same coordinates either way.

// Nothing here means anything without the feature that can link a library:
// `expected_provenance` is compiled from the pin by the build script, and what
// a library must answer is not a question a build that cannot load one has.
// The gates that hold in every build are in `boundary.rs`.
#![cfg(feature = "planegcs")]
// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_sketch_solver as solver;
use ferritecad_sketch_solver::{Constraint, ConstraintId, Outcome, PointId, Sketch};

/// Leaves the caller when this build has no planegcs, and refuses to under
/// required mode.
///
/// One gate for every site that asks, because the answer is not simply "is it
/// linked". The printed sentence is what the workflow greps for.
fn ready() -> bool {
    if solver::is_available() {
        return true;
    }
    assert!(
        !solver::is_required(),
        "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so no gate may skip: this build has no product \
         sketch solver, and a solver that is not there has not been shown to work"
    );
    eprintln!("skipped: this build has no sketch solver");
    false
}

macro_rules! solver_or_skip {
    () => {
        if !ready() {
            return;
        }
    };
}

// Deliberately sparse and out of order. An identifier that happened to equal
// its own storage position would hide every mistake in the direction that
// matters most: what a conflict is reported in.
const P0: PointId = PointId(900);
const P1: PointId = PointId(7);
const P2: PointId = PointId(65_536);
const P3: PointId = PointId(41);

const PIN: ConstraintId = ConstraintId(1_000_003);
const BOTTOM: ConstraintId = ConstraintId(2);
const TOP: ConstraintId = ConstraintId(77);
const LEFT: ConstraintId = ConstraintId(4_000);
const RIGHT: ConstraintId = ConstraintId(31);
const WIDTH: ConstraintId = ConstraintId(500_000);
const HEIGHT: ConstraintId = ConstraintId(9);

/// A rectangle, corners anticlockwise from the origin, started out of place so
/// the solver has something to do.
fn rectangle(width: f64, height: f64) -> Sketch {
    let mut sketch = Sketch::new();
    sketch
        .add_point(P0, 0.3, -0.2)
        .add_point(P1, width - 0.4, 0.1)
        .add_point(P2, width + 0.2, height + 0.3)
        .add_point(P3, -0.1, height - 0.2);
    sketch
        .add_constraint(
            PIN,
            Constraint::Fixed {
                point: P0,
                x: 0.0,
                y: 0.0,
            },
        )
        .add_constraint(BOTTOM, Constraint::Horizontal { a: P0, b: P1 })
        .add_constraint(TOP, Constraint::Horizontal { a: P3, b: P2 })
        .add_constraint(LEFT, Constraint::Vertical { a: P0, b: P3 })
        .add_constraint(RIGHT, Constraint::Vertical { a: P1, b: P2 })
        .add_constraint(
            WIDTH,
            Constraint::Distance {
                a: P0,
                b: P1,
                distance: width,
            },
        )
        .add_constraint(
            HEIGHT,
            Constraint::Distance {
                a: P0,
                b: P3,
                distance: height,
            },
        );
    sketch
}

/// The same rectangle without its pin, so it can still slide.
fn unpinned(width: f64, height: f64) -> Sketch {
    let full = rectangle(width, height);
    let mut sketch = Sketch::new();
    for position in full.points() {
        sketch.add_point(position.point, position.x, position.y);
    }
    for &(id, constraint) in full.constraints() {
        if id != PIN {
            sketch.add_constraint(id, constraint);
        }
    }
    sketch
}

fn solved(sketch: &Sketch) -> solver::Solution {
    match solver::solve(sketch).expect("an available solver answers") {
        Outcome::Solved(solution) => solution,
        other => panic!("expected a solved sketch, got {other:?}"),
    }
}

fn at(solution: &solver::Solution, point: PointId) -> (f64, f64) {
    let position = solution
        .position(point)
        .expect("a solved sketch reports every one of its points");
    (position.x, position.y)
}

#[test]
fn a_required_build_opens_the_product_solver_and_the_pinned_library_answers() {
    solver_or_skip!();
    let provenance = solver::provenance().expect("an available solver identifies its library");
    assert_eq!(
        provenance,
        solver::expected_provenance(),
        "the library that was loaded is not the pinned one"
    );
    assert!(
        provenance.contains("planegcs from FreeCAD 1.0.1"),
        "the pin no longer names the release the decision was made on: {provenance}"
    );
    eprintln!("product sketch solver: {provenance}");
}

#[test]
fn the_product_path_asks_planegcs_rather_than_answering_for_it() {
    solver_or_skip!();
    // Every other gate compares numbers, and a path that quietly solved the
    // sketch with arithmetic of its own would clear all of them: same
    // coordinates, same diagnosis, same refusals, and a product built on
    // nothing. The counter is per thread, so this is a count of this test's
    // own crossings.
    let sketch = rectangle(60.0, 40.0);
    let before = solver::native_solves();
    let solution = solved(&sketch);
    assert_eq!(
        solver::native_solves(),
        before + 1,
        "the product solver returned an answer without asking planegcs"
    );
    assert!(solution.worst_residual() <= solver::RESIDUAL_LIMIT);
}

#[test]
fn a_solved_rectangle_is_actually_a_rectangle() {
    solver_or_skip!();
    // Residuals near zero is what the solver claims. This checks the geometry
    // it produced, which is what the claim is supposed to mean.
    let solution = solved(&rectangle(60.0, 40.0));
    let (x0, y0) = at(&solution, P0);
    let (x1, y1) = at(&solution, P1);
    let (x2, y2) = at(&solution, P2);
    let (x3, y3) = at(&solution, P3);

    assert!(
        x0.abs() < 1e-6 && y0.abs() < 1e-6,
        "the pinned corner moved"
    );
    assert!((y1 - y0).abs() < 1e-6, "the bottom is not horizontal");
    assert!((x3 - x0).abs() < 1e-6, "the left side is not vertical");
    assert!(((x1 - x0).abs() - 60.0).abs() < 1e-6, "the width is wrong");
    assert!(((y3 - y0).abs() - 40.0).abs() < 1e-6, "the height is wrong");
    assert!(
        (x2 - x1).abs() < 1e-6 && (y2 - y3).abs() < 1e-6,
        "the far corner is loose"
    );
    assert_eq!(solution.degrees_of_freedom(), 0);
    assert!(solution.redundant().is_empty());
}

#[test]
fn every_one_of_the_eight_constraint_types_crosses_the_product_boundary() {
    solver_or_skip!();
    // One sketch per type, each with that type doing the work, and each
    // checked by the geometry it is supposed to produce rather than by the
    // solver's own opinion of itself. A type that was silently dropped on the
    // way to planegcs would leave a sketch that still "solved".
    let a = PointId(3);
    let b = PointId(500);
    let c = PointId(12);
    let d = PointId(88);
    let base = |sketch: &mut Sketch| {
        sketch
            .add_point(a, 0.1, 0.2)
            .add_point(b, 9.7, 0.4)
            .add_point(c, 0.3, 9.6)
            .add_point(d, 10.2, 10.4);
        sketch.add_constraint(
            ConstraintId(1),
            Constraint::Fixed {
                point: a,
                x: 0.0,
                y: 0.0,
            },
        );
    };
    let id = ConstraintId(4_242);

    /// One constraint type, and what the geometry must look like once it
    /// has been satisfied.
    type Case = (
        &'static str,
        Constraint,
        Box<dyn Fn(&solver::Solution) -> bool>,
    );

    let cases: Vec<Case> = vec![
        (
            "Coincident",
            Constraint::Coincident { a: b, b: c },
            Box::new(move |s| {
                let (bx, by) = at(s, b);
                let (cx, cy) = at(s, c);
                (bx - cx).abs() < 1e-6 && (by - cy).abs() < 1e-6
            }),
        ),
        (
            "Fixed",
            Constraint::Fixed {
                point: b,
                x: 3.0,
                y: 4.0,
            },
            Box::new(move |s| {
                let (bx, by) = at(s, b);
                (bx - 3.0).abs() < 1e-6 && (by - 4.0).abs() < 1e-6
            }),
        ),
        (
            "Distance",
            Constraint::Distance {
                a,
                b,
                distance: 25.0,
            },
            Box::new(move |s| {
                let (bx, by) = at(s, b);
                ((bx * bx + by * by).sqrt() - 25.0).abs() < 1e-6
            }),
        ),
        (
            "Horizontal",
            Constraint::Horizontal { a, b },
            Box::new(move |s| (at(s, a).1 - at(s, b).1).abs() < 1e-6),
        ),
        (
            "Vertical",
            Constraint::Vertical { a, b },
            Box::new(move |s| (at(s, a).0 - at(s, b).0).abs() < 1e-6),
        ),
        (
            "EqualLength",
            Constraint::EqualLength {
                a: (a, b),
                b: (c, d),
            },
            Box::new(move |s| {
                let length = |p: PointId, q: PointId| {
                    let ((px, py), (qx, qy)) = (at(s, p), at(s, q));
                    ((px - qx).powi(2) + (py - qy).powi(2)).sqrt()
                };
                (length(a, b) - length(c, d)).abs() < 1e-6
            }),
        ),
        (
            "Perpendicular",
            Constraint::Perpendicular {
                a: (a, b),
                b: (c, d),
            },
            Box::new(move |s| {
                let (ax, ay) = at(s, a);
                let (bx, by) = at(s, b);
                let (cx, cy) = at(s, c);
                let (dx, dy) = at(s, d);
                ((bx - ax) * (dx - cx) + (by - ay) * (dy - cy)).abs() < 1e-6
            }),
        ),
        (
            "Parallel",
            Constraint::Parallel {
                a: (a, b),
                b: (c, d),
            },
            Box::new(move |s| {
                let (ax, ay) = at(s, a);
                let (bx, by) = at(s, b);
                let (cx, cy) = at(s, c);
                let (dx, dy) = at(s, d);
                ((bx - ax) * (dy - cy) - (by - ay) * (dx - cx)).abs() < 1e-6
            }),
        ),
    ];

    assert_eq!(cases.len(), 8, "the contract states eight constraint types");
    for (name, constraint, holds) in cases {
        let mut sketch = Sketch::new();
        base(&mut sketch);
        sketch.add_constraint(id, constraint);

        let before = solver::native_solves();
        let solution = solved(&sketch);
        assert_eq!(
            solver::native_solves(),
            before + 1,
            "{name} was answered without asking planegcs"
        );
        assert!(
            holds(&solution),
            "{name} crossed the boundary and the geometry does not satisfy it: {:?}",
            solution.positions()
        );
    }
}

#[test]
fn a_sketch_with_freedom_left_solves_and_says_how_much() {
    solver_or_skip!();
    let solution = solved(&unpinned(60.0, 40.0));
    // Two: the rectangle is fully shaped but can still slide in x and y.
    assert_eq!(
        solution.degrees_of_freedom(),
        2,
        "the measured degrees of freedom are not what this sketch has"
    );
    assert!(solution.is_under_constrained());
    assert!(solution.redundant().is_empty());

    // Pinned, the same sketch has none left.
    assert_eq!(solved(&rectangle(60.0, 40.0)).degrees_of_freedom(), 0);
}

#[test]
fn a_constraint_said_twice_is_redundant_and_still_solves() {
    solver_or_skip!();
    // Saying a thing twice does not make a sketch impossible, and calling it a
    // conflict would tell somebody to delete a constraint their drawing needs.
    let repeated = ConstraintId(123_456);
    let mut sketch = rectangle(60.0, 40.0);
    sketch.add_constraint(repeated, Constraint::Horizontal { a: P0, b: P1 });

    let solution = solved(&sketch);
    assert_eq!(
        solution.redundant(),
        [repeated],
        "the repeated constraint was not named as the redundant one"
    );
    assert_eq!(solution.degrees_of_freedom(), 0);
    assert!((at(&solution, P1).0 - 60.0).abs() < 1e-6);

    let diagnosis = solver::diagnose(&sketch).expect("an available solver diagnoses");
    assert_eq!(diagnosis.redundant(), [repeated]);
    assert!(
        diagnosis.conflicting().is_empty(),
        "a redundant constraint was reported as a conflict: {diagnosis:?}"
    );
}

/// Two dimensions on one edge: 60 and 70.
fn contradictory() -> (Sketch, ConstraintId) {
    let second = ConstraintId(800_001);
    let mut sketch = rectangle(60.0, 40.0);
    sketch.add_constraint(
        second,
        Constraint::Distance {
            a: P0,
            b: P1,
            distance: 70.0,
        },
    );
    (sketch, second)
}

#[test]
fn a_conflict_is_reported_in_the_callers_own_sparse_identifiers() {
    solver_or_skip!();
    let (sketch, second) = contradictory();
    let Outcome::Conflicting {
        constraints,
        redundant,
    } = solver::solve(&sketch).expect("an available solver answers")
    else {
        panic!("two different lengths on one edge is a conflict");
    };

    // Exactly the two dimensions that disagree, by the numbers the caller
    // issued. Both are far from their storage positions, which are 5 and 7.
    assert_eq!(
        constraints,
        vec![WIDTH, second],
        "the conflict named something other than the two disagreeing dimensions"
    );
    assert!(redundant.is_empty());

    // And nothing was published: a refused sketch is not a sketch half moved.
    assert!(
        solver::solve(&sketch)
            .expect("an available solver answers")
            .solution()
            .is_none(),
        "a conflicting sketch published positions"
    );
}

#[test]
fn a_native_tag_is_never_returned_as_a_storage_position() {
    solver_or_skip!();
    // The mistake this exists to catch returns 5 and 7 — where the two
    // dimensions happen to sit — instead of the numbers the caller wrote.
    let (sketch, second) = contradictory();
    let diagnosis = solver::diagnose(&sketch).expect("an available solver diagnoses");
    let blamed = diagnosis.conflicting();

    let positions: Vec<ConstraintId> = (0..sketch.constraints().len() as u64)
        .map(ConstraintId)
        .collect();
    for id in blamed {
        assert!(
            sketch.constraints().iter().any(|(known, _)| known == id),
            "{id:?} is not a constraint this sketch has"
        );
        assert!(
            !positions.contains(id),
            "{id:?} is a storage position rather than a caller's identifier"
        );
    }
    assert_eq!(blamed, [WIDTH, second]);
}

#[test]
fn caller_identifiers_survive_a_permuted_store() {
    solver_or_skip!();
    // The same sketch, its constraints added in the opposite order. Storage
    // order is this crate's business; what comes back must not depend on it.
    let (forwards, second) = contradictory();
    let mut backwards = Sketch::new();
    for position in forwards.points() {
        backwards.add_point(position.point, position.x, position.y);
    }
    for &(id, constraint) in forwards.constraints().iter().rev() {
        backwards.add_constraint(id, constraint);
    }

    let one = solver::diagnose(&forwards).expect("an available solver diagnoses");
    let other = solver::diagnose(&backwards).expect("an available solver diagnoses");
    assert_eq!(
        one.conflicting(),
        other.conflicting(),
        "reordering the store changed which constraints were blamed"
    );
    assert_eq!(one.conflicting(), [WIDTH, second]);
    assert_eq!(one.degrees_of_freedom(), other.degrees_of_freedom());
}

#[test]
fn a_sketch_that_cannot_be_satisfied_publishes_nothing() {
    solver_or_skip!();
    // The worst thing a solver can do is say yes to a drawing the geometry
    // cannot produce. A refusal costs a correction; a false success costs a
    // part. 10 + 10 < 40, so no arrangement of three points satisfies this.
    let (a, b, c) = (PointId(11), PointId(22), PointId(33));
    let mut sketch = Sketch::new();
    sketch
        .add_point(a, 0.0, 0.0)
        .add_point(b, 30.0, 1.0)
        .add_point(c, 15.0, 9.0);
    sketch
        .add_constraint(
            ConstraintId(1),
            Constraint::Fixed {
                point: a,
                x: 0.0,
                y: 0.0,
            },
        )
        .add_constraint(
            ConstraintId(2),
            Constraint::Distance {
                a,
                b,
                distance: 40.0,
            },
        )
        .add_constraint(
            ConstraintId(3),
            Constraint::Distance {
                a: b,
                b: c,
                distance: 10.0,
            },
        )
        .add_constraint(
            ConstraintId(4),
            Constraint::Distance {
                a: c,
                b: a,
                distance: 10.0,
            },
        );

    let answer = solver::solve(&sketch).expect("an available solver answers");
    assert!(
        !answer.is_solved(),
        "an impossible triangle was reported as solved: {answer:?}"
    );
    assert!(
        answer.solution().is_none(),
        "a refused sketch published positions: {answer:?}"
    );
    match answer {
        Outcome::DidNotConverge { worst_residual } => {
            // None when planegcs refused outright and left nothing to measure;
            // a number only when there was a state that misses.
            if let Some(worst) = worst_residual {
                assert!(
                    worst > solver::RESIDUAL_LIMIT,
                    "a residual inside the limit is not a failure to converge"
                );
            }
        }
        Outcome::Conflicting { .. } => {}
        // `Outcome` is non-exhaustive, so a future variant lands here rather
        // than being read as one of the two above.
        other => panic!("an impossible triangle produced {other:?}"),
    }
}

#[test]
fn a_gesture_holds_one_native_system_and_moves_the_point_it_was_given() {
    solver_or_skip!();
    let sketch = unpinned(60.0, 40.0);
    let systems_before = solver::native_sessions();

    let mut gesture = solver::Drag::begin(&sketch, P0).expect("an available solver drags");
    assert_eq!(gesture.point(), P0);

    let steps = 50;
    for step in 1..=steps {
        let target = (step as f64 * 1.5, step as f64 * 0.75);
        let Outcome::Solved(solution) = gesture
            .move_to(target.0, target.1)
            .expect("a gesture sample answers")
        else {
            panic!("the sketch came apart at drag sample {step}");
        };

        // The point really went where it was put.
        let (x, y) = at(&solution, P0);
        assert!(
            (x - target.0).abs() < 1e-6 && (y - target.1).abs() < 1e-6,
            "sample {step} did not follow the pointer: at ({x}, {y}), wanted {target:?}"
        );
        // And the rectangle is still 60 by 40 wherever it has been dragged to.
        let (x1, y1) = at(&solution, P1);
        let width = ((x1 - x).powi(2) + (y1 - y).powi(2)).sqrt();
        assert!(
            (width - 60.0).abs() < 1e-6,
            "sample {step} stretched the rectangle to {width}"
        );
        assert!(solution.worst_residual() <= solver::RESIDUAL_LIMIT);
    }

    // One system for the whole gesture. Rebuilding it every sample returns the
    // same coordinates at a different price, so nothing above would notice.
    assert_eq!(
        solver::native_sessions(),
        systems_before + 1,
        "a gesture of {steps} samples built more than one native system"
    );
}

#[test]
fn a_native_system_is_released_exactly_once() {
    solver_or_skip!();
    // A leak and a double release are both invisible in a result: the
    // coordinates are the same either way, and the second may not fault until
    // much later. The count is per thread, so this is this test's own.
    let sketch = rectangle(60.0, 40.0);
    let live_before = solver::native_live_sessions();

    {
        let mut gesture =
            solver::Drag::begin(&unpinned(60.0, 40.0), P0).expect("an available solver drags");
        assert_eq!(
            solver::native_live_sessions(),
            live_before + 1,
            "a gesture did not build a system"
        );
        gesture.move_to(1.0, 2.0).expect("a sample answers");
    }
    assert_eq!(
        solver::native_live_sessions(),
        live_before,
        "a gesture's native system outlived the gesture"
    );

    // And the same for a one-shot solve, which builds and releases one too.
    for _ in 0..5 {
        let _ = solved(&sketch);
    }
    assert_eq!(
        solver::native_live_sessions(),
        live_before,
        "solving leaked a native system"
    );
}

#[test]
fn a_semantic_summary_is_printed_for_cross_platform_comparison() {
    solver_or_skip!();
    // Meaning, not digits. Three platforms will not produce identical doubles
    // and must not be asked to: the same solve on the same source can land a
    // few ulp apart and be equally right. What has to match is what the solver
    // concluded, so that is what is printed — integers, booleans and the
    // caller's own identifiers — and no timing, because there is no recorded
    // hardware profile and a busy runner is not a slow solver.
    let mut lines = Vec::new();
    let mut say = |line: String| lines.push(format!("product {line}"));

    let full = solved(&rectangle(60.0, 40.0));
    say(format!(
        "solve sketch=rectangle solved=true dof={} redundant={} conflicting=0",
        full.degrees_of_freedom(),
        full.redundant().len()
    ));

    let loose = solved(&unpinned(60.0, 40.0));
    say(format!(
        "solve sketch=underconstrained solved=true dof={} redundant={} conflicting=0",
        loose.degrees_of_freedom(),
        loose.redundant().len()
    ));

    let repeated_id = ConstraintId(123_456);
    let mut repeated = rectangle(60.0, 40.0);
    repeated.add_constraint(repeated_id, Constraint::Horizontal { a: P0, b: P1 });
    let solution = solved(&repeated);
    say(format!(
        "solve sketch=redundant solved=true dof={} redundant={:?} conflicting=0",
        solution.degrees_of_freedom(),
        solution.redundant()
    ));

    let (sketch, _) = contradictory();
    let diagnosis = solver::diagnose(&sketch).expect("an available solver diagnoses");
    say(format!(
        "diagnose sketch=contradictory conflicting={:?} redundant={:?}",
        diagnosis.conflicting(),
        diagnosis.redundant()
    ));

    let mut gesture = solver::Drag::begin(&unpinned(60.0, 40.0), P0).expect("a gesture begins");
    let followed = (1..=50).all(|step| {
        let target = (step as f64 * 1.5, step as f64 * 0.75);
        matches!(gesture.move_to(target.0, target.1), Ok(Outcome::Solved(_)))
    });
    say(format!("drag samples=50 all_solved={followed}"));

    say(format!(
        "provenance={}",
        solver::provenance().expect("an available solver identifies its library")
    ));

    // The leading newline is not decoration: the harness writes "test <name>
    // ... " without one, and the first line would otherwise arrive with that
    // prefix attached and be dropped by whatever reads them.
    eprintln!("\n{}", lines.join("\n"));
}
