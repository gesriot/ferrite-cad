// SPDX-License-Identifier: MIT
//! What a rebuild says when a drawing contradicts itself.
//!
//! A conflicting solve produces no coordinates, so it produces no profile, no
//! solid and no scene. Until now it also produced nothing a program could act
//! on: a sentence, and a caller left to read it. These gates are about the
//! other thing it produces — the constraints the solve blamed, said in the
//! document's own words, carried out with the refusal.
//!
//! Everything here runs against a real file and a real planegcs. The
//! identifiers asserted are the ones this file wrote into the document, so a
//! build that invented one, matched one by position or answered with a
//! neighbour's would be naming something these gates did not write.
//!
//! The exact conflicting sets asserted here are measured, not assumed: they
//! are what a linked planegcs answered for these fixtures over three
//! consecutive runs. See the §21B-3b1 checkpoint in the implementation plan.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
    ObjectPayload, Point2, Sketch, SketchConstraint, SketchConstraintRule, SketchCurve,
    SketchGeometry, SketchPointRef, SketchPointSelector, SolidOperation,
};
use ferritecad_eval::{SketchConflict, rebuild_cold};
use ferritecad_kernel::{OperationContext, mock::MockKernel};
use ferritecad_sketch_solver as solver;
use ferritecad_types::{ErrorKind, ObjectId, StableEntityId, Transform};
use tempfile::TempDir;

const WIDTH: f64 = 60.0;
const HEIGHT: f64 = 40.0;

/// The four lines, stored nowhere near where the constraints put them.
const STORED: [((f64, f64), (f64, f64)); 4] = [
    ((0.5, -0.3), (59.2, 0.4)),
    ((59.4, 0.6), (60.6, 39.5)),
    ((60.3, 39.8), (-0.4, 40.3)),
    ((-0.2, 40.1), (0.3, 0.2)),
];

