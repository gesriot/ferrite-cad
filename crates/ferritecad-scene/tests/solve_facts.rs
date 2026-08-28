// SPDX-License-Identifier: MIT
//! What a loaded scene knows about the sketches it was drawn from.
//!
//! A picture of a solid says nothing about the drawing behind it: the sketch
//! is not on screen, the extrusion is. These gates are about the third thing a
//! load produces beside the picture and what a click means — what the solve of
//! each constrained sketch found out, carried out in the document's own words
//! by the one rebuild that could have found it out.
//!
//! Everything here goes through [`snapshot_of`] against a real file. Nothing
//! calls a solver, and nothing may: if these facts could be obtained by asking
//! again, they would not have to be carried.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::Path;

use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
    ObjectPayload, Point2, Sketch, SketchConstraint, SketchConstraintRule, SketchCurve,
    SketchGeometry, SketchPointRef, SketchPointSelector, SolidOperation, StepImportRequest,
};
use ferritecad_exchange::{ColourSource, Definition, Import, Instance, Scene};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane, TessellationParams,
    mock::MockKernel,
};
use ferritecad_scene::{LoadedScene, snapshot_of};
use ferritecad_sketch_solver as solver;
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId, Transform};

const CORNERS: [(f64, f64); 4] = [(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)];
const WIDTH: f64 = 60.0;

