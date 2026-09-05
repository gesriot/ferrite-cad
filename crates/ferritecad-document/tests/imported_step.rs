// SPDX-License-Identifier: MIT
//! Storing an imported file and reading it back into a new kernel session.
//!
//! No Open CASCADE here. The importer is a stand-in implementing the same trait
//! the real adapter does, and it lets these tests state things a kernel cannot
//! be asked to produce on demand: a scene whose definitions were reordered, a
//! source blob corrupted underneath a document, a reading that now refuses a
//! file it once accepted. What runs against the real kernel is in
//! `crates/ferritecad-occt/tests/step_persistence_occt.rs`.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Access, Body, Document, IMPORTED_STEP_CAPABILITY, ImportedDefinitionRef, ImportedStep,
    ImporterIdentity, ObjectKind, ObjectPayload, STEP_SOURCE_FORMAT, StepImportRequest,
    StepImporter,
};
use ferritecad_exchange::{
    ColourSource, Definition, Diagnostic, Import, Instance, KeyedInstance, KeyedScene,
    LegacyDefinition, LegacyInstance, LegacyScene, PersistedScene, Scene, Severity, Stage,
    StoredOccurrences, StoredScene,
};
use ferritecad_kernel::{KernelIdentity, SessionId, ShapeHandle};
use ferritecad_types::{CadError, ContentHash, ErrorKind, ObjectId, Result};
use rusqlite::Connection;
use tempfile::TempDir;

/// Bytes that stand in for a STEP file. Nothing in this crate parses them.
const SOURCE: &[u8] = b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";

const IDENTITY: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

fn workspace() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().expect("a temporary directory is available");
    let path = dir.path().join("part.fcad");
    (dir, path)
}

fn kernel() -> KernelIdentity {
    KernelIdentity::new("mock", "1.0", "test").expect("valid")
}

fn moved(x: f64) -> [f64; 12] {
    let mut placement = IDENTITY;
    placement[3] = x;
    placement
}

/// One assembly: a plate and two bolts, one of them recoloured.
const PLATE: &str = "step.product_definition#5";
const BOLT: &str = "step.product_definition#31";

fn scene(session: SessionId) -> Scene {
    Scene {
        source_unit: "MM".to_owned(),
        schema: "AP242".to_owned(),
        definitions: vec![
            Definition {
                shape: ShapeHandle::new(session, 1),
                name: "Plate".to_owned(),
                solids: 1,
                key: "step.product_definition#5".to_owned(),
            },
            Definition {
                shape: ShapeHandle::new(session, 2),
                name: "Bolt".to_owned(),
                solids: 1,
                key: "step.product_definition#31".to_owned(),
            },
        ],
        instances: vec![
            Instance {
                definition: 0,
                parent: None,
                name: "Assembly".to_owned(),
                placement: IDENTITY,
                colour_source: ColourSource::None,
                colour: [0.0; 3],
            },
            Instance {
                definition: 1,
                parent: Some(0),
                name: "Bolt/1".to_owned(),
                placement: moved(10.0),
                colour_source: ColourSource::Definition,
                colour: [0.6, 0.03, 0.03],
            },
            Instance {
                definition: 1,
                parent: Some(0),
                name: "Bolt/2".to_owned(),
                placement: moved(20.0),
                colour_source: ColourSource::Instance,
                colour: [0.1, 0.2, 0.3],
            },
        ],
    }
}

fn warned(message: &str) -> Diagnostic {
    Diagnostic {
        stage: Stage::Load,
        severity: Severity::Warning,
        entity: "#42".to_owned(),
        message: message.to_owned(),
    }
}

fn imported(session: SessionId, diagnostics: Vec<Diagnostic>) -> Import {
    Import::Imported {
        scene: scene(session),
        diagnostics,
    }
}

/// A kernel session that answers with whatever it was told to, and keeps a
/// record of being asked and of being given anything back.
struct Importer<F> {
    reply: F,
    identity: KernelIdentity,
    asked: bool,
    released: Vec<ShapeHandle>,
}

impl<F: FnMut(&[u8]) -> Result<Import>> Importer<F> {
    fn new(reply: F) -> Self {
        Self {
            reply,
            identity: kernel(),
            asked: false,
            released: Vec::new(),
        }
    }

    fn with_identity(identity: KernelIdentity, reply: F) -> Self {
        Self {
            reply,
            identity,
            asked: false,
            released: Vec::new(),
        }
    }
}

impl<F: FnMut(&[u8]) -> Result<Import>> StepImporter for Importer<F> {
    fn identity(&self) -> &KernelIdentity {
        &self.identity
    }

    fn import(&mut self, source: &[u8]) -> Result<Import> {
        self.asked = true;
        (self.reply)(source)
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.released.push(shape);
    }
}

/// Stores one import of [`SOURCE`] and returns the object it was stored as.
fn store(document: &mut Document, import: &Import) -> Result<(ObjectId, ImportedStep)> {
    let object = ObjectId::new();
    let stored = document.store_step_import(StepImportRequest {
        object,
        name: Some("Bracket assembly"),
        source: SOURCE,
        source_name: Some("bracket.step"),
        import,
        importer: &kernel(),
    })?;
    Ok((object, stored))
}