fn ready() -> bool {
    if solver::is_available() {
        return true;
    }
    assert!(
        !solver::is_required(),
        "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so no gate may skip: a conflict this build \
         cannot produce is a conflict this build has not been shown to explain"
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

fn line(id: StableEntityId, start: (f64, f64), end: (f64, f64)) -> SketchCurve {
    SketchCurve {
        id,
        construction: false,
        geometry: SketchGeometry::Line {
            start: Point2::new(start.0, start.1).expect("finite"),
            end: Point2::new(end.0, end.1).expect("finite"),
        },
    }
}

fn at(curve: StableEntityId, selector: SketchPointSelector) -> SketchPointRef {
    SketchPointRef::new(curve, selector)
}

fn plate_curves() -> Vec<SketchCurve> {
    STORED
        .iter()
        .map(|(start, end)| line(StableEntityId::new(), *start, *end))
        .collect()
}

/// The eleven rules that turn four loose lines into a 60 by 40 plate.
fn plate_rules(edges: &[StableEntityId]) -> Vec<SketchConstraintRule> {
    use SketchPointSelector::{End, Start};
    vec![
        SketchConstraintRule::Coincident {
            a: at(edges[0], End),
            b: at(edges[1], Start),
        },
        SketchConstraintRule::Coincident {
            a: at(edges[1], End),
            b: at(edges[2], Start),
        },
        SketchConstraintRule::Coincident {
            a: at(edges[2], End),
            b: at(edges[3], Start),
        },
        SketchConstraintRule::Coincident {
            a: at(edges[3], End),
            b: at(edges[0], Start),
        },
        SketchConstraintRule::Fixed {
            point: at(edges[0], Start),
            x: 0.0,
            y: 0.0,
        },
        SketchConstraintRule::Horizontal {
            a: at(edges[0], Start),
            b: at(edges[0], End),
        },
        SketchConstraintRule::Vertical {
            a: at(edges[1], Start),
            b: at(edges[1], End),
        },
        SketchConstraintRule::Horizontal {
            a: at(edges[2], Start),
            b: at(edges[2], End),
        },
        SketchConstraintRule::Vertical {
            a: at(edges[3], Start),
            b: at(edges[3], End),
        },
        SketchConstraintRule::Distance {
            a: at(edges[0], Start),
            b: at(edges[0], End),
            distance: WIDTH,
        },
        SketchConstraintRule::Distance {
            a: at(edges[1], Start),
            b: at(edges[1], End),
            distance: HEIGHT,
        },
    ]
}

fn width(edges: &[StableEntityId], distance: f64) -> SketchConstraintRule {
    SketchConstraintRule::Distance {
        a: at(edges[0], SketchPointSelector::Start),
        b: at(edges[0], SketchPointSelector::End),
        distance,
    }
}

fn height(edges: &[StableEntityId], distance: f64) -> SketchConstraintRule {
    SketchConstraintRule::Distance {
        a: at(edges[1], SketchPointSelector::Start),
        b: at(edges[1], SketchPointSelector::End),
        distance,
    }
}

fn named(rules: Vec<SketchConstraintRule>) -> Vec<SketchConstraint> {
    rules
        .into_iter()
        .map(|rule| SketchConstraint {
            id: StableEntityId::new(),
            rule,
        })
        .collect()
}

/// One document, one sketch per entry, each extruded into a body of its own.
fn write(
    dir: &TempDir,
    file: &str,
    sketches: Vec<(Option<&str>, Vec<SketchCurve>, Vec<SketchConstraint>)>,
) -> (std::path::PathBuf, Vec<ObjectId>) {
    let path = dir.path().join(file);
    let mut document = Document::create(&path).expect("creates");
    let plane = ObjectId::new();
    let ids: Vec<ObjectId> = sketches.iter().map(|_| ObjectId::new()).collect();

    document
        .write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )?;
            let mut order = 1i64;
            for (index, (name, curves, constraints)) in sketches.iter().enumerate() {
                let sketch = ids[index];
                let extrude = ObjectId::new();
                let body = ObjectId::new();
                w.put_object(
                    sketch,
                    None,
                    order,
                    *name,
                    &ObjectPayload::Sketch(Sketch {
                        plane,
                        curves: curves.clone(),
                        constraints: constraints.clone(),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: sketch,
                    dependency: plane,
                    role: DependencyRole::Plane,
                })?;
                w.put_object(
                    extrude,
                    None,
                    order + 1,
                    Some("Extrude"),
                    &ObjectPayload::Extrude(Extrude {
                        profile: sketch,
                        end_condition: EndCondition::Blind {
                            distance: Expression::constant(10.0)?,
                        },
                        reversed: false,
                        operation: SolidOperation::NewBody,
                        target_body: None,
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: extrude,
                    dependency: sketch,
                    role: DependencyRole::Profile,
                })?;
                w.put_object(
                    body,
                    None,
                    order + 2,
                    Some("Plate"),
                    &ObjectPayload::Body(Body {
                        tip_feature: Some(extrude),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: body,
                    dependency: extrude,
                    role: DependencyRole::BodyTip,
                })?;
                order += 3;
            }
            Ok(())
        })
        .expect("populates");
    document.close().expect("closes");
    (path, ids)
}

/// Every constraint the file gives back for one sketch, read from the file.
///
/// The gates compare against this rather than against the values they wrote,
/// so what they check is what a document holds and not what a test remembers.
fn stored_constraints(path: &std::path::Path, sketch: ObjectId) -> Vec<SketchConstraint> {
    let document = Document::open_read_only(path).expect("reopens");
    let record = document
        .objects()
        .expect("objects")
        .into_iter()
        .find(|record| record.id == sketch)
        .expect("the document holds that sketch");
    match record.payload {
        ObjectPayload::Sketch(sketch) => sketch.constraints,
        _ => panic!("that object is not a sketch"),
    }
}

/// Rebuilds cold and expects a refusal.
fn refused(path: &std::path::Path) -> (ferritecad_types::CadError, MockKernel) {
    let mut kernel = MockKernel::new();
    let document = Document::open_read_only(path).expect("reopens");
    let error = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("a drawing that contradicts itself has no solid");
    (error, kernel)
}

// ---------------------------------------------------------------------------
// The facts leave the rebuild
// ---------------------------------------------------------------------------

#[test]
fn a_conflict_carries_the_constraints_it_blamed_in_the_documents_own_words() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("a temporary directory is available");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = plate_rules(&edges);
    // A second width, disagreeing with the first. Nothing else changes.
    rules.push(width(&edges, WIDTH + 15.0));
    let (path, sketches) = write(
        &dir,
        "one.fcad",
        vec![(Some("Profile"), curves, named(rules))],
    );

    let (error, kernel) = refused(&path);

    assert_eq!(
        error.kind(),
        ErrorKind::Constraint,
        "a drawing that cannot hold is a constraint failure: {error}"
    );
    let conflict = SketchConflict::of(&error)
        .expect("a conflicting solve reached the caller as a sentence and nothing it could act on");

    assert_eq!(
        conflict.sketch(),
        sketches[0],
        "the conflict is about some other object than the sketch that holds it"
    );

    // Measured: this fixture makes planegcs blame the stored width and the one
    // that contradicts it, and nothing else.
    let stored = stored_constraints(&path, sketches[0]);
    let blamed: Vec<StableEntityId> = conflict.constraints().iter().map(|c| c.id()).collect();
    assert_eq!(
        blamed,
        vec![stored[9].id, stored[11].id],
        "the conflict does not name the two constraints the solver blamed, in the order the \
         document stores them"
    );

    // And each identifier carries the rule the document stores under exactly
    // it, read back out of the file rather than out of this test's memory.
    for constraint in conflict.constraints() {
        let expected = stored
            .iter()
            .find(|c| c.id == constraint.id())
            .expect("every blamed identifier is one the document stores");
        assert_eq!(
            constraint.rule(),
            &expected.rule,
            "constraint {} was reported carrying a rule the document stores under some other \
             identifier",
            constraint.id()
        );
    }

    assert_eq!(
        kernel.extrude_count(),
        0,
        "a conflict reached a kernel and asked it to build something"
    );
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a refused rebuild left shapes behind"
    );
}

