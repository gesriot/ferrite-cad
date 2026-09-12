// SPDX-License-Identifier: MIT
//! SQLite snapshots preserve complete storage without migrating the source.
use ferritecad_document::{Access, Document, Envelope, ObjectPayload};
use ferritecad_types::ObjectId;
use rusqlite::Connection;

#[test]
fn content_version_observes_implicit_row_identity_even_when_an_alias_is_shadowed() {
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    Document::create(&source)
        .expect("create")
        .close()
        .expect("close");
    let connection = Connection::open(&source).expect("SQL");
    connection
        .execute_batch(
            "CREATE TABLE extension(rowid TEXT, value BLOB); \
         INSERT INTO extension(_rowid_, rowid, value) VALUES (1, 'user column', X'0102');",
        )
        .expect("extension with a shadowed rowid alias");
    let version = Document::open_read_only(&source)
        .expect("reading")
        .content_version()
        .expect("complete version");
    connection
        .execute("UPDATE extension SET _rowid_ = 42", [])
        .expect("change identity");
    let changed = Document::open_read_only(&source)
        .expect("new reading")
        .content_version()
        .expect("new version");
    assert_ne!(
        version, changed,
        "a changed implicit row identity was invisible to the version"
    );
}

#[test]
fn content_version_refuses_an_inaccessible_implicit_identity() {
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    Document::create(&source)
        .expect("create")
        .close()
        .expect("close");
    Connection::open(&source)
        .expect("SQL")
        .execute_batch("CREATE TABLE extension(rowid TEXT, _ROWID_ TEXT, oid TEXT);")
        .expect("all aliases shadowed");
    let error = Document::open_read_only(&source)
        .expect("reading")
        .content_version()
        .expect_err("must not issue an incomplete version")
        .to_string();
    assert!(
        error.contains("extension") && error.contains("all rowid aliases"),
        "{error}"
    );
}

#[test]
fn stored_triggers_do_not_make_an_editable_copy_with_unaccounted_write_effects() {
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    Document::create(&source)
        .expect("create")
        .close()
        .expect("close");
    Connection::open(&source)
        .expect("SQL")
        .execute_batch(
            "CREATE TRIGGER extension_write AFTER UPDATE ON objects BEGIN \
         UPDATE objects SET name = 'unexpected rename'; END;",
        )
        .expect("stored trigger");
    let document = Document::open_read_only(&source).expect("still readable");
    let access = document.copy_access().expect("copy access");
    assert!(
        matches!(&access, Access::ReadOnly { reason } if reason.contains("extension_write")),
        "the refusal must identify the stored trigger: {access:?}"
    );
}

#[test]
fn extension_foreign_key_actions_are_not_accepted_as_ordinary_edits() {
    for definition in [
        "CREATE UNIQUE INDEX extension_key ON objects(payload_hash); \
         CREATE TABLE extension(value BLOB REFERENCES objects(payload_hash) ON UPDATE CASCADE);",
        "CREATE TABLE extension(value TEXT REFERENCES capabilities(name) ON DELETE CASCADE);",
    ] {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("source.fcad");
        Document::create(&source)
            .expect("create")
            .close()
            .expect("close");
        Connection::open(&source)
            .expect("SQL")
            .execute_batch(definition)
            .expect("extension");
        let access = Document::open_read_only(&source)
            .expect("reading")
            .copy_access()
            .expect("access");
        assert!(
            matches!(&access, Access::ReadOnly { reason } if reason.contains("extension") && reason.contains("foreign-key action")),
            "{access:?}"
        );
    }
}

