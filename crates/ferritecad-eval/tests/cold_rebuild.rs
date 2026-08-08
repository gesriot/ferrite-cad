// SPDX-License-Identifier: MIT
//! Rebuilding a stored document into geometry, against the kernel contract.
//!
//! These exercise the contract from the consumer's side, which is the point of
//! doing this before an adapter exists: a contract that is awkward to call is
//! cheap to change now and expensive once a C ABI depends on its shape.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
    ObjectPayload, Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{
    CancelToken, GeometryKernel, HistoryInput, OperationContext, ProgressSink, TessellationParams,
    mock::MockKernel,
};
use ferritecad_types::{ErrorKind, ObjectId, Result, StableEntityId, Transform};
use tempfile::TempDir;

/// The plate the CLI's `create --sample` writes, with its identifiers kept.
struct Plate {
    plane: ObjectId,
    sketch: ObjectId,
    extrude: ObjectId,
    body: ObjectId,
    segments: Vec<StableEntityId>,
}

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

fn populate(document: &mut Document, width: f64, depth: f64, height: f64) -> Result<Plate> {
    let plate = Plate {
        plane: ObjectId::new(),
        sketch: ObjectId::new(),
        extrude: ObjectId::new(),
        body: ObjectId::new(),
        segments: (0..4).map(|_| StableEntityId::new()).collect(),
    };

    let corners = [(0.0, 0.0), (width, 0.0), (width, depth), (0.0, depth)];
    let mut curves = Vec::new();
    for (index, start) in corners.iter().enumerate() {
        curves.push(line(
            plate.segments[index],
            *start,
            corners[(index + 1) % corners.len()],
        )?);
    }

    document.write(|w| {
        w.put_object(
            plate.plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        w.put_object(
            plate.sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane: plate.plane,
                curves,
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: plate.sketch,
            dependency: plate.plane,
            role: DependencyRole::Plane,
        })?;
        w.put_object(
            plate.extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                profile: plate.sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                target_body: None,
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: plate.extrude,
            dependency: plate.sketch,
            role: DependencyRole::Profile,
        })?;
        w.put_object(
            plate.body,
            None,
            3,
            Some("Plate"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(plate.extrude),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: plate.body,
            dependency: plate.extrude,
            role: DependencyRole::BodyTip,
        })?;
        Ok(())
    })?;

    Ok(plate)
}

fn sample() -> (TempDir, Document, Plate) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("part.fcad")).expect("creates");
    let plate = populate(&mut document, 60.0, 40.0, 10.0).expect("populates");
    (dir, document, plate)
}

#[test]
fn a_plate_rebuilds_into_a_solid_with_named_faces() {
    let (_dir, document, plate) = sample();
    let mut kernel = MockKernel::new();

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("the sample rebuilds");

    // The datum, sketch, extrude and body were all visited, in dependency order.
    assert_eq!(built.order().len(), 4);
    assert_eq!(built.order()[0], plate.plane);
    assert!(built.profile(plate.sketch).is_some());

    let shape = built
        .shape(plate.extrude)
        .expect("the extrude made a solid");
    assert_eq!(
        built.shape(plate.body),
        Some(shape),
        "the body names its tip feature's shape rather than a second solid"
    );
    assert_eq!(built.shape_count(), 1, "only the extrude created geometry");

    // Every profile segment raised exactly one side face.
    let history = built
        .history(plate.extrude)
        .expect("the extrude has history");
    for segment in &plate.segments {
        assert_eq!(
            history.generated(&HistoryInput::Segment(*segment)).count(),
            1,
            "segment {segment} should have raised one face"
        );
    }

    let caps = built.caps(plate.extrude).expect("an extrusion has caps");
    assert_eq!(caps.start.len(), 1);
    assert_eq!(caps.end.len(), 1);
    assert_ne!(caps.start[0], caps.end[0]);

    built.release_all(&mut kernel);
}

#[test]
fn two_cold_rebuilds_agree() {
    let (_dir, document, plate) = sample();
    let context = OperationContext::default();

    let mut first = MockKernel::new();
    let one = rebuild_cold(&document, &mut first, &context).expect("rebuilds");
    let one_mesh = first
        .tessellate(
            one.shape(plate.extrude).expect("a solid"),
            &TessellationParams::default(),
            &context,
        )
        .expect("tessellates");
    let one_order = one.order().to_vec();
    one.release_all(&mut first);

    let mut second = MockKernel::new();
    let other = rebuild_cold(&document, &mut second, &context).expect("rebuilds");
    let other_mesh = second
        .tessellate(
            other.shape(plate.extrude).expect("a solid"),
            &TessellationParams::default(),
            &context,
        )
        .expect("tessellates");
    let other_order = other.order().to_vec();
    other.release_all(&mut second);

    assert_eq!(one_order, other_order, "evaluation order is deterministic");
    assert_eq!(one_mesh.positions, other_mesh.positions);
    assert_eq!(one_mesh.normals, other_mesh.normals);
    assert_eq!(one_mesh.indices, other_mesh.indices);
}

