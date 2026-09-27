// SPDX-License-Identifier: MIT
//! Changing the radius of the one saved Fillet, in a new copy (§28B).
//!
//! The feature stays the feature: the same UUID, the same producer and joint,
//! the same seven names. The only model change is `radius_mm` in its payload.
//! The history is read again from the saved document on every discovery,
//! preparation and write, through the one frame reader the Cut editors and
//! §28A share ([`crate::cut_edit::saved_history_under_fillet`]); the radius
//! answers to §28A's policy ([`crate::FilletCorner::check_radius`]).
use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};

use crate::cut_edit::{same_meaning, saved_history_under_fillet};
use crate::fillet::{fillet_references, rectangle_corners};
use crate::{Document, Fillet, FilletCorner, FilletEdge, ObjectPayload, ObjectRecord};

/// The saved Fillet a radius edit changes, exactly as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedFillet {
    pub feature: ObjectId,
    pub body: ObjectId,
    /// The base Extrude: the feature consumed and the producer of the edge.
    pub previous: ObjectId,
    pub edge: FilletEdge,
    /// The corner the edge was swept from, with its labels and bound.
    pub corner: FilletCorner,
    pub profile: ObjectId,
    pub height_mm: f64,
    pub radius_mm: f64,
}

impl SavedFillet {
    /// The radius policy this Fillet's corner answers to.
    pub fn check_radius(&self, radius_mm: f64) -> Result<()> {
        self.corner.check_radius(radius_mm)
    }
}

/// One saved Fillet of a reading: editable, or the reason it is not.
#[derive(Debug, Clone, PartialEq)]
pub struct FilletRadiusChoice {
    pub feature: ObjectId,
    pub name: Option<String>,
    /// The stored payload, reported whatever the frame says.
    pub stored: Fillet,
    pub saved: Option<SavedFillet>,
    pub refusal: Option<String>,
}

impl FilletRadiusChoice {
    /// The same check preparation applies, with no SQLite or kernel work.
    pub fn validate(&self, radius_mm: f64) -> Result<()> {
        let saved = self
            .saved
            .as_ref()
            .ok_or_else(|| CadError::unsupported(self.refusal.clone().unwrap_or_default()))?;
        saved.check_radius(radius_mm)
    }
}