#[test]
fn backup_keeps_unknown_envelopes_tables_and_the_pinned_reading() {
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    let output = root.path().join("snapshot.fcad");
    let mut doc = Document::create(&source).expect("create");
    let bytes = Envelope::new("future.note", 7, vec![], vec![0x81, 0x01])
        .to_bytes()
        .expect("envelope");
    let payload = ObjectPayload::from_storage_bytes(&bytes).expect("unknown preserved");
    let id = ObjectId::new();
    doc.write(|w| w.put_object(id, None, 21, Some("Unrecognised note"), &payload))
        .expect("unknown object");
    doc.close().expect("close");
    let conn = Connection::open(&source).expect("raw");
    conn.execute_batch("CREATE TABLE extension (key TEXT PRIMARY KEY, bytes BLOB) WITHOUT ROWID; INSERT INTO extension VALUES ('one', X'010203FF');").expect("unknown table");
    conn.execute_batch("CREATE TABLE extension_ref (key TEXT REFERENCES extension(key) ON DELETE RESTRICT); INSERT INTO extension_ref VALUES ('one');")
        .expect("an extension constraint without side-effecting actions stays supported");
    conn.execute_batch("CREATE TABLE extension_collation (key TEXT COLLATE NOCASE, value BLOB, PRIMARY KEY (key COLLATE BINARY)) WITHOUT ROWID; INSERT INTO extension_collation VALUES ('a', X'01'), ('A', X'01');")
        .expect("a primary key can distinguish values its column collation treats as equal");
    let before = std::fs::read(&source).expect("source bytes");
    let pinned = Document::open_read_only(&source).expect("pinned reading");
    let version = pinned.content_version().expect("version");
    // An uncommitted writer exists. Backup must read committed SQLite state,
    // and copying the live file's bytes is not an acceptable substitute.
    conn.execute_batch("BEGIN IMMEDIATE; UPDATE extension SET bytes = X'FF';")
        .expect("writer in progress");
    pinned.snapshot_to(&output).expect("SQLite snapshot");
    let copy = Document::open_read_only(&output).expect("snapshot");
    assert_eq!(copy.content_version().expect("copied version"), version);
    assert_eq!(
        copy.objects().expect("copied objects"),
        pinned.objects().expect("objects")
    );
    assert_eq!(copy.meta().document_id, pinned.meta().document_id);
    assert_eq!(copy.copy_access().expect("copy access"), Access::ReadWrite);
    assert_eq!(pinned.content_version().expect("still pinned"), version);
    pinned.close().expect("close reading");
    conn.execute_batch("COMMIT")
        .expect("commit writer after snapshot");
    drop(conn);
    assert_ne!(
        Document::open_read_only(&source)
            .expect("new reading")
            .content_version()
            .expect("new version"),
        version
    );
    assert_ne!(
        std::fs::read(&source).expect("writer modified source"),
        before
    );
}

#[test]
fn snapshot_never_promotes_a_future_capability_to_writable() {
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    let output = root.path().join("copy.fcad");
    let mut doc = Document::create(&source).expect("create");
    let bytes = Envelope::new(
        "future.feature",
        1,
        vec!["future.required.v1".into()],
        vec![0x01],
    )
    .to_bytes()
    .expect("bytes");
    let payload = ObjectPayload::from_storage_bytes(&bytes).expect("unknown");
    doc.write(|w| w.put_object(ObjectId::new(), None, 0, None, &payload))
        .expect("store");
    doc.close().expect("close");
    let before = std::fs::read(&source).expect("bytes");
    let pinned = Document::open_read_only(&source).expect("read only");
    assert!(!pinned.copy_access().expect("access").is_writable());
    pinned.snapshot_to(&output).expect("snapshot");
    pinned.close().expect("close");
    let mut copy = Document::open(&output).expect("open copy");
    assert!(!copy.access().is_writable());
    assert!(copy.write(|_| Ok(())).is_err());
    assert_eq!(before, std::fs::read(&source).expect("source unchanged"));
}

#[test]
fn snapshot_leaves_source_bytes_and_foreign_sidecars_unchanged() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("source.fcad");
    Document::create(&path)
        .expect("create")
        .close()
        .expect("close");
    let before = std::fs::read(&path).expect("before");
    let sidecar = path.with_extension("fcad-cache");
    std::fs::write(&sidecar, b"foreign cache sentinel").expect("sidecar");
    let document = Document::open_read_only(&path).expect("read only");
    document
        .snapshot_to(&root.path().join("snapshot.fcad"))
        .expect("snapshot");
    document.close().expect("close");
    assert_eq!(std::fs::read(&path).expect("source"), before);
    assert_eq!(
        std::fs::read(sidecar).expect("sidecar"),
        b"foreign cache sentinel"
    );
}

