// SPDX-License-Identifier: MIT
//! SQLite snapshots preserve complete storage without migrating the source.
use ferritecad_document::{Access, Document, Envelope, ObjectPayload};
use ferritecad_types::ObjectId;
use rusqlite::Connection;

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
