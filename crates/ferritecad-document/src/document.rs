// SPDX-License-Identifier: MIT
use std::collections::BTreeSet;
use std::io::{ErrorKind as IoErrorKind, Read};
use std::path::{Path, PathBuf};

use ferritecad_exchange::{Diagnostic as ImportDiagnostic, Import, Scene, StoredScene};
use ferritecad_kernel::{KernelIdentity, ShapeHandle};
use ferritecad_types::{
    CadError, ContentHash, Dimension, DocumentId, ImportedSourceId, ObjectId, Result,
    StableEntityId, Unit,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};

use crate::envelope::Envelope;
use crate::graph::{Dependency, DependencyRole, evaluation_order};
use crate::model::{
    CORE_CAPABILITY, EXTRUDE_CAP_EDGE_CAPABILITY, EntityKind, ImportedDefinitionRef, ImportedStep,
    ImporterIdentity, ObjectPayload, STEP_SOURCE_FORMAT, SemanticRole, TopologyRef,
    TopologyRefPayload,
};
use crate::schema::{
    self, CACHE_EXTENSION, DOCUMENT_APPLICATION_ID, FORMAT_VERSION, MINIMUM_READER_VERSION,
    SUPPORTED_CAPABILITIES,
};
use crate::validate::ValidationReport;

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
    pub format_version: u32,
    pub minimum_reader_version: u32,
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

