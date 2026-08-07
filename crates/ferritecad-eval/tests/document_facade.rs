// SPDX-License-Identifier: MIT
//! Planning against a real document.
//!
//! The unit tests work on bare identifiers and edges. These check the one thing
//! they cannot: that the graph actually stored by the document layer — written
//! by the same sample flow the CLI uses — plans the way the pure functions say
//! it should.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::BTreeSet;

use ferritecad_document::{
    Body, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition, EntityKind,
    Expression, Extrude, ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch, SketchCurve,
    SketchGeometry, SolidOperation, TopologyRef,
};
use ferritecad_eval::DocumentGraph;
use ferritecad_types::{ObjectId, Result, StableEntityId, Transform};
use tempfile::TempDir;

/// The identifiers of the sample plate, so tests can name its parts.
struct Plate {
    plane: ObjectId,
    sketch: ObjectId,
    extrude: ObjectId,
    body: ObjectId,
}

/// The CLI's `create --sample` flow: plane, rectangular profile, extrusion,
/// body, and the topology references naming the extrusion's faces.
///
/// Kept in step with `ferritecad-cli/src/sample.rs` deliberately rather than
/// shared: the CLI is a binary crate, and a test that reaches into one would
/// couple the planner's tests to the command line's shape.
fn populate(document: &mut Document, width: f64, depth: f64, height: f64) -> Result<Plate> {
    let plate = Plate {
        plane: ObjectId::new(),
        sketch: ObjectId::new(),
        extrude: ObjectId::new(),
        body: ObjectId::new(),
    };

    let corners = [
        Point2::new(0.0, 0.0)?,
        Point2::new(width, 0.0)?,
        Point2::new(width, depth)?,
        Point2::new(0.0, depth)?,
    ];

    let mut curves = Vec::with_capacity(corners.len());
    for (index, start) in corners.iter().enumerate() {
        curves.push(SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Line {
                start: *start,
                end: corners[(index + 1) % corners.len()],
            },
        });
    }
    let first_segment = curves[0].id;

    document.write(|writer| {
        writer.put_object(
            plate.plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;

        writer.put_object(
            plate.sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane: plate.plane,
                curves,
            }),
        )?;
        writer.add_dependency(Dependency {
            dependent: plate.sketch,
            dependency: plate.plane,
            role: DependencyRole::Plane,
        })?;

        writer.put_object(
            plate.body,
            None,
            3,
            Some("Plate"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(plate.extrude),
            }),
        )?;

        writer.put_object(
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
        writer.add_dependency(Dependency {
            dependent: plate.extrude,
            dependency: plate.sketch,
            role: DependencyRole::Profile,
        })?;
        writer.add_dependency(Dependency {
            dependent: plate.body,
            dependency: plate.extrude,
            role: DependencyRole::BodyTip,
        })?;

        for side in [CapSide::Start, CapSide::End] {
            writer.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: plate.extrude,
                producer_feature: plate.extrude,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::ExtrudeCap { side },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })?;
        }

        writer.put_topology_ref(&TopologyRef {
            id: StableEntityId::new(),
            owner: plate.extrude,
            producer_feature: plate.extrude,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::ExtrudeSide {
                profile_segment: first_segment,
            },
            selection: SelectionRule::AllDerivedFrom {
                ancestor: first_segment,
            },
            fallback_signature: None,
        })?;

        Ok(())
    })?;

    Ok(plate)
}

fn sample_document() -> (TempDir, Document, Plate) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("part.fcad")).expect("creates");
    let plate = populate(&mut document, 60.0, 40.0, 10.0).expect("populates");
    (dir, document, plate)
}

#[test]
fn editing_the_sketch_rebuilds_the_extrusion_and_the_body() {
    let (_dir, document, plate) = sample_document();
    let graph = DocumentGraph::read(&document).expect("reads the graph");

    let plan = graph.plan(&[plate.sketch]).expect("plans");

    assert_eq!(plan.order(), &[plate.sketch, plate.extrude, plate.body]);
    assert!(
        !plan.contains(plate.plane),
        "the plane is upstream of the edit and must stay cached"
    );
    assert_eq!(
        plan.levels(),
        &[vec![plate.sketch], vec![plate.extrude], vec![plate.body]],
        "the sample is a chain, so every level holds one object"
    );
}

