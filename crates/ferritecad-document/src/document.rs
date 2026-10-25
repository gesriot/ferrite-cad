// SPDX-License-Identifier: MIT
use std::io::{ErrorKind as IoErrorKind, Read};
use std::path::{Path, PathBuf};

use ferritecad_exchange::{Diagnostic as ImportDiagnostic, Import, Scene};
use ferritecad_kernel::{KernelIdentity, ShapeHandle};
use ferritecad_types::{
    CadError, CanonicalHasher, ContentHash, Dimension, DocumentId, ImportedSourceId, ObjectId,
    OccurrenceId, Result, Unit,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};

use crate::graph::{Dependency, DependencyRole, evaluation_order};
use crate::model::{
    ImportedDefinitionRef, ImportedStep, ImporterIdentity, ObjectPayload, STEP_SOURCE_FORMAT,
};
use crate::schema::{self, CACHE_EXTENSION, DOCUMENT_APPLICATION_ID};
use crate::validate::ValidationReport;

mod checkpoint;
pub use checkpoint::{
    CheckpointEntry, MAX_CHECKPOINT_BYTES, MAX_CHECKPOINT_NAME_CHARS, MAX_CHECKPOINTS,
    checkpoint_name,
};

/// SQLite's own clock, so a timestamp is generated inside the writing
/// transaction rather than by a separate dependency.
const NOW_UTC: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// Whether this build may write to the open document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Access {
    ReadWrite,
    /// The document was opened, but writing it would risk losing meaning the
    /// reason describes.
    ReadOnly {
        reason: String,
    },
}

impl Access {
    pub fn is_writable(&self) -> bool {
        matches!(self, Self::ReadWrite)
    }
}

/// Document-wide facts, held in the single row of the `meta` table.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentMeta {
    pub document_id: DocumentId,
    /// Preferred unit for showing lengths. Stored values are millimetres.
    pub display_length_unit: Unit,
    /// Preferred unit for showing angles. Stored values are radians.
    pub display_angle_unit: Unit,
    pub created_at: String,
    pub modified_at: String,
    pub generator: String,
}

/// A stored object together with its storage-level facts.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectRecord {
    pub id: ObjectId,
    pub parent: Option<ObjectId>,
    /// Position among siblings, which is presentation order and nothing else.
    /// Evaluation order comes from the dependency graph.
    pub ordinal: i64,
    pub name: Option<String>,
    pub payload: ObjectPayload,
    pub payload_hash: ContentHash,
    storage_bytes: Vec<u8>,
}

impl ObjectRecord {
    pub(crate) fn storage_bytes(&self) -> &[u8] {
        &self.storage_bytes
    }
}

/// An imported STEP object together with the exact bytes it was built from.
///
/// The bytes have already been checked against their stored length and content
/// hash by the time this exists; see [`Document::step_import`].
#[derive(Debug, Clone, PartialEq)]
pub struct StoredStepImport {
    pub imported: ImportedStep,
    pub source: Vec<u8>,
}

/// What a caller holds after importing a file and before storing it.
///
/// Everything here already exists: the bytes have been read, the kernel has
/// made what it can of them, and the result is in hand. That is the point —
/// nothing that can fail is left to happen inside the writing transaction.
#[derive(Debug)]
pub struct StepImportRequest<'a> {
    pub object: ObjectId,
    /// What to call the object in the document. The names the file gave its own
    /// parts are in the scene and are not touched.
    pub name: Option<&'a str>,
    /// The exact bytes. These become the document's copy and its source of
    /// truth; no path is followed and no external file is consulted again.
    pub source: &'a [u8],
    /// What the file was called where it came from, if that is worth recording.
    /// Provenance for a person to read, never a location anything opens.
    pub source_name: Option<&'a str>,
    pub import: &'a Import,
    pub importer: &'a KernelIdentity,
}

/// What a document needs from a kernel session to re-read a stored import.
///
/// A trait rather than a closure because re-reading has two halves that must
/// belong to the same session: building shapes, and taking them back. A refusal
/// happens *after* the importer has built a whole scene, and something has to
/// release it — not the caller, which never sees a scene it was not allowed to
/// have, and not the document, which has no session. So the session that made
/// them is asked.
///
/// This is also the boundary that keeps this crate free of any kernel. Nothing
/// here links one, and the adapter that does implements this from the other
/// side.
pub trait StepImporter {
    /// Which implementation is making the current observation.
    ///
    /// This is provenance, not a compatibility gate. It is returned beside
    /// `diagnostics_now` so a caller can attribute both readings instead of
    /// knowing only who made the historical one.
    fn identity(&self) -> &KernelIdentity;
    fn import(&mut self, source: &[u8]) -> Result<Import>;
    /// Called for every shape of a scene that could not be bound.
    fn release(&mut self, shape: ShapeHandle);
}

/// A stored import, read again in a live kernel session.
///
/// The two diagnostic sets are named apart because they are two observations,
/// made at different times by possibly different builds. Merging them, or
/// letting one stand in for the other, would attribute to one reading what only
/// the other ever saw.
#[derive(Debug, Clone, PartialEq)]
pub struct ReopenedStepImport {
    /// Handles issued by the session that has just re-read the bytes, and only
    /// after the whole scene was proven to match what was stored.
    pub scene: Scene,
    /// The bytes this reading came from.
    ///
    /// Kept so a reference can be checked against the source it names rather
    /// than resolved against whatever import happens to be at hand.
    source: ImportedSourceId,
    /// The durable identity of each placement, taken from the stored payload
    /// after the fresh reading was proven to be the same scene.
    ///
    /// Never from the fresh reading. A kernel that has just read the bytes
    /// again knows the geometry and knows nothing about which occurrence this
    /// document decided each place was, so a value taken from there would be
    /// this session's traversal wearing an identity's name.
    occurrences: Vec<OccurrenceId>,
    /// What the importer said when the file was first brought into this
    /// document. Historical; this build did not observe it.
    pub diagnostics_at_import: Vec<ImportDiagnostic>,
    /// What the importer said just now, reading the same bytes.
    pub diagnostics_now: Vec<ImportDiagnostic>,
    /// Which kernel produced the stored scene.
    pub imported_by: ImporterIdentity,
    /// Which kernel produced the fresh handles and `diagnostics_now`.
    pub reopened_by: ImporterIdentity,
}

