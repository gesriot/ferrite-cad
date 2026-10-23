// SPDX-License-Identifier: MIT
use std::collections::BTreeMap;

use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

use crate::{EditChange, Point2, Sketch, SketchGeometry};

/// Merge all endpoint moves against one reading before emitting each changed Line once.
pub fn move_sketch_vertices(
    id: ObjectId,
    sketch: &Sketch,
    moves: &[SketchVertex],
) -> Result<Vec<EditChange>> {
    let mut geometries = BTreeMap::<StableEntityId, SketchGeometry>::new();
    for vertex in moves {
        let curve = sketch
            .curves
            .iter()
            .find(|c| c.id == vertex.curve_id)
            .ok_or_else(|| {
                CadError::input(format!("sketch {id} has no curve {}", vertex.curve_id))
            })?;
        let SketchGeometry::Line { start, .. } = curve.geometry else {
            return Err(CadError::input("a polygon vertex must name a Line"));
        };
        let incoming: Vec<_> = sketch
            .curves
            .iter()
            .filter(|c| matches!(c.geometry, SketchGeometry::Line { end, .. } if end == start))
            .collect();
        let [before] = incoming.as_slice() else {
            return Err(CadError::input(format!(
                "vertex {} has no unique incoming Line",
                vertex.curve_id
            )));
        };
        let point = Point2::new(vertex.start_mm[0], vertex.start_mm[1])?;
        if let SketchGeometry::Line { start, .. } = geometries
            .entry(curve.id)
            .or_insert_with(|| curve.geometry.clone())
        {
            *start = point;
        }
        if let SketchGeometry::Line { end, .. } = geometries
            .entry(before.id)
            .or_insert_with(|| before.geometry.clone())
        {
            *end = point;
        }
    }
    Ok(sketch
        .curves
        .iter()
        .filter_map(|curve| {
            geometries
                .remove(&curve.id)
                .filter(|geometry| *geometry != curve.geometry)
                .map(|geometry| EditChange::SetCurveGeometry {
                    sketch: id,
                    curve: curve.id,
                    geometry,
                })
        })
        .collect())
}

/// Validate only persistence invariants for a form's proposed changes.
pub fn validate_sketch_changes(
    id: ObjectId,
    sketch: &Sketch,
    changes: &[EditChange],
) -> Result<()> {
    let mut payload = crate::ObjectPayload::Sketch(sketch.clone());
    for change in changes {
        if change.object() != id {
            return Err(CadError::input("change belongs to another sketch"));
        }
        change.apply(&mut payload)?;
    }
    payload.validate()
}

#[derive(Debug, Clone, PartialEq)]
pub struct SketchVertex {
    pub curve_id: StableEntityId,
    pub start_mm: [f64; 2],
}
