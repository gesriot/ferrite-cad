// SPDX-License-Identifier: MIT
//! Stored geometry the existing forms can draw; Apply decides whether it builds.
use ferritecad_document::{
    EditChange, EndCondition, Expression, FeatureAddition, ObjectPayload, ObjectRecord, Point2,
    Sketch, SketchCurve, SketchGeometry, SweptEdge,
};
use ferritecad_types::{ObjectId, Result, StableEntityId};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CutExtent {
    Blind { depth_mm: f64 },
    ThroughAll,
}
impl CutExtent {
    pub fn blind_depth_mm(self) -> Option<f64> {
        match self {
            Self::Blind { depth_mm } => Some(depth_mm),
            Self::ThroughAll => None,
        }
    }
    pub fn end(self) -> Result<EndCondition> {
        Ok(match self {
            Self::Blind { depth_mm } => EndCondition::Blind {
                distance: Expression::constant(depth_mm)?,
            },
            Self::ThroughAll => EndCondition::ThroughAll,
        })
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct CircularCut {
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub extent: CutExtent,
}
impl CircularCut {
    pub fn geometry(self) -> Result<SketchGeometry> {
        Ok(SketchGeometry::Circle {
            center: Point2::new(self.center_mm[0], self.center_mm[1])?,
            radius: self.radius_mm,
        })
    }
    pub fn additions(self, target: &CutTarget) -> Result<Vec<EditChange>> {
        let sketch = ObjectId::new();
        let payload = Sketch {
            plane: target.plane,
            curves: vec![SketchCurve {
                id: StableEntityId::new(),
                construction: false,
                geometry: self.geometry()?,
            }],
            constraints: vec![],
        };
        ObjectPayload::Sketch(payload.clone()).to_storage_bytes()?;
        let feature = FeatureAddition::Extrude {
            regions: Vec::new(),
            operation: ferritecad_document::SolidOperation::Cut,
            profile: sketch,
            end_condition: self.extent.end()?,
            reversed: false,
        };
        feature
            .payload(Some(target.tip_feature))?
            .to_storage_bytes()?;
        Ok(vec![
            EditChange::AddSketch {
                object: sketch,
                name: Some("Cut tool".into()),
                sketch: payload,
            },
            EditChange::AddFeature {
                object: ObjectId::new(),
                body: target.body,
                name: Some("Cut".into()),
                feature,
            },
        ])
    }
    pub fn changes(self, saved: &SavedCut) -> Result<Vec<EditChange>> {
        let mut payload = saved.sketch.clone();
        let geometry = self.geometry()?;
        payload
            .curves
            .iter_mut()
            .find(|c| c.id == saved.tool_curve)
            .expect("displayed circle exists")
            .geometry = geometry.clone();
        ObjectPayload::Sketch(payload).to_storage_bytes()?;
        let end_condition = self.extent.end()?;
        let mut cut = saved.cut.clone();
        cut.end_condition = end_condition.clone();
        ObjectPayload::Extrude(cut).to_storage_bytes()?;
        Ok(vec![
            EditChange::SetCurveGeometry {
                sketch: saved.tool_sketch,
                curve: saved.tool_curve,
                geometry,
            },
            EditChange::SetEndCondition {
                object: saved.feature,
                end_condition,
            },
        ])
    }
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CutTarget {
    pub body: ObjectId,
    pub plane: ObjectId,
    pub tip_feature: ObjectId,
    pub boundary: Vec<[f64; 2]>,
    pub height_mm: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CutChoice {
    pub body: ObjectId,
    pub name: Option<String>,
    pub target: Option<CutTarget>,
    pub refusal: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedCut {
    pub feature: ObjectId,
    pub tool_sketch: ObjectId,
    pub tool_curve: StableEntityId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub extent: CutExtent,
    pub target: CutTarget,
    pub previous_feature: ObjectId,
    sketch: Sketch,
    cut: ferritecad_document::Extrude,
}
impl std::ops::Deref for SavedCut {
    type Target = CutTarget;
    fn deref(&self) -> &CutTarget {
        &self.target
    }
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CutParameterChoice {
    pub feature: ObjectId,
    pub name: Option<String>,
    pub saved: Option<SavedCut>,
    pub refusal: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Corner {
    pub feature: ObjectId,
    pub joint: ferritecad_types::ProfileJoint,
    pub corner_mm: [f64; 2],
    pub adjacent_lengths_mm: [f64; 2],
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeTarget {
    pub body: ObjectId,
    pub base_feature: ObjectId,
    pub profile: ObjectId,
    pub height_mm: f64,
    pub previous_feature: ObjectId,
    pub constrained: bool,
    pub corners: Vec<Corner>,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeChoice {
    pub body: ObjectId,
    pub name: Option<String>,
    pub target: Option<EdgeTarget>,
    pub refusal: Option<String>,
}
impl EdgeTarget {
    pub fn addition(&self, feature: FeatureAddition, name: &str) -> Result<Vec<EditChange>> {
        feature
            .payload(Some(self.previous_feature))?
            .to_storage_bytes()?;
        Ok(vec![EditChange::AddFeature {
            object: ObjectId::new(),
            body: self.body,
            name: Some(name.into()),
            feature,
        }])
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgeFillet {
    pub edge: SweptEdge,
    pub radius_mm: f64,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgeChamfer {
    pub edge: SweptEdge,
    pub distance_mm: f64,
}
impl EdgeFillet {
    pub fn feature(self) -> Result<FeatureAddition> {
        Ok(FeatureAddition::Fillet {
            edge: self.edge,
            radius_mm: Expression::constant(self.radius_mm)?,
        })
    }
}
impl EdgeChamfer {
    pub fn feature(self) -> Result<FeatureAddition> {
        Ok(FeatureAddition::Chamfer {
            edge: self.edge,
            distance_mm: Expression::constant(self.distance_mm)?,
        })
    }
}

pub(crate) fn read(
    objects: &[ObjectRecord],
) -> (Vec<CutChoice>, Vec<CutParameterChoice>, Vec<EdgeChoice>) {
    let mut bodies = Vec::new();
    let mut cuts = Vec::new();
    let mut edges = Vec::new();
    for body in objects {
        let ObjectPayload::Body(b) = &body.payload else {
            continue;
        };
        let Some(tip) = b.tip_feature else { continue };
        let mut chain = Vec::new();
        let mut cursor = Some(tip);
        let mut seen = std::collections::BTreeSet::new();
        while let Some(id) = cursor {
            if !seen.insert(id) {
                break;
            }
            let Some(o) = objects.iter().find(|o| o.id == id) else {
                break;
            };
            chain.push(o);
            cursor = o.payload.previous_feature();
        }
        let Some(&base) = chain.last() else { continue };
        let (profile, height_mm) = match &base.payload {
            ObjectPayload::Extrude(e) => (
                e.profile,
                match &e.end_condition {
                    EndCondition::Blind { distance } => distance.value(),
                    EndCondition::Symmetric { distance } => distance.value() * 2.,
                    _ => 10.,
                },
            ),
            // Only a suggested Blind depth for the existing Cut form, never a reach limit.
            ObjectPayload::Revolve(r) => (r.profile, 10.),
            _ => continue,
        };
        let Some(sketch) = objects.iter().find_map(|o| match &o.payload {
            ObjectPayload::Sketch(s) if o.id == profile => Some(s),
            _ => None,
        }) else {
            continue;
        };
        let lines = sketch
            .curves
            .iter()
            .filter(|c| !c.construction)
            .filter_map(|c| match c.geometry {
                SketchGeometry::Line { start, end } => Some((c.id, start, end)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let target = CutTarget {
            body: body.id,
            plane: sketch.plane,
            tip_feature: tip,
            boundary: lines.iter().map(|(_, p, _)| [p.x, p.y]).collect(),
            height_mm,
        };
        bodies.push(CutChoice {
            body: body.id,
            name: body.name.clone(),
            target: Some(target.clone()),
            refusal: None,
        });
        let mut corners = Vec::new();
        for (i, (id, start, end)) in lines.iter().enumerate() {
            let (next_id, next_start, next_end) = lines[(i + 1) % lines.len()];
            if *end != next_start {
                continue;
            }
            let Ok(joint) = ferritecad_types::ProfileJoint::new(*id, next_id) else {
                continue;
            };
            corners.push(Corner {
                feature: base.id,
                joint,
                corner_mm: [end.x, end.y],
                adjacent_lengths_mm: [
                    (end.x - start.x).hypot(end.y - start.y),
                    (next_end.x - end.x).hypot(next_end.y - end.y),
                ],
            });
        }
        if matches!(&base.payload, ObjectPayload::Extrude(e)
            if !e.reversed && matches!(e.end_condition, EndCondition::Blind { .. }))
        {
            edges.push(EdgeChoice {
                body: body.id,
                name: body.name.clone(),
                target: Some(EdgeTarget {
                    body: body.id,
                    base_feature: base.id,
                    profile,
                    height_mm,
                    previous_feature: tip,
                    constrained: !sketch.constraints.is_empty(),
                    corners,
                }),
                refusal: None,
            });
        }
        for o in chain.into_iter().rev() {
            let ObjectPayload::Extrude(cut) = &o.payload else {
                continue;
            };
            if cut.operation != ferritecad_document::SolidOperation::Cut {
                continue;
            }
            let Some(tool) = objects.iter().find_map(|o| match &o.payload {
                ObjectPayload::Sketch(s) if o.id == cut.profile => Some(s),
                _ => None,
            }) else {
                continue;
            };
            let curves = tool
                .curves
                .iter()
                .filter(|c| !c.construction)
                .collect::<Vec<_>>();
            let [curve] = curves.as_slice() else { continue };
            let SketchGeometry::Circle { center, radius } = curve.geometry else {
                continue;
            };
            let extent = match &cut.end_condition {
                EndCondition::Blind { distance } => CutExtent::Blind {
                    depth_mm: distance.value(),
                },
                EndCondition::ThroughAll => CutExtent::ThroughAll,
                _ => continue,
            };
            cuts.push(CutParameterChoice {
                feature: o.id,
                name: o.name.clone(),
                refusal: None,
                saved: Some(SavedCut {
                    feature: o.id,
                    tool_sketch: cut.profile,
                    tool_curve: curve.id,
                    center_mm: [center.x, center.y],
                    radius_mm: radius,
                    extent,
                    target: target.clone(),
                    previous_feature: cut.previous.unwrap_or(base.id),
                    sketch: tool.clone(),
                    cut: cut.clone(),
                }),
            });
        }
    }
    (bodies, cuts, edges)
}
