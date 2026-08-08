// SPDX-License-Identifier: MIT
//! Carrying names across a session boundary.
//!
//! A [`TopologyMap`] is session-local by construction, and a cached B-Rep
//! carries geometry with no history. Neither alone survives the trip: the map
//! names handles that die with their session, and the blob restores faces that
//! answer to nothing.
//!
//! What travels is a small table of *slots*. An archive holds the shape
//! together with the sub-shapes worth keeping, and a slot says which of those
//! a name refers to. The table is meaningless without its blob and says
//! nothing about geometry on its own — which is exactly the property that lets
//! it be written down, where a handle could not be.

use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::CapSide;
use ferritecad_kernel::{ArchiveSlot, BrepBlob, GeometryKernel, SubShapeKind};
use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};

use crate::map::TopologyMap;

/// A name an archive can carry, in a form that can be ordered and stored.
///
/// Deliberately narrower than
/// [`SemanticRole`][ferritecad_document::SemanticRole]: it holds the roles
/// this slice can actually archive, so a role with no geometry behind it
/// cannot be written into a table and found missing later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BoundName {
    /// The face closing the start of a sweep.
    StartCap,
    /// The face closing the end of a sweep.
    EndCap,
    /// A face raised from one profile segment.
    Side { profile_segment: StableEntityId },
}

impl BoundName {
    /// The cap name for a side this build understands.
    ///
    /// `CapSide` is non-exhaustive; a future side has no name here rather than
    /// being folded into one of the two known ends.
    pub fn cap(side: CapSide) -> Option<Self> {
        match side {
            CapSide::Start => Some(Self::StartCap),
            CapSide::End => Some(Self::EndCap),
            _ => None,
        }
    }
}

/// One feature's geometry and the names that reach into it.
///
/// The blob and the table are one thing and must stay together: a slot without
/// its archive addresses nothing, and an archive without its table restores
/// faces nobody can name. The payload checksum is carried so a table paired
/// with the wrong blob is refused rather than resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedFeature {
    producer: ObjectId,
    blob: BrepBlob,
    blob_hash: ContentHash,
    /// Ordered so two archives of the same rebuild compare equal.
    bindings: BTreeMap<BoundName, ArchiveSlot>,
}

impl ArchivedFeature {
    pub fn producer(&self) -> ObjectId {
        self.producer
    }

    pub fn blob(&self) -> &BrepBlob {
        &self.blob
    }

    /// The names this archive carries, in a deterministic order.
    pub fn bindings(&self) -> impl ExactSizeIterator<Item = (BoundName, ArchiveSlot)> + '_ {
        self.bindings.iter().map(|(name, slot)| (*name, *slot))
    }

    pub fn slot(&self, name: BoundName) -> Option<ArchiveSlot> {
        self.bindings.get(&name).copied()
    }

    /// Rebuilds an archive from parts that were stored separately.
    ///
    /// Refuses a table that cannot describe anything: an empty one, a slot
    /// naming the archived shape itself, two names sharing a slot, or a
    /// checksum that does not match the blob it arrived with.
    pub fn from_parts(
        producer: ObjectId,
        blob: BrepBlob,
        blob_hash: ContentHash,
        bindings: impl IntoIterator<Item = (BoundName, ArchiveSlot)>,
    ) -> Result<Self> {
        let mut table = BTreeMap::new();
        let mut taken = BTreeSet::new();

        for (name, slot) in bindings {
            if slot.is_root() {
                return Err(CadError::topology(format!(
                    "the binding for feature {producer} points {name:?} at the archived shape \
                     itself, which is not a sub-shape"
                )));
            }
            if !taken.insert(slot) {
                // Two names on one face is not a resolvable ambiguity; it is a
                // table that was built wrong, and using it would give one of
                // the two names the other's geometry.
                return Err(CadError::topology(format!(
                    "the binding for feature {producer} gives slot {} to more than one name",
                    slot.index()
                )));
            }
            if table.insert(name, slot).is_some() {
                return Err(CadError::topology(format!(
                    "the binding for feature {producer} names {name:?} twice"
                )));
            }
        }

        if table.is_empty() {
            return Err(CadError::topology(format!(
                "the binding for feature {producer} names nothing"
            )));
        }

        if blob.content_hash() != blob_hash {
            return Err(CadError::topology(format!(
                "the binding for feature {producer} was recorded against a different archive; \
                 discard the pair rather than resolving it"
            )));
        }

        Ok(Self {
            producer,
            blob,
            blob_hash,
            bindings: table,
        })
    }
}

