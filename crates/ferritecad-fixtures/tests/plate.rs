// SPDX-License-Identifier: MIT
//! The committed plate, held to what it has always meant.
//!
//! The document was written by an earlier build and checked in, so a change
//! that loses a stored name, makes it ambiguous or collapses two names onto one
//! face fails here rather than in somebody's model. The same checks run against
//! Open CASCADE in the pin workflow and compare one file. Each resolved face is
//! measured from its own triangles, so a one-to-one permutation changes the
//! manifest even though every name still reaches one distinct face.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::CacheStore;
use ferritecad_eval::{CacheOutcome, rebuild_cached, rebuild_cold};
use ferritecad_fixtures::{drop_segment, open_plate, plate_manifest, render_manifest, set_height};
use ferritecad_kernel::{GeometryKernel, OperationContext, mock::MockKernel};

#[test]
fn the_committed_plate_still_means_what_it_meant() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = open_plate(dir.path()).expect("the fixture opens");
    let mut kernel = MockKernel::new();

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a stored document rebuilds");

    assert_eq!(
        render_manifest(&document, &built, &mut kernel).expect("renders"),
        plate_manifest().expect("the committed manifest is readable"),
        "the plate resolves differently than it used to; if this is intended, \
         regenerate the manifest and say why in the commit"
    );

    built.release_all(&mut kernel);
}

#[test]
fn a_warm_rebuild_resolves_exactly_what_a_cold_one_does() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = open_plate(dir.path()).expect("the fixture opens");
    let document_id = document.meta().document_id;
    let context = OperationContext::default();

    // Cold, in a session of its own.
    let mut cold_kernel = MockKernel::new();
    let cold = rebuild_cold(&document, &mut cold_kernel, &context).expect("rebuilds");
    let cold_manifest = render_manifest(&document, &cold, &mut cold_kernel).expect("renders");
    let cold_faces = faces(&cold);
    cold.release_all(&mut cold_kernel);

    // A second session fills the sidecar.
    let mut writer = MockKernel::new();
    let mut cache = open_cache(dir.path(), &writer, document_id);
    let (built, _) =
        rebuild_cached(&document, &mut writer, &mut cache, &context).expect("rebuilds");
    built.release_all(&mut writer);
    drop(cache);

    // A third session, which computes nothing.
    let mut reader = MockKernel::new();
    let mut cache = open_cache(dir.path(), &reader, document_id);
    let (warm, events) =
        rebuild_cached(&document, &mut reader, &mut cache, &context).expect("rebuilds");

    assert_eq!(reader.extrude_count(), 0, "the third session should hit");
    assert!(events.iter().all(|e| e.outcome == CacheOutcome::Hit));
    assert_eq!(
        render_manifest(&document, &warm, &mut reader).expect("renders"),
        cold_manifest,
        "a warm rebuild must be indistinguishable from a cold one"
    );

    let warm_faces = faces(&warm);
    assert_eq!(warm_faces.len(), 6, "four sides and two caps");
    assert_eq!(
        warm_faces
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        6,
        "six names must not collapse onto fewer faces"
    );
    assert_ne!(
        warm_faces, cold_faces,
        "handles belong to the session that issued them"
    );

    warm.release_all(&mut reader);
}

#[test]
fn changing_the_height_keeps_every_name_and_no_handle() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = open_plate(dir.path()).expect("the fixture opens");
    let context = OperationContext::default();
    let mut kernel = MockKernel::new();

    let before = rebuild_cold(&document, &mut kernel, &context).expect("rebuilds");
    let before_manifest = render_manifest(&document, &before, &mut kernel).expect("renders");
    let before_faces = faces(&before);
    before.release_all(&mut kernel);

    set_height(&mut document, 25.0).expect("the plate is made taller");

    let after = rebuild_cold(&document, &mut kernel, &context).expect("rebuilds taller");
    let after_manifest = render_manifest(&document, &after, &mut kernel).expect("renders");

    // Every name still reaches exactly one face, and the same names as before.
    assert_eq!(
        names_only(&before_manifest),
        names_only(&after_manifest),
        "a different distance is not a different set of names"
    );

    // And the plate really did change, so the comparison above is not vacuous.
    assert_ne!(
        before_manifest, after_manifest,
        "a taller plate has taller faces; if these match, nothing was rebuilt"
    );

    let after_faces = faces(&after);
    assert_eq!(after_faces.len(), 6);
    assert_ne!(
        after_faces, before_faces,
        "the new solid is new geometry, so its handles are new"
    );

    after.release_all(&mut kernel);
}

/// A manifest with the measurements removed: what each name is, not where.
fn names_only(manifest: &str) -> Vec<&str> {
    manifest
        .lines()
        .filter(|line| !line.trim_start().starts_with("area "))
        .collect()
}

#[test]
fn dropping_a_segment_loses_that_name_and_no_other() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = open_plate(dir.path()).expect("the fixture opens");
    let mut kernel = MockKernel::new();

    let before = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("original rebuild");
    let old_faces = before
        .topology()
        .features
        .values()
        .flat_map(|f| f.bindings.keys().copied())
        .filter(|n| n.kind == ferritecad_kernel::SubShapeKind::Face)
        .collect::<std::collections::BTreeSet<_>>();
    before.release_all(&mut kernel);
    drop_segment(&mut document).expect("a segment is removed");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a three-sided profile still rebuilds");

    let named = built
        .topology()
        .features
        .values()
        .next()
        .expect("feature names");
    let mut resolved = Vec::new();
    let mut lost = 0;
    for name in old_faces {
        match named.resolve(name) {
            Ok(entity) => resolved.push(entity),
            Err(_) => lost += 1,
        }
    }
    assert_eq!(lost, 1, "only the removed side loses its name");

    // Three sides and two caps, all different. A silently retargeted name would
    // show up here as a repeat.
    assert_eq!(resolved.len(), 5);
    resolved.sort_unstable();
    resolved.dedup();
    assert_eq!(
        resolved.len(),
        5,
        "a lost name must not borrow a neighbour's face"
    );

    built.release_all(&mut kernel);
}

fn open_cache(
    dir: &std::path::Path,
    kernel: &MockKernel,
    document: ferritecad_types::DocumentId,
) -> CacheStore {
    CacheStore::open(
        dir.join("plate.fcad-cache"),
        document,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("the sidecar opens")
}

/// Every face named by the rebuilt model, in a stable order.
fn faces(built: &ferritecad_eval::RebuildResult) -> Vec<String> {
    let mut all: Vec<String> = built
        .topology()
        .features
        .values()
        .flat_map(|f| f.bindings.iter())
        .filter(|(name, _)| name.kind == ferritecad_kernel::SubShapeKind::Face)
        .flat_map(|(_, entities)| entities.iter())
        .map(ToString::to_string)
        .collect();
    all.sort();
    all
}

#[test]
#[ignore = "regenerates the committed plate and its geometry manifest"]
fn regenerate_plate() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("generate plate");
    let document = ferritecad_document::Document::open(&path).expect("generated document");
    let mut kernel = MockKernel::new();
    let built =
        rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("rebuild");
    let manifest = render_manifest(&document, &built, &mut kernel).expect("manifest");
    built.release_all(&mut kernel);
    document.close().expect("close");
    std::fs::copy(&path, ferritecad_fixtures::plate_source()).expect("write fixture");
    std::fs::write(ferritecad_fixtures::plate_manifest_path(), manifest).expect("write manifest");
}
