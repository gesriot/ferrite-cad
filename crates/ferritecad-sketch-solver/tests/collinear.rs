// SPDX-License-Identifier: MIT
//! Collinearity of two segments, measured against the real library.
//!
//! Every solved sketch is judged here from its solved points alone: the signed
//! distance of a point from the infinite line through two others, and the sine
//! of the angle between two segments, both written out again below and never
//! read back from the request, the residual or the solver. A residual that
//! agreed with itself would pass a solver that held only the direction, and a
//! test that measured only the cross product would pass `Parallel`.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_sketch_solver as solver;
use ferritecad_sketch_solver::{Constraint, ConstraintId, Outcome, PointId, Sketch};

/// Sparse identifiers on purpose: nothing may depend on position.
const P0: PointId = PointId(3);
const P1: PointId = PointId(91);
const P2: PointId = PointId(12);
const P3: PointId = PointId(400);
const P4: PointId = PointId(7);
const P5: PointId = PointId(8);

const COLLINEAR: ConstraintId = ConstraintId(70);
const OTHER: ConstraintId = ConstraintId(71);
const CLOSURE: ConstraintId = ConstraintId(72);

const A: (PointId, PointId) = (P0, P1);
const B: (PointId, PointId) = (P2, P3);

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

fn sketch(points: &[(PointId, f64, f64)]) -> Sketch {
    let mut sketch = Sketch::new();
    for &(id, x, y) in points {
        sketch.add_point(id, x, y);
    }
    sketch
}

fn pin(sketch: &mut Sketch, id: u64, point: PointId, at: (f64, f64)) {
    sketch.add_constraint(
        ConstraintId(id),
        Constraint::Fixed {
            point,
            x: at.0,
            y: at.1,
        },
    );
}

/// `a` pinned in place and `b` somewhere else: the two segments of a pair that
/// share nothing.
fn pair(a: ((f64, f64), (f64, f64)), b: ((f64, f64), (f64, f64))) -> Sketch {
    let mut sketch = sketch(&[
        (P0, a.0.0, a.0.1),
        (P1, a.1.0, a.1.1),
        (P2, b.0.0, b.0.1),
        (P3, b.1.0, b.1.1),
    ]);
    pin(&mut sketch, 1, P0, a.0);
    pin(&mut sketch, 2, P1, a.1);
    sketch
}

fn with(mut sketch: Sketch, id: ConstraintId, constraint: Constraint) -> Sketch {
    sketch.add_constraint(id, constraint);
    sketch
}

fn point(solution: &solver::Solution, id: PointId) -> (f64, f64) {
    let at = solution.position(id).expect("the point is in the answer");
    (at.x, at.y)
}

/// The signed distance of `p` from the infinite line through the solved
/// points of `line`, positive to its left, in the sketch's own units.
fn offset(solution: &solver::Solution, p: PointId, line: (PointId, PointId)) -> f64 {
    let (a, b, p) = (
        point(solution, line.0),
        point(solution, line.1),
        point(solution, p),
    );
    ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)) / (b.0 - a.0).hypot(b.1 - a.1)
}

