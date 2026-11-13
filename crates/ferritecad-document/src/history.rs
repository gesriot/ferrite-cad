// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_types::{CadError, ObjectId, Result};

use crate::{Body, ObjectPayload, ObjectRecord};

pub(crate) fn check_rollback(
    body: &Body,
    label: &str,
    marker: Option<ObjectId>,
    lookup: impl Fn(ObjectId) -> Option<(Option<ObjectId>, String)>,
) -> Result<()> {
    let Some(marker) = marker else { return Ok(()) };
    let feature = lookup(marker)
        .map(|(_, name)| name)
        .unwrap_or_else(|| format!("feature {marker}"));
    if body.tip_feature.is_none() {
        return Err(CadError::input(format!(
            "{label} is empty and cannot roll back to {feature}."
        )));
    }
    if body.tip_feature == Some(marker) {
        return Err(CadError::input(format!(
            "{feature} is the tip of {label}; use clear_rollback to show the end of its history."
        )));
    }
    let mut next = body.tip_feature;
    let mut seen = BTreeSet::new();
    while let Some(id) = next {
        if id == marker {
            return Ok(());
        }
        if !seen.insert(id) {
            break;
        }
        next = lookup(id).and_then(|(previous, _)| previous);
    }
    Err(CadError::input(format!(
        "{feature} is not in the history of {label}."
    )))
}

pub(crate) fn check_markers(objects: &[ObjectRecord]) -> Result<()> {
    let by_id: BTreeMap<_, _> = objects.iter().map(|o| (o.id, o)).collect();
    for object in objects {
        if let ObjectPayload::Body(body) = &object.payload {
            let label = |o: &ObjectRecord| o.payload.label(o.name.as_deref(), o.id);
            check_rollback(body, &label(object), body.rollback, |id| {
                by_id
                    .get(&id)
                    .map(|o| (o.payload.previous_feature(), label(o)))
            })?;
        }
    }
    Ok(())
}
