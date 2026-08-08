// SPDX-License-Identifier: MIT
//! Carrying names across a session boundary.
//!
//! The question this slice exists to answer: after the session that built a
//! solid is gone, can the references a document stores still be resolved
//! against geometry restored from a blob? These tests answer it end to end,
//! and check the ways it must refuse rather than guess.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::{collections::BTreeMap, mem::size_of};

use ferritecad_document::{CapSide, EntityKind, SelectionRule, SemanticRole, TopologyRef};
use ferritecad_kernel::{
    ArchiveSlot, BrepBlob, ExtrudeExtent, ExtrudeRequest, GeometryKernel, KernelIdentity,
    OperationContext, PlanarPoint, Profile, ProfileLoop, ProfileSegment, SegmentGeometry,
    SketchPlane, mock::MockKernel,
};
use ferritecad_topology::{
    ArchivedFeature, BoundName, TopologyMap, archive_feature, resolve, restore_feature,
};
use ferritecad_types::{ContentHash, ErrorKind, ObjectId, Result, StableEntityId};

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
    let mut map = TopologyMap::new();
    map.record_extrude(plate.feature, plate.request.profile(), &result)
        .expect("records");
    map
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
    let mut restored = TopologyMap::new();
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
fn the_order_of_the_binding_table_does_not_change_what_resolves() {
    let plate = plate().expect("a valid plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");

    // Rebuild the table with its entries handed over backwards. The table is
    // ordered internally, so the answers must not move.
    let mut reversed: Vec<(BoundName, ArchiveSlot)> = archived.bindings().collect();
    reversed.reverse();
    let shuffled = ArchivedFeature::from_parts(
        archived.producer(),
        archived.blob().clone(),
        archived.blob().content_hash(),
        reversed,
    )
    .expect("the same table, given in another order");

    let mut one = TopologyMap::new();
    let mut first = MockKernel::new();
    restore_feature(&mut first, &archived, &mut one).expect("restores");

    let mut other = TopologyMap::new();
    let mut second = MockKernel::new();
    restore_feature(&mut second, &shuffled, &mut other).expect("restores");

    for side in [CapSide::Start, CapSide::End] {
        let a = resolve(&one, &cap_reference(plate.feature, side)).expect("resolves");
        let b = resolve(&other, &cap_reference(plate.feature, side)).expect("resolves");
        assert_eq!(a.len(), b.len());
        // The handles differ because the sessions differ; what must agree is
        // which slot each name took.
        assert_eq!(
            archived.slot(BoundName::cap(side).expect("a known side")),
            shuffled.slot(BoundName::cap(side).expect("a known side"))
        );
    }
    for segment in &plate.segments {
        let name = BoundName::Side {
            profile_segment: *segment,
        };
        assert_eq!(archived.slot(name), shuffled.slot(name));
        assert_eq!(
            resolve(&one, &side_reference(plate.feature, *segment))
                .expect("resolves")
                .len(),
            resolve(&other, &side_reference(plate.feature, *segment))
                .expect("resolves")
                .len()
        );
    }
}

