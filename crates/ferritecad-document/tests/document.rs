// SPDX-License-Identifier: MIT
//! End-to-end checks on the document format.
//!
//! These are the stage 1 gate: a document must survive a full save, reload and
//! re-save with its meaning intact, must preserve what it did not understand,
//! must refuse to write when writing would lose something, and must not depend
//! on its cache for any of it.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Access, Body, CORE_CAPABILITY, CacheStore, CapSide, DatumPlane, Dependency, DependencyRole,
    Document, EXTRUDE_CAP_EDGE_CAPABILITY, EndCondition, EntityKind, Envelope, Expression, Extrude,
    ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch, SketchCurve, SketchGeometry,
    SolidOperation, TopologyRef,
};
use ferritecad_types::{
    CadError, ContentHash, ErrorKind, ObjectId, Result, StableEntityId, Transform, Unit,
};
use tempfile::TempDir;

/// A plane, a square profile on it, an extrusion and a body.
struct Plate {
    plane: ObjectId,
    sketch: ObjectId,
    extrude: ObjectId,
    body: ObjectId,
    first_segment: StableEntityId,
}

fn populate(document: &mut Document) -> Result<Plate> {
    let plate = Plate {
        plane: ObjectId::new(),
        sketch: ObjectId::new(),
        extrude: ObjectId::new(),
        body: ObjectId::new(),
        first_segment: StableEntityId::new(),
    };

    let curves = vec![
        SketchCurve {
            id: plate.first_segment,
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::ORIGIN,
                end: Point2::new(20.0, 0.0)?,
            },
        },
        SketchCurve {
            id: StableEntityId::new(),
            construction: true,
            geometry: SketchGeometry::Circle {
                center: Point2::new(10.0, 10.0)?,
                radius: 4.0,
            },
        },
    ];

    document.write(|w| {
        w.put_object(
            plate.plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        w.put_object(
            plate.sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane: plate.plane,
                curves,
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: plate.sketch,
            dependency: plate.plane,
            role: DependencyRole::Plane,
        })?;
        w.put_object(
            plate.extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                profile: plate.sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::new("thickness", 8.0)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                target_body: None,
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: plate.extrude,
            dependency: plate.sketch,
            role: DependencyRole::Profile,
        })?;
        w.put_object(
            plate.body,
            None,
            3,
            Some("Plate"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(plate.extrude),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: plate.body,
            dependency: plate.extrude,
            role: DependencyRole::BodyTip,
        })?;
        w.put_topology_ref(&TopologyRef {
            id: StableEntityId::new(),
            owner: plate.extrude,
            producer_feature: plate.extrude,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::ExtrudeSide {
                profile_segment: plate.first_segment,
            },
            selection: SelectionRule::AllDerivedFrom {
                ancestor: plate.first_segment,
            },
            fallback_signature: None,
        })?;
        Ok(())
    })?;

    Ok(plate)
}

fn workspace() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("part.fcad");
    (dir, path)
}

#[test]
fn a_document_survives_save_reload_and_resave_unchanged() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    populate(&mut document).expect("populates");
    let before_objects = document.objects().expect("reads");
    let before_deps = document.dependencies().expect("reads");
    let before_refs = document.topology_refs().expect("reads");
    let before_id = document.meta().document_id;
    document.close().expect("closes");

    let mut reopened = Document::open(&path).expect("reopens");
    assert_eq!(reopened.meta().document_id, before_id);
    assert_eq!(reopened.objects().expect("reads"), before_objects);
    assert_eq!(reopened.dependencies().expect("reads"), before_deps);
    assert_eq!(reopened.topology_refs().expect("reads"), before_refs);

    // Rewriting every object must be a no-op on meaning.
    reopened
        .write(|w| {
            for object in &before_objects {
                w.put_object(
                    object.id,
                    object.parent,
                    object.ordinal,
                    object.name.as_deref(),
                    &object.payload,
                )?;
            }
            Ok(())
        })
        .expect("rewrites");
    reopened.close().expect("closes");

    let final_read = Document::open(&path).expect("reopens");
    assert_eq!(final_read.objects().expect("reads"), before_objects);
    assert_eq!(final_read.topology_refs().expect("reads"), before_refs);
}

