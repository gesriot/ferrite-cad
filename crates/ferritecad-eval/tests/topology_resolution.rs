// SPDX-License-Identifier: MIT
//! Resolving a document's stored names against a rebuild.
//!
//! These are the tests the whole naming design exists for. A reference must
//! survive an edit to the feature that produced it, survive the sketch being
//! stored in a different order, survive a save and reload — and must fail
//! loudly when the geometry it names is gone, rather than quietly selecting a
//! neighbour.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Body, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition, EntityKind,
    Expression, Extrude, ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch, SketchCurve,
    SketchGeometry, SolidOperation, TopologyRef,
};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{CancelToken, OperationContext, mock::MockKernel};
use ferritecad_types::{ErrorKind, ObjectId, Result, StableEntityId, Transform};

/// The plate, plus the references a document would store about it.
struct Plate {
    extrude: ObjectId,
    segments: Vec<StableEntityId>,
    start_cap: TopologyRef,
    end_cap: TopologyRef,
    /// One per segment, selected as a family.
    sides: Vec<TopologyRef>,
}

fn cap_reference(feature: ObjectId, side: CapSide) -> TopologyRef {
    TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: SemanticRole::ExtrudeCap { side },
        selection: SelectionRule::Exact,
        fallback_signature: None,
    }
}

fn side_reference(feature: ObjectId, segment: StableEntityId) -> TopologyRef {
    TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: SemanticRole::ExtrudeSide {
            profile_segment: segment,
        },
        selection: SelectionRule::AllDerivedFrom { ancestor: segment },
        fallback_signature: None,
    }
}

/// Writes the plate. `order` permutes how the curves are stored, which must
/// change nothing about what resolves.
fn populate(document: &mut Document, height: f64, order: &[usize]) -> Result<Plate> {
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();
    let body = ObjectId::new();
    let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();

    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let mut curves = Vec::new();
    for index in order {
        let start = corners[*index];
        let end = corners[(index + 1) % corners.len()];
        curves.push(SketchCurve {
            id: segments[*index],
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
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves,
                constraints: Vec::new(),
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
                    distance: Expression::constant(height)?,
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
        })?;

        // The document stores these; the rebuild resolves them.
        for reference in [
            cap_reference(extrude, CapSide::Start),
            cap_reference(extrude, CapSide::End),
        ] {
            w.put_topology_ref(&reference)?;
        }
        for segment in &segments {
            w.put_topology_ref(&side_reference(extrude, *segment))?;
        }
        Ok(())
    })?;

    Ok(Plate {
        extrude,
        start_cap: cap_reference(extrude, CapSide::Start),
        end_cap: cap_reference(extrude, CapSide::End),
        sides: segments
            .iter()
            .map(|s| side_reference(extrude, *s))
            .collect(),
        segments,
    })
}

fn sample(height: f64) -> (tempfile::TempDir, Document, Plate) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("plate.fcad")).expect("creates");
    let plate = populate(&mut document, height, &[0, 1, 2, 3]).expect("populates");
    (dir, document, plate)
}

#[test]
fn every_stored_reference_of_the_plate_resolves() {
    let (_dir, document, plate) = sample(10.0);
    let mut kernel = MockKernel::new();
    let built =
        rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("rebuilds");

    let start = built
        .resolve(&plate.start_cap)
        .expect("the start cap resolves");
    let end = built.resolve(&plate.end_cap).expect("the end cap resolves");
    assert_eq!(start.len(), 1);
    assert_eq!(end.len(), 1);
    assert_ne!(start[0], end[0]);

    for (index, reference) in plate.sides.iter().enumerate() {
        let faces = built
            .resolve(reference)
            .unwrap_or_else(|e| panic!("side {index} should resolve: {e}"));
        assert_eq!(faces.len(), 1);
    }

    // Six named faces, all distinct: four sides and two caps.
    let mut all: Vec<_> = plate
        .sides
        .iter()
        .flat_map(|r| built.resolve(r).expect("resolves"))
        .chain(start)
        .chain(end)
        .collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 6);

    built.release_all(&mut kernel);
}

