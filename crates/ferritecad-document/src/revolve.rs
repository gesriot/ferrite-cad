// SPDX-License-Identifier: MIT
//! What a saved full-turn Revolve is, as discovery reports it (§27A).
//!
//! Read from the same pinned `objects()` every other catalogue answers about.
//! Nothing here edits a Revolve: this slice creates one and reports it, and
//! every older editor refuses it on its own terms.

use ferritecad_types::{ObjectId, StableEntityId};

use crate::{
    Document, FullTurnRevolution, ObjectPayload, ObjectRecord, RevolveAxis, RevolveExtent,
    SketchGeometry, SolidOperation,
};

/// One saved Line of a Revolve's profile, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct RevolveSegment {
    pub curve_id: StableEntityId,
    pub start_mm: [f64; 2],
    pub end_mm: [f64; 2],
}

/// One saved Revolve, and whether its profile is the class this build turns.
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
    /// The profile's Lines in stored order, when every curve is a Line.
    pub segments: Option<Vec<RevolveSegment>>,
    /// Why the stored profile is outside the §27A class, if it is.
    pub profile_refusal: Option<String>,
}

pub fn revolve_choices(_document: &Document, objects: &[ObjectRecord]) -> Vec<RevolveChoice> {
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
            let profile_refusal = match (sketch, &segments) {
                (None, _) => Some("the Revolve's profile is not a Sketch".to_owned()),
                (Some(s), _) if !s.constraints.is_empty() => {
                    Some("the Revolve's profile carries constraints".to_owned())
                }
                (Some(_), None) => Some(
                    "the Revolve's profile holds something other than non-construction Lines"
                        .to_owned(),
                ),
                (Some(_), Some(lines)) => {
                    let closed = lines
                        .iter()
                        .enumerate()
                        .all(|(i, l)| l.end_mm == lines[(i + 1) % lines.len()].start_mm);
                    if !closed {
                        Some("the Revolve's Lines are not a closed sequence".to_owned())
                    } else {
                        FullTurnRevolution::new(lines.iter().map(|l| l.start_mm).collect())
                            .err()
                            .map(|e| e.to_string())
                    }
                }
            };
            Some(RevolveChoice {
                feature: object.id,
                name: object.name.clone(),
                body,
                profile_sketch: revolve.profile,
                plane: sketch.map(|s| s.plane),
                axis: revolve.axis,
                extent: revolve.extent,
                operation: revolve.operation,
                segments,
                profile_refusal,
            })
        })
        .collect()
}
