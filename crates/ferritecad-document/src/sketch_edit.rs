// SPDX-License-Identifier: MIT
//! Coordinate-only editing of the persisted, ordered Line polygon.
use std::collections::BTreeSet;

use ferritecad_types::{CadError, ObjectId, Result, StableEntityId, Transform};

use crate::{
    Dependency, DependencyRole, Document, ObjectPayload, ObjectRecord, Point2, PolygonExtrusion,
    SketchGeometry, SolidOperation, editable_extrude,
};

/// The start of this persisted curve; its predecessor's end is the same vertex.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchVertex {
    pub curve_id: StableEntityId,
    pub start_mm: [f64; 2],
}

/// A row in objects() order. Refusal is local; copy_access still takes priority.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchChoice {
    pub sketch: ObjectId,
    pub name: Option<String>,
    /// None for an unsupported sketch, never an invented/reordered contour.
    pub vertices: Option<Vec<SketchVertex>>,
    pub height_mm: Option<f64>,
    pub refusal: Option<String>,
}

pub fn sketch_choices(document: &Document, objects: &[ObjectRecord]) -> Vec<SketchChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .map(|o| match supported(document, objects, o) {
            Ok((vertices, height)) => SketchChoice {
                sketch: o.id,
                name: o.name.clone(),
                vertices: Some(vertices),
                height_mm: Some(height),
                refusal: None,
            },
            Err(error) => SketchChoice {
                sketch: o.id,
                name: o.name.clone(),
                vertices: None,
                height_mm: None,
                refusal: Some(error.to_string()),
            },
        })
        .collect()
}

fn unsupported(message: &str) -> CadError {
    CadError::unsupported(message)
}

fn supported(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
) -> Result<(Vec<SketchVertex>, f64)> {
    // Refuse unknown fields/noncanonical envelopes in the one payload we will
    // rewrite, rather than silently discard bytes this reader did not retain.
    require_lossless_payload(object)?;
    // A bounded class, not an attempt to simplify a larger model. For a large
    // unsupported catalogue this returns before any per-object SQL reads.
    if objects.len() != 4 || objects.iter().any(|o| o.parent.is_some()) {
        return Err(unsupported(
            "sketch edit requires exactly one XY plane, Sketch, Blind Extrude and Body",
        ));
    }
    let ObjectPayload::Sketch(sketch) = &object.payload else {
        return Err(unsupported("selected object is not a Sketch"));
    };
    let plane = objects
        .iter()
        .find(|o| o.id == sketch.plane)
        .ok_or_else(|| unsupported("missing sketch plane"))?;
    if !matches!(&plane.payload, ObjectPayload::DatumPlane(p) if p.placement == Transform::IDENTITY)
    {
        return Err(unsupported(
            "sketch edit requires the untransformed XY plane",
        ));
    }
    let extrude = objects.iter().find(|o| matches!(&o.payload, ObjectPayload::Extrude(e)
        if e.profile == object.id && !e.reversed && e.operation == SolidOperation::NewBody && e.target_body.is_none()))
        .ok_or_else(|| unsupported("sketch edit requires one forward Blind Extrude/NewBody"))?;
    let body = objects
        .iter()
        .find(|o| matches!(&o.payload, ObjectPayload::Body(b) if b.tip_feature == Some(extrude.id)))
        .ok_or_else(|| unsupported("sketch edit requires the extrusion's single Body"))?;
    let height = editable_extrude(document, extrude)?;
    let expected = BTreeSet::from([
        Dependency {
            dependent: object.id,
            dependency: plane.id,
            role: DependencyRole::Plane,
        },
        Dependency {
            dependent: extrude.id,
            dependency: object.id,
            role: DependencyRole::Profile,
        },
        Dependency {
            dependent: body.id,
            dependency: extrude.id,
            role: DependencyRole::BodyTip,
        },
    ]);
    if document
        .dependencies()?
        .into_iter()
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err(unsupported(
            "sketch edit requires only plane, profile and body-tip dependencies",
        ));
    }
    if !sketch.constraints.is_empty()
        || sketch.curves.len() < 3
        || sketch.curves.len() > PolygonExtrusion::MAX_POINTS
    {
        return Err(unsupported(
            "sketch edit requires 3..256 unconstrained Line segments",
        ));
    }
    let mut vertices = Vec::new();
    let mut ends = Vec::new();
    let mut ids = BTreeSet::new();
    for curve in &sketch.curves {
        if curve.construction || !ids.insert(curve.id) {
            return Err(unsupported(
                "construction or duplicate curve IDs are unsupported",
            ));
        }
        let SketchGeometry::Line { start, end } = curve.geometry else {
            return Err(unsupported("sketch edit supports only Lines"));
        };
        vertices.push(SketchVertex {
            curve_id: curve.id,
            start_mm: [start.x, start.y],
        });
        ends.push([end.x, end.y]);
    }
    for (i, end) in ends.iter().enumerate() {
        if *end != vertices[(i + 1) % vertices.len()].start_mm {
            return Err(unsupported(
                "saved Lines must be an exactly closed sequence in stored order",
            ));
        }
    }
    PolygonExtrusion::new(vertices.iter().map(|v| v.start_mm).collect(), height)
        .map_err(|e| unsupported(&format!("saved polygon is outside edit policy: {e}")))?;
    Ok((vertices, height))
}

