// SPDX-License-Identifier: MIT
//! The committed plate, held to what it has always meant.
//!
//! These are regression tests in the strict sense: the document was written by
//! an earlier build and checked in, so a change that quietly alters what a
//! stored name resolves to fails here rather than in somebody's model. The same
//! checks run against Open CASCADE in the pin workflow; a difference between
//! the two kernels shows up as a difference in one file.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{CacheStore, SemanticRole};
use ferritecad_eval::{CacheOutcome, rebuild_cached, rebuild_cold};
use ferritecad_fixtures::{
    drop_segment, open_plate, plate_manifest, plate_manifest_path, render_manifest, set_height,
    write_plate,
};
use ferritecad_kernel::{GeometryKernel, OperationContext, mock::MockKernel};
use ferritecad_types::ErrorKind;

#[test]
fn the_committed_plate_still_means_what_it_meant() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = open_plate(dir.path()).expect("the fixture opens");
    let mut kernel = MockKernel::new();

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a stored document rebuilds");

    assert_eq!(
        render_manifest(&document, &built).expect("renders"),
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
    let cold_manifest = render_manifest(&document, &cold).expect("renders");
    let cold_faces = faces(&document, &cold);
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
        render_manifest(&document, &warm).expect("renders"),
        cold_manifest,
        "a warm rebuild must be indistinguishable from a cold one"
    );

    let warm_faces = faces(&document, &warm);
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
    let manifest = render_manifest(&document, &before).expect("renders");
    let before_faces = faces(&document, &before);
    before.release_all(&mut kernel);

    set_height(&mut document, 25.0).expect("the plate is made taller");

    let after = rebuild_cold(&document, &mut kernel, &context).expect("rebuilds taller");
    assert_eq!(
        render_manifest(&document, &after).expect("renders"),
        manifest,
        "a different distance is not a different set of faces"
    );

    let after_faces = faces(&document, &after);
    assert_eq!(after_faces.len(), 6);
    assert_ne!(
        after_faces, before_faces,
        "the new solid is new geometry, so its handles are new"
    );

    after.release_all(&mut kernel);
}

#[test]
fn dropping_a_segment_loses_that_name_and_no_other() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = open_plate(dir.path()).expect("the fixture opens");
    let mut kernel = MockKernel::new();

    let gone = drop_segment(&mut document).expect("a segment is removed");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a three-sided profile still rebuilds");

    let mut resolved = Vec::new();
    for reference in document.topology_refs().expect("reads references") {
        let names_the_gone_segment = matches!(
            reference.output_role,
            SemanticRole::ExtrudeSide { profile_segment } if profile_segment == gone
        );

        match built.resolve(&reference) {
            Ok(found) => {
                assert!(
                    !names_the_gone_segment,
                    "the removed segment's reference must not resolve to anything, \
                     and it chose {} face(s)",
                    found.len()
                );
                assert_eq!(found.len(), 1);
                resolved.extend(found);
            }
            Err(error) => {
                assert!(
                    names_the_gone_segment,
                    "only the removed segment should have lost its face, but {:?} failed: {error}",
                    reference.output_role
                );
                assert_eq!(error.kind(), ErrorKind::Topology);
            }
        }
    }

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

/// Writes the fixture and its manifest. Not part of the gate.
///
/// Run deliberately, and expect the identifiers in the manifest to change: a
/// regenerated document is a new document. Review the diff before committing
/// it — that review is the only thing standing between an intended change and
/// an accidental one.
#[test]
#[ignore = "rewrites the committed fixture; run it on purpose"]
fn regenerate_the_committed_plate() {
    let path = ferritecad_fixtures::plate_source();
    if path.exists() {
        std::fs::remove_file(&path).expect("removes the old fixture");
    }
    write_plate(&path).expect("writes the plate");

    let dir = tempfile::tempdir().expect("temp dir");
    let document = open_plate(dir.path()).expect("reopens what was just written");
    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("the new fixture rebuilds");

    std::fs::write(
        plate_manifest_path(),
        render_manifest(&document, &built).expect("renders"),
    )
    .expect("writes the manifest");
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

/// Every face the document's stored references resolve to, in a stable order.
fn faces(
    document: &ferritecad_document::Document,
    built: &ferritecad_eval::RebuildResult,
) -> Vec<String> {
    let mut all: Vec<String> = document
        .topology_refs()
        .expect("reads references")
        .iter()
        .flat_map(|reference| built.resolve(reference).unwrap_or_default())
        .map(|face| face.to_string())
        .collect();
    all.sort();
    all
}
