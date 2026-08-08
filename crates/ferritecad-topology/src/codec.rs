// SPDX-License-Identifier: MIT
//! Writing an archive down, and reading one back with suspicion.
//!
//! An [`ArchivedFeature`] is only useful if it outlives the process that made
//! it, so it needs a byte form. This module is that form and nothing else: it
//! produces and consumes `Vec<u8>`, and knows nothing about where those bytes
//! are kept. Storage is somebody else's problem, which is what lets this crate
//! keep its promise to hold no cache.
//!
//! # Every name is written as a number chosen on purpose
//!
//! [`BoundName`] is encoded with explicit tag constants, never with its
//! `Debug` text and never with its declaration order. Both of those would tie
//! the on-disk meaning of an old cache to the source layout of a newer build:
//! reordering an enum or renaming a variant would silently re-point every
//! stored name at different geometry. A tag is a decision that can be kept.
//!
//! # Reading is fail-closed
//!
//! Nothing here repairs, guesses or skips. A version from the future, an
//! unknown tag, a short read, trailing bytes, the wrong producer, another
//! kernel build, a checksum that disagrees — each is an error, and the caller's
//! only recourse is to discard the entry and rebuild. That is cheap. The
//! alternative is a name resolving to a face that is not the one it means.

use std::mem::size_of;

use ferritecad_kernel::{ArchiveSlot, BrepBlob, KernelIdentity};
use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};

use crate::archive::{ArchivedFeature, BoundName};

/// What a cache entry holding an encoded archive is called.
///
/// The B-Rep and the names that reach into it are one entry under one kind.
/// Two kinds could not be written atomically, and a store holding the geometry
/// of one rebuild beside the table of another is exactly the failure this
/// design exists to prevent.
pub const ARCHIVE_CACHE_KIND: &str = "brep.named.v1";

const MAGIC: &[u8; 4] = b"FCNA";

/// Bumped when the meaning of these bytes changes.
///
/// A reader refuses anything higher rather than interpreting a layout it
/// predates. An older version could be read by a future build if it ever
/// becomes worth the code; today there is nothing older to read.
const FORMAT_VERSION: u16 = 1;

/// Bytes before the checksummed archive payload.
const HEADER_LEN: usize = MAGIC.len() + size_of::<u16>() + size_of::<u64>() + 32;

// Stable on disk. Never reuse a number for a different meaning, and never
// renumber one: an old cache entry outlives the build that wrote it.
const TAG_START_CAP: u16 = 1;
const TAG_END_CAP: u16 = 2;
const TAG_SIDE: u16 = 3;

impl ArchivedFeature {
    /// Writes the archive out as bytes.
    ///
    /// Deterministic: the binding table is already ordered, so the same
    /// archive encodes to the same bytes on every machine and the cache can
    /// address the result by its content.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&self.producer().to_bytes());

        let kernel = self.blob().kernel();
        put_str(&mut payload, kernel.id())?;
        put_str(&mut payload, kernel.version())?;
        put_str(&mut payload, kernel.build())?;

        payload.extend_from_slice(self.blob_hash().as_bytes());
        let bytes = self.blob().bytes();
        let blob_length = u64::try_from(bytes.len())
            .map_err(|_| malformed("the B-Rep is too large to archive"))?;
        payload.extend_from_slice(&blob_length.to_le_bytes());
        payload.extend_from_slice(bytes);

        let bindings = self.bindings();
        let binding_count = u32::try_from(bindings.len())
            .map_err(|_| malformed("there are too many topology names to archive"))?;
        payload.extend_from_slice(&binding_count.to_le_bytes());
        for (name, slot) in bindings {
            match name {
                BoundName::StartCap => payload.extend_from_slice(&TAG_START_CAP.to_le_bytes()),
                BoundName::EndCap => payload.extend_from_slice(&TAG_END_CAP.to_le_bytes()),
                BoundName::Side { profile_segment } => {
                    payload.extend_from_slice(&TAG_SIDE.to_le_bytes());
                    payload.extend_from_slice(&profile_segment.to_bytes());
                }
            }
            payload.extend_from_slice(&slot.index().to_le_bytes());
        }

