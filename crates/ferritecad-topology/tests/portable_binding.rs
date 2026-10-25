// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::{CapSide, EntityKind, SelectionRule, SemanticRole, TopologyRef};
use ferritecad_kernel::{
    ArchiveSlot, ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint,
    Profile, ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane, SubShapeKind,
    mock::MockKernel,
};
use ferritecad_topology::{TopologyMap, archive_feature, resolve, restore_feature};
use ferritecad_types::{ErrorKind, ObjectId, ProfileJoint, Result, StableEntityId};
use std::collections::BTreeSet;
#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;
struct Plate {
    request: ExtrudeRequest,
    segments: Vec<StableEntityId>,
    feature: ObjectId,
}
fn plate() -> Result<Plate> {
    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let points: Vec<PlanarPoint> = corners
        .iter()
        .map(|(x, y)| PlanarPoint::new(*x, *y))
        .collect::<Result<_>>()?;

    let mut segments = Vec::new();
    let mut labels = Vec::new();
    for (index, start) in points.iter().enumerate() {
        let label = StableEntityId::new();
        labels.push(label);
        segments.push(ProfileSegment::new(
            label,
            SegmentGeometry::line(*start, points[(index + 1) % points.len()])?,
        ));
    }

    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(segments)?,
        Vec::new(),
    )?;

    Ok(Plate {
        request: ExtrudeRequest::new(profile, ExtrudeExtent::blind(10.0)?, false),
        segments: labels,
        feature: ObjectId::new(),
    })
}
fn build(kernel: &mut MockKernel, plate: &Plate) -> TopologyMap {
    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("the mock builds");
    naming::bind(kernel, plate.feature, plate.request.profile(), &result).expect("complete names")
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

#[test]
fn names_survive_the_session_that_made_them() {
    let plate = plate().expect("a valid plate");

    // Everything the writing session knows, gathered before it ends.
    let (archived, before) = {
        let mut writer = MockKernel::new();
        let map = build(&mut writer, &plate);

        let start = resolve(&map, &cap_reference(plate.feature, CapSide::Start))
            .expect("the start cap resolves while the session lives");
        let sides: Vec<usize> = plate
            .segments
            .iter()
            .map(|s| {
                resolve(&map, &side_reference(plate.feature, *s))
                    .expect("a side resolves")
                    .len()
            })
            .collect();

        let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");
        (archived, (start, sides))
    };

    // A different session, with no knowledge of the first.
    let mut reader = MockKernel::new();
    let mut restored =
        naming::prepared(plate.feature, plate.request.profile(), false).expect("model roots");
    restore_feature(&mut reader, &archived, &mut restored).expect("restores");

    let start = resolve(&restored, &cap_reference(plate.feature, CapSide::Start))
        .expect("the start cap resolves again");
    let end = resolve(&restored, &cap_reference(plate.feature, CapSide::End))
        .expect("the end cap resolves again");
    assert_eq!(start.len(), 1);
    assert_eq!(end.len(), 1);
    assert_ne!(start[0], end[0], "the two caps are different faces");

    for (index, segment) in plate.segments.iter().enumerate() {
        let faces = resolve(&restored, &side_reference(plate.feature, *segment))
            .unwrap_or_else(|e| panic!("side {index} should resolve after restoring: {e}"));
        assert_eq!(faces.len(), before.1[index]);
    }

    // Same names, same counts — and emphatically not the same handles.
    assert_eq!(start.len(), before.0.len());
    assert_ne!(
        start, before.0,
        "a restored face is a handle of the new session"
    );

    // Six names, six distinct faces: nothing collapsed onto one.
    let mut all: Vec<_> = plate
        .segments
        .iter()
        .flat_map(|s| resolve(&restored, &side_reference(plate.feature, *s)).expect("resolves"))
        .chain(start)
        .chain(end)
        .collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 6);
}

