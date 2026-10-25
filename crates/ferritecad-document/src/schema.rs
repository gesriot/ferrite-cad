// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, Result};
use rusqlite::Connection;

/// File extension of the native document.
pub const DOCUMENT_EXTENSION: &str = "fcad";

/// Extension of the regenerable cache sidecar.
pub const CACHE_EXTENSION: &str = "fcad-cache";

/// `PRAGMA application_id` for a document: the ASCII bytes `FCAD`.
///
/// Set so `file(1)` and recovery tools can tell a FerriteCAD document from any
/// other SQLite database.
pub const DOCUMENT_APPLICATION_ID: i32 = i32::from_be_bytes(*b"FCAD");

/// `PRAGMA application_id` for a cache sidecar: the ASCII bytes `FCAC`.
pub const CACHE_APPLICATION_ID: i32 = i32::from_be_bytes(*b"FCAC");

/// Current document schema. Identifiers and hashes have fixed lengths.
const DOCUMENT_SCHEMA: &str = r#"
CREATE TABLE meta (
    id                     INTEGER PRIMARY KEY CHECK (id = 1),
    document_id            BLOB    NOT NULL CHECK (length(document_id) = 16),
    display_length_unit    TEXT    NOT NULL,
    display_angle_unit     TEXT    NOT NULL,
    created_at             TEXT    NOT NULL,
    modified_at            TEXT    NOT NULL,
    generator              TEXT    NOT NULL
) STRICT;

CREATE TABLE objects (
    id             BLOB    PRIMARY KEY NOT NULL CHECK (length(id) = 16),
    kind           TEXT    NOT NULL,
    parent_id      BLOB    REFERENCES objects(id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL DEFAULT 0,
    name           TEXT,
    payload        BLOB    NOT NULL,
    payload_hash   BLOB    NOT NULL CHECK (length(payload_hash) = 32)
) STRICT;

CREATE INDEX objects_by_parent ON objects(parent_id, ordinal);
CREATE INDEX objects_by_kind ON objects(kind);

CREATE TABLE deps (
    dependent_id  BLOB NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    dependency_id BLOB NOT NULL REFERENCES objects(id) ON DELETE RESTRICT,
    role          TEXT NOT NULL,
    PRIMARY KEY (dependent_id, dependency_id, role)
) STRICT, WITHOUT ROWID;

CREATE INDEX deps_by_dependency ON deps(dependency_id);

CREATE TABLE imported_sources (
    id           BLOB    PRIMARY KEY NOT NULL CHECK (length(id) = 16),
    format       TEXT    NOT NULL,
    bytes        BLOB    NOT NULL,
    content_hash BLOB    NOT NULL CHECK (length(content_hash) = 32),
    byte_len     INTEGER NOT NULL CHECK (byte_len >= 0),
    created_at   TEXT    NOT NULL,
    CHECK (length(bytes) = byte_len)
) STRICT;

CREATE UNIQUE INDEX imported_sources_by_content ON imported_sources(format, content_hash);

CREATE TABLE imported_source_refs (
    object_id BLOB NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    source_id BLOB NOT NULL REFERENCES imported_sources(id) ON DELETE RESTRICT,
    PRIMARY KEY (object_id, source_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX imported_source_refs_by_source ON imported_source_refs(source_id);


CREATE TABLE checkpoints (
    id         BLOB    PRIMARY KEY NOT NULL CHECK (length(id) = 16),
    name       TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    created_at TEXT    NOT NULL,
    model      BLOB    NOT NULL CHECK (length(model) = 32),
    byte_len   INTEGER NOT NULL CHECK (byte_len > 0),
    image_hash BLOB    NOT NULL CHECK (length(image_hash) = 32),
    image      BLOB    NOT NULL,
    CHECK (length(image) = byte_len)
) STRICT;
"#;

/// Current schema for regenerable derived data.
const CACHE_SCHEMA: &str = r#"
CREATE TABLE cache_meta (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    document_id    BLOB    NOT NULL CHECK (length(document_id) = 16),
    kernel_id      TEXT    NOT NULL,
    kernel_version TEXT    NOT NULL
) STRICT;

CREATE TABLE blobs (
    hash       BLOB    PRIMARY KEY NOT NULL CHECK (length(hash) = 32),
    byte_len   INTEGER NOT NULL,
    created_at TEXT    NOT NULL
) STRICT;

CREATE TABLE blob_chunks (
    hash BLOB    NOT NULL REFERENCES blobs(hash) ON DELETE CASCADE,
    seq  INTEGER NOT NULL,
    data BLOB    NOT NULL,
    PRIMARY KEY (hash, seq)
) STRICT;

CREATE TABLE blob_refs (
    object_id BLOB NOT NULL CHECK (length(object_id) = 16),
    cache_key BLOB NOT NULL CHECK (length(cache_key) = 32),
    kind      TEXT NOT NULL,
    hash      BLOB NOT NULL REFERENCES blobs(hash) ON DELETE CASCADE,
    PRIMARY KEY (object_id, cache_key, kind)
) STRICT, WITHOUT ROWID;

CREATE INDEX blob_refs_by_hash ON blob_refs(hash);
"#;

/// Applies the pragmas a document connection must always run with.
///
/// `foreign_keys` is per-connection in SQLite and off by default, so it is set
/// here rather than assumed. Journalling stays in rollback mode: a document is
/// meant to be a single file that can be copied, mailed or committed, and a WAL
/// database is three files while it is open.
pub fn configure_document_connection(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "DELETE")
        .map_err(|e| CadError::io("setting document journal mode", e))?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(|e| CadError::io("setting document durability", e))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| CadError::io("enabling foreign keys", e))?;
    Ok(())
}

/// Applies the pragmas a cache connection runs with.
///
/// The sidecar trades durability for speed on purpose: losing it costs a
/// rebuild, not data.
pub fn configure_cache_connection(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| CadError::io("setting cache journal mode", e))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| CadError::io("setting cache durability", e))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| CadError::io("enabling foreign keys on cache", e))?;
    Ok(())
}