        let payload_length = u64::try_from(payload.len())
            .map_err(|_| malformed("the named archive is too large to frame"))?;
        let payload_hash = ContentHash::of_bytes(&payload);
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&payload_length.to_le_bytes());
        out.extend_from_slice(payload_hash.as_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }

    /// Reads an archive back, refusing anything that is not exactly one.
    ///
    /// The caller must say which feature it expects and which kernel is
    /// loaded. Both are checked against what the bytes claim, so an entry that
    /// was reached by a stale key or written by another build is rejected here
    /// even if the lookup that found it was satisfied.
    pub fn decode(bytes: &[u8], producer: ObjectId, kernel: &KernelIdentity) -> Result<Self> {
        let mut reader = Reader::new(bytes);

        if reader.take(MAGIC.len(), "magic")? != MAGIC {
            return Err(malformed("these bytes are not a named archive"));
        }

        let version = reader.u16("format version")?;
        if version != FORMAT_VERSION {
            return Err(malformed(format!(
                "this archive is version {version} and this build reads version {FORMAT_VERSION}; \
                 discard the entry and rebuild"
            )));
        }

        let payload_length = usize::try_from(reader.u64("archive payload length")?)
            .map_err(|_| malformed("this archive claims a payload larger than memory"))?;
        let payload_hash = ContentHash::from_bytes(reader.array("archive payload checksum")?);
        let payload = reader.take(payload_length, "archive payload")?;
        reader.finish("archive payload")?;

        if ContentHash::of_bytes(payload) != payload_hash {
            return Err(malformed(
                "the named archive payload does not match its checksum",
            ));
        }

        let mut reader = Reader::new(payload);
        let stored_producer = ObjectId::from_bytes(reader.array("producer")?)?;
        if stored_producer != producer {
            return Err(malformed(format!(
                "this archive belongs to feature {stored_producer}, not to {producer}"
            )));
        }

        let stored_kernel = KernelIdentity::new(
            reader.string("kernel id")?,
            reader.string("kernel version")?,
            reader.string("kernel build")?,
        )?;

        let blob_hash = ContentHash::from_bytes(reader.array("payload checksum")?);
        let length = usize::try_from(reader.u64("payload length")?)
            .map_err(|_| malformed("this archive claims a payload larger than memory"))?;
        let blob = BrepBlob::new(stored_kernel, reader.take(length, "payload")?.to_vec());

        // Before anything is believed about the geometry: the identity check
        // is what stops a blob from a different bridge build being handed to
        // this one, which the sidecar's own metadata does not record.
        blob.require_kernel(kernel)?;

        let count = reader.u32("binding count")?;
        let mut bindings = Vec::with_capacity(count.min(1024) as usize);
        for _ in 0..count {
            let tag = reader.u16("binding tag")?;
            let name = match tag {
                TAG_START_CAP => BoundName::StartCap,
                TAG_END_CAP => BoundName::EndCap,
                TAG_SIDE => BoundName::Side {
                    profile_segment: StableEntityId::from_bytes(reader.array("profile segment")?)?,
                },
                unknown => {
                    return Err(malformed(format!(
                        "this archive names something with tag {unknown}, which this build does \
                         not know; a name it cannot read is not a name it may ignore"
                    )));
                }
            };
            bindings.push((name, ArchiveSlot::new(reader.u32("slot")?)));
        }

        reader.finish("last binding")?;

        // `from_parts` is the single gate on what a table may say: no root
        // slot, no repeated name, no shared slot, no empty table, and a
        // checksum that matches the payload it arrived with.
        Self::from_parts(producer, blob, blob_hash, bindings)
    }
}

