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
use ferritecad_kernel::{
    ExtrudeRequest, KernelIdentity, OperationContext, RevolveRequest, cut_cache_key,
    extrude_cache_key, revolve_cache_key,
};
use ferritecad_topology::{ARCHIVE_CACHE_KIND, ArchivedFeature};
use ferritecad_types::{CanonicalHasher, ContentHash, ObjectId, Result};

/// Where an extrusion's archive lives in the sidecar.
pub fn extrude_archive_key(
    kernel: &KernelIdentity,
    request: &ExtrudeRequest,
    context: &OperationContext,
) -> ContentHash {
    extrude_cache_key(kernel, request, context)
}

/// Where a revolution's archive lives in the sidecar.
///
/// The kernel identity, the tolerance, the resolved profile with its Line
/// labels, the axis and the angle, under a domain of its own; see
/// [`revolve_cache_key`]. A profile that differs in any coordinate or label is
/// another entry.
pub fn revolve_archive_key(
    kernel: &KernelIdentity,
    request: &RevolveRequest,
    context: &OperationContext,
) -> ContentHash {
    revolve_cache_key(kernel, request, context)
}

/// Where a cut's archive lives in the sidecar.
///
/// Both inputs are named by their own keys rather than by their handles, so the
/// entry moves when either input does — a different plate, a different tool, a
/// different tolerance or a different kernel are all different results. The
/// algorithm version and the kernel identity are folded in by
/// [`cut_cache_key`] itself.
pub fn cut_archive_key(
    kernel: &KernelIdentity,
    previous: ObjectId,
    target_key: &ContentHash,
    tool_key: &ContentHash,
    through_all: bool,
    context: &OperationContext,
) -> ContentHash {
    // Geometry alone cannot key names: changing a predecessor UUID without
    // changing its drawing must not restore bindings to the old producer.
    let mut hasher = CanonicalHasher::new("eval.cut.named");
    hasher.algorithm_version(1);
    hasher.field("previous").bytes(&previous.to_bytes());
    // Fed only for ThroughAll, so every Blind key is the key it always was.
    // The tool key already moves with the computed length; this keeps a
    // ThroughAll archive from ever answering a Blind request of equal numbers.
    if through_all {
        hasher.field("through_all").bool(true);
    }
    hasher
        .field("geometry")
        .bytes(cut_cache_key(kernel, target_key, tool_key, context).as_bytes());
    hasher.finish()
}

/// Writes one feature's geometry and names into the sidecar, under a key the
/// caller computed.
///
/// Taking the key rather than the request is what lets an extrusion and a
/// boolean share one store: they are keyed by different facts and archived by
/// the same bytes.
pub fn store_feature_archive(
    cache: &mut CacheStore,
    kernel: &KernelIdentity,
    key: ContentHash,
    archived: &ArchivedFeature,
) -> Result<ContentHash> {
    archived.blob().require_kernel(kernel)?;
    let bytes = archived.encode()?;
    cache.put(archived.producer(), key, ARCHIVE_CACHE_KIND, &bytes)
}

/// Reads back what a previous run stored under one key, if anything.
pub fn load_feature_archive(
    cache: &CacheStore,
    kernel: &KernelIdentity,
    key: ContentHash,
    producer: ObjectId,
) -> Result<Option<ArchivedFeature>> {
    let Some(entry) = cache.get(producer, key, ARCHIVE_CACHE_KIND)? else {
        return Ok(None);
    };
    ArchivedFeature::decode(&entry.bytes, producer, kernel).map(Some)
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
    // Refuse a mismatched caller before it can replace a good entry with bytes
    // that the key's own kernel will necessarily reject on the next read.
    archived.blob().require_kernel(kernel)?;
    let bytes = archived.encode()?;
    cache.put(
        archived.producer(),
        extrude_archive_key(kernel, request, context),
        ARCHIVE_CACHE_KIND,
        &bytes,
    )
}

/// Reads back what a previous run stored for this feature, if anything.
///
/// The outcomes the storage API preserves are kept apart on purpose:
///
/// - `Ok(None)` — the store found no usable bytes under this key. This covers
///   both a normal miss and damage its own content hash detected; `CacheStore`
///   deliberately turns either into a miss because both require a rebuild.
/// - `Ok(Some(_))` — an archive that passed every check.
/// - `Err(_)` — the store returned internally intact bytes that are not a valid
///   named archive: another producer, another kernel build, a layout this build
///   cannot read, or a failed inner checksum. The caller may still rebuild and
///   may report this narrower class of rejected cache entry.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_cut_cache_keys_include_predecessor_identity() {
        let kernel = KernelIdentity::new("test", "1", "test").expect("identity");
        let target = ContentHash::of_bytes(b"same plate geometry and curve UUIDs");
        let tool = ContentHash::of_bytes(b"same tool geometry");
        let a = ObjectId::new();
        let b = ObjectId::new();
        let context = OperationContext::default();
        let one = cut_archive_key(&kernel, a, &target, &tool, false, &context);
        let other = cut_archive_key(&kernel, b, &target, &tool, false, &context);
        assert_ne!(
            one, other,
            "same geometry must not restore another producer's names"
        );
        assert_ne!(
            cut_archive_key(&kernel, a, &one, &tool, false, &context),
            cut_archive_key(&kernel, a, &other, &tool, false, &context),
            "the origin change propagates through the dependent chain"
        );
        assert_ne!(
            cut_archive_key(&kernel, a, &target, &tool, true, &context),
            one,
            "a ThroughAll archive never answers a Blind cut of the same tool"
        );
    }
}
