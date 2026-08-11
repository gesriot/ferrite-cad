// SPDX-License-Identifier: MIT
//! A STEP file into a document, and out again in a session that never saw it.
//!
//! The §18A tests proved a file can be read. These prove the reading survives:
//! the document is closed, the kernel session is dropped, a new session opens
//! the same file, and what comes back is the same assembly attached to handles
//! only that new session could have issued.
//!
//! What makes this a gate rather than a demonstration is that the comparison is
//! strict. Every name, every placement, every colour and the whole instance
//! tree must match what was stored, or nothing is bound at all. Open CASCADE is
//! the only thing here that can say whether re-reading identical bytes really
//! does produce an identical scene, which is why this cannot be tested on a
//! stand-in.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::PathBuf;

use ferritecad_document::{
    Document, ImportedStep, ImporterIdentity, ObjectPayload, StepImportRequest, StepImporter,
};
use ferritecad_exchange::{Import, Severity};
use ferritecad_kernel::{GeometryKernel, ShapeHandle};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{ContentHash, ObjectId, Result};
use tempfile::TempDir;

/// The adapter behind the document's importer contract.
///
/// Written here rather than in `ferritecad-occt` on purpose: nothing in the
/// shipped dependency graph points from the kernel adapter back at the
/// document, and a two-method wrapper in a test is a smaller price than
/// reversing that.
struct Session<'a>(&'a mut OcctKernel);

impl StepImporter for Session<'_> {
    fn import(&mut self, source: &[u8]) -> Result<Import> {
        self.0.import_step(source)
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.0.release(shape);
    }
}

fn corpus(kind: &str, name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(kind)
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn release(kernel: &mut OcctKernel, import: &Import) {
    if let Some(scene) = import.scene() {
        for shape in scene.shapes() {
            kernel.release(shape);
        }
    }
}

macro_rules! kernel_or_skip {
    () => {
        if !is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return;
        } else {
            OcctKernel::new().expect("opens")
        }
    };
}

fn workspace() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("a temporary directory is available");
    let path = dir.path().join("imported.fcad");
    (dir, path)
}

#[test]
fn every_sound_file_reopens_in_a_session_that_never_read_it() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    for name in [
        "01-single-part.step",
        "02-flat-assembly.step",
        "03-nested-assembly.step",
        "04-instance-colours.step",
        "05-inch-units.step",
        "06-unicode-names.step",
        "07-bare-geometry.step",
    ] {
        let (_dir, path) = workspace();
        let bytes = corpus("canonical", name);
        let object = ObjectId::new();

        // One session reads the file and one document stores it.
        let (stored, first_handles) = {
            let mut kernel = OcctKernel::new().expect("opens");
            let outcome = kernel
                .import_step(&bytes)
                .unwrap_or_else(|e| panic!("{name}: the import failed: {e}"));
            let scene = outcome
                .scene()
                .unwrap_or_else(|| panic!("{name} was rejected: {:?}", outcome.diagnostics()));
            let handles: Vec<ShapeHandle> = scene.shapes().collect();

            let mut document = Document::create(&path).expect("creates");
            let stored = document
                .store_step_import(StepImportRequest {
                    object,
                    name: Some(name),
                    source: &bytes,
                    source_name: Some(name),
                    import: &outcome,
                    importer: kernel.identity(),
                })
                .unwrap_or_else(|e| panic!("{name}: storing failed: {e}"));
            document.close().expect("closes");

            release(&mut kernel, &outcome);
            assert_eq!(kernel.live_shape_count(), 0, "{name} leaked a shape");
            (stored, handles)
        };
        // Both the session and the document are gone by here.

        assert_eq!(
            stored.source_byte_len,
            bytes.len() as u64,
            "{name}: the stored length is not the file's"
        );
        assert_eq!(stored.source_hash, ContentHash::of_bytes(&bytes));

        let mut kernel = OcctKernel::new().expect("opens a second session");
        let document = Document::open(&path).expect("reopens");
        let reopened = document
            .reopen_step_import(object, &mut Session(&mut kernel))
            .unwrap_or_else(|e| panic!("{name}: the stored scene did not bind: {e}"));

        // Fresh handles, from a session that did not exist when this was saved.
        let fresh: Vec<ShapeHandle> = reopened.scene.shapes().collect();
        assert_eq!(fresh.len(), first_handles.len(), "{name}");
        assert!(
            fresh.iter().all(|shape| !first_handles.contains(shape)),
            "{name}: reopening handed back handles from the session that is gone"
        );

        // And they are real geometry in this session, not merely distinct.
        for (definition, shape) in reopened.scene.definitions.iter().zip(&fresh) {
            assert_eq!(definition.shape, *shape);
            assert!(
                kernel.is_valid(*shape).expect("checks"),
                "{name}: the shape of {} is not sound after reopening",
                definition.name
            );
        }

        // Binding already required this; asserting it names what was at stake.
        let now = reopened.scene.persist().expect("projects");
        assert_eq!(now, stored.scene, "{name}: the scene changed");
        assert_eq!(now.source_unit, stored.scene.source_unit);
        assert_eq!(now.schema, stored.scene.schema);
        assert_eq!(
            now.definitions.len(),
            stored.scene.definitions.len(),
            "{name}"
        );
        assert_eq!(now.instances.len(), stored.scene.instances.len(), "{name}");

        assert_eq!(
            reopened.imported_by,
            ImporterIdentity::of(kernel.identity()),
            "{name}: the same build read it both times"
        );

        for shape in fresh {
            kernel.release(shape);
        }
        assert_eq!(kernel.live_shape_count(), 0, "{name} leaked on reopening");
    }
}