fn require_lossless_payload(object: &ObjectRecord) -> Result<()> {
    if object.payload.to_storage_bytes()?.as_slice() != object.storage_bytes() {
        return Err(unsupported(
            "selected Sketch storage cannot be rewritten losslessly by this reader",
        ));
    }
    Ok(())
}

/// Prepare only the selected payload. Every curve and object ID is retained.
pub fn replace_sketch_coordinates(
    document: &Document,
    selected: ObjectId,
    vertices: &[SketchVertex],
) -> Result<ObjectRecord> {
    let objects = document.objects()?;
    let mut object = objects
        .iter()
        .find(|o| o.id == selected)
        .cloned()
        .ok_or_else(|| CadError::input("selected Sketch UUID does not exist"))?;
    let (original, height) = supported(document, &objects, &object)?;
    let polygon = validate_coordinates(&original, height, vertices)?;
    let ObjectPayload::Sketch(sketch) = &mut object.payload else {
        unreachable!("checked Sketch")
    };
    for (i, curve) in sketch.curves.iter_mut().enumerate() {
        curve.geometry = SketchGeometry::Line {
            start: polygon.points()[i],
            end: polygon.points()[(i + 1) % vertices.len()],
        };
    }
    Ok(object)
}

impl SketchChoice {
    /// The same draft check used before copying. No SQLite or kernel work.
    pub fn validate_coordinates(&self, vertices: &[SketchVertex]) -> Result<PolygonExtrusion> {
        let original = self
            .vertices
            .as_deref()
            .ok_or_else(|| unsupported("unsupported Sketch choice"))?;
        let height = self
            .height_mm
            .ok_or_else(|| unsupported("missing extrusion height"))?;
        validate_coordinates(original, height, vertices)
    }
}
fn validate_coordinates(
    original: &[SketchVertex],
    height: f64,
    vertices: &[SketchVertex],
) -> Result<PolygonExtrusion> {
    if vertices.len() != original.len()
        || vertices
            .iter()
            .zip(original)
            .any(|(a, b)| a.curve_id != b.curve_id)
    {
        return Err(CadError::input(
            "request must contain every saved curve UUID exactly once in saved order",
        ));
    }
    let polygon = PolygonExtrusion::new(vertices.iter().map(|v| v.start_mm).collect(), height)?;
    let old: Vec<_> = original
        .iter()
        .map(|v| Point2 {
            x: v.start_mm[0],
            y: v.start_mm[1],
        })
        .collect();
    if winding(&old) != winding(polygon.points()) {
        return Err(CadError::input("sketch edit cannot change winding"));
    }
    Ok(polygon)
}

fn winding(points: &[Point2]) -> bool {
    let a = points[0];
    (1..points.len() - 1)
        .map(|i| {
            (points[i].x - a.x) * (points[i + 1].y - a.y)
                - (points[i].y - a.y) * (points[i + 1].x - a.x)
        })
        .sum::<f64>()
        > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_selected_payload_fields_refuse_instead_of_disappearing() {
        use crate::{Envelope, Sketch, SketchCurve};
        let root = tempfile::tempdir().expect("directory");
        let path = root.path().join("source.fcad");
        let mut d = Document::create(&path).expect("doc");
        let id = ObjectId::new();
        let payload = ObjectPayload::Sketch(Sketch {
            plane: ObjectId::new(),
            constraints: vec![],
            curves: vec![SketchCurve {
                id: StableEntityId::new(),
                construction: false,
                geometry: SketchGeometry::Line {
                    start: Point2::ORIGIN,
                    end: Point2::new(1., 0.).expect("point"),
                },
            }],
        });
        d.write(|w| w.put_object(id, None, 0, None, &payload))
            .expect("store");
        require_lossless_payload(&d.object(id).expect("row").expect("record"))
            .expect("ordinary encoding");
        d.close().expect("close");
        let mut envelope =
            Envelope::from_bytes(&payload.to_storage_bytes().expect("bytes")).expect("envelope");
        let mut value: ciborium::value::Value =
            ciborium::from_reader(envelope.payload.as_slice()).expect("CBOR");
        value
            .as_map_mut()
            .expect("map")
            .push(("future_detail".into(), "must not disappear".into()));
        envelope.payload.clear();
        ciborium::into_writer(&value, &mut envelope.payload).expect("CBOR");
        let bytes = envelope.to_bytes().expect("envelope");
        let raw = rusqlite::Connection::open(&path).expect("SQL");
        raw.execute(
            "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3",
            rusqlite::params![
                bytes,
                ferritecad_types::ContentHash::of_bytes(&bytes)
                    .as_bytes()
                    .as_slice(),
                id.to_bytes().as_slice()
            ],
        )
        .expect("future field");
        drop(raw);
        let before = std::fs::read(&path).expect("source");
        let d = Document::open_read_only(&path).expect("still readable");
        let record = d.object(id).expect("row").expect("record");
        let error = require_lossless_payload(&record).expect_err("must refuse loss");
        assert!(error.to_string().contains("losslessly"));
        d.close().expect("close");
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }
}