/// The sine of the angle between two segments, from solved points.
fn sine(solution: &solver::Solution, a: (PointId, PointId), b: (PointId, PointId)) -> f64 {
    let (a0, a1, b0, b1) = (
        point(solution, a.0),
        point(solution, a.1),
        point(solution, b.0),
        point(solution, b.1),
    );
    let (ax, ay, bx, by) = (a1.0 - a0.0, a1.1 - a0.1, b1.0 - b0.0, b1.1 - b0.1);
    (ax * by - ay * bx) / (ax.hypot(ay) * bx.hypot(by))
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

/// Both ends of both segments on one line, and the line is the one asked for.
fn on_one_line(solution: &solver::Solution, a: (PointId, PointId), b: (PointId, PointId)) {
    for (p, line) in [(a.0, b), (a.1, b), (b.0, a), (b.1, a)] {
        let off = offset(solution, p, line);
        assert!(off.abs() < 1e-9, "{p:?} is {off} from the other's line");
    }
    assert!(sine(solution, a, b).abs() < 1e-9);
}

#[test]
fn disjoint_parallel_segments_on_different_lines_are_brought_onto_one_and_parallel_alone_is_not() {
    if !native() {
        return;
    }
    // `a` runs along +X from the origin and is pinned; `b` is the same
    // direction, 5 above it and well past its end. They share no point.
    let start = pair(((0.0, 0.0), (10.0, 0.0)), ((20.0, 5.0), (30.0, 5.0)));
    let solution = solved(&with(
        start.clone(),
        COLLINEAR,
        Constraint::Collinear { a: A, b: B },
    ));
    on_one_line(&solution, A, B);
    for p in [P2, P3] {
        assert!(
            point(&solution, p).1.abs() < 1e-9,
            "{p:?} left the line y = 0"
        );
    }
    assert_eq!(point(&solution, P0), (0.0, 0.0));
    assert_eq!(point(&solution, P1), (10.0, 0.0));
    // The segments were not made to touch, and `b` kept its length: a solution
    // that collapsed it onto the line would also be "collinear".
    assert!(point(&solution, P2).0 > 11.0, "{:?}", point(&solution, P2));
    assert!(length(&solution, B) > 5.0, "{}", length(&solution, B));

    // The negative control that one Parallel passes and Collinear must not:
    // the same pair under Parallel is parallel and 5 apart.
    let parallel = solved(&with(start, COLLINEAR, Constraint::Parallel { a: A, b: B }));
    assert!(sine(&parallel, A, B).abs() < 1e-9, "parallel holds");
    for p in [P2, P3] {
        assert!(
            (offset(&parallel, p, A) - 5.0).abs() < 1e-9,
            "Parallel moved {p:?} onto the line"
        );
    }
    println!("\nFCAD_31G_SOLVER_OFFSET_EXECUTED");
}

#[test]
fn an_inclined_pair_holds_the_same_line_whichever_way_each_segment_is_written() {
    if !native() {
        return;
    }
    // `a` is pinned along (3, 4); `b` lies beside it, turned and offset.
    let start = pair(((0.0, 0.0), (6.0, 8.0)), ((20.0, 2.0), (26.0, 11.0)));
    let reversed_a = (P1, P0);
    let reversed_b = (P3, P2);
    for (name, a, b) in [
        ("A, B", A, B),
        ("B, A", B, A),
        ("A reversed", reversed_a, B),
        ("B reversed", A, reversed_b),
        ("both reversed", reversed_a, reversed_b),
        ("B, A both reversed", reversed_b, reversed_a),
    ] {
        let solution = solved(&with(
            start.clone(),
            COLLINEAR,
            Constraint::Collinear { a, b },
        ));
        on_one_line(&solution, A, B);
        // The line is the pinned one: (3, 4) direction through the origin.
        for p in [P2, P3] {
            let (x, y) = point(&solution, p);
            assert!(
                (4.0 * x - 3.0 * y).abs() < 1e-8,
                "{name}: {p:?} = ({x}, {y})"
            );
        }
        assert!(length(&solution, B) > 3.0, "{name}: b collapsed");
    }
    println!("\nFCAD_31G_SOLVER_SPELLINGS_EXECUTED");
}

#[test]
fn a_collinear_removes_two_degrees_of_freedom_from_a_separate_pair() {
    if !native() {
        return;
    }
    let free = sketch(&[
        (P0, 0.0, 0.0),
        (P1, 10.0, 0.0),
        (P2, 20.0, 5.0),
        (P3, 30.0, 5.0),
    ]);
    let before = solver::diagnose(&free).expect("diagnosable");
    assert_eq!(before.degrees_of_freedom(), 8);
    let after = solver::diagnose(&with(
        free.clone(),
        COLLINEAR,
        Constraint::Collinear { a: A, b: B },
    ))
    .expect("diagnosable");
    // Measured, not assumed: direction and offset are two equations.
    assert_eq!(after.degrees_of_freedom(), 6, "{after:?}");
    assert!(after.conflicting().is_empty() && after.redundant().is_empty());
    // And with `a` pinned the pair keeps only what a point on a line has: the
    // two positions of `b` along it.
    let pinned = pair(((0.0, 0.0), (10.0, 0.0)), ((20.0, 5.0), (30.0, 5.0)));
    let diagnosis = solver::diagnose(&with(
        pinned,
        COLLINEAR,
        Constraint::Collinear { a: A, b: B },
    ))
    .expect("diagnosable");
    assert_eq!(diagnosis.degrees_of_freedom(), 2, "{diagnosis:?}");
    println!("\nFCAD_31G_SOLVER_DOF_EXECUTED");
}

/// P0→P1 then P2→P3 with P1 and P2 told to coincide, `b` leaving at a slope.
fn closed_pair() -> Sketch {
    with(
        sketch(&[
            (P0, 0.0, 0.0),
            (P1, 10.0, 0.0),
            (P2, 10.0, 0.0),
            (P3, 20.0, 4.0),
        ]),
        CLOSURE,
        Constraint::Coincident { a: P1, b: P2 },
    )
}

#[test]
fn a_closed_pair_keeps_the_direction_part_of_a_collinear_and_it_is_not_called_redundant() {
    if !native() {
        return;
    }
    let closed = closed_pair();
    let closure = solver::diagnose(&closed).expect("diagnosable");
    assert_eq!(closure.degrees_of_freedom(), 6);
    let before = solved(&closed);
    let tilt = sine(&before, A, B);
    assert!(tilt > 0.3, "the closure leaves the direction free: {tilt}");

    // The shared point makes one of the two equations of a Collinear true
    // already; the other is the direction, and it is needed. In every spelling
    // of which segment leads and which way each is written.
    for (name, a, b) in [
        ("a ends where b starts", A, B),
        ("b leads", B, A),
        ("a reversed", (P1, P0), B),
        ("b reversed", A, (P3, P2)),
    ] {
        let sketch = with(closed.clone(), COLLINEAR, Constraint::Collinear { a, b });
        let diagnosis = solver::diagnose(&sketch).expect("diagnosable");
        // Exactly one degree of freedom, not two and not none: the part the
        // closure supplied is not counted twice and the other part is real.
        assert_eq!(diagnosis.degrees_of_freedom(), 5, "{name}: {diagnosis:?}");
        assert!(
            diagnosis.redundant().is_empty() && diagnosis.conflicting().is_empty(),
            "{name}: a constraint with one needed equation is not redundant: {diagnosis:?}"
        );
        let solution = solved(&sketch);
        assert!(solution.redundant().is_empty(), "{name}");
        on_one_line(&solution, A, B);
        assert!(
            sine(&solution, A, B).abs() < 1e-9,
            "{name}: the direction was left at {tilt}"
        );
        assert!(length(&solution, A) > 3.0 && length(&solution, B) > 3.0);
    }
    println!("\nFCAD_31G_SOLVER_PARTIAL_EXECUTED");
}

#[test]
fn a_pinned_offset_makes_a_collinear_conflict_and_the_conflict_names_it() {
    if !native() {
        return;
    }
    // `a` pinned on y = 0 and the start of `b` pinned 5 above it.
    let mut conflicting = pair(((0.0, 0.0), (10.0, 0.0)), ((20.0, 5.0), (30.0, 5.0)));
    pin(&mut conflicting, 3, P2, (20.0, 5.0));
    conflicting.add_constraint(COLLINEAR, Constraint::Collinear { a: A, b: B });
    match solver::solve(&conflicting).expect("a diagnosable system") {
        Outcome::Conflicting { constraints, .. } => {
            assert!(constraints.contains(&COLLINEAR), "{constraints:?}");
            // Every pin that holds the contradiction is blamed, by its own id.
            assert!(
                constraints.iter().all(|id| [1, 2, 3, 70].contains(&id.0)),
                "{constraints:?}"
            );
        }
        other => panic!("a pinned offset must conflict, got {other:?}"),
    }
    // Remove the pin that contradicts it and the same pair is solved.
    let mut fine = pair(((0.0, 0.0), (10.0, 0.0)), ((20.0, 5.0), (30.0, 5.0)));
    fine.add_constraint(COLLINEAR, Constraint::Collinear { a: A, b: B });
    on_one_line(&solved(&fine), A, B);
}

#[test]
fn a_repeated_collinear_is_redundant_by_its_own_id_only_when_every_equation_is() {
    if !native() {
        return;
    }
    // The same relation said twice, the second with both segments turned and
    // exchanged: all of its equations repeat the first's.
    let twice = with(
        with(
            sketch(&[
                (P0, 0.0, 0.0),
                (P1, 10.0, 0.0),
                (P2, 20.0, 5.0),
                (P3, 30.0, 5.0),
            ]),
            COLLINEAR,
            Constraint::Collinear { a: A, b: B },
        ),
        OTHER,
        Constraint::Collinear {
            a: (P3, P2),
            b: (P1, P0),
        },
    );
    let diagnosis = solver::diagnose(&twice).expect("diagnosable");
    assert_eq!(diagnosis.redundant().len(), 1, "{diagnosis:?}");
    assert!([COLLINEAR, OTHER].contains(&diagnosis.redundant()[0]));
    assert!(diagnosis.conflicting().is_empty());
    assert_eq!(diagnosis.degrees_of_freedom(), 6, "counted once");
    let solution = solved(&twice);
    assert_eq!(solution.redundant(), diagnosis.redundant());
    on_one_line(&solution, A, B);

    // Three segments that already lie on one line: the third relation of the
    // triangle repeats the other two, and every equation of it does.
    let on_a_line = [
        (P0, 0.0, 0.0),
        (P1, 10.0, 0.0),
        (P2, 20.0, 0.0),
        (P3, 30.0, 0.0),
        (P4, 40.0, 0.0),
        (P5, 50.0, 0.0),
    ];
    let (c, d) = ((P4, P5), (P2, P3));
    let mut chain = sketch(&on_a_line);
    chain
        .add_constraint(ConstraintId(1), Constraint::Collinear { a: A, b: d })
        .add_constraint(ConstraintId(2), Constraint::Collinear { a: d, b: c })
        .add_constraint(ConstraintId(3), Constraint::Collinear { a: A, b: c });
    let diagnosis = solver::diagnose(&chain).expect("diagnosable");
    assert_eq!(diagnosis.degrees_of_freedom(), 8, "{diagnosis:?}");
    assert_eq!(diagnosis.redundant().len(), 1, "{diagnosis:?}");
    assert!((1..=3).contains(&diagnosis.redundant()[0].0));
    println!("\nFCAD_31G_SOLVER_REDUNDANT_EXECUTED");
}

#[test]
fn a_dependency_that_holds_only_on_the_solved_line_is_not_seen_at_the_start_but_is_solved() {
    if !native() {
        return;
    }
    // planegcs ranks the equations at the starting positions. Here the third
    // relation is implied only once the first two hold, not at this start, so
    // it is neither named nor counted as redundant, and the degrees of freedom
    // are the ones of six independent equations. The answer is still one line.
    let mut chain = sketch(&[
        (P0, 0.0, 0.0),
        (P1, 10.0, 0.0),
        (P2, 20.0, 3.0),
        (P3, 30.0, 4.0),
        (P4, 40.0, -2.0),
        (P5, 50.0, 6.0),
    ]);
    let (c, d) = ((P4, P5), (P2, P3));
    chain
        .add_constraint(ConstraintId(1), Constraint::Collinear { a: A, b: d })
        .add_constraint(ConstraintId(2), Constraint::Collinear { a: d, b: c })
        .add_constraint(ConstraintId(3), Constraint::Collinear { a: A, b: c });
    let diagnosis = solver::diagnose(&chain).expect("diagnosable");
    assert!(diagnosis.conflicting().is_empty(), "{diagnosis:?}");
    assert_eq!(diagnosis.degrees_of_freedom(), 6, "{diagnosis:?}");
    assert!(diagnosis.redundant().is_empty(), "{diagnosis:?}");
    let solution = solved(&chain);
    on_one_line(&solution, A, d);
    on_one_line(&solution, d, c);
    on_one_line(&solution, A, c);
}

#[test]
fn a_segment_the_solver_collapses_is_never_a_satisfied_collinear() {
    if !native() {
        return;
    }
    // A collapsed segment lies on every line, which is the one way to "satisfy"
    // a collinearity by arithmetic. `a` is free and told to coincide with
    // itself, `b` is pinned: both ends of a point on `b`'s line are on it.
    let mut collapsed = sketch(&[
        (P0, 0.0, 3.0),
        (P1, 10.0, 7.0),
        (P2, 20.0, 5.0),
        (P3, 30.0, 5.0),
    ]);
    pin(&mut collapsed, 1, P2, (20.0, 5.0));
    pin(&mut collapsed, 2, P3, (30.0, 5.0));
    collapsed
        .add_constraint(COLLINEAR, Constraint::Collinear { a: A, b: B })
        .add_constraint(OTHER, Constraint::Coincident { a: P0, b: P1 });
    // planegcs reports success after pulling the segment to a point on the
    // line; the measured residual is what refuses it, as infinitely far.
    match solver::solve(&collapsed).expect("a diagnosable system") {
        Outcome::Solved(solution) => panic!(
            "a collapsed segment was accepted: length {}",
            length(&solution, A)
        ),
        Outcome::DidNotConverge { worst_residual } => {
            assert_eq!(worst_residual, Some(f64::INFINITY));
        }
        other => panic!("expected a refused answer with no direction, got {other:?}"),
    }
    // The line itself collapsing is refused earlier, by the diagnosis, and it
    // names the collinear among its reasons.
    let mut line = pair(((0.0, 0.0), (10.0, 0.0)), ((20.0, 5.0), (30.0, 5.0)));
    line.add_constraint(COLLINEAR, Constraint::Collinear { a: A, b: B })
        .add_constraint(OTHER, Constraint::Coincident { a: P2, b: P3 });
    match solver::solve(&line).expect("a diagnosable system") {
        Outcome::Conflicting { constraints, .. } => {
            assert!(constraints.contains(&COLLINEAR), "{constraints:?}");
        }
        Outcome::DidNotConverge { worst_residual } => {
            assert_eq!(worst_residual, Some(f64::INFINITY));
        }
        other => panic!("a collapsed line was called solved: {other:?}"),
    }
    // Perpendicular to the same pair cannot hold with nothing collapsed.
    let impossible = with(
        with(
            pair(((0.0, 0.0), (10.0, 0.0)), ((20.0, 5.0), (30.0, 5.0))),
            COLLINEAR,
            Constraint::Collinear { a: A, b: B },
        ),
        OTHER,
        Constraint::Perpendicular { a: A, b: B },
    );
    assert!(
        !matches!(
            solver::solve(&impossible).expect("a diagnosable system"),
            Outcome::Solved(_)
        ),
        "collinear and perpendicular were both called satisfied"
    );
    println!("\nFCAD_31G_SOLVER_COLLAPSE_EXECUTED");
}

#[test]
fn a_drag_keeps_the_segments_collinear_while_one_end_follows_the_pointer() {
    if !native() {
        return;
    }
    // The drag path holds one native system and nudges a pin. The first
    // segment may turn about its pinned start; the second keeps to its line.
    let mut sketch = sketch(&[
        (P0, 0.0, 0.0),
        (P1, 10.0, 0.0),
        (P2, 20.0, 2.0),
        (P3, 30.0, 2.0),
    ]);
    pin(&mut sketch, 1, P0, (0.0, 0.0));
    sketch.add_constraint(COLLINEAR, Constraint::Collinear { a: A, b: B });
    let mut drag = solver::Drag::begin(&sketch, P3).expect("a draggable system");
    for target in [(31.0, 8.0), (28.0, 14.0), (25.0, -6.0)] {
        let outcome = drag.move_to(target.0, target.1).expect("a solved sample");
        let Outcome::Solved(solution) = outcome else {
            panic!("a drag sample did not solve: {outcome:?}");
        };
        on_one_line(&solution, A, B);
        let (x, y) = point(&solution, P3);
        assert!(
            (x - target.0).hypot(y - target.1) < 1e-6,
            "the end did not follow {target:?}: ({x}, {y})"
        );
    }
    println!("\nFCAD_31G_SOLVER_DRAG_EXECUTED");
}
