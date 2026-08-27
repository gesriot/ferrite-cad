// SPDX-License-Identifier: MIT
//! What the evaluator does with a sketch whose constraints nothing has solved.
//!
//! Until §21B-1b connects the solver, the honest answer is a refusal. The
//! stored coordinates of a constrained sketch are wherever its curves were last
//! left; the constraints are what the drawing means. Building from the first
//! and ignoring the second produces a solid that looks finished and disagrees
//! with the model, and a rebuild cannot report that, because it succeeded.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
    ObjectPayload, Point2, Sketch, SketchConstraint, SketchConstraintRule, SketchCurve,
    SketchGeometry, SketchPointRef, SketchPointSelector, SolidOperation,
};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{OperationContext, mock::MockKernel};
use ferritecad_types::{ErrorKind, ObjectId, Result, StableEntityId, Transform};
use tempfile::TempDir;

fn line(id: StableEntityId, start: (f64, f64), end: (f64, f64)) -> Result<SketchCurve> {
    Ok(SketchCurve {
        id,
        construction: false,
        geometry: SketchGeometry::Line {
            start: Point2::new(start.0, start.1)?,
            end: Point2::new(end.0, end.1)?,
        },
    })
}

/// The same square plate the other rebuild gates use, optionally carrying one
/// constraint. Nothing else differs, so any difference in outcome is the
/// constraint and not the shape.
fn plate(constrained: bool) -> (TempDir, Document) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("part.fcad")).expect("creates");

    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();
    let body = ObjectId::new();
    let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();

    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let curves: Vec<SketchCurve> = (0..4)
        .map(|index| {
            line(
                segments[index],
                corners[index],
                corners[(index + 1) % corners.len()],
            )
            .expect("finite")
        })
        .collect();

    // A coincidence between the end of one edge and the start of the next: the
    // relationship a chained profile already satisfies by construction, so the
    // stored coordinates are not even wrong. The refusal must not depend on
    // the constraint being violated.
    let constraints = if constrained {
        vec![SketchConstraint {
            id: StableEntityId::new(),
            rule: SketchConstraintRule::Coincident {
                a: SketchPointRef::new(segments[0], SketchPointSelector::End),
                b: SketchPointRef::new(segments[1], SketchPointSelector::Start),
            },
        }]
    } else {
        Vec::new()
    };

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
            w.put_object(
                sketch,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(Sketch {
                    plane,
                    curves,
                    constraints,
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
                2,
                Some("Extrude1"),
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
                3,
                Some("Plate"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: body,
                dependency: extrude,
                role: DependencyRole::BodyTip,
            })
        })
        .expect("populates");

    (dir, document)
}

#[test]
fn a_constrained_sketch_is_refused_and_no_kernel_is_asked_for_anything() {
    let (_dir, document) = plate(true);
    let mut kernel = MockKernel::new();

    let error = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("a sketch whose constraints nobody solved must not become a solid");

    assert_eq!(
        error.kind(),
        ErrorKind::Unsupported,
        "the refusal must say this build cannot serve the request, not that the model is wrong"
    );
    let message = error.to_string();
    assert!(message.contains("constraint"), "{message}");
    assert!(
        message.contains("no solver has been asked to satisfy"),
        "the refusal must say why, not just that: {message}"
    );

    assert_eq!(
        kernel.extrude_count(),
        0,
        "the kernel was asked to build something before the refusal"
    );
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a refused rebuild left shapes behind"
    );
}

#[test]
fn the_same_plate_without_constraints_still_rebuilds() {
    // The other half: the refusal is about constraints and nothing else. Take
    // them away and the identical document builds the identical solid.
    let (_dir, document) = plate(false);
    let mut kernel = MockKernel::new();

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("an unconstrained plate rebuilds as it always did");
    assert_eq!(built.order().len(), 4);
    assert_eq!(kernel.extrude_count(), 1);
    built.release_all(&mut kernel);
}

#[test]
fn a_constraint_on_construction_geometry_refuses_too() {
    // Construction geometry produces no edges, so the profile conversion skips
    // it. That must not let a constraint stated over it slip past unnoticed:
    // the constraint is exactly as unsolved as any other.
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("part.fcad")).expect("creates");

    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();
    let guide = StableEntityId::new();
    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let mut curves: Vec<SketchCurve> = (0..4)
        .map(|index| {
            line(
                segments[index],
                corners[index],
                corners[(index + 1) % corners.len()],
            )
            .expect("finite")
        })
        .collect();
    let mut construction = line(guide, (0.0, 0.0), (60.0, 40.0)).expect("finite");
    construction.construction = true;
    curves.push(construction);

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
            w.put_object(
                sketch,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(Sketch {
                    plane,
                    curves,
                    constraints: vec![SketchConstraint {
                        id: StableEntityId::new(),
                        rule: SketchConstraintRule::Horizontal {
                            a: SketchPointRef::new(guide, SketchPointSelector::Start),
                            b: SketchPointRef::new(guide, SketchPointSelector::End),
                        },
                    }],
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: sketch,
                dependency: plane,
                role: DependencyRole::Plane,
            })
        })
        .expect("populates");

    let mut kernel = MockKernel::new();
    let error = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("a constraint on construction geometry is still an unsolved constraint");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert_eq!(kernel.extrude_count(), 0);
}