#[test]
fn validation_metadata_and_findings_hold_the_same_committed_snapshot() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("validation.fcad");
    let mut document = Document::create(&path).expect("create");
    let id = ObjectId::new();
    let bytes = Envelope::new("future.note", 1, vec![], vec![1])
        .to_bytes()
        .expect("envelope");
    let payload = ObjectPayload::from_storage_bytes(&bytes).expect("unknown");
    document
        .write(|w| w.put_object(id, None, 0, None, &payload))
        .expect("object");
    document.close().expect("close fixture");
    let reading = Document::open_read_only(&path).expect("pin metadata");
    let old_id = reading.meta().document_id;
    let old_report = reading.validate().expect("one warning");
    assert_eq!(old_report.warnings().count(), 1);
    let new_id = ferritecad_types::DocumentId::new();
    let writer = Connection::open(&path).expect("writer");
    writer
        .busy_timeout(std::time::Duration::ZERO)
        .expect("no timing race or sleep");
    writer
        .execute_batch("BEGIN IMMEDIATE; DELETE FROM objects;")
        .expect("change findings");
    writer
        .execute(
            "UPDATE meta SET document_id = ?1",
            [new_id.to_bytes().as_slice()],
        )
        .expect("change metadata in same transaction");
    // A deferred transaction without a pinned read would allow this commit,
    // yielding stale cached metadata paired with fresh validation findings.
    let error = writer
        .execute_batch("COMMIT")
        .expect_err("snapshot holds a read lock");
    assert!(
        matches!(error, rusqlite::Error::SqliteFailure(ref e, _) if e.code == rusqlite::ErrorCode::DatabaseBusy)
    );
    assert_eq!(reading.meta().document_id, old_id);
    assert_eq!(reading.validate().expect("same report"), old_report);
    reading.close().expect("release reading");
    writer.execute_batch("COMMIT").expect("commit after close");
    drop(writer);
    let later = Document::open_read_only(&path).expect("new reading");
    assert_eq!(later.meta().document_id, new_id);
    assert!(
        later
            .validate()
            .expect("new findings")
            .diagnostics
            .is_empty()
    );
    later.close().expect("close");
}

#[test]
fn rollback_header_does_not_allow_writes_to_foreign_wal_sidecars() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("document.fcad");
    Document::create(&path)
        .expect("create")
        .close()
        .expect("close");
    let before = std::fs::read(&path).expect("bytes");
    assert_eq!(&before[18..20], &[1, 1], "DELETE header");
    for suffix in ["-wal", "-shm"] {
        let sidecar = root.path().join(format!("document.fcad{suffix}"));
        std::fs::write(&sidecar, b"foreign sidecar").expect("sentinel");
        let mtime = sidecar
            .metadata()
            .expect("metadata")
            .modified()
            .expect("mtime");
        let error = Document::open_read_only(&path).expect_err("SQLite must not touch SHM");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);
        assert!(error.to_string().contains("WAL sidecar"), "{error}");
        #[cfg(unix)]
        {
            // The alias has no adjacent sidecars of its own.
            let alias = root.path().join("alias.fcad");
            std::os::unix::fs::symlink(&path, &alias).expect("source alias");
            let error = Document::open_read_only(&alias).expect_err("resolved source sidecar");
            assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);
            assert!(error.to_string().contains("WAL sidecar"));
            std::fs::remove_file(alias).expect("own symlink");
        }
        assert_eq!(std::fs::read(&path).expect("source"), before);
        assert_eq!(
            std::fs::read(&sidecar).expect("sidecar"),
            b"foreign sidecar"
        );
        assert_eq!(
            sidecar
                .metadata()
                .expect("metadata")
                .modified()
                .expect("mtime"),
            mtime
        );
        std::fs::remove_file(sidecar).expect("own sentinel");
    }
}