impl ReopenedStepImport {
    /// The immutable source identity this reading was verified against.
    ///
    /// Read-only on purpose: allowing a caller to replace it would turn the
    /// source check in [`Self::resolve`] into a value the caller can arrange to
    /// pass for bytes this reading did not come from.
    pub fn source(&self) -> ImportedSourceId {
        self.source
    }

    /// Returns the stored identity of each placement, aligned with the bound scene.
    pub fn occurrences(&self) -> &[OccurrenceId] {
        &self.occurrences
    }

    /// Finds the definition a durable reference names, in this reading.
    ///
    /// Resolution happens inside one source and nowhere else. The reference
    /// names the bytes it belongs to, this reading knows which bytes it came
    /// from, and a mismatch is refused rather than searched around: the same
    /// key text occurs in unrelated files and means something different in
    /// each. Nothing falls back to a name, a position or a nearest match,
    /// because a reference that resolves to something plausible is worse than
    /// one that does not resolve at all.
    ///
    /// The two ways this fails are two different facts, and they are reported
    /// as two different kinds so a caller can tell them apart without reading
    /// prose:
    ///
    /// * [`Input`][ferritecad_types::ErrorKind::Input] — the reference is about
    ///   another source, or names more than one definition of this reading.
    /// * [`Topology`][ferritecad_types::ErrorKind::Topology] — the right
    ///   source, and nothing in it carries that identity any more. That is a
    ///   lost reference, which is the one case a user has to be told about by
    ///   name.
    pub fn resolve(&self, reference: &ImportedDefinitionRef) -> Result<ShapeHandle> {
        if reference.source() != self.source {
            return Err(CadError::input(format!(
                "{reference} cannot be resolved against source {}: a definition key \
                 identifies a part within one file, and the same key in another file \
                 is another part",
                self.source
            )));
        }

        let mut found = self
            .scene
            .definitions
            .iter()
            .filter(|definition| definition.key == reference.definition_key());
        let definition = found.next().ok_or_else(|| {
            CadError::topology(format!(
                "{reference} no longer names anything this file describes"
            ))
        })?;
        if found.next().is_some() {
            return Err(CadError::input(format!(
                "{reference} names more than one definition in this reading, so it \
                 would resolve to whichever was looked at first"
            )));
        }
        Ok(definition.shape)
    }
}

/// An open native document.
#[derive(Debug)]
pub struct Document {
    conn: Connection,
    path: PathBuf,
    meta: DocumentMeta,
    access: Access,
}

