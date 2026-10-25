// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_kernel::{ArchiveSlot, BrepBlob, GeometryKernel, ShapeTopology, SubShapeHandle};
use ferritecad_types::{CadError, ObjectId, Result};

use crate::ModelNaming;
use crate::generative::{FeatureNaming, Name};

/// One validated atomic B-Rep and typed naming graph. Both constructors validate
/// the complete record; restore still verifies every slot against the live B-Rep.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedFeature {
    pub(crate) derivations: crate::derivation::Derivations,
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
    pub fn entities(&self) -> impl Iterator<Item = (ArchiveSlot, Name)> + '_ {
        self.entities.iter().map(|(s, n)| (*s, *n))
    }
    pub fn blob(&self) -> &BrepBlob {
        &self.blob
    }

    pub(crate) fn validate(&self) -> Result<()> {
        crate::derivation::validate(&self.derivations)?;
        let bad = || CadError::topology("incomplete or contradictory generative archive");
        if self.entities.is_empty()
            || self.entities.keys().any(|s| s.is_root())
            || self.entities.keys().ne(self.incidence.keys())
        {
            return Err(bad());
        }
        for name in self.entities.values().chain(self.ancestry.keys()) {
            let output = self.derivations.get(&name.digest).is_some_and(|node| {
                node.kind() == name.kind
                    && matches!(
                        &**node,
                        crate::derivation::Derivation::Expression { tag, .. } if *tag != "curve-end"
                    )
            });
            if !output {
                return Err(bad());
            }
        }
        for (digest, node) in &self.derivations {
            if matches!(
                &**node,
                crate::derivation::Derivation::Expression {
                    tag: "generated" | "boundary" | "split-boundary-history",
                    ..
                }
            ) && !self.ancestry.contains_key(&Name {
                kind: node.kind(),
                digest: *digest,
            }) {
                return Err(bad());
            }
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
    map: &ModelNaming,
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
        derivations: named.derivations.clone(),
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
    into: &mut ModelNaming,
) -> Result<()> {
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
        let named = FeatureNaming {
            derivations: archived.derivations.clone(),
            topology,
            entities,
            bindings,
            ancestry: archived
                .ancestry
                .iter()
                .map(|(name, set)| (*name, rebind(set)))
                .collect(),
        };
        into.insert_feature(archived.producer, named)
    })();
    if result.is_err() {
        kernel.release(shape);
    }
    result
}

#[cfg(test)]
#[path = "archive_tests.rs"]
pub(crate) mod tests;
