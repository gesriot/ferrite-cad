// SPDX-License-Identifier: MIT
//! Keeping a rebuilt feature, and its names, until next time.
//!
//! One entry holds both. The geometry without the table restores faces nobody
//! can name; the table without the geometry addresses nothing. Two entries
//! could not be written in one transaction, so a crash between them would
//! leave a store whose table describes a blob that is not there — or worse, an
//! older one that is.
//!
//! Nothing here is consulted during a rebuild yet. This slice makes the
//! artifact durable and proves it survives; using it is the next one, and
//! [`rebuild_cold`](crate::rebuild_cold) is deliberately unchanged.
//!
//! # Why the key is computed here rather than taken
//!
//! [`extrude_cache_key`] folds in the full [`KernelIdentity`], the tolerance
//! and the resolved request — the profile geometry as the kernel will actually
//! receive it. A key built from the document's own
//! [`cache_key`][ferritecad_document::Extrude::cache_key] would omit both, and
//! two documents that resolve to different solids would share an entry.
//!
//! The sidecar's metadata records only the kernel's id and version, so a
//! rebuilt bridge — same Open CASCADE, different shim or compiler — does not
//! cause the file to be discarded when it is opened. It does not need to: the
//! `build` field is part of every key here, so such an entry is never found,
//! and the identity written inside the record refuses it even if it were.

use ferritecad_document::CacheStore;
use ferritecad_kernel::{ExtrudeRequest, KernelIdentity, OperationContext, extrude_cache_key};
use ferritecad_topology::{ARCHIVE_CACHE_KIND, ArchivedFeature};
use ferritecad_types::{ContentHash, ObjectId, Result};

/// Where an extrusion's archive lives in the sidecar.
pub fn extrude_archive_key(
    kernel: &KernelIdentity,
    request: &ExtrudeRequest,
    context: &OperationContext,
) -> ContentHash {
    extrude_cache_key(kernel, request, context)
}

/// Writes one feature's geometry and names into the sidecar.
///
/// Replaces whatever that key held. An entry is derived data: overwriting it
/// loses nothing that cannot be computed again.
pub fn store_extrude_archive(
    cache: &mut CacheStore,
    kernel: &KernelIdentity,
    request: &ExtrudeRequest,
    context: &OperationContext,
    archived: &ArchivedFeature,
) -> Result<ContentHash> {
    cache.put(
        archived.producer(),
        extrude_archive_key(kernel, request, context),
        ARCHIVE_CACHE_KIND,
        &archived.encode(),
    )
}

/// Reads back what a previous run stored for this feature, if anything.
///
/// The three outcomes are kept apart on purpose:
///
/// - `Ok(None)` — nothing was stored under this key. Rebuild; that is normal.
/// - `Ok(Some(_))` — an archive that passed every check.
/// - `Err(_)` — something *was* stored and is not usable: another producer,
///   another kernel build, a layout this build cannot read, a failed checksum.
///   The caller may still rebuild, but this is a damaged sidecar rather than a
///   cold start, and the difference is worth reporting.
pub fn load_extrude_archive(
    cache: &CacheStore,
    kernel: &KernelIdentity,
    request: &ExtrudeRequest,
    context: &OperationContext,
    producer: ObjectId,
) -> Result<Option<ArchivedFeature>> {
    let key = extrude_archive_key(kernel, request, context);
    let Some(entry) = cache.get(producer, key, ARCHIVE_CACHE_KIND)? else {
        return Ok(None);
    };
    ArchivedFeature::decode(&entry.bytes, producer, kernel).map(Some)
}
