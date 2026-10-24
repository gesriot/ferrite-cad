// SPDX-License-Identifier: MIT
//! Isolated generative naming experiment; no document writer or evaluator hook.
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::{Document, ObjectPayload};
use ferritecad_kernel::{
    GeometryKernel, NamingInput, NamingOperation, NamingRelation, ShapeHandle, ShapeTopology,
    SubShapeHandle, SubShapeKind,
};
use ferritecad_types::{CadError, CanonicalHasher, ContentHash, ObjectId, Result, StableEntityId};

/// A compact Merkle name. Its derivation is reconstructed from the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Name {
    pub kind: SubShapeKind,
    pub digest: ContentHash,
}

#[derive(Debug, Clone)]
pub struct FeatureNaming {
    pub topology: ShapeTopology,
    pub entities: BTreeMap<SubShapeHandle, Name>,
    pub bindings: BTreeMap<Name, BTreeSet<SubShapeHandle>>,
    /// Derived ancestry sets, including the primary names of previous results.
    pub ancestry: BTreeMap<Name, BTreeSet<SubShapeHandle>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    One,
    AllDescendants,
}

impl FeatureNaming {
    pub fn resolve_set(
        &self,
        name: Name,
        selection: Selection,
    ) -> Result<&BTreeSet<SubShapeHandle>> {
        let found = self.ancestry.get(&name);
        if let Some(entities) = found
            && !entities.is_empty()
            && entities.iter().all(|s| s.kind() == name.kind)
            && (selection == Selection::AllDescendants || entities.len() == 1)
        {
            return Ok(entities);
        }
        Err(CadError::topology(format!(
            "name {} ({}) resolves to {} entities under {selection:?}; refusing to choose",
            name.digest,
            name.kind,
            found.map_or(0, BTreeSet::len)
        )))
    }

    pub fn resolve(&self, name: Name) -> Result<SubShapeHandle> {
        Ok(*self
            .resolve_set(name, Selection::One)?
            .first()
            .expect("exact resolution has one member"))
    }