#[test]
fn an_explicitly_read_only_open_can_read_but_never_be_promoted_to_a_writer() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    populate(&mut document).expect("populates");
    document.close().expect("closes");
    let before = std::fs::read(&path).expect("reads before");

    let mut opened = Document::open_read_only(&path).expect("opens without writing");
    assert!(matches!(opened.access(), Access::ReadOnly { .. }));
    assert!(!opened.objects().expect("reads objects").is_empty());
    let error = opened
        .write(|_| Ok(()))
        .expect_err("the connection cannot be promoted to a writer");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    opened.close().expect("closes");

    assert_eq!(std::fs::read(&path).expect("reads after"), before);
}

#[test]
fn a_read_only_open_refuses_wal_without_changing_the_directory() {
    let (dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    populate(&mut document).expect("populates");
    document.close().expect("closes");

    // WAL is valid SQLite state but differs from FerriteCAD's normal DELETE
    // mode. The ordinary open path deliberately normalises it and therefore
    // changes the source file; this makes that write observable.
    let conn = rusqlite::Connection::open(&path).expect("opens raw");
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .expect("sets WAL");
    assert_eq!(mode, "wal");
    drop(conn);

    let before = std::fs::read(&path).expect("reads before");
    let mut before_entries: Vec<_> = std::fs::read_dir(dir.path())
        .expect("lists before")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    before_entries.sort();

    let error = Document::open_read_only(&path).expect_err("WAL would need auxiliary files");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(error.to_string().contains("WAL"), "{error}");

    assert_eq!(std::fs::read(&path).expect("reads after"), before);
    let mut after_entries: Vec<_> = std::fs::read_dir(dir.path())
        .expect("lists after")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    after_entries.sort();
    assert_eq!(
        after_entries, before_entries,
        "opening left an auxiliary file"
    );

    assert_eq!(&before[18..20], &[2, 2], "the WAL header remains intact");
}

#[test]
fn a_read_only_open_refuses_an_old_schema_instead_of_migrating_it() {
    let (_dir, path) = workspace();
    Document::create(&path)
        .expect("creates")
        .close()
        .expect("closes");

    let conn = rusqlite::Connection::open(&path).expect("opens raw");
    conn.pragma_update(None, "user_version", 1i64)
        .expect("marks the schema old");
    drop(conn);
    let before = std::fs::read(&path).expect("reads before");

    let error = Document::open_read_only(&path).expect_err("migration would be a write");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(error.to_string().contains("needs migration"), "{error}");
    assert_eq!(std::fs::read(&path).expect("reads after"), before);
}

#[test]
fn an_object_of_an_unknown_type_survives_a_full_cycle_byte_for_byte() {
    let (_dir, path) = workspace();
    let id = ObjectId::new();

    // Something a future build writes and this one has never heard of.
    let original = Envelope::new(
        "feature.loft",
        3,
        vec!["core.part.v1".to_owned()],
        vec![0x83, 0x01, 0x02, 0x03],
    )
    .to_bytes()
    .expect("serialises");
    let payload = ObjectPayload::from_storage_bytes(&original).expect("header is readable");
    assert!(matches!(payload, ObjectPayload::Unknown(_)));

    let mut document = Document::create(&path).expect("creates");
    document
        .write(|w| {
            w.put_object(id, None, 0, Some("Loft1"), &payload)?;
            Ok(())
        })
        .expect("writes");
    document.close().expect("closes");

    let mut reopened = Document::open(&path).expect("reopens");
    let read = reopened.object(id).expect("reads").expect("present");
    assert_eq!(
        read.payload.to_storage_bytes().expect("re-encodes"),
        original,
        "an unknown payload must be returned exactly as it was stored"
    );

    // And a save that touches it must still not alter it.
    reopened
        .write(|w| {
            w.put_object(id, None, 0, Some("Loft1"), &read.payload)?;
            Ok(())
        })
        .expect("rewrites");
    reopened.close().expect("closes");

    let final_read = Document::open(&path).expect("reopens");
    assert_eq!(
        final_read
            .object(id)
            .expect("reads")
            .expect("present")
            .payload
            .to_storage_bytes()
            .expect("re-encodes"),
        original
    );
}

#[test]
fn a_document_needing_an_unimplemented_capability_opens_read_only() {
    let (_dir, path) = workspace();

    let future = Envelope::new(
        "feature.sheetmetal",
        1,
        vec!["sheetmetal.v1".to_owned()],
        vec![0xf6],
    )
    .to_bytes()
    .expect("serialises");
    let payload = ObjectPayload::from_storage_bytes(&future).expect("header is readable");

    let mut document = Document::create(&path).expect("creates");
    document
        .write(|w| {
            w.put_object(ObjectId::new(), None, 0, Some("Flange"), &payload)?;
            Ok(())
        })
        .expect("writes");
    document.close().expect("closes");

    let mut reopened = Document::open(&path).expect("a read-only document still opens");
    match reopened.access() {
        Access::ReadOnly { reason } => assert!(reason.contains("sheetmetal.v1"), "{reason}"),
        other => panic!("expected read-only access, got {other:?}"),
    }

    let err = reopened
        .write(|w| {
            w.put_object(
                ObjectId::new(),
                None,
                9,
                None,
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            Ok(())
        })
        .expect_err("writing a read-only document must fail");
    assert_eq!(err.kind(), ErrorKind::Unsupported);
}

#[test]
fn an_envelope_capability_cannot_be_hidden_by_a_stale_index() {
    let (_dir, path) = workspace();
    let future = Envelope::new(
        "feature.sheetmetal",
        1,
        vec!["sheetmetal.v1".to_owned()],
        vec![0xf6],
    )
    .to_bytes()
    .expect("serialises");
    let payload = ObjectPayload::from_storage_bytes(&future).expect("header is readable");

    let mut document = Document::create(&path).expect("creates");
    document
        .write(|w| {
            w.put_object(ObjectId::new(), None, 0, None, &payload)?;
            Ok(())
        })
        .expect("writes unknown payload");
    document.close().expect("closes");

    // A damaged/hand-edited index must not trick the reader into write access.
    let conn = rusqlite::Connection::open(&path).expect("opens raw");
    conn.execute("DELETE FROM capabilities WHERE name = 'sheetmetal.v1'", [])
        .expect("removes only the index entry");
    drop(conn);

    let reopened = Document::open(&path).expect("opens for inspection");
    assert!(matches!(reopened.access(), Access::ReadOnly { .. }));
}

#[test]
fn a_stale_capability_index_does_not_keep_a_document_read_only() {
    let (_dir, path) = workspace();
    let id = ObjectId::new();

    let mut document = Document::create(&path).expect("creates");
    document
        .write(|w| {
            w.put_object(
                id,
                None,
                0,
                None,
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            Ok(())
        })
        .expect("writes body");
    document.close().expect("closes");

    // Simulate an old implementation that only appended capability rows and
    // never removed them after a future object disappeared.
    let conn = rusqlite::Connection::open(&path).expect("opens raw");
    conn.execute(
        "INSERT INTO capabilities (name, required) VALUES ('sheetmetal.v1', 1)",
        [],
    )
    .expect("leaves a stale index row");
    drop(conn);

    let mut reopened = Document::open(&path).expect("opens");
    assert!(reopened.access().is_writable());
    reopened
        .write(|w| {
            w.put_object(
                id,
                None,
                0,
                None,
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            Ok(())
        })
        .expect("a successful edit refreshes the index");
    reopened.close().expect("closes");

    let conn = rusqlite::Connection::open(&path).expect("opens raw");
    let stale: i64 = conn
        .query_row(
            "SELECT count(*) FROM capabilities WHERE name = 'sheetmetal.v1'",
            [],
            |row| row.get(0),
        )
        .expect("counts");
    assert_eq!(stale, 0);
}

#[test]
fn deleting_the_cache_sidecar_changes_nothing() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document).expect("populates");
    let expected = document.objects().expect("reads");
    let cache_path = document.cache_path();
    let document_id = document.meta().document_id;
    document.close().expect("closes");

    let mut cache =
        CacheStore::open(&cache_path, document_id, "occt", "8.0.0").expect("sidecar opens");
    cache
        .put(
            plate.extrude,
            ContentHash::of_bytes(b"whatever key"),
            "brep",
            b"pretend this is a solid",
        )
        .expect("stores");
    drop(cache);
    assert!(cache_path.exists());

    CacheStore::discard(&cache_path).expect("discards");
    assert!(!cache_path.exists());

    let cold = Document::open(&path).expect("reopens without a cache");
    assert_eq!(cold.objects().expect("reads"), expected);
    assert!(cold.validate().expect("validates").is_ok());
    assert_eq!(
        cold.evaluation_order().expect("orders").first().copied(),
        Some(plate.plane)
    );
}

#[test]
fn a_failed_edit_leaves_the_document_exactly_as_it_was() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    populate(&mut document).expect("populates");
    let before = document.objects().expect("reads");

    let err = document
        .write(|w| {
            w.put_object(
                ObjectId::new(),
                None,
                7,
                Some("Doomed"),
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            Err::<(), _>(CadError::constraint("the caller changed its mind"))
        })
        .expect_err("the closure failed, so the edit must fail");
    assert_eq!(err.kind(), ErrorKind::Constraint);

    assert_eq!(document.objects().expect("reads"), before);
    document.close().expect("closes");

    let reopened = Document::open(&path).expect("reopens");
    assert_eq!(reopened.objects().expect("reads"), before);
}

#[test]
fn removing_an_object_something_depends_on_is_refused() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document).expect("populates");

    let err = document
        .write(|w| w.remove_object(plate.sketch))
        .expect_err("the extrude still needs the sketch");
    assert_eq!(err.kind(), ErrorKind::Io);

    // The refusal must not have taken anything with it.
    assert!(document.object(plate.sketch).expect("reads").is_some());
    assert!(document.object(plate.extrude).expect("reads").is_some());
}

