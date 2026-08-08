// SPDX-License-Identifier: MIT
//! The committed plate, held to what it has always meant.
//!
//! The document was written by an earlier build and checked in, so a change
//! that loses a stored name, makes it ambiguous or collapses two names onto one
//! face fails here rather than in somebody's model. The same checks run against
//! Open CASCADE in the pin workflow and compare one file. The manifest does not
//! yet geometrically identify each face, so a one-to-one permutation remains a
//! stated gap rather than a guarantee these tests do not provide.

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
        render_manifest(&document, &warm, &mut reader).expect("renders"),
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
    let before_manifest = render_manifest(&document, &before, &mut kernel).expect("renders");
    let before_faces = faces(&document, &before);
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

    let after_faces = faces(&document, &after);
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

/// Rewrites the manifest from the document that is already committed.
///
/// Not part of the gate, and deliberately does *not* touch the `.fcad`: that
/// file is a document an earlier build wrote, and rewriting it would trade the
/// only property that makes it a regression fixture for a fresh set of
/// identifiers. It is written once, by [`write_plate`], if it is ever lost.
///
/// Review the diff before committing it. That review is the only thing between
/// an intended change in meaning and an accidental one.
#[test]
#[ignore = "rewrites the committed manifest; run it on purpose"]
fn regenerate_the_committed_manifest() {
    let source = ferritecad_fixtures::plate_source();
    if !source.exists() {
        write_plate(&source).expect("writes a plate where none was");
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let document = open_plate(dir.path()).expect("the fixture opens");
    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("the fixture rebuilds");

    std::fs::write(
        plate_manifest_path(),
        render_manifest(&document, &built, &mut kernel).expect("renders"),
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
