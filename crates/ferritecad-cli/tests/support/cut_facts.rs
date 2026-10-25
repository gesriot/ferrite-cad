// SPDX-License-Identifier: MIT
#![allow(dead_code)]
use ferritecad_document::{
    Document, EditChange, EndCondition, Expression, ObjectPayload, SketchGeometry,
};
use ferritecad_types::{ObjectId, StableEntityId};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CutDepth {
    Blind { depth_mm: f64 },
    ThroughAll,
}
impl CutDepth {
    pub fn blind_depth_mm(self) -> Option<f64> {
        match self {
            Self::Blind { depth_mm } => Some(depth_mm),
            Self::ThroughAll => None,
        }
    }
    pub fn reach_mm(self, height: f64) -> f64 {
        self.blind_depth_mm().unwrap_or(height).min(height)
    }
    pub fn leaves_a_floor(self, height: f64) -> bool {
        self.reach_mm(height) < height
    }
    pub fn end(self) -> EndCondition {
        match self {
            Self::Blind { depth_mm } => EndCondition::Blind {
                distance: Expression::constant(depth_mm).expect("depth"),
            },
            Self::ThroughAll => EndCondition::ThroughAll,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutSpec {
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub extent: CutDepth,
}
impl CutSpec {
    pub fn additions(self, d: &Document, body: ObjectId) -> Vec<EditChange> {
        ferritecad_fixtures::feature_changes::circular_tool(
            d,
            body,
            self.center_mm,
            self.radius_mm,
            self.extent.end(),
        )
        .expect("tool")
        .changes
    }
    pub fn changes(self, saved: &CutObservation) -> Vec<EditChange> {
        vec![
            EditChange::SetCurveGeometry {
                sketch: saved.tool_sketch,
                curve: saved.tool_curve,
                geometry: SketchGeometry::Circle {
                    center: ferritecad_document::Point2::new(self.center_mm[0], self.center_mm[1])
                        .expect("point"),
                    radius: self.radius_mm,
                },
            },
            EditChange::SetEndCondition {
                object: saved.feature,
                end_condition: self.extent.end(),
            },
        ]
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct CutObservation {
    pub feature: ObjectId,
    pub body: ObjectId,
    pub base_feature: ObjectId,
    pub profile_sketch: ObjectId,
    pub tool_sketch: ObjectId,
    pub tool_curve: StableEntityId,
    pub previous_feature: ObjectId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub extent: CutDepth,
}
pub fn observed(path: &Path) -> Vec<CutObservation> {
    let d = Document::open_read_only(path).expect("fixture");
    let body = ferritecad_fixtures::feature_changes::body(&d).expect("body");
    let (base, profile, _) =
        ferritecad_fixtures::feature_changes::base_profile(&d, body).expect("base");
    let objects = d.objects().expect("objects");
    let mut previous = base;
    let mut result = Vec::new();
    while let Some((id, e)) = objects.iter().find_map(|o| match &o.payload {
        ObjectPayload::Extrude(e) if e.previous == Some(previous) => Some((o.id, e)),
        _ => None,
    }) {
        let ObjectPayload::Sketch(tool) = &objects
            .iter()
            .find(|o| o.id == e.profile)
            .expect("tool")
            .payload
        else {
            panic!("Sketch")
        };
        let SketchGeometry::Circle { center, radius } = tool.curves[0].geometry else {
            panic!("Circle")
        };
        result.push(CutObservation {
            feature: id,
            body,
            base_feature: base,
            profile_sketch: profile,
            tool_sketch: e.profile,
            tool_curve: tool.curves[0].id,
            previous_feature: previous,
            center_mm: [center.x, center.y],
            radius_mm: radius,
            extent: match &e.end_condition {
                EndCondition::Blind { distance } => CutDepth::Blind {
                    depth_mm: distance.value(),
                },
                EndCondition::ThroughAll => CutDepth::ThroughAll,
                _ => panic!("end"),
            },
        });
        previous = id;
    }
    result
}
pub fn write_add(source: &Path, request: &Path, spec: CutSpec) {
    let d = Document::open_read_only(source).expect("source");
    let body = ferritecad_fixtures::feature_changes::body(&d).expect("body");
    std::fs::write(
        request,
        ferritecad_fixtures::edit_request_json(&spec.additions(&d, body)).to_string(),
    )
    .expect("request");
}
pub fn write_edit(request: &Path, saved: &CutObservation, spec: CutSpec) {
    std::fs::write(
        request,
        ferritecad_fixtures::edit_request_json(&spec.changes(saved)).to_string(),
    )
    .expect("request");
}
