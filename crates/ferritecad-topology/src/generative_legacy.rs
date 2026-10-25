// SPDX-License-Identifier: MIT
//! Temporary translation of document roles through typed generative ancestry.
use std::collections::BTreeSet;

use ferritecad_document::{CapSide, SemanticRole, SweptEdge, TopologyRef};
use ferritecad_kernel::{NamingOperation, SubShapeHandle, SubShapeKind};
use ferritecad_types::{CadError, ObjectId, Result};

use crate::generative::{ModelNaming, Name, cap_root, curve_root, generated};

impl ModelNaming {
    fn plane_of(&self, feature: ObjectId) -> Result<ObjectId> {
        self.planes
            .get(&feature)
            .copied()
            .ok_or_else(|| CadError::input("naming root has no plane"))
    }

    fn family(&self, at: ObjectId, seed: Name) -> BTreeSet<SubShapeHandle> {
        self.features
            .get(&at)
            .and_then(|f| f.ancestry.get(&seed))
            .cloned()
            .unwrap_or_default()
    }

    fn boundary(
        &self,
        at: ObjectId,
        kind: SubShapeKind,
        faces: &[BTreeSet<SubShapeHandle>],
    ) -> BTreeSet<SubShapeHandle> {
        self.features
            .get(&at)
            .map(|f| {
                f.topology
                    .entities(kind)
                    .filter(|entity| {
                        faces
                            .iter()
                            .all(|set| !set.is_disjoint(&f.topology.adjacent_faces[entity]))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn swept_edge(&self, at: ObjectId, edge: SweptEdge) -> Result<SubShapeHandle> {
        let plane = self.plane_of(edge.feature)?;
        let faces: Vec<_> = edge
            .joint
            .segments()
            .into_iter()
            .map(|curve| {
                self.family(
                    at,
                    generated(
                        edge.feature,
                        NamingOperation::Extrude,
                        curve_root(edge.feature, curve, Some(plane)),
                        SubShapeKind::Face,
                    ),
                )
            })
            .collect();
        let found = self.boundary(at, SubShapeKind::Edge, &faces);
        one(found, &format!("swept edge {edge:?} at {at}"))
    }

    pub fn resolve_legacy(&self, reference: &TopologyRef) -> Result<BTreeSet<SubShapeHandle>> {
        crate::resolve(self, reference).map(|v| v.into_iter().collect())
    }
    pub(crate) fn legacy_removed(&self, reference: &TopologyRef) -> bool {
        self.legacy_seed(reference).is_some_and(|seed| {
            self.features
                .get(&reference.producer_feature)
                .and_then(|f| f.ancestry.get(&seed))
                .is_some_and(BTreeSet::is_empty)
        })
    }
    fn face_seed(
        &self,
        origin: ObjectId,
        curve: Option<ferritecad_types::StableEntityId>,
        side: CapSide,
    ) -> Option<Name> {
        let plane = self.planes.get(&origin).copied()?;
        let root = match curve {
            Some(curve) => curve_root(origin, curve, Some(plane)),
            None => cap_root(origin, end(side).ok()?, Some(plane)),
        };
        let operation = if self.operations.get(&origin) == Some(&NamingOperation::Revolve) {
            NamingOperation::Revolve
        } else {
            NamingOperation::Extrude
        };
        Some(generated(origin, operation, root, SubShapeKind::Face))
    }
    fn rounded_seed(&self, origin: ObjectId, edge: SweptEdge) -> Option<Name> {
        let before = *self.previous.get(&origin)?;
        let selected = self.swept_edge(before, edge).ok()?;
        let input = *self.features.get(&before)?.entities.get(&selected)?;
        Some(generated(
            origin,
            *self.operations.get(&origin)?,
            input,
            SubShapeKind::Face,
        ))
    }
    fn legacy_seed(&self, reference: &TopologyRef) -> Option<Name> {
        let at = reference.producer_feature;
        match reference.output_role {
            SemanticRole::CarriedCap { side } => {
                self.face_seed(*self.previous.get(&at)?, None, side)
            }
            SemanticRole::CarriedSide { profile_segment } => self.face_seed(
                *self.previous.get(&at)?,
                Some(profile_segment),
                CapSide::Start,
            ),
            SemanticRole::OriginCap {
                origin_feature,
                side,
            } => self.face_seed(origin_feature, None, side),
            SemanticRole::OriginSide {
                origin_feature,
                profile_segment,
            } => self.face_seed(origin_feature, Some(profile_segment), CapSide::Start),
            SemanticRole::OriginFilletFace {
                origin_feature,
                edge_feature,
                joint,
            } => self.rounded_seed(
                origin_feature,
                SweptEdge {
                    feature: edge_feature,
                    joint,
                },
            ),
            _ => None,
        }
    }
    pub(crate) fn legacy_candidates(&self, reference: &TopologyRef) -> BTreeSet<SubShapeHandle> {
        let at = reference.producer_feature;
        let operation = self.operations.get(&at).copied();
        let incompatible = match &reference.output_role {
            SemanticRole::ExtrudeCap { .. } | SemanticRole::ExtrudeSide { .. } => !matches!(
                operation,
                Some(NamingOperation::Extrude | NamingOperation::Cut)
            ),
            SemanticRole::ExtrudeCapEdge { .. }
            | SemanticRole::ExtrudeSweepEdge { .. }
            | SemanticRole::ExtrudeCapVertex { .. } => operation != Some(NamingOperation::Extrude),
            SemanticRole::RevolveCap { .. } | SemanticRole::RevolveFace { .. } => {
                operation != Some(NamingOperation::Revolve)
            }
            SemanticRole::EdgeFilletFace { .. } => operation != Some(NamingOperation::Fillet),
            SemanticRole::EdgeChamferFace { .. } => operation != Some(NamingOperation::Chamfer),
            _ => false,
        };
        if incompatible {
            return BTreeSet::new();
        }

        let curve = |origin, id| {
            self.face_seed(origin, Some(id), CapSide::Start)
                .map(|seed| self.family(at, seed))
                .unwrap_or_default()
        };
        let cap = |origin, side| {
            self.face_seed(origin, None, side)
                .map(|seed| self.family(at, seed))
                .unwrap_or_default()
        };
        let rounded = |origin, edge| {
            self.rounded_seed(origin, edge)
                .map(|seed| self.family(at, seed))
                .unwrap_or_default()
        };
        let previous = || self.previous.get(&at).copied();
        match &reference.output_role {
            SemanticRole::ExtrudeCap { side } | SemanticRole::RevolveCap { side } => cap(at, *side),
            SemanticRole::ExtrudeSide { profile_segment }
            | SemanticRole::RevolveFace { profile_segment } => curve(at, *profile_segment),
            SemanticRole::CarriedCap { side } => {
                previous().map(|p| cap(p, *side)).unwrap_or_default()
            }
            SemanticRole::CarriedSide { profile_segment } => previous()
                .map(|p| curve(p, *profile_segment))
                .unwrap_or_default(),
            SemanticRole::OriginCap {
                origin_feature,
                side,
            } => cap(*origin_feature, *side),
            SemanticRole::OriginSide {
                origin_feature,
                profile_segment,
            } => curve(*origin_feature, *profile_segment),
            SemanticRole::ExtrudeCapEdge {
                side,
                profile_segment,
            } => self.boundary(
                at,
                SubShapeKind::Edge,
                &[cap(at, *side), curve(at, *profile_segment)],
            ),
            SemanticRole::ExtrudeSweepEdge { joint } => {
                let faces: Vec<_> = joint.segments().iter().map(|id| curve(at, *id)).collect();
                let edges = self.boundary(at, SubShapeKind::Edge, &faces);
                // A segment pair meeting twice never defined a legacy ProfileJoint.
                if edges.len() == 1 {
                    edges
                } else {
                    BTreeSet::new()
                }
            }
            SemanticRole::ExtrudeCapVertex { side, joint } => {
                let mut faces: Vec<_> = joint
                    .segments()
                    .into_iter()
                    .map(|id| curve(at, id))
                    .collect();
                if self.boundary(at, SubShapeKind::Edge, &faces).len() != 1 {
                    return BTreeSet::new();
                }
                faces.push(cap(at, *side));
                self.boundary(at, SubShapeKind::Vertex, &faces)
            }
            SemanticRole::EdgeFilletFace {
                edge_feature,
                joint,
            }
            | SemanticRole::EdgeChamferFace {
                edge_feature,
                joint,
            } => rounded(
                at,
                SweptEdge {
                    feature: *edge_feature,
                    joint: *joint,
                },
            ),
            SemanticRole::OriginFilletFace {
                origin_feature,
                edge_feature,
                joint,
            } => rounded(
                *origin_feature,
                SweptEdge {
                    feature: *edge_feature,
                    joint: *joint,
                },
            ),
            _ => BTreeSet::new(),
        }
    }
}

fn end(side: CapSide) -> Result<bool> {
    match side {
        CapSide::Start => Ok(false),
        CapSide::End => Ok(true),
        _ => Err(CadError::unsupported("unknown cap side")),
    }
}

fn one(found: BTreeSet<SubShapeHandle>, description: &str) -> Result<SubShapeHandle> {
    if found.len() == 1 {
        Ok(*found.first().expect("one member"))
    } else {
        Err(CadError::topology(format!(
            "{description} has {} descendants; refusing to choose",
            found.len()
        )))
    }
}