#[test]
fn a_stored_import_reopens_into_a_new_session_with_that_session_s_handles() {
    let (_dir, path) = workspace();
    let first = SessionId::new();

    let mut document = Document::create(&path).expect("creates");
    let (object, stored) = store(&mut document, &imported(first, Vec::new())).expect("stores");
    let original: Vec<ShapeHandle> = scene(first).shapes().collect();
    document.close().expect("closes");

    // A different process, a different kernel session, different slots in it.
    let later = SessionId::new();
    let mut current = scene(later);
    current.definitions[0].shape = ShapeHandle::new(later, 4001);
    current.definitions[1].shape = ShapeHandle::new(later, 4002);

    let document = Document::open(&path).expect("reopens");
    let mut current = Some(current);
    let mut importer = Importer::new(|bytes: &[u8]| {
        assert_eq!(bytes, SOURCE, "the importer must be given the stored bytes");
        Ok(Import::Imported {
            scene: current.take().expect("read once"),
            diagnostics: Vec::new(),
        })
    });
    let reopened = document
        .reopen_step_import(object, &mut importer)
        .expect("the same file binds");
    assert!(
        importer.released.is_empty(),
        "a successful binding gave shapes back that the caller now holds"
    );

    let fresh: Vec<ShapeHandle> = reopened.scene.shapes().collect();
    assert_eq!(
        fresh,
        vec![ShapeHandle::new(later, 4001), ShapeHandle::new(later, 4002)]
    );
    assert!(
        fresh.iter().all(|shape| !original.contains(shape)),
        "reopening handed back the handles of a session that is gone"
    );

    // Everything portable came back as it went in. The re-projection mints its
    // own placement identities, because a projection is what a new import
    // writes down; the stored ones are checked separately, below.
    let mut persisted = reopened.scene.persist().expect("projects");
    let StoredOccurrences::Recorded(recorded) = stored.scene.occurrences() else {
        panic!("a current-layout scene records placement identities");
    };
    assert_eq!(
        reopened.occurrences(),
        &StoredOccurrences::Recorded(recorded.clone()),
        "reopening did not hand back the stored placement identities"
    );
    for (instance, occurrence) in persisted.instances.iter_mut().zip(&recorded) {
        assert_ne!(
            instance.occurrence, *occurrence,
            "re-projecting a reopened scene reproduced the stored identity, so something \
             other than the payload is producing it"
        );
        instance.occurrence = *occurrence;
    }
    assert_eq!(StoredScene::V3(persisted.clone()), stored.scene);
    assert_eq!(persisted.source_unit, "MM");
    assert_eq!(persisted.definitions[1].name, "Bolt");
    assert_eq!(persisted.instances[2].colour_source, ColourSource::Instance);
    assert_eq!(persisted.instances[2].placement, moved(20.0));
}

#[test]
fn what_the_import_said_then_and_what_it_says_now_stay_apart() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    let then = vec![warned("unresolved reference")];
    let (object, _) =
        store(&mut document, &imported(SessionId::new(), then.clone())).expect("stores");
    document.close().expect("closes");

    let now = vec![warned("entity defined SEVERAL TIMES")];
    let document = Document::open(&path).expect("reopens");
    let mut importer = Importer::new(|_: &[u8]| {
        Ok(Import::Imported {
            scene: scene(SessionId::new()),
            diagnostics: now.clone(),
        })
    });
    let reopened = document
        .reopen_step_import(object, &mut importer)
        .expect("binds");

    assert_eq!(reopened.diagnostics_at_import, then);
    assert_eq!(reopened.diagnostics_now, now);
    assert_ne!(reopened.diagnostics_at_import, reopened.diagnostics_now);

    // And the historical set is still what the document holds, unchanged by
    // this reading having happened.
    let stored = document
        .step_import(object)
        .expect("reads")
        .expect("is there");
    assert_eq!(stored.imported.diagnostics_at_import, then);
}

#[test]
fn a_scene_that_no_longer_matches_is_refused_rather_than_bound() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    let (object, _) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");
    let document = Document::open(&path).expect("reopens");

    let refuses = |what: &str, damage: fn(&mut Scene)| {
        // Built out here so the test knows exactly which shapes that reading
        // produced, rather than assuming a count the damage may have changed.
        let mut damaged = scene(SessionId::new());
        damage(&mut damaged);
        let mut built: Vec<u64> = damaged.shapes().map(|shape| shape.index()).collect();
        built.sort_unstable();

        let mut damaged = Some(damaged);
        let mut importer = Importer::new(|_: &[u8]| {
            Ok(Import::Imported {
                scene: damaged.take().expect("read once"),
                diagnostics: Vec::new(),
            })
        });
        let error = document
            .reopen_step_import(object, &mut importer)
            .expect_err(&format!("{what} should not have bound"));
        assert_eq!(error.kind(), ErrorKind::Input, "{what}: {error}");
        // A refusal binds nothing, so it may hold nothing: every shape that
        // reading built was handed back to the session that made it.
        let mut released: Vec<u64> = importer.released.iter().map(ShapeHandle::index).collect();
        released.sort_unstable();
        assert_eq!(
            released, built,
            "{what}: the refused scene's shapes were not all given back"
        );
    };

    refuses("a definition that is no longer there", |scene| {
        scene.definitions.remove(1);
        for instance in &mut scene.instances {
            instance.definition = 0;
        }
    });
    refuses("a definition that was not there before", |scene| {
        let mut extra = scene.definitions[1].clone();
        extra.key = "step.product_definition#99".to_owned();
        extra.name = "Washer".to_owned();
        scene.definitions.push(extra);
    });
    refuses("a definition whose identity changed", |scene| {
        scene.definitions[1].key = "step.product_definition#99".to_owned();
    });
    refuses("two definitions claiming one identity", |scene| {
        scene.definitions[1].key = scene.definitions[0].key.clone();
    });
    refuses("a renamed definition", |scene| {
        scene.definitions[1].name = "Screw".to_owned();
    });
    refuses("a definition of a different size", |scene| {
        scene.definitions[0].solids = 2;
    });
    refuses("a different unit", |scene| {
        scene.source_unit = "INCH".to_owned();
    });
    refuses("a different schema", |scene| {
        scene.schema = "AP203".to_owned();
    });
    refuses("a flattened hierarchy", |scene| {
        scene.instances[1].parent = None;
    });
    refuses("a moved placement", |scene| {
        scene.instances[1].placement = moved(10.5);
    });
    refuses("a recoloured instance", |scene| {
        scene.instances[2].colour = [0.1, 0.2, 0.4];
    });
    refuses("a colour that now comes from elsewhere", |scene| {
        scene.instances[2].colour_source = ColourSource::Definition;
    });

    // A file that once imported and now does not is a refusal, not an empty
    // scene silently bound to nothing.
    let mut importer = Importer::new(|_: &[u8]| {
        Ok(Import::Rejected {
            diagnostics: vec![warned("syntax error")],
        })
    });
    let error = document
        .reopen_step_import(object, &mut importer)
        .expect_err("a rejection cannot be bound");
    assert!(error.to_string().contains("refused now"), "{error}");
}

