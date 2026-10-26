// SPDX-License-Identifier: MIT
use ferritecad_document::{
    Document, EditChange, EndCondition, FeatureAddition, ObjectPayload, Point2, Sketch,
    SketchCurve, SketchGeometry, SweptEdge,
};
use ferritecad_types::{CadError, ObjectId, ProfileJoint, Result, StableEntityId};

pub fn body(document: &Document) -> Result<ObjectId> {
    document
        .objects()?
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .map(|o| o.id)
        .ok_or_else(|| CadError::input("fixture has no Body"))
}
pub fn base_profile(document: &Document, body: ObjectId) -> Result<(ObjectId, ObjectId, Sketch)> {
    let objects = document.objects()?;
    let ObjectPayload::Body(b) = &objects
        .iter()
        .find(|o| o.id == body)
        .ok_or_else(|| CadError::input("fixture Body missing"))?
        .payload
    else {
        return Err(CadError::input("fixture Body kind"));
    };
    let mut next = b.tip_feature;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = next {
        if !seen.insert(id) {
            break;
        }
        let o = objects
            .iter()
            .find(|o| o.id == id)
            .ok_or_else(|| CadError::input("fixture predecessor missing"))?;
        next = o.payload.previous_feature();
        if next.is_none()
            && let ObjectPayload::Extrude(e) = &o.payload
        {
            let ObjectPayload::Sketch(s) = &objects
                .iter()
                .find(|o| o.id == e.profile)
                .ok_or_else(|| CadError::input("fixture profile missing"))?
                .payload
            else {
                break;
            };
            return Ok((o.id, e.profile, s.clone()));
        }
    }
    Err(CadError::input("fixture has no base extrusion"))
}
#[derive(Debug)]
pub struct CircularTool {
    pub feature: ObjectId,
    pub sketch: ObjectId,
    pub curve: StableEntityId,
    pub changes: Vec<EditChange>,
}
pub fn circular_tool(
    document: &Document,
    body: ObjectId,
    center: [f64; 2],
    radius: f64,
    end_condition: EndCondition,
) -> Result<CircularTool> {
    let (_, _, base) = base_profile(document, body)?;
    let feature = ObjectId::new();
    let sketch = ObjectId::new();
    let curve = StableEntityId::new();
    Ok(CircularTool {
        feature,
        sketch,
        curve,
        changes: vec![
            EditChange::AddSketch {
                object: sketch,
                name: Some("Cut tool".into()),
                sketch: Sketch {
                    plane: base.plane,
                    curves: vec![SketchCurve {
                        id: curve,
                        construction: false,
                        geometry: SketchGeometry::Circle {
                            center: Point2::new(center[0], center[1])?,
                            radius,
                        },
                    }],
                    constraints: vec![],
                },
            },
            EditChange::AddFeature {
                object: feature,
                body,
                name: Some("Cut".into()),
                feature: FeatureAddition::Extrude {
                    operation: ferritecad_document::SolidOperation::Cut,
                    profile: sketch,
                    end_condition,
                    reversed: false,
                },
            },
        ],
    })
}
pub fn swept_edge(document: &Document, body: ObjectId, corner: usize) -> Result<SweptEdge> {
    let (feature, _, sketch) = base_profile(document, body)?;
    let count = sketch.curves.len();
    Ok(SweptEdge {
        feature,
        joint: ProfileJoint::new(
            sketch.curves[(corner + count - 1) % count].id,
            sketch.curves[corner % count].id,
        )?,
    })
}

pub fn edge_addition(
    document: &Document,
    body: ObjectId,
    corner: usize,
    amount: f64,
    chamfer: bool,
) -> Result<EditChange> {
    let edge = swept_edge(document, body, corner)?;
    let amount = ferritecad_document::Expression::constant(amount)?;
    Ok(EditChange::AddFeature {
        object: ObjectId::new(),
        body,
        name: Some(if chamfer { "Chamfer" } else { "Fillet" }.into()),
        feature: if chamfer {
            FeatureAddition::Chamfer {
                edge,
                distance_mm: amount,
            }
        } else {
            FeatureAddition::Fillet {
                edge,
                radius_mm: amount,
            }
        },
    })
}