#[test]
fn names_units_colours_and_the_tree_are_what_survives() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    // Chosen for what they carry: an assembly with a repeated definition and a
    // recoloured placement, non-ASCII names, and a file that is not in
    // millimetres.
    for name in [
        "04-instance-colours.step",
        "06-unicode-names.step",
        "05-inch-units.step",
    ] {
        let (_dir, path) = workspace();
        let bytes = corpus("canonical", name);
        let object = ObjectId::new();

        let stored = {
            let mut kernel = OcctKernel::new().expect("opens");
            let outcome = kernel.import_step(&bytes).expect("imports");
            let mut document = Document::create(&path).expect("creates");
            let stored = document
                .store_step_import(StepImportRequest {
                    object,
                    name: None,
                    source: &bytes,
                    source_name: Some(name),
                    import: &outcome,
                    importer: kernel.identity(),
                })
                .expect("stores");
            document.close().expect("closes");
            release(&mut kernel, &outcome);
            stored
        };

        let mut kernel = OcctKernel::new().expect("opens");
        let document = Document::open(&path).expect("reopens");
        let reopened = document
            .reopen_step_import(object, &mut Session(&mut kernel))
            .unwrap_or_else(|e| panic!("{name}: did not bind: {e}"));
        let now = reopened.scene.persist().expect("projects");

        for (before, after) in stored.scene.definitions.iter().zip(&now.definitions) {
            assert_eq!(before.name, after.name, "{name}: a name changed");
            assert_eq!(before.solids, after.solids, "{name}: a solid count changed");
        }
        for (before, after) in stored.scene.instances.iter().zip(&now.instances) {
            assert_eq!(before.parent, after.parent, "{name}: the tree changed");
            assert_eq!(before.definition, after.definition, "{name}");
            assert_eq!(before.name, after.name, "{name}");
            assert_eq!(
                before.placement, after.placement,
                "{name}: a placement moved between two readings of the same bytes"
            );
            assert_eq!(before.colour_source, after.colour_source, "{name}");
            assert_eq!(before.colour, after.colour, "{name}");
        }

        match name {
            "04-instance-colours.step" => {
                assert_eq!(now.definitions.len(), 2);
                let bolt = now
                    .definitions
                    .iter()
                    .position(|definition| definition.name == "Bolt")
                    .expect("the bolt is named");
                assert_eq!(
                    now.instances
                        .iter()
                        .filter(|instance| instance.definition as usize == bolt)
                        .count(),
                    4,
                    "four placements of one definition"
                );
                assert_eq!(
                    now.instances
                        .iter()
                        .filter(|instance| {
                            instance.colour_source == ferritecad_exchange::ColourSource::Instance
                        })
                        .count(),
                    1,
                    "exactly one bolt is painted over its definition"
                );
            }
            "06-unicode-names.step" => {
                let names: Vec<&str> = now
                    .definitions
                    .iter()
                    .map(|definition| definition.name.as_str())
                    .collect();
                assert!(
                    names.iter().any(|name| name.contains('組')),
                    "the Japanese name did not survive storage: {names:?}"
                );
                assert!(
                    names.iter().any(|name| name.contains('К')),
                    "the Cyrillic name did not survive storage: {names:?}"
                );
            }
            "05-inch-units.step" => {
                assert_eq!(
                    now.source_unit.to_uppercase(),
                    "INCH",
                    "the unit the file declared, not the one we would prefer"
                );
            }
            _ => unreachable!(),
        }

        for shape in reopened.scene.shapes() {
            kernel.release(shape);
        }
        assert_eq!(kernel.live_shape_count(), 0, "{name} leaked on reopening");
    }
}

