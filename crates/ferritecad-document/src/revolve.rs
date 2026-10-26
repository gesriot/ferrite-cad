// SPDX-License-Identifier: MIT
//! Saved Revolve fields for CLI and window discovery.

use ferritecad_types::{ObjectId, StableEntityId};

use crate::{
    ObjectPayload, ObjectRecord, RevolveAxis, RevolveExtent, SketchGeometry, SolidOperation,
};

/// One saved Line of a Revolve's profile, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct RevolveSegment {
    pub curve_id: StableEntityId,
    pub start_mm: [f64; 2],
    pub end_mm: [f64; 2],
}

/// One saved Revolve and its editable profile coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct RevolveChoice {
    pub feature: ObjectId,
    pub name: Option<String>,
    /// The Body whose tip this Revolve is; `None` if no Body names it.
    pub body: Option<ObjectId>,
    pub profile_sketch: ObjectId,
    /// The datum the profile Sketch is drawn on, when it is one.
    pub plane: Option<ObjectId>,
    pub axis: RevolveAxis,
    pub extent: RevolveExtent,
    pub operation: SolidOperation,
    /// The profile's Lines in stored order, when every curve is a Line. For
    /// a constrained profile these are the stored inputs to the solver, not
    /// the solved drawing (§27G).
    pub segments: Option<Vec<RevolveSegment>>,
    /// Why the profile cannot be read, if it is missing.
    pub profile_refusal: Option<String>,
}

pub fn revolve_choices(objects: &[ObjectRecord]) -> Vec<RevolveChoice> {
    objects
        .iter()
        .filter_map(|object| {
            let ObjectPayload::Revolve(revolve) = &object.payload else {
                return None;
            };
            let body = objects.iter().find_map(|o| match &o.payload {
                ObjectPayload::Body(b) if b.tip_feature == Some(object.id) => Some(o.id),
                _ => None,
            });
            let sketch = objects.iter().find_map(|o| match &o.payload {
                ObjectPayload::Sketch(s) if o.id == revolve.profile => Some(s),
                _ => None,
            });
            let segments = sketch.and_then(|s| {
                s.curves
                    .iter()
                    .map(|c| match c.geometry {
                        SketchGeometry::Line { start, end } if !c.construction => {
                            Some(RevolveSegment {
                                curve_id: c.id,
                                start_mm: [start.x, start.y],
                                end_mm: [end.x, end.y],
                            })
                        }
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
            });
            let profile_refusal = sketch
                .is_none()
                .then(|| "the Revolve's profile is not a Sketch".into());
            Some(RevolveChoice {
                feature: object.id,
                name: object.name.clone(),
                body,
                profile_sketch: revolve.profile,
                plane: sketch.map(|s| s.plane),
                axis: revolve.axis,
                extent: revolve.extent.clone(),
                operation: revolve.operation,
                segments,
                profile_refusal,
            })
        })
        .collect()
}
