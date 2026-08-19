// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, Result};
use rusqlite::Connection;

/// File extension of the native document.
pub const DOCUMENT_EXTENSION: &str = "fcad";

/// Extension of the regenerable cache sidecar.
pub const CACHE_EXTENSION: &str = "fcad-cache";

/// Version of the document *format* this build writes, recorded in `meta`.
///
/// Distinct from the SQL schema version in `PRAGMA user_version`, and the two
/// have been apart since schema v2. The SQL version counts migrations and moves
/// whenever a table or column is added; this one describes what a reader must
/// understand to make sense of the contents, and moves only when that changes.
/// Adding a storage table does not by itself change it: SQL compatibility is
/// enforced independently (a schema-v2 binary refuses schema v3), while a
/// capability declaration is the finer-grained instrument for readers that do
/// understand the container schema but not a particular object's meaning.
pub const FORMAT_VERSION: u32 = 1;

/// The oldest reader able to make sense of what this build writes.
///
/// Written into `meta` so a future reader can refuse a document instead of
/// misreading it. It only moves when a change is not backward compatible.
pub const MINIMUM_READER_VERSION: u32 = 1;

/// Capabilities this build implements.
///
/// A document that requires anything outside this list opens read-only: it is
/// better to refuse to write than to drop what we did not understand.
pub const SUPPORTED_CAPABILITIES: &[&str] = &[
    "core.part.v1",
    "exchange.step.imported.v1",
    "topology.extrude-cap-edge.v1",
    "topology.extrude-sweep-edge.v1",
];

/// `PRAGMA application_id` for a document: the ASCII bytes `FCAD`.
///
/// Set so `file(1)` and recovery tools can tell a FerriteCAD document from any
/// other SQLite database.
pub const DOCUMENT_APPLICATION_ID: i32 = i32::from_be_bytes(*b"FCAD");

/// `PRAGMA application_id` for a cache sidecar: the ASCII bytes `FCAC`.
pub const CACHE_APPLICATION_ID: i32 = i32::from_be_bytes(*b"FCAC");