#[test]
fn a_reopened_document_rebuilds_identically() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("part.fcad");

    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document, 60.0, 40.0, 10.0).expect("populates");
    let context = OperationContext::default();

    let mut before_kernel = MockKernel::new();
    let before = rebuild_cold(&document, &mut before_kernel, &context).expect("rebuilds");
    let before_mesh = before_kernel
        .tessellate(
            before.shape(plate.extrude).expect("a solid"),
            &TessellationParams::default(),
            &context,
        )
        .expect("tessellates");
    before.release_all(&mut before_kernel);
    document.close().expect("closes");

    // The point of a cold rebuild: nothing from the session that wrote it is
    // needed to reproduce it.
    let reopened = Document::open(&path).expect("reopens");
    let mut after_kernel = MockKernel::new();
    let after = rebuild_cold(&reopened, &mut after_kernel, &context).expect("rebuilds");
    let after_mesh = after_kernel
        .tessellate(
            after.shape(plate.extrude).expect("a solid"),
            &TessellationParams::default(),
            &context,
        )
        .expect("tessellates");
    after.release_all(&mut after_kernel);

    assert_eq!(before_mesh.positions, after_mesh.positions);
    assert_eq!(before_mesh.indices, after_mesh.indices);
}

#[test]
fn releasing_gives_every_shape_back() {
    let (_dir, document, _plate) = sample();
    let mut kernel = MockKernel::new();

    let built =
        rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("rebuilds");
    assert_eq!(kernel.live_shape_count(), 1);

    built.release_all(&mut kernel);
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a released rebuild leaves the kernel holding nothing"
    );
}

#[test]
fn a_failed_rebuild_releases_what_it_had_already_made() {
    let (_dir, mut document, plate) = sample();

    // A second extrude that cannot be built, ordered after the good one.
    let broken = ObjectId::new();
    document
        .write(|w| {
            w.put_object(
                broken,
                None,
                4,
                Some("Cut1"),
                &ObjectPayload::Extrude(Extrude {
                    profile: plate.sketch,
                    end_condition: EndCondition::ThroughAll,
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: broken,
                dependency: plate.extrude,
                role: DependencyRole::TargetBody,
            })?;
            w.add_dependency(Dependency {
                dependent: broken,
                dependency: plate.sketch,
                role: DependencyRole::Profile,
            })?;
            Ok(())
        })
        .expect("the document stores it; it is the rebuild that cannot");

    let mut kernel = MockKernel::new();
    let err = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("ThroughAll is not implemented");

    assert_eq!(err.kind(), ErrorKind::Unsupported);
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the solid built before the failure must not be left behind"
    );
}

#[test]
fn cancellation_stops_the_rebuild_and_leaves_nothing_behind() {
    let (_dir, document, _plate) = sample();
    let token = CancelToken::new();
    token.cancel();

    let mut kernel = MockKernel::new();
    let err = rebuild_cold(
        &document,
        &mut kernel,
        &OperationContext::default().with_cancel(token),
    )
    .expect_err("a cancelled rebuild produces nothing");

    assert_eq!(err.kind(), ErrorKind::Cancellation);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_cancelled_empty_document_is_not_reported_as_rebuilt() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = Document::create(dir.path().join("part.fcad")).expect("creates");
    let token = CancelToken::new();
    token.cancel();

    let mut kernel = MockKernel::new();
    let err = rebuild_cold(
        &document,
        &mut kernel,
        &OperationContext::default().with_cancel(token),
    )
    .expect_err("cancellation applies even when there are no features");

    assert_eq!(err.kind(), ErrorKind::Cancellation);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn cancelling_partway_through_releases_the_finished_features() {
    let (_dir, document, _plate) = sample();
    let token = CancelToken::new();

    // Cancel as soon as the kernel reports any progress, which happens inside
    // the extrusion — after the datum and sketch have been evaluated.
    let trigger = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |_| trigger.cancel()));

    let mut kernel = MockKernel::new();
    let err = rebuild_cold(&document, &mut kernel, &context)
        .expect_err("cancelling mid-rebuild abandons it");

    assert_eq!(err.kind(), ErrorKind::Cancellation);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn cancellation_at_the_last_progress_event_releases_the_result() {
    let (_dir, mut document, plate) = sample();
    document
        .write(|w| {
            w.remove_dependency(Dependency {
                dependent: plate.body,
                dependency: plate.extrude,
                role: DependencyRole::BodyTip,
            })?;
            w.remove_object(plate.body)?;
            Ok(())
        })
        .expect("removes the body so the extrusion is the final object");

    let token = CancelToken::new();
    let trigger = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |fraction| {
            if fraction >= 1.0 {
                trigger.cancel();
            }
        }));

    let mut kernel = MockKernel::new();
    let err = rebuild_cold(&document, &mut kernel, &context)
        .expect_err("a cancellation reported as the final shape completes still wins");

    assert_eq!(err.kind(), ErrorKind::Cancellation);
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the just-created shape is registered before cancellation cleanup"
    );
}

