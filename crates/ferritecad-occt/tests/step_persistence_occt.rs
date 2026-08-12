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
    Document, ImportedDefinitionRef, ImportedStep, ImporterIdentity, ObjectPayload,
    StepImportRequest, StepImporter,
};
use ferritecad_exchange::StoredScene;
use ferritecad_exchange::{Import, Severity};
use ferritecad_kernel::{GeometryKernel, ShapeHandle};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{ContentHash, ErrorKind, ObjectId, Result};
use tempfile::TempDir;

/// The adapter behind the document's importer contract.
///
/// Written here rather than in `ferritecad-occt` on purpose: nothing in the
/// shipped dependency graph points from the kernel adapter back at the
/// document, and a two-method wrapper in a test is a smaller price than
/// reversing that.
struct Session<'a>(&'a mut OcctKernel);

impl StepImporter for Session<'_> {
    fn identity(&self) -> &ferritecad_kernel::KernelIdentity {
        self.0.identity()
    }

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
        assert_eq!(
            StoredScene::V2(now.clone()),
            stored.scene,
            "{name}: the scene changed"
        );
        assert_eq!(now.source_unit, stored.scene.source_unit());
        assert_eq!(now.schema, stored.scene.schema());
        assert_eq!(
            now.definitions.len(),
            stored.scene.definition_count(),
            "{name}"
        );
        assert_eq!(now.instances.len(), stored.scene.instance_count(), "{name}");

        assert_eq!(
            reopened.imported_by,
            ImporterIdentity::of(kernel.identity()),
            "{name}: the same build read it both times"
        );
        assert_eq!(
            reopened.reopened_by,
            ImporterIdentity::of(kernel.identity()),
            "{name}: the fresh handles and diagnostics need current provenance"
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

        let StoredScene::V2(stored_scene) = &stored.scene else {
            panic!("{name}: a fresh import must store a v2 scene");
        };
        for (before, after) in stored_scene.definitions.iter().zip(&now.definitions) {
            assert_eq!(before.name, after.name, "{name}: a name changed");
            assert_eq!(before.solids, after.solids, "{name}: a solid count changed");
        }
        for (before, after) in stored_scene.instances.iter().zip(&now.instances) {
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
                    .find(|definition| definition.name == "Bolt")
                    .expect("the bolt is named");
                assert_eq!(
                    now.instances
                        .iter()
                        .filter(|instance| instance.definition == bolt.key)
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
                    scene: StoredScene::V2(persisted),
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
fn a_durable_reference_survives_a_new_session_and_never_crosses_sources() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let (_dir, path) = workspace();
    let plate_bytes = corpus("canonical", "01-single-part.step");
    let assembly_bytes = corpus("canonical", "02-flat-assembly.step");
    let plate_object = ObjectId::new();
    let assembly_object = ObjectId::new();

    // Two files, two sources, one document.
    let (plate_ref, assembly_ref, first_handle) = {
        let mut kernel = OcctKernel::new().expect("opens");
        let mut document = Document::create(&path).expect("creates");

        let plate = kernel.import_step(&plate_bytes).expect("imports");
        let stored_plate = document
            .store_step_import(StepImportRequest {
                object: plate_object,
                name: Some("Plate"),
                source: &plate_bytes,
                source_name: Some("01-single-part.step"),
                import: &plate,
                importer: kernel.identity(),
            })
            .expect("stores");
        release(&mut kernel, &plate);

        let assembly = kernel.import_step(&assembly_bytes).expect("imports");
        let stored_assembly = document
            .store_step_import(StepImportRequest {
                object: assembly_object,
                name: Some("Bracket"),
                source: &assembly_bytes,
                source_name: Some("02-flat-assembly.step"),
                import: &assembly,
                importer: kernel.identity(),
            })
            .expect("stores");

        // The collision this reference type exists for, taken from the corpus
        // rather than invented: a STEP entity identifier is unique inside its
        // own file and nowhere else, so both of these files describe a
        // definition keyed step.product_definition#5 — a plate in one and a
        // bracket in the other.
        let StoredScene::V2(plate_scene) = &stored_plate.scene else {
            panic!("a fresh import stores a v2 scene");
        };
        let StoredScene::V2(assembly_scene) = &stored_assembly.scene else {
            panic!("a fresh import stores a v2 scene");
        };
        let shared: Vec<&str> = plate_scene
            .definitions
            .iter()
            .map(|definition| definition.key.as_str())
            .filter(|key| {
                assembly_scene
                    .definitions
                    .iter()
                    .any(|other| other.key == *key)
            })
            .collect();
        assert!(
            !shared.is_empty(),
            "the corpus no longer demonstrates a key shared between two files, \
             which is the case this reference type exists for"
        );
        let key = shared[0].to_owned();

        let handle = assembly
            .scene()
            .expect("imports")
            .definitions
            .iter()
            .find(|definition| definition.key == key)
            .expect("the shared key is in this file")
            .shape;
        release(&mut kernel, &assembly);
        document.close().expect("closes");

        (
            ImportedDefinitionRef::new(stored_plate.source, &key).expect("valid"),
            ImportedDefinitionRef::new(stored_assembly.source, &key).expect("valid"),
            handle,
        )
    };
    // Both the session that read those files and the document are gone by here.

    assert_ne!(
        plate_ref, assembly_ref,
        "the same key in two files must not be the same reference"
    );

    let mut kernel = OcctKernel::new().expect("opens a second session");
    let document = Document::open(&path).expect("reopens");
    let reopened = document
        .reopen_step_import(assembly_object, &mut Session(&mut kernel))
        .expect("the assembly binds");

    // The reference into this source resolves, to this session's handle and
    // not to the one that died with the first session.
    let resolved = reopened.resolve(&assembly_ref).expect("resolves");
    assert_ne!(
        resolved, first_handle,
        "resolving handed back a handle from a session that is gone"
    );
    assert!(kernel.is_valid(resolved).expect("checks"));
    assert_eq!(
        resolved,
        reopened
            .scene
            .definitions
            .iter()
            .find(|definition| definition.key == assembly_ref.definition_key())
            .expect("the key is there")
            .shape
    );

    // The identical key belonging to the other file is refused, although a
    // definition carrying that very text is sitting right here.
    let error = reopened
        .resolve(&plate_ref)
        .expect_err("a key from another source must not resolve here");
    assert_eq!(error.kind(), ErrorKind::Input, "{error}");

    // And a key this file does not describe is lost rather than approximated.
    let missing = ImportedDefinitionRef::new(reopened.source(), "step.product_definition#999999")
        .expect("valid");
    let error = reopened.resolve(&missing).expect_err("nothing names that");
    assert_eq!(error.kind(), ErrorKind::Topology, "{error}");

    for shape in reopened.scene.shapes() {
        kernel.release(shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_reordered_reading_of_the_same_file_still_binds() {
    let mut kernel = kernel_or_skip!();

    let (_dir, path) = workspace();
    let bytes = corpus("canonical", "02-flat-assembly.step");
    let object = ObjectId::new();

    let outcome = kernel.import_step(&bytes).expect("imports");
    let mut document = Document::create(&path).expect("creates");
    let stored = document
        .store_step_import(StepImportRequest {
            object,
            name: None,
            source: &bytes,
            source_name: None,
            import: &outcome,
            importer: kernel.identity(),
        })
        .expect("stores");
    release(&mut kernel, &outcome);
    document.close().expect("closes");

    let StoredScene::V2(scene) = &stored.scene else {
        panic!("a fresh import stores a v2 scene");
    };
    assert!(
        scene
            .definitions
            .iter()
            .all(|definition| !definition.key.is_empty()),
        "every stored definition carries the identity its file gave it"
    );

    // Open CASCADE is not asked to reorder anything — it has no reason to and
    // this cannot make it. What is checked is that when the same file comes
    // back described in another order, the stored scene recognises it. So the
    // reading is real and the reordering is applied to it afterwards.
    let mut kernel = OcctKernel::new().expect("opens a second session");
    let document = Document::open(&path).expect("reopens");
    let mut reordering = Reordering {
        kernel: &mut kernel,
    };
    let reopened = document
        .reopen_step_import(object, &mut reordering)
        .expect("the same assembly, listed differently, is the same assembly");

    // Every placement still holds the part it names, which is what a stored
    // position could not have promised.
    let bound = &reopened.scene;
    for instance in &bound.instances {
        let definition = &bound.definitions[instance.definition];
        let expected = scene
            .instances
            .iter()
            .find(|stored| stored.name == instance.name)
            .expect("the same placements came back");
        assert_eq!(
            definition.key, expected.definition,
            "{} was re-attached to the wrong part",
            instance.name
        );
    }

    for shape in bound.shapes() {
        kernel.release(shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

/// An importer that reads normally and then reverses the definition order.
///
/// The reversal is the test's, not the kernel's. What is being measured is the
/// binding rule, and no corpus file makes Open CASCADE hand its definitions
/// back in a different order on demand.
struct Reordering<'a> {
    kernel: &'a mut OcctKernel,
}

impl StepImporter for Reordering<'_> {
    fn identity(&self) -> &ferritecad_kernel::KernelIdentity {
        self.kernel.identity()
    }

    fn import(&mut self, source: &[u8]) -> Result<Import> {
        let outcome = self.kernel.import_step(source)?;
        let Import::Imported { scene, diagnostics } = outcome else {
            return Ok(outcome);
        };

        let count = scene.definitions.len();
        let mut reversed = scene;
        reversed.definitions.reverse();
        for instance in &mut reversed.instances {
            instance.definition = count - 1 - instance.definition;
        }
        Ok(Import::Imported {
            scene: reversed,
            diagnostics,
        })
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.kernel.release(shape);
    }
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
