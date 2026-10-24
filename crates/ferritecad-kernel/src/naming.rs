// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use crate::{SubShapeHandle, SubShapeKind};
use ferritecad_types::StableEntityId;

/// Complete elementary topology. Handles and incidence are session-local.
#[derive(Debug, Clone, Default)]
pub struct ShapeTopology {
    pub adjacent_faces: BTreeMap<SubShapeHandle, BTreeSet<SubShapeHandle>>,
}

impl ShapeTopology {
    pub fn entities(&self, kind: SubShapeKind) -> impl Iterator<Item = SubShapeHandle> + '_ {
        self.adjacent_faces
            .keys()
            .copied()
            .filter(move |s| s.kind() == kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NamingInput {
    SubShape(SubShapeHandle),
    Curve(StableEntityId),
    CurveEnd { curve: StableEntityId, end: bool },
    Cap { end: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NamingRelation {
    Kept,
    Modified,
    Generated,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingOperation {
    Extrude,
    Revolve,
    Cut,
    Fillet,
    Chamfer,
}

impl NamingOperation {
    pub fn tag(self) -> u8 {
        match self {
            Self::Extrude => 0,
            Self::Revolve => 1,
            Self::Cut => 2,
            Self::Fillet => 3,
            Self::Chamfer => 4,
        }
    }
}

/// Every face, edge and vertex of every input has an account. Deleted refers
/// to survival of the input; generated descendants may still exist.
#[derive(Debug, Clone, Default)]
pub struct NamingHistory {
    pub operation: Option<NamingOperation>,
    pub relations: BTreeSet<(NamingInput, NamingRelation, Option<SubShapeHandle>)>,
}
