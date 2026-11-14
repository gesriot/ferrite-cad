// SPDX-License-Identifier: MIT
#![allow(dead_code, reason = "shared by focused curve-edit test targets")]
use ferritecad_document::*;
use ferritecad_types::{ObjectId, StableEntityId, Transform};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub fn point(x: f64, y: f64) -> Point2 {
    Point2::new(x, y).expect("finite fixture point")
}
pub fn curve(geometry: SketchGeometry) -> SketchCurve {
    SketchCurve {
        id: StableEntityId::new(),
        construction: false,
        geometry,
    }
}
pub fn line(a: [f64; 2], b: [f64; 2]) -> SketchCurve {
    curve(SketchGeometry::Line {
        start: point(a[0], a[1]),
        end: point(b[0], b[1]),
    })
}
pub fn rectangle() -> Vec<SketchCurve> {
    vec![
        line([0., 0.], [10., 0.]),
        line([10., 0.], [10., 8.]),
        line([10., 8.], [0., 8.]),
        line([0., 8.], [0., 0.]),
    ]
}
pub fn reverse(curve: &mut SketchCurve) {
    match &mut curve.geometry {
        SketchGeometry::Line { start, end } => std::mem::swap(start, end),
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } => std::mem::swap(start_angle, end_angle),
        _ => unreachable!("fixture reverses only open curves"),
    }
}
pub fn variants(base: &[SketchCurve]) -> Vec<Vec<SketchCurve>> {
    let mut result = vec![base.to_vec(); 6];
    for c in &mut result[1] {
        reverse(c);
    }
    result[1].reverse();
    reverse(&mut result[2][1]);
    reverse(&mut result[3][1]);
    reverse(&mut result[3][3]);
    result[4].swap(1, 3);
    reverse(&mut result[5][1]);
    reverse(&mut result[5][3]);
    result[5].swap(0, 2);
    result
}
pub fn put(doc: &mut Document, id: ObjectId, name: &str, payload: ObjectPayload) {
    doc.write(|w| {
        w.put_object(id, None, 0, Some(name), &payload)?;
        for dep in payload.dependencies(id) {
            w.add_dependency(dep)?;
        }
        Ok(())
    })
    .expect("fixture payload");
}
pub struct Drawing {
    pub root: tempfile::TempDir,
    pub path: std::path::PathBuf,
    pub doc: Document,
    pub plane: ObjectId,
    pub sketch: ObjectId,
    pub feature: ObjectId,
    pub body: ObjectId,
}
impl Drawing {
    pub fn new(curves: Vec<SketchCurve>, consumed: bool) -> Self {
        let root = tempfile::tempdir().expect("fixture directory");
        let path = root.path().join("drawing.fcad");
        let doc = Document::create(&path).expect("document");
        let mut f = Self {
            root,
            path,
            doc,
            plane: ObjectId::new(),
            sketch: ObjectId::new(),
            feature: ObjectId::new(),
            body: ObjectId::new(),
        };
        put(
            &mut f.doc,
            f.plane,
            "XY",
            ObjectPayload::DatumPlane(DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            }),
        );
        put(
            &mut f.doc,
            f.sketch,
            "Drawing",
            ObjectPayload::Sketch(Sketch {
                plane: f.plane,
                curves,
                constraints: vec![],
            }),
        );
        if consumed {
            put(
                &mut f.doc,
                f.feature,
                "Extrude",
                ObjectPayload::Extrude(Extrude {
                    profile: f.sketch,
                    regions: vec![],
                    previous: None,
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(3.).expect("height"),
                    },
                }),
            );
            put(
                &mut f.doc,
                f.body,
                "Part",
                ObjectPayload::Body(Body {
                    tip_feature: Some(f.feature),
                    rollback: None,
                }),
            );
        }
        f
    }
    pub fn saved(&self) -> Sketch {
        let ObjectPayload::Sketch(s) = self
            .doc
            .object(self.sketch)
            .expect("read")
            .expect("sketch")
            .payload
        else {
            unreachable!("fixture sketch")
        };
        s
    }
    pub fn replace(&mut self, curves: Vec<SketchCurve>) {
        let mut sketch = self.saved();
        sketch.curves = curves;
        self.doc
            .write(|w| {
                w.put_object(
                    self.sketch,
                    None,
                    0,
                    Some("Drawing"),
                    &ObjectPayload::Sketch(sketch),
                )
            })
            .expect("replace drawing");
    }
}
