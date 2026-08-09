// SPDX-License-Identifier: MIT
//! The committed plate, against the kernel that ships.
//!
//! The mock proves the evaluator keeps its naming promises; this proves Open
//! CASCADE keeps the same ones about the same stored document. Both gates
//! compare against one manifest, including the area and centroid measured from
//! the triangles attached to each resolved face.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{CacheStore, SemanticRole};
use ferritecad_eval::{CacheOutcome, rebuild_cached, rebuild_cold};
use ferritecad_fixtures::{drop_segment, open_plate, plate_manifest, render_manifest, set_height};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::ErrorKind;

#[test]
fn open_cascade_resolves_the_committed_plate_as_recorded() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let document = open_plate(dir.path()).expect("the fixture opens");
    let mut kernel = OcctKernel::new().expect("opens");

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("Open CASCADE rebuilds the stored plate");

    // The same file the mock is held to. Lost, ambiguous, collapsed or
    // geometrically exchanged names all differ here.
    assert_eq!(
        render_manifest(&document, &built, &mut kernel).expect("renders"),
        plate_manifest().expect("the committed manifest is readable")
    );

    let (faces, volume) = kernel
        .shape_stats(solid(&document, &built))
        .expect("measures");
    assert_eq!(faces, 6);
    assert!(
        (volume - 24_000.0).abs() < 1e-6,
        "60 x 40 x 10 is 24000 mm^3, got {volume}"
    );

    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn open_cascade_agrees_with_itself_warm_and_cold() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let document = open_plate(dir.path()).expect("the fixture opens");
    let document_id = document.meta().document_id;
    let context = OperationContext::default();

    let mut writer = OcctKernel::new().expect("opens");
    let mut cache = open_cache(dir.path(), &writer, document_id);
    let (cold, _) = rebuild_cached(&document, &mut writer, &mut cache, &context).expect("rebuilds");
    let manifest = render_manifest(&document, &cold, &mut writer).expect("renders");
    cold.release_all(&mut writer);
    drop(cache);

    // A session with no geometry of its own.
    let mut reader = OcctKernel::new().expect("opens");
    let mut cache = open_cache(dir.path(), &reader, document_id);
    let (warm, events) =
        rebuild_cached(&document, &mut reader, &mut cache, &context).expect("rebuilds");

    assert!(
        events.iter().all(|e| e.outcome == CacheOutcome::Hit),
        "the second run should have come out of the sidecar: {events:?}"
    );
    assert_eq!(
        manifest,
        render_manifest(&document, &warm, &mut reader).expect("renders")
    );

    let (faces, volume) = reader
        .shape_stats(solid(&document, &warm))
        .expect("measures");
    assert_eq!(faces, 6);
    assert!((volume - 24_000.0).abs() < 1e-6);

    warm.release_all(&mut reader);
    assert_eq!(reader.live_shape_count(), 0);
}

#[test]
fn open_cascade_loses_only_the_dropped_segments_name() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = open_plate(dir.path()).expect("the fixture opens");
    let mut kernel = OcctKernel::new().expect("opens");

    // Taller as well, so the edit is not only topological.
    set_height(&mut document, 25.0).expect("resizes");
    let gone = drop_segment(&mut document).expect("removes a segment");

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
                assert!(!names_the_gone_segment, "a lost name must not resolve");
                assert_eq!(found.len(), 1);
                resolved.extend(found);
            }
            Err(error) => {
                assert!(
                    names_the_gone_segment,
                    "{:?} failed: {error}",
                    reference.output_role
                );
                assert_eq!(error.kind(), ErrorKind::Topology);
            }
        }
    }

    resolved.sort_unstable();
    let named = resolved.len();
    resolved.dedup();
    assert_eq!(named, 5);
    assert_eq!(
        resolved.len(),
        5,
        "a lost name must not borrow a neighbour's face"
    );

    let (faces, _) = kernel
        .shape_stats(solid(&document, &built))
        .expect("measures");
    assert_eq!(faces, 5, "a triangular prism has three sides and two caps");

    built.release_all(&mut kernel);
}

/// The plate's one solid, found through the document rather than a fixed id.
fn solid(
    document: &ferritecad_document::Document,
    built: &ferritecad_eval::RebuildResult,
) -> ferritecad_kernel::ShapeHandle {
    let extrude = document
        .objects()
        .expect("reads objects")
        .into_iter()
        .find(|object| {
            matches!(
                object.payload,
                ferritecad_document::ObjectPayload::Extrude(_)
            )
        })
        .expect("the plate has an extrusion");
    built.shape(extrude.id).expect("the extrusion made a solid")
}

fn open_cache(
    dir: &std::path::Path,
    kernel: &OcctKernel,
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
