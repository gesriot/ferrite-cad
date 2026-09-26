// SPDX-License-Identifier: MIT
//! What a saved full-turn Revolve is, as discovery reports it (§27A).
//!
//! Read from the same pinned `objects()` every other catalogue answers about.
//! Its profile's coordinates are edited through the shared coordinate editor
//! (§27B); the class check both use, [`stated_revolution`], lives here.

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
    /// The profile's Lines in stored order, when every curve is a Line. For
    /// a constrained profile these are the stored inputs to the solver, not
    /// the solved drawing (§27G).
    pub segments: Option<Vec<RevolveSegment>>,
    /// Why the stored profile is outside the §27A/§27C classes, if it is.
    pub profile_refusal: Option<String>,
    /// §27C: the Line the Revolve states lies on the axis, for a solid part;
    /// `None` for a part with a bore. As stored, whether or not it agrees with
    /// the profile — `profile_refusal` says when it does not.
    pub axis_segment: Option<StableEntityId>,
}

/// The one check every reader of a saved Revolve applies: the profile's own
/// class, derived from its coordinates, must be the one the Revolve states.
///
/// `lines` are the profile's saved Lines in stored order, as `(curve UUID,
/// start)`. A Revolve saved with a bore whose profile now closes on the axis,
/// one saved solid whose profile no longer does, and one whose axis Line is
/// a different Line all refuse: each would add, lose or move a face that saved
/// names depend on. Used by discovery, the evaluator, coordinate-edit
/// preparation and the writer's re-derivation, so none of them decides alone.
pub fn stated_revolution(
    stated_axis: Option<StableEntityId>,
    lines: &[(StableEntityId, [f64; 2])],
) -> ferritecad_types::Result<FullTurnRevolution> {
    let turn = FullTurnRevolution::new(lines.iter().map(|(_, p)| *p).collect())?;
    let derived = turn.axis_line().map(|i| lines[i].0);
    match (stated_axis, derived) {
        (None, None) => Ok(turn),
        (Some(stated), Some(found)) if stated == found => Ok(turn),
        (None, Some(found)) => Err(ferritecad_types::CadError::input(format!(
            "this Revolve is a part with a bore, but its profile closes on the axis along Line \
             {found}: a part cannot change between hollow and solid, because that adds or \
             removes faces its saved names depend on"
        ))),
        (Some(stated), None) => Err(ferritecad_types::CadError::input(format!(
            "this Revolve is a solid part closed on the axis along Line {stated}, but its profile \
             no longer touches the axis: a part cannot change between solid and hollow, because \
             that adds or removes faces its saved names depend on"
        ))),
        (Some(stated), Some(found)) => Err(ferritecad_types::CadError::input(format!(
            "this Revolve closes on the axis along Line {stated}, but the profile lies on the axis \
             along Line {found}: the axis Line cannot change"
        ))),
    }
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
            // §27G: constraints are not a refusal of their own. The stored
            // Lines are the solver's starting geometry and answer to the same
            // class check; the solved Lines answer to it again in the
            // evaluator, on every rebuild.
            let profile_refusal = match (sketch, &segments) {
                (None, _) => Some("the Revolve's profile is not a Sketch".to_owned()),
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
                        let lines: Vec<_> =
                            lines.iter().map(|l| (l.curve_id, l.start_mm)).collect();
                        stated_revolution(revolve.axis_segment, &lines)
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
                axis_segment: revolve.axis_segment,
            })
        })
        .collect()
}