#[test]
fn a_cycle_in_the_graph_is_reported_rather_than_evaluated() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document).expect("populates");

    document
        .write(|w| {
            w.add_dependency(Dependency {
                dependent: plate.plane,
                dependency: plate.extrude,
                role: DependencyRole::TopologyReference,
            })
        })
        .expect("the edge is storable; it is the graph that is wrong");

    let err = document
        .evaluation_order()
        .expect_err("a cyclic graph has no evaluation order");
    assert!(err.to_string().contains("cycle"));

    let report = document.validate().expect("validates");
    assert!(!report.is_ok());
    assert!(
        report.errors().any(|d| d.code == "graph.not-orderable"),
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn a_reference_without_a_matching_dependency_edge_fails_validation() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document).expect("populates");

    // The payload still says the extrude reads this sketch; only the edge that
    // orders the rebuild is gone. Nothing must quietly paper over that.
    document
        .write(|w| {
            w.remove_dependency(Dependency {
                dependent: plate.extrude,
                dependency: plate.sketch,
                role: DependencyRole::Profile,
            })
        })
        .expect("removes the edge");

    let report = document.validate().expect("validates");
    assert!(!report.is_ok());
    assert!(
        report
            .errors()
            .any(|d| d.code == "reference.missing-edge" && d.object == Some(plate.extrude)),
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn a_corrupted_payload_is_detected_rather_than_decoded() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document).expect("populates");
    document.close().expect("closes");

    // Simulate on-disk damage: change the payload without changing its hash.
    let tampered = Envelope::new(
        "body",
        1,
        vec![CORE_CAPABILITY.to_owned()],
        vec![
            0xa1, 0x6b, b't', b'i', b'p', b'_', b'f', b'e', b'a', b't', b'u', b'r', b'e', 0xf6,
        ],
    )
    .to_bytes()
    .expect("serialises");

    let conn = rusqlite::Connection::open(&path).expect("opens raw");
    conn.execute(
        "UPDATE objects SET payload = ?1 WHERE id = ?2",
        rusqlite::params![tampered, plate.body.to_bytes().as_slice()],
    )
    .expect("tampers");
    drop(conn);

    let reopened = Document::open(&path).expect("reopens");
    let report = reopened.validate().expect("validates");
    assert!(!report.is_ok());
    assert!(
        report
            .errors()
            .any(|d| d.code == "object.payload-hash-mismatch"),
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn a_kind_column_cannot_disagree_with_its_cbor_envelope() {
    let (_dir, path) = workspace();
    let id = ObjectId::new();

    let mut document = Document::create(&path).expect("creates");
    document
        .write(|w| {
            w.put_object(
                id,
                None,
                0,
                None,
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            Ok(())
        })
        .expect("writes");
    document.close().expect("closes");

    let conn = rusqlite::Connection::open(&path).expect("opens raw");
    conn.execute(
        "UPDATE objects SET kind = 'sketch' WHERE id = ?1",
        rusqlite::params![id.to_bytes().as_slice()],
    )
    .expect("tampers");
    drop(conn);

    let reopened = Document::open(&path).expect("container is readable");
    let err = reopened
        .object(id)
        .expect_err("metadata mismatch is unsafe");
    assert_eq!(err.kind(), ErrorKind::Input);
    assert!(err.to_string().contains("disagrees"));
}

#[test]
fn creating_over_an_existing_file_is_refused() {
    let (_dir, path) = workspace();

    Document::create(&path)
        .expect("creates")
        .close()
        .expect("closes");

    let err = Document::create(&path).expect_err("must not overwrite");
    assert_eq!(err.kind(), ErrorKind::Input);
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn a_file_that_is_not_a_ferritecad_document_is_refused() {
    let (dir, _) = workspace();
    let path = dir.path().join("foreign.fcad");

    let conn = rusqlite::Connection::open(&path).expect("creates a plain database");
    conn.execute_batch("CREATE TABLE notes (body TEXT); PRAGMA user_version = 1;")
        .expect("writes");
    drop(conn);

    let err = Document::open(&path).expect_err("a foreign database must be refused");
    assert!(err.to_string().contains("not a FerriteCAD"), "{err}");
}

#[test]
fn display_units_are_remembered_but_values_stay_internal() {
    let (_dir, path) = workspace();

    let mut document =
        Document::create_with(&path, Unit::Inch, Unit::Degree).expect("creates in inches");
    let id = ObjectId::new();
    document
        .write(|w| {
            w.put_object(
                id,
                None,
                0,
                Some("Boss"),
                // One inch, entered as such, stored as millimetres.
                &ObjectPayload::Extrude(Extrude {
                    profile: ObjectId::new(),
                    end_condition: EndCondition::Blind {
                        distance: Expression::new("1in", 25.4)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
                }),
            )?;
            Ok(())
        })
        .expect("writes");
    document.close().expect("closes");

    let reopened = Document::open(&path).expect("reopens");
    assert_eq!(reopened.meta().display_length_unit, Unit::Inch);

    let object = reopened.object(id).expect("reads").expect("present");
    let ObjectPayload::Extrude(extrude) = object.payload else {
        panic!("expected an extrude");
    };
    let EndCondition::Blind { distance } = extrude.end_condition else {
        panic!("expected a blind extrude");
    };
    assert_eq!(distance.value(), 25.4);
    assert_eq!(distance.source, "1in");
}

#[test]
fn mismatched_display_units_are_refused_before_creating_a_file() {
    let (_dir, path) = workspace();
    let err = Document::create_with(&path, Unit::Degree, Unit::Inch)
        .expect_err("angles cannot be a display length unit");
    assert_eq!(err.kind(), ErrorKind::Input);
    assert!(!path.exists());
}

#[test]
fn non_finite_geometry_cannot_cross_the_persistence_boundary() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let err = document
        .write(|w| {
            w.put_object(
                ObjectId::new(),
                None,
                0,
                None,
                &ObjectPayload::Sketch(Sketch {
                    plane: ObjectId::new(),
                    curves: vec![SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Circle {
                            center: Point2::ORIGIN,
                            radius: f64::NAN,
                        },
                    }],
                }),
            )?;
            Ok(())
        })
        .expect_err("NaN is not valid source-of-truth geometry");
    assert_eq!(err.kind(), ErrorKind::Input);
    assert!(document.objects().expect("reads").is_empty());
}

#[test]
fn refusing_a_foreign_database_does_not_change_its_journal_mode() {
    let (dir, _) = workspace();
    let path = dir.path().join("foreign.fcad");
    let conn = rusqlite::Connection::open(&path).expect("creates foreign database");
    let before: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .expect("sets WAL");
    assert_eq!(before, "wal");
    drop(conn);

    assert!(Document::open(&path).is_err());

    let conn = rusqlite::Connection::open(&path).expect("reopens foreign database");
    let after: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("reads journal mode");
    assert_eq!(after, "wal");
}

#[test]
fn modification_time_advances_only_on_a_successful_edit() {
    let (_dir, path) = workspace();

    let mut document = Document::create(&path).expect("creates");
    let created = document.meta().modified_at.clone();

    let _ = document.write(|_| Err::<(), _>(CadError::constraint("no")));
    assert_eq!(document.meta().modified_at, created);

    document
        .write(|w| {
            w.put_object(
                ObjectId::new(),
                None,
                0,
                None,
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            Ok(())
        })
        .expect("writes");
    assert!(document.meta().modified_at >= created);
}

#[test]
fn naming_a_cap_edge_declares_a_capability_and_nothing_else_does() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document).expect("populates");

    // Before: this document names faces only, so it asks a reader for exactly
    // what it asked for before this build existed.
    let stored = document.topology_refs().expect("reads");
    assert!(!stored.is_empty(), "the fixture stores a reference");
    let reopened = Document::open(&path).expect("opens");
    assert!(
        reopened.access().is_writable(),
        "a document of faces must stay writable"
    );
    drop(reopened);

    let cap_edge = StableEntityId::new();
    document
        .write(|w| {
            w.put_topology_ref(&TopologyRef {
                id: cap_edge,
                owner: plate.extrude,
                producer_feature: plate.extrude,
                expected_kind: EntityKind::Edge,
                output_role: SemanticRole::ExtrudeCapEdge {
                    side: CapSide::Start,
                    profile_segment: plate.first_segment,
                },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })
        })
        .expect("stores the cap edge reference");

    // After: it comes back exactly as written, and this build may still write.
    let reopened = Document::open(&path).expect("opens");
    assert!(
        reopened.access().is_writable(),
        "this build implements the capability it declares"
    );
    let refs = reopened.topology_refs().expect("reads");
    let restored = refs
        .iter()
        .find(|stored| stored.id == cap_edge)
        .expect("the cap edge reference is there");
    assert_eq!(
        restored.output_role,
        SemanticRole::ExtrudeCapEdge {
            side: CapSide::Start,
            profile_segment: plate.first_segment,
        }
    );
    assert_eq!(restored.expected_kind, EntityKind::Edge);
}

#[test]
fn a_cap_edge_role_cannot_hide_the_capability_it_needs() {
    let (_dir, path) = workspace();
    let mut document = Document::create(&path).expect("creates");
    let plate = populate(&mut document).expect("populates");
    let cap_edge = StableEntityId::new();
    document
        .write(|w| {
            w.put_topology_ref(&TopologyRef {
                id: cap_edge,
                owner: plate.extrude,
                producer_feature: plate.extrude,
                expected_kind: EntityKind::Edge,
                output_role: SemanticRole::ExtrudeCapEdge {
                    side: CapSide::Start,
                    profile_segment: plate.first_segment,
                },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })
        })
        .expect("stores the cap edge reference");
    document.close().expect("closes");

    // Simulate damage that is internally consistent at the byte/hash level:
    // the role is still a cap edge, but the envelope lies by declaring only
    // the older core capability. Looking only at the declaration would grant
    // write access and defeat the purpose of capability negotiation.
    let conn = rusqlite::Connection::open(&path).expect("opens raw");
    let stored: Vec<u8> = conn
        .query_row(
            "SELECT payload FROM topology_refs WHERE id = ?1",
            [cap_edge.to_bytes().as_slice()],
            |row| row.get(0),
        )
        .expect("reads topology reference envelope");
    let mut envelope = Envelope::from_bytes(&stored).expect("decodes envelope");
    assert!(
        envelope
            .required_capabilities
            .iter()
            .any(|name| name == EXTRUDE_CAP_EDGE_CAPABILITY),
        "the writer must establish the precondition"
    );
    envelope.required_capabilities = vec![CORE_CAPABILITY.to_owned()];
    let damaged = envelope.to_bytes().expect("re-encodes damaged envelope");
    let damaged_hash = ContentHash::of_bytes(&damaged);
    conn.execute(
        "UPDATE topology_refs SET payload = ?1, payload_hash = ?2 WHERE id = ?3",
        rusqlite::params![
            damaged,
            damaged_hash.as_bytes().as_slice(),
            cap_edge.to_bytes().as_slice()
        ],
    )
    .expect("updates bytes and their integrity hash");
    drop(conn);

    let refusal = match Document::open(&path) {
        Ok(_) => panic!("an under-declared cap-edge role gained write access"),
        Err(error) => error,
    };
    assert_eq!(refusal.kind(), ErrorKind::Input);
    assert!(
        refusal.to_string().contains(EXTRUDE_CAP_EDGE_CAPABILITY),
        "the refusal should name the missing contract, got: {refusal}"
    );
}