#[test]
fn editing_the_plane_rebuilds_the_whole_part() {
    let (_dir, document, plate) = sample_document();
    let graph = DocumentGraph::read(&document).expect("reads the graph");

    let plan = graph.plan(&[plate.plane]).expect("plans");

    assert_eq!(
        plan.order(),
        &[plate.plane, plate.sketch, plate.extrude, plate.body]
    );
    assert_eq!(plan.len(), 4);
}

#[test]
fn editing_only_the_extrusion_leaves_its_profile_cached() {
    let (_dir, document, plate) = sample_document();
    let graph = DocumentGraph::read(&document).expect("reads the graph");

    let plan = graph.plan(&[plate.extrude]).expect("plans");

    assert_eq!(plan.order(), &[plate.extrude, plate.body]);
    assert!(!plan.contains(plate.sketch));
    assert!(!plan.contains(plate.plane));
    assert_eq!(
        plan.levels(),
        &[vec![plate.extrude], vec![plate.body]],
        "a clean dependency imposes no wait"
    );
}

#[test]
fn a_facade_plan_agrees_with_the_documents_own_evaluation_order() {
    let (_dir, document, _plate) = sample_document();
    let graph = DocumentGraph::read(&document).expect("reads the graph");

    let full = graph.plan_full().expect("plans everything");
    let document_order = document.evaluation_order().expect("orders");

    assert_eq!(
        full.order(),
        document_order.as_slice(),
        "a cold rebuild must follow exactly the order the document reports"
    );
}

#[test]
fn the_dirty_set_matches_the_planned_objects() {
    let (_dir, document, plate) = sample_document();
    let graph = DocumentGraph::read(&document).expect("reads the graph");

    let dirty = graph.dirty_set(&[plate.sketch]).expect("propagates");
    let planned: BTreeSet<ObjectId> = graph
        .plan(&[plate.sketch])
        .expect("plans")
        .order()
        .iter()
        .copied()
        .collect();

    assert_eq!(dirty, planned);
}

#[test]
fn a_snapshot_is_stable_across_repeated_queries() {
    let (_dir, document, plate) = sample_document();
    let graph = DocumentGraph::read(&document).expect("reads the graph");

    let first = graph.plan(&[plate.sketch]).expect("plans");
    let second = graph.plan(&[plate.sketch]).expect("plans again");
    assert_eq!(first, second);

    // And a graph read again from the same unchanged document agrees.
    let reread = DocumentGraph::read(&document).expect("reads the graph again");
    assert_eq!(reread.plan(&[plate.sketch]).expect("plans"), first);
}

#[test]
fn a_reopened_document_plans_identically() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("part.fcad");

    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document, 60.0, 40.0, 10.0).expect("populates");
    let before = DocumentGraph::read(&document)
        .expect("reads")
        .plan(&[plate.sketch])
        .expect("plans");
    document.close().expect("closes");

    let reopened = Document::open(&path).expect("reopens");
    let after = DocumentGraph::read(&reopened)
        .expect("reads")
        .plan(&[plate.sketch])
        .expect("plans");

    assert_eq!(after, before, "a plan must not depend on session state");
}

#[test]
fn a_change_to_an_object_the_document_does_not_hold_is_refused() {
    let (_dir, document, _plate) = sample_document();
    let graph = DocumentGraph::read(&document).expect("reads the graph");

    let err = graph
        .plan(&[ObjectId::new()])
        .expect_err("a change we cannot place must not be silently dropped");
    assert_eq!(err.kind(), ferritecad_types::ErrorKind::Input);
}

#[test]
fn a_cycle_introduced_in_a_document_is_refused_rather_than_partially_planned() {
    let (_dir, mut document, plate) = sample_document();

    // The document layer stores the edge; it is the graph that becomes invalid.
    document
        .write(|writer| {
            writer.add_dependency(Dependency {
                dependent: plate.plane,
                dependency: plate.body,
                role: DependencyRole::TopologyReference,
            })
        })
        .expect("the edge is storable");

    let graph = DocumentGraph::read(&document).expect("reads the graph");
    let err = graph
        .plan(&[plate.sketch])
        .expect_err("a cyclic graph has no valid rebuild order");

    assert_eq!(err.kind(), ferritecad_types::ErrorKind::Input);
    assert!(err.to_string().contains("cycle"), "{err}");
}