#[test]
fn changing_the_extrusion_height_keeps_the_names_though_the_handles_change() {
    // The point of semantic naming: the geometry is rebuilt, every handle is
    // new, and the references still name the same things.
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("plate.fcad")).expect("creates");
    let plate = populate(&mut document, 10.0, &[0, 1, 2, 3]).expect("populates");

    let mut kernel = MockKernel::new();
    let before =
        rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("rebuilds");
    let before_cap = before.resolve(&plate.end_cap).expect("resolves");
    let before_side = before.resolve(&plate.sides[2]).expect("resolves");
    before.release_all(&mut kernel);

    let profile = document
        .object(plate.extrude)
        .expect("reads")
        .and_then(|o| match o.payload {
            ObjectPayload::Extrude(e) => Some(e.profile),
            _ => None,
        })
        .expect("the sample writes an extrude");

    document
        .write(|w| {
            w.put_object(
                plate.extrude,
                None,
                2,
                Some("Extrude1"),
                &ObjectPayload::Extrude(Extrude {
                    profile,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(25.0)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
                }),
            )?;
            Ok(())
        })
        .expect("the height changes");

    let after =
        rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("rebuilds");
    let after_cap = after.resolve(&plate.end_cap).expect("still resolves");
    let after_side = after.resolve(&plate.sides[2]).expect("still resolves");

    assert_eq!(after_cap.len(), 1);
    assert_eq!(after_side.len(), 1);
    assert_ne!(
        after_cap, before_cap,
        "a new rebuild issues new handles; that is why they are never stored"
    );
    assert_ne!(after_side, before_side);

    after.release_all(&mut kernel);
}

#[test]
fn the_order_curves_are_stored_in_does_not_change_what_resolves() {
    let context = OperationContext::default();

    let straight = tempfile::tempdir().expect("temp dir");
    let mut ordered = Document::create(straight.path().join("a.fcad")).expect("creates");
    let a = populate(&mut ordered, 8.0, &[0, 1, 2, 3]).expect("populates");

    let shuffled = tempfile::tempdir().expect("temp dir");
    let mut jumbled = Document::create(shuffled.path().join("b.fcad")).expect("creates");
    let b = populate(&mut jumbled, 8.0, &[2, 0, 3, 1]).expect("populates");

    let mut first = MockKernel::new();
    let one = rebuild_cold(&ordered, &mut first, &context).expect("rebuilds");
    let mut second = MockKernel::new();
    let other = rebuild_cold(&jumbled, &mut second, &context).expect("rebuilds");

    // Handles are session-local, so the comparison is on what resolves and how
    // many faces answer — never on the raw values.
    for index in 0..4 {
        assert_eq!(
            one.resolve(&a.sides[index]).expect("resolves").len(),
            other.resolve(&b.sides[index]).expect("resolves").len(),
            "segment {index} names the same number of faces either way"
        );
    }
    assert_eq!(
        one.resolve(&a.start_cap).expect("resolves").len(),
        other.resolve(&b.start_cap).expect("resolves").len()
    );

    one.release_all(&mut first);
    other.release_all(&mut second);
}

#[test]
fn references_resolve_the_same_after_a_save_and_reopen() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad");

    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document, 12.0, &[0, 1, 2, 3]).expect("populates");
    let stored: Vec<TopologyRef> = document.topology_refs().expect("reads");
    document.close().expect("closes");

    let reopened = Document::open(&path).expect("reopens");
    assert_eq!(
        reopened.topology_refs().expect("reads"),
        stored,
        "the references survive the round trip unchanged"
    );

    let mut kernel = MockKernel::new();
    let built =
        rebuild_cold(&reopened, &mut kernel, &OperationContext::default()).expect("rebuilds");

    // Resolve the references as the document stores them, not as the test
    // remembers them.
    let mut resolved = 0;
    for reference in reopened.topology_refs().expect("reads") {
        let faces = built
            .resolve(&reference)
            .unwrap_or_else(|e| panic!("stored reference {} should resolve: {e}", reference.id));
        assert!(!faces.is_empty());
        resolved += 1;
    }
    assert_eq!(resolved, 6, "two caps and four sides");
    assert_eq!(built.resolve(&plate.end_cap).expect("resolves").len(), 1);

    built.release_all(&mut kernel);
}

