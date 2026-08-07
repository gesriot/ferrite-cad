//! The regenerable cache sidecar.
//!
//! Everything here can be deleted at any moment without changing what a
//! document means. That is why it is a separate file rather than a set of
//! tables inside the document: the invariant is enforced by the filesystem,
//! it is testable in one line, and the two files can be journalled differently
//! — the document conservatively so it stays a single portable file, the cache
//! for speed.
//!
//! A cache entry is addressed by `(object, cache key, representation kind)`,
//! where the key already folds in the algorithm version, the tolerances and
//! the kernel identity. A key that matches is an optimisation; a key that does
//! not simply rebuilds.

use std::path::{Path, PathBuf};

use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::schema::{self, CACHE_APPLICATION_ID};

/// Chunk size for stored blobs.
///
/// Large B-Rep payloads are split so a single row never has to be materialised
/// whole, and so a partially useful blob can be streamed later.
const CHUNK_BYTES: usize = 1 << 20;

/// What a cache entry holds.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry {
    pub hash: ContentHash,
    /// What sort of derived data this is, e.g. `"brep"` or `"tessellation"`.
    pub kind: String,
    pub bytes: Vec<u8>,
}

/// A cache sidecar bound to exactly one document and one kernel build.
#[derive(Debug)]
pub struct CacheStore {
    conn: Connection,
    path: PathBuf,
}

impl CacheStore {
    /// Opens the sidecar for `document_id`, rebuilding it from scratch if what
    /// is there belongs to a different document, format or kernel.
    ///
    /// A stale sidecar is discarded rather than migrated or repaired. Repairing
    /// derived data risks keeping an entry that no longer means what its key
    /// says, and the only cost of discarding is time.
    pub fn open(
        path: impl AsRef<Path>,
        document_id: DocumentId,
        kernel_id: &str,
        kernel_version: &str,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if path.exists() && !matches(&path, document_id, kernel_id, kernel_version)? {
            std::fs::remove_file(&path).map_err(|e| {
                CadError::io(format!("discarding stale cache {}", path.display()), e)
            })?;
            // WAL companions belong to the discarded database, not to the new one.
            for suffix in ["-wal", "-shm"] {
                let mut companion = path.clone().into_os_string();
                companion.push(suffix);
                let _ = std::fs::remove_file(PathBuf::from(companion));
            }
        }

        let mut conn = Connection::open(&path)
            .map_err(|e| CadError::io(format!("opening cache {}", path.display()), e))?;
        schema::configure_cache_connection(&conn)?;
        schema::migrate_cache(&mut conn)?;

        conn.execute(
            "INSERT INTO cache_meta (id, document_id, format_version, kernel_id, kernel_version)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO NOTHING",
            params![
                document_id.to_bytes().as_slice(),
                schema::FORMAT_VERSION,
                kernel_id,
                kernel_version,
            ],
        )
        .map_err(|e| CadError::io("binding cache to its document", e))?;

        Ok(Self { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns one representation for a key, or `None` to rebuild.
    ///
    /// `kind` is part of the lookup. A B-Rep and a tessellation can have the
    /// same object and input key, and byte-level de-duplication must not make
    /// one masquerade as the other.
    pub fn get(
        &self,
        object: ObjectId,
        key: ContentHash,
        kind: &str,
    ) -> Result<Option<CacheEntry>> {
        let found: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT b.hash
                 FROM blob_refs r JOIN blobs b ON b.hash = r.hash
                 WHERE r.object_id = ?1 AND r.cache_key = ?2 AND r.kind = ?3",
                params![
                    object.to_bytes().as_slice(),
                    key.as_bytes().as_slice(),
                    kind,
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CadError::io("looking up cache entry", e))?;

        let Some(hash) = found else {
            return Ok(None);
        };
        let hash = ContentHash::from_slice(&hash)?;

        let mut stmt = self
            .conn
            .prepare("SELECT data FROM blob_chunks WHERE hash = ?1 ORDER BY seq")
            .map_err(|e| CadError::io("preparing chunk query", e))?;
        let rows = stmt
            .query_map(params![hash.as_bytes().as_slice()], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .map_err(|e| CadError::io("reading cache chunks", e))?;

        let mut bytes = Vec::new();
        for row in rows {
            bytes.extend_from_slice(&row.map_err(|e| CadError::io("reading cache chunk", e))?);
        }

        // The content is addressed by its own hash, so a mismatch means the
        // sidecar is damaged. Report a miss and let the caller rebuild.
        if ContentHash::of_bytes(&bytes) != hash {
            return Ok(None);
        }

        Ok(Some(CacheEntry {
            hash,
            kind: kind.to_owned(),
            bytes,
        }))
    }

    /// Stores derived bytes under a key, replacing whatever that key held.
    pub fn put(
        &mut self,
        object: ObjectId,
        key: ContentHash,
        kind: &str,
        bytes: &[u8],
    ) -> Result<ContentHash> {
        let hash = ContentHash::of_bytes(bytes);

        let tx = self
            .conn
            .transaction()
            .map_err(|e| CadError::io("starting cache write", e))?;

        let inserted = tx
            .execute(
                "INSERT INTO blobs (hash, byte_len, created_at)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 ON CONFLICT(hash) DO NOTHING",
                params![hash.as_bytes().as_slice(), bytes.len() as i64],
            )
            .map_err(|e| CadError::io("writing cache blob", e))?;

        // Content-addressed: identical bytes are already stored under the same
        // hash, so the chunks only need writing the first time.
        if inserted > 0 {
            for (seq, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
                tx.execute(
                    "INSERT INTO blob_chunks (hash, seq, data) VALUES (?1, ?2, ?3)",
                    params![hash.as_bytes().as_slice(), seq as i64, chunk],
                )
                .map_err(|e| CadError::io("writing cache chunk", e))?;
            }
        }

        tx.execute(
            "INSERT INTO blob_refs (object_id, cache_key, kind, hash) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(object_id, cache_key, kind) DO UPDATE SET hash = excluded.hash",
            params![
                object.to_bytes().as_slice(),
                key.as_bytes().as_slice(),
                kind,
                hash.as_bytes().as_slice()
            ],
        )
        .map_err(|e| CadError::io("recording cache entry", e))?;

        tx.commit()
            .map_err(|e| CadError::io("committing cache write", e))?;
        Ok(hash)
    }

    /// Drops every entry, keeping the sidecar's binding to its document.
    pub fn clear(&mut self) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| CadError::io("starting cache clear", e))?;
        tx.execute_batch("DELETE FROM blob_refs; DELETE FROM blobs;")
            .map_err(|e| CadError::io("clearing cache", e))?;
        tx.commit()
            .map_err(|e| CadError::io("committing cache clear", e))?;
        Ok(())
    }

    /// Removes entries no `blob_refs` row points at any more.
    pub fn collect_garbage(&mut self) -> Result<usize> {
        self.conn
            .execute(
                "DELETE FROM blobs WHERE hash NOT IN (SELECT hash FROM blob_refs)",
                [],
            )
            .map_err(|e| CadError::io("collecting cache garbage", e))
    }

    /// Deletes a sidecar and its journal companions from disk.
    pub fn discard(path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| CadError::io(format!("removing cache {}", path.display()), e))?;
        }
        for suffix in ["-wal", "-shm"] {
            let mut companion = path.to_path_buf().into_os_string();
            companion.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(companion));
        }
        Ok(())
    }
}