#[test]
fn what_was_wrong_with_a_file_is_kept_and_not_confused_with_this_reading() {
    let mut kernel = kernel_or_skip!();

    // Read completely, and not silently: the diagnostics are the only thing
    // separating these from a sound file, which is exactly why storing them
    // matters. Both are read again here, so both sets exist at once.
    for (name, expected) in [
        ("02-broken-reference.step", "unresolved"),
        ("05-duplicate-entity-id.step", "several times"),
    ] {
        let (_dir, path) = workspace();
        let bytes = corpus("damaged", name);
        let object = ObjectId::new();

        let outcome = kernel.import_step(&bytes).expect("imports");
        assert!(outcome.scene().is_some(), "{name} should still import");
        assert!(outcome.failures() > 0, "{name} was read without complaint");
        let then = outcome.diagnostics().to_vec();

        let mut document = Document::create(&path).expect("creates");
        document
            .store_step_import(StepImportRequest {
                object,
                name: None,
                source: &bytes,
                source_name: Some(name),
                import: &outcome,
                importer: kernel.identity(),
            })
            .expect("stores");
        document.close().expect("closes");
        release(&mut kernel, &outcome);

        let document = Document::open(&path).expect("reopens");
        let reopened = document
            .reopen_step_import(object, &mut Session(&mut kernel))
            .unwrap_or_else(|e| panic!("{name}: a damaged file must still bind: {e}"));

        assert_eq!(
            reopened.diagnostics_at_import, then,
            "{name}: what was said at import was not kept as it was said"
        );
        assert!(
            reopened
                .diagnostics_at_import
                .iter()
                .any(
                    |diagnostic| diagnostic.message.to_lowercase().contains(expected)
                        || diagnostic.entity.to_lowercase().contains(expected)
                ),
            "{name}: the stored diagnostics lost the reason: {:?}",
            reopened.diagnostics_at_import
        );
        assert!(
            reopened
                .diagnostics_now
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Fail),
            "{name}: this reading noticed nothing, and it should have"
        );
        // Two observations, reported as two. They agree here because the same
        // build read the same bytes; the point is that either could change
        // without the other being rewritten to match.
        assert_eq!(reopened.diagnostics_now, reopened.diagnostics_at_import);

        for shape in reopened.scene.shapes() {
            kernel.release(shape);
        }
        assert_eq!(kernel.live_shape_count(), 0, "{name} leaked");
    }
}