#[test]
fn several_blamed_constraints_arrive_in_the_order_the_document_stores_them() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("a temporary directory is available");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = plate_rules(&edges);
    rules.push(width(&edges, WIDTH + 15.0));
    rules.push(height(&edges, HEIGHT + 9.0));
    let (path, sketches) = write(
        &dir,
        "two.fcad",
        vec![(Some("Profile"), curves, named(rules))],
    );

    let (error, _) = refused(&path);
    let conflict = SketchConflict::of(&error).expect("a conflict carries what it blamed");

    // Measured: both stored sizes and both contradictions, four in all.
    let stored = stored_constraints(&path, sketches[0]);
    let blamed: Vec<StableEntityId> = conflict.constraints().iter().map(|c| c.id()).collect();
    assert_eq!(
        blamed,
        vec![stored[9].id, stored[10].id, stored[11].id, stored[12].id],
        "four blamed constraints did not arrive whole and in the document's order"
    );
}

#[test]
fn moving_the_contradiction_to_the_front_moves_what_is_reported_with_it() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("a temporary directory is available");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    // The same document as the gate above, with the two contradicting rules
    // stored first. What is blamed must follow them there: a report that named
    // a fixed region of the list would be indistinguishable in the other
    // fixture and wrong here.
    let mut rules = vec![width(&edges, WIDTH + 15.0), height(&edges, HEIGHT + 9.0)];
    rules.extend(plate_rules(&edges));
    let (path, sketches) = write(
        &dir,
        "front.fcad",
        vec![(Some("Profile"), curves, named(rules))],
    );

    let (error, _) = refused(&path);
    let conflict = SketchConflict::of(&error).expect("a conflict carries what it blamed");

    let stored = stored_constraints(&path, sketches[0]);
    let blamed: Vec<StableEntityId> = conflict.constraints().iter().map(|c| c.id()).collect();
    assert_eq!(
        blamed,
        vec![stored[0].id, stored[1].id, stored[11].id, stored[12].id],
        "what was blamed did not follow the constraints that were moved"
    );
}

