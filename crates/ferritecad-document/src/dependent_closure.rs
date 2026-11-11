// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_types::{CadError, ObjectId, Result};

use crate::{DatumPlane, DependencyRole, Document, EndCondition, KeepSelection, ObjectPayload};

impl Document {
    /// Explicit removal set, ordered by object identity. History links are spliced except
    /// when removing a root leaves the entire remaining chain without a predecessor.
    pub fn dependent_closure(&self, object: ObjectId) -> Result<Vec<ObjectId>> {
        let objects: BTreeMap<_, _> = self.objects()?.into_iter().map(|o| (o.id, o)).collect();
        if !objects.contains_key(&object) {
            return Err(CadError::input(format!("unknown object {object}")));
        }
        let mut users = BTreeMap::<ObjectId, BTreeSet<ObjectId>>::new();
        for edge in self.dependencies()? {
            match edge.role {
                DependencyRole::Profile
                | DependencyRole::Plane
                | DependencyRole::Support
                | DependencyRole::Parameter => {
                    users
                        .entry(edge.dependency)
                        .or_default()
                        .insert(edge.dependent);
                }
                DependencyRole::Predecessor | DependencyRole::BodyTip => {}
            }
        }
        for row in objects.values() {
            if let Some(parent) = row.parent {
                users.entry(parent).or_default().insert(row.id);
            }
            let mut root = row.id;
            let mut seen = BTreeSet::new();
            while let Some(previous) = objects[&root].payload.previous_feature() {
                if !seen.insert(root) || !objects.contains_key(&previous) {
                    return Err(CadError::input(
                        "the stored history is incomplete or cyclic",
                    ));
                }
                root = previous;
            }
            if root != row.id {
                users.entry(root).or_default().insert(row.id);
            }
            let mut reference = |r: &crate::EntityReference| {
                users.entry(r.meaning.producer).or_default().insert(row.id);
            };
            match &row.payload {
                ObjectPayload::DatumPlane(DatumPlane::Face { face, .. }) => reference(face),
                ObjectPayload::Extrude(feature) => match &feature.end_condition {
                    EndCondition::UpToFace { face, .. } => reference(face),
                    EndCondition::UpToVertex { vertex, .. } => reference(vertex),
                    EndCondition::Blind { .. }
                    | EndCondition::Symmetric { .. }
                    | EndCondition::ThroughAll
                    | EndCondition::UpToNext => {}
                },
                ObjectPayload::Fillet(feature) => feature.edges.iter().for_each(reference),
                ObjectPayload::Chamfer(feature) => feature.edges.iter().for_each(reference),
                ObjectPayload::KeepSolids(feature) => match &feature.selection {
                    KeepSelection::All => {}
                    KeepSelection::ContainingFaces(faces) => {
                        faces.iter().for_each(reference);
                    }
                },
                ObjectPayload::Body(body) => {
                    let mut next = body.tip_feature;
                    let mut chain = BTreeSet::new();
                    while let Some(id) = next {
                        if !chain.insert(id) {
                            break;
                        }
                        let feature = objects.get(&id).ok_or_else(|| {
                            CadError::input(format!("missing history feature {id}"))
                        })?;
                        next = feature.payload.previous_feature();
                    }
                    users.entry(row.id).or_default().extend(chain);
                }
                ObjectPayload::Parameter(_)
                | ObjectPayload::Sketch(_)
                | ObjectPayload::DatumPlane(DatumPlane::Fixed { .. } | DatumPlane::Offset { .. })
                | ObjectPayload::Revolve(_)
                | ObjectPayload::ImportedStep(_) => {}
            }
        }
        let mut removed = BTreeSet::new();
        let mut pending = BTreeSet::from([object]);
        while let Some(id) = pending.pop_first() {
            if !removed.insert(id) {
                continue;
            }
            pending.extend(users.get(&id).into_iter().flatten().copied());
        }
        Ok(removed.into_iter().collect())
    }
}