#[test]
fn a_reference_resolves_only_inside_the_source_it_names() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");

    // Two imports of two different files whose definitions happen to share a
    // key. This is not contrived: the committed corpus does it, because a STEP
    // entity identifier is only ever unique within its own file.
    let first = ObjectId::new();
    let stored_first = document
        .store_step_import(StepImportRequest {
            object: first,
            name: Some("Plate"),
            source: SOURCE,
            source_name: None,
            import: &imported(SessionId::new(), Vec::new()),
            importer: &kernel(),
        })
        .expect("stores");

    let second = ObjectId::new();
    let other_bytes: &[u8] = b"ISO-10303-21;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";
    let stored_second = document
        .store_step_import(StepImportRequest {
            object: second,
            name: Some("Bracket"),
            source: other_bytes,
            source_name: None,
            import: &imported(SessionId::new(), Vec::new()),
            importer: &kernel(),
        })
        .expect("stores");
    document.close().expect("closes");

    assert_ne!(
        stored_first.source, stored_second.source,
        "two different files are two different sources"
    );

    // Both scenes describe a definition keyed step.product_definition#5.
    let shared = "step.product_definition#5";
    let into_first = ImportedDefinitionRef::new(stored_first.source, shared).expect("valid");
    let into_second = ImportedDefinitionRef::new(stored_second.source, shared).expect("valid");
    assert_ne!(
        into_first, into_second,
        "the source is part of the identity"
    );

    let document = Document::open(&path).expect("reopens");
    let later = SessionId::new();
    let mut importer = Importer::new(move |_: &[u8]| Ok(imported(later, Vec::new())));
    let reopened = document
        .reopen_step_import(second, &mut importer)
        .expect("binds");

    // The reference into this source resolves, and to this session's handle.
    let handle = reopened.resolve(&into_second).expect("resolves");
    assert_eq!(handle.session(), later);
    assert_eq!(
        handle,
        reopened
            .scene
            .definitions
            .iter()
            .find(|definition| definition.key == shared)
            .expect("the key is there")
            .shape
    );

    // The identical key belonging to the other source is refused, not
    // resolved to the plausible thing sitting right here.
    let error = reopened
        .resolve(&into_first)
        .expect_err("a key from another file must not resolve here");
    assert_eq!(error.kind(), ErrorKind::Input, "{error}");
    assert!(error.to_string().contains("another file"), "{error}");
}

#[test]
fn a_key_this_file_no_longer_describes_is_lost_not_approximated() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    let document = Document::open(&path).expect("reopens");
    let mut importer = Importer::new(|_: &[u8]| Ok(imported(SessionId::new(), Vec::new())));
    let reopened = document
        .reopen_step_import(object, &mut importer)
        .expect("binds");

    let missing =
        ImportedDefinitionRef::new(stored.source, "step.product_definition#404").expect("valid");
    let error = reopened
        .resolve(&missing)
        .expect_err("an unknown key must not resolve");

    // Lost, and reported as such: a document that opened and rebuilt and no
    // longer finds a name is a different thing from one that is malformed.
    assert_eq!(error.kind(), ErrorKind::Topology, "{error}");
    assert!(error.to_string().contains("no longer names"), "{error}");

    // Nothing was resolved to the nearest available part.
    for definition in &reopened.scene.definitions {
        assert_ne!(definition.key, missing.definition_key());
    }
}

#[test]
fn a_reference_must_name_something() {
    let source = ferritecad_types::ImportedSourceId::new();
    assert!(ImportedDefinitionRef::new(source, "").is_err());
    assert!(ImportedDefinitionRef::new(source, "   ").is_err());
    assert!(ImportedDefinitionRef::new(source, "step.product_definition#1").is_ok());

    // And a stored one is validated on the way back in rather than trusted
    // for having decoded. Written in the wire shape by hand, because the
    // constructor will not produce an empty key to encode.
    #[derive(serde::Serialize)]
    struct Raw {
        source: ferritecad_types::ImportedSourceId,
        definition_key: &'static str,
    }

    let mut empty = Vec::new();
    ciborium::into_writer(
        &Raw {
            source,
            definition_key: "",
        },
        &mut empty,
    )
    .expect("encodes");
    assert!(
        ciborium::from_reader::<ImportedDefinitionRef, _>(empty.as_slice()).is_err(),
        "an empty key decoded into a reference"
    );

    let valid =
        ImportedDefinitionRef::new(source, "step.product_definition#1").expect("valid reference");
    let mut whole = Vec::new();
    ciborium::into_writer(&valid, &mut whole).expect("encodes the public type");
    let read: ImportedDefinitionRef =
        ciborium::from_reader(whole.as_slice()).expect("a valid reference round-trips");
    assert_eq!(read, valid);
}

#[test]
fn a_document_written_before_identities_still_opens_and_binds() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    // Rewrite the object as a version 1 build left it: the same import with a
    // scene that names its definitions by position and has no keys at all.
    let legacy = as_version_1(current_scene(&stored));
    write_legacy_object(&path, object, &stored, &legacy);

    let document = Document::open(&path).expect("a version 1 document still opens");
    assert_eq!(
        document.access(),
        &Access::ReadWrite,
        "a version 1 import is understood, not merely preserved"
    );

    let read = document
        .step_import(object)
        .expect("reads")
        .expect("is there");
    assert_eq!(read.imported.scene.version(), 1);
    assert!(
        read.imported.scene.keys().is_none(),
        "a version 1 scene must not claim identities it does not have"
    );

    // It still binds, by the rule it was written under.
    let mut importer = Importer::new(|_: &[u8]| Ok(imported(SessionId::new(), Vec::new())));
    let reopened = document
        .reopen_step_import(object, &mut importer)
        .expect("a version 1 scene binds by position");
    assert_eq!(reopened.scene.definitions.len(), 2);
    assert_eq!(reopened.stored_version(), 1);

    // What it cannot do is answer a durable reference. The definitions in this
    // reading do carry keys — every import produces them now — but this
    // document never recorded which key belonged to which part, so resolving
    // against them would answer from this reading rather than from anything
    // stored. That is refused, and refused as its own kind: the key is not
    // lost, it never existed here.
    let present = reopened.scene.definitions[0].key.clone();
    let reference = ImportedDefinitionRef::new(stored.source, &present).expect("valid");
    let error = reopened
        .resolve(&reference)
        .expect_err("a version 1 scene has no identities to resolve against");
    assert_eq!(error.kind(), ErrorKind::Unsupported, "{error}");
    assert!(error.to_string().contains("never recorded"), "{error}");

    // And a reordering is refused, because position was all it ever had.
    let mut importer = Importer::new(|_: &[u8]| {
        let mut swapped = scene(SessionId::new());
        swapped.definitions.swap(0, 1);
        for instance in &mut swapped.instances {
            instance.definition = 1 - instance.definition;
        }
        Ok(Import::Imported {
            scene: swapped,
            diagnostics: Vec::new(),
        })
    });
    let error = document
        .reopen_step_import(object, &mut importer)
        .expect_err("version 1 cannot tell a reordering from a change");
    assert_eq!(error.kind(), ErrorKind::Input, "{error}");
    assert_eq!(
        importer.released.len(),
        2,
        "the refused reading kept its shapes"
    );
}