/// A topology reference as stored, including its owner and producer.
pub type StoredTopologyRef = TopologyRef;

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
    /// The layout the stored scene was written at.
    ///
    /// Version 1 recorded no identities, so nothing it holds can answer a
    /// durable reference; see [`Self::resolve`].
    stored_version: u32,
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

    /// The stored scene layout whose identity contract this reading was bound
    /// under.
    ///
    /// Read-only because changing a legacy reading from v1 to v2 would make
    /// [`Self::resolve`] answer from keys observed now even though the document
    /// never recorded which key belonged to which definition.
    pub fn stored_version(&self) -> u32 {
        self.stored_version
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
    /// The three ways this fails are three different facts, and they are
    /// reported as three different kinds so a caller can tell them apart
    /// without reading prose:
    ///
    /// * [`Input`][ferritecad_types::ErrorKind::Input] — the reference is about
    ///   another source. It is not lost; it was never about this import.
    /// * [`Unsupported`][ferritecad_types::ErrorKind::Unsupported] — the stored
    ///   scene predates identities. The key is not missing; no key was ever
    ///   recorded, and inventing one from a position would answer a question
    ///   this document never agreed to.
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
        if self.stored_version < 2 {
            return Err(CadError::unsupported(format!(
                "the scene stored for source {} was written before definitions carried \
                 identities, so {reference} cannot be resolved. It is not lost; it was \
                 never recorded. Importing the file again gives this document \
                 identities it can answer for.",
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
        schema::migrate_document(&mut conn)?;

        let document_id = DocumentId::new();
        let generator = format!("FerriteCAD {}", env!("CARGO_PKG_VERSION"));

        let tx = conn
            .transaction()
            .map_err(|e| CadError::io("starting document initialisation", e))?;
        tx.execute(
            &format!(
                "INSERT INTO meta (
                     id, format_version, minimum_reader_version, document_id,
                     display_length_unit, display_angle_unit, created_at, modified_at, generator
                 ) VALUES (1, ?1, ?2, ?3, ?4, ?5, {NOW_UTC}, {NOW_UTC}, ?6)"
            ),
            params![
                FORMAT_VERSION,
                MINIMUM_READER_VERSION,
                document_id.to_bytes().as_slice(),
                display_length_unit.symbol(),
                display_angle_unit.symbol(),
                generator,
            ],
        )
        .map_err(|e| CadError::io("writing document metadata", e))?;
        tx.execute(
            "INSERT INTO capabilities (name, required) VALUES (?1, 1)",
            params![CORE_CAPABILITY],
        )
        .map_err(|e| CadError::io("recording required capabilities", e))?;
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

    /// Opens an existing document, migrating it if it predates this build.
    ///
    /// Opening never fails because of a capability this build lacks; it
    /// downgrades to read-only instead, so a user can always look at their
    /// model even when this build cannot safely rewrite it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut conn = open_connection(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        // Do not change journal mode or any other persistent pragma until we
        // know this is our file. Opening a foreign SQLite database must be a
        // read-only inspection from our application's point of view.
        schema::check_application_id(&conn, DOCUMENT_APPLICATION_ID, "document")?;
        schema::configure_document_connection(&conn)?;
        schema::migrate_document(&mut conn)?;

        let meta = read_meta(&conn)?;
        require_supported_format(&path, &meta)?;

        let access = determine_access(&conn)?;
        Ok(Self {
            conn,
            path,
            meta,
            access,
        })
    }

    /// Opens an existing, current-schema document without modifying it.
    ///
    /// Unlike [`Self::open`], this path neither changes persistent pragmas nor
    /// runs migrations. An older schema is refused with an actionable error:
    /// silently migrating would violate the read-only contract, while reading
    /// it as if the new columns already existed would be misleading. A file in
    /// SQLite WAL mode is likewise refused before SQLite opens it: FerriteCAD
    /// documents are single-file rollback-journal databases, and even a
    /// read-only WAL connection may create `-wal`/`-shm` files.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        refuse_wal_journal(&path)?;
        let conn = open_connection(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        schema::check_application_id(&conn, DOCUMENT_APPLICATION_ID, "document")?;
        schema::require_current_document_schema(&conn)?;

        let meta = read_meta(&conn)?;
        require_supported_format(&path, &meta)?;
        Ok(Self {
            conn,
            path,
            meta,
            access: Access::ReadOnly {
                reason: "it was explicitly opened without write access".to_owned(),
            },
        })
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
            .transaction()
            .map_err(|e| CadError::io("starting document edit", e))?;

        let outcome = {
            let mut writer = DocumentWriter { tx: &tx };
            edit(&mut writer)
        };

        match outcome {
            Ok(value) => {
                reclaim_imported_sources(&tx)?;
                rebuild_capabilities(&tx)?;
                tx.execute(
                    &format!("UPDATE meta SET modified_at = {NOW_UTC} WHERE id = 1"),
                    [],
                )
                .map_err(|e| CadError::io("stamping modification time", e))?;
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
                "SELECT id, kind, schema_version, parent_id, ordinal, name, payload, payload_hash
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
                "SELECT id, kind, schema_version, parent_id, ordinal, name, payload, payload_hash
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

    pub fn topology_refs(&self) -> Result<Vec<StoredTopologyRef>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, owner_id, producer_feature, expected_kind, payload, payload_hash
                 FROM topology_refs ORDER BY owner_id, id",
            )
            .map_err(|e| CadError::io("preparing topology reference query", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Option<Vec<u8>>>(5)?,
                ))
            })
            .map_err(|e| CadError::io("reading topology references", e))?;

        let mut refs = Vec::new();
        for row in rows {
            let (id, owner, producer, kind, payload, payload_hash) =
                row.map_err(|e| CadError::io("reading topology reference row", e))?;
            let id = StableEntityId::from_slice(&id)?;
            if let Some(payload_hash) = payload_hash {
                let payload_hash = ContentHash::from_slice(&payload_hash)?;
                if ContentHash::of_bytes(&payload) != payload_hash {
                    return Err(CadError::input(format!(
                        "topology reference {id} does not match its stored hash"
                    )));
                }
            }
            let envelope = Envelope::from_bytes(&payload)?;
            if envelope.type_name != "topology_ref" || envelope.schema_version != 1 {
                return Err(CadError::unsupported(format!(
                    "topology reference {id} has unsupported {} schema v{}",
                    envelope.type_name, envelope.schema_version
                )));
            }
            // Every capability the payload declares must be one this build
            // implements. A reference that asks for something newer is
            // refused by name here and makes the whole document read-only in
            // `determine_access`, instead of arriving as a role nothing can
            // parse.
            let unsupported: Vec<&str> = envelope
                .required_capabilities
                .iter()
                .map(String::as_str)
                .filter(|name| !SUPPORTED_CAPABILITIES.contains(name))
                .collect();
            if !unsupported.is_empty() {
                return Err(CadError::unsupported(format!(
                    "topology reference {id} requires unsupported capabilities: {}",
                    unsupported.join(", ")
                )));
            }
            let decoded: TopologyRefPayload = envelope.decode()?;
            require_role_capabilities(&envelope, &decoded.output_role)?;
            let reference = TopologyRef {
                id,
                owner: ObjectId::from_slice(&owner)?,
                producer_feature: ObjectId::from_slice(&producer)?,
                expected_kind: EntityKind::parse(&kind)?,
                output_role: decoded.output_role,
                selection: decoded.selection,
                fallback_signature: decoded.fallback_signature,
            };
            reference.validate()?;
            refs.push(reference);
        }
        Ok(refs)
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
                scene: StoredScene::V2(persisted),
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
        Ok(ReopenedStepImport {
            scene,
            source: stored.imported.source,
            stored_version: stored.imported.scene.version(),
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

    /// Checks everything that must hold for this document to be rebuildable.
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

        if !matches!(payload, ObjectPayload::Unknown(_)) {
            // Replacing an imported object with a known ordinary one also
            // replaces its ownership contract. Leaving the old claim behind
            // would keep a BLOB that no object can reach forever. An unknown
            // payload is different: this build cannot prove it does not own
            // the source, so forward-compatible preservation keeps its claim.
            self.tx
                .execute(
                    "DELETE FROM imported_source_refs WHERE object_id = ?1",
                    params![id.to_bytes().as_slice()],
                )
                .map_err(|e| CadError::io(format!("clearing source claims of object {id}"), e))?;
        }
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
                "INSERT INTO objects (id, kind, schema_version, parent_id, ordinal, name, payload, payload_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                     kind = excluded.kind,
                     schema_version = excluded.schema_version,
                     parent_id = excluded.parent_id,
                     ordinal = excluded.ordinal,
                     name = excluded.name,
                     payload = excluded.payload,
                     payload_hash = excluded.payload_hash",
                params![
                    id.to_bytes().as_slice(),
                    payload.type_name(),
                    payload.schema_version(),
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

    pub fn put_topology_ref(&mut self, reference: &TopologyRef) -> Result<()> {
        reference.validate()?;
        let payload = Envelope::encode(
            "topology_ref",
            1,
            required_capabilities_of(&reference.output_role),
            &TopologyRefPayload {
                output_role: reference.output_role.clone(),
                selection: reference.selection.clone(),
                fallback_signature: reference.fallback_signature.clone(),
            },
        )?
        .to_bytes()?;
        let payload_hash = ContentHash::of_bytes(&payload);

        self.tx
            .execute(
                "INSERT INTO topology_refs (
                     id, owner_id, producer_feature, expected_kind, payload, payload_hash
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                     owner_id = excluded.owner_id,
                     producer_feature = excluded.producer_feature,
                     expected_kind = excluded.expected_kind,
                     payload = excluded.payload,
                     payload_hash = excluded.payload_hash",
                params![
                    reference.id.to_bytes().as_slice(),
                    reference.owner.to_bytes().as_slice(),
                    reference.producer_feature.to_bytes().as_slice(),
                    reference.expected_kind.as_str(),
                    payload,
                    payload_hash.as_bytes().as_slice(),
                ],
            )
            .map_err(|e| CadError::io(format!("writing topology reference {}", reference.id), e))?;
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

fn require_supported_format(path: &Path, meta: &DocumentMeta) -> Result<()> {
    if meta.format_version > FORMAT_VERSION {
        return Err(CadError::unsupported(format!(
            "{} uses document format v{}, this build writes v{}",
            path.display(),
            meta.format_version,
            FORMAT_VERSION
        )));
    }
    if meta.minimum_reader_version > FORMAT_VERSION {
        return Err(CadError::unsupported(format!(
            "{} needs a reader for format v{} or newer; this build writes v{}",
            path.display(),
            meta.minimum_reader_version,
            FORMAT_VERSION
        )));
    }
    Ok(())
}

fn read_object_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<ObjectRecord>> {
    let id: Vec<u8> = row.get(0)?;
    let stored_kind: String = row.get(1)?;
    let stored_schema_version: u32 = row.get(2)?;
    let parent: Option<Vec<u8>> = row.get(3)?;
    let ordinal: i64 = row.get(4)?;
    let name: Option<String> = row.get(5)?;
    let storage_bytes: Vec<u8> = row.get(6)?;
    let payload_hash: Vec<u8> = row.get(7)?;

    Ok((|| {
        let payload = ObjectPayload::from_storage_bytes(&storage_bytes)?;
        if payload.type_name() != stored_kind || payload.schema_version() != stored_schema_version {
            return Err(CadError::input(format!(
                "object metadata disagrees with its envelope: column is {} v{}, envelope is {} v{}",
                stored_kind,
                stored_schema_version,
                payload.type_name(),
                payload.schema_version()
            )));
        }
        Ok(ObjectRecord {
            id: ObjectId::from_slice(&id)?,
            parent: parent.as_deref().map(ObjectId::from_slice).transpose()?,
            ordinal,
            name,
            payload,
            payload_hash: ContentHash::from_slice(&payload_hash)?,
            storage_bytes,
        })
    })())
}

fn read_meta(conn: &Connection) -> Result<DocumentMeta> {
    conn.query_row(
        "SELECT format_version, minimum_reader_version, document_id,
                display_length_unit, display_angle_unit, created_at, modified_at, generator
         FROM meta WHERE id = 1",
        [],
        |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )
    .map_err(|e| CadError::io("reading document metadata", e))
    .and_then(
        |(
            format_version,
            minimum_reader_version,
            document_id,
            length_unit,
            angle_unit,
            created_at,
            modified_at,
            generator,
        )| {
            let display_length_unit: Unit = length_unit.parse()?;
            let display_angle_unit: Unit = angle_unit.parse()?;
            validate_display_units(display_length_unit, display_angle_unit)?;
            Ok(DocumentMeta {
                document_id: DocumentId::from_slice(&document_id)?,
                format_version,
                minimum_reader_version,
                display_length_unit,
                display_angle_unit,
                created_at,
                modified_at,
                generator,
            })
        },
    )
}

/// What a reader must implement before it may read one stored role.
///
/// The core capability always, and the cap-edge one only for the role that
/// needs it. A document that names no such edge therefore declares exactly
/// what it declared before this build existed, and stays writable by a reader
/// that predates it.
fn required_capabilities_of(role: &SemanticRole) -> Vec<String> {
    let mut names = vec![CORE_CAPABILITY.to_owned()];
    if matches!(role, SemanticRole::ExtrudeCapEdge { .. }) {
        names.push(EXTRUDE_CAP_EDGE_CAPABILITY.to_owned());
    }
    names
}

/// Refuses a known role whose envelope omits part of its capability contract.
///
/// Checking only what an envelope *does* declare is not enough: a damaged or
/// hand-edited cap-edge reference could otherwise omit its new capability and
/// make an older reader believe the document was safe to rewrite. Extra
/// supported capabilities remain conservative and are therefore allowed.
fn require_role_capabilities(envelope: &Envelope, role: &SemanticRole) -> Result<()> {
    let missing: Vec<String> = required_capabilities_of(role)
        .into_iter()
        .filter(|required| !envelope.required_capabilities.contains(required))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(CadError::input(format!(
            "{} schema v{} omits required capabilities: {}",
            envelope.type_name,
            envelope.schema_version,
            missing.join(", ")
        )))
    }
}

/// Decides whether this build may write the document it just opened.
fn determine_access(conn: &Connection) -> Result<Access> {
    let mut missing: Vec<String> = declared_capabilities(conn)?
        .into_iter()
        .filter(|name| !SUPPORTED_CAPABILITIES.contains(&name.as_str()))
        .collect();
    missing.sort();

    if missing.is_empty() {
        Ok(Access::ReadWrite)
    } else {
        Ok(Access::ReadOnly {
            reason: format!(
                "it requires {}, which this build does not implement",
                missing.join(", ")
            ),
        })
    }
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

/// Returns the capability contract declared by the actual envelopes, not just
/// the denormalised index table. A hand-edited or damaged `capabilities` table
/// must never trick an older build into rewriting a future object.
fn declared_capabilities(conn: &Connection) -> Result<BTreeSet<String>> {
    let mut capabilities = BTreeSet::new();
    for (table, context) in [
        ("objects", "object"),
        ("topology_refs", "topology reference"),
    ] {
        let mut stmt = conn
            .prepare(&format!("SELECT payload FROM {table}"))
            .map_err(|e| CadError::io(format!("preparing {context} capability query"), e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(|e| CadError::io(format!("reading {context} capabilities"), e))?;
        for row in rows {
            let bytes = row.map_err(|e| CadError::io(format!("reading {context} payload"), e))?;
            let envelope = Envelope::from_bytes(&bytes)?;
            let all_supported = envelope
                .required_capabilities
                .iter()
                .all(|name| SUPPORTED_CAPABILITIES.contains(&name.as_str()));
            capabilities.extend(envelope.required_capabilities.iter().cloned());

            // A future topology-reference role must still be allowed to open
            // read-only without this build trying to decode it. A known,
            // wholly supported payload has no such excuse: its declared
            // capabilities must cover the role actually stored inside it.
            if table == "topology_refs"
                && envelope.type_name == "topology_ref"
                && envelope.schema_version == 1
                && all_supported
            {
                let decoded: TopologyRefPayload = envelope.decode()?;
                require_role_capabilities(&envelope, &decoded.output_role)?;
            }
        }
    }
    Ok(capabilities)
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
/// It runs on the successful path of every edit, and only there. An object with
/// an unsupported capability makes the document read-only. A newer layout with
/// a capability this build already knows may remain writable, but its explicit
/// `imported_source_refs` row is preserved with the unknown envelope and keeps
/// the source reachable even though this build cannot inspect its payload.
fn reclaim_imported_sources(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "DELETE FROM imported_sources
         WHERE id NOT IN (SELECT source_id FROM imported_source_refs)",
        [],
    )
    .map_err(|e| CadError::io("reclaiming unreferenced imported sources", e))?;
    Ok(())
}

/// Rebuilds the denormalised capabilities index after a successful edit.
///
/// Removing an unknown object must remove its capability too; incrementally
/// inserting rows only makes a document read-only forever after such removal.
fn rebuild_capabilities(tx: &Transaction<'_>) -> Result<()> {
    let capabilities = declared_capabilities(tx)?;
    tx.execute("DELETE FROM capabilities", [])
        .map_err(|e| CadError::io("clearing capability index", e))?;
    tx.execute(
        "INSERT INTO capabilities (name, required) VALUES (?1, 1)",
        params![CORE_CAPABILITY],
    )
    .map_err(|e| CadError::io("recording core capability", e))?;
    for capability in capabilities {
        tx.execute(
            "INSERT INTO capabilities (name, required) VALUES (?1, 1)
             ON CONFLICT(name) DO NOTHING",
            params![capability],
        )
        .map_err(|e| CadError::io("recording required capability", e))?;
    }
    Ok(())
}
