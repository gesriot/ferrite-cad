// SPDX-License-Identifier: MIT
use ferritecad_document::{
    Document, EditChange, EndCondition, EntityKind, EntityReference, FeatureAddition,
    ObjectPayload, Point2, ReferenceMeaning, Selection, Sketch, SketchCurve, SketchGeometry,
};
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

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
                    regions: Vec::new(),
                    operation: ferritecad_document::SolidOperation::Cut,
                    profile: sketch,
                    end_condition,
                    reversed: false,
                },
            },
        ],
    })
}
/// The fixture explicitly specifies the birth derivation of its chosen extrusion edge.
/// No geometry search or production corner translator participates in selection.
pub fn plate_edge(document: &Document, body: ObjectId, corner: usize) -> Result<EntityReference> {
    let (feature, _, sketch) = base_profile(document, body)?;
    let count = sketch.curves.len();
    let before = sketch.curves[(corner + count - 1) % count].id;
    let after = sketch.curves[corner % count].id;
    Ok(extruded_fixture_edge(feature, sketch.plane, before, after))
}

pub fn extruded_fixture_edge(
    feature: ObjectId,
    plane: ObjectId,
    before: StableEntityId,
    after: StableEntityId,
) -> EntityReference {
    use ferritecad_types::{CanonicalHasher, ContentHash};
    use std::collections::BTreeSet;
    let expression = |kind: &str,
                      tag: &str,
                      owner: Option<ObjectId>,
                      inputs: Vec<(&str, ContentHash)>,
                      extra: u8| {
        let mut hash = CanonicalHasher::new("naming.expression");
        hash.bytes(tag.as_bytes())
            .bytes(kind.as_bytes())
            .bytes(&[extra]);
        if let Some(owner) = owner {
            hash.bytes(&owner.to_bytes());
        }
        // These fixture expressions have same-kind operands, so digest order is canonical.
        for (kind, digest) in inputs.into_iter().collect::<BTreeSet<_>>() {
            hash.bytes(kind.as_bytes()).hash(&digest);
        }
        hash.finish()
    };
    let mut faces = Vec::new();
    let mut witnesses = Vec::new();
    for (curve, end) in [(before, 1), (after, 0)] {
        let mut hash = CanonicalHasher::new("naming.curve");
        hash.bytes(&feature.to_bytes())
            .bytes(&curve.to_bytes())
            .bytes(&plane.to_bytes());
        let root = hash.finish();
        faces.push((
            "face",
            expression("face", "generated", Some(feature), vec![("edge", root)], 0),
        ));
        let endpoint = expression("vertex", "curve-end", None, vec![("edge", root)], end);
        witnesses.push((
            "edge",
            expression(
                "edge",
                "generated",
                Some(feature),
                vec![("vertex", endpoint)],
                0,
            ),
        ));
    }
    let incident = expression("edge", "incident-faces", None, faces, 0);
    witnesses.push(("edge", incident));
    EntityReference::new(ReferenceMeaning {
        producer: feature,
        kind: EntityKind::Edge,
        digest: expression("edge", "boundary", Some(feature), witnesses, 0),
        selection: Selection::One,
    })
}

pub fn edge_addition(
    document: &Document,
    body: ObjectId,
    corner: usize,
    amount: f64,
    chamfer: bool,
) -> Result<EditChange> {
    let edge = plate_edge(document, body, corner)?;
    let amount = ferritecad_document::Expression::constant(amount)?;
    Ok(EditChange::AddFeature {
        object: ObjectId::new(),
        body,
        name: Some(if chamfer { "Chamfer" } else { "Fillet" }.into()),
        feature: if chamfer {
            FeatureAddition::Chamfer {
                edges: vec![edge],
                distance_mm: amount,
            }
        } else {
            FeatureAddition::Fillet {
                edges: vec![edge],
                radius_mm: amount,
            }
        },
    })
}