#[test]
fn a_restored_feature_still_refuses_a_name_it_never_had() {
    let plate = plate().expect("a valid plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");

    let mut reader = MockKernel::new();
    let mut restored =
        naming::prepared(plate.feature, plate.request.profile(), false).expect("model roots");
    restore_feature(&mut reader, &archived, &mut restored).expect("restores");

    // A segment that was never part of this profile. Restoring must not have
    // invented a name, and the resolver must not hand back a neighbour.
    let stranger = StableEntityId::new();
    let err = resolve(&restored, &side_reference(plate.feature, stranger))
        .expect_err("that name was never archived");
    assert_eq!(err.kind(), ErrorKind::Topology);

    // The names that were archived still resolve, so the refusal is about the
    // stranger and not about the restore.
    assert_eq!(
        resolve(&restored, &side_reference(plate.feature, plate.segments[0]))
            .expect("resolves")
            .len(),
        1
    );
}

#[test]
fn an_archive_from_another_kernel_is_refused() {
    let plate = plate().expect("a valid plate");
    let mut writer = MockKernel::with_version("1.0.0");
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");

    let mut newer = MockKernel::with_version("2.0.0");
    let mut restored =
        naming::prepared(plate.feature, plate.request.profile(), false).expect("model roots");
    let err = restore_feature(&mut newer, &archived, &mut restored)
        .expect_err("a different kernel build may compute different geometry");

    assert_eq!(err.kind(), ErrorKind::Kernel);
    assert!(restored.is_empty(), "a refused restore leaves no names");
}

