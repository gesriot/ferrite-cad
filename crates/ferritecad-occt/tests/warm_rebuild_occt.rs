// SPDX-License-Identifier: MIT
//! A warm rebuild against real geometry.
//!
//! The evaluator's cache logic is kernel-agnostic and the mock proves its
//! rules, but the claim the product makes is about Open CASCADE: reopen a
//! document and get the same solid back without recomputing it. This is that
//! claim, end to end, with a real B-Rep through a real sidecar file.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Body, CacheStore, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition,
    Expression, Extrude, ObjectPayload, Point2, Sketch, SketchCurve, SketchGeometry,
    SolidOperation,
};
use ferritecad_eval::{CacheOutcome, rebuild_cached};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{ObjectId, Result, StableEntityId, Transform};

struct Plate {
    extrude: ObjectId,
    body: ObjectId,
    segments: Vec<StableEntityId>,
}

fn populate(document: &mut Document, width: f64, depth: f64, height: f64) -> Result<Plate> {
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let plate = Plate {
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
fn open_cascade_geometry_comes_back_without_being_recomputed() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad");
    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document, 60.0, 40.0, 10.0).expect("populates");
    let document_id = document.meta().document_id;
    let cache_path = dir.path().join("plate.fcad-cache");
    let context = OperationContext::default();

    // The first run computes the solid and leaves an archive behind.
    let (faces, volume) = {
        let mut kernel = OcctKernel::new().expect("opens");
        let mut cache = CacheStore::open(
            &cache_path,
            document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("the sidecar opens");

        let (built, events) =
            rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds");
        assert_eq!(events[0].outcome, CacheOutcome::Miss);
        assert!(
            events
                .iter()
                .all(|e| e.outcome != CacheOutcome::WriteFailed),
            "the archive should have been written: {events:?}"
        );

        let shape = built.shape(plate.extrude).expect("a solid");
        let stats = kernel.shape_stats(shape).expect("measures");
        built.release_all(&mut kernel);
        assert_eq!(kernel.live_shape_count(), 0);
        stats
    };
    assert_eq!(faces, 6);
    assert!(
        (volume - 24_000.0).abs() < 1e-6,
        "60 x 40 x 10, got {volume}"
    );

    // A session that never built this plate.
    let mut kernel = OcctKernel::new().expect("opens");
    let mut cache = CacheStore::open(
        &cache_path,
        document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("the sidecar opens again");

    let (built, events) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds");
    assert_eq!(
        events.iter().map(|e| e.outcome).collect::<Vec<_>>(),
        vec![CacheOutcome::Hit],
        "the plate's one extrusion should have come out of the sidecar"
    );

    let shape = built.shape(plate.extrude).expect("a restored solid");
    let (restored_faces, restored_volume) = kernel.shape_stats(shape).expect("measures");
    assert_eq!(
        restored_faces, faces,
        "the same solid, not merely a valid one"
    );
    assert!((restored_volume - volume).abs() < 1e-9);

    // And it answers to the same names as a computed one.
    let names = built
        .topology()
        .feature(plate.extrude)
        .expect("the restored extrusion is named");
    for segment in &plate.segments {
        assert_eq!(
            names.side(*segment).count(),
            1,
            "segment {segment} should name one face after a hit"
        );
    }
    assert_eq!(names.cap(CapSide::Start).expect("a start cap").len(), 1);
    assert_eq!(names.cap(CapSide::End).expect("an end cap").len(), 1);

    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}