fn ready() -> bool {
    if solver::is_available() {
        return true;
    }
    assert!(
        !solver::is_required(),
        "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so no gate may skip: a scene that cannot be \
         loaded from a constrained document has not been shown to carry anything"
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

fn params() -> TessellationParams {
    TessellationParams::default()
}

/// Refuses to read a STEP file, because these documents hold none.
fn no_step(_: &mut MockKernel, _: &[u8]) -> Result<Import> {
    Err(CadError::unsupported(
        "this gate's documents hold no imported geometry",
    ))
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
    (0..4)
        .map(|index| {
            line(
                StableEntityId::new(),
                CORNERS[index],
                CORNERS[(index + 1) % CORNERS.len()],
            )
        })
        .collect()
}

/// Nine constraints that close and square the plate without sizing it: two
/// degrees of freedom left over.
fn frame(edges: &[StableEntityId]) -> Vec<SketchConstraintRule> {
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
    ]
}

fn width_of(edges: &[StableEntityId]) -> SketchConstraintRule {
    SketchConstraintRule::Distance {
        a: at(edges[0], SketchPointSelector::Start),
        b: at(edges[0], SketchPointSelector::End),
        distance: WIDTH,
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

/// One sketch, one extrude and one body, written to a real file.
///
/// `sketches` names each sketch: its display name, its curves and its
/// constraints. Every one is extruded and given a body of its own, so a
/// document with two sketches draws two solids.
fn write(
    path: &Path,
    sketches: Vec<(Option<&str>, Vec<SketchCurve>, Vec<SketchConstraint>)>,
) -> Vec<ObjectId> {
    let mut document = Document::create(path).expect("creates");
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
    ids
}

fn load(path: &Path) -> LoadedScene {
    let mut kernel = MockKernel::new();
    let loaded = snapshot_of(
        path,
        &mut kernel,
        no_step,
        &params(),
        &OperationContext::default(),
    )
    .expect("the document loads");
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a load kept shapes the session will never hear about again"
    );
    loaded
}

// ---------------------------------------------------------------------------
// The facts reach the scene
// ---------------------------------------------------------------------------

#[test]
fn a_loaded_scene_says_what_the_solve_of_each_sketch_found_out() {
    solver_or_skip!();

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    rules.push(width_of(&edges));
    let constraints = named(rules);
    let repeated = constraints[10].id;
    let sketches = write(&path, vec![(Some("Profile"), curves, constraints)]);

    let loaded = load(&path);

    assert_eq!(loaded.sketch_solves.len(), 1, "one sketch, one account");
    let facts = &loaded.sketch_solves[0];
    assert_eq!(
        facts.sketch, sketches[0],
        "the account names another object"
    );
    assert_eq!(facts.name.as_deref(), Some("Profile"));
    assert_eq!(facts.report.degrees_of_freedom(), 1);
    assert!(facts.report.is_under_constrained());
    assert_eq!(facts.report.redundant(), [repeated]);
}

#[test]
fn two_sketches_keep_their_own_facts_and_the_documents_order() {
    solver_or_skip!();

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("two.fcad");

    // The first is free in two directions and repeats nothing; the second is
    // sized once, so it is free in one and repeats the size it was given
    // twice. Two sketches whose facts could not be swapped without showing.
    let loose = plate_curves();
    let loose_edges: Vec<StableEntityId> = loose.iter().map(|curve| curve.id).collect();
    let tight = plate_curves();
    let tight_edges: Vec<StableEntityId> = tight.iter().map(|curve| curve.id).collect();
    let mut tight_rules = frame(&tight_edges);
    tight_rules.push(width_of(&tight_edges));
    tight_rules.push(width_of(&tight_edges));
    let tight_constraints = named(tight_rules);
    let repeated = tight_constraints[10].id;

    let sketches = write(
        &path,
        vec![
            (Some("Loose"), loose, named(frame(&loose_edges))),
            (Some("Sized"), tight, tight_constraints),
        ],
    );

    let loaded = load(&path);

    assert_eq!(loaded.sketch_solves.len(), 2);
    assert_eq!(
        loaded
            .sketch_solves
            .iter()
            .map(|facts| facts.sketch)
            .collect::<Vec<_>>(),
        sketches,
        "the accounts must arrive in the order the document stores the sketches"
    );
    assert_eq!(loaded.sketch_solves[0].name.as_deref(), Some("Loose"));
    assert_eq!(loaded.sketch_solves[0].report.degrees_of_freedom(), 2);
    assert!(loaded.sketch_solves[0].report.redundant().is_empty());
    assert_eq!(loaded.sketch_solves[1].name.as_deref(), Some("Sized"));
    assert_eq!(loaded.sketch_solves[1].report.degrees_of_freedom(), 1);
    assert_eq!(loaded.sketch_solves[1].report.redundant(), [repeated]);
}

#[test]
fn a_sketch_nobody_constrained_produces_no_account_of_a_solve() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plain.fcad");
    write(&path, vec![(Some("Profile"), plate_curves(), Vec::new())]);

    let loaded = load(&path);

    assert!(
        loaded.sketch_solves.is_empty(),
        "nothing solved this sketch, and the scene says something about it anyway"
    );
}

#[test]
fn a_document_of_stored_geometry_carries_no_accounts() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("imported.fcad");
    let mut kernel = MockKernel::new();
    store_import(&path, &mut kernel);

    let loaded = snapshot_of(
        &path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: one_part(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("the stored import reopens");

    assert!(
        loaded.sketch_solves.is_empty(),
        "an imported definition was never solved, and the scene invented an account of it"
    );
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn an_import_beside_a_solved_sketch_gets_no_account_of_its_own() {
    // The two kinds of geometry in one document. One was solved and one was
    // read from stored bytes, and only one of them has anything to say about
    // how it was solved. A scene that handed every object the facts it had
    // would look right on a document holding only sketches.
    solver_or_skip!();

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("both.fcad");
    let mut kernel = MockKernel::new();

    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    let sketches = add_import_to(
        &path,
        &mut kernel,
        vec![(Some("Profile"), curves, named(rules))],
    );

    let loaded = snapshot_of(
        &path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: one_part(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("a document of both kinds loads");

    assert_eq!(
        loaded.sketch_solves.len(),
        1,
        "only the sketch was solved: {:?}",
        loaded.sketch_solves
    );
    assert_eq!(loaded.sketch_solves[0].sketch, sketches[0]);
    assert_eq!(loaded.sketch_solves[0].name.as_deref(), Some("Profile"));
    assert_eq!(loaded.sketch_solves[0].report.degrees_of_freedom(), 1);
    assert!(
        loaded.snapshot.meshes().len() >= 2,
        "both kinds of geometry must actually be drawn, or this proves nothing"
    );
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn an_empty_document_carries_no_accounts() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("empty.fcad");
    Document::create(&path)
        .expect("creates")
        .close()
        .expect("closes");

    assert!(load(&path).sketch_solves.is_empty());
}

// ---------------------------------------------------------------------------
// The picture does not change
// ---------------------------------------------------------------------------

#[test]
fn what_a_solve_found_out_is_no_part_of_the_picture() {
    // Two documents whose sketches are the same drawing and say so
    // differently: one is sized once, the other says the same size twice. The
    // pixels, the picture's own identity and the catalogue must be the same;
    // the accounts must not be.
    //
    // Stored already satisfying every constraint, so both solves have nothing
    // to move and the comparison is of what was drawn rather than of how close
    // two arithmetics came. A drawing that had to be moved is what every other
    // gate here is about.
    solver_or_skip!();

    let plain = tempfile::tempdir().expect("a temporary directory is available");
    let repeats = tempfile::tempdir().expect("a temporary directory is available");
    let plain_path = plain.path().join("plate.fcad");
    let repeats_path = repeats.path().join("plate.fcad");

    let settled = [(0.0, 0.0), (WIDTH, 0.0), (WIDTH, 30.0), (0.0, 30.0)];
    let curves: Vec<SketchCurve> = (0..4)
        .map(|index| {
            line(
                StableEntityId::new(),
                settled[index],
                settled[(index + 1) % settled.len()],
            )
        })
        .collect();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut once = frame(&edges);
    once.push(width_of(&edges));
    let mut twice = once.clone();
    twice.push(width_of(&edges));

    write(
        &plain_path,
        vec![(Some("Profile"), curves.clone(), named(once))],
    );
    write(&repeats_path, vec![(Some("Profile"), curves, named(twice))]);

    let one = load(&plain_path);
    let two = load(&repeats_path);

    // Value equality of the whole picture, which is the strongest thing that
    // can be said here and the one a renderer acts on: the same packed
    // triangles, the same draws, the same face, edge and vertex identities and
    // the same bound content identity. A renderer that re-uploads when the
    // snapshot it holds differs re-uploads for neither of these.
    assert_eq!(
        one.snapshot, two.snapshot,
        "identical geometry constrained differently produced a different picture"
    );
    assert_eq!(one.snapshot.face_count(), two.snapshot.face_count());
    assert_eq!(one.snapshot.pick_of(0), two.snapshot.pick_of(0));
    assert_eq!(
        one.catalogue.len(),
        two.catalogue.len(),
        "the catalogue counts sketches now"
    );

    assert!(one.sketch_solves[0].report.redundant().is_empty());
    assert_eq!(two.sketch_solves[0].report.redundant().len(), 1);
    assert_ne!(
        one.sketch_solves, two.sketch_solves,
        "two sketches that say the same size a different number of times reported the same thing"
    );
}

#[test]
fn a_document_that_cannot_be_loaded_gives_back_no_scene() {
    solver_or_skip!();

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("impossible.fcad");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    rules.push(SketchConstraintRule::Distance {
        a: at(edges[0], SketchPointSelector::Start),
        b: at(edges[0], SketchPointSelector::End),
        distance: WIDTH + 15.0,
    });
    write(&path, vec![(Some("Profile"), curves, named(rules))]);

    let mut kernel = MockKernel::new();
    let error = snapshot_of(
        &path,
        &mut kernel,
        no_step,
        &params(),
        &OperationContext::default(),
    )
    .expect_err("a plate that is two widths at once has no picture");

    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Constraint);
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a failed load left shapes behind"
    );
}

// ---------------------------------------------------------------------------
// A document that holds only stored geometry
// ---------------------------------------------------------------------------

fn one_part(kernel: &mut MockKernel) -> Scene {
    let corners = [
        PlanarPoint::new(0.0, 0.0),
        PlanarPoint::new(10.0, 0.0),
        PlanarPoint::new(10.0, 10.0),
        PlanarPoint::new(0.0, 10.0),
    ]
    .map(|corner| corner.expect("finite"));
    let segments = corners
        .iter()
        .enumerate()
        .map(|(index, start)| {
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(*start, corners[(index + 1) % corners.len()])
                    .expect("distinct"),
            )
        })
        .collect();
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(segments).expect("closes"),
        Vec::new(),
    )
    .expect("valid");
    let shape = kernel
        .extrude(
            &ExtrudeRequest::new(profile, ExtrudeExtent::blind(4.0).expect("positive"), false),
            &OperationContext::default(),
        )
        .expect("the mock builds a solid")
        .shape;

    Scene {
        source_unit: "MILLIMETRE".to_owned(),
        schema: "AP214".to_owned(),
        definitions: vec![Definition {
            shape,
            name: "Plate".to_owned(),
            solids: 1,
            key: "step.product_definition#5".to_owned(),
        }],
        instances: vec![Instance {
            definition: 0,
            parent: None,
            name: "Plate".to_owned(),
            placement: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            colour_source: ColourSource::None,
            colour: [0.0; 3],
        }],
    }
}

/// The same document, with one stored import added beside its sketches.
fn add_import_to(
    path: &Path,
    kernel: &mut MockKernel,
    sketches: Vec<(Option<&str>, Vec<SketchCurve>, Vec<SketchConstraint>)>,
) -> Vec<ObjectId> {
    let ids = write(path, sketches);
    let mut document = Document::open(path).expect("reopens");
    let import = Import::Imported {
        scene: one_part(kernel),
        diagnostics: Vec::new(),
    };
    document
        .store_step_import(StepImportRequest {
            object: ObjectId::new(),
            name: Some("Imported"),
            source: b"ISO-10303-21; this is what the document stores",
            source_name: None,
            import: &import,
            importer: kernel.identity(),
        })
        .expect("stores");
    document.close().expect("closes");
    for shape in import.scene().expect("a scene was stored").shapes() {
        kernel.release(shape);
    }
    ids
}

fn store_import(path: &Path, kernel: &mut MockKernel) {
    let import = Import::Imported {
        scene: one_part(kernel),
        diagnostics: Vec::new(),
    };
    let mut document = Document::create(path).expect("creates");
    document
        .store_step_import(StepImportRequest {
            object: ObjectId::new(),
            name: Some("Imported"),
            source: b"ISO-10303-21; this is what the document stores",
            source_name: None,
            import: &import,
            importer: kernel.identity(),
        })
        .expect("stores");
    document.close().expect("closes");
    for shape in import.scene().expect("a scene was stored").shapes() {
        kernel.release(shape);
    }
}