#[test]
fn a_bare_blob_carries_no_names_and_an_archive_is_not_a_bare_blob() {
    let plate = plate().expect("a valid plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);
    let archived = archive_feature(&mut kernel, &map, plate.feature).expect("archives");

    // An archive read as a plain shape would restore the wrong thing quietly,
    // so the two formats are made unreadable as each other.
    assert!(
        kernel.decode_shape(archived.blob()).is_err(),
        "an archive must not decode as a bare shape"
    );

    let shape = map
        .feature(plate.feature)
        .and_then(|names| names.shape())
        .expect("the feature built a shape");
    let bare = kernel.encode_shape(shape).expect("encodes");
    assert!(
        kernel
            .decode_shape_with(&bare, &[ArchiveSlot::new(1)])
            .is_err(),
        "a bare blob has no sub-shape table to read"
    );
}

#[test]
fn archiving_a_feature_that_produced_nothing_is_refused() {
    let empty = TopologyMap::new();
    let mut kernel = MockKernel::new();

    let err = archive_feature(&mut kernel, &empty, ObjectId::new())
        .expect_err("there is nothing to archive");
    assert_eq!(err.kind(), ErrorKind::Topology);
}

#[test]
fn a_sub_shape_from_another_shape_is_refused_by_the_kernel() {
    let plate = plate().expect("a valid plate");
    let mut kernel = MockKernel::new();

    let first = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    let second = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");

    // Archiving one shape while naming a face of another would produce an
    // archive whose names point outside it.
    let foreign = naming::cap(&mut kernel, second.shape, false);
    let err = kernel
        .encode_shape_with(first.shape, &[foreign])
        .expect_err("the face belongs to the other solid");
    assert_eq!(err.kind(), ErrorKind::Kernel);
}

#[test]
fn the_root_slot_is_not_a_sub_shape() {
    let plate = plate().expect("a valid plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);
    let archived = archive_feature(&mut kernel, &map, plate.feature).expect("archives");

    let before = kernel.live_shape_count();
    let err = kernel
        .decode_shape_with(archived.blob(), &[ArchiveSlot::ROOT])
        .expect_err("slot zero is the shape itself");
    assert_eq!(err.kind(), ErrorKind::Kernel);
    assert_eq!(kernel.live_shape_count(), before);
}

fn references(plate: &Plate) -> Vec<TopologyRef> {
    let mut refs = Vec::new();
    for side in [CapSide::Start, CapSide::End] {
        refs.push(cap_reference(plate.feature, side));
        for (i, segment) in plate.segments.iter().enumerate() {
            let joint = ProfileJoint::new(*segment, plate.segments[(i + 1) % 4]).expect("joint");
            for (kind, role) in [
                (
                    EntityKind::Edge,
                    SemanticRole::ExtrudeCapEdge {
                        side,
                        profile_segment: *segment,
                    },
                ),
                (
                    EntityKind::Vertex,
                    SemanticRole::ExtrudeCapVertex { side, joint },
                ),
            ] {
                let mut reference = cap_reference(plate.feature, side);
                reference.expected_kind = kind;
                reference.output_role = role;
                refs.push(reference);
            }
        }
    }
    for (i, segment) in plate.segments.iter().enumerate() {
        let mut side = side_reference(plate.feature, *segment);
        side.selection = SelectionRule::Exact;
        refs.push(side);
        let mut edge = cap_reference(plate.feature, CapSide::Start);
        edge.expected_kind = EntityKind::Edge;
        edge.output_role = SemanticRole::ExtrudeSweepEdge {
            joint: ProfileJoint::new(*segment, plate.segments[(i + 1) % 4]).expect("joint"),
        };
        refs.push(edge);
    }
    refs
}
#[test]
fn every_reference_keeps_its_kind_exact_selection_and_missing_reference_refusals() {
    let plate = plate().expect("plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);
    let mut reached = BTreeSet::new();
    for reference in references(&plate) {
        let entities = resolve(&map, &reference).expect("exact");
        assert_eq!(entities.len(), 1);
        assert!(reached.insert(entities[0]));
        for kind in [EntityKind::Face, EntityKind::Edge, EntityKind::Vertex] {
            if kind == reference.expected_kind {
                continue;
            }
            let mut wrong = reference.clone();
            wrong.expected_kind = kind;
            assert_eq!(
                resolve(&map, &wrong).expect_err("wrong kind").kind(),
                ErrorKind::Input
            );
        }
        if !matches!(reference.output_role, SemanticRole::ExtrudeSide { .. }) {
            let mut wrong = reference.clone();
            wrong.selection = SelectionRule::AllDerivedFrom {
                ancestor: plate.segments[0],
            };
            assert_eq!(
                resolve(&map, &wrong).expect_err("exact only").kind(),
                ErrorKind::Input
            );
        }
        let mut missing = reference.clone();
        missing.producer_feature = ObjectId::new();
        let refusal = resolve(&map, &missing).expect_err("missing feature");
        assert_eq!(refusal.kind(), ErrorKind::Topology);
        assert!(
            refusal.to_string().contains(
                if matches!(
                    reference.output_role,
                    SemanticRole::ExtrudeCap { .. } | SemanticRole::ExtrudeSide { .. }
                ) {
                    "produced none"
                } else {
                    "produced nothing"
                }
            ),
            "{refusal}"
        );
    }
    assert_eq!(reached.len(), 26);
}
#[test]
fn every_prism_meaning_round_trips_and_portable_names_contain_no_session_identity() {
    let plate = plate().expect("plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archive = archive_feature(&mut writer, &map, plate.feature).expect("archive");
    assert_eq!(
        archive,
        archive_feature(&mut writer, &map, plate.feature).expect("repeat")
    );
    let bytes = archive.encode().expect("bytes");
    let shape = map.features[&plate.feature].shape().expect("shape");
    for forbidden in [shape.to_string(), shape.session().to_string()] {
        assert!(!String::from_utf8_lossy(&bytes).contains(&forbidden));
    }
    for (_, name) in archive.entities() {
        let text = format!("{name:?}");
        for forbidden in ["session", "shape#", "Handle", "PickId", "occurrence"] {
            assert!(!text.contains(forbidden));
        }
    }
    let mut reader = MockKernel::new();
    let mut restored =
        naming::prepared(plate.feature, plate.request.profile(), false).expect("roots");
    restore_feature(&mut reader, &archive, &mut restored).expect("restore");
    let mut old = BTreeSet::new();
    let mut new = BTreeSet::new();
    for reference in references(&plate) {
        let a = resolve(&map, &reference).expect("cold")[0];
        let b = resolve(&restored, &reference).expect("cached")[0];
        assert_ne!(a, b);
        assert_eq!(
            map.features[&plate.feature].entities[&a],
            restored.features[&plate.feature].entities[&b]
        );
        assert!(old.insert(a));
        assert!(new.insert(b));
    }
    assert_eq!(old.len(), 26);
    assert_eq!(new.len(), 26);
    assert!(
        reader
            .naming_history(restored.features[&plate.feature].shape().expect("shape"))
            .is_err()
    );
}
#[test]
fn exact_references_never_choose_among_ambiguous_members_or_use_geometric_hints() {
    let plate = plate().expect("plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);
    for reference in references(&plate) {
        let mut ambiguous = map.clone();
        let named = ambiguous.features.get_mut(&plate.feature).expect("feature");
        let first = resolve(&map, &reference).expect("first")[0];
        let second = named
            .topology
            .entities(first.kind())
            .find(|s| *s != first)
            .expect("another entity");
        if first.kind() == SubShapeKind::Face {
            for members in named.ancestry.values_mut() {
                if members.contains(&first) {
                    members.insert(second);
                }
            }
        } else {
            let faces = named.topology.adjacent_faces[&first].clone();
            named.topology.adjacent_faces.insert(second, faces);
        }
        let refusal = resolve(&ambiguous, &reference).expect_err("never pick first");
        assert_eq!(refusal.kind(), ErrorKind::Topology);
        assert!(
            refusal.to_string().contains(
                if matches!(reference.output_role, SemanticRole::ExtrudeSweepEdge { .. }) {
                    "produced none"
                } else {
                    "refusing rather than choosing"
                }
            ),
            "{refusal}"
        );
        let mut absent = map.clone();
        absent
            .features
            .get_mut(&plate.feature)
            .expect("feature")
            .topology
            .adjacent_faces
            .remove(&first);
        for members in absent
            .features
            .get_mut(&plate.feature)
            .expect("feature")
            .ancestry
            .values_mut()
        {
            members.remove(&first);
        }
        let mut hint = reference.clone();
        hint.fallback_signature = Some(ferritecad_document::GeomSignature {
            kind: reference.expected_kind,
            measure: 0.,
            centroid: ferritecad_types::Point3::ORIGIN,
        });
        let refusal = resolve(&absent, &hint).expect_err("no geometric fallback");
        assert_eq!(refusal.kind(), ErrorKind::Topology);
        assert!(refusal.to_string().contains("produced none"), "{refusal}");
    }
}

#[test]
fn unsupported_roles_and_foreign_or_disconnected_profile_joints_keep_their_refusals() {
    let plate = plate().expect("plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);
    let segment = StableEntityId::new();
    for role in [
        SemanticRole::SketchSegment { segment },
        SemanticRole::FilletFace {
            source_edge: segment,
        },
    ] {
        let mut reference = cap_reference(plate.feature, CapSide::Start);
        reference.output_role = role;
        let error = resolve(&map, &reference).expect_err("unsupported role");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert!(error.to_string().contains(&reference.id.to_string()));
    }
    for joint in [
        ProfileJoint::new(plate.segments[0], plate.segments[2]).expect("disconnected"),
        ProfileJoint::new(plate.segments[0], segment).expect("foreign"),
    ] {
        for (kind, role) in [
            (EntityKind::Edge, SemanticRole::ExtrudeSweepEdge { joint }),
            (
                EntityKind::Vertex,
                SemanticRole::ExtrudeCapVertex {
                    side: CapSide::End,
                    joint,
                },
            ),
        ] {
            let mut reference = cap_reference(plate.feature, CapSide::Start);
            reference.expected_kind = kind;
            reference.output_role = role;
            let error = resolve(&map, &reference).expect_err("no invented joint");
            assert_eq!(error.kind(), ErrorKind::Topology);
            assert!(error.to_string().contains("produced none"));
        }
    }
}
