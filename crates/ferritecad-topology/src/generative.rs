// SPDX-License-Identifier: MIT
//! Eager names derived from complete operation provenance.
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::{ObjectPayload, ObjectRecord};
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
        const REASON: &str = "identical input provenance and named face incidence; \
                              no history witness separates the descendants";
        self.bindings
            .iter()
            .filter(|(_, entities)| entities.len() > 1)
            .map(|(name, entities)| (*name, entities.len(), REASON))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelNaming {
    pub(crate) operations: BTreeMap<ObjectId, NamingOperation>,
    pub(crate) planes: BTreeMap<ObjectId, ObjectId>,
    pub(crate) previous: BTreeMap<ObjectId, ObjectId>,
    pub features: BTreeMap<ObjectId, FeatureNaming>,
}

impl ModelNaming {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn prepare(&mut self, objects: &BTreeMap<ObjectId, ObjectRecord>) -> Result<()> {
        for object in objects.values() {
            let profile = match &object.payload {
                ObjectPayload::Extrude(e) => Some(e.profile),
                ObjectPayload::Revolve(r) => Some(r.profile),
                _ => None,
            };
            if let Some(profile) = profile {
                let Some(ObjectPayload::Sketch(sketch)) = objects.get(&profile).map(|o| &o.payload)
                else {
                    return Err(CadError::input("feature profile has no plane"));
                };
                self.planes.insert(object.id, sketch.plane);
            }
            let operation = match &object.payload {
                ObjectPayload::Extrude(e) if e.previous.is_some() => NamingOperation::Cut,
                ObjectPayload::Extrude(_) => NamingOperation::Extrude,
                ObjectPayload::Revolve(_) => NamingOperation::Revolve,
                ObjectPayload::Fillet(_) => NamingOperation::Fillet,
                ObjectPayload::Chamfer(_) => NamingOperation::Chamfer,
                _ => continue,
            };
            self.operations.insert(object.id, operation);
            if let Some(previous) = object.payload.previous_feature() {
                self.previous.insert(object.id, previous);
            }
        }
        Ok(())
    }

    pub fn feature(&self, id: ObjectId) -> Option<&FeatureNaming> {
        self.features.get(&id)
    }
    pub fn producers(&self) -> impl ExactSizeIterator<Item = ObjectId> + '_ {
        self.features.keys().copied()
    }
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }
    pub fn remove_feature(&mut self, id: ObjectId) {
        self.features.remove(&id);
    }

    pub fn name_operation(
        &self,
        owner: ObjectId,
        shape: ShapeHandle,
        inputs: &[&FeatureNaming],
        kernel: &mut (impl GeometryKernel + ?Sized),
    ) -> Result<FeatureNaming> {
        derive(
            shape,
            owner,
            self.planes.get(&owner).copied(),
            inputs,
            kernel,
        )
    }
}

impl FeatureNaming {
    pub fn named_face_count(&self) -> usize {
        self.topology.entities(SubShapeKind::Face).count()
    }
    pub fn shape(&self) -> Option<ShapeHandle> {
        self.entities.keys().next().map(|s| s.shape())
    }
}

fn derive(
    shape: ShapeHandle,
    owner: ObjectId,
    plane: Option<ObjectId>,
    inputs: &[&FeatureNaming],
    kernel: &mut (impl GeometryKernel + ?Sized),
) -> Result<FeatureNaming> {
    let topology = kernel.shape_topology(shape)?;
    let history = kernel.naming_history(shape)?;
    let operation = history
        .operation
        .ok_or_else(|| CadError::topology("operation history has no operator identity"))?;
    let derived: BTreeMap<_, _> = inputs.iter().map(|f| (f.shape(), *f)).collect();
    for source in inputs {
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
    // An entity modified into exactly one output keeps its name, so names born later do not
    // depend on how many unrelated features touched their neighbours.
    let mut modified = BTreeMap::<NamingInput, usize>::new();
    for (input, relation, _) in &history.relations {
        if *relation == NamingRelation::Modified {
            *modified.entry(*input).or_default() += 1;
        }
    }
    let mut ancestry: BTreeMap<Name, BTreeSet<SubShapeHandle>> = inputs
        .iter()
        .flat_map(|source| source.ancestry.keys().copied())
        .map(|name| (name, BTreeSet::new()))
        .collect();
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
                let source = derived
                    .get(&Some(sub.shape()))
                    .ok_or_else(|| CadError::topology("history names an unknown input shape"))?;
                let root = *source
                    .entities
                    .get(sub)
                    .ok_or_else(|| CadError::topology("history names an unknown input entity"))?;
                (root, Some((source, sub)))
            }
            NamingInput::Curve(curve) => (curve_root(owner, *curve, plane), None),
            NamingInput::CurveEnd { curve, end } => {
                let root = curve_root(owner, *curve, plane);
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
            NamingInput::Cap { end } => (cap_root(owner, *end, plane), None),
        };
        let term = match relation {
            NamingRelation::Modified if modified[input] > 1 => hash_name(
                output.kind(),
                "modified",
                Some(owner),
                [root],
                operation.tag(),
            ),
            NamingRelation::Kept | NamingRelation::Modified => {
                kept.entry(*output)
                    .or_insert_with(BTreeSet::new)
                    .insert(root);
                root
            }
            NamingRelation::Generated => generated(owner, operation, root, output.kind()),
            NamingRelation::Deleted => {
                return Err(CadError::topology("deleted history has an output"));
            }
        };
        terms.entry(*output).or_default().insert(term);
        if *relation == NamingRelation::Generated {
            ancestry.entry(term).or_default().insert(*output);
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
    Ok(FeatureNaming {
        topology,
        entities,
        bindings,
        ancestry,
    })
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
