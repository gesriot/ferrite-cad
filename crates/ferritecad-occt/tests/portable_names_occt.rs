// SPDX-License-Identifier: MIT
//! Names carried across a session boundary, against real geometry.
//!
//! The mock proves the contract; this proves Open CASCADE keeps its side of
//! it. The archive is a compound the bridge writes deliberately, because the
//! obvious alternative does not work: a `BinTools_ShapeSet` index ignores the
//! location, and the two caps of a prism share a `TShape`, so both resolve to
//! one index. A reference to the top would have quietly returned the bottom.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{CapSide, EntityKind, SelectionRule, SemanticRole, TopologyRef};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_topology::{TopologyMap, archive_feature, resolve, restore_feature};
use ferritecad_types::{ErrorKind, ObjectId, Result, StableEntityId};

fn plate(height: f64) -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
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
    Ok((
        ExtrudeRequest::new(profile, ExtrudeExtent::blind(height)?, false),
        labels,
    ))
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
fn open_cascade_names_survive_the_session_that_made_them() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let (request, segments) = plate(10.0).expect("a valid plate");
    let feature = ObjectId::new();

    // Everything the writing session knows, taken before it ends.
    let (archived, before_faces, before_start) = {
        let mut writer = OcctKernel::new().expect("opens");
        let result = writer
            .extrude(&request, &OperationContext::default())
            .expect("Open CASCADE builds the plate");

        let mut map = TopologyMap::new();
        map.record_extrude(feature, request.profile(), &result)
            .expect("records");

        let start = resolve(&map, &cap_reference(feature, CapSide::Start))
            .expect("the start cap resolves while the session lives");
        let stats = writer.shape_stats(result.shape).expect("measures");

        let archived = archive_feature(&mut writer, &map, feature).expect("archives");
        writer.release(result.shape);
        (archived, stats, start)
    };

    // A session that never saw the original.
    let mut reader = OcctKernel::new().expect("opens");
    let mut restored = TopologyMap::new();
    restore_feature(&mut reader, &archived, &mut restored).expect("restores");

    let shape = restored
        .feature(feature)
        .and_then(|names| names.shape())
        .expect("the restore produced a shape");
    let after_faces = reader.shape_stats(shape).expect("measures");
    assert_eq!(before_faces.0, after_faces.0, "face count survives");
    assert!(
        (before_faces.1 - after_faces.1).abs() < 1e-9,
        "volume survives: {} vs {}",
        before_faces.1,
        after_faces.1
    );

    let start = resolve(&restored, &cap_reference(feature, CapSide::Start))
        .expect("the start cap resolves again");
    let end = resolve(&restored, &cap_reference(feature, CapSide::End))
        .expect("the end cap resolves again");

    assert_eq!(start.len(), 1);
    assert_eq!(end.len(), 1);

    // The heart of it. Both caps of a prism share a TShape in Open CASCADE, so
    // a naming scheme built on shape-set indices would return one face for
    // both of these.
    assert_ne!(start[0], end[0], "the two caps must not collapse into one");
    assert_ne!(
        start, before_start,
        "a restored handle belongs to the new session"
    );

    let mut all = Vec::new();
    for segment in &segments {
        let faces = resolve(&restored, &side_reference(feature, *segment))
            .unwrap_or_else(|e| panic!("side {segment} should resolve after restoring: {e}"));
        assert_eq!(faces.len(), 1);
        all.extend(faces);
    }
    all.extend(start);
    all.extend(end);
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 6, "six names, six distinct faces");

    reader.release(shape);
    assert_eq!(reader.live_shape_count(), 0);
}

#[test]
fn a_restored_open_cascade_shape_refuses_a_name_it_never_had() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let (request, segments) = plate(6.0).expect("a valid plate");
    let feature = ObjectId::new();

    let archived = {
        let mut writer = OcctKernel::new().expect("opens");
        let result = writer
            .extrude(&request, &OperationContext::default())
            .expect("builds");
        let mut map = TopologyMap::new();
        map.record_extrude(feature, request.profile(), &result)
            .expect("records");
        let archived = archive_feature(&mut writer, &map, feature).expect("archives");
        writer.release(result.shape);
        archived
    };

    let mut reader = OcctKernel::new().expect("opens");
    let mut restored = TopologyMap::new();
    restore_feature(&mut reader, &archived, &mut restored).expect("restores");

    let err = resolve(&restored, &side_reference(feature, StableEntityId::new()))
        .expect_err("that segment was never archived");
    assert_eq!(err.kind(), ErrorKind::Topology);

    // And the names that were archived still work, so the refusal is specific.
    assert_eq!(
        resolve(&restored, &side_reference(feature, segments[2]))
            .expect("resolves")
            .len(),
        1
    );

    if let Some(shape) = restored.feature(feature).and_then(|n| n.shape()) {
        reader.release(shape);
    }
}

#[test]
fn an_open_cascade_archive_is_not_readable_as_a_bare_shape() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let (request, _) = plate(4.0).expect("a valid plate");
    let feature = ObjectId::new();
    let mut kernel = OcctKernel::new().expect("opens");

    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("builds");
    let mut map = TopologyMap::new();
    map.record_extrude(feature, request.profile(), &result)
        .expect("records");
    let archived = archive_feature(&mut kernel, &map, feature).expect("archives");

    // An archive read as a plain shape would hand back the compound instead of
    // the solid — correct-looking geometry of the wrong thing.
    assert!(kernel.decode_shape(archived.blob()).is_err());

    let bare = kernel.encode_shape(result.shape).expect("encodes");
    assert!(
        kernel
            .decode_shape_with(&bare, &[ferritecad_kernel::ArchiveSlot::new(1)])
            .is_err(),
        "a bare blob has no sub-shape table"
    );

    kernel.release(result.shape);
}
