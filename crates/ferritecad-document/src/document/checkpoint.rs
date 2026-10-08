// SPDX-License-Identifier: MIT
//! §30N: named checkpoints, stored in the document they belong to.
//!
//! A checkpoint is a row of the `checkpoints` table (schema v4): its identity,
//! the name a person gave it, when it was made, and `image` — a complete
//! document file holding the model of that moment, whose own catalog is empty.
//! The image is made by the SQLite online backup every copy already uses and
//! compacted with `VACUUM INTO`, which keeps row identities; nothing in it is
//! decoded or re-serialised, so unknown tables and payloads survive as they are.
//!
//! The catalog belongs to the working document. Rows are inserted and deleted,
//! never updated: a stored checkpoint is immutable. Restoring takes the model out
//! of an image and keeps the catalog it is restored into (see
//! [`Document::replace_checkpoints_from`]).
//!
//! Listing reads no image. Writing and reading an image stream it through
//! SQLite's incremental blob I/O, so neither holds a whole model in memory.

use std::io::{Read, Write};
use std::path::Path;

use ferritecad_types::{CadError, CheckpointId, ContentHash, Result};
use rusqlite::{OpenFlags, OptionalExtension, params};

use super::{Access, Document, NOW_UTC, open_connection};
use crate::schema;

/// The most checkpoints one document holds. A Create past it is refused; none
/// is removed to make room.
pub const MAX_CHECKPOINTS: usize = 32;

/// The most image bytes one document holds in checkpoints, all of them together.
/// Every private version of an open document carries the catalog, so this also
/// bounds what each Undo step and each crash copy costs.
pub const MAX_CHECKPOINT_BYTES: u64 = 16 * 1024 * 1024;

/// The longest name, in characters.
pub const MAX_CHECKPOINT_NAME_CHARS: usize = 80;

/// One checkpoint as listed: everything but the image.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckpointEntry {
    pub id: CheckpointId,
    pub name: String,
    /// When it was made: SQLite's UTC clock, `YYYY-MM-DDTHH:MM:SS.SSSZ`.
    pub created_at: String,
    /// The image's length in bytes.
    pub bytes: u64,
    /// [`Document::model_without_checkpoints`] of the image: equal to that of
    /// every version whose model (catalog aside) is this checkpoint's.
    pub model: ContentHash,
}

/// The name a checkpoint is stored under: `name` without surrounding whitespace,
/// 1 to [`MAX_CHECKPOINT_NAME_CHARS`] characters, no control characters. Equal
/// names are allowed; the identity is the UUID.
pub fn checkpoint_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CadError::input("a checkpoint needs a name"));
    }
    let length = trimmed.chars().count();
    if length > MAX_CHECKPOINT_NAME_CHARS {
        return Err(CadError::input(format!(
            "a checkpoint name has at most {MAX_CHECKPOINT_NAME_CHARS} characters; this one has \
             {length}"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(CadError::input(
            "a checkpoint name cannot contain control characters (tabs, line breaks)",
        ));
    }
    Ok(trimmed.to_owned())
}

const LIST: &str =
    "SELECT rowid, id, name, created_at, model, byte_len FROM checkpoints ORDER BY created_at, id";

fn damaged(what: impl std::fmt::Display) -> CadError {
    CadError::input(format!("the document's checkpoint list is damaged: {what}"))
}

/// Reads one listed row, refusing anything this build did not write.
fn entry(row: &rusqlite::Row<'_>) -> Result<(i64, CheckpointEntry)> {
    let sql = |e| CadError::io("reading a checkpoint", e);
    let rowid: i64 = row.get(0).map_err(sql)?;
    let id: Vec<u8> = row.get(1).map_err(sql)?;
    let id = CheckpointId::from_slice(&id).map_err(|e| damaged(format!("row {rowid}: {e}")))?;
    let stored: String = row.get(2).map_err(sql)?;
    let name = checkpoint_name(&stored).map_err(|e| damaged(format!("{id}: {e}")))?;
    if name != stored {
        return Err(damaged(format!("{id}: its name is not trimmed")));
    }
    let model: Vec<u8> = row.get(4).map_err(sql)?;
    let bytes: i64 = row.get(5).map_err(sql)?;
    Ok((
        rowid,
        CheckpointEntry {
            id,
            name,
            created_at: row.get(3).map_err(sql)?,
            bytes: u64::try_from(bytes)
                .ok()
                .filter(|bytes| *bytes > 0)
                .ok_or_else(|| damaged(format!("{id}: length {bytes}")))?,
            model: ContentHash::from_slice(&model).map_err(|e| damaged(format!("{id}: {e}")))?,
        },
    ))
}

/// Writes everything `reader` yields to `writer` and returns the BLAKE3 of it
/// and how many bytes there were, with bounded buffering.
fn copy_hashed(reader: impl Read, writer: &mut impl Write) -> std::io::Result<(ContentHash, u64)> {
    struct Tee<'a, R, W> {
        reader: R,
        writer: &'a mut W,
        count: u64,
    }
    impl<R: Read, W: Write> Read for Tee<'_, R, W> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.reader.read(buffer)?;
            self.writer.write_all(&buffer[..read])?;
            self.count += read as u64;
            Ok(read)
        }
    }
    let mut tee = Tee {
        reader,
        writer,
        count: 0,
    };
    let hash = ContentHash::of_reader(&mut tee)?;
    Ok((hash, tee.count))
}