#[test]
fn a_document_written_before_placements_had_identities_still_opens_and_binds() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    // Rewrite the object as a version 2 build left it: keys on every
    // definition, and nothing at all saying which placement is which.
    let keyed = as_version_2(current_scene(&stored));
    write_object_at_layout(&path, object, &stored, 2, &keyed);

    let document = Document::open(&path).expect("a version 2 document still opens");
    assert_eq!(
        document.access(),
        &Access::ReadWrite,
        "a version 2 import is understood, not merely preserved"
    );

    let read = document
        .step_import(object)
        .expect("reads")
        .expect("is there");
    assert_eq!(read.imported.scene.version(), 2);
    assert_eq!(
        read.imported.scene.keys(),
        Some(vec![PLATE, BOLT]),
        "a version 2 scene keeps the definition identities it does have"
    );
    assert_eq!(
        read.imported.scene.occurrences(),
        StoredOccurrences::Unrecorded,
        "a version 2 scene must not claim placement identities it never recorded"
    );

    // It still binds, and a durable reference to a definition still resolves:
    // what version 2 lacks is placement identity and nothing else.
    let mut importer = Importer::new(|_: &[u8]| Ok(imported(SessionId::new(), Vec::new())));
    let reopened = document
        .reopen_step_import(object, &mut importer)
        .expect("a version 2 scene binds by key");
    assert_eq!(reopened.stored_version(), 2);
    assert_eq!(
        reopened.occurrences(),
        &StoredOccurrences::Unrecorded,
        "reopening invented placement identities the document never had"
    );
    let reference = ImportedDefinitionRef::new(stored.source, PLATE).expect("valid");
    reopened
        .resolve(&reference)
        .expect("a version 2 definition reference still resolves");

    // Reading it did not rewrite it, and neither does an edit elsewhere in the
    // document: the object goes back at the layout it came in at.
    let before = std::fs::read(&path).expect("snapshots the document");
    let record = document.object(object).expect("reads").expect("is there");
    assert_eq!(record.payload.schema_version(), 2);
    assert_eq!(
        std::fs::read(&path).expect("re-reads the document"),
        before,
        "opening and reading a version 2 document rewrote it"
    );
    let written = record.payload.to_storage_bytes().expect("writes back");
    let envelope = ferritecad_document::Envelope::from_bytes(&written).expect("decodes");
    assert_eq!(
        envelope.schema_version, 2,
        "a version 2 import was written back claiming identities it does not have"
    );
}

#[test]
fn two_placements_answering_to_one_identity_are_refused_before_anything_is_read() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    // The one damaged state the current layout's types permit. A missing
    // identity cannot be written at all — the field is not optional — and a
    // malformed one is refused while the UUID is still being read.
    let mut collided = current_scene(&stored).clone();
    assert!(collided.instances.len() >= 2);
    collided.instances[1].occurrence = collided.instances[0].occurrence;
    write_object_at_layout(&path, object, &stored, 3, &collided);

    let document = Document::open(&path).expect("the document still opens");
    let error = document
        .step_import(object)
        .expect_err("a scene whose placements collide is not read");
    assert_eq!(error.kind(), ErrorKind::Input, "{error}");
    assert!(error.to_string().contains("occurrence"), "{error}");
}

#[test]
fn a_placement_identity_that_is_not_a_uuidv7_is_refused_while_it_is_read() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    // A placement whose identity is a version 4 UUID. Written in the exact wire
    // form a real identity takes — sixteen CBOR bytes, not text — so the
    // refusal below is about the version nibble and not about a type mismatch
    // the reader would have caught for any value at all. The first spelling of
    // this gate got that wrong and a mutation campaign said so: a mutant that
    // removed the version check survived it.
    #[derive(serde::Serialize)]
    struct LooseInstance {
        occurrence: serde_bytes::ByteBuf,
        definition: String,
        parent: Option<u32>,
        name: String,
        placement: [f64; 12],
        colour_source: ferritecad_exchange::ColourSource,
        colour: [f64; 3],
    }
    #[derive(serde::Serialize)]
    struct LooseScene {
        source_unit: String,
        schema: String,
        definitions: Vec<ferritecad_exchange::PersistedDefinition>,
        instances: Vec<LooseInstance>,
    }

    let current = current_scene(&stored);
    let loose = LooseScene {
        source_unit: current.source_unit.clone(),
        schema: current.schema.clone(),
        definitions: current.definitions.clone(),
        instances: current
            .instances
            .iter()
            .enumerate()
            .map(|(index, instance)| LooseInstance {
                occurrence: serde_bytes::ByteBuf::from(if index == 0 {
                    // 550e8400-e29b-41d4-a716-446655440000, a version 4 UUID.
                    [
                        0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66,
                        0x55, 0x44, 0x00, 0x00,
                    ]
                } else {
                    instance.occurrence.to_bytes()
                }),
                definition: instance.definition.clone(),
                parent: instance.parent,
                name: instance.name.clone(),
                placement: instance.placement,
                colour_source: instance.colour_source,
                colour: instance.colour,
            })
            .collect(),
    };
    write_object_at_layout(&path, object, &stored, 3, &loose);

    let document = Document::open(&path).expect("the document still opens");
    let error = document
        .step_import(object)
        .expect_err("a placement identity that is not a UUIDv7 is not read");
    assert_eq!(error.kind(), ErrorKind::Input, "{error}");
    assert!(
        error.to_string().contains("UUIDv7") || format!("{error:?}").contains("UUIDv7"),
        "refused, but not for being the wrong kind of identifier: {error:?}"
    );
}

#[test]
fn a_current_layout_payload_with_no_placement_identities_is_refused() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    // A version 2 payload under a version 3 header. This is the header/payload
    // disagreement the format already refuses, and it is what stops a document
    // from being read as though it had identities it never wrote.
    let keyed = as_version_2(current_scene(&stored));
    write_object_at_layout(&path, object, &stored, 3, &keyed);

    let document = Document::open(&path).expect("the document still opens");
    let error = document
        .step_import(object)
        .expect_err("a version 2 payload is not a version 3 scene");
    assert_eq!(error.kind(), ErrorKind::Input, "{error}");
}