/// Archives what one feature produced, together with the names for it.
///
/// The order the sub-shapes are handed to the kernel is the order of
/// [`BoundName`], so two runs of the same rebuild archive the same way.
pub fn archive_feature(
    kernel: &mut dyn GeometryKernel,
    map: &TopologyMap,
    producer: ObjectId,
) -> Result<ArchivedFeature> {
    let names = map.feature(producer).ok_or_else(|| {
        CadError::topology(format!(
            "feature {producer} produced no named geometry to archive"
        ))
    })?;
    let shape = names.shape().ok_or_else(|| {
        CadError::topology(format!("feature {producer} produced no shape to archive"))
    })?;

    // Gathered through the ordered map so the sequence is the same every run.
    let mut wanted = Vec::new();
    for side in [CapSide::Start, CapSide::End] {
        let Some(name) = BoundName::cap(side) else {
            continue;
        };
        let faces: Vec<_> = names.cap(side).into_iter().flatten().collect();
        match faces.len() {
            0 => {}
            1 => wanted.push((name, faces[0])),
            found => {
                return Err(CadError::topology(format!(
                    "feature {producer} produced {found} faces for {name:?}; a cap is one face, \
                     and archiving an ambiguous name would hand back the wrong one"
                )));
            }
        }
    }
    for segment in names.named_segments() {
        for face in names.side(segment) {
            wanted.push((
                BoundName::Side {
                    profile_segment: segment,
                },
                face,
            ));
        }
    }

    // A side that raised several faces cannot be archived under one slot in
    // this slice; the family rule needs several, and one slot names one.
    let mut seen = BTreeSet::new();
    for (name, _) in &wanted {
        if !seen.insert(*name) {
            return Err(CadError::topology(format!(
                "feature {producer} raised more than one face for {name:?}; archiving a family of \
                 faces is not implemented"
            )));
        }
    }

    // Distinct names must not be smuggled past `ArchivedFeature` by assigning
    // the same face two distinct slots. The table can see slot identity only;
    // this is the last point that can still compare the live handles.
    let mut claimed = BTreeMap::new();
    for (name, face) in &wanted {
        if face.kind() != SubShapeKind::Face {
            return Err(CadError::topology(format!(
                "feature {producer} names {name:?} as a {}, which is not a face",
                face.kind()
            )));
        }
        if face.shape() != shape {
            return Err(CadError::topology(format!(
                "feature {producer} names {name:?} on a shape it did not build"
            )));
        }
        if let Some(previous) = claimed.insert(*face, *name) {
            return Err(CadError::topology(format!(
                "feature {producer} gives both {previous:?} and {name:?} to the same face; \
                 refusing rather than archiving a silent alias"
            )));
        }
    }

    let faces: Vec<_> = wanted.iter().map(|(_, face)| *face).collect();
    let (blob, slots) = kernel.encode_shape_with(shape, &faces)?;

    if slots.len() != wanted.len() {
        return Err(CadError::kernel(format!(
            "archiving feature {producer} asked for {} slots and received {}",
            wanted.len(),
            slots.len()
        )));
    }

    let blob_hash = blob.content_hash();
    ArchivedFeature::from_parts(
        producer,
        blob,
        blob_hash,
        wanted.into_iter().map(|(name, _)| name).zip(slots),
    )
}