/// Schema of the source of truth.
///
/// `STRICT` is used throughout so SQLite enforces column types instead of
/// silently coercing them, and every identifier column checks its own length —
/// a 15-byte UUID is a corrupt document, not a value to work around.
const MIGRATIONS: &[&str] = &[
    // v1
    r#"
CREATE TABLE meta (
    id                     INTEGER PRIMARY KEY CHECK (id = 1),
    format_version         INTEGER NOT NULL,
    minimum_reader_version INTEGER NOT NULL,
    document_id            BLOB    NOT NULL CHECK (length(document_id) = 16),
    display_length_unit    TEXT    NOT NULL,
    display_angle_unit     TEXT    NOT NULL,
    created_at             TEXT    NOT NULL,
    modified_at            TEXT    NOT NULL,
    generator              TEXT    NOT NULL
) STRICT;

-- Denormalised index of capabilities a reader must implement to write this
-- document. It is rebuilt after every successful edit; opening verifies the
-- envelope declarations too, because a hand-edited index is not authoritative.
CREATE TABLE capabilities (
    name     TEXT    PRIMARY KEY NOT NULL,
    required INTEGER NOT NULL CHECK (required IN (0, 1))
) STRICT;

-- Features, sketches, bodies, parameters and datums. `payload` is a CBOR
-- envelope; for an object type this build does not know, it is stored and
-- returned unchanged.
CREATE TABLE objects (
    id             BLOB    PRIMARY KEY NOT NULL CHECK (length(id) = 16),
    kind           TEXT    NOT NULL,
    schema_version INTEGER NOT NULL,
    parent_id      BLOB    REFERENCES objects(id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL DEFAULT 0,
    name           TEXT,
    payload        BLOB    NOT NULL,
    payload_hash   BLOB    NOT NULL CHECK (length(payload_hash) = 32)
) STRICT;

CREATE INDEX objects_by_parent ON objects(parent_id, ordinal);
CREATE INDEX objects_by_kind ON objects(kind);

-- Directed edges of the feature DAG. RESTRICT on the dependency side stops a
-- delete from silently orphaning a dependent feature.
CREATE TABLE deps (
    dependent_id  BLOB NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    dependency_id BLOB NOT NULL REFERENCES objects(id) ON DELETE RESTRICT,
    role          TEXT NOT NULL,
    PRIMARY KEY (dependent_id, dependency_id, role)
) STRICT, WITHOUT ROWID;

CREATE INDEX deps_by_dependency ON deps(dependency_id);

-- Stable semantic names for produced geometry. The binding of a name to a
-- concrete kernel sub-shape is cache and lives in the sidecar; what is stored
-- here is the intent that survives a rebuild.
CREATE TABLE topology_refs (
    id                BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 16),
    owner_id          BLOB NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    producer_feature  BLOB NOT NULL REFERENCES objects(id) ON DELETE RESTRICT,
    expected_kind     TEXT NOT NULL,
    payload           BLOB NOT NULL
) STRICT;

CREATE INDEX topology_refs_by_owner ON topology_refs(owner_id);
CREATE INDEX topology_refs_by_producer ON topology_refs(producer_feature);
"#,
    // v2: topology payloads are source-of-truth data as well. New rows carry
    // a BLAKE3 integrity hash; v1 rows retain NULL because SQLite cannot
    // calculate BLAKE3 during a migration. They remain readable and gain a
    // hash the next time their reference is explicitly rewritten.
    r#"
ALTER TABLE topology_refs
    ADD COLUMN payload_hash BLOB CHECK (payload_hash IS NULL OR length(payload_hash) = 32);
"#,
    // v3: the exact bytes of an imported file. They are source of truth in the
    // same sense the feature graph is — the scene stored beside them is one
    // reading, and a reading can be redone in a new kernel session, but only
    // while the bytes are still here. They are deliberately not in an object
    // payload: a payload is decoded to be understood, and multi-megabyte
    // source data has no business being decoded to answer a question about a
    // document's structure.
    r#"
CREATE TABLE imported_sources (
    id           BLOB    PRIMARY KEY NOT NULL CHECK (length(id) = 16),
    -- A project-owned tag rather than a media type: what this names is how
    -- FerriteCAD reads the bytes, which is a narrower claim than what the
    -- bytes are, and one this project is entitled to make.
    format       TEXT    NOT NULL,
    bytes        BLOB    NOT NULL,
    -- BLAKE3-256, the same digest as objects.payload_hash and
    -- topology_refs.payload_hash. Checked on every read, before the bytes
    -- reach anything that would interpret them.
    content_hash BLOB    NOT NULL CHECK (length(content_hash) = 32),
    byte_len     INTEGER NOT NULL CHECK (byte_len >= 0),
    created_at   TEXT    NOT NULL,
    -- A length that disagrees with the blob is corruption SQLite can catch
    -- without help, so it does.
    CHECK (length(bytes) = byte_len)
) STRICT;

-- One file, one row, however many objects were imported from it. A source is
-- immutable, so identical content is the same source rather than a copy.
CREATE UNIQUE INDEX imported_sources_by_content ON imported_sources(format, content_hash);

-- What makes a source reachable, as a row rather than a field inside a CBOR
-- payload. SQLite can enforce a row: deleting an object drops its claim on the
-- bytes even when this build cannot decode that object, and an object this
-- build has never heard of keeps its source alive for the build that can.
CREATE TABLE imported_source_refs (
    object_id BLOB NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    source_id BLOB NOT NULL REFERENCES imported_sources(id) ON DELETE RESTRICT,
    PRIMARY KEY (object_id, source_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX imported_source_refs_by_source ON imported_source_refs(source_id);
"#,
];

/// Refuses a document whose SQL schema cannot be read without changing it.
///
/// The ordinary open path migrates older schemas. A caller that promised to
/// leave the source file untouched needs the opposite contract: fail before a
/// query depends on a column that an unapplied migration would have created.
pub(crate) fn require_current_document_schema(conn: &Connection) -> Result<()> {
    let current = schema_version(conn)?;
    let target = MIGRATIONS.len() as u32;

    if current < target {
        return Err(CadError::unsupported(format!(
            "this document uses schema v{current} and needs migration to v{target}; it was opened \
             read-only and will not be changed"
        )));
    }
    if current > target {
        return Err(CadError::unsupported(format!(
            "this document was written by a newer FerriteCAD (schema v{current}, this build \
             understands up to v{target})"
        )));
    }
    Ok(())
}

/// Schema of the cache sidecar. Nothing here may affect a rebuild's result.
const CACHE_MIGRATIONS: &[&str] = &[
    // v1
    r#"
-- Binds the sidecar to exactly one document. A sidecar whose document_id or
-- format_version does not match is discarded, never repaired.
CREATE TABLE cache_meta (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    document_id    BLOB    NOT NULL CHECK (length(document_id) = 16),
    format_version INTEGER NOT NULL,
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

-- Maps (object, cache key, representation kind) to content. `kind` belongs to
-- the reference rather than the content-addressed blob: byte-identical B-Rep
-- and tessellation payloads are still different representations.
CREATE TABLE blob_refs (
    object_id BLOB NOT NULL CHECK (length(object_id) = 16),
    cache_key BLOB NOT NULL CHECK (length(cache_key) = 32),
    kind      TEXT NOT NULL,
    hash      BLOB NOT NULL REFERENCES blobs(hash) ON DELETE CASCADE,
    PRIMARY KEY (object_id, cache_key, kind)
) STRICT, WITHOUT ROWID;

CREATE INDEX blob_refs_by_hash ON blob_refs(hash);
"#,
    // v2: cache data is regenerable, so recreate the reference layout instead
    // of attempting a lossy migration from the old blob-level `kind` field.
    r#"
DROP TABLE blob_refs;
DROP TABLE blob_chunks;
DROP TABLE blobs;

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
"#,
];

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

/// Brings a document database up to [`FORMAT_VERSION`].
pub fn migrate_document(conn: &mut Connection) -> Result<()> {
    migrate(conn, MIGRATIONS, DOCUMENT_APPLICATION_ID, "document")
}

/// Brings a cache sidecar up to its current schema.
pub fn migrate_cache(conn: &mut Connection) -> Result<()> {
    migrate(conn, CACHE_MIGRATIONS, CACHE_APPLICATION_ID, "cache")
}

/// Reads the schema version recorded in `PRAGMA user_version`.
pub fn schema_version(conn: &Connection) -> Result<u32> {
    conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|e| CadError::io("reading schema version", e))
        .map(|v| v as u32)
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

/// Applies every migration past the recorded version, one transaction each.
///
/// A partially applied migration is the one failure mode that cannot be
/// recovered from by retrying, so the version bump shares a transaction with
/// the statements it describes.
fn migrate(
    conn: &mut Connection,
    migrations: &[&str],
    application_id: i32,
    what: &str,
) -> Result<()> {
    let current = schema_version(conn)?;
    let target = migrations.len() as u32;

    if current > target {
        return Err(CadError::unsupported(format!(
            "this {what} was written by a newer FerriteCAD (schema v{current}, this build \
             understands up to v{target})"
        )));
    }

    if current == 0 {
        conn.pragma_update(None, "application_id", application_id)
            .map_err(|e| CadError::io(format!("stamping {what} application id"), e))?;
    } else {
        check_application_id(conn, application_id, what)?;
    }

    for (index, statements) in migrations.iter().enumerate().skip(current as usize) {
        let version = index as u32 + 1;
        let tx = conn
            .transaction()
            .map_err(|e| CadError::io(format!("starting {what} migration v{version}"), e))?;
        tx.execute_batch(statements)
            .map_err(|e| CadError::io(format!("applying {what} migration v{version}"), e))?;
        tx.pragma_update(None, "user_version", i64::from(version))
            .map_err(|e| CadError::io(format!("recording {what} schema v{version}"), e))?;
        tx.commit()
            .map_err(|e| CadError::io(format!("committing {what} migration v{version}"), e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_ids_are_the_ascii_tags() {
        assert_eq!(DOCUMENT_APPLICATION_ID.to_be_bytes(), *b"FCAD");
        assert_eq!(CACHE_APPLICATION_ID.to_be_bytes(), *b"FCAC");
    }

    #[test]
    fn migration_is_idempotent() {
        let mut conn = Connection::open_in_memory().expect("in-memory sqlite always opens");
        migrate_document(&mut conn).expect("fresh database migrates");
        assert_eq!(schema_version(&conn).expect("version readable"), 3);

        migrate_document(&mut conn).expect("re-running applies nothing");
        assert_eq!(schema_version(&conn).expect("version readable"), 3);
    }

    /// Brings a database up to `version` and no further, as an older build
    /// would have left it.
    fn migrate_to(conn: &mut Connection, version: usize) {
        conn.pragma_update(None, "application_id", DOCUMENT_APPLICATION_ID)
            .expect("pragma is writable");
        for statements in &MIGRATIONS[..version] {
            conn.execute_batch(statements).expect("migration applies");
        }
        conn.pragma_update(None, "user_version", version as i64)
            .expect("pragma is writable");
    }

    #[test]
    fn a_schema_v2_document_reaches_v3_with_everything_it_had() {
        let mut conn = Connection::open_in_memory().expect("in-memory sqlite always opens");
        migrate_to(&mut conn, 2);

        let object = [7u8; 16];
        let reference = [9u8; 16];
        conn.execute(
            "INSERT INTO objects (id, kind, schema_version, ordinal, name, payload, payload_hash)
             VALUES (?1, 'sketch', 1, 0, 'Profile', ?2, ?3)",
            rusqlite::params![
                object.as_slice(),
                b"payload".as_slice(),
                [1u8; 32].as_slice()
            ],
        )
        .expect("v2 accepts an object");
        conn.execute(
            "INSERT INTO topology_refs (id, owner_id, producer_feature, expected_kind, payload)
             VALUES (?1, ?2, ?2, 'face', ?3)",
            rusqlite::params![
                reference.as_slice(),
                object.as_slice(),
                b"reference".as_slice()
            ],
        )
        .expect("v2 accepts a topology reference");

        migrate_document(&mut conn).expect("v2 migrates to v3");
        assert_eq!(schema_version(&conn).expect("version readable"), 3);

        let (name, payload): (String, Vec<u8>) = conn
            .query_row(
                "SELECT name, payload FROM objects WHERE id = ?1",
                rusqlite::params![object.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the object survived");
        assert_eq!(name, "Profile");
        assert_eq!(payload, b"payload");

        let stored: Vec<u8> = conn
            .query_row(
                "SELECT payload FROM topology_refs WHERE id = ?1",
                rusqlite::params![reference.as_slice()],
                |row| row.get(0),
            )
            .expect("the topology reference survived");
        assert_eq!(stored, b"reference");

        // And the new table is there and empty, rather than absent or guessed at.
        let sources: i64 = conn
            .query_row("SELECT count(*) FROM imported_sources", [], |row| {
                row.get(0)
            })
            .expect("imported_sources exists");
        assert_eq!(sources, 0);
    }

    #[test]
    fn a_source_must_agree_with_its_own_length() {
        let mut conn = Connection::open_in_memory().expect("in-memory sqlite always opens");
        migrate_document(&mut conn).expect("fresh database migrates");

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
    fn a_newer_schema_is_refused_rather_than_guessed_at() {
        let mut conn = Connection::open_in_memory().expect("in-memory sqlite always opens");
        migrate_document(&mut conn).expect("fresh database migrates");
        conn.pragma_update(None, "user_version", 99i64)
            .expect("pragma is writable");

        let err = migrate_document(&mut conn).expect_err("a future schema must be refused");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Unsupported);
    }

    #[test]
    fn a_foreign_sqlite_file_is_refused() {
        let mut conn = Connection::open_in_memory().expect("in-memory sqlite always opens");
        conn.pragma_update(None, "application_id", 0x1234_5678i32)
            .expect("pragma is writable");
        conn.pragma_update(None, "user_version", 1i64)
            .expect("pragma is writable");

        let err =
            migrate_document(&mut conn).expect_err("a foreign application id must be refused");
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
