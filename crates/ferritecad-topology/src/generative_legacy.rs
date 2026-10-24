// SPDX-License-Identifier: MIT
//! Experimental translation of the existing meanings, independent of their resolver.
use std::collections::BTreeSet;

use ferritecad_document::{
    CapSide, EntityKind, SelectionRule, SemanticRole, SweptEdge, TopologyRef,
};
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
            .map(|curve| self.family(at, curve_root(edge.feature, curve, Some(plane))))
            .collect();
        let found = self.boundary(at, SubShapeKind::Edge, &faces);
        one(found, &format!("swept edge {edge:?} at {at}"))
    }

    /// Translates role meanings into root ancestry and named face incidence;
    /// does not consult TopologyMap, resolve(), geometry or traversal positions.
    pub fn resolve_legacy(&self, reference: &TopologyRef) -> Result<BTreeSet<SubShapeHandle>> {
        let at = reference.producer_feature;
        if let SelectionRule::AllDerivedFrom { ancestor } = reference.selection {
            let segment = match &reference.output_role {
                SemanticRole::ExtrudeSide { profile_segment }
                | SemanticRole::RevolveFace { profile_segment }
                | SemanticRole::CarriedSide { profile_segment }
                | SemanticRole::OriginSide {
                    profile_segment, ..
                } => Some(*profile_segment),
                _ => None,
            };
            if segment != Some(ancestor) {
                return Err(CadError::input(
                    "reference selection contradicts its named ancestor",
                ));
            }
        }

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
            return Err(CadError::topology(
                "reference names a different operation kind",
            ));
        }

        let curve = |origin, id| -> Result<_> {
            Ok(self.family(at, curve_root(origin, id, Some(self.plane_of(origin)?))))
        };
        let cap = |origin, side| -> Result<_> {
            Ok(self.family(
                at,
                cap_root(origin, end(side)?, Some(self.plane_of(origin)?)),
            ))
        };
        let previous = |id| {
            self.previous
                .get(&id)
                .copied()
                .ok_or_else(|| CadError::topology("reference producer has no predecessor"))
        };
        let rounded = |origin, edge| -> Result<_> {
            let before = previous(origin)?;
            let selected = self.swept_edge(before, edge)?;
            let input = self.features[&before].entities[&selected];
            Ok(self.family(
                at,
                generated(origin, self.operations[&origin], input, SubShapeKind::Face),
            ))
        };
        let found = match &reference.output_role {
            SemanticRole::ExtrudeCap { side } | SemanticRole::RevolveCap { side } => {
                cap(at, *side)?
            }
            SemanticRole::ExtrudeSide { profile_segment }
            | SemanticRole::RevolveFace { profile_segment } => curve(at, *profile_segment)?,
            SemanticRole::CarriedCap { side } => cap(previous(at)?, *side)?,
            SemanticRole::CarriedSide { profile_segment } => {
                curve(previous(at)?, *profile_segment)?
            }
            SemanticRole::OriginCap {
                origin_feature,
                side,
            } => cap(*origin_feature, *side)?,
            SemanticRole::OriginSide {
                origin_feature,
                profile_segment,
            } => curve(*origin_feature, *profile_segment)?,
            SemanticRole::ExtrudeCapEdge {
                side,
                profile_segment,
            } => self.boundary(
                at,
                SubShapeKind::Edge,
                &[cap(at, *side)?, curve(at, *profile_segment)?],
            ),
            SemanticRole::ExtrudeSweepEdge { joint } => BTreeSet::from([self.swept_edge(
                at,
                SweptEdge {
                    feature: at,
                    joint: *joint,
                },
            )?]),
            SemanticRole::ExtrudeCapVertex { side, joint } => {
                let mut faces = vec![cap(at, *side)?];
                for id in joint.segments() {
                    faces.push(curve(at, id)?);
                }
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
            )?,
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
            )?,
            _ => {
                return Err(CadError::unsupported(
                    "legacy meaning has no generative translation",
                ));
            }
        };
        let expected = match reference.expected_kind {
            EntityKind::Face => SubShapeKind::Face,
            EntityKind::Edge => SubShapeKind::Edge,
            EntityKind::Vertex => SubShapeKind::Vertex,
            _ => return Err(CadError::unsupported("unknown reference entity kind")),
        };
        let found: BTreeSet<_> = found.into_iter().filter(|s| s.kind() == expected).collect();
        if found.is_empty() {
            return Err(CadError::topology(format!(
                "reference {} has no descendants",
                reference.id
            )));
        }
        match reference.selection {
            SelectionRule::Exact => Ok(BTreeSet::from([one(found, &reference.id.to_string())?])),
            SelectionRule::AllDerivedFrom { .. } => Ok(found),
            _ => Err(CadError::unsupported("unknown reference selection")),
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