#[test]
fn archiving_twice_produces_the_same_table() {
    let plate = plate().expect("a valid plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);

    let one = archive_feature(&mut kernel, &map, plate.feature).expect("archives");
    let other = archive_feature(&mut kernel, &map, plate.feature).expect("archives");

    assert_eq!(
        one.bindings().collect::<Vec<_>>(),
        other.bindings().collect::<Vec<_>>()
    );
    assert_eq!(one.blob().content_hash(), other.blob().content_hash());
}

#[test]
fn a_restored_feature_still_refuses_a_name_it_never_had() {
    let plate = plate().expect("a valid plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");

    let mut reader = MockKernel::new();
    let mut restored = TopologyMap::new();
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
    let mut restored = TopologyMap::new();
    let err = restore_feature(&mut newer, &archived, &mut restored)
        .expect_err("a different kernel build may compute different geometry");

    assert_eq!(err.kind(), ErrorKind::Kernel);
    assert!(restored.is_empty(), "a refused restore leaves no names");
}

#[test]
fn a_slot_outside_the_archive_is_refused() {
    let plate = plate().expect("a valid plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");

    let bogus = ArchivedFeature::from_parts(
        archived.producer(),
        archived.blob().clone(),
        archived.blob().content_hash(),
        [(BoundName::StartCap, ArchiveSlot::new(9_999))],
    )
    .expect("the table itself is well formed");

    let mut reader = MockKernel::new();
    let mut restored = TopologyMap::new();
    let err = restore_feature(&mut reader, &bogus, &mut restored)
        .expect_err("the slot addresses nothing in this archive");
    assert_eq!(err.kind(), ErrorKind::Kernel);
    assert_eq!(
        reader.live_shape_count(),
        0,
        "a failed decode must not leave an unreachable shape"
    );
}

#[test]
fn two_names_for_one_live_face_are_refused_before_archiving() {
    let plate = plate().expect("a valid plate");
    let mut kernel = MockKernel::new();
    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");

    let shared = result.start_cap[0];
    let mut aliased = TopologyMap::new();
    aliased
        .record_restored(
            plate.feature,
            result.shape,
            &[shared],
            &[shared],
            &BTreeMap::new(),
        )
        .expect("the runtime map can express ancestry aliases");

    let err = archive_feature(&mut kernel, &aliased, plate.feature)
        .expect_err("one archived face may not answer to two durable names");
    assert_eq!(err.kind(), ErrorKind::Topology);
    assert!(err.to_string().contains("silent alias"));
}

#[test]
fn two_slots_that_decode_to_one_face_are_refused_and_released() {
    let plate = plate().expect("a valid plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");

    // The mock archive ends with one u64 face identifier per binding. Make
    // the second slot carry the first slot's face while leaving both slot
    // numbers distinct, which bypasses a table-only duplicate-slot check.
    let binding_count = archived.bindings().len();
    assert!(binding_count >= 2);
    let mut bytes = archived.blob().bytes().to_vec();
    let entries_start = bytes.len() - binding_count * size_of::<u64>();
    let first = bytes[entries_start..entries_start + size_of::<u64>()].to_vec();
    bytes[entries_start + size_of::<u64>()..entries_start + 2 * size_of::<u64>()]
        .copy_from_slice(&first);

    let blob = BrepBlob::new(writer.identity().clone(), bytes);
    let hash = blob.content_hash();
    let aliased = ArchivedFeature::from_parts(archived.producer(), blob, hash, archived.bindings())
        .expect("the slots are distinct, so only decoded geometry exposes the alias");

    let mut reader = MockKernel::new();
    let mut restored = TopologyMap::new();
    let err = restore_feature(&mut reader, &aliased, &mut restored)
        .expect_err("two names must not resolve to one decoded face");
    assert_eq!(err.kind(), ErrorKind::Topology);
    assert!(err.to_string().contains("silent alias"));
    assert!(restored.is_empty());
    assert_eq!(
        reader.live_shape_count(),
        0,
        "the decoded shape is released"
    );
}

#[test]
fn an_invalid_face_inside_the_mock_archive_is_refused_before_storage() {
    let plate = plate().expect("a valid plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");

    let binding_count = archived.bindings().len();
    let mut bytes = archived.blob().bytes().to_vec();
    let entries_start = bytes.len() - binding_count * size_of::<u64>();
    bytes[entries_start..entries_start + size_of::<u64>()].copy_from_slice(&u64::MAX.to_le_bytes());

    let blob = BrepBlob::new(writer.identity().clone(), bytes);
    let hash = blob.content_hash();
    let damaged = ArchivedFeature::from_parts(archived.producer(), blob, hash, archived.bindings())
        .expect("the binding table itself is still well formed");

    let mut reader = MockKernel::new();
    let mut restored = TopologyMap::new();
    assert!(restore_feature(&mut reader, &damaged, &mut restored).is_err());
    assert!(restored.is_empty());
    assert_eq!(reader.live_shape_count(), 0);
}

#[test]
fn a_corrupt_archive_is_refused_rather_than_misread() {
    let plate = plate().expect("a valid plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");
    let identity = writer.identity().clone();

    let mut truncated = archived.blob().bytes().to_vec();
    truncated.truncate(truncated.len() / 2);
    let damaged = BrepBlob::new(identity, truncated);

    // The table has to be rebuilt against the damaged blob's own checksum,
    // otherwise the pairing check refuses it before the blob is even read.
    let hash = damaged.content_hash();
    let paired =
        ArchivedFeature::from_parts(archived.producer(), damaged, hash, archived.bindings())
            .expect("the table is well formed");

    let mut reader = MockKernel::new();
    let mut restored = TopologyMap::new();
    assert!(restore_feature(&mut reader, &paired, &mut restored).is_err());
    assert!(restored.is_empty());
}

#[test]
fn a_table_paired_with_the_wrong_archive_is_refused() {
    let plate = plate().expect("a valid plate");
    let mut kernel = MockKernel::new();
    let map = build(&mut kernel, &plate);
    let archived = archive_feature(&mut kernel, &map, plate.feature).expect("archives");

    let err = ArchivedFeature::from_parts(
        archived.producer(),
        archived.blob().clone(),
        ContentHash::of_bytes(b"a different archive entirely"),
        archived.bindings(),
    )
    .expect_err("the pair does not belong together");
    assert_eq!(err.kind(), ErrorKind::Topology);
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
    let err = kernel
        .encode_shape_with(first.shape, &[second.start_cap[0]])
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
fn a_kernel_identity_is_checked_before_any_geometry_is_read() {
    let plate = plate().expect("a valid plate");
    let mut writer = MockKernel::new();
    let map = build(&mut writer, &plate);
    let archived = archive_feature(&mut writer, &map, plate.feature).expect("archives");

    let foreign = BrepBlob::new(
        KernelIdentity::new("occt", "8.0.1", "not this bridge").expect("valid"),
        archived.blob().bytes().to_vec(),
    );
    let hash = foreign.content_hash();
    let paired =
        ArchivedFeature::from_parts(archived.producer(), foreign, hash, archived.bindings())
            .expect("the table is well formed");

    let mut reader = MockKernel::new();
    let mut restored = TopologyMap::new();
    let err = restore_feature(&mut reader, &paired, &mut restored)
        .expect_err("another kernel wrote this");
    assert_eq!(err.kind(), ErrorKind::Kernel);
}
