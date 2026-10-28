// SPDX-License-Identifier: MIT
use ferritecad_document::*;
use ferritecad_types::{ObjectId, StableEntityId};

pub struct Regions {
    pub body: ObjectId,
    pub feature: ObjectId,
    pub sketch: ObjectId,
    pub curves: [StableEntityId; 2],
    pub changes: Vec<EditChange>,
    pub dependent: ObjectId,
    pub dependency: Vec<EditChange>,
}

pub fn fixture(document: &Document, revolve: bool) -> Regions {
    let plane = document
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::DatumPlane(_)))
        .expect("plane")
        .id;
    let body = ObjectId::new();
    let feature = ObjectId::new();
    let sketch = ObjectId::new();
    let curves = [StableEntityId::new(), StableEntityId::new()];
    let dependent = ObjectId::new();
    let tool = ObjectId::new();
    let curve = |id, x, radius| SketchCurve {
        id,
        construction: false,
        geometry: SketchGeometry::Circle {
            center: Point2::new(x, 0.).expect("center"),
            radius,
        },
    };
    let sweep = |profile, regions, operation| {
        if revolve {
            FeatureAddition::Revolve {
                profile,
                regions,
                operation,
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
            }
        } else {
            FeatureAddition::Extrude {
                profile,
                regions,
                operation,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(5.).expect("height"),
                },
                reversed: false,
            }
        }
    };
    Regions {
        body,
        feature,
        sketch,
        curves,
        dependent,
        changes: vec![
            EditChange::AddSketch {
                object: sketch,
                name: None,
                sketch: Sketch {
                    plane,
                    curves: vec![curve(curves[0], 8., 2.), curve(curves[1], 18., 2.)],
                    constraints: vec![],
                },
            },
            EditChange::AddBody {
                object: body,
                name: None,
                feature_object: feature,
                feature_name: None,
                feature: sweep(sketch, vec![curves[0]], SolidOperation::NewBody),
            },
        ],
        dependency: vec![
            EditChange::AddSketch {
                object: tool,
                name: None,
                sketch: Sketch {
                    plane,
                    curves: vec![curve(StableEntityId::new(), 8., 1.)],
                    constraints: vec![],
                },
            },
            EditChange::AddFeature {
                object: dependent,
                body,
                name: None,
                feature: sweep(tool, vec![], SolidOperation::Cut),
            },
        ],
    }
}

impl Regions {
    pub fn switch(&self) -> Vec<EditChange> {
        vec![EditChange::SetRegions {
            object: self.feature,
            regions: vec![self.curves[1], self.curves[1]],
        }]
    }
    pub fn resize_and_move(&self) -> Vec<EditChange> {
        vec![EditChange::SetCurveGeometry {
            sketch: self.sketch,
            curve: self.curves[0],
            geometry: SketchGeometry::Circle {
                center: Point2::new(9., 0.).expect("center"),
                radius: 3.,
            },
        }]
    }
}

pub fn volume(path: &std::path::Path, feature: ObjectId) -> f64 {
    let d = Document::open_read_only(path).expect("document");
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built =
        ferritecad_eval::rebuild_cold(&d, &mut k, &ferritecad_kernel::OperationContext::default())
            .expect("rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    let shape = built.shape(feature).expect("shape");
    assert!(k.is_valid(shape).expect("independent BRepCheck"));
    let result = k.shape_stats(shape).expect("volume").1;
    built.release_all(&mut k);
    result
}