#[test]
fn an_import_is_stored_at_the_current_layout_and_declares_no_new_capability() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");

    assert_eq!(stored.scene.version(), 3);
    let record = document.object(object).expect("reads").expect("is there");
    assert_eq!(record.payload.schema_version(), 3);
    // Placement identity is a layout change, not a vocabulary one, so it
    // arrives as a version and adds no capability beside the one an imported
    // object has always required. A reader that cannot parse the layout is
    // already stopped by the layout.
    assert_eq!(
        record.payload.required_capabilities(),
        vec![IMPORTED_STEP_CAPABILITY.to_owned()]
    );
    assert_eq!(
        ObjectKind::ImportedStep.known_capabilities(),
        &[IMPORTED_STEP_CAPABILITY],
        "a second capability appeared for a layout change"
    );
    assert_eq!(
        ObjectKind::ImportedStep.readable_schema_versions(),
        &[3, 2, 1],
        "an older imported layout stopped being readable"
    );
}

/// The current-layout scene of a stored import.
fn current_scene(stored: &ImportedStep) -> &PersistedScene {
    match &stored.scene {
        StoredScene::V3(scene) => scene,
        other => panic!(
            "a fresh import stores a current-layout scene, found v{}",
            other.version()
        ),
    }
}

/// The same scene as version 1 recorded it: definitions by position, no keys
/// and no placement identities.
fn as_version_1(scene: &PersistedScene) -> LegacyScene {
    LegacyScene {
        source_unit: scene.source_unit.clone(),
        schema: scene.schema.clone(),
        definitions: scene
            .definitions
            .iter()
            .map(|definition| LegacyDefinition {
                name: definition.name.clone(),
                solids: definition.solids,
            })
            .collect(),
        instances: scene
            .instances
            .iter()
            .map(|instance| LegacyInstance {
                definition: scene
                    .definitions
                    .iter()
                    .position(|definition| definition.key == instance.definition)
                    .expect("the stored scene is consistent") as u32,
                parent: instance.parent,
                name: instance.name.clone(),
                placement: instance.placement,
                colour_source: instance.colour_source,
                colour: instance.colour,
            })
            .collect(),
    }
}

/// The same scene as version 2 recorded it: keys, and nothing that says which
/// placement is which.
fn as_version_2(scene: &PersistedScene) -> KeyedScene {
    KeyedScene {
        source_unit: scene.source_unit.clone(),
        schema: scene.schema.clone(),
        definitions: scene.definitions.clone(),
        instances: scene
            .instances
            .iter()
            .map(|instance| KeyedInstance {
                definition: instance.definition.clone(),
                parent: instance.parent,
                name: instance.name.clone(),
                placement: instance.placement,
                colour_source: instance.colour_source,
                colour: instance.colour,
            })
            .collect(),
    }
}

/// Rewrites an imported object's payload as a version 1 build would have.
///
/// There is no supported way to write one, and there should not be: a build
/// that has keys must not produce a scene without them. This reaches past the
/// writer for the one thing only a test needs — a document that predates the
/// format it is being read by.
fn write_legacy_object(
    path: &std::path::Path,
    object: ObjectId,
    stored: &ImportedStep,
    scene: &LegacyScene,
) {
    write_object_at_layout(path, object, stored, 1, scene);
}

/// Rewrites an imported object's payload at a chosen layout, with a chosen
/// scene.
///
/// There is no supported way to write an older layout, and there should not be:
/// a build that has placement identities must not produce a scene without them.
/// This reaches past the writer for the two things only a test needs — a
/// document that predates the format it is being read by, and a document
/// damaged in a way the writer would have refused.
fn write_object_at_layout<S: serde::Serialize>(
    path: &std::path::Path,
    object: ObjectId,
    stored: &ImportedStep,
    version: u32,
    scene: &S,
) {
    #[derive(serde::Serialize)]
    struct Payload<'a, S> {
        source: ferritecad_types::ImportedSourceId,
        source_hash: ContentHash,
        source_byte_len: u64,
        source_name: Option<String>,
        scene: &'a S,
        imported_by: ImporterIdentity,
        diagnostics_at_import: Vec<Diagnostic>,
    }

    let envelope = ferritecad_document::Envelope::encode(
        "exchange.step.imported",
        version,
        vec![IMPORTED_STEP_CAPABILITY.to_owned()],
        &Payload {
            source: stored.source,
            source_hash: stored.source_hash,
            source_byte_len: stored.source_byte_len,
            source_name: stored.source_name.clone(),
            scene,
            imported_by: stored.imported_by.clone(),
            diagnostics_at_import: stored.diagnostics_at_import.clone(),
        },
    )
    .expect("encodes")
    .to_bytes()
    .expect("serialises");

    with_sql(path, |conn| {
        conn.execute(
            "UPDATE objects SET schema_version = ?1, payload = ?2, payload_hash = ?3 \
             WHERE id = ?4",
            rusqlite::params![
                version,
                envelope.as_slice(),
                ContentHash::of_bytes(&envelope).as_bytes().as_slice(),
                object.to_bytes().as_slice()
            ],
        )
        .expect("updates");
    });
}

#[test]
fn a_reordered_import_binds_and_every_placement_keeps_its_own_geometry() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");

    // Two definitions sharing a name and differing in what they hold. Under
    // version 1 nothing but position told them apart, so this was the case
    // that had to be refused; with keys it is an ordinary reordering, and the
    // point is that each placement still gets the geometry it asked for.
    let same_name = |session: SessionId| {
        let mut scene = scene(session);
        scene.definitions[1].name = "Plate".to_owned();
        scene.definitions[1].solids = 3;
        scene
    };

    let object = ObjectId::new();
    document
        .store_step_import(StepImportRequest {
            object,
            name: None,
            source: SOURCE,
            source_name: None,
            import: &Import::Imported {
                scene: same_name(SessionId::new()),
                diagnostics: Vec::new(),
            },
            importer: &kernel(),
        })
        .expect("stores");
    document.close().expect("closes");

    let later = SessionId::new();
    let document = Document::open(&path).expect("reopens");
    let mut importer = Importer::new(|_: &[u8]| {
        let mut swapped = same_name(later);
        swapped.definitions.swap(0, 1);
        for instance in &mut swapped.instances {
            instance.definition = 1 - instance.definition;
        }
        Ok(Import::Imported {
            scene: swapped,
            diagnostics: Vec::new(),
        })
    });
    let reopened = document
        .reopen_step_import(object, &mut importer)
        .expect("a reordered import describes the same assembly");
    assert!(
        importer.released.is_empty(),
        "a successful binding gave shapes back"
    );

    // The assembly is the plate, and the two bolts are the three-solid part.
    // Reading that off the keys rather than off the positions is the whole
    // difference between this and version 1.
    let bound = &reopened.scene;
    let placed = |instance: usize| &bound.definitions[bound.instances[instance].definition];
    assert_eq!(placed(0).key, "step.product_definition#5");
    assert_eq!(placed(0).solids, 1);
    assert_eq!(placed(1).key, "step.product_definition#31");
    assert_eq!(placed(1).solids, 3);
    assert_eq!(placed(2).key, "step.product_definition#31");

    // And the handles are this session's, attached to the part each placement
    // actually names.
    assert_eq!(placed(1).shape.session(), later);
    assert_ne!(placed(0).shape, placed(1).shape);
}

