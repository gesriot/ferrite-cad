// SPDX-License-Identifier: MIT
//! A stored document rebuilt into real geometry.
//!
//! This is the slice's point: a `.fcad` file on disk, through the cold
//! evaluator, into Open CASCADE, with no mock anywhere. The evaluator names no
//! kernel type — it takes `&mut dyn GeometryKernel` — so the only difference
//! from the mock-backed test is which value is passed in.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
    ObjectPayload, Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{HistoryInput, OperationContext};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{ObjectId, Result, StableEntityId, Transform};

struct Plate {
    sketch: ObjectId,
    extrude: ObjectId,
    body: ObjectId,
    segments: Vec<StableEntityId>,
}

fn populate(document: &mut Document, width: f64, depth: f64, height: f64) -> Result<Plate> {
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let plate = Plate {
        sketch,
        extrude: ObjectId::new(),
        body: ObjectId::new(),
        segments: (0..4).map(|_| StableEntityId::new()).collect(),
    };

    let corners = [(0.0, 0.0), (width, 0.0), (width, depth), (0.0, depth)];
    let mut curves = Vec::new();
    for (index, start) in corners.iter().enumerate() {
        let end = corners[(index + 1) % corners.len()];
        curves.push(SketchCurve {
            id: plate.segments[index],
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(start.0, start.1)?,
                end: Point2::new(end.0, end.1)?,
            },
        });
    }

    document.write(|w| {
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
            &ObjectPayload::Sketch(Sketch { plane, curves }),
        )?;
        w.add_dependency(Dependency {
            dependent: sketch,
            dependency: plane,
            role: DependencyRole::Plane,
        })?;
        w.put_object(
            plate.extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                profile: sketch,
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
            dependency: sketch,
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

#[test]
fn a_saved_document_rebuilds_into_open_cascade_geometry() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad");

    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document, 60.0, 40.0, 10.0).expect("populates");
    document.close().expect("closes");

    // Reopened from disk: nothing from the writing session is in play.
    let reopened = Document::open(&path).expect("reopens");
    let mut kernel = OcctKernel::new().expect("opens a session");

    let built = rebuild_cold(&reopened, &mut kernel, &OperationContext::default())
        .expect("the plate rebuilds through Open CASCADE");

    let shape = built
        .shape(plate.extrude)
        .expect("the extrude made a solid");
    assert_eq!(
        built.shape(plate.body),
        Some(shape),
        "the body names its tip feature's solid"
    );

    let (faces, volume) = kernel.shape_stats(shape).expect("measures");
    assert_eq!(faces, 6);
    assert!(
        (volume - 24_000.0).abs() < 1e-6,
        "60 x 40 x 10 is 24000 mm^3, got {volume}"
    );

    // Every sketch segment is named by the face it raised. This is the whole
    // reason history exists, and the reason the bridge shares corner vertices.
    let history = built
        .history(plate.extrude)
        .expect("the extrude has history");
    for segment in &plate.segments {
        assert_eq!(
            history.generated(HistoryInput::Segment(*segment)).count(),
            1,
            "segment {segment} should have raised one face"
        );
    }

    let caps = built.caps(plate.extrude).expect("an extrusion has caps");
    assert_eq!(caps.start.len(), 1);
    assert_eq!(caps.end.len(), 1);

    built.release_all(&mut kernel);
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a released rebuild leaves Open CASCADE holding nothing"
    );
}

#[test]
fn two_cold_rebuilds_of_one_document_agree() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("plate.fcad")).expect("creates");
    let plate = populate(&mut document, 25.0, 15.0, 3.0).expect("populates");
    let context = OperationContext::default();

    let measure = |kernel: &mut OcctKernel| {
        let built = rebuild_cold(&document, kernel, &context).expect("rebuilds");
        let shape = built.shape(plate.extrude).expect("a solid");
        let stats = kernel.shape_stats(shape).expect("measures");
        built.release_all(kernel);
        stats
    };

    let mut first = OcctKernel::new().expect("opens");
    let one = measure(&mut first);

    let mut second = OcctKernel::new().expect("opens");
    let other = measure(&mut second);

    assert_eq!(one.0, other.0, "face count is reproducible");
    assert!(
        (one.1 - other.1).abs() < 1e-9,
        "volume is reproducible: {} vs {}",
        one.1,
        other.1
    );
    assert!((one.1 - 25.0 * 15.0 * 3.0).abs() < 1e-6);
}

#[test]
fn a_feature_this_slice_cannot_build_fails_without_leaking() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("plate.fcad")).expect("creates");
    let plate = populate(&mut document, 20.0, 20.0, 5.0).expect("populates");

    // A second extrusion the evaluator refuses, ordered after the good one.
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

    let mut kernel = OcctKernel::new().expect("opens");
    let err = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("ThroughAll is not implemented");

    assert_eq!(err.kind(), ferritecad_types::ErrorKind::Unsupported);
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the solid built before the failure must not be left in Open CASCADE"
    );
}
