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

#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;

use ferritecad_document::CacheStore;
use ferritecad_eval::{load_extrude_archive, store_extrude_archive};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_topology::{archive_feature, restore_feature};
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

#[test]
fn open_cascade_geometry_and_its_names_survive_a_file() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad-cache");
    let document = DocumentId::new();
    let (request, segments) = plate().expect("a valid plate");
    let feature = ObjectId::new();
    let context = OperationContext::default();

    // The writing run. Everything it holds is gone by the closing brace.
    let (before_stats, blob_len, edge_names) = {
        let mut kernel = OcctKernel::new().expect("opens");
        let mut cache = CacheStore::open(
            &path,
            document,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("the sidecar opens");

        let result = kernel.extrude(&request, &context).expect("builds");
        let map = naming::bind(&mut kernel, feature, request.profile(), &result)
            .expect("complete naming");
        let stats = kernel.shape_stats(result.shape).expect("measures");

        let archived = archive_feature(&mut kernel, &map, feature).expect("archives");
        let blob_len = archived.blob().bytes().len();
        store_extrude_archive(&mut cache, kernel.identity(), &request, &context, &archived)
            .expect("stores");

        let edge_names: Vec<_> = [false, true]
            .into_iter()
            .flat_map(|end| {
                naming::cap_edges(&mut kernel, result.shape, end)
                    .into_values()
                    .map(|edge| map.feature(feature).expect("named").entities[&edge])
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(edge_names.len(), 8);
        kernel.release(result.shape);
        (stats, blob_len, edge_names)
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

    let mut restored = naming::prepared(feature, request.profile(), false).expect("model roots");
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

    // The cap edges survive too, and come back as edges of the restored shape
    // rather than as whatever the archive's other entries happened to be. This
    // is the whole point of writing them down: the handles that named them died
    // with the session that issued them.
    let mut restored_edges = std::collections::BTreeSet::new();
    for name in edge_names {
        let found = naming::resolve(&restored, feature, &name).expect("cap edge survives");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind(), ferritecad_kernel::SubShapeKind::Edge);
        assert_eq!(found[0].shape(), shape, "restored onto the restored shape");
        assert!(restored_edges.insert(found[0]), "one edge answered twice");
    }
    assert_eq!(restored_edges.len(), 8, "four segments, two caps");

    // And nothing that came back is a handle the previous session issued: the
    // shape is a new one, so every name resolves through it.
    let written = format!("{archived:?}");
    for word in ["session#", "shape#", "edge#", "face#"] {
        assert!(
            !written.contains(word),
            "the archive carries a live {word} name"
        );
    }

    let mut all = Vec::new();
    for end in [false, true] {
        let name = naming::cap_name(feature, naming::plane_id(), end);
        let faces = naming::resolve(&restored, feature, &name).expect("cap after reopen");
        assert_eq!(faces.len(), 1);
        all.extend(faces);
    }
    for segment in &segments {
        let name = naming::side_name(feature, naming::plane_id(), *segment);
        let faces = naming::resolve(&restored, feature, &name).expect("side after reopen");
        assert_eq!(faces.len(), 1);
        all.extend(faces);
    }

    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 6, "six names, six distinct faces");

    kernel.release(shape);
    assert_eq!(kernel.live_shape_count(), 0);
}