#[test]
fn another_kernel_may_reopen_what_this_one_stored() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    let linux =
        KernelIdentity::new("occt", "8.0.1", "x86_64-unknown-linux-gnu/abc").expect("valid");
    let object = ObjectId::new();
    document
        .store_step_import(StepImportRequest {
            object,
            name: None,
            source: SOURCE,
            source_name: None,
            import: &imported(SessionId::new(), Vec::new()),
            importer: &linux,
        })
        .expect("stores");
    document.close().expect("closes");

    // The same release on another platform: `build` carries the target triple,
    // so it differs by construction. A document that could not cross platforms
    // would be no use, and the scene is what has to agree.
    let document = Document::open(&path).expect("reopens");
    let mac = KernelIdentity::new("occt", "8.0.1", "aarch64-apple-darwin/different-build")
        .expect("valid");
    let mut importer = Importer::with_identity(mac.clone(), |_: &[u8]| {
        Ok(imported(SessionId::new(), Vec::new()))
    });
    let reopened = document
        .reopen_step_import(object, &mut importer)
        .expect("another platform reopens it");

    assert_eq!(reopened.imported_by, ImporterIdentity::of(&linux));
    assert_eq!(
        reopened.imported_by.build, "x86_64-unknown-linux-gnu/abc",
        "the identity is provenance and is reported as read, not as re-observed"
    );
    assert_eq!(
        reopened.reopened_by,
        ImporterIdentity::of(&mac),
        "the current diagnostics and handles must name their own producer"
    );
}

#[test]
fn a_refused_import_leaves_the_document_byte_for_byte_as_it_was() {
    let (_dir, path) = workspace();

    Document::create(&path)
        .expect("creates")
        .close()
        .expect("closes");
    // Settle first: opening migrates and sets persistent pragmas, so a file
    // compared before its first open would differ for reasons that have
    // nothing to do with an import.
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

    let mut document = Document::open(&path).expect("opens");

    // A rejection has no scene, and the store path says so without opening a
    // transaction.
    let rejected = Import::Rejected {
        diagnostics: vec![warned("unexpected end of file")],
    };
    let error = document
        .store_step_import(StepImportRequest {
            object: ObjectId::new(),
            name: None,
            source: SOURCE,
            source_name: None,
            import: &rejected,
            importer: &kernel(),
        })
        .expect_err("there is nothing to store");
    assert_eq!(error.kind(), ErrorKind::Input);

    // And an edit that writes bytes and then fails takes them with it.
    let error = document
        .write(|w| {
            w.put_step_source(SOURCE)?;
            Err::<(), _>(CadError::constraint("the caller changed its mind"))
        })
        .expect_err("the closure failed, so the edit must fail");
    assert_eq!(error.kind(), ErrorKind::Constraint);
    document.close().expect("closes");

    assert_eq!(
        std::fs::read(&path).expect("reads"),
        before,
        "a refused import changed the file"
    );
    let document = Document::open(&path).expect("opens");
    assert_eq!(document.meta().modified_at, modified_before);
}

#[test]
fn the_same_file_stored_twice_is_one_copy_of_the_bytes() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");

    let (_, first) = store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    let (_, second) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    assert_eq!(
        first.source, second.source,
        "identical content is the same source, not a copy of it"
    );
    document.close().expect("closes");

    let (sources, blobs): (i64, i64) = with_sql(&path, |conn| {
        conn.query_row(
            "SELECT count(*), sum(byte_len) FROM imported_sources",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("counts")
    });
    assert_eq!(sources, 1);
    assert_eq!(blobs, SOURCE.len() as i64);

    // Different bytes are a different source, never an edit of this one.
    let mut document = Document::open(&path).expect("reopens");
    let object = ObjectId::new();
    let other = document
        .store_step_import(StepImportRequest {
            object,
            name: None,
            source: b"ISO-10303-21;\nEND-ISO-10303-21;\n",
            source_name: None,
            import: &imported(SessionId::new(), Vec::new()),
            importer: &kernel(),
        })
        .expect("stores");
    assert_ne!(other.source, first.source);
    assert_ne!(other.source_hash, first.source_hash);
}

#[test]
fn removing_an_imported_object_leaves_no_bytes_nothing_can_reach() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");

    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document
        .write(|w| w.remove_object(object))
        .expect("removing an import is allowed");
    document.close().expect("closes");

    let (sources, refs): (i64, i64) = with_sql(&path, |conn| {
        conn.query_row(
            "SELECT (SELECT count(*) FROM imported_sources),
                    (SELECT count(*) FROM imported_source_refs)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("counts")
    });
    assert_eq!(refs, 0, "the claim on the bytes outlived the object");
    assert_eq!(
        sources, 0,
        "source {} is still in the file and nothing can reach it",
        stored.source
    );

    let document = Document::open(&path).expect("reopens");
    assert!(document.step_import(object).expect("reads").is_none());
}

#[test]
fn bytes_nothing_claims_do_not_survive_the_edit_that_wrote_them() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");

    // Reachability is what keeps bytes in a document. A source written without
    // an object to claim it is not a store for later; it is collected here, so
    // it can never be mistaken for one.
    document
        .write(|w| {
            w.put_step_source(SOURCE)?;
            Ok(())
        })
        .expect("the edit itself succeeds");
    document.close().expect("closes");

    let sources: i64 = with_sql(&path, |conn| {
        conn.query_row("SELECT count(*) FROM imported_sources", [], |row| {
            row.get(0)
        })
        .expect("counts")
    });
    assert_eq!(sources, 0);
}