#[test]
fn a_file_the_kernel_refuses_writes_nothing_at_all() {
    let mut kernel = kernel_or_skip!();

    for name in ["01-truncated.step", "03-missing-terminator.step"] {
        let (_dir, path) = workspace();
        let bytes = corpus("damaged", name);

        // Settle the document first: creating and opening it applies the
        // migration and the persistent pragmas, so anything that moves after
        // this moved because of the import.
        Document::create(&path)
            .expect("creates")
            .close()
            .expect("closes");
        Document::open(&path)
            .expect("opens")
            .close()
            .expect("closes");
        let before = std::fs::read(&path).expect("reads");
        let modified_before = {
            let document = Document::open(&path).expect("opens");
            let modified = document.meta().modified_at.clone();
            document.close().expect("closes");
            modified
        };

        let outcome = kernel
            .import_step(&bytes)
            .expect("the call itself succeeds");
        assert!(
            matches!(outcome, Import::Rejected { .. }),
            "{name} should have been rejected"
        );

        let mut document = Document::open(&path).expect("opens");
        let error = document
            .store_step_import(StepImportRequest {
                object: ObjectId::new(),
                name: None,
                source: &bytes,
                source_name: Some(name),
                import: &outcome,
                importer: kernel.identity(),
            })
            .expect_err("a rejection has no scene to store");
        assert!(error.to_string().contains("not imported"), "{error}");
        document.close().expect("closes");

        assert_eq!(
            std::fs::read(&path).expect("reads"),
            before,
            "{name}: a refused import changed the document"
        );
        let document = Document::open(&path).expect("opens");
        assert_eq!(
            document.meta().modified_at,
            modified_before,
            "{name}: a refused import stamped the document as modified"
        );
        assert!(document.objects().expect("reads").is_empty());
        document.close().expect("closes");
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_stored_scene_that_does_not_describe_the_file_is_refused() {
    let mut kernel = kernel_or_skip!();

    let (_dir, path) = workspace();
    let bytes = corpus("canonical", "02-flat-assembly.step");
    let object = ObjectId::new();

    let outcome = kernel.import_step(&bytes).expect("imports");
    let scene = outcome.scene().expect("imports");
    let mut persisted = scene.persist().expect("projects");
    release(&mut kernel, &outcome);

    // A document that claims one part more than the file has. Written through
    // the primitives so the mismatch is deliberate rather than an accident the
    // composed path would have prevented.
    let renamed = persisted.definitions[0].name.clone();
    persisted.definitions[0].name = format!("{renamed} (edited)");

    let mut document = Document::create(&path).expect("creates");
    document
        .write(|w| {
            let source = w.put_step_source(&bytes)?;
            w.put_imported_step(
                object,
                None,
                0,
                None,
                &ImportedStep {
                    source,
                    source_hash: ContentHash::of_bytes(&bytes),
                    source_byte_len: bytes.len() as u64,
                    source_name: None,
                    scene: persisted,
                    imported_by: ImporterIdentity::of(kernel.identity()),
                    diagnostics_at_import: Vec::new(),
                },
            )?;
            Ok(())
        })
        .expect("writes");
    document.close().expect("closes");

    assert_eq!(
        kernel.live_shape_count(),
        0,
        "nothing is held before the read"
    );

    let document = Document::open(&path).expect("reopens");
    let error = document
        .reopen_step_import(object, &mut Session(&mut kernel))
        .expect_err("a definition this file does not describe must not bind");
    assert!(
        error.to_string().contains(&renamed),
        "the refusal should name what differed: {error}"
    );

    // Refusing is not enough. The reading really happened, Open CASCADE really
    // built those shapes, and the caller never saw them — so the refusal itself
    // has to give them back, or a document that fails to reopen leaks every
    // time it is tried.
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the refused reading left its shapes behind"
    );
    document.close().expect("closes");
}

#[test]
fn the_same_file_stored_twice_is_one_copy_of_the_bytes() {
    let mut kernel = kernel_or_skip!();

    let (_dir, path) = workspace();
    let bytes = corpus("canonical", "03-nested-assembly.step");
    let mut document = Document::create(&path).expect("creates");

    let mut sources = Vec::new();
    for _ in 0..2 {
        let outcome = kernel.import_step(&bytes).expect("imports");
        let stored = document
            .store_step_import(StepImportRequest {
                object: ObjectId::new(),
                name: None,
                source: &bytes,
                source_name: None,
                import: &outcome,
                importer: kernel.identity(),
            })
            .expect("stores");
        sources.push(stored.source);
        release(&mut kernel, &outcome);
    }
    assert_eq!(
        sources[0], sources[1],
        "two imports of one file must share one copy of its bytes"
    );

    let objects = document.objects().expect("reads");
    assert_eq!(objects.len(), 2);
    assert!(
        objects
            .iter()
            .all(|object| matches!(object.payload, ObjectPayload::ImportedStep(_)))
    );
    document.close().expect("closes");
    assert_eq!(kernel.live_shape_count(), 0);
}