/// Whether an existing sidecar still describes this document and kernel.
fn matches(
    path: &Path,
    document_id: DocumentId,
    kernel_id: &str,
    kernel_version: &str,
) -> Result<bool> {
    let conn = match Connection::open(path) {
        Ok(conn) => conn,
        // An unreadable sidecar is simply not a match; it will be replaced.
        Err(_) => return Ok(false),
    };

    if schema::check_application_id(&conn, CACHE_APPLICATION_ID, "cache").is_err() {
        return Ok(false);
    }

    let row: Option<(Vec<u8>, u32, String, String)> = conn
        .query_row(
            "SELECT document_id, format_version, kernel_id, kernel_version
             FROM cache_meta WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .unwrap_or(None);

    let Some((stored_id, format_version, stored_kernel, stored_version)) = row else {
        return Ok(false);
    };

    Ok(stored_id == document_id.to_bytes()
        && format_version == schema::FORMAT_VERSION
        && stored_kernel == kernel_id
        && stored_version == kernel_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir, document: DocumentId) -> CacheStore {
        CacheStore::open(dir.path().join("m.fcad-cache"), document, "occt", "8.0.0")
            .expect("sidecar opens")
    }

    #[test]
    fn stored_bytes_come_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let document = DocumentId::new();
        let object = ObjectId::new();
        let key = ContentHash::of_bytes(b"key");

        let mut cache = store(&dir, document);
        cache.put(object, key, "brep", b"solid").expect("stores");

        let entry = cache
            .get(object, key, "brep")
            .expect("reads")
            .expect("present");
        assert_eq!(entry.bytes, b"solid");
        assert_eq!(entry.kind, "brep");
    }

    #[test]
    fn a_different_key_is_a_miss_not_a_wrong_hit() {
        let dir = tempfile::tempdir().expect("temp dir");
        let document = DocumentId::new();
        let object = ObjectId::new();

        let mut cache = store(&dir, document);
        cache
            .put(object, ContentHash::of_bytes(b"height 10"), "brep", b"a")
            .expect("stores");

        assert!(
            cache
                .get(object, ContentHash::of_bytes(b"height 11"), "brep")
                .expect("reads")
                .is_none()
        );
    }

    #[test]
    fn a_sidecar_from_another_document_is_discarded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("m.fcad-cache");
        let object = ObjectId::new();
        let key = ContentHash::of_bytes(b"key");

        let mut original =
            CacheStore::open(&path, DocumentId::new(), "occt", "8.0.0").expect("sidecar opens");
        original.put(object, key, "brep", b"solid").expect("stores");
        drop(original);

        let reopened = CacheStore::open(&path, DocumentId::new(), "occt", "8.0.0")
            .expect("a mismatched sidecar is replaced, not an error");
        assert!(reopened.get(object, key, "brep").expect("reads").is_none());
    }

    #[test]
    fn a_sidecar_from_another_kernel_build_is_discarded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("m.fcad-cache");
        let document = DocumentId::new();
        let object = ObjectId::new();
        let key = ContentHash::of_bytes(b"key");

        let mut original =
            CacheStore::open(&path, document, "occt", "8.0.0").expect("sidecar opens");
        original.put(object, key, "brep", b"solid").expect("stores");
        drop(original);

        let reopened =
            CacheStore::open(&path, document, "occt", "8.1.0").expect("replaced on mismatch");
        assert!(reopened.get(object, key, "brep").expect("reads").is_none());
    }

    #[test]
    fn identical_content_is_stored_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let document = DocumentId::new();
        let mut cache = store(&dir, document);

        let first = cache
            .put(
                ObjectId::new(),
                ContentHash::of_bytes(b"k1"),
                "brep",
                b"same",
            )
            .expect("stores");
        let second = cache
            .put(
                ObjectId::new(),
                ContentHash::of_bytes(b"k2"),
                "brep",
                b"same",
            )
            .expect("stores");
        assert_eq!(first, second);

        let blobs: i64 = cache
            .conn
            .query_row("SELECT count(*) FROM blobs", [], |row| row.get(0))
            .expect("counts");
        assert_eq!(blobs, 1);
    }

    #[test]
    fn byte_identical_representations_keep_their_own_kinds() {
        let dir = tempfile::tempdir().expect("temp dir");
        let document = DocumentId::new();
        let object = ObjectId::new();
        let key = ContentHash::of_bytes(b"same inputs");
        let mut cache = store(&dir, document);

        cache
            .put(object, key, "brep", b"same bytes")
            .expect("stores B-Rep");
        cache
            .put(object, key, "tessellation", b"same bytes")
            .expect("stores mesh");

        assert_eq!(
            cache
                .get(object, key, "brep")
                .expect("reads")
                .expect("B-Rep is present")
                .kind,
            "brep"
        );
        assert_eq!(
            cache
                .get(object, key, "tessellation")
                .expect("reads")
                .expect("mesh is present")
                .kind,
            "tessellation"
        );
    }

    #[test]
    fn payloads_larger_than_a_chunk_reassemble() {
        let dir = tempfile::tempdir().expect("temp dir");
        let document = DocumentId::new();
        let object = ObjectId::new();
        let key = ContentHash::of_bytes(b"big");
        let payload: Vec<u8> = (0..(CHUNK_BYTES * 2 + 7))
            .map(|i| (i % 251) as u8)
            .collect();

        let mut cache = store(&dir, document);
        cache.put(object, key, "brep", &payload).expect("stores");

        let entry = cache
            .get(object, key, "brep")
            .expect("reads")
            .expect("present");
        assert_eq!(entry.bytes, payload);
    }

    #[test]
    fn clearing_leaves_the_sidecar_usable() {
        let dir = tempfile::tempdir().expect("temp dir");
        let document = DocumentId::new();
        let object = ObjectId::new();
        let key = ContentHash::of_bytes(b"key");

        let mut cache = store(&dir, document);
        cache.put(object, key, "brep", b"solid").expect("stores");
        cache.clear().expect("clears");

        assert!(cache.get(object, key, "brep").expect("reads").is_none());
        cache
            .put(object, key, "brep", b"solid")
            .expect("stores again");
        assert!(cache.get(object, key, "brep").expect("reads").is_some());
    }

    #[test]
    fn garbage_collection_keeps_referenced_blobs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let document = DocumentId::new();
        let kept = ObjectId::new();
        let key = ContentHash::of_bytes(b"key");

        let mut cache = store(&dir, document);
        cache.put(kept, key, "brep", b"kept").expect("stores");
        cache
            .conn
            .execute(
                "INSERT INTO blobs (hash, byte_len, created_at)
                 VALUES (?1, 0, 'now')",
                params![ContentHash::of_bytes(b"orphan").as_bytes().as_slice()],
            )
            .expect("inserts an unreferenced blob");

        assert_eq!(cache.collect_garbage().expect("collects"), 1);
        assert!(cache.get(kept, key, "brep").expect("reads").is_some());
    }
}