    pub fn ambiguous(&self) -> impl Iterator<Item = (Name, usize, &'static str)> + '_ {
        self.bindings.iter().filter(|(_, entities)| entities.len() > 1)
            .map(|(name, entities)| (*name, entities.len(),
                "identical input provenance and named face incidence; no history witness separates the descendants"))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelNaming {
    pub(crate) operations: BTreeMap<ObjectId, NamingOperation>,
    pub(crate) planes: BTreeMap<ObjectId, ObjectId>,
    pub(crate) previous: BTreeMap<ObjectId, ObjectId>,
    pub features: BTreeMap<ObjectId, FeatureNaming>,
}

/// Names the feature results of an already rebuilt document. The caller supplies
/// its session's feature handles; no geometry is rebuilt or policy re-evaluated.
pub fn name_rebuilt(
    document: &Document,
    shapes: &BTreeMap<ObjectId, ShapeHandle>,
    kernel: &mut impl GeometryKernel,
) -> Result<ModelNaming> {
    let objects = document.objects()?;
    let mut owners = BTreeMap::new();
    let mut planes = BTreeMap::new();
    for object in &objects {
        let profile = match &object.payload {
            ObjectPayload::Extrude(e) => Some(e.profile),
            ObjectPayload::Revolve(r) => Some(r.profile),
            _ => None,
        };
        if let Some(profile) = profile {
            let plane = objects
                .iter()
                .find_map(|o| match &o.payload {
                    ObjectPayload::Sketch(s) if o.id == profile => Some(s.plane),
                    _ => None,
                })
                .ok_or_else(|| CadError::input("feature profile has no plane"))?;
            planes.insert(object.id, plane);
        }
        if matches!(
            object.payload,
            ObjectPayload::Extrude(_)
                | ObjectPayload::Revolve(_)
                | ObjectPayload::Fillet(_)
                | ObjectPayload::Chamfer(_)
        ) && let Some(shape) = shapes.get(&object.id)
        {
            owners.insert(*shape, object.id);
        }
    }
    let mut derived = BTreeMap::new();
    for (shape, owner) in &owners {
        derive(
            *shape,
            *owner,
            &owners,
            &planes,
            kernel,
            &mut derived,
            &mut BTreeSet::new(),
        )?;
    }
    Ok(ModelNaming {
        operations: objects
            .iter()
            .filter_map(|o| {
                Some((
                    o.id,
                    match &o.payload {
                        ObjectPayload::Extrude(e) if e.previous.is_some() => NamingOperation::Cut,
                        ObjectPayload::Extrude(_) => NamingOperation::Extrude,
                        ObjectPayload::Revolve(_) => NamingOperation::Revolve,
                        ObjectPayload::Fillet(_) => NamingOperation::Fillet,
                        ObjectPayload::Chamfer(_) => NamingOperation::Chamfer,
                        _ => return None,
                    },
                ))
            })
            .collect(),
        planes,
        previous: objects
            .iter()
            .filter_map(|o| o.payload.previous_feature().map(|p| (o.id, p)))
            .collect(),
        features: owners
            .into_iter()
            .map(|(shape, owner)| {
                Ok((
                    owner,
                    derived
                        .remove(&shape)
                        .ok_or_else(|| CadError::topology("feature has no naming result"))?,
                ))
            })
            .collect::<Result<_>>()?,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "the recursive pass shares one rebuild scope and session"
)]
fn derive(
    shape: ShapeHandle,
    owner: ObjectId,
    owners: &BTreeMap<ShapeHandle, ObjectId>,
    planes: &BTreeMap<ObjectId, ObjectId>,
    kernel: &mut impl GeometryKernel,
    derived: &mut BTreeMap<ShapeHandle, FeatureNaming>,
    visiting: &mut BTreeSet<ShapeHandle>,
) -> Result<()> {
    if derived.contains_key(&shape) {
        return Ok(());
    }
    if !visiting.insert(shape) {
        return Err(CadError::topology("cyclic operation provenance"));
    }
    let topology = kernel.shape_topology(shape)?;
    let history = kernel.naming_history(shape)?;
    let operation = history
        .operation
        .ok_or_else(|| CadError::topology("operation history has no operator identity"))?;
    let inputs: BTreeSet<_> = history
        .relations
        .iter()
        .filter_map(|(input, _, _)| match input {
            NamingInput::SubShape(s) => Some(s.shape()),
            _ => None,
        })
        .collect();
    for input in &inputs {
        derive(
            *input,
            owners.get(input).copied().unwrap_or(owner),
            owners,
            planes,
            kernel,
            derived,
            visiting,
        )?;
        let source = &derived[input];
        for sub in source.entities.keys() {
            if !history
                .relations
                .iter()
                .any(|(input, _, _)| *input == NamingInput::SubShape(*sub))
            {
                return Err(CadError::topology(format!("incomplete history for {sub}")));
            }
        }
    }
    let mut ancestry: BTreeMap<Name, BTreeSet<SubShapeHandle>> = BTreeMap::new();
    let mut terms: BTreeMap<SubShapeHandle, BTreeSet<Name>> = BTreeMap::new();
    let mut kept = BTreeMap::new();
    for (input, relation, output) in &history.relations {
        let Some(output) = output else { continue };
        if !topology.adjacent_faces.contains_key(output) {
            return Err(CadError::topology(
                "history points outside the result inventory",
            ));
        }
        let (root, source) = match input {
            NamingInput::SubShape(sub) => {
                let source = &derived[&sub.shape()];
                let root = *source
                    .entities
                    .get(sub)
                    .ok_or_else(|| CadError::topology("history names an unknown input entity"))?;
                (root, Some((source, sub)))
            }
            NamingInput::Curve(curve) => {
                (curve_root(owner, *curve, planes.get(&owner).copied()), None)
            }
            NamingInput::CurveEnd { curve, end } => {
                let root = curve_root(owner, *curve, planes.get(&owner).copied());
                (
                    hash_name(
                        SubShapeKind::Vertex,
                        "curve-end",
                        None,
                        [root],
                        u8::from(*end),
                    ),
                    None,
                )
            }
            NamingInput::Cap { end } => (cap_root(owner, *end, planes.get(&owner).copied()), None),
        };
        let term = match relation {
            NamingRelation::Kept => {
                kept.entry(*output)
                    .or_insert_with(BTreeSet::new)
                    .insert(root);
                root
            }
            NamingRelation::Modified => hash_name(
                output.kind(),
                "modified",
                Some(owner),
                [root],
                operation.tag(),
            ),
            NamingRelation::Generated => generated(owner, operation, root, output.kind()),
            NamingRelation::Deleted => {
                return Err(CadError::topology("deleted history has an output"));
            }
        };
        terms.entry(*output).or_default().insert(term);
        if *relation == NamingRelation::Generated {
            ancestry.entry(term).or_default().insert(*output);
            if source.is_none() {
                ancestry.entry(root).or_default().insert(*output);
            }
        } else if let Some((source, sub)) = source {
            for (ancestor, members) in &source.ancestry {
                if members.contains(sub) {
                    ancestry.entry(*ancestor).or_default().insert(*output);
                }
            }
            ancestry.entry(root).or_default().insert(*output);
        }
    }
    let mut entities = BTreeMap::new();
    for kind in [SubShapeKind::Face, SubShapeKind::Edge, SubShapeKind::Vertex] {
        if kind == SubShapeKind::Edge {
            let mut groups: BTreeMap<Name, Vec<SubShapeHandle>> = BTreeMap::new();
            for (face, name) in &entities {
                groups.entry(*name).or_default().push(*face);
            }
            for (base, faces) in groups {
                if faces.len() < 2 {
                    continue;
                }
                for face in faces {
                    let witnesses: BTreeSet<_> = topology
                        .adjacent_faces
                        .iter()
                        .filter(|(_, adjacent)| adjacent.contains(&face))
                        .flat_map(|(boundary, _)| {
                            terms.get(boundary).into_iter().flatten().copied()
                        })
                        .collect();
                    let refined = hash_name(
                        SubShapeKind::Face,
                        "split-boundary-history",
                        Some(owner),
                        witnesses.into_iter().chain([base]),
                        0,
                    );
                    entities.insert(face, refined);
                }
            }
        }
        for entity in topology.entities(kind) {
            let provenance = terms.get(&entity).cloned().unwrap_or_default();
            let name = if let Some(original) = kept.get(&entity).filter(|names| names.len() == 1) {
                *original.first().expect("one kept meaning")
            } else if kind == SubShapeKind::Face {
                if provenance.is_empty() {
                    return Err(CadError::topology(format!(
                        "face {entity} has no operation provenance"
                    )));
                }
                if provenance.len() == 1 {
                    *provenance.first().expect("one term")
                } else {
                    hash_name(kind, "merge", Some(owner), provenance, 0)
                }
            } else {
                let adjacent = topology.adjacent_faces[&entity]
                    .iter()
                    .map(|face| {
                        entities
                            .get(face)
                            .copied()
                            .ok_or_else(|| CadError::topology("incident face is not named"))
                    })
                    .collect::<Result<BTreeSet<_>>>()?;
                if adjacent.is_empty() {
                    return Err(CadError::topology("entity has no incident named faces"));
                }
                let incidence = hash_name(kind, "incident-faces", None, adjacent, 0);
                // Provenance is a witness, never a traversal ordinal or a coordinate.
                hash_name(
                    kind,
                    "boundary",
                    Some(owner),
                    provenance.into_iter().chain([incidence]),
                    0,
                )
            };
            entities.insert(entity, name);
        }
    }
    let mut bindings: BTreeMap<Name, BTreeSet<SubShapeHandle>> = BTreeMap::new();
    for (entity, name) in &entities {
        bindings.entry(*name).or_default().insert(*entity);
        ancestry.entry(*name).or_default().insert(*entity);
    }
    visiting.remove(&shape);
    derived.insert(
        shape,
        FeatureNaming {
            topology,
            entities,
            bindings,
            ancestry,
        },
    );
    Ok(())
}

pub fn curve_root(feature: ObjectId, curve: StableEntityId, plane: Option<ObjectId>) -> Name {
    let mut h = CanonicalHasher::new("naming.curve");
    h.bytes(&feature.to_bytes()).bytes(&curve.to_bytes());
    if let Some(plane) = plane {
        h.bytes(&plane.to_bytes());
    }
    Name {
        kind: SubShapeKind::Edge,
        digest: h.finish(),
    }
}

pub fn cap_root(feature: ObjectId, end: bool, plane: Option<ObjectId>) -> Name {
    let mut h = CanonicalHasher::new("naming.cap");
    h.bytes(&feature.to_bytes()).bytes(&[u8::from(end)]);
    if let Some(plane) = plane {
        h.bytes(&plane.to_bytes());
    }
    Name {
        kind: SubShapeKind::Face,
        digest: h.finish(),
    }
}

pub fn generated(
    feature: ObjectId,
    operation: NamingOperation,
    input: Name,
    kind: SubShapeKind,
) -> Name {
    hash_name(kind, "generated", Some(feature), [input], operation.tag())
}

fn hash_name(
    kind: SubShapeKind,
    tag: &str,
    feature: Option<ObjectId>,
    inputs: impl IntoIterator<Item = Name>,
    extra: u8,
) -> Name {
    let mut h = CanonicalHasher::new("naming.expression");
    h.bytes(tag.as_bytes())
        .bytes(kind.as_str().as_bytes())
        .bytes(&[extra]);
    if let Some(feature) = feature {
        h.bytes(&feature.to_bytes());
    }
    for input in inputs.into_iter().collect::<BTreeSet<_>>() {
        h.bytes(input.kind.as_str().as_bytes()).hash(&input.digest);
    }
    Name {
        kind,
        digest: h.finish(),
    }
}
