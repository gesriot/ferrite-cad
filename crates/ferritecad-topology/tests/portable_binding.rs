// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_kernel::{
    ArchiveSlot, ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint,
    Profile, ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane, SubShapeKind,
    mock::MockKernel,
};
use ferritecad_topology::generative::{Name, Selection};
use ferritecad_topology::{ModelNaming, archive_feature, restore_feature};
use ferritecad_types::{ContentHash, ErrorKind, ObjectId, Result, StableEntityId};
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
fn build(kernel: &mut MockKernel, plate: &Plate) -> ModelNaming {
    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("the mock builds");
    naming::bind(kernel, plate.feature, plate.request.profile(), &result).expect("complete names")
}

#[test]
fn names_survive_the_session_that_made_them() {
    let plate = plate().expect("a valid plate");
    let (archived, before) = {
        let mut writer = MockKernel::new();
        let map = build(&mut writer, &plate);
        let names = map.features[&plate.feature].bindings.clone();
        assert_eq!(names.len(), 26);
        (
            archive_feature(&mut writer, &map, plate.feature).expect("archives"),
            names,
        )
    };
    let mut reader = MockKernel::new();
    let mut restored =
        naming::prepared(plate.feature, plate.request.profile(), false).expect("model roots");
    restore_feature(&mut reader, &archived, &mut restored).expect("restores");
    let names = &restored.features[&plate.feature];
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        names.bindings.keys().collect::<Vec<_>>()
    );
    let mut all = BTreeSet::new();
    for (name, old) in before {
        let found = names.resolve(name).expect("same name resolves");
        assert_eq!(old.len(), 1);
        assert!(
            !old.contains(&found),
            "restored handles belong to the new session"
        );
        assert_eq!(found.kind(), name.kind);
        assert!(all.insert(found), "names never collapse onto a neighbour");
    }
    assert_eq!(all.len(), 26);
    assert_eq!(
        all.iter()
            .filter(|s| s.kind() == SubShapeKind::Face)
            .count(),
        6
    );
}

#[test]
fn a_restored_feature_still_refuses_a_name_it_never_had() {
    let plate = plate().expect("plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");
    let mut reader = MockKernel::new();
    let mut restored =
        naming::prepared(plate.feature, plate.request.profile(), false).expect("model roots");
    restore_feature(&mut reader, &archived, &mut restored).expect("restores");
    let names = &restored.features[&plate.feature];
    let stranger = Name {
        kind: SubShapeKind::Face,
        digest: ContentHash::of_bytes(b"absent"),
    };
    assert_eq!(
        names.resolve(stranger).expect_err("no neighbour").kind(),
        ErrorKind::Topology
    );
    for name in map.features[&plate.feature].bindings.keys() {
        assert_eq!(names.resolve(*name).expect("present").kind(), name.kind);
    }
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
    let empty = ModelNaming::new();
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

#[test]
fn every_name_keeps_its_kind_and_exact_selection() {
    let plate = plate().expect("plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);
    let named = &map.features[&plate.feature];
    let mut reached = BTreeSet::new();
    for name in named.bindings.keys() {
        let entity = named.resolve(*name).expect("exact");
        assert!(reached.insert(entity));
        assert_eq!(entity.kind(), name.kind);
        assert_eq!(
            named
                .resolve_set(*name, Selection::AllDescendants)
                .expect("family")
                .len(),
            1
        );
        for kind in [SubShapeKind::Face, SubShapeKind::Edge, SubShapeKind::Vertex] {
            if kind != name.kind {
                let wrong = Name {
                    kind,
                    digest: name.digest,
                };
                assert_eq!(
                    named.resolve(wrong).expect_err("wrong kind").kind(),
                    ErrorKind::Topology
                );
            }
        }
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
    for name in map.features[&plate.feature].bindings.keys() {
        let a = map.features[&plate.feature].resolve(*name).expect("cold");
        let b = restored.features[&plate.feature]
            .resolve(*name)
            .expect("cached");
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
fn exact_names_never_choose_among_ambiguous_or_absent_members() {
    let plate = plate().expect("plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);
    for (name, members) in &map.features[&plate.feature].bindings {
        let mut ambiguous = map.features[&plate.feature].clone();
        let first = *members.first().expect("member");
        let second = ambiguous
            .topology
            .entities(first.kind())
            .find(|s| *s != first)
            .expect("another entity");
        ambiguous
            .ancestry
            .get_mut(name)
            .expect("name")
            .insert(second);
        let refusal = ambiguous.resolve(*name).expect_err("never pick first");
        assert_eq!(refusal.kind(), ErrorKind::Topology);
        assert!(refusal.to_string().contains("refusing to choose"));
        assert_eq!(
            ambiguous
                .resolve_set(*name, Selection::AllDescendants)
                .expect("explicit set")
                .len(),
            2
        );
        ambiguous.ancestry.remove(name);
        assert_eq!(
            ambiguous.resolve(*name).expect_err("absent").kind(),
            ErrorKind::Topology
        );
    }
}

#[test]
fn foreign_or_disconnected_profile_joints_keep_their_refusals() {
    let plate = plate().expect("plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);
    for pair in [
        [plate.segments[0], plate.segments[2]],
        [plate.segments[0], StableEntityId::new()],
    ] {
        let edge = ferritecad_document::SweptEdge {
            feature: plate.feature,
            joint: ferritecad_types::ProfileJoint::new(pair[0], pair[1]).expect("joint"),
        };
        let error = map
            .swept_edge(plate.feature, edge)
            .expect_err("no invented joint");
        assert_eq!(error.kind(), ErrorKind::Topology);
        assert!(
            error
                .to_string()
                .contains("has 0 descendants; refusing to choose")
        );
    }
}