impl Document {
    /// Creates a new document, refusing to overwrite an existing file.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::create_with(path, Unit::Millimeter, Unit::Degree)
    }

    /// Creates a document with explicit display units.
    pub fn create_with(
        path: impl AsRef<Path>,
        display_length_unit: Unit,
        display_angle_unit: Unit,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        validate_display_units(display_length_unit, display_angle_unit)?;
        if path.exists() {
            return Err(CadError::input(format!(
                "{} already exists; creating would destroy it",
                path.display()
            )));
        }

        let mut conn = open_connection(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        schema::configure_document_connection(&conn)?;
        schema::create_document(&mut conn)?;

        let document_id = DocumentId::new();
        let generator = format!("FerriteCAD {}", env!("CARGO_PKG_VERSION"));

        let tx = conn
            .transaction()
            .map_err(|e| CadError::io("starting document initialisation", e))?;
        tx.execute(
            &format!(
                "INSERT INTO meta (
                     id, document_id,
                     display_length_unit, display_angle_unit, created_at, modified_at, generator
                 ) VALUES (1, ?1, ?2, ?3, {NOW_UTC}, {NOW_UTC}, ?4)"
            ),
            params![
                document_id.to_bytes().as_slice(),
                display_length_unit.symbol(),
                display_angle_unit.symbol(),
                generator,
            ],
        )
        .map_err(|e| CadError::io("writing document metadata", e))?;
        tx.commit()
            .map_err(|e| CadError::io("committing document initialisation", e))?;

        let meta = read_meta(&conn)?;
        Ok(Self {
            conn,
            path,
            meta,
            access: Access::ReadWrite,
        })
    }

    /// Opens an existing document after checking all object payloads.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut document = Self::open_read_only(path)?;
        let conn = open_connection(&document.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        schema::configure_document_connection(&conn)?;
        document.conn = conn;
        document.access = Access::ReadWrite;
        Ok(document)
    }

    /// Opens a document snapshot without changing the file or its sidecars.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        refuse_wal_journal(&path)?;
        refuse_wal_sidecars(&path)?;
        let conn = open_connection(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        // Pin one SQLite reading for every query (metadata, geometry and edit
        // identity alike). A concurrent commit cannot split a load in two.
        conn.execute_batch("BEGIN")
            .map_err(|e| CadError::io("starting document snapshot", e))?;
        schema::check_application_id(&conn, DOCUMENT_APPLICATION_ID, "document")?;
        schema::check_document_tables(&conn)?;
        let meta = read_meta(&conn)?;
        let document = Self {
            conn,
            path,
            meta,
            access: Access::ReadOnly {
                reason: "it was explicitly opened without write access".to_owned(),
            },
        };
        document.objects()?;
        Ok(document)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn meta(&self) -> &DocumentMeta {
        &self.meta
    }

    pub fn access(&self) -> &Access {
        &self.access
    }

    /// Checks whether a copy can be edited without unaccounted trigger effects.
    pub fn copy_access(&self) -> Result<Access> {
        // The bounded edit promises to change only its selected feature. A
        // stored trigger can change arbitrary other data during that write;
        // neither executing nor silently stripping it preserves that promise.
        let trigger: Option<String> = self
            .conn
            .query_row(
                "SELECT name FROM sqlite_schema WHERE type = 'trigger' ORDER BY name LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CadError::io("checking stored SQL triggers", e))?;
        if let Some(name) = trigger {
            return Ok(Access::ReadOnly {
                reason: format!(
                    "editing a document with stored SQL trigger {name:?} is unsupported"
                ),
            });
        }
        // Foreign-key actions are implicit write programs too. Permit the
        // three cascades in the current document schema; extensions with
        // further mutating actions need an edit contract of their own.
        let mut statement = self
            .conn
            .prepare(
                "SELECT s.name, f.\"from\", f.\"table\", f.\"to\", f.on_update, f.on_delete \
             FROM sqlite_schema AS s, pragma_foreign_key_list(s.name, 'main') AS f \
             WHERE s.type = 'table' ORDER BY s.name, f.id, f.seq",
            )
            .map_err(|e| CadError::io("checking SQL foreign-key actions", e))?;
        let keys = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| CadError::io("reading SQL foreign-key actions", e))?;
        for key in keys {
            let (table, column, parent, target, update, delete) =
                key.map_err(|e| CadError::io("reading SQL foreign-key action", e))?;
            let no_write = |action: &str| matches!(action, "NO ACTION" | "RESTRICT");
            if no_write(&update) && no_write(&delete) {
                continue;
            }
            let native_cascade = update == "NO ACTION"
                && delete == "CASCADE"
                && parent == "objects"
                && target.as_deref() == Some("id")
                && matches!(
                    (table.as_str(), column.as_str()),
                    ("objects", "parent_id")
                        | ("deps", "dependent_id")
                        | ("imported_source_refs", "object_id")
                );
            if !native_cascade {
                return Ok(Access::ReadOnly {
                    reason: format!(
                        "editing with foreign-key action on {table:?}.{column:?} (ON UPDATE {update}, ON DELETE {delete}) is unsupported"
                    ),
                });
            }
        }
        Ok(Access::ReadWrite)
    }

    /// A complete logical content version, including extension tables
    /// and imported BLOBs. Implicit row identities are included: they are SQL
    /// observable data, even when no declared column aliases them. No filesystem
    /// clock or SQLite page layout enters it. Tables that hide every rowid alias
    /// are refused, rather than assigned an incomplete version.
    /// Read this from `open_read_only`, whose transaction pins the whole reading.
    pub fn content_version(&self) -> Result<ContentHash> {
        self.logical_content("document.logical-content", false, false)
    }

    /// The same logical content with only `meta.modified_at` set aside (read
    /// as NULL), under its own domain tag so it can never be mistaken for a
    /// [`Self::content_version`].
    ///
    /// What an open document session means by "the model is the same": the
    /// stamp every writer refreshes says when something was written, not what
    /// the model is, so a no-op edit or an edit back to the original value is
    /// the saved model again. Everything else, including unknown tables and
    /// implicit row identities, still counts. `content_version` itself is
    /// unchanged byte for byte.
    pub fn model_version(&self) -> Result<ContentHash> {
        self.logical_content("document.model-content", true, false)
    }

    /// Hashes model content with the checkpoint catalog excluded.
    pub fn model_without_checkpoints(&self) -> Result<ContentHash> {
        self.logical_content("document.model-without-checkpoints", true, true)
    }

    fn logical_content(
        &self,
        domain: &str,
        ignore_modified_at: bool,
        without_checkpoints: bool,
    ) -> Result<ContentHash> {
        fn quoted(name: &str) -> String {
            format!("\"{}\"", name.replace('"', "\"\""))
        }
        let read = || -> Result<ContentHash> {
            let sql_error = |e| CadError::io("reading complete document version", e);
            let mut hash = CanonicalHasher::new(domain);
            hash.algorithm_version(2);
            let mut schema = self
                .conn
                .prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type, name")
                .map_err(sql_error)?;
            let mut rows = schema.query([]).map_err(sql_error)?;
            let mut tables = Vec::new();
            while let Some(row) = rows.next().map_err(sql_error)? {
                if without_checkpoints
                    && row.get_ref(2).map_err(sql_error)?.as_str().ok() == Some("checkpoints")
                {
                    continue;
                }
                for column in 0..4 {
                    let value: Option<String> = row.get(column).map_err(sql_error)?;
                    hash.bool(value.is_some())
                        .str(value.as_deref().unwrap_or_default());
                }
                if row.get::<_, String>(0).map_err(sql_error)? == "table" {
                    tables.push(row.get::<_, String>(1).map_err(sql_error)?);
                }
            }
            for table in tables {
                hash.field(&table);
                let without_rowid: bool = self
                    .conn
                    .query_row(
                        "SELECT wr FROM pragma_table_list WHERE schema = 'main' AND name = ?1",
                        [&table],
                        |row| row.get(0),
                    )
                    .map_err(sql_error)?;
                let identity = if without_rowid {
                    String::new()
                } else {
                    let mut columns = self
                        .conn
                        .prepare("SELECT name FROM pragma_table_xinfo(?1, 'main')")
                        .map_err(sql_error)?;
                    let names: Vec<String> = columns
                        .query_map([&table], |row| row.get(0))
                        .map_err(sql_error)?
                        .collect::<rusqlite::Result<_>>()
                        .map_err(sql_error)?;
                    let alias = ["_rowid_", "rowid", "oid"].into_iter().find(|alias| {
                        !names.iter().any(|name| name.eq_ignore_ascii_case(alias))
                    }).ok_or_else(|| CadError::unsupported(format!(
                        "cannot read complete identity of table {table:?}: all rowid aliases are shadowed"
                    )))?;
                    format!("{alias}, ")
                };
                let select = format!("SELECT {identity}* FROM {}", quoted(&table));
                let count = self
                    .conn
                    .prepare(&select)
                    .map_err(sql_error)?
                    .column_count();
                // Rowid is the first, unique sort key. WITHOUT ROWID tables
                // include their primary key in *. Use binary text comparison:
                // a column's collation may differ from its key's collation.
                let order = (1..=count)
                    .map(|i| format!("{i} COLLATE BINARY"))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut statement = self
                    .conn
                    .prepare(&format!("{select} ORDER BY {order}"))
                    .map_err(sql_error)?;
                // The result columns by position, so the stamp is found by
                // name whatever the rowid prefix does to the indices.
                let statement_columns: Vec<String> = statement
                    .column_names()
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                let mut rows = statement.query([]).map_err(sql_error)?;
                while let Some(row) = rows.next().map_err(sql_error)? {
                    hash.field("row");
                    for column in 0..count {
                        use rusqlite::types::ValueRef;
                        let stamp = ignore_modified_at
                            && table == "meta"
                            && statement_columns
                                .get(column)
                                .is_some_and(|name| name == "modified_at");
                        if stamp {
                            hash.field("null");
                            continue;
                        }
                        match row.get_ref(column).map_err(sql_error)? {
                            ValueRef::Null => {
                                hash.field("null");
                            }
                            ValueRef::Integer(v) => {
                                hash.field("integer").bytes(&v.to_be_bytes());
                            }
                            ValueRef::Real(v) => {
                                hash.field("real").bytes(&v.to_bits().to_be_bytes());
                            }
                            ValueRef::Text(v) => {
                                hash.field("text").bytes(v);
                            }
                            ValueRef::Blob(v) => {
                                hash.field("blob").bytes(v);
                            }
                        }
                    }
                }
            }
            Ok(hash.finish())
        };
        read()
    }

    /// Copies this pinned reading through SQLite's online backup API. The
    /// destination must be inside caller-owned private storage and absent.
    /// All pages are copied, including data this build does not decode.
    pub fn snapshot_to(&self, destination: &Path) -> Result<()> {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| CadError::io("reserving document snapshot", e))?;
        self.snapshot_into_reserved(destination)
    }

    /// The caller has exclusively created this empty file in private storage.
    fn snapshot_into_reserved(&self, destination: &Path) -> Result<()> {
        let mut target = open_connection(destination, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let backup = rusqlite::backup::Backup::new(&self.conn, &mut target)
            .map_err(|e| CadError::io("starting SQLite backup", e))?;
        let step = backup
            .step(-1)
            .map_err(|e| CadError::io("copying SQLite snapshot", e))?;
        if step != rusqlite::backup::StepResult::Done {
            return Err(CadError::input("source is busy; reopen it and try again"));
        }
        drop(backup);
        target
            .close()
            .map_err(|(_, e)| CadError::io("closing SQLite snapshot", e))
    }

    /// Where this document's regenerable cache sidecar lives.
    pub fn cache_path(&self) -> PathBuf {
        self.path.with_extension(CACHE_EXTENSION)
    }

    /// Runs `edit` inside one transaction, committing only if it succeeds.
    ///
    /// A failed edit leaves the document exactly as it was. Nothing partial is
    /// ever visible, which is what lets a cancelled or failed rebuild be
    /// abandoned without cleanup.
    pub fn write<T>(
        &mut self,
        edit: impl FnOnce(&mut DocumentWriter<'_>) -> Result<T>,
    ) -> Result<T> {
        self.write_transaction(edit, true)
    }

    fn write_transaction<T>(
        &mut self,
        edit: impl FnOnce(&mut DocumentWriter<'_>) -> Result<T>,
        stamp: bool,
    ) -> Result<T> {
        self.write_checked_transaction(|_| Ok(()), edit, stamp)
    }

    pub(crate) fn write_reading<T>(
        &mut self,
        edit: impl FnOnce(&Self, &mut DocumentWriter<'_>) -> Result<T>,
    ) -> Result<T> {
        self.write_reading_checked(|_| Ok(()), edit, true)
    }

    fn write_checked_transaction<T>(
        &mut self,
        check: impl FnOnce(&Self) -> Result<()>,
        edit: impl FnOnce(&mut DocumentWriter<'_>) -> Result<T>,
        stamp: bool,
    ) -> Result<T> {
        self.write_reading_checked(check, |_, writer| edit(writer), stamp)
    }

    fn write_reading_checked<T>(
        &mut self,
        check: impl FnOnce(&Self) -> Result<()>,
        edit: impl FnOnce(&Self, &mut DocumentWriter<'_>) -> Result<T>,
        stamp: bool,
    ) -> Result<T> {
        if let Access::ReadOnly { reason } = &self.access {
            return Err(CadError::unsupported(format!(
                "{} is open read-only: {reason}",
                self.path.display()
            )));
        }

        // Reachability rows are the only thing that makes automatic source
        // reclamation safe. Refuse to edit a damaged document before a
        // transaction can turn recoverable orphan bytes into permanent loss.
        self.require_imported_source_reachability()?;

        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| CadError::io("starting document edit", e))?;

        // Re-derive against the same SQLite snapshot the write will consume.
        check(self)?;
        let outcome = {
            let mut writer = DocumentWriter { tx: &tx };
            edit(self, &mut writer)
        };

        match outcome {
            Ok(value) => {
                if stamp {
                    reclaim_imported_sources(&tx)?;
                    tx.execute(
                        &format!("UPDATE meta SET modified_at = {NOW_UTC} WHERE id = 1"),
                        [],
                    )
                    .map_err(|e| CadError::io("stamping modification time", e))?;
                }
                tx.commit()
                    .map_err(|e| CadError::io("committing document edit", e))?;
                self.meta = read_meta(&self.conn)?;
                Ok(value)
            }
            Err(error) => {
                // The rollback is what `Transaction` does on drop; being
                // explicit keeps the intent visible at the failure site.
                drop(tx);
                Err(error)
            }
        }
    }

    pub fn object(&self, id: ObjectId) -> Result<Option<ObjectRecord>> {
        self.conn
            .query_row(
                "SELECT id, kind, parent_id, ordinal, name, payload, payload_hash
                 FROM objects WHERE id = ?1",
                params![id.to_bytes().as_slice()],
                read_object_row,
            )
            .optional()
            .map_err(|e| CadError::io(format!("reading object {id}"), e))?
            .transpose()
    }

    /// Every object, ordered by parent and then presentation position.
    pub fn objects(&self) -> Result<Vec<ObjectRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, parent_id, ordinal, name, payload, payload_hash
                 FROM objects ORDER BY parent_id, ordinal, id",
            )
            .map_err(|e| CadError::io("preparing object query", e))?;

        let rows = stmt
            .query_map([], read_object_row)
            .map_err(|e| CadError::io("reading objects", e))?;

        let mut objects = Vec::new();
        for row in rows {
            objects.push(row.map_err(|e| CadError::io("reading object row", e))??);
        }
        Ok(objects)
    }

    pub fn dependencies(&self) -> Result<Vec<Dependency>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT dependent_id, dependency_id, role
                 FROM deps ORDER BY dependent_id, dependency_id, role",
            )
            .map_err(|e| CadError::io("preparing dependency query", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| CadError::io("reading dependencies", e))?;

        let mut deps = Vec::new();
        for row in rows {
            let (dependent, dependency, role) =
                row.map_err(|e| CadError::io("reading dependency row", e))?;
            deps.push(Dependency {
                dependent: ObjectId::from_slice(&dependent)?,
                dependency: ObjectId::from_slice(&dependency)?,
                role: DependencyRole::parse(&role)?,
            });
        }
        Ok(deps)
    }

    /// Stores a successful import as one object and one copy of its bytes.
    ///
    /// The ordering is the contract. Reading the file, transferring it and
    /// projecting its scene have all happened before this is called; what is
    /// left that could fail — projecting the scene, sizing the source — is done
    /// before the transaction opens, and the transaction itself writes the
    /// bytes, the object and the object's claim on those bytes together or not
    /// at all. A [`Import::Rejected`] has no scene to store and is refused here
    /// without the document being opened for writing.
    ///
    /// Returns what was written, including the source identifier that ended up
    /// holding the bytes — which is an existing one when this document already
    /// held an identical file.
    pub fn store_step_import(&mut self, request: StepImportRequest<'_>) -> Result<ImportedStep> {
        let scene = match request.import {
            Import::Imported { scene, .. } => scene,
            Import::Rejected { diagnostics } => {
                return Err(CadError::input(format!(
                    "this file was not imported, so there is no scene to store: {}",
                    describe(diagnostics)
                )));
            }
        };

        let persisted = scene.persist()?;
        let source_hash = ContentHash::of_bytes(request.source);
        let source_byte_len = request.source.len() as u64;
        let imported_by = ImporterIdentity::of(request.importer);
        let diagnostics_at_import = request.import.diagnostics().to_vec();

        let source_bytes = request.source;
        let object = request.object;
        let name = request.name;
        let source_name = request.source_name.map(str::to_owned);

        self.write(move |w| {
            let source = w.put_step_source(source_bytes)?;
            let imported = ImportedStep {
                source,
                source_hash,
                source_byte_len,
                source_name,
                scene: persisted,
                imported_by,
                diagnostics_at_import,
            };
            w.put_imported_step(object, None, 0, name, &imported)?;
            Ok(imported)
        })
    }

    /// Reads an imported STEP object and the bytes it names.
    ///
    /// Everything checkable is checked here, before the bytes go anywhere that
    /// would interpret them: the source row exists, its blob is as long as it
    /// says, its content hash matches what is actually stored, and both agree
    /// with what the object recorded when it was written. A caller that reaches
    /// a kernel with these bytes has already been told if they are not the ones
    /// this document was built from.
    ///
    /// `Ok(None)` means no such object. An object of another type is an error:
    /// asking a sketch for its STEP bytes is a mistake, not an absence.
    pub fn step_import(&self, id: ObjectId) -> Result<Option<StoredStepImport>> {
        let Some(record) = self.object(id)? else {
            return Ok(None);
        };
        let ObjectPayload::ImportedStep(imported) = record.payload else {
            return Err(CadError::input(format!(
                "object {id} is a {}, not an imported STEP file",
                record.payload.type_name()
            )));
        };

        require_imported_source_ref(&self.conn, id, imported.source)?;
        let source = self.read_imported_source(&imported)?;
        Ok(Some(StoredStepImport { imported, source }))
    }

    /// Re-reads a stored import in a live kernel session.
    ///
    /// `importer` is handed bytes this document has already verified and must
    /// return what a kernel made of them. Whatever it produces is checked
    /// against the stored scene in full — units, schema, every definition, the
    /// whole instance tree, placements and colours — and only then are its
    /// handles returned. A file that no longer imports as what was saved is
    /// refused; nothing is matched up approximately.
    ///
    /// A refusal releases every shape the importer built. Nothing is bound, so
    /// nothing is left held.
    ///
    /// A differing [`ImporterIdentity`] is not itself a refusal. `build`
    /// carries the target triple, so the same release on another operating
    /// system differs there by construction, and a document that could not
    /// cross platforms would be no use. What must agree is the scene.
    pub fn reopen_step_import(
        &self,
        id: ObjectId,
        importer: &mut impl StepImporter,
    ) -> Result<ReopenedStepImport> {
        let stored = self.step_import(id)?.ok_or_else(|| {
            CadError::input(format!("this document has no imported STEP object {id}"))
        })?;

        let reopened_by = ImporterIdentity::of(importer.identity());
        let outcome = importer.import(&stored.source)?;
        let (scene, diagnostics_now) = match outcome {
            Import::Imported { scene, diagnostics } => (scene, diagnostics),
            Import::Rejected { diagnostics } => {
                return Err(CadError::input(format!(
                    "the STEP file stored as {id} imported when it was saved and is refused now, \
                     so its scene could not be re-attached: {}",
                    describe(&diagnostics)
                )));
            }
        };

        // Collected before the scene is handed over, because binding consumes
        // it and a refusal must still be able to give the shapes back.
        let shapes: Vec<ShapeHandle> = scene.shapes().collect();
        let scene = match stored.imported.scene.bind(scene) {
            Ok(scene) => scene,
            Err(error) => {
                for shape in shapes {
                    importer.release(shape);
                }
                return Err(error);
            }
        };
        // Read after the binding and not before: what the payload says about
        // placements is only meaningful once the fresh reading has been proven
        // to be the same scene the payload describes.
        let occurrences = stored.imported.scene.occurrences();
        Ok(ReopenedStepImport {
            scene,
            source: stored.imported.source,
            occurrences,
            diagnostics_at_import: stored.imported.diagnostics_at_import,
            diagnostics_now,
            imported_by: stored.imported.imported_by,
            reopened_by,
        })
    }

    /// Confirms the denormalised reachability table is safe to use for GC.
    ///
    /// This deliberately does not hash every potentially large source before
    /// every edit; `step_import` and `validate` do that when the bytes are
    /// read. Here the question is narrower: would collecting an unreferenced
    /// row destroy bytes a known object still names?
    pub(crate) fn require_imported_source_reachability(&self) -> Result<()> {
        for record in self.objects()? {
            if let ObjectPayload::ImportedStep(imported) = record.payload {
                require_imported_source_ref(&self.conn, record.id, imported.source)?;
            }
        }

        let orphan_count: i64 = self
            .conn
            .query_row(
                "SELECT count(*) FROM imported_sources s
                 WHERE NOT EXISTS (
                     SELECT 1 FROM imported_source_refs r WHERE r.source_id = s.id
                 )",
                [],
                |row| row.get(0),
            )
            .map_err(|e| CadError::io("checking imported source reachability", e))?;
        if orphan_count != 0 {
            return Err(CadError::input(format!(
                "this document has {orphan_count} imported source row(s) with no owning object; \
                 refusing to edit because automatic cleanup could destroy recoverable source bytes"
            )));
        }
        Ok(())
    }

    fn read_imported_source(&self, imported: &ImportedStep) -> Result<Vec<u8>> {
        read_imported_source(&self.conn, imported)
    }

    /// The order features must be rebuilt in.
    pub fn evaluation_order(&self) -> Result<Vec<ObjectId>> {
        let nodes: Vec<ObjectId> = self.objects()?.into_iter().map(|o| o.id).collect();
        evaluation_order(&nodes, &self.dependencies()?)
    }

    /// Checks the stored document's integrity and semantic structure.
    pub fn validate(&self) -> Result<ValidationReport> {
        crate::validate::validate(self)
    }

    /// Closes the connection.
    ///
    /// Copying, sending or committing a document should happen after this, not
    /// while it is open. The document uses rollback journalling precisely so
    /// that a closed document is one self-contained file.
    pub fn close(self) -> Result<()> {
        self.conn
            .close()
            .map_err(|(_, e)| CadError::io("closing document", e))
    }
}

/// Reads and verifies one immutable source row.
///
/// Shared by the ordinary read path and the low-level writer: persistence must
/// reject a mismatched source/object pair when it is written, not merely the
/// next time somebody tries to use it.
fn read_imported_source(conn: &Connection, imported: &ImportedStep) -> Result<Vec<u8>> {
    let source = imported.source;
    let row: Option<(Vec<u8>, Vec<u8>, i64, String)> = conn
        .query_row(
            "SELECT bytes, content_hash, byte_len, format FROM imported_sources WHERE id = ?1",
            params![source.to_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|e| CadError::io(format!("reading imported source {source}"), e))?;

    let (bytes, content_hash, byte_len, format) = row.ok_or_else(|| {
        CadError::input(format!(
            "imported source {source} is not in this document; its bytes are the only copy \
             and cannot be recovered from what is left"
        ))
    })?;

    if format != STEP_SOURCE_FORMAT {
        return Err(CadError::input(format!(
            "imported source {source} is stored as {format:?}, and this object was imported \
             from {STEP_SOURCE_FORMAT:?}"
        )));
    }

    let stored_hash = ContentHash::from_slice(&content_hash)?;
    let byte_len = u64::try_from(byte_len).map_err(|_| {
        CadError::input(format!(
            "imported source {source} declares {byte_len} bytes"
        ))
    })?;

    // Length first: the cheapest disagreement to find, and the one a
    // truncated write produces.
    if bytes.len() as u64 != byte_len {
        return Err(CadError::input(format!(
            "imported source {source} holds {} byte(s) and declares {byte_len}",
            bytes.len()
        )));
    }
    if byte_len != imported.source_byte_len {
        return Err(CadError::input(format!(
            "imported source {source} holds {byte_len} byte(s); the object built from it \
             recorded {}",
            imported.source_byte_len
        )));
    }
    if ContentHash::of_bytes(&bytes) != stored_hash {
        return Err(CadError::input(format!(
            "imported source {source} does not match its stored hash; the file may be corrupt"
        )));
    }
    if stored_hash != imported.source_hash {
        return Err(CadError::input(format!(
            "imported source {source} holds different bytes from the ones this object was \
             built from: stored {stored_hash}, expected {}",
            imported.source_hash
        )));
    }
    Ok(bytes)
}

/// Requires exactly one reachability row, and requires it to name the source
/// the object's payload names. Multiple rows are corruption too: this object
/// contract has exactly one source.
fn require_imported_source_ref(
    conn: &Connection,
    object: ObjectId,
    expected: ImportedSourceId,
) -> Result<()> {
    let mut stmt = conn
        .prepare(
            "SELECT source_id FROM imported_source_refs
             WHERE object_id = ?1 ORDER BY source_id",
        )
        .map_err(|e| CadError::io("preparing imported source reference query", e))?;
    let rows = stmt
        .query_map(params![object.to_bytes().as_slice()], |row| {
            row.get::<_, Vec<u8>>(0)
        })
        .map_err(|e| CadError::io(format!("reading source claims of object {object}"), e))?;
    let mut found = Vec::new();
    for row in rows {
        found.push(
            row.map_err(|e| CadError::io(format!("reading source claim of object {object}"), e))?,
        );
    }

    if found.len() != 1 {
        return Err(CadError::input(format!(
            "imported STEP object {object} must claim exactly one source row, found {}",
            found.len()
        )));
    }
    let actual = ImportedSourceId::from_slice(&found[0])?;
    if actual != expected {
        return Err(CadError::input(format!(
            "imported STEP object {object} names source {expected} in its payload but claims \
             source {actual} in the reachability table"
        )));
    }
    Ok(())
}

/// The write half of a document, valid only inside [`Document::write`].
#[derive(Debug)]
pub struct DocumentWriter<'a> {
    tx: &'a Transaction<'a>,
}

impl DocumentWriter<'_> {
    /// An edit changes the payload, not ownership or claims on attached source bytes.
    pub(crate) fn update_payload(&mut self, id: ObjectId, payload: &ObjectPayload) -> Result<()> {
        let bytes = payload.to_storage_bytes()?;
        let hash = ContentHash::of_bytes(&bytes);
        let changed = self
            .tx
            .execute(
                "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3 AND kind=?4",
                params![
                    bytes,
                    hash.as_bytes().as_slice(),
                    id.to_bytes().as_slice(),
                    payload.type_name()
                ],
            )
            .map_err(|e| CadError::io(format!("updating payload of object {id}"), e))?;
        if changed != 1 {
            return Err(CadError::input(format!(
                "object {id} is missing or has a different kind"
            )));
        }
        Ok(())
    }

    /// Inserts or replaces an object.
    pub fn put_object(
        &mut self,
        id: ObjectId,
        parent: Option<ObjectId>,
        ordinal: i64,
        name: Option<&str>,
        payload: &ObjectPayload,
    ) -> Result<ContentHash> {
        if let ObjectPayload::ImportedStep(_) = payload {
            return Err(CadError::unsupported(
                "an imported STEP object owns source bytes as well as a payload; write it with \
                 put_imported_step so the object and its claim on those bytes are recorded \
                 together",
            ));
        }
        let hash = self.write_object(id, parent, ordinal, name, payload)?;

        self.tx
            .execute(
                "DELETE FROM imported_source_refs WHERE object_id = ?1",
                params![id.to_bytes().as_slice()],
            )
            .map_err(|e| CadError::io(format!("clearing source claims of object {id}"), e))?;
        Ok(hash)
    }

    fn write_object(
        &mut self,
        id: ObjectId,
        parent: Option<ObjectId>,
        ordinal: i64,
        name: Option<&str>,
        payload: &ObjectPayload,
    ) -> Result<ContentHash> {
        let bytes = payload.to_storage_bytes()?;
        let hash = ContentHash::of_bytes(&bytes);

        self.tx
            .execute(
                "INSERT INTO objects (id, kind, parent_id, ordinal, name, payload, payload_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                     kind = excluded.kind,
                     parent_id = excluded.parent_id,
                     ordinal = excluded.ordinal,
                     name = excluded.name,
                     payload = excluded.payload,
                     payload_hash = excluded.payload_hash",
                params![
                    id.to_bytes().as_slice(),
                    payload.type_name(),
                    parent.map(|p| p.to_bytes().to_vec()),
                    ordinal,
                    name,
                    bytes,
                    hash.as_bytes().as_slice(),
                ],
            )
            .map_err(|e| CadError::io(format!("writing object {id}"), e))?;

        Ok(hash)
    }

    /// Stores the exact bytes of a STEP file, reusing an identical source.
    ///
    /// The bytes are the source of truth; the scene stored beside them is one
    /// reading of them and can be redone. Content is the identity, so importing
    /// the same file twice costs one copy — and, for the same reason, bytes
    /// that differ by one character are a different source with a different
    /// identifier, never an edit of an existing one.
    ///
    /// A source nothing refers to does not survive the transaction that created
    /// it: reachability is what keeps bytes in a document, and
    /// [`put_imported_step`][Self::put_imported_step] is what establishes it.
    pub fn put_step_source(&mut self, bytes: &[u8]) -> Result<ImportedSourceId> {
        let hash = ContentHash::of_bytes(bytes);
        let byte_len = i64::try_from(bytes.len()).map_err(|_| {
            CadError::input(format!(
                "a source of {} bytes is beyond what this document addresses",
                bytes.len()
            ))
        })?;

        let existing: Option<(Vec<u8>, Vec<u8>, i64)> = self
            .tx
            .query_row(
                "SELECT id, bytes, byte_len FROM imported_sources
                 WHERE format = ?1 AND content_hash = ?2",
                params![STEP_SOURCE_FORMAT, hash.as_bytes().as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|e| CadError::io("looking for an identical imported source", e))?;
        if let Some((id, stored, stored_len)) = existing {
            if stored_len != byte_len || stored.as_slice() != bytes {
                return Err(CadError::input(
                    "an imported source has this content hash but different bytes or length; \
                     refusing to reuse a corrupt row or assume a hash collision is harmless",
                ));
            }
            return ImportedSourceId::from_slice(&id);
        }

        let id = ImportedSourceId::new();
        self.tx
            .execute(
                &format!(
                    "INSERT INTO imported_sources (id, format, bytes, content_hash, byte_len, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, {NOW_UTC})"
                ),
                params![
                    id.to_bytes().as_slice(),
                    STEP_SOURCE_FORMAT,
                    bytes,
                    hash.as_bytes().as_slice(),
                    byte_len,
                ],
            )
            .map_err(|e| CadError::io("writing imported source bytes", e))?;
        Ok(id)
    }

    /// Records an imported STEP object and its claim on the source bytes.
    ///
    /// Both rows are written here so neither can be forgotten: an object
    /// without the claim would have its bytes reclaimed at the end of this very
    /// transaction, and a claim without an object would keep bytes nothing can
    /// reach. The source must already exist — write it with
    /// [`put_step_source`][Self::put_step_source] first, inside the same
    /// [`Document::write`].
    ///
    /// Everything that can fail about an import — reading it, hashing it,
    /// projecting the scene, comparing it — has to have happened before the
    /// payload handed here could be constructed, which is what keeps a refused
    /// import from ever opening a transaction.
    pub fn put_imported_step(
        &mut self,
        id: ObjectId,
        parent: Option<ObjectId>,
        ordinal: i64,
        name: Option<&str>,
        imported: &ImportedStep,
    ) -> Result<ContentHash> {
        // The payload repeats the source hash and length so a swapped source
        // row can be detected later. Enforce the relationship on the way in as
        // well: the low-level writer is public and must not be able to create a
        // document that only discovers its own inconsistency on reopening.
        read_imported_source(self.tx, imported)?;

        let payload = ObjectPayload::ImportedStep(imported.clone());
        let hash = self.write_object(id, parent, ordinal, name, &payload)?;

        // Rewriting an object may point it at different bytes; the claim it
        // used to make must go with the reading it used to hold.
        self.tx
            .execute(
                "DELETE FROM imported_source_refs WHERE object_id = ?1",
                params![id.to_bytes().as_slice()],
            )
            .map_err(|e| CadError::io(format!("clearing source claims of object {id}"), e))?;
        self.tx
            .execute(
                "INSERT INTO imported_source_refs (object_id, source_id) VALUES (?1, ?2)",
                params![
                    id.to_bytes().as_slice(),
                    imported.source.to_bytes().as_slice(),
                ],
            )
            .map_err(|e| {
                CadError::io(
                    format!(
                        "recording that object {id} is built from source {}; it must be written \
                         first",
                        imported.source
                    ),
                    e,
                )
            })?;
        Ok(hash)
    }

    /// Removes an object and everything owned by it.
    ///
    /// Fails if another object still depends on it, rather than leaving a
    /// dependent pointing at nothing.
    pub fn remove_object(&mut self, id: ObjectId) -> Result<()> {
        self.tx
            .execute(
                "DELETE FROM objects WHERE id = ?1",
                params![id.to_bytes().as_slice()],
            )
            .map_err(|e| {
                CadError::io(
                    format!("removing object {id}; something may still depend on it"),
                    e,
                )
            })?;
        Ok(())
    }

    pub fn add_dependency(&mut self, dependency: Dependency) -> Result<()> {
        self.tx
            .execute(
                "INSERT INTO deps (dependent_id, dependency_id, role) VALUES (?1, ?2, ?3)
                 ON CONFLICT DO NOTHING",
                params![
                    dependency.dependent.to_bytes().as_slice(),
                    dependency.dependency.to_bytes().as_slice(),
                    dependency.role.as_str(),
                ],
            )
            .map_err(|e| CadError::io("adding dependency", e))?;
        Ok(())
    }

    pub fn remove_dependency(&mut self, dependency: Dependency) -> Result<()> {
        self.tx
            .execute(
                "DELETE FROM deps WHERE dependent_id = ?1 AND dependency_id = ?2 AND role = ?3",
                params![
                    dependency.dependent.to_bytes().as_slice(),
                    dependency.dependency.to_bytes().as_slice(),
                    dependency.role.as_str(),
                ],
            )
            .map_err(|e| CadError::io("removing dependency", e))?;
        Ok(())
    }
}

fn open_connection(path: &Path, flags: OpenFlags) -> Result<Connection> {
    Connection::open_with_flags(path, flags)
        .map_err(|e| CadError::io(format!("opening {}", path.display()), e))
}

/// Reads only the two SQLite header bytes that describe the journalling mode.
///
/// This has to happen before SQLite opens the connection. In WAL mode a
/// nominally read-only connection is allowed to create shared-memory files,
/// which would break the caller's promise to leave the directory untouched.
fn refuse_wal_journal(path: &Path) -> Result<()> {
    const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
    let mut file = std::fs::File::open(path)
        .map_err(|e| CadError::io(format!("opening {}", path.display()), e))?;
    let mut header = [0u8; 20];
    match file.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == IoErrorKind::UnexpectedEof => return Ok(()),
        Err(error) => {
            return Err(CadError::io(
                format!("reading {} header", path.display()),
                error,
            ));
        }
    }

    if &header[..SQLITE_MAGIC.len()] == SQLITE_MAGIC && (header[18] == 2 || header[19] == 2) {
        return Err(CadError::unsupported(format!(
            "{} uses SQLite WAL journalling; a read-only command will neither create auxiliary \
             files nor rewrite it to FerriteCAD's single-file DELETE mode",
            path.display()
        )));
    }
    Ok(())
}