fn put_str(out: &mut Vec<u8>, value: &str) -> Result<()> {
    let length = u32::try_from(value.len())
        .map_err(|_| malformed("a kernel identity field is too large to archive"))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn malformed(what: impl Into<String>) -> CadError {
    CadError::input(what)
}

/// A cursor that treats every short read as corruption.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize, what: &str) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| {
                malformed(format!(
                    "this archive ends inside its {what}: {count} more byte(s) were needed and {} \
                     remain",
                    self.bytes.len() - self.at
                ))
            })?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self, what: &str) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N, what)?);
        Ok(out)
    }

    fn u16(&mut self, what: &str) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array(what)?))
    }

    fn u32(&mut self, what: &str) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array(what)?))
    }

    fn u64(&mut self, what: &str) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array(what)?))
    }

    fn string(&mut self, what: &str) -> Result<String> {
        let length = usize::try_from(self.u32(what)?)
            .map_err(|_| malformed(format!("this archive claims an unreadable {what}")))?;
        let bytes = self.take(length, what)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| malformed(format!("this archive's {what} is not text")))
    }

    /// Refuses anything left over.
    ///
    /// Trailing bytes mean the writer and this reader disagree about the
    /// layout. Ignoring them would let a longer record be read as a shorter
    /// one, which is how a partial parse becomes a confident wrong answer.
    fn finish(self, what: &str) -> Result<()> {
        let left = self.bytes.len() - self.at;
        if left > 0 {
            return Err(malformed(format!(
                "this archive has {left} byte(s) after its {what}"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{archive_feature, map::TopologyMap};
    use ferritecad_kernel::{
        ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint, Profile,
        ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane, mock::MockKernel,
    };

    fn archived() -> (ArchivedFeature, ObjectId, KernelIdentity) {
        let corners = [(0.0, 0.0), (30.0, 0.0), (30.0, 20.0), (0.0, 20.0)];
        let points: Vec<PlanarPoint> = corners
            .iter()
            .map(|(x, y)| PlanarPoint::new(*x, *y).expect("a finite point"))
            .collect();
        let segments: Vec<ProfileSegment> = (0..points.len())
            .map(|i| {
                ProfileSegment::new(
                    StableEntityId::new(),
                    SegmentGeometry::line(points[i], points[(i + 1) % points.len()])
                        .expect("a line"),
                )
            })
            .collect();
        let profile = Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(segments).expect("a closed loop"),
            Vec::new(),
        )
        .expect("a valid profile");
        let request = ExtrudeRequest::new(
            profile,
            ExtrudeExtent::blind(5.0).expect("a positive distance"),
            false,
        );

        let mut kernel = MockKernel::new();
        let result = kernel
            .extrude(&request, &OperationContext::default())
            .expect("the mock builds");
        let producer = ObjectId::new();
        let mut map = TopologyMap::new();
        map.record_extrude(producer, request.profile(), &result)
            .expect("records");

        let identity = kernel.identity().clone();
        let archived = archive_feature(&mut kernel, &map, producer).expect("archives");
        (archived, producer, identity)
    }

    #[test]
    fn an_archive_survives_its_byte_form() {
        let (original, producer, kernel) = archived();
        let bytes = original.encode().expect("encodes");
        let restored = ArchivedFeature::decode(&bytes, producer, &kernel).expect("reads back");
        assert_eq!(restored, original);
    }

    #[test]
    fn the_same_archive_encodes_the_same_way_every_time() {
        let (archive, _, _) = archived();
        assert_eq!(
            archive.encode().expect("encodes"),
            archive.encode().expect("encodes")
        );
    }

    #[test]
    fn another_features_archive_is_refused() {
        let (archive, _, kernel) = archived();
        let bytes = archive.encode().expect("encodes");
        let err = ArchivedFeature::decode(&bytes, ObjectId::new(), &kernel)
            .expect_err("the producer does not match");
        assert!(err.to_string().contains("belongs to feature"));
    }

    #[test]
    fn another_kernel_build_is_refused() {
        let (archive, producer, _) = archived();
        let other = KernelIdentity::new("mock", "1.0.0", "another-compiler").expect("valid");
        let bytes = archive.encode().expect("encodes");
        assert!(ArchivedFeature::decode(&bytes, producer, &other).is_err());
    }

    #[test]
    fn a_version_from_the_future_is_refused() {
        let (archive, producer, kernel) = archived();
        let mut bytes = archive.encode().expect("encodes");
        bytes[4..6].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());

        let err = ArchivedFeature::decode(&bytes, producer, &kernel)
            .expect_err("a newer layout is not readable");
        assert!(err.to_string().contains("discard the entry"));
    }

    #[test]
    fn an_unknown_name_tag_is_refused_rather_than_skipped() {
        let (archive, producer, kernel) = archived();
        let bytes = archive.encode().expect("encodes");

        // The first tag sits directly after the binding count.
        let at = bytes.len()
            - archive
                .bindings()
                .map(|(name, _)| match name {
                    BoundName::Side { .. } => 2 + 16 + 4,
                    _ => 2 + 4,
                })
                .sum::<usize>();
        let mut damaged = bytes.clone();
        damaged[at..at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        reseal(&mut damaged);

        let err = ArchivedFeature::decode(&damaged, producer, &kernel)
            .expect_err("an unreadable name is not an ignorable one");
        assert!(err.to_string().contains("does not know"));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let (archive, producer, kernel) = archived();
        let mut bytes = archive.encode().expect("encodes");
        bytes.push(0);

        let err = ArchivedFeature::decode(&bytes, producer, &kernel).expect_err("there is more");
        assert!(err.to_string().contains("after its archive payload"));
    }

    #[test]
    fn a_truncated_archive_is_refused_at_every_length() {
        let (archive, producer, kernel) = archived();
        let bytes = archive.encode().expect("encodes");

        for length in 0..bytes.len() {
            assert!(
                ArchivedFeature::decode(&bytes[..length], producer, &kernel).is_err(),
                "{length} bytes of an archive is not an archive"
            );
        }
        assert!(ArchivedFeature::decode(&bytes, producer, &kernel).is_ok());
    }

    #[test]
    fn a_damaged_payload_is_refused_by_its_checksum() {
        let (archive, producer, kernel) = archived();
        let mut bytes = archive.encode().expect("encodes");

        // Somewhere inside the payload, past every header field.
        let at = bytes.len() / 2;
        bytes[at] ^= 0xff;
        reseal(&mut bytes);

        let err = ArchivedFeature::decode(&bytes, producer, &kernel)
            .expect_err("the payload no longer matches its checksum");
        assert!(err.to_string().contains("different archive"));
    }

    #[test]
    fn a_changed_binding_is_refused_by_the_archive_checksum() {
        let (archive, producer, kernel) = archived();
        let mut bytes = archive.encode().expect("encodes");

        // A slot is semantic data just as much as the B-Rep. Without the
        // archive-level checksum, changing this to another valid slot could
        // silently make a durable name resolve to the wrong face.
        *bytes.last_mut().expect("the archive has a binding") ^= 0x01;

        let err = ArchivedFeature::decode(&bytes, producer, &kernel)
            .expect_err("a changed binding is not an intact archive");
        assert!(
            err.to_string()
                .contains("payload does not match its checksum")
        );
    }

    #[test]
    fn a_table_naming_one_slot_twice_is_refused() {
        let (archive, producer, kernel) = archived();
        let bytes = archive.encode().expect("encodes");

        // Rewrite every slot as 1, which two names may not share.
        let mut damaged = bytes.clone();
        let mut at = bytes.len();
        for (name, _) in archive.bindings() {
            at -= match name {
                BoundName::Side { .. } => 2 + 16 + 4,
                _ => 2 + 4,
            };
        }
        for (name, _) in archive.bindings() {
            let width = match name {
                BoundName::Side { .. } => 2 + 16 + 4,
                _ => 2 + 4,
            };
            damaged[at + width - 4..at + width].copy_from_slice(&1u32.to_le_bytes());
            at += width;
        }
        reseal(&mut damaged);

        assert!(ArchivedFeature::decode(&damaged, producer, &kernel).is_err());
    }

    fn reseal(bytes: &mut [u8]) {
        let payload_hash = ContentHash::of_bytes(&bytes[HEADER_LEN..]);
        bytes[HEADER_LEN - 32..HEADER_LEN].copy_from_slice(payload_hash.as_bytes());
    }
}
