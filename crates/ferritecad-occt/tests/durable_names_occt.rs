// SPDX-License-Identifier: MIT
//! Real B-Rep bytes through a real file, and back.
//!
//! The codec is kernel-agnostic and the mock proves its rules, but the bytes a
//! mock writes are a few dozen of our own making. What actually goes into the
//! sidecar is Open CASCADE's binary B-Rep, and it travels through SQLite blob
//! chunks on the way. This checks that path end to end with the geometry the
//! product will really store.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    CacheStore, CapSide, EntityKind, SelectionRule, SemanticRole, TopologyRef,
};
use ferritecad_eval::{load_extrude_archive, store_extrude_archive};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_topology::{TopologyMap, archive_feature, resolve, restore_feature};
use ferritecad_types::{DocumentId, ObjectId, Result, StableEntityId};

fn plate() -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
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

    Ok((
        ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(segments)?,
                Vec::new(),
            )?,
            ExtrudeExtent::blind(10.0)?,
            false,
        ),
        labels,
    ))
}

fn face_reference(feature: ObjectId, role: SemanticRole, selection: SelectionRule) -> TopologyRef {
    TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: role,
        selection,
        fallback_signature: None,
    }
}

#[test]
fn open_cascade_geometry_and_its_names_survive_a_file() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad-cache");
    let document = DocumentId::new();
    let (request, segments) = plate().expect("a valid plate");
    let feature = ObjectId::new();
    let context = OperationContext::default();

    // The writing run. Everything it holds is gone by the closing brace.
    let (before_stats, blob_len) = {
        let mut kernel = OcctKernel::new().expect("opens");
        let mut cache = CacheStore::open(
            &path,
            document,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("the sidecar opens");

        let result = kernel.extrude(&request, &context).expect("builds");
        let mut map = TopologyMap::new();
        map.record_extrude(feature, request.profile(), &result)
            .expect("records");
        let stats = kernel.shape_stats(result.shape).expect("measures");

        let archived = archive_feature(&mut kernel, &map, feature).expect("archives");
        let blob_len = archived.blob().bytes().len();
        store_extrude_archive(&mut cache, kernel.identity(), &request, &context, &archived)
            .expect("stores");

        kernel.release(result.shape);
        (stats, blob_len)
    };
    assert!(blob_len > 0, "Open CASCADE wrote an empty archive");

    // The reading run.
    let mut kernel = OcctKernel::new().expect("opens");
    let cache = CacheStore::open(
        &path,
        document,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("the sidecar opens again");

    let archived = load_extrude_archive(&cache, kernel.identity(), &request, &context, feature)
        .expect("reads")
        .expect("the entry the previous run wrote is there");
    assert_eq!(archived.blob().bytes().len(), blob_len);

    let mut restored = TopologyMap::new();
    restore_feature(&mut kernel, &archived, &mut restored).expect("restores");

    let shape = restored
        .feature(feature)
        .and_then(|names| names.shape())
        .expect("a restored shape");
    let after_stats = kernel.shape_stats(shape).expect("measures");
    assert_eq!(
        before_stats.0, after_stats.0,
        "face count survives the file"
    );
    assert!(
        (before_stats.1 - after_stats.1).abs() < 1e-9,
        "volume survives the file: {} vs {}",
        before_stats.1,
        after_stats.1
    );

    let mut all = Vec::new();
    for side in [CapSide::Start, CapSide::End] {
        let faces = resolve(
            &restored,
            &face_reference(
                feature,
                SemanticRole::ExtrudeCap { side },
                SelectionRule::Exact,
            ),
        )
        .unwrap_or_else(|e| panic!("the {side:?} cap should resolve after a reopen: {e}"));
        assert_eq!(faces.len(), 1);
        all.extend(faces);
    }
    for segment in &segments {
        let faces = resolve(
            &restored,
            &face_reference(
                feature,
                SemanticRole::ExtrudeSide {
                    profile_segment: *segment,
                },
                SelectionRule::AllDerivedFrom { ancestor: *segment },
            ),
        )
        .unwrap_or_else(|e| panic!("side {segment} should resolve after a reopen: {e}"));
        assert_eq!(faces.len(), 1);
        all.extend(faces);
    }

    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 6, "six names, six distinct faces");

    kernel.release(shape);
    assert_eq!(kernel.live_shape_count(), 0);
}