/// SQLite can consult a stale WAL even with a rollback-mode database header,
/// and SQLITE_OPEN_READ_ONLY still permits writing shared-memory bookkeeping.
/// Check both the requested spelling and the resolved file: SQLite's VFS may
/// resolve a symlink before choosing the adjacent sidecar paths.
fn refuse_wal_sidecars(path: &Path) -> Result<()> {
    fn check(path: &Path) -> Result<()> {
        for suffix in ["-wal", "-shm"] {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            let sidecar = PathBuf::from(name);
            match std::fs::symlink_metadata(&sidecar) {
                Ok(_) => {
                    return Err(CadError::unsupported(format!(
                        "{} has a SQLite WAL sidecar {}; a read-only command will neither \
                         modify nor remove it",
                        path.display(),
                        sidecar.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(CadError::io(
                        format!("checking WAL sidecar {}", sidecar.display()),
                        error,
                    ));
                }
            }
        }
        Ok(())
    }
    check(path)?;
    let resolved = std::fs::canonicalize(path)
        .map_err(|error| CadError::io(format!("resolving {}", path.display()), error))?;
    if resolved != path {
        check(&resolved)?;
    }
    Ok(())
}

fn read_object_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<ObjectRecord>> {
    let id: Vec<u8> = row.get(0)?;
    let stored_kind: String = row.get(1)?;
    let parent: Option<Vec<u8>> = row.get(2)?;
    let ordinal: i64 = row.get(3)?;
    let name: Option<String> = row.get(4)?;
    let storage_bytes: Vec<u8> = row.get(5)?;
    let payload_hash: Vec<u8> = row.get(6)?;
    Ok((|| {
        let id = ObjectId::from_slice(&id)?;
        let payload = ObjectPayload::from_storage_bytes(&storage_bytes).map_err(|e| {
            CadError::input_because(format!("reading object {id} of type {stored_kind}"), e)
        })?;
        if payload.type_name() != stored_kind {
            return Err(CadError::input(format!(
                "object {id} of type {stored_kind} has envelope type {}",
                payload.type_name()
            )));
        }
        let payload_hash = ContentHash::from_slice(&payload_hash)?;
        if ContentHash::of_bytes(&storage_bytes) != payload_hash {
            return Err(CadError::input(format!(
                "object {id} of type {stored_kind} does not match its stored hash"
            )));
        }
        Ok(ObjectRecord {
            id,
            parent: parent.as_deref().map(ObjectId::from_slice).transpose()?,
            ordinal,
            name,
            payload,
            payload_hash,
            storage_bytes,
        })
    })())
}

fn read_meta(conn: &Connection) -> Result<DocumentMeta> {
    let (id, length, angle, created_at, modified_at, generator) = conn
        .query_row(
            "SELECT document_id, display_length_unit, display_angle_unit,
                    created_at, modified_at, generator
             FROM meta WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(|e| CadError::io("reading document metadata", e))?;
    let display_length_unit = length.parse()?;
    let display_angle_unit = angle.parse()?;
    validate_display_units(display_length_unit, display_angle_unit)?;
    Ok(DocumentMeta {
        document_id: DocumentId::from_slice(&id)?,
        display_length_unit,
        display_angle_unit,
        created_at,
        modified_at,
        generator,
    })
}

fn validate_display_units(length: Unit, angle: Unit) -> Result<()> {
    if length.dimension() != Dimension::Length {
        return Err(CadError::input(format!(
            "display length unit must measure length, found {length}"
        )));
    }
    if angle.dimension() != Dimension::Angle {
        return Err(CadError::input(format!(
            "display angle unit must measure angles, found {angle}"
        )));
    }
    Ok(())
}

/// Summarises diagnostics for an error message, keeping it to one line.
fn describe(diagnostics: &[ImportDiagnostic]) -> String {
    if diagnostics.is_empty() {
        return "it said nothing about why".to_owned();
    }
    diagnostics
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Drops source bytes nothing refers to any more, inside the same transaction.
///
/// Deleting an imported object cascades away its claim on a source; without
/// this the bytes would stay, reachable by nothing and removable by no command.
/// A source is only ever written together with the object that claims it, so
/// this cannot collect something a caller was about to use.
///
fn reclaim_imported_sources(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "DELETE FROM imported_sources
         WHERE id NOT IN (SELECT source_id FROM imported_source_refs)",
        [],
    )
    .map_err(|e| CadError::io("reclaiming unreferenced imported sources", e))?;
    Ok(())
}
