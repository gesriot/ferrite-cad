// SPDX-License-Identifier: MIT
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ferritecad_types::{
    CadError, ContentHash, Dimension, DocumentId, ObjectId, Result, StableEntityId, Unit,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};

use crate::envelope::Envelope;
use crate::graph::{Dependency, DependencyRole, evaluation_order};
use crate::model::{CORE_CAPABILITY, EntityKind, ObjectPayload, TopologyRef, TopologyRefPayload};
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

        let access = determine_access(&conn)?;
        Ok(Self {
            conn,
            path,
            meta,
            access,
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
            if envelope.required_capabilities != vec![CORE_CAPABILITY.to_owned()] {
                return Err(CadError::unsupported(format!(
                    "topology reference {id} requires unsupported capabilities: {}",
                    envelope.required_capabilities.join(", ")
                )));
            }
            let decoded: TopologyRefPayload = envelope.decode()?;
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
            vec![CORE_CAPABILITY.to_owned()],
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
            capabilities.extend(envelope.required_capabilities);
        }
    }
    Ok(capabilities)
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