#[test]
fn a_reference_to_a_deleted_segment_is_lost_rather_than_retargeted() {
    let (_dir, mut document, plate) = sample(10.0);
    let removed = plate.segments[1];

    // Redraw the profile as a triangle: the referenced segment is gone.
    let sketch_id = document
        .objects()
        .expect("reads")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .map(|o| o.id)
        .expect("the sample writes a sketch");
    let plane_id = document
        .objects()
        .expect("reads")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::DatumPlane(_)))
        .map(|o| o.id)
        .expect("the sample writes a datum");

    let corners = [(0.0, 0.0), (60.0, 0.0), (0.0, 40.0)];
    let kept: Vec<StableEntityId> = vec![plate.segments[0], plate.segments[2], plate.segments[3]];
    let mut curves = Vec::new();
    for (index, start) in corners.iter().enumerate() {
        let end = corners[(index + 1) % corners.len()];
        curves.push(SketchCurve {
            id: kept[index],
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(start.0, start.1).expect("finite"),
                end: Point2::new(end.0, end.1).expect("finite"),
            },
        });
    }

    document
        .write(|w| {
            w.put_object(
                sketch_id,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(Sketch {
                    plane: plane_id,
                    curves,
                    constraints: Vec::new(),
                }),
            )?;
            Ok(())
        })
        .expect("the sketch is redrawn");

    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("the triangle still builds");

    let err = built
        .resolve(&side_reference(plate.extrude, removed))
        .expect_err("the segment it names is gone");

    // A lost reference, reported. Emphatically not one of the three faces that
    // are there.
    assert_eq!(err.kind(), ErrorKind::Topology);

    // The segments that survived still resolve, so the failure is about the
    // removed one and not about the rebuild.
    for segment in &kept {
        assert_eq!(
            built
                .resolve(&side_reference(plate.extrude, *segment))
                .expect("a surviving segment resolves")
                .len(),
            1
        );
    }

    built.release_all(&mut kernel);
}

#[test]
fn a_contradictory_reference_is_an_input_error_not_a_lost_one() {
    let (_dir, document, plate) = sample(10.0);
    let mut kernel = MockKernel::new();
    let built =
        rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("rebuilds");

    // Expecting an edge from a role that always names a face.
    let mut wrong_kind = plate.start_cap.clone();
    wrong_kind.expected_kind = EntityKind::Edge;
    assert_eq!(
        built
            .resolve(&wrong_kind)
            .expect_err("caps are faces")
            .kind(),
        ErrorKind::Input
    );

    // A family selection whose ancestor is not the segment named by the role.
    let mut wrong_ancestor = plate.sides[0].clone();
    wrong_ancestor.selection = SelectionRule::AllDerivedFrom {
        ancestor: plate.segments[3],
    };
    assert_eq!(
        built
            .resolve(&wrong_ancestor)
            .expect_err("the role and the rule disagree")
            .kind(),
        ErrorKind::Input
    );

    built.release_all(&mut kernel);
}

#[test]
fn two_sessions_agree_on_semantics_and_cardinality_not_on_handles() {
    let (_dir, document, plate) = sample(10.0);
    let context = OperationContext::default();

    let mut first = MockKernel::new();
    let one = rebuild_cold(&document, &mut first, &context).expect("rebuilds");
    let mut second = MockKernel::new();
    let other = rebuild_cold(&document, &mut second, &context).expect("rebuilds");

    for reference in std::iter::once(&plate.start_cap)
        .chain(std::iter::once(&plate.end_cap))
        .chain(plate.sides.iter())
    {
        let a = one.resolve(reference).expect("resolves");
        let b = other.resolve(reference).expect("resolves");
        assert_eq!(a.len(), b.len(), "the same name selects the same count");
        assert_ne!(a, b, "and two sessions never share a handle");
    }

    one.release_all(&mut first);
    other.release_all(&mut second);
}

#[test]
fn a_cancelled_rebuild_leaves_no_shapes_and_no_names() {
    let (_dir, document, plate) = sample(10.0);
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
    let _ = plate;
}

#[test]
fn an_empty_rebuild_resolves_nothing_rather_than_something() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = Document::create(dir.path().join("empty.fcad")).expect("creates");

    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("an empty document is valid");

    assert!(built.topology().is_empty());
    let err = built
        .resolve(&cap_reference(ObjectId::new(), CapSide::Start))
        .expect_err("nothing was built");
    assert_eq!(err.kind(), ErrorKind::Topology);

    built.release_all(&mut kernel);
}
