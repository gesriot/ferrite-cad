// SPDX-License-Identifier: MIT
//! Storing an imported file and reading it back into a new kernel session.
//!
//! No Open CASCADE here. The importer is a closure, which is exactly the shape
//! the real one has, and it lets these tests state things a kernel cannot be
//! asked to produce on demand: a scene whose definitions were reordered, a
//! source blob corrupted underneath a document, a reading that now refuses a
//! file it once accepted. What runs against the real kernel is in
//! `crates/ferritecad-occt/tests/step_persistence_occt.rs`.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::cell::Cell;

use ferritecad_document::{
    Access, Document, IMPORTED_STEP_CAPABILITY, ImportedStep, ImporterIdentity, ObjectPayload,
    STEP_SOURCE_FORMAT, StepImportRequest,
};
use ferritecad_exchange::{
    ColourSource, Definition, Diagnostic, Import, Instance, Scene, Severity, Stage,
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
            },
            Definition {
                shape: ShapeHandle::new(session, 2),
                name: "Bolt".to_owned(),
                solids: 1,
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
    let reopened = document
        .reopen_step_import(object, |bytes| {
            assert_eq!(bytes, SOURCE, "the importer must be given the stored bytes");
            Ok(Import::Imported {
                scene: current,
                diagnostics: Vec::new(),
            })
        })
        .expect("the same file binds");

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
    assert_eq!(persisted, stored.scene);
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
    let reopened = document
        .reopen_step_import(object, |_| {
            Ok(Import::Imported {
                scene: scene(SessionId::new()),
                diagnostics: now.clone(),
            })
        })
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
        let error = document
            .reopen_step_import(object, |_| {
                let mut current = scene(SessionId::new());
                damage(&mut current);
                Ok(Import::Imported {
                    scene: current,
                    diagnostics: Vec::new(),
                })
            })
            .expect_err(&format!("{what} should not have bound"));
        assert_eq!(error.kind(), ErrorKind::Input, "{what}: {error}");
    };

    refuses("a reordered pair of definitions", |scene| {
        scene.definitions.swap(0, 1);
        for instance in &mut scene.instances {
            instance.definition = 1 - instance.definition;
        }
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
    let error = document
        .reopen_step_import(object, |_| {
            Ok(Import::Rejected {
                diagnostics: vec![warned("syntax error")],
            })
        })
        .expect_err("a rejection cannot be bound");
    assert!(error.to_string().contains("refused now"), "{error}");
}

#[test]
fn definitions_of_the_same_name_are_not_told_apart_by_it() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");

    // Two definitions sharing a name and differing in what they hold. Nothing
    // but position distinguishes them, so nothing but position may bind them.
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

    let document = Document::open(&path).expect("reopens");
    let error = document
        .reopen_step_import(object, |_| {
            let mut swapped = same_name(SessionId::new());
            swapped.definitions.swap(0, 1);
            for instance in &mut swapped.instances {
                instance.definition = 1 - instance.definition;
            }
            Ok(Import::Imported {
                scene: swapped,
                diagnostics: Vec::new(),
            })
        })
        .expect_err("a shared name must not license a swap");
    assert!(
        error.to_string().contains("solid count"),
        "the refusal should name what gave it away: {error}"
    );
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
    let reopened = document
        .reopen_step_import(object, |_| Ok(imported(SessionId::new(), Vec::new())))
        .expect("another platform reopens it");

    assert_eq!(reopened.imported_by, ImporterIdentity::of(&linux));
    assert_eq!(
        reopened.imported_by.build, "x86_64-unknown-linux-gnu/abc",
        "the identity is provenance and is reported as read, not as re-observed"
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
    let asked = Cell::new(false);
    let error = document
        .reopen_step_import(object, |_| {
            asked.set(true);
            Ok(imported(SessionId::new(), Vec::new()))
        })
        .expect_err("damaged bytes must not be imported");
    assert!(
        !asked.get(),
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
