// SPDX-License-Identifier: MIT
//! A rebuilt feature outliving both the session and the process that made it.
//!
//! The previous slice showed names surviving a session boundary within one
//! run. This one adds the part that makes it useful: the archive goes to a
//! file, everything in memory is dropped, and a later run opens the file cold
//! and gets its faces back under the same names.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::Path;

use ferritecad_document::{
    CacheStore, CapSide, EntityKind, SelectionRule, SemanticRole, TopologyRef,
};
use ferritecad_eval::{extrude_archive_key, load_extrude_archive, store_extrude_archive};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane, mock::MockKernel,
};
use ferritecad_topology::{ARCHIVE_CACHE_KIND, TopologyMap, archive_feature, resolve};
use ferritecad_types::{DocumentId, ErrorKind, ObjectId, Result, StableEntityId};

struct Plate {
    request: ExtrudeRequest,
    segments: Vec<StableEntityId>,
    feature: ObjectId,
}

fn plate(height: f64) -> Result<Plate> {
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

    Ok(Plate {
        request: ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(segments)?,
                Vec::new(),
            )?,
            ExtrudeExtent::blind(height)?,
            false,
        ),
        segments: labels,
        feature: ObjectId::new(),
    })
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

/// Builds the plate, stores its archive, and drops session and store.
fn write(path: &Path, document: DocumentId, plate: &Plate) {
    let mut kernel = MockKernel::new();
    let mut cache = CacheStore::open(
        path,
        document,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("the sidecar opens");

    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    let mut map = TopologyMap::new();
    map.record_extrude(plate.feature, plate.request.profile(), &result)
        .expect("records");

    let archived = archive_feature(&mut kernel, &map, plate.feature).expect("archives");
    store_extrude_archive(
        &mut cache,
        kernel.identity(),
        &plate.request,
        &OperationContext::default(),
        &archived,
    )
    .expect("stores");

    kernel.release(result.shape);
}

#[test]
fn names_survive_a_closed_file_and_a_new_session() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad-cache");
    let document = DocumentId::new();
    let plate = plate(10.0).expect("a valid plate");

    write(&path, document, &plate);

    // Nothing from the writing run is alive past this point: no kernel
    // session, no map, no open sidecar. Only the file.
    let mut kernel = MockKernel::new();
    let cache = CacheStore::open(
        &path,
        document,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("the sidecar opens again");

    let archived = load_extrude_archive(
        &cache,
        kernel.identity(),
        &plate.request,
        &OperationContext::default(),
        plate.feature,
    )
    .expect("reads")
    .expect("the entry written by the previous run is there");

    let mut restored = TopologyMap::new();
    ferritecad_topology::restore_feature(&mut kernel, &archived, &mut restored)
        .expect("restores into this session");

    let start = resolve(&restored, &cap_reference(plate.feature, CapSide::Start))
        .expect("the start cap resolves");
    let end = resolve(&restored, &cap_reference(plate.feature, CapSide::End))
        .expect("the end cap resolves");
    assert_eq!(start.len(), 1);
    assert_eq!(end.len(), 1);
    assert_ne!(start[0], end[0]);

    let mut all = Vec::new();
    for segment in &plate.segments {
        let faces = resolve(&restored, &side_reference(plate.feature, *segment))
            .unwrap_or_else(|e| panic!("side {segment} should resolve after a reopen: {e}"));
        assert_eq!(faces.len(), 1);
        all.extend(faces);
    }
    all.extend(start);
    all.extend(end);
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 6, "six names, six distinct faces");

    if let Some(shape) = restored.feature(plate.feature).and_then(|n| n.shape()) {
        kernel.release(shape);
    }
}

#[test]
fn a_different_extrusion_is_a_miss_not_a_wrong_hit() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad-cache");
    let document = DocumentId::new();
    let plate = plate(10.0).expect("a valid plate");

    write(&path, document, &plate);

    let kernel = MockKernel::new();
    let cache = CacheStore::open(
        &path,
        document,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("opens");

    // Same feature, same profile, a different distance.
    let taller = ExtrudeRequest::new(
        plate.request.profile().clone(),
        ExtrudeExtent::blind(11.0).expect("a positive distance"),
        false,
    );
    assert!(
        load_extrude_archive(
            &cache,
            kernel.identity(),
            &taller,
            &OperationContext::default(),
            plate.feature,
        )
        .expect("reads")
        .is_none(),
        "a changed extent must not find the old solid"
    );
}

#[test]
fn a_stored_entry_belongs_to_the_feature_that_wrote_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad-cache");
    let document = DocumentId::new();
    let plate = plate(10.0).expect("a valid plate");

    write(&path, document, &plate);

    let kernel = MockKernel::new();
    let cache = CacheStore::open(
        &path,
        document,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("opens");

    // Another feature reading with the same inputs finds nothing of its own.
    assert!(
        load_extrude_archive(
            &cache,
            kernel.identity(),
            &plate.request,
            &OperationContext::default(),
            ObjectId::new(),
        )
        .expect("reads")
        .is_none()
    );
}

#[test]
fn a_damaged_entry_is_reported_rather_than_treated_as_absent() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad-cache");
    let document = DocumentId::new();
    let plate = plate(10.0).expect("a valid plate");

    write(&path, document, &plate);

    let kernel = MockKernel::new();
    let mut cache = CacheStore::open(
        &path,
        document,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("opens");

    // Overwrite the entry with bytes that are consistent as a cache blob and
    // meaningless as an archive. The sidecar's own checksum cannot catch this;
    // the codec must.
    let key = extrude_archive_key(
        kernel.identity(),
        &plate.request,
        &OperationContext::default(),
    );
    cache
        .put(plate.feature, key, ARCHIVE_CACHE_KIND, b"not an archive")
        .expect("stores");

    let err = load_extrude_archive(
        &cache,
        kernel.identity(),
        &plate.request,
        &OperationContext::default(),
        plate.feature,
    )
    .expect_err("a damaged entry is not a miss");
    assert_eq!(err.kind(), ErrorKind::Input);
}

#[test]
fn the_geometry_and_its_names_are_one_entry() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad-cache");
    let document = DocumentId::new();
    let plate = plate(10.0).expect("a valid plate");

    write(&path, document, &plate);

    let kernel = MockKernel::new();
    let cache = CacheStore::open(
        &path,
        document,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("opens");
    let key = extrude_archive_key(
        kernel.identity(),
        &plate.request,
        &OperationContext::default(),
    );

    // There is no second kind to fall out of step with the first.
    assert!(
        cache
            .get(plate.feature, key, ARCHIVE_CACHE_KIND)
            .expect("reads")
            .is_some()
    );
    for kind in ["brep", "brep.names", "tessellation"] {
        assert!(
            cache
                .get(plate.feature, key, kind)
                .expect("reads")
                .is_none(),
            "{kind} must not exist as a separate entry"
        );
    }
}