#[test]
fn identical_rules_under_different_identifiers_stay_apart() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("a temporary directory is available");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = plate_rules(&edges);
    // The same width, twice more, and then once differently. Three of the four
    // rules the solve blames say exactly the same thing.
    rules.push(width(&edges, WIDTH));
    rules.push(width(&edges, WIDTH));
    rules.push(width(&edges, WIDTH + 15.0));
    let (path, sketches) = write(
        &dir,
        "same.fcad",
        vec![(Some("Profile"), curves, named(rules))],
    );

    let (error, _) = refused(&path);
    let conflict = SketchConflict::of(&error).expect("a conflict carries what it blamed");

    // Measured: the stored width, the two repetitions of it, and the odd one.
    let stored = stored_constraints(&path, sketches[0]);
    let blamed: Vec<StableEntityId> = conflict.constraints().iter().map(|c| c.id()).collect();
    assert_eq!(
        blamed,
        vec![stored[9].id, stored[11].id, stored[12].id, stored[13].id],
        "identical rules under different identifiers were merged into one report"
    );

    // Three of them say the same thing and are still three.
    let identical: Vec<&ferritecad_eval::ConflictingConstraint> = conflict
        .constraints()
        .iter()
        .filter(|c| c.rule() == &width(&edges, WIDTH))
        .collect();
    assert_eq!(
        identical.len(),
        3,
        "three constraints saying the same thing did not stay three"
    );
    assert_ne!(identical[0].id(), identical[1].id());
    assert_ne!(identical[1].id(), identical[2].id());
    assert_ne!(identical[0].id(), identical[2].id());
}

#[test]
fn the_conflict_names_the_sketch_that_holds_it_and_not_its_neighbour() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("a temporary directory is available");

    // A sketch that solves, stored first, and a sketch that cannot, stored
    // second. A report that named the document's first sketch, or that carried
    // the neighbour's rules, would be wrong in a way one sketch cannot show.
    let good = plate_curves();
    let good_edges: Vec<StableEntityId> = good.iter().map(|curve| curve.id).collect();
    let bad = plate_curves();
    let bad_edges: Vec<StableEntityId> = bad.iter().map(|curve| curve.id).collect();
    let mut bad_rules = plate_rules(&bad_edges);
    bad_rules.push(width(&bad_edges, WIDTH + 15.0));

    let (path, sketches) = write(
        &dir,
        "pair.fcad",
        vec![
            (Some("Good"), good, named(plate_rules(&good_edges))),
            (Some("Bad"), bad, named(bad_rules)),
        ],
    );

    let (error, _) = refused(&path);
    let conflict = SketchConflict::of(&error).expect("a conflict carries what it blamed");

    assert_eq!(
        conflict.sketch(),
        sketches[1],
        "the conflict was filed against the sketch that solved"
    );

    // Every identifier it names belongs to the sketch it names, and none of
    // them belongs to the neighbour.
    let stored = stored_constraints(&path, sketches[1]);
    let blamed: Vec<StableEntityId> = conflict.constraints().iter().map(|c| c.id()).collect();
    assert_eq!(
        blamed,
        vec![stored[9].id, stored[11].id],
        "the conflict did not name the constraints of the sketch that holds it"
    );
    let held: Vec<StableEntityId> = stored.iter().map(|c| c.id).collect();
    let neighbour: Vec<StableEntityId> = stored_constraints(&path, sketches[0])
        .iter()
        .map(|c| c.id)
        .collect();
    for constraint in conflict.constraints() {
        assert!(
            held.contains(&constraint.id()),
            "{} is not a constraint of the sketch this conflict names",
            constraint.id()
        );
        assert!(
            !neighbour.contains(&constraint.id()),
            "{} belongs to the sketch next door",
            constraint.id()
        );
    }
}