#[test]
fn a_missing_semantic_dependency_never_reaches_the_kernel() {
    let (_dir, mut document, plate) = sample();
    document
        .write(|w| {
            w.remove_dependency(Dependency {
                dependent: plate.extrude,
                dependency: plate.sketch,
                role: DependencyRole::Profile,
            })?;
            Ok(())
        })
        .expect("makes the stored graph semantically invalid");

    let mut kernel = MockKernel::new();
    let err = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("payload references must have matching dependency edges");

    assert_eq!(err.kind(), ErrorKind::Input);
    assert!(err.to_string().contains("reference.missing-edge"));
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_sketch_with_a_circle_is_unsupported_rather_than_approximated() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("part.fcad")).expect("creates");

    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();

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
                Some("Round"),
                &ObjectPayload::Sketch(Sketch {
                    plane,
                    curves: vec![SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Circle {
                            center: Point2::ORIGIN,
                            radius: 5.0,
                        },
                    }],
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
                Some("Boss"),
                &ObjectPayload::Extrude(Extrude {
                    profile: sketch,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(5.0)?,
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
            Ok(())
        })
        .expect("writes");

    let mut kernel = MockKernel::new();
    let err = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("a circular profile is not implemented in this slice");

    assert_eq!(err.kind(), ErrorKind::Unsupported);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_symmetric_extrusion_straddles_the_sketch_plane() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("part.fcad")).expect("creates");
    let plate = populate(&mut document, 20.0, 20.0, 10.0).expect("populates");

    document
        .write(|w| {
            w.put_object(
                plate.extrude,
                None,
                2,
                Some("Extrude1"),
                &ObjectPayload::Extrude(Extrude {
                    profile: plate.sketch,
                    end_condition: EndCondition::Symmetric {
                        distance: Expression::constant(4.0)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
                }),
            )?;
            Ok(())
        })
        .expect("rewrites the feature");

    let context = OperationContext::default();
    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &context).expect("rebuilds");
    let mesh = kernel
        .tessellate(
            built.shape(plate.extrude).expect("a solid"),
            &TessellationParams::default(),
            &context,
        )
        .expect("tessellates");

    let heights: Vec<f32> = mesh.positions.chunks_exact(3).map(|p| p[2]).collect();
    let lowest = heights.iter().copied().fold(f32::INFINITY, f32::min);
    let highest = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);

    // Four either side, not four in total.
    assert!((lowest + 4.0).abs() < 1e-4);
    assert!((highest - 4.0).abs() < 1e-4);

    built.release_all(&mut kernel);
}

#[test]
fn an_object_this_build_cannot_interpret_stops_the_rebuild() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("part.fcad")).expect("creates");

    let future = ferritecad_document::Envelope::new("feature.loft", 1, Vec::new(), vec![0xf6])
        .to_bytes()
        .expect("serialises");
    let payload = ObjectPayload::from_storage_bytes(&future).expect("header is readable");

    document
        .write(|w| {
            w.put_object(ObjectId::new(), None, 0, Some("Loft1"), &payload)?;
            Ok(())
        })
        .expect("preserved verbatim");

    let mut kernel = MockKernel::new();
    let err = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("an unknown feature cannot be rebuilt");

    // Preserving it on disk and rebuilding it are different questions, and the
    // answer to the second is a plain refusal.
    assert_eq!(err.kind(), ErrorKind::Unsupported);
}

#[test]
fn an_empty_document_rebuilds_to_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = Document::create(dir.path().join("part.fcad")).expect("creates");

    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("an empty document is valid");

    assert!(built.order().is_empty());
    assert_eq!(built.shape_count(), 0);
    built.release_all(&mut kernel);
}