pub(crate) fn create_document(conn: &mut Connection) -> Result<()> {
    create(conn, DOCUMENT_SCHEMA, DOCUMENT_APPLICATION_ID, "document")
}

pub(crate) fn create_cache(conn: &mut Connection) -> Result<()> {
    create(conn, CACHE_SCHEMA, CACHE_APPLICATION_ID, "cache")
}

fn create(conn: &mut Connection, sql: &str, application_id: i32, what: &str) -> Result<()> {
    let tx = conn
        .transaction()
        .map_err(|e| CadError::io(format!("starting {what} creation"), e))?;
    tx.execute_batch(sql)
        .map_err(|e| CadError::io(format!("creating {what} tables"), e))?;
    tx.pragma_update(None, "application_id", application_id)
        .map_err(|e| CadError::io(format!("stamping {what} application id"), e))?;
    tx.commit()
        .map_err(|e| CadError::io(format!("committing {what} creation"), e))
}

/// Confirms the file is one of ours before any other interpretation of it.
pub fn check_application_id(conn: &Connection, expected: i32, what: &str) -> Result<()> {
    let found: i32 = conn
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|e| CadError::io(format!("reading {what} application id"), e))?;

    if found != expected {
        return Err(CadError::input(format!(
            "not a FerriteCAD {what}: application id is {found:#010x}, expected {expected:#010x}"
        )));
    }
    Ok(())
}

/// Reject an output catalogue before reading any model payload.
pub(crate) fn check_document_tables(conn: &Connection) -> Result<()> {
    let catalogue: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'topology_refs')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| CadError::io("checking document tables", e))?;
    if catalogue {
        return Err(CadError::input(
            "unsupported document table topology_refs: output names must be derived from features",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_types::ObjectId;

    fn fresh_document() -> Connection {
        let mut conn = Connection::open_in_memory().expect("in-memory sqlite always opens");
        create_document(&mut conn).expect("the current schema applies");
        conn
    }

    #[test]
    fn application_ids_are_the_ascii_tags() {
        assert_eq!(DOCUMENT_APPLICATION_ID.to_be_bytes(), *b"FCAD");
        assert_eq!(CACHE_APPLICATION_ID.to_be_bytes(), *b"FCAC");
    }

    #[test]
    fn a_created_document_carries_its_application_id() {
        let conn = fresh_document();
        check_application_id(&conn, DOCUMENT_APPLICATION_ID, "document")
            .expect("a created document is ours");
    }

    #[test]
    fn a_checkpoint_row_must_be_well_formed() {
        let conn = fresh_document();
        let insert = |name: &str, len: i64, image: &[u8]| {
            conn.execute(
                "INSERT INTO checkpoints (id, name, created_at, model, byte_len, image_hash, image)
                 VALUES (?1, ?2, '2026-10-08T00:00:00.000Z', ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    ObjectId::new().to_bytes().as_slice(),
                    name,
                    [1u8; 32].as_slice(),
                    len,
                    [2u8; 32].as_slice(),
                    image
                ],
            )
        };
        insert("A", 3, b"abc").expect("a well-formed row");
        assert!(insert("", 3, b"abc").is_err(), "an empty name");
        assert!(insert(&"x".repeat(81), 3, b"abc").is_err(), "81 characters");
        assert!(insert("B", 4, b"abc").is_err(), "a length that disagrees");
        assert!(insert("C", 0, b"").is_err(), "an empty image");
    }

    #[test]
    fn a_source_must_agree_with_its_own_length() {
        let conn = fresh_document();
        let insert =
            "INSERT INTO imported_sources (id, format, bytes, content_hash, byte_len, created_at)
                      VALUES (?1, 'exchange.step', ?2, ?3, ?4, '2026-01-01T00:00:00.000Z')";
        assert!(
            conn.execute(
                insert,
                rusqlite::params![
                    [1u8; 16].as_slice(),
                    b"ISO-10303-21;".as_slice(),
                    [2u8; 32].as_slice(),
                    99i64
                ],
            )
            .is_err(),
            "a blob shorter than its declared length is corruption SQLite can catch"
        );
        assert!(
            conn.execute(
                insert,
                rusqlite::params![
                    [1u8; 16].as_slice(),
                    b"ISO-10303-21;".as_slice(),
                    [2u8; 31].as_slice(),
                    13i64
                ],
            )
            .is_err(),
            "a 31-byte digest is not a BLAKE3-256 hash"
        );
    }

    #[test]
    fn a_foreign_sqlite_file_is_refused() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite always opens");
        conn.pragma_update(None, "application_id", 0x1234_5678i32)
            .expect("pragma is writable");

        let err = check_application_id(&conn, DOCUMENT_APPLICATION_ID, "document")
            .expect_err("a foreign application id must be refused");
        assert!(err.to_string().contains("not a FerriteCAD"));
    }

    #[test]
    fn document_connection_enables_foreign_keys() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite always opens");
        configure_document_connection(&conn).expect("pragmas apply");

        let enabled: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("pragma is readable");
        assert_eq!(enabled, 1);
    }
}