// ---------------------------------------------------------------------------
// What a conflict is not
// ---------------------------------------------------------------------------

#[test]
fn a_failure_that_is_not_a_conflict_carries_no_conflict() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("a temporary directory is available");

    // A sketch whose only curve is construction geometry: there is no profile
    // to extrude, and the rebuild says so. It is a refusal on the same path
    // and it is not a conflict.
    let mut curves = plate_curves();
    for curve in &mut curves {
        curve.construction = true;
    }
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let (path, _) = write(
        &dir,
        "empty.fcad",
        vec![(Some("Guides"), curves, named(plate_rules(&edges)))],
    );

    let (error, _) = refused(&path);
    assert!(
        SketchConflict::of(&error).is_none(),
        "a sketch with nothing to extrude was reported as a set of constraints that disagree: \
         {error}"
    );
}

#[test]
fn a_solve_that_refused_for_some_other_reason_carries_no_conflict() {
    solver_or_skip!();

    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = plate_rules(&edges);
    // A constraint reaching for a point of a curve this sketch does not hold.
    // Persistence refuses to store one, so this sketch is built in memory and
    // handed straight to the conversion: the point is the seam, not the file.
    // The solve refuses before a solver is asked anything, and it refuses with
    // `ErrorKind::Constraint` — the same kind a conflict has, which is the
    // shape a fabricated report would hide behind.
    let stranger = StableEntityId::new();
    rules.push(SketchConstraintRule::Coincident {
        a: at(edges[0], SketchPointSelector::Start),
        b: at(stranger, SketchPointSelector::End),
    });
    let sketch = Sketch {
        plane: ObjectId::new(),
        curves,
        constraints: named(rules),
    };

    let error = ferritecad_eval::profile_from_sketch(
        &sketch,
        ObjectId::new(),
        ferritecad_kernel::SketchPlane::world_xy(),
    )
    .expect_err("a constraint naming nothing cannot be stated to a solver");

    assert_eq!(
        error.kind(),
        ErrorKind::Constraint,
        "a constraint naming nothing is a constraint failure: {error}"
    );
    assert!(
        SketchConflict::of(&error).is_none(),
        "a sketch that could not be stated at all was reported as one whose constraints \
         disagree: {error}"
    );
}

#[test]
fn a_build_with_no_solver_refuses_without_inventing_a_conflict() {
    if solver::is_available() {
        // This is about the answer a build with no library gives, and this
        // build has one. Deliberately not the `solver_or_skip!` shape: that
        // one refuses to skip under FERRITECAD_REQUIRE_PLANEGCS=1, and this
        // gate cannot run there at all.
        eprintln!("not applicable: this build has a library, and this is about one without");
        return;
    }

    let dir = tempfile::tempdir().expect("a temporary directory is available");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let (path, _) = write(
        &dir,
        "unlinked.fcad",
        vec![(Some("Profile"), curves, named(plate_rules(&edges)))],
    );

    let (error, _) = refused(&path);
    assert_eq!(
        error.kind(),
        ErrorKind::Unsupported,
        "a build with no solver cannot solve, and that is not a constraint failure: {error}"
    );
    assert!(
        SketchConflict::of(&error).is_none(),
        "a build with no solver said which of this drawing's constraints disagree: {error}"
    );
}

#[test]
fn a_document_that_will_not_open_carries_no_conflict() {
    let dir = tempfile::tempdir().expect("a temporary directory is available");
    let path = dir.path().join("not-a-document.fcad");
    std::fs::write(&path, b"this is not a document").expect("writes");

    let error = Document::open_read_only(&path).expect_err("that is not a document");
    assert!(
        SketchConflict::of(&error).is_none(),
        "a file that is not a document was reported as a set of constraints that disagree: \
         {error}"
    );
}

