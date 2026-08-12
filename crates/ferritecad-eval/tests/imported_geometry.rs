// SPDX-License-Identifier: MIT
//! What a rebuild does about geometry it did not build.
//!
//! An imported STEP object holds a scene that came from bytes rather than from
//! features. There is nothing here to recompute, so a rebuild neither builds
//! it nor refuses the document that holds it – and says so, because a caller
//! that needs the whole model has to be able to tell that this is not it.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{Document, ImportedStep, ObjectPayload, StepImportRequest, StepImporter};
use ferritecad_eval::rebuild_cold;
use ferritecad_exchange::{ColourSource, Definition, Import, Instance, Scene};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, GeometryKernel, KernelIdentity, OperationContext, PlanarPoint,
    Profile, ProfileLoop, ProfileSegment, SegmentGeometry, ShapeHandle, SketchPlane,
    mock::MockKernel,
};
use ferritecad_types::{ObjectId, Result, StableEntityId};

/// A solid the mock has actually issued, so a scene refers to real handles.
fn solid(kernel: &mut MockKernel) -> ShapeHandle {
    let corners = [
        PlanarPoint::new(0.0, 0.0),
        PlanarPoint::new(10.0, 0.0),
        PlanarPoint::new(10.0, 10.0),
        PlanarPoint::new(0.0, 10.0),
    ]
    .map(|corner| corner.expect("finite"));
    let segments = corners
        .iter()
        .enumerate()
        .map(|(index, start)| {
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(*start, corners[(index + 1) % corners.len()])
                    .expect("distinct"),
            )
        })
        .collect();
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(segments).expect("closes"),
        Vec::new(),
    )
    .expect("valid");

    kernel
        .extrude(
            &ExtrudeRequest::new(profile, ExtrudeExtent::blind(4.0).expect("positive"), false),
            &OperationContext::default(),
        )
        .expect("the mock builds a solid")
        .shape
}

fn one_part(kernel: &mut MockKernel) -> Scene {
    Scene {
        source_unit: "MILLIMETRE".to_owned(),
        schema: "AP214".to_owned(),
        definitions: vec![Definition {
            shape: solid(kernel),
            name: "Plate".to_owned(),
            solids: 1,
            key: "step.product_definition#5".to_owned(),
        }],
        instances: vec![Instance {
            definition: 0,
            parent: None,
            name: "Plate".to_owned(),
            placement: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            colour_source: ColourSource::None,
            colour: [0.0; 3],
        }],
    }
}

/// Stores one import and hands its shapes back, leaving only the document.
fn store(path: &std::path::Path, kernel: &mut MockKernel) -> Result<(ObjectId, ImportedStep)> {
    let import = Import::Imported {
        scene: one_part(kernel),
        diagnostics: Vec::new(),
    };
    let object = ObjectId::new();
    let mut document = Document::create(path)?;
    let stored = document.store_step_import(StepImportRequest {
        object,
        name: Some("Imported"),
        source: b"ISO-10303-21; whatever the document was handed",
        source_name: None,
        import: &import,
        importer: kernel.identity(),
    })?;
    for shape in import.scene().expect("a scene was stored").shapes() {
        kernel.release(shape);
    }
    Ok((object, stored))
}

#[test]
fn a_document_holding_an_import_still_rebuilds() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("imported.fcad");
    let mut kernel = MockKernel::new();
    let (object, _) = store(&path, &mut kernel).expect("stores an import");

    let document = Document::open_read_only(&path).expect("reopens");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("an imported object is not a reason to refuse the document");

    // Named rather than passed over. A rebuild that reported nothing would
    // read as if this document held no geometry at all.
    assert_eq!(built.imports(), [object]);
    assert_eq!(built.shape_count(), 0, "an import is not rebuilt");
    assert!(
        !built.order().contains(&object),
        "an object that was not evaluated is listed among those that were"
    );

    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn the_stored_scene_is_what_the_document_gives_back() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("imported.fcad");
    let mut kernel = MockKernel::new();
    let (object, stored) = store(&path, &mut kernel).expect("stores an import");

    // The rebuild leaves the import alone, so what is on disk is still the
    // whole of it: this is the half a viewport has to read for itself.
    let document = Document::open_read_only(&path).expect("reopens");
    let ObjectPayload::ImportedStep(imported) = document
        .objects()
        .expect("reads")
        .into_iter()
        .find(|record| record.id == object)
        .expect("the import is in the document")
        .payload
    else {
        panic!("the stored object is not an import");
    };
    assert_eq!(imported.source_hash, stored.source_hash);
    assert_eq!(imported.scene.definition_count(), 1);
    assert_eq!(imported.scene.instance_count(), 1);
}

/// A reading that returns the same scene with fresh handles.
struct Again<'a>(&'a mut MockKernel);

impl StepImporter for Again<'_> {
    fn identity(&self) -> &KernelIdentity {
        self.0.identity()
    }

    fn import(&mut self, _source: &[u8]) -> Result<Import> {
        Ok(Import::Imported {
            scene: one_part(self.0),
            diagnostics: Vec::new(),
        })
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.0.release(shape);
    }
}

#[test]
fn a_rebuild_and_a_reopening_can_share_one_session() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("imported.fcad");
    let mut kernel = MockKernel::new();
    let (object, _) = store(&path, &mut kernel).expect("stores an import");

    // Both halves of a document's geometry, in the order a viewport wants
    // them and out of the same kernel. Neither may leave anything behind.
    let document = Document::open_read_only(&path).expect("reopens");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("builds");
    let reopened = {
        let mut importer = Again(&mut kernel);
        document
            .reopen_step_import(object, &mut importer)
            .expect("the stored scene reopens")
    };
    assert_eq!(reopened.scene.definitions.len(), 1);
    assert_eq!(kernel.live_shape_count(), 1, "the imported solid is live");

    for shape in reopened.scene.shapes() {
        kernel.release(shape);
    }
    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}
