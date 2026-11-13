// SPDX-License-Identifier: MIT
//! Kernel-free presentation of stored history, never a claim of DAG ownership.

use std::collections::{BTreeMap, BTreeSet};

use ferritecad_types::ObjectId;

use crate::{Dependency, DependencyRole, ObjectPayload, ObjectRecord, SolidOperation};

/// One occurrence: canonical object, history reference in a Body, or profile
/// reference in a feature. Fields: object, optional Body, optional profile
/// consumer. Contexts are stored UUIDs, never row positions.
pub type ModelRowId = (ObjectId, Option<ObjectId>, Option<ObjectId>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTreeRow {
    pub id: ModelRowId,
    pub parent: Option<ModelRowId>,
    pub depth: usize,
    pub name: String,
    pub kind: String,
    pub note: String,
    /// Evaluated scene fact; the stored-only projection has no solid count.
    pub solid_count: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelTree {
    pub unbuilt_features: usize,
    pub rows: Vec<ModelTreeRow>,
}

impl ModelTree {
    /// Keep the existing edit catalogue's refusal policy. An unreadable DAG
    /// can still list stored objects, but cannot claim a construction history.
    pub fn from_dependencies(
        objects: &[ObjectRecord],
        deps: ferritecad_types::Result<Vec<Dependency>>,
    ) -> Self {
        match deps {
            Ok(deps) => Self::project(objects, &deps),
            Err(reason) => {
                let mut tree = Self::project(objects, &[]);
                for row in &mut tree.rows {
                    row.note = format!("History unavailable: {reason}. {}", row.note);
                }
                tree
            }
        }
    }

    pub fn project(objects: &[ObjectRecord], deps: &[Dependency]) -> Self {
        let by_id: BTreeMap<_, _> = objects.iter().map(|o| (o.id, o)).collect();
        let dependencies = |id, role| {
            deps.iter()
                .filter(|d| d.dependent == id && d.role == role)
                .map(|d| d.dependency)
                .collect::<BTreeSet<_>>()
        };
        let mut rows = Vec::new();
        let mut in_history = BTreeSet::new();
        for body in by_id
            .values()
            .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
        {
            let root = (body.id, None, None);
            rows.push(row(body, root, None, 0, "Body history references"));
            let mut chain = Vec::new();
            let mut seen = BTreeSet::new();
            let mut next = dependencies(body.id, DependencyRole::BodyTip);
            // Valid native history is a chain. A branch is reported, never
            // flattened into a fictional order. All its objects remain below.
            while next.len() == 1 {
                let id = *next.first().expect("one dependency");
                if !seen.insert(id) {
                    rows.last_mut().expect("body row").note = "History cycle; see Objects".into();
                    chain.clear();
                    break;
                }
                let Some(object) = by_id.get(&id) else {
                    rows.last_mut().expect("body row").note =
                        format!("Missing history reference {id}");
                    chain.clear();
                    break;
                };
                chain.push(*object);
                next = dependencies(id, DependencyRole::Predecessor);
            }
            if next.len() > 1 {
                rows.last_mut().expect("body row").note =
                    "Branched history; see Objects and dependencies".into();
                // Do not advertise the partial tail as a complete history.
                chain.clear();
            }
            let ObjectPayload::Body(part) = &body.payload else {
                unreachable!("Body projection")
            };
            let marker = part
                .rollback
                .and_then(|id| chain.iter().position(|o| o.id == id));
            if let Some(later) = marker {
                let shown = chain[later];
                let name = shown.name.clone().unwrap_or_else(|| shown.id.to_string());
                let row = rows.last_mut().expect("Body row");
                row.name.push_str(" · rolled back");
                let count = if later == 1 {
                    "1 later feature is not shown.".to_owned()
                } else {
                    format!("{later} later features are not shown.")
                };
                row.note = format!("Shows the result of {name}; {count}");
            }
            let mut not_shown = false;
            for feature in chain.into_iter().rev() {
                let occurrence = (feature.id, Some(body.id), None);
                in_history.insert(feature.id);
                rows.push(row(feature, occurrence, Some(root), 1, "History reference"));
                if not_shown {
                    rows.last_mut()
                        .expect("feature row")
                        .name
                        .push_str(" · not shown");
                }
                if marker.is_some() && part.rollback == Some(feature.id) {
                    not_shown = true;
                }
                for profile in dependencies(feature.id, DependencyRole::Profile) {
                    if let Some(object) = by_id.get(&profile) {
                        rows.push(row(
                            object,
                            (profile, Some(body.id), Some(feature.id)),
                            Some(occurrence),
                            2,
                            "Profile reference; also listed in Objects",
                        ));
                    }
                }
            }
        }
        for object in by_id
            .values()
            .filter(|o| !matches!(o.payload, ObjectPayload::Body(_)) && !in_history.contains(&o.id))
        {
            rows.push(row(object, (object.id, None, None), None, 0, match object.payload {
                ObjectPayload::ImportedStep(_) => "Imported source; definitions and visibility remain in the viewport inspector",
                _ => "Stored object; references do not imply ownership",
            }));
        }
        Self {
            rows,
            unbuilt_features: 0,
        }
    }
}

fn row(
    object: &ObjectRecord,
    id: ModelRowId,
    parent: Option<ModelRowId>,
    depth: usize,
    note: &str,
) -> ModelTreeRow {
    let kind = match &object.payload {
        ObjectPayload::Extrude(e) if e.operation == SolidOperation::Cut => "Cut".to_owned(),
        other => other.type_name().to_owned(),
    };
    ModelTreeRow {
        id,
        parent,
        depth,
        name: object.name.clone().unwrap_or_else(|| "Unnamed".into()),
        kind,
        note: note.into(),
        solid_count: None,
    }
}
