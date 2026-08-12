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
    Access, Body, Document, IMPORTED_STEP_CAPABILITY, ImportedStep, ImporterIdentity,
    ObjectPayload, STEP_SOURCE_FORMAT, StepImportRequest, StepImporter,
};
use ferritecad_exchange::{
    ColourSource, Definition, Diagnostic, Import, Instance, LegacyDefinition, LegacyInstance,
    LegacyScene, Scene, Severity, Stage, StoredScene,
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

    // Everything portable came back as it went in.
    let persisted = reopened.scene.persist().expect("projects");
    assert_eq!(StoredScene::V2(persisted.clone()), stored.scene);
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
fn a_document_written_before_identities_still_opens_and_binds() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let (object, stored) =
        store(&mut document, &imported(SessionId::new(), Vec::new())).expect("stores");
    document.close().expect("closes");

    // Rewrite the object as a version 1 build left it: the same import with a
    // scene that names its definitions by position and has no keys at all.
    let StoredScene::V2(scene_v2) = &stored.scene else {
        panic!("a fresh import stores a v2 scene");
    };
    let legacy = LegacyScene {
        source_unit: scene_v2.source_unit.clone(),
        schema: scene_v2.schema.clone(),
        definitions: scene_v2
            .definitions
            .iter()
            .map(|definition| LegacyDefinition {
                name: definition.name.clone(),
                solids: definition.solids,
            })
            .collect(),
        instances: scene_v2
            .instances
            .iter()
            .map(|instance| LegacyInstance {
                definition: scene_v2
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
    };
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
    #[derive(serde::Serialize)]
    struct LegacyPayload<'a> {
        source: ferritecad_types::ImportedSourceId,
        source_hash: ContentHash,
        source_byte_len: u64,
        source_name: Option<String>,
        scene: &'a LegacyScene,
        imported_by: ImporterIdentity,
        diagnostics_at_import: Vec<Diagnostic>,
    }

    let envelope = ferritecad_document::Envelope::encode(
        "exchange.step.imported",
        1,
        vec![IMPORTED_STEP_CAPABILITY.to_owned()],
        &LegacyPayload {
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
            "UPDATE objects SET schema_version = 1, payload = ?1, payload_hash = ?2 WHERE id = ?3",
            rusqlite::params![
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
                scene: StoredScene::V2(scene(SessionId::new()).persist()?),
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
