// SPDX-License-Identifier: MIT
//! Resolve a profile corner through the two curve-generated face families.
use std::collections::BTreeSet;

use ferritecad_document::SweptEdge;
use ferritecad_kernel::{NamingOperation, SubShapeHandle, SubShapeKind};
use ferritecad_types::{CadError, ObjectId, Result};

use crate::generative::{ModelNaming, Name, curve_root, generated};

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
        let found = self.swept_edges(at, edge)?;
        one(found, &format!("swept edge {edge:?} at {at}"))
    }

    /// Every edge of `at` between the faces swept from the joint's two curves.
    pub fn swept_edges(&self, at: ObjectId, edge: SweptEdge) -> Result<BTreeSet<SubShapeHandle>> {
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
        Ok(self.boundary(at, SubShapeKind::Edge, &faces))
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
