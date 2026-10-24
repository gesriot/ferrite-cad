// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_kernel::{ArchiveSlot, BrepBlob, GeometryKernel, ShapeTopology, SubShapeHandle};
use ferritecad_types::{CadError, ObjectId, Result};

use crate::TopologyMap;
use crate::generative::{FeatureNaming, Name};

/// One atomic B-Rep and complete typed naming graph. Slots have no meaning outside this blob.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedFeature {
    pub(crate) producer: ObjectId,
    pub(crate) blob: BrepBlob,
    pub(crate) entities: BTreeMap<ArchiveSlot, Name>,
    pub(crate) ancestry: BTreeMap<Name, BTreeSet<ArchiveSlot>>,
    pub(crate) incidence: BTreeMap<ArchiveSlot, BTreeSet<ArchiveSlot>>,
}

impl ArchivedFeature {
    pub fn producer(&self) -> ObjectId {
        self.producer
    }
    pub fn blob(&self) -> &BrepBlob {
        &self.blob
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let bad = || CadError::topology("incomplete or contradictory generative archive");
        if self.entities.is_empty()
            || self.entities.keys().any(|s| s.is_root())
            || self.entities.keys().ne(self.incidence.keys())
        {
            return Err(bad());
        }
        for (slot, name) in &self.entities {
            if !self
                .ancestry
                .get(name)
                .is_some_and(|members| members.contains(slot))
            {
                return Err(bad());
            }
            let faces = &self.incidence[slot];
            if (name.kind != ferritecad_kernel::SubShapeKind::Face && faces.is_empty())
                || faces.iter().any(|face| {
                    self.entities
                        .get(face)
                        .is_none_or(|n| n.kind != ferritecad_kernel::SubShapeKind::Face)
                })
            {
                return Err(bad());
            }
            if name.kind == ferritecad_kernel::SubShapeKind::Face && !faces.is_empty() {
                return Err(bad());
            }
        }
        for (name, members) in &self.ancestry {
            if members
                .iter()
                .any(|slot| self.entities.get(slot).is_none_or(|n| n.kind != name.kind))
            {
                return Err(bad());
            }
        }
        Ok(())
    }
}

pub fn archive_feature<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    map: &TopologyMap,
    producer: ObjectId,
) -> Result<ArchivedFeature> {
    let named = map
        .feature(producer)
        .ok_or_else(|| CadError::topology("feature has no naming result"))?;
    let shape = named
        .shape()
        .ok_or_else(|| CadError::topology("feature has no shape"))?;
    let entities: Vec<_> = named.entities.keys().copied().collect();
    let (blob, slots) = kernel.encode_shape_with(shape, &entities)?;
    if slots.len() != entities.len() || slots.iter().collect::<BTreeSet<_>>().len() != slots.len() {
        return Err(CadError::topology(
            "kernel returned incomplete archive slots",
        ));
    }
    let slots: BTreeMap<_, _> = entities.into_iter().zip(slots).collect();
    let members = |set: &BTreeSet<SubShapeHandle>| -> Result<BTreeSet<ArchiveSlot>> {
        set.iter()
            .map(|s| {
                slots
                    .get(s)
                    .copied()
                    .ok_or_else(|| CadError::topology("name lies outside archived topology"))
            })
            .collect()
    };
    let archived = ArchivedFeature {
        producer,
        blob,
        entities: named.entities.iter().map(|(s, n)| (slots[s], *n)).collect(),
        ancestry: named
            .ancestry
            .iter()
            .map(|(n, set)| Ok((*n, members(set)?)))
            .collect::<Result<_>>()?,
        incidence: named
            .topology
            .adjacent_faces
            .iter()
            .map(|(s, set)| Ok((slots[s], members(set)?)))
            .collect::<Result<_>>()?,
    };
    archived.validate()?;
    Ok(archived)
}

pub fn restore_feature<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    archived: &ArchivedFeature,
    into: &mut TopologyMap,
) -> Result<()> {
    archived.validate()?;
    let slots: Vec<_> = archived.entities.keys().copied().collect();
    let (shape, restored) = kernel.decode_shape_with(&archived.blob, &slots)?;
    let result = (|| {
        if restored.len() != slots.len()
            || restored.iter().collect::<BTreeSet<_>>().len() != slots.len()
        {
            return Err(CadError::topology(
                "kernel restored incomplete or aliased topology",
            ));
        }
        let rebinding: BTreeMap<_, _> = slots.into_iter().zip(restored).collect();
        let mut entities = BTreeMap::new();
        let mut bindings = BTreeMap::<_, BTreeSet<_>>::new();
        for (slot, name) in &archived.entities {
            let sub = rebinding[slot];
            if sub.shape() != shape || sub.kind() != name.kind {
                return Err(CadError::topology(
                    "archive slot restored with another shape or kind",
                ));
            }
            entities.insert(sub, *name);
            bindings.entry(*name).or_default().insert(sub);
        }
        let rebind = |set: &BTreeSet<ArchiveSlot>| set.iter().map(|slot| rebinding[slot]).collect();
        let topology = ShapeTopology {
            adjacent_faces: archived
                .incidence
                .iter()
                .map(|(slot, faces)| (rebinding[slot], rebind(faces)))
                .collect(),
        };
        if kernel.shape_topology(shape)?.adjacent_faces != topology.adjacent_faces {
            return Err(CadError::topology(
                "archive incidence disagrees with restored B-Rep",
            ));
        }
        Ok(FeatureNaming {
            topology,
            entities,
            bindings,
            ancestry: archived
                .ancestry
                .iter()
                .map(|(name, set)| (*name, rebind(set)))
                .collect(),
        })
    })();
    match result {
        Ok(named) => {
            into.features.insert(archived.producer, named);
            Ok(())
        }
        Err(error) => {
            kernel.release(shape);
            Err(error)
        }
    }
}
