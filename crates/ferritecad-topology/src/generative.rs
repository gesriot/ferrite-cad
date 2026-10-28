// SPDX-License-Identifier: MIT
//! Eager names derived from complete operation provenance.
use std::collections::{BTreeMap, BTreeSet};

use crate::derivation::{self, Derivation, Derivations};
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
    pub derivations: Derivations,
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
    derivations: Derivations,
    pub(crate) operations: BTreeMap<ObjectId, NamingOperation>,
    pub(crate) planes: BTreeMap<ObjectId, ObjectId>,
    sweeps: BTreeMap<ObjectId, NamingOperation>,
    roots: BTreeMap<ObjectId, BTreeSet<NamingInput>>,
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
                let mut roots = BTreeSet::new();
                for curve in sketch.curves.iter().filter(|curve| !curve.construction) {
                    roots.insert(NamingInput::Curve(curve.id));
                    if !matches!(
                        curve.geometry,
                        ferritecad_document::SketchGeometry::Circle { .. }
                    ) {
                        roots.insert(NamingInput::CurveEnd {
                            curve: curve.id,
                            end: false,
                        });
                        roots.insert(NamingInput::CurveEnd {
                            curve: curve.id,
                            end: true,
                        });
                    }
                }
                self.roots.insert(object.id, roots);
            }
            match &object.payload {
                ObjectPayload::Extrude(_) => {
                    self.sweeps.insert(object.id, NamingOperation::Extrude);
                }
                ObjectPayload::Revolve(_) => {
                    self.sweeps.insert(object.id, NamingOperation::Revolve);
                }
                _ => {}
            }
            let operation = match &object.payload {
                ObjectPayload::Extrude(e) if e.previous.is_some() => {
                    boolean_operation(e.operation)?
                }
                ObjectPayload::Extrude(_) => NamingOperation::Extrude,
                ObjectPayload::Revolve(r) if r.previous.is_some() => {
                    boolean_operation(r.operation)?
                }
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

    /// Restrict sweep provenance to the geometrically resolved selection.
    pub fn select_profile(
        &mut self,
        owner: ObjectId,
        profile: &ferritecad_kernel::Profile,
    ) -> Result<()> {
        let saved = self
            .roots
            .get_mut(&owner)
            .ok_or_else(|| CadError::topology("sweep has no profile roots"))?;
        let curves: BTreeSet<_> = profile.segments().map(|s| s.label).collect();
        if curves
            .iter()
            .any(|c| !saved.contains(&NamingInput::Curve(*c)))
        {
            return Err(CadError::topology("selected profile has a foreign curve"));
        }
        saved.retain(|input| match input {
            NamingInput::Curve(c) | NamingInput::CurveEnd { curve: c, .. } => curves.contains(c),
            _ => false,
        });
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
        let history = kernel.naming_history(shape)?;
        let expected = self.operations.get(&owner).copied();
        if history.operation != expected
            && !(expected.is_some_and(is_boolean)
                && history.operation == self.sweeps.get(&owner).copied()
                && inputs.is_empty())
        {
            return Err(CadError::topology(
                "operation provenance disagrees with the feature",
            ));
        }
        if inputs.is_empty() {
            let expected = self
                .roots
                .get(&owner)
                .ok_or_else(|| CadError::topology("sweep has no profile roots"))?;
            let found: BTreeSet<_> = history
                .relations
                .iter()
                .map(|(input, _, _)| *input)
                .filter(|i| !matches!(i, NamingInput::Cap { .. }))
                .collect();
            if &found != expected {
                return Err(CadError::topology(
                    "incomplete or foreign profile provenance",
                ));
            }
            if history.operation == Some(NamingOperation::Extrude)
                && [false, true].iter().any(|end| {
                    !history
                        .relations
                        .iter()
                        .any(|(i, _, _)| *i == NamingInput::Cap { end: *end })
                })
            {
                return Err(CadError::topology("extrusion provenance is missing a cap"));
            }
        }
        derive(
            shape,
            owner,
            self.planes.get(&owner).copied(),
            inputs,
            kernel,
            history,
        )
    }

    /// Publishes a feature's names after checking its graph against the model.
    pub fn insert_feature(&mut self, owner: ObjectId, named: FeatureNaming) -> Result<()> {
        self.check_derivations(owner, &named.derivations)?;
        for (digest, node) in &named.derivations {
            self.derivations
                .entry(*digest)
                .or_insert_with(|| node.clone());
        }
        self.features.insert(owner, named);
        Ok(())
    }

    pub(crate) fn check_derivations(&self, producer: ObjectId, graph: &Derivations) -> Result<()> {
        let mut lineage = BTreeSet::new();
        let mut cursor = Some(producer);
        while let Some(owner) = cursor {
            if !self.operations.contains_key(&owner) || !lineage.insert(owner) {
                return Err(CadError::topology("invalid naming producer lineage"));
            }
            cursor = self.previous.get(&owner).copied();
        }
        for (digest, node) in graph {
            if self
                .derivations
                .get(digest)
                .is_some_and(|prior| **prior != **node)
            {
                return Err(CadError::topology(format!(
                    "unequal derivations collide at {digest}; refusing to merge names"
                )));
            }
            let valid = match &**node {
                Derivation::Curve(owner, curve, plane) => {
                    lineage.contains(owner)
                        && self.planes.get(owner).copied() == *plane
                        && self
                            .roots
                            .get(owner)
                            .is_some_and(|r| r.contains(&NamingInput::Curve(*curve)))
                }
                Derivation::Cap(owner, _, plane) => {
                    lineage.contains(owner)
                        && self.roots.contains_key(owner)
                        && self.planes.get(owner).copied() == *plane
                }
                Derivation::Expression {
                    feature,
                    tag,
                    extra,
                    ..
                } => {
                    feature.is_none_or(|f| lineage.contains(&f))
                        && (!matches!(*tag, "generated" | "modified")
                            || feature.is_some_and(|f| {
                                self.operations.get(&f).is_some_and(|op| {
                                    op.tag() == *extra
                                        || (is_boolean(*op)
                                            && self
                                                .sweeps
                                                .get(&f)
                                                .is_some_and(|s| *extra == s.tag()))
                                })
                            }))
                }
            };
            if !valid {
                return Err(CadError::topology(
                    "cached derivation has a foreign model root",
                ));
            }
        }
        Ok(())
    }

    pub fn cache_key(&self, owner: ObjectId, geometry: ContentHash) -> ContentHash {
        let mut h = CanonicalHasher::new("naming.archive");
        h.algorithm_version(1)
            .hash(&geometry)
            .bytes(&owner.to_bytes());
        if let Some(plane) = self.planes.get(&owner) {
            h.bytes(&plane.to_bytes());
        }
        h.finish()
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
    history: ferritecad_kernel::NamingHistory,
) -> Result<FeatureNaming> {
    let topology = kernel.shape_topology(shape)?;
    let operation = history
        .operation
        .ok_or_else(|| CadError::topology("operation history has no operator identity"))?;
    validate_history(shape, &topology, &history, inputs)?;
    let mut derivations = Derivations::new();
    for source in inputs {
        for (digest, node) in &source.derivations {
            derivation::insert(&mut derivations, *digest, node.clone())?;
        }
    }
    let derived: BTreeMap<_, _> = inputs.iter().map(|f| (f.shape(), *f)).collect();
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
            NamingInput::Curve(curve) => (
                derivation::intern(&mut derivations, Derivation::Curve(owner, *curve, plane))?,
                None,
            ),
            NamingInput::CurveEnd { curve, end } => {
                let root =
                    derivation::intern(&mut derivations, Derivation::Curve(owner, *curve, plane))?;
                (
                    derivation::expression(
                        &mut derivations,
                        SubShapeKind::Vertex,
                        "curve-end",
                        None,
                        [root],
                        u8::from(*end),
                    )?,
                    None,
                )
            }
            NamingInput::Cap { end } => (
                derivation::intern(&mut derivations, Derivation::Cap(owner, *end, plane))?,
                None,
            ),
        };
        let term = match relation {
            NamingRelation::Modified if modified[input] > 1 => derivation::expression(
                &mut derivations,
                output.kind(),
                "modified",
                Some(owner),
                [root],
                operation.tag(),
            )?,
            NamingRelation::Kept | NamingRelation::Modified => {
                kept.entry(*output)
                    .or_insert_with(BTreeSet::new)
                    .insert(root);
                root
            }
            NamingRelation::Generated => derivation::expression(
                &mut derivations,
                output.kind(),
                "generated",
                Some(owner),
                [root],
                operation.tag(),
            )?,
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
                    let witnesses: BTreeSet<_> =
                        if let Some(boundary) = history.cap_boundaries.get(&face) {
                            boundary
                                .iter()
                                .map(|curve| {
                                    derivation::intern(
                                        &mut derivations,
                                        Derivation::Curve(owner, *curve, plane),
                                    )
                                })
                                .collect::<Result<_>>()?
                        } else {
                            topology
                                .adjacent_faces
                                .iter()
                                .filter(|(_, adjacent)| adjacent.contains(&face))
                                .flat_map(|(boundary, _)| {
                                    terms.get(boundary).into_iter().flatten().copied()
                                })
                                .collect()
                        };
                    let refined = derivation::expression(
                        &mut derivations,
                        SubShapeKind::Face,
                        "split-boundary-history",
                        Some(owner),
                        witnesses.into_iter().chain([base]),
                        0,
                    )?;
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
                    derivation::expression(
                        &mut derivations,
                        kind,
                        "merge",
                        Some(owner),
                        provenance,
                        0,
                    )?
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
                let incidence = derivation::expression(
                    &mut derivations,
                    kind,
                    "incident-faces",
                    None,
                    adjacent,
                    0,
                )?;
                // Provenance is a witness, never a traversal ordinal or a coordinate.
                derivation::expression(
                    &mut derivations,
                    kind,
                    "boundary",
                    Some(owner),
                    provenance.into_iter().chain([incidence]),
                    0,
                )?
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
        derivations,
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

pub(crate) fn hash_name(
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

fn validate_history(
    shape: ShapeHandle,
    topology: &ShapeTopology,
    history: &ferritecad_kernel::NamingHistory,
    inputs: &[&FeatureNaming],
) -> Result<()> {
    let bad = || CadError::topology("incomplete or contradictory operation provenance");
    if topology.adjacent_faces.is_empty() {
        return Err(bad());
    }
    for (entity, faces) in &topology.adjacent_faces {
        if entity.shape() != shape
            || !matches!(
                entity.kind(),
                SubShapeKind::Face | SubShapeKind::Edge | SubShapeKind::Vertex
            )
            || (entity.kind() == SubShapeKind::Face) != faces.is_empty()
            || faces
                .iter()
                .any(|f| f.kind() != SubShapeKind::Face || !topology.adjacent_faces.contains_key(f))
        {
            return Err(bad());
        }
    }
    let caps: BTreeSet<_> = history
        .relations
        .iter()
        .filter_map(|(input, _, output)| {
            matches!(input, NamingInput::Cap { .. })
                .then_some(*output)
                .flatten()
        })
        .collect();
    if caps.len() > 2
        && history
            .cap_boundaries
            .keys()
            .copied()
            .collect::<BTreeSet<_>>()
            != caps
    {
        return Err(bad());
    }
    for (face, curves) in &history.cap_boundaries {
        if !caps.contains(face) || curves.is_empty() || !inputs.is_empty() {
            return Err(bad());
        }
        for curve in curves {
            if !history.relations.iter().any(|(input, relation, output)| {
                *input == NamingInput::Curve(*curve)
                    && *relation == NamingRelation::Generated
                    && output.is_some_and(|edge| {
                        edge.kind() == SubShapeKind::Edge
                            && topology
                                .adjacent_faces
                                .get(&edge)
                                .is_some_and(|f| f.contains(face))
                    })
            }) {
                return Err(bad());
            }
        }
    }
    let source: BTreeSet<_> = inputs
        .iter()
        .flat_map(|f| f.entities.keys().copied())
        .collect();
    let mut survival = BTreeMap::<_, BTreeMap<NamingRelation, usize>>::new();
    for (input, relation, output) in &history.relations {
        if (*relation == NamingRelation::Deleted) != output.is_none() {
            return Err(bad());
        }
        if let Some(output) = output
            && !topology.adjacent_faces.contains_key(output)
        {
            return Err(bad());
        }
        if let NamingInput::SubShape(sub) = input {
            if !source.contains(sub) {
                return Err(bad());
            }
            if *relation != NamingRelation::Generated {
                if output.is_some_and(|o| o.kind() != sub.kind()) {
                    return Err(bad());
                }
                *survival
                    .entry(*sub)
                    .or_default()
                    .entry(*relation)
                    .or_default() += 1;
            }
        } else if !inputs.is_empty()
            || !matches!(
                relation,
                NamingRelation::Generated | NamingRelation::Deleted
            )
        {
            return Err(bad());
        }
    }
    for sub in source {
        let account = survival.get(&sub).ok_or_else(bad)?;
        if account.len() != 1
            || account
                .iter()
                .any(|(relation, n)| *relation != NamingRelation::Modified && *n != 1)
        {
            return Err(bad());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "provenance_tests.rs"]
mod tests;

fn is_boolean(operation: NamingOperation) -> bool {
    matches!(
        operation,
        NamingOperation::Cut | NamingOperation::Fuse | NamingOperation::Common
    )
}
fn boolean_operation(operation: ferritecad_document::SolidOperation) -> Result<NamingOperation> {
    use ferritecad_document::SolidOperation as S;
    match operation {
        S::Add => Ok(NamingOperation::Fuse),
        S::Cut => Ok(NamingOperation::Cut),
        S::Intersect => Ok(NamingOperation::Common),
        _ => Err(CadError::input("boolean operation needs a target")),
    }
}