// ---------------------------------------------------------------------------
// What a conflict costs
// ---------------------------------------------------------------------------

#[test]
fn a_conflict_asks_the_solver_once_and_reads_the_document_once() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("a temporary directory is available");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = plate_rules(&edges);
    rules.push(width(&edges, WIDTH + 15.0));
    // Two features reading the one sketch, so a build that solved per consumer
    // would show it here.
    let (path, sketches) = write(
        &dir,
        "cost.fcad",
        vec![(Some("Profile"), curves, named(rules))],
    );
    let stored = stored_constraints(&path, sketches[0]);

    let sessions = solver::native_sessions();
    let solves = solver::native_solves();
    let live = solver::native_live_sessions();

    let (error, _) = refused(&path);
    let conflict = SketchConflict::of(&error).expect("a conflict carries what it blamed");

    assert_eq!(
        solver::native_sessions() - sessions,
        1,
        "one conflicting sketch built more than one native system"
    );
    // A conflict is diagnosed and refused before the system is solved, so the
    // native solve counter must not move at all. A build that solved anyway
    // and then reported the conflict would show here.
    assert_eq!(
        solver::native_solves(),
        solves,
        "a sketch that was refused before it was solved was solved anyway"
    );
    assert_eq!(
        solver::native_live_sessions(),
        live,
        "a refused solve left a native system alive"
    );

    // The document is gone, and the facts are still whole: they were carried
    // out with the refusal rather than fetched when somebody asked.
    std::fs::remove_file(&path).expect("removes");
    let blamed: Vec<StableEntityId> = conflict.constraints().iter().map(|c| c.id()).collect();
    assert_eq!(blamed, vec![stored[9].id, stored[11].id]);
    assert_eq!(conflict.constraints()[0].rule(), &stored[9].rule);
}

#[test]
fn a_conflict_publishes_nothing_that_lasts_one_solve() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("a temporary directory is available");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = plate_rules(&edges);
    rules.push(width(&edges, WIDTH + 15.0));
    let (path, _) = write(
        &dir,
        "quiet.fcad",
        vec![(Some("Profile"), curves, named(rules))],
    );

    let (error, _) = refused(&path);
    let conflict = SketchConflict::of(&error).expect("a conflict carries what it blamed");

    let printed = format!("{conflict:?} {conflict} {error}");
    for forbidden in [
        "PointId",
        "ConstraintId",
        "SolverError",
        "Outcome",
        "session",
        "Session",
        "ordinal",
        "equation",
    ] {
        assert!(
            !printed.contains(forbidden),
            "a conflict published {forbidden}, which means nothing outside one solve:\n{printed}"
        );
    }
}

// ---------------------------------------------------------------------------
// The successful path is unchanged
// ---------------------------------------------------------------------------

#[test]
fn a_document_that_solves_still_builds_and_reports_what_it_found() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("a temporary directory is available");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = plate_rules(&edges);
    // The width, said twice: solvable, and redundant.
    rules.push(width(&edges, WIDTH));
    let (path, sketches) = write(
        &dir,
        "good.fcad",
        vec![(Some("Profile"), curves, named(rules))],
    );
    let stored = stored_constraints(&path, sketches[0]);

    let mut kernel = MockKernel::new();
    let document = Document::open_read_only(&path).expect("reopens");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a plate that agrees with itself builds");

    let report = built
        .solve_report(sketches[0])
        .expect("a constrained sketch that solved reports what the solve found out");
    assert_eq!(report.degrees_of_freedom(), 0);
    assert!(!report.is_under_constrained());
    assert_eq!(
        report.redundant(),
        [stored[11].id],
        "the repeated width was not reported under the identifier the document stores"
    );
    assert_eq!(built.solve_reports().count(), 1);
    drop(built);
}