/// The name SQLite gives one of a database file's companions.
fn companion(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

impl Document {
    /// The checkpoints of this version, oldest first, without reading an image.
    /// A schema v3 document has none. A row this build would not have written is
    /// a refusal of the whole list, never a silently shorter one.
    pub fn checkpoints(&self) -> Result<Vec<CheckpointEntry>> {
        Ok(self.listed()?.into_iter().map(|(_, entry)| entry).collect())
    }

    fn listed(&self) -> Result<Vec<(i64, CheckpointEntry)>> {
        if !schema::has_checkpoint_table(&self.conn)? {
            return Ok(Vec::new());
        }
        let sql = |e| CadError::io("listing checkpoints", e);
        let mut statement = self.conn.prepare(LIST).map_err(sql)?;
        let mut rows = statement.query([]).map_err(sql)?;
        let mut listed = Vec::new();
        let mut bytes = 0_u64;
        while let Some(row) = rows.next().map_err(sql)? {
            let item = entry(row)?;
            bytes = bytes.saturating_add(item.1.bytes);
            if listed.len() == MAX_CHECKPOINTS || bytes > MAX_CHECKPOINT_BYTES {
                return Err(damaged(
                    "the catalog exceeds the checkpoint count or byte limit",
                ));
            }
            listed.push(item);
        }
        Ok(listed)
    }

    fn listed_one(&self, id: CheckpointId) -> Result<(i64, CheckpointEntry)> {
        self.listed()?
            .into_iter()
            .find(|(_, entry)| entry.id == id)
            .ok_or_else(|| CadError::input(format!("this document has no checkpoint {id}")))
    }

    fn require_writable(&self) -> Result<()> {
        if let Access::ReadOnly { reason } = &self.access {
            return Err(CadError::unsupported(format!(
                "{} is open read-only: {reason}",
                self.path.display()
            )));
        }
        if !schema::has_checkpoint_table(&self.conn)? {
            return Err(CadError::unsupported(
                "this document's schema has no checkpoint table; open it for writing to migrate it",
            ));
        }
        Ok(())
    }

    /// Writes the image of this version's model to `image`: the version without
    /// its checkpoints, compacted. `scratch` is a working copy that is removed
    /// again. Both paths must be absent and in storage the caller owns.
    ///
    /// The image is checked before this returns: same document, no checkpoints,
    /// and [`Self::model_without_checkpoints`] equal to this version's. Returns
    /// that value.
    pub fn write_checkpoint_image(&self, scratch: &Path, image: &Path) -> Result<ContentHash> {
        if !schema::has_checkpoint_table(&self.conn)? {
            return Err(CadError::unsupported(
                "this document's schema has no checkpoint table; open it for writing to migrate it",
            ));
        }
        let expected = self.model_without_checkpoints()?;
        let target = image
            .to_str()
            .ok_or_else(|| CadError::input("a checkpoint image needs a UTF-8 working path"))?;
        let mut owns_scratch = false;
        let mut owns_image = false;
        let made = (|| {
            for (path, owned) in [(scratch, &mut owns_scratch), (image, &mut owns_image)] {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|e| CadError::io("reserving a checkpoint image", e))?;
                *owned = true;
            }
            self.snapshot_into_reserved(scratch)?;
            let copy = open_connection(scratch, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            schema::configure_document_connection(&copy)?;
            let sql = |e| CadError::io("writing a checkpoint image", e);
            copy.execute("DELETE FROM checkpoints", []).map_err(sql)?;
            // Compaction that keeps row identities (SQLite's transfer copy under
            // VACUUM INTO). The path is bound, never spliced into SQL text.
            copy.execute("VACUUM INTO ?1", [target]).map_err(sql)?;
            copy.close()
                .map_err(|(_, e)| CadError::io("closing a checkpoint working copy", e))?;
            let written = Document::open_read_only(image)?;
            let found = (
                written.meta().document_id,
                written.checkpoints()?.is_empty(),
                written.model_without_checkpoints()?,
            );
            written.close()?;
            if found != (self.meta.document_id, true, expected) {
                return Err(CadError::io(
                    "writing a checkpoint image",
                    "the image is not the model it was made from",
                ));
            }
            Ok(expected)
        })();
        if owns_scratch {
            let _ = std::fs::remove_file(scratch);
            let _ = std::fs::remove_file(companion(scratch, "-journal"));
        }
        if owns_image && made.is_err() {
            let _ = std::fs::remove_file(image);
            let _ = std::fs::remove_file(companion(image, "-journal"));
        }
        made
    }

    /// Adds a checkpoint called `name` whose image is the file at `image` (made
    /// by [`Self::write_checkpoint_image`] from this version). One transaction:
    /// the limits are read and the row is written on the same reading.
    ///
    /// # Errors
    ///
    /// A bad name, a full catalog ([`MAX_CHECKPOINTS`], [`MAX_CHECKPOINT_BYTES`]),
    /// an image of another document or one that holds checkpoints itself. Nothing
    /// is written then, and nothing is ever removed to make room.
    pub fn add_checkpoint(&mut self, name: &str, image: &Path) -> Result<CheckpointEntry> {
        let name = checkpoint_name(name)?;
        self.require_writable()?;
        let model = {
            let candidate = Document::open_read_only(image)?;
            let facts = (
                candidate.meta().document_id,
                candidate.checkpoints()?.is_empty(),
                candidate.model_without_checkpoints()?,
            );
            candidate.close()?;
            if facts.0 != self.meta.document_id || !facts.1 {
                return Err(CadError::input(
                    "a checkpoint image must be this document's model without checkpoints",
                ));
            }
            facts.2
        };
        let mut file = std::fs::File::open(image)
            .map_err(|e| CadError::io("reading a checkpoint image", e))?;
        let bytes = file
            .metadata()
            .map_err(|e| CadError::io("measuring a checkpoint image", e))?
            .len();
        let sql = |e| CadError::io("writing a checkpoint", e);
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        if model != self.model_without_checkpoints()? {
            return Err(CadError::input(
                "the model changed after the checkpoint image was prepared",
            ));
        }
        let existing = self.listed()?;
        if existing.len() >= MAX_CHECKPOINTS {
            return Err(CadError::input(format!(
                "this document already has {MAX_CHECKPOINTS} checkpoints, the most it keeps; \
                 delete one before adding another"
            )));
        }
        let held: u64 = existing.iter().map(|(_, entry)| entry.bytes).sum();
        if held.saturating_add(bytes) > MAX_CHECKPOINT_BYTES {
            return Err(CadError::input(format!(
                "this checkpoint needs {bytes} bytes and the document's checkpoints already hold \
                 {held} of at most {MAX_CHECKPOINT_BYTES}; delete one before adding another"
            )));
        }
        let length =
            i64::try_from(bytes).map_err(|_| CadError::input("checkpoint image too large"))?;
        let id = CheckpointId::new();
        tx.execute(
            &format!(
                "INSERT INTO checkpoints (id, name, created_at, model, byte_len, image_hash, image)
                 VALUES (?1, ?2, {NOW_UTC}, ?3, ?4, zeroblob(32), zeroblob(?4))"
            ),
            params![
                id.to_bytes().as_slice(),
                name,
                model.as_bytes().as_slice(),
                length
            ],
        )
        .map_err(sql)?;
        let rowid = tx.last_insert_rowid();
        let hash = {
            let mut blob = tx
                .blob_open(rusqlite::MAIN_DB, "checkpoints", "image", rowid, false)
                .map_err(sql)?;
            let (hash, copied) = copy_hashed(&mut file, &mut blob)
                .map_err(|e| CadError::io("storing a checkpoint image", e))?;
            if copied != bytes {
                return Err(CadError::io(
                    "storing a checkpoint image",
                    "the image changed length while it was copied",
                ));
            }
            hash
        };
        tx.execute(
            "UPDATE checkpoints SET image_hash = ?1 WHERE rowid = ?2",
            params![hash.as_bytes().as_slice(), rowid],
        )
        .map_err(sql)?;
        tx.execute(
            &format!("UPDATE meta SET modified_at = {NOW_UTC} WHERE id = 1"),
            [],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        self.listed_one(id).map(|(_, entry)| entry)
    }

    /// Removes one checkpoint. Nothing else in the document changes. The pages
    /// its image occupied are zeroed, not merely freed: a deleted checkpoint's
    /// model does not linger in the file (they are reused by the next one).
    pub fn remove_checkpoint(&mut self, id: CheckpointId) -> Result<CheckpointEntry> {
        self.require_writable()?;
        let (rowid, entry) = self.listed_one(id)?;
        let sql = |e| CadError::io("deleting a checkpoint", e);
        self.conn
            .pragma_update(None, "secure_delete", "ON")
            .map_err(sql)?;
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        tx.execute("DELETE FROM checkpoints WHERE rowid = ?1", [rowid])
            .map_err(sql)?;
        tx.execute(
            &format!("UPDATE meta SET modified_at = {NOW_UTC} WHERE id = 1"),
            [],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(entry)
    }

    /// Writes one checkpoint's image to `destination` (absent, the caller's),
    /// streaming it, and checks it before returning: length and BLAKE3 as
    /// stored, a FerriteCAD document of this document's id, no checkpoints, and
    /// the model recorded for it. On any failure `destination` is removed.
    pub fn extract_checkpoint_image(
        &self,
        id: CheckpointId,
        destination: &Path,
    ) -> Result<CheckpointEntry> {
        let (rowid, entry) = self.listed_one(id)?;
        let mut owns_destination = false;
        let outcome = (|| {
            let sql = |e| CadError::io("reading a checkpoint image", e);
            let stored: Vec<u8> = self
                .conn
                .query_row(
                    "SELECT image_hash FROM checkpoints WHERE rowid = ?1",
                    [rowid],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql)?
                .ok_or_else(|| CadError::input(format!("this document has no checkpoint {id}")))?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|e| CadError::io("reserving a checkpoint copy", e))?;
            owns_destination = true;
            let blob = self
                .conn
                .blob_open(rusqlite::MAIN_DB, "checkpoints", "image", rowid, true)
                .map_err(sql)?;
            let (hash, length) = copy_hashed(blob, &mut file)
                .map_err(|e| CadError::io("copying a checkpoint image", e))?;
            file.sync_all()
                .map_err(|e| CadError::io("flushing a checkpoint copy", e))?;
            drop(file);
            if length != entry.bytes || hash.as_bytes().as_slice() != stored.as_slice() {
                return Err(damaged(format!(
                    "{id}: its image does not have the length and hash stored with it"
                )));
            }
            let image = Document::open_read_only(destination)
                .map_err(|e| damaged(format!("{id}: its image is not a document: {e}")))?;
            let facts = (
                image.meta().document_id,
                image.checkpoints()?.is_empty(),
                image.model_without_checkpoints()?,
            );
            image.close()?;
            if facts != (self.meta.document_id, true, entry.model) {
                return Err(damaged(format!(
                    "{id}: its image is not this document's model as it was recorded"
                )));
            }
            Ok(())
        })();
        if owns_destination && outcome.is_err() {
            let _ = std::fs::remove_file(destination);
        }
        outcome.map(|()| entry)
    }

    /// Makes this document's catalog exactly `from`'s, row identities included:
    /// what Restore does to the model it brings back, so that the list a person
    /// keeps is never replaced by an older one. `from` is any readable version of
    /// the same document; a schema v3 one has no checkpoints.
    pub fn replace_checkpoints_from(&mut self, from: &Document) -> Result<()> {
        self.require_writable()?;
        if from.meta.document_id != self.meta.document_id {
            return Err(CadError::input(
                "checkpoints belong to one document and cannot be moved to another",
            ));
        }
        let rows = from.listed()?;
        let sql = |e| CadError::io("copying the checkpoint list", e);
        self.conn
            .pragma_update(None, "secure_delete", "ON")
            .map_err(sql)?;
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        tx.execute("DELETE FROM checkpoints", []).map_err(sql)?;
        for (rowid, entry) in &rows {
            let (hash, length): (Vec<u8>, i64) = from
                .conn
                .query_row(
                    "SELECT image_hash, byte_len FROM checkpoints WHERE rowid = ?1",
                    [rowid],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(sql)?;
            tx.execute(
                "INSERT INTO checkpoints
                     (rowid, id, name, created_at, model, byte_len, image_hash, image)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, zeroblob(?6))",
                params![
                    rowid,
                    entry.id.to_bytes().as_slice(),
                    entry.name,
                    entry.created_at,
                    entry.model.as_bytes().as_slice(),
                    length,
                    hash
                ],
            )
            .map_err(sql)?;
            let mut source = from
                .conn
                .blob_open(rusqlite::MAIN_DB, "checkpoints", "image", *rowid, true)
                .map_err(sql)?;
            let mut target = tx
                .blob_open(rusqlite::MAIN_DB, "checkpoints", "image", *rowid, false)
                .map_err(sql)?;
            // Restore uses one verified image, but preserves the other rows
            // verbatim. An unrelated damaged image must not block this Restore.
            let copied = std::io::copy(&mut source, &mut target)
                .map_err(|e| CadError::io("copying a checkpoint image", e))?;
            if copied != entry.bytes {
                return Err(damaged(format!(
                    "{}: its image does not have the length stored with it",
                    entry.id
                )));
            }
        }
        tx.execute(
            &format!("UPDATE meta SET modified_at = {NOW_UTC} WHERE id = 1"),
            [],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_trimmed_bounded_and_printable_and_need_not_be_unique() {
        assert_eq!(
            checkpoint_name("  Before cut \u{00e9} ").expect("valid"),
            "Before cut \u{00e9}"
        );
        assert_eq!(
            checkpoint_name(&"я".repeat(80))
                .expect("80 characters")
                .chars()
                .count(),
            80
        );
        for refused in ["", "   ", "a\tb", "line\nbreak", &"x".repeat(81)] {
            assert!(checkpoint_name(refused).is_err(), "{refused:?}");
        }
    }
}
