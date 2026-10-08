// SPDX-License-Identifier: MIT
//! §30N storage, without a kernel: the catalog on the committed schema v3 plate,
//! what an image keeps, the limits, damage, and that reading never writes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ferritecad_document::{
    CheckpointEntry, Document, MAX_CHECKPOINT_BYTES, MAX_CHECKPOINT_NAME_CHARS, MAX_CHECKPOINTS,
};
use ferritecad_types::CheckpointId;

fn plate_v3() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../ferritecad-fixtures/plate/plate.fcad")
}

fn user_version(path: &Path) -> i64 {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("opens");
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("reads")
}

/// Every row of every table, rowid first, as text; `skip` names tables left out.
fn cells(path: &Path, skip: &[&str]) -> BTreeMap<String, Vec<String>> {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("opens");
    let tables: Vec<(String, bool)> = conn
        .prepare("SELECT name, wr FROM pragma_table_list WHERE schema = 'main' AND type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .expect("lists")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("lists")
        .collect::<rusqlite::Result<_>>()
        .expect("lists");
    let mut all = BTreeMap::new();
    for (table, without_rowid) in tables {
        if skip.contains(&table.as_str()) {
            continue;
        }
        let select = if without_rowid {
            format!("SELECT * FROM \"{table}\"")
        } else {
            format!("SELECT rowid, * FROM \"{table}\"")
        };
        let mut statement = conn.prepare(&select).expect("selects");
        let count = statement.column_count();
        let mut rows: Vec<String> = statement
            .query_map([], |row| {
                Ok((0..count)
                    .map(|i| format!("{:?}", row.get_ref(i).expect("cell")))
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .expect("rows")
            .collect::<rusqlite::Result<_>>()
            .expect("rows");
        rows.sort();
        all.insert(table, rows);
    }
    let schema: Vec<String> = conn
        .prepare("SELECT type || ' ' || name || ' ' || coalesce(sql, '') FROM sqlite_schema ORDER BY type, name")
        .expect("schema")
        .query_map([], |row| row.get(0))
        .expect("schema")
        .collect::<rusqlite::Result<_>>()
        .expect("schema");
    all.insert("<schema>".to_owned(), schema);
    all
}

/// The committed v3 plate migrated in a private copy, with data in a table this
/// build has never heard of.
fn working(dir: &Path) -> (PathBuf, Document) {
    let path = dir.join("working.fcad");
    std::fs::copy(plate_v3(), &path).expect("copies the fixture");
    let document = Document::open(&path).expect("a writer migrates its private copy");
    let conn = rusqlite::Connection::open(&path).expect("opens");
    conn.execute_batch(
        "CREATE TABLE future_notes (note TEXT NOT NULL, data BLOB);
         INSERT INTO future_notes VALUES ('kept by a later build', x'00ff10');",
    )
    .expect("an unknown table");
    drop(conn);
    (path, document)
}

fn checkpoint(document: &mut Document, dir: &Path, name: &str) -> CheckpointEntry {
    let tag = CheckpointId::new();
    let image = dir.join(format!("image-{tag}.fcad"));
    let model = document
        .write_checkpoint_image(&dir.join(format!("scratch-{tag}.fcad")), &image)
        .expect("an image of this version");
    let entry = document.add_checkpoint(name, &image).expect("adds");
    assert_eq!(entry.model, model);
    std::fs::remove_file(image).expect("the caller owns the image");
    entry
}

fn rename_first_object(document: &mut Document, name: &str) {
    let object = document.objects().expect("objects").remove(0);
    document
        .write(|w| {
            w.put_object(
                object.id,
                object.parent,
                object.ordinal,
                Some(name),
                &object.payload,
            )
        })
        .expect("a model change");
}

#[test]
fn a_schema_v3_document_is_listed_and_read_without_being_written() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("old.fcad");
    std::fs::copy(plate_v3(), &path).expect("copies");
    assert_eq!(user_version(&path), 3, "the committed plate is schema v3");
    let before = std::fs::read(&path).expect("bytes");
    let modified = std::fs::metadata(&path)
        .expect("meta")
        .modified()
        .expect("time");

    let document = Document::open_read_only(&path).expect("v3 reads without migration");
    assert!(document.checkpoints().expect("lists").is_empty());
    assert_ne!(
        document.model_without_checkpoints().expect("hash"),
        document.model_version().expect("hash"),
        "two hashes, two domains"
    );
    let missing = document
        .extract_checkpoint_image(CheckpointId::new(), &dir.path().join("x.fcad"))
        .expect_err("nothing to extract");
    assert!(missing.to_string().contains("no checkpoint"), "{missing}");
    document.close().expect("closes");

    assert_eq!(
        std::fs::read(&path).expect("bytes"),
        before,
        "not one byte written"
    );
    assert_eq!(
        std::fs::metadata(&path)
            .expect("meta")
            .modified()
            .expect("time"),
        modified
    );
    assert_eq!(user_version(&path), 3);
    assert!(!dir.path().join("x.fcad").exists());
}

#[test]
fn in_a_v3_file_a_table_called_checkpoints_is_somebody_elses() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("old.fcad");
    std::fs::copy(plate_v3(), &path).expect("copies");
    let conn = rusqlite::Connection::open(&path).expect("opens");
    conn.execute_batch(
        "CREATE TABLE checkpoints (note TEXT); INSERT INTO checkpoints VALUES ('x');",
    )
    .expect("an extension's table");
    drop(conn);
    assert_eq!(user_version(&path), 3);
    let read = |path: &Path| {
        let document = Document::open_read_only(path).expect("reads");
        let facts = (
            document.checkpoints().expect("not a catalog").len(),
            document.model_without_checkpoints().expect("hash"),
        );
        document.close().expect("closes");
        facts
    };
    let (listed, before) = read(&path);
    assert_eq!(listed, 0, "not read as checkpoints");
    let conn = rusqlite::Connection::open(&path).expect("opens");
    conn.execute("UPDATE checkpoints SET note = 'y'", [])
        .expect("changes");
    drop(conn);
    assert_ne!(
        read(&path).1,
        before,
        "its rows are model content like any unknown table"
    );
    let bytes = std::fs::read(&path).expect("bytes");
    Document::open(&path).expect_err("v4 cannot be added over it; refused, not merged");
    assert_eq!(
        std::fs::read(&path).expect("bytes"),
        bytes,
        "and nothing was written"
    );
}

#[test]
fn an_image_is_the_whole_model_of_its_moment_and_the_catalog_is_never_nested() {
    let dir = tempfile::tempdir().expect("dir");
    let (path, mut document) = working(dir.path());
    let at_a = dir.path().join("at-a.fcad");
    document.snapshot_to(&at_a).expect("what A should hold");
    let model_a = document.model_without_checkpoints().expect("hash");
    let a = checkpoint(&mut document, dir.path(), "  Before the change ");
    assert_eq!(a.name, "Before the change");
    assert_eq!(a.model, model_a);
    assert_eq!(
        document.model_without_checkpoints().expect("hash"),
        model_a,
        "adding a checkpoint does not change the model"
    );

    rename_first_object(&mut document, "changed after A");
    let b = checkpoint(&mut document, dir.path(), "Before the change");
    assert_ne!(a.id, b.id, "equal names, two identities");
    assert_ne!(a.model, b.model);
    let listed = document.checkpoints().expect("lists");
    assert_eq!(listed, vec![a.clone(), b.clone()], "oldest first");
    let sizes = (a.bytes, b.bytes);

    // Extracting A gives A's model in every cell and row identity; its own
    // catalog is empty, so B's image holds no copy of A.
    let out = dir.path().join("a.fcad");
    assert_eq!(
        document
            .extract_checkpoint_image(a.id, &out)
            .expect("extracts"),
        a
    );
    assert_eq!(
        cells(&out, &["checkpoints"]),
        cells(&at_a, &["checkpoints"]),
        "every SQL cell, row identities and the unknown table included"
    );
    let extracted = Document::open_read_only(&out).expect("a document");
    assert!(extracted.checkpoints().expect("lists").is_empty());
    assert_eq!(
        extracted.model_without_checkpoints().expect("hash"),
        a.model
    );
    extracted.close().expect("closes");
    let out_b = dir.path().join("b.fcad");
    document
        .extract_checkpoint_image(b.id, &out_b)
        .expect("extracts");
    assert!(
        std::fs::metadata(&out_b).expect("meta").len() < 2 * sizes.0,
        "B is one model, not A plus B"
    );

    // A later edit rewrites no checkpoint.
    rename_first_object(&mut document, "changed after B");
    assert_eq!(document.checkpoints().expect("lists"), listed);

    // Restore brings back A's model and keeps the catalog it is restored into.
    let restored = dir.path().join("restored.fcad");
    document
        .extract_checkpoint_image(a.id, &restored)
        .expect("extracts");
    let mut restored_doc = Document::open(&restored).expect("writable private copy");
    restored_doc
        .replace_checkpoints_from(&document)
        .expect("the catalog follows");
    assert_eq!(restored_doc.checkpoints().expect("lists"), listed);
    assert_eq!(
        restored_doc.model_without_checkpoints().expect("hash"),
        a.model
    );
    restored_doc.close().expect("closes");
    assert_eq!(
        cells(&restored, &["meta"])["checkpoints"],
        cells(&path, &["meta"])["checkpoints"],
        "the same rows, row identities and images"
    );

    // Removing one leaves the other and the model alone.
    let model = document.model_without_checkpoints().expect("hash");
    assert_eq!(document.remove_checkpoint(a.id).expect("removes"), a);
    assert_eq!(document.checkpoints().expect("lists"), vec![b]);
    assert_eq!(document.model_without_checkpoints().expect("hash"), model);
    assert!(document.remove_checkpoint(a.id).is_err(), "already gone");
    document.close().expect("closes");
}

#[test]
fn the_limits_refuse_an_addition_and_never_remove_anything() {
    assert_eq!(
        (MAX_CHECKPOINTS, MAX_CHECKPOINT_BYTES),
        (32, 16 * 1024 * 1024)
    );
    assert_eq!(MAX_CHECKPOINT_NAME_CHARS, 80);
    let dir = tempfile::tempdir().expect("dir");
    let (_, mut document) = working(dir.path());
    let image = dir.path().join("image.fcad");
    document
        .write_checkpoint_image(&dir.path().join("scratch.fcad"), &image)
        .expect("image");
    for index in 0..MAX_CHECKPOINTS {
        document
            .add_checkpoint(&format!("n{index}"), &image)
            .expect("within the count");
    }
    let before = document.checkpoints().expect("lists");
    let refused = document
        .add_checkpoint("one too many", &image)
        .expect_err("the count is the limit");
    assert!(
        refused.to_string().contains("already has 32 checkpoints"),
        "{refused}"
    );
    assert_eq!(
        document.checkpoints().expect("lists"),
        before,
        "nothing removed"
    );
    document.close().expect("closes");

    // Bytes: a model of a little over 6 MiB fits twice, not three times.
    let (path, mut document) = working(&dir.path().join("big").tap_mkdir());
    let conn = rusqlite::Connection::open(&path).expect("opens");
    conn.execute(
        "INSERT INTO future_notes VALUES ('large', zeroblob(6 * 1024 * 1024 + 4096))",
        [],
    )
    .expect("a large row");
    drop(conn);
    let large = dir.path().join("large.fcad");
    document
        .write_checkpoint_image(&dir.path().join("scratch-large.fcad"), &large)
        .expect("image");
    let size = std::fs::metadata(&large).expect("meta").len();
    assert!(2 * size <= MAX_CHECKPOINT_BYTES && 3 * size > MAX_CHECKPOINT_BYTES);
    document.add_checkpoint("one", &large).expect("fits");
    document.add_checkpoint("two", &large).expect("fits");
    let refused = document
        .add_checkpoint("three", &large)
        .expect_err("the bytes are the limit");
    assert!(
        refused
            .to_string()
            .contains("delete one before adding another"),
        "{refused}"
    );
    assert_eq!(document.checkpoints().expect("lists").len(), 2);
    document.close().expect("closes");
}

#[test]
fn a_deleted_checkpoint_leaves_none_of_its_bytes_in_the_file() {
    let dir = tempfile::tempdir().expect("dir");
    let (path, mut document) = working(dir.path());
    let marker = format!("MARKER-30N-{}", CheckpointId::new());
    let count = |path: &Path| {
        let bytes = std::fs::read(path).expect("bytes");
        bytes
            .windows(marker.len())
            .filter(|window| *window == marker.as_bytes())
            .count()
    };
    let conn = rusqlite::Connection::open(&path).expect("opens");
    conn.execute("INSERT INTO future_notes VALUES (?1, NULL)", [&marker])
        .expect("marker");
    drop(conn);
    let a = checkpoint(&mut document, dir.path(), "with the marker");
    // Take the marker out of the model, zeroing its page, so only A holds it.
    let conn = rusqlite::Connection::open(&path).expect("opens");
    conn.execute_batch("PRAGMA secure_delete = ON; DELETE FROM future_notes WHERE data IS NULL;")
        .expect("model without the marker");
    drop(conn);
    assert!(count(&path) >= 1, "the checkpoint image holds the marker");
    document.remove_checkpoint(a.id).expect("removes");
    document.close().expect("closes");
    assert_eq!(count(&path), 0, "nothing of the deleted image is left");
}

trait Mkdir {
    fn tap_mkdir(self) -> PathBuf;
}
impl Mkdir for PathBuf {
    fn tap_mkdir(self) -> PathBuf {
        std::fs::create_dir_all(&self).expect("dir");
        self
    }
}

#[test]
fn damage_is_refused_by_name_and_foreign_images_are_not_stored() {
    let dir = tempfile::tempdir().expect("dir");
    let (path, mut document) = working(dir.path());
    let a = checkpoint(&mut document, dir.path(), "A");
    let b = checkpoint(&mut document, dir.path(), "B");
    document.close().expect("closes");

    // One flipped image byte: that checkpoint is refused, the other still works,
    // and the refused copy is not left behind.
    let conn = rusqlite::Connection::open(&path).expect("opens");
    let rowid: i64 = conn
        .query_row(
            "SELECT rowid FROM checkpoints WHERE id = ?1",
            [a.id.to_bytes().as_slice()],
            |row| row.get(0),
        )
        .expect("row");
    {
        let mut blob = conn
            .blob_open(rusqlite::MAIN_DB, "checkpoints", "image", rowid, false)
            .expect("blob");
        use std::io::{Seek, Write};
        blob.seek(std::io::SeekFrom::Start(200)).expect("seek");
        blob.write_all(&[0xA5]).expect("flip");
    }
    drop(conn);
    let document = Document::open_read_only(&path).expect("still opens");
    let out = dir.path().join("a.fcad");
    let refused = document
        .extract_checkpoint_image(a.id, &out)
        .expect_err("damaged");
    assert!(refused.to_string().contains("damaged"), "{refused}");
    assert!(!out.exists(), "nothing left behind");
    document
        .extract_checkpoint_image(b.id, &dir.path().join("b.fcad"))
        .expect("the other one is intact");
    let mut restored = Document::open(dir.path().join("b.fcad")).expect("restore B");
    restored
        .replace_checkpoints_from(&document)
        .expect("damaged A does not block restoring B");
    restored.close().expect("close");
    assert_eq!(
        cells(&path, &[])["checkpoints"],
        cells(&dir.path().join("b.fcad"), &[])["checkpoints"]
    );
    document.close().expect("closes");

    // A name this build would never have written damages the list as a whole.
    let conn = rusqlite::Connection::open(&path).expect("opens");
    conn.execute(
        "UPDATE checkpoints SET name = 'tab\there' WHERE id = ?1",
        [b.id.to_bytes().as_slice()],
    )
    .expect("hand edit");
    drop(conn);
    let document = Document::open_read_only(&path).expect("the model still opens");
    let refused = document.checkpoints().expect_err("a damaged list");
    assert!(
        refused.to_string().contains("checkpoint list is damaged"),
        "{refused}"
    );
    document.close().expect("closes");

    // Another document's model is never stored as a checkpoint.
    let other_dir = dir.path().join("other").tap_mkdir();
    let other = other_dir.join("other.fcad");
    Document::create(&other)
        .expect("another document")
        .close()
        .expect("closes");
    let (_, mut mine) = working(&dir.path().join("mine").tap_mkdir());
    let refused = mine
        .add_checkpoint("foreign", &other)
        .expect_err("not ours");
    assert!(
        refused.to_string().contains("this document's model"),
        "{refused}"
    );
    assert!(mine.checkpoints().expect("lists").is_empty());
    mine.close().expect("closes");
}

#[test]
fn refused_checkpoint_outputs_never_remove_existing_files() {
    let dir = tempfile::tempdir().expect("dir");
    let (path, mut document) = working(dir.path());
    let a = checkpoint(&mut document, dir.path(), "A");
    let before = std::fs::read(&path).expect("source bytes");
    let occupied = dir.path().join("occupied");
    std::fs::write(&occupied, b"keep this file").expect("sentinel");
    for output in [&occupied, &path] {
        let bytes = std::fs::read(output).expect("before");
        assert!(document.extract_checkpoint_image(a.id, output).is_err());
        assert_eq!(
            std::fs::read(output).expect("existing output survives"),
            bytes
        );
    }
    let scratch = dir.path().join("scratch");
    let image = dir.path().join("image");
    assert!(document.write_checkpoint_image(&occupied, &image).is_err());
    assert_eq!(
        std::fs::read(&occupied).expect("scratch occupant survives"),
        b"keep this file"
    );
    assert!(!image.exists());
    assert!(
        document
            .write_checkpoint_image(&scratch, &occupied)
            .is_err()
    );
    assert_eq!(
        std::fs::read(&occupied).expect("image occupant survives"),
        b"keep this file"
    );
    assert!(!scratch.exists());
    assert_eq!(std::fs::read(path).expect("source survives"), before);
}

#[test]
fn adding_an_image_of_an_older_model_is_refused_without_a_write() {
    let dir = tempfile::tempdir().expect("dir");
    let (path, mut document) = working(dir.path());
    let image = dir.path().join("image");
    document
        .write_checkpoint_image(&dir.path().join("scratch"), &image)
        .expect("image");
    rename_first_object(&mut document, "changed after preparing the image");
    let before = std::fs::read(&path).expect("before");
    let error = document
        .add_checkpoint("stale", &image)
        .expect_err("not the current model");
    assert!(error.to_string().contains("model"), "{error}");
    assert_eq!(std::fs::read(&path).expect("after"), before);
    assert!(document.checkpoints().expect("list").is_empty());
}

#[test]
fn an_oversized_catalog_is_refused_before_its_rows_are_collected() {
    let dir = tempfile::tempdir().expect("dir");
    let (path, mut document) = working(dir.path());
    let a = checkpoint(&mut document, dir.path(), "A");
    document.close().expect("close");
    let conn = rusqlite::Connection::open(&path).expect("open");
    for _ in 0..MAX_CHECKPOINTS {
        conn.execute("INSERT INTO checkpoints SELECT ?1, name, created_at, model, byte_len, image_hash, image FROM checkpoints WHERE id = ?2",
            rusqlite::params![CheckpointId::new().to_bytes().as_slice(), a.id.to_bytes().as_slice()]).expect("foreign extra row");
    }
    drop(conn);
    let document = Document::open_read_only(&path).expect("model opens");
    let error = document.checkpoints().expect_err("oversized catalog");
    assert!(error.to_string().contains("exceeds"), "{error}");
}