/// Restores a feature's geometry and its names into a fresh map.
///
/// The kernel that reads the archive need not be the one that wrote it — that
/// is the point — but it must agree about identity, format and checksum, which
/// the kernel checks before any geometry is decoded.
///
/// On success, the caller owns the restored root shape reachable through
/// `into.feature(archived.producer())` and must eventually release it through
/// the same kernel session. On failure, this function releases any shape that
/// it decoded before returning.
pub fn restore_feature(
    kernel: &mut dyn GeometryKernel,
    archived: &ArchivedFeature,
    into: &mut TopologyMap,
) -> Result<()> {
    let names: Vec<BoundName> = archived.bindings.keys().copied().collect();
    let slots: Vec<ArchiveSlot> = archived.bindings.values().copied().collect();

    let (shape, faces) = kernel.decode_shape_with(&archived.blob, &slots)?;
    let restored = (|| -> Result<()> {
        if faces.len() != names.len() {
            return Err(CadError::kernel(format!(
                "restoring feature {} asked for {} sub-shapes and received {}",
                archived.producer,
                names.len(),
                faces.len()
            )));
        }

        let mut start_cap = Vec::new();
        let mut end_cap = Vec::new();
        let mut sides: BTreeMap<StableEntityId, Vec<_>> = BTreeMap::new();
        let mut claimed = BTreeMap::new();

        for (name, face) in names.into_iter().zip(faces) {
            if face.kind() != SubShapeKind::Face {
                return Err(CadError::topology(format!(
                    "the archive of feature {} restored {name:?} as a {}, which is not a face",
                    archived.producer,
                    face.kind()
                )));
            }
            if face.shape() != shape {
                return Err(CadError::topology(format!(
                    "the archive of feature {} restored {name:?} on another shape",
                    archived.producer
                )));
            }
            if let Some(previous) = claimed.insert(face, name) {
                return Err(CadError::topology(format!(
                    "the archive of feature {} restored both {previous:?} and {name:?} as the \
                     same face; refusing rather than accepting a silent alias",
                    archived.producer
                )));
            }

            match name {
                BoundName::StartCap => start_cap.push(face),
                BoundName::EndCap => end_cap.push(face),
                BoundName::Side { profile_segment } => {
                    sides.entry(profile_segment).or_default().push(face)
                }
            }
        }

        into.record_restored(archived.producer, shape, &start_cap, &end_cap, &sides)
    })();

    if restored.is_err() {
        kernel.release(shape);
    }
    restored
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_kernel::mock::MockKernel;

    #[test]
    fn a_table_naming_the_archived_shape_itself_is_refused() {
        let kernel = MockKernel::new();
        let blob = BrepBlob::new(kernel.identity().clone(), vec![1, 2, 3]);
        let hash = blob.content_hash();

        let err = ArchivedFeature::from_parts(
            ObjectId::new(),
            blob,
            hash,
            [(BoundName::StartCap, ArchiveSlot::ROOT)],
        )
        .expect_err("slot zero is the shape");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Topology);
    }

    #[test]
    fn two_names_on_one_slot_are_refused() {
        let kernel = MockKernel::new();
        let blob = BrepBlob::new(kernel.identity().clone(), vec![1, 2, 3]);
        let hash = blob.content_hash();

        let err = ArchivedFeature::from_parts(
            ObjectId::new(),
            blob,
            hash,
            [
                (BoundName::StartCap, ArchiveSlot::new(1)),
                (BoundName::EndCap, ArchiveSlot::new(1)),
            ],
        )
        .expect_err("one face cannot answer to two names");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Topology);
    }

    #[test]
    fn an_empty_table_is_refused() {
        let kernel = MockKernel::new();
        let blob = BrepBlob::new(kernel.identity().clone(), vec![1, 2, 3]);
        let hash = blob.content_hash();

        assert!(ArchivedFeature::from_parts(ObjectId::new(), blob, hash, []).is_err());
    }

    #[test]
    fn a_table_recorded_against_another_archive_is_refused() {
        let kernel = MockKernel::new();
        let blob = BrepBlob::new(kernel.identity().clone(), vec![1, 2, 3]);
        let wrong = ContentHash::of_bytes(b"some other archive");

        let err = ArchivedFeature::from_parts(
            ObjectId::new(),
            blob,
            wrong,
            [(BoundName::StartCap, ArchiveSlot::new(1))],
        )
        .expect_err("the checksum does not match");
        assert!(err.to_string().contains("different archive"));
    }

    #[test]
    fn an_unknown_cap_side_has_no_name() {
        assert_eq!(BoundName::cap(CapSide::Start), Some(BoundName::StartCap));
        assert_eq!(BoundName::cap(CapSide::End), Some(BoundName::EndCap));
    }
}