#[test]
fn corrupted_source_bytes_are_noticed_before_any_importer_sees_them() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    // One byte, in place, so the declared length still holds and only the hash
    // can tell. This is the corruption a checksum exists for.
    let mut damaged = SOURCE.to_vec();
    damaged[8] = b'X';
    with_sql(&path, |conn| {
        conn.execute(
            "UPDATE imported_sources SET bytes = ?1 WHERE id = ?2",
            rusqlite::params![damaged.as_slice(), stored.source.to_bytes().as_slice()],
        )
        .expect("the blob is rewritable from outside");
    });

    let document = Document::open(&path).expect("reopens");
    let report = document.validate().expect("validation itself runs");
    assert!(
        report
            .errors()
            .any(|diagnostic| diagnostic.code == "imported-source.invalid"),
        "validate must not call a document rebuildable when its source bytes are corrupt: {report:?}"
    );
    let mut importer = Importer::new(|_: &[u8]| Ok(imported(SessionId::new(), Vec::new())));
    let error = document
        .reopen_step_import(object, &mut importer)
        .expect_err("damaged bytes must not be imported");
    assert!(
        !importer.asked,
        "the importer was handed bytes this document already knew were wrong"
    );
    assert!(error.to_string().contains("stored hash"), "{error}");
}

#[test]
fn a_source_row_swapped_under_an_object_is_caught_by_the_object() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    // Bytes and hash consistent with each other, and not the ones this object
    // was built from. The row's own integrity check passes; the object's does
    // not, which is why both exist.
    let other: &[u8] = b"ISO-10303-21;\nEND-ISO-10303-21;\n";
    with_sql(&path, |conn| {
        conn.execute(
            "UPDATE imported_sources SET bytes = ?1, content_hash = ?2, byte_len = ?3 WHERE id = ?4",
            rusqlite::params![
                other,
                ContentHash::of_bytes(other).as_bytes().as_slice(),
                other.len() as i64,
                stored.source.to_bytes().as_slice()
            ],
        )
        .expect("the row is rewritable from outside");
    });

    let document = Document::open(&path).expect("reopens");
    let error = document
        .step_import(object)
        .expect_err("the object records what it was built from");
    assert!(error.to_string().contains("recorded"), "{error}");
}

#[test]
fn a_missing_source_is_reported_as_the_loss_it_is() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    with_sql(&path, |conn| {
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .expect("pragma applies");
        conn.execute(
            "DELETE FROM imported_sources WHERE id = ?1",
            rusqlite::params![stored.source.to_bytes().as_slice()],
        )
        .expect("deletes");
    });

    let document = Document::open(&path).expect("reopens");
    let error = document
        .step_import(object)
        .expect_err("the bytes are gone");
    assert!(error.to_string().contains("only copy"), "{error}");
}

#[test]
fn an_imported_object_cannot_be_written_as_a_plain_object() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");

    // put_object would write the payload and no claim on the bytes, and the
    // bytes would be collected at the end of that very transaction.
    let error = document
        .write(|w| {
            w.put_object(
                object,
                None,
                0,
                None,
                &ObjectPayload::ImportedStep(stored.clone()),
            )
        })
        .expect_err("this object owns bytes as well as a payload");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(error.to_string().contains("put_imported_step"), "{error}");
    document.close().expect("closes");
}