/// The class, read from the saved history around one Fillet object.
fn saved_fillet(
    document: &Document,
    objects: &[ObjectRecord],
    fillet: &ObjectRecord,
) -> Result<SavedFillet> {
    let ObjectPayload::Fillet(stored) = &fillet.payload else {
        return Err(CadError::input(format!(
            "object {} is {}, not a Fillet",
            fillet.id,
            fillet.payload.type_name()
        )));
    };
    let fillets = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
        .count();
    if fillets != 1 {
        return Err(CadError::unsupported(format!(
            "this slice edits the radius of the one Fillet of a plate, and this document holds \
             {fillets}"
        )));
    }
    let history = saved_history_under_fillet(document, objects, fillet)?;
    if !history.cuts.is_empty() {
        return Err(CadError::unsupported(
            "this slice edits the Fillet of a plate with no Cut",
        ));
    }
    let target = history.target;
    if stored.previous != target.base_feature || stored.edge.feature != target.base_feature {
        return Err(CadError::unsupported(
            "this slice edits a Fillet that rounds an edge of the plate's own Extrude",
        ));
    }
    let profile = objects
        .iter()
        .find(|o| o.id == target.profile)
        .ok_or_else(|| CadError::unsupported("the part's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(CadError::unsupported("the part's profile is not a Sketch"));
    };
    let corners = rectangle_corners(target.base_feature, sketch)?;
    let corner = crate::corner_for(&corners, stored.edge)?;
    corner.check_radius(stored.radius_mm)?;
    // Exactly the seven names §28A gives this Fillet, by meaning, and no other
    // name owned by it. Their UUIDs are what the copy keeps.
    let wanted = fillet_references(
        fillet.id,
        target.base_feature,
        stored.edge.joint,
        &target.profile_segments,
    );
    let mut owned: Vec<_> = document
        .topology_refs()?
        .into_iter()
        .filter(|r| r.owner == fillet.id || r.producer_feature == fillet.id)
        .collect();
    for want in &wanted {
        let at = owned
            .iter()
            .position(|r| same_meaning(r, want))
            .ok_or_else(|| {
                CadError::unsupported(
                    "the saved Fillet does not name the faces this build gives a Fillet",
                )
            })?;
        owned.remove(at);
    }
    if !owned.is_empty() {
        return Err(CadError::unsupported(
            "the saved Fillet names more faces than this build gives a Fillet",
        ));
    }
    Ok(SavedFillet {
        feature: fillet.id,
        body: target.body,
        previous: stored.previous,
        edge: stored.edge,
        corner,
        profile: target.profile,
        height_mm: target.height_mm,
        radius_mm: stored.radius_mm,
    })
}

/// One row per saved Fillet, each editable or with its reason, from one
/// snapshot. Kernel-free.
pub fn fillet_radius_choices(
    document: &Document,
    objects: &[ObjectRecord],
) -> Vec<FilletRadiusChoice> {
    objects
        .iter()
        .filter_map(|o| match &o.payload {
            ObjectPayload::Fillet(stored) => Some((o, stored.clone())),
            _ => None,
        })
        .map(|(o, stored)| {
            let saved = saved_fillet(document, objects, o).map_err(|e| e.to_string());
            FilletRadiusChoice {
                feature: o.id,
                name: o.name.clone(),
                stored,
                refusal: saved.as_ref().err().cloned(),
                saved: saved.ok(),
            }
        })
        .collect()
}

/// A checked radius edit: the selected Fillet row with only its radius
/// replaced, the saved facts it was read with, and the complete version of
/// the document it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedFilletRadius {
    pub(crate) feature: ObjectRecord,
    pub(crate) saved: SavedFillet,
    pub(crate) source_version: ContentHash,
}

impl PreparedFilletRadius {
    pub fn feature(&self) -> &ObjectRecord {
        &self.feature
    }
    /// The Fillet as saved, before the edit.
    pub fn saved(&self) -> &SavedFillet {
        &self.saved
    }
    pub fn radius_mm(&self) -> f64 {
        match &self.feature.payload {
            ObjectPayload::Fillet(f) => f.radius_mm,
            _ => f64::NAN,
        }
    }
    /// The Line UUIDs of the rounded corner, in canonical order.
    pub fn joint(&self) -> [StableEntityId; 2] {
        self.saved.edge.joint.segments()
    }
}

pub fn prepare_fillet_radius(
    document: &Document,
    feature: ObjectId,
    radius_mm: f64,
) -> Result<PreparedFilletRadius> {
    let objects = document.objects()?;
    let mut record = objects
        .iter()
        .find(|o| o.id == feature)
        .cloned()
        .ok_or_else(|| {
            CadError::input(format!("feature {feature} does not exist in this document"))
        })?;
    let saved = saved_fillet(document, &objects, &record)?;
    saved.check_radius(radius_mm)?;
    let ObjectPayload::Fillet(fillet) = &mut record.payload else {
        unreachable!("checked Fillet")
    };
    fillet.radius_mm = radius_mm;
    Ok(PreparedFilletRadius {
        feature: record,
        saved,
        source_version: document.content_version()?,
    })
}

/// The writer's check, against the snapshot the write consumes: the same
/// document version, and the same prepared value derived again from it.
pub(crate) fn rederive(document: &Document, prepared: &PreparedFilletRadius) -> Result<()> {
    if document.content_version()? != prepared.source_version {
        return Err(CadError::input(
            "document changed after Fillet radius preparation",
        ));
    }
    let ObjectPayload::Fillet(fillet) = &prepared.feature.payload else {
        return Err(CadError::input(
            "prepared Fillet radius edit must carry a Fillet",
        ));
    };
    let checked = prepare_fillet_radius(document, prepared.feature.id, fillet.radius_mm)?;
    if checked != *prepared {
        return Err(CadError::input(
            "prepared Fillet radius edit does not describe the current document",
        ));
    }
    Ok(())
}
