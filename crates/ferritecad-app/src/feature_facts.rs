// SPDX-License-Identifier: MIT
//! Stored geometry the existing forms can draw; Apply decides whether it builds.
use ferritecad_document::{
    EditChange, EndCondition, EntityReference, Expression, FeatureAddition, ObjectPayload,
    ObjectRecord, Point2, Sketch, SketchCurve, SketchGeometry,
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PickedEdge {
    pub reference: EntityReference,
    pub description: String,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeTarget {
    pub body: ObjectId,
    pub previous_feature: ObjectId,
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
#[derive(Debug, Clone)]
pub(crate) struct EdgeFillet {
    pub edges: Vec<EntityReference>,
    pub radius_mm: f64,
}
#[derive(Debug, Clone)]
pub(crate) struct EdgeChamfer {
    pub edges: Vec<EntityReference>,
    pub distance_mm: f64,
}
impl EdgeFillet {
    pub fn feature(&self) -> Result<FeatureAddition> {
        Ok(FeatureAddition::Fillet {
            edges: self.edges.clone(),
            radius_mm: Expression::constant(self.radius_mm)?,
        })
    }
}
impl EdgeChamfer {
    pub fn feature(&self) -> Result<FeatureAddition> {
        Ok(FeatureAddition::Chamfer {
            edges: self.edges.clone(),
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
        edges.push(EdgeChoice {
            body: body.id,
            name: body.name.clone(),
            target: Some(EdgeTarget {
                body: body.id,
                previous_feature: tip,
            }),
            refusal: None,
        });
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
                    EndCondition::ThroughAll
                    | EndCondition::UpToFace { .. }
                    | EndCondition::UpToVertex { .. } => 10.,
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
        for o in chain.into_iter().rev() {
            let ObjectPayload::Extrude(cut) = &o.payload else {
                continue;
            };
            if cut.operation != ferritecad_document::SolidOperation::Cut {
                continue;
            }
            if matches!(
                cut.end_condition,
                EndCondition::UpToFace { .. } | EndCondition::UpToVertex { .. }
            ) {
                cuts.push(CutParameterChoice {
                    feature: o.id,
                    name: o.name.clone(),
                    saved: None,
                    refusal: Some("this feature's end condition is edited through `edit`".into()),
                });
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
                EndCondition::Symmetric { .. } => continue,
                EndCondition::UpToFace { .. } | EndCondition::UpToVertex { .. } => {
                    unreachable!("referenced conditions have an explicit refusal")
                }
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