#[test]
fn replacing_an_import_with_an_ordinary_object_releases_its_source() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, _) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");

    document
        .write(|w| {
            w.put_object(
                object,
                None,
                0,
                Some("Native body"),
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            Ok(())
        })
        .expect("replaces the object");
    document.close().expect("closes");

    let (sources, refs): (i64, i64) = with_sql(&path, |conn| {
        conn.query_row(
            "SELECT (SELECT count(*) FROM imported_sources),
                    (SELECT count(*) FROM imported_source_refs)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("counts")
    });
    assert_eq!((sources, refs), (0, 0));
}

#[test]
fn a_low_level_writer_cannot_pair_an_object_with_different_source_facts() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let result = document.write(|w| {
        let source = w.put_step_source(SOURCE)?;
        w.put_imported_step(
            ObjectId::new(),
            None,
            0,
            None,
            &ImportedStep {
                source,
                source_hash: ContentHash::of_bytes(b"different"),
                source_byte_len: SOURCE.len() as u64,
                source_name: None,
                scene: StoredScene::V3(scene(SessionId::new()).persist()?),
                imported_by: ImporterIdentity::of(&kernel()),
                diagnostics_at_import: Vec::new(),
            },
        )?;
        Ok(())
    });
    assert!(
        result.is_err(),
        "the mismatch must be refused while writing"
    );
    document.close().expect("closes");

    let (objects, sources): (i64, i64) = with_sql(&path, |conn| {
        conn.query_row(
            "SELECT (SELECT count(*) FROM objects),
                    (SELECT count(*) FROM imported_sources)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("counts")
    });
    assert_eq!((objects, sources), (0, 0));
}

#[test]
fn a_corrupt_deduplicated_source_is_not_reused_for_a_new_import() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (_, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    let mut damaged = SOURCE.to_vec();
    damaged[8] ^= 1;
    with_sql(&path, |conn| {
        conn.execute(
            "UPDATE imported_sources SET bytes = ?1 WHERE id = ?2",
            rusqlite::params![damaged, stored.source.to_bytes().as_slice()],
        )
        .expect("damages while preserving length and recorded hash");
    });

    let mut document = Document::open(&path).expect("opens");
    let before = document.objects().expect("reads").len();
    let error = document
        .store_step_import(StepImportRequest {
            object: ObjectId::new(),
            name: None,
            source: SOURCE,
            source_name: None,
            import: &imported(SessionId::new(), Vec::new()),
            importer: &kernel(),
        })
        .expect_err("a corrupt row must not be reused by hash alone");
    assert!(error.to_string().contains("different bytes"), "{error}");
    assert_eq!(document.objects().expect("reads").len(), before);
}

#[test]
fn a_missing_reachability_row_blocks_edits_before_gc_can_destroy_the_source() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    with_sql(&path, |conn| {
        conn.execute(
            "DELETE FROM imported_source_refs WHERE object_id = ?1",
            rusqlite::params![object.to_bytes().as_slice()],
        )
        .expect("removes the reachability row");
    });

    let mut document = Document::open(&path).expect("opens");
    let report = document.validate().expect("validation itself runs");
    assert!(
        report.errors().any(|diagnostic| {
            matches!(
                diagnostic.code,
                "imported-source.invalid" | "imported-source.unreachable"
            )
        }),
        "validate must report a broken ownership row: {report:?}"
    );
    let new_object = ObjectId::new();
    let error = document
        .write(|w| {
            w.put_object(
                new_object,
                None,
                0,
                None,
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            Ok(())
        })
        .expect_err("an unrelated edit must not collect recoverable bytes");
    assert!(error.to_string().contains("exactly one source"), "{error}");
    document.close().expect("closes");

    let (sources, objects): (i64, i64) = with_sql(&path, |conn| {
        conn.query_row(
            "SELECT (SELECT count(*) FROM imported_sources),
                    (SELECT count(*) FROM objects WHERE id = ?1)",
            rusqlite::params![new_object.to_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("counts")
    });
    assert_eq!(sources, 1, "source {} was destroyed", stored.source);
    assert_eq!(objects, 0, "the unrelated edit partially committed");
}

#[test]
fn an_import_this_build_cannot_read_keeps_its_bytes_and_its_document() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    let original = document
        .object(object)
        .expect("reads")
        .expect("is there")
        .payload_hash;
    document.close().expect("closes");

    // Rewrite the envelope as a build from the future would have left it: same
    // type, a layout this build does not know, a capability it cannot claim.
    let future = ferritecad_document::Envelope::new(
        "exchange.step.imported",
        2,
        vec!["exchange.step.imported.v2".to_owned()],
        vec![0xf6],
    )
    .to_bytes()
    .expect("serialises");
    with_sql(&path, |conn| {
        conn.execute(
            "UPDATE objects SET schema_version = 2, payload = ?1, payload_hash = ?2 WHERE id = ?3",
            rusqlite::params![
                future.as_slice(),
                ContentHash::of_bytes(&future).as_bytes().as_slice(),
                object.to_bytes().as_slice()
            ],
        )
        .expect("updates");
    });

    let document = Document::open(&path).expect("opens anyway");
    match document.access() {
        Access::ReadOnly { reason } => assert!(reason.contains("exchange.step.imported.v2")),
        other => panic!("a capability this build lacks must not be writable, found {other:?}"),
    }

    let record = document.object(object).expect("reads").expect("is there");
    assert!(matches!(record.payload, ObjectPayload::Unknown(_)));
    assert_ne!(record.payload_hash, original);
    // Preserved verbatim, and the source it names is still there because no
    // write — and so no reclamation — can happen in a read-only document.
    assert_eq!(
        record.payload.to_storage_bytes().expect("writes back"),
        future
    );
    document.close().expect("closes");

    let (sources, refs): (i64, i64) = with_sql(&path, |conn| {
        conn.query_row(
            "SELECT (SELECT count(*) FROM imported_sources),
                    (SELECT count(*) FROM imported_source_refs)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("counts")
    });
    assert_eq!(sources, 1, "source {} was reclaimed", stored.source);
    assert_eq!(
        refs, 1,
        "the unreadable object's claim on its bytes was lost"
    );
}

#[test]
fn a_newer_import_layout_with_a_known_capability_survives_an_edit() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    // Simulate exactly the compatibility decision made for scene v3 from the
    // point of view of this reader: the capability is already understood but
    // the payload layout is newer. The envelope must be preserved as an
    // unknown object without making the whole document read-only.
    let future = ferritecad_document::Envelope::new(
        "exchange.step.imported",
        4,
        vec![IMPORTED_STEP_CAPABILITY.to_owned()],
        vec![0xf6],
    )
    .to_bytes()
    .expect("serialises");
    with_sql(&path, |conn| {
        conn.execute(
            "UPDATE objects SET schema_version = 4, payload = ?1, payload_hash = ?2 WHERE id = ?3",
            rusqlite::params![
                future.as_slice(),
                ContentHash::of_bytes(&future).as_bytes().as_slice(),
                object.to_bytes().as_slice()
            ],
        )
        .expect("updates");
    });

    let mut document = Document::open(&path).expect("opens");
    assert_eq!(
        document.access(),
        &Access::ReadWrite,
        "a known capability does not make an unknown layout read-only"
    );
    let record = document.object(object).expect("reads").expect("is there");
    assert!(matches!(record.payload, ObjectPayload::Unknown(_)));
    assert_eq!(
        record.payload.to_storage_bytes().expect("writes back"),
        future
    );

    // Exercise the successful write path, including source reclamation and
    // capability-index rebuilding. Neither may discard data owned by an
    // envelope whose layout this reader cannot inspect.
    document
        .write(|writer| {
            writer.put_object(
                ObjectId::new(),
                None,
                1,
                Some("Body1"),
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            Ok(())
        })
        .expect("an unrelated edit succeeds");
    document.close().expect("closes");

    let document = Document::open(&path).expect("reopens");
    let record = document.object(object).expect("reads").expect("is there");
    assert_eq!(
        record.payload.to_storage_bytes().expect("writes back"),
        future,
        "the unknown imported envelope was rewritten"
    );
    document.close().expect("closes");

    let (sources, refs): (i64, i64) = with_sql(&path, |conn| {
        conn.query_row(
            "SELECT (SELECT count(*) FROM imported_sources),
                    (SELECT count(*) FROM imported_source_refs
                     WHERE object_id = ?1 AND source_id = ?2)",
            rusqlite::params![
                object.to_bytes().as_slice(),
                stored.source.to_bytes().as_slice()
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("counts")
    });
    assert_eq!(sources, 1, "source {} was reclaimed", stored.source);
    assert_eq!(refs, 1, "the unknown object's source claim was lost");
}

#[test]
fn an_import_declares_its_own_capability_and_its_own_format() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, _) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    let document = Document::open(&path).expect("reopens");
    assert_eq!(document.access(), &Access::ReadWrite);
    let record = document.object(object).expect("reads").expect("is there");
    assert_eq!(
        record.payload.required_capabilities(),
        vec![IMPORTED_STEP_CAPABILITY.to_owned()]
    );
    document.close().expect("closes");

    let format: String = with_sql(&path, |conn| {
        conn.query_row("SELECT format FROM imported_sources", [], |row| row.get(0))
            .expect("reads")
    });
    assert_eq!(format, STEP_SOURCE_FORMAT);
}

/// Opens the document as a plain SQLite database, which is how a document gets
/// damaged in the first place and the only way to arrange it deliberately.
fn with_sql<T>(path: &std::path::Path, edit: impl FnOnce(&Connection) -> T) -> T {
    let conn = Connection::open(path).expect("the document is a SQLite file");
    let value = edit(&conn);
    conn.close().expect("closes");
    value
}
