// SPDX-License-Identifier: MIT
use ferritecad_document::*;
use ferritecad_kernel::{GeometryKernel, NamingInput, OperationContext};
use ferritecad_types::{ObjectId, Result, StableEntityId, Transform};

#[derive(Debug)]
pub struct Plate {
    pub plane: ObjectId,
    pub body: ObjectId,
    pub base: ObjectId,
    pub curves: Vec<StableEntityId>,
}

pub fn rectangle(plane: ObjectId, min: [f64; 2], max: [f64; 2]) -> Result<Sketch> {
    let points = [min, [max[0], min[1]], max, [min[0], max[1]]];
    Ok(Sketch {
        plane,
        constraints: vec![],
        curves: (0..4)
            .map(|i| {
                Ok(SketchCurve {
                    id: StableEntityId::new(),
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: Point2::new(points[i][0], points[i][1])?,
                        end: Point2::new(points[(i + 1) % 4][0], points[(i + 1) % 4][1])?,
                    },
                })
            })
            .collect::<Result<_>>()?,
    })
}

pub fn extrusion(
    profile: ObjectId,
    operation: SolidOperation,
    distance: f64,
) -> Result<FeatureAddition> {
    Ok(FeatureAddition::Extrude {
        profile,
        operation,
        regions: vec![],
        reversed: false,
        end_condition: EndCondition::Blind {
            distance: Expression::constant(distance)?,
        },
    })
}

pub fn plate(document: &mut Document) -> Result<Plate> {
    let (plane, sketch, body, base) = (
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
    );
    let profile = rectangle(plane, [0., 0.], [60., 40.])?;
    let curves = profile.curves.iter().map(|c| c.id).collect();
    document.write_changes(&[
        EditChange::AddDatumPlane {
            object: plane,
            name: None,
            plane: DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            },
        },
        EditChange::AddSketch {
            object: sketch,
            name: None,
            sketch: profile,
        },
        EditChange::AddBody {
            object: body,
            name: Some("Plate".into()),
            feature_object: base,
            feature_name: Some("Base".into()),
            feature: extrusion(sketch, SolidOperation::NewBody, 10.)?,
        },
    ])?;
    Ok(Plate {
        plane,
        body,
        base,
        curves,
    })
}

#[derive(Debug)]
pub struct Slot {
    pub feature: ObjectId,
    pub keep: ObjectId,
    pub side: EntityReference,
    pub other_side: EntityReference,
    pub split: EntityReference,
    pub changes: Vec<EditChange>,
}

pub fn slot(document: &Document, plate: &Plate, kernel: &mut impl GeometryKernel) -> Result<Slot> {
    let built = ferritecad_eval::rebuild_cold(document, kernel, &OperationContext::default())?;
    let references = [3, 1, 0].map(|i| {
        crate::datum_planes::sweep_face(
            &built,
            kernel,
            plate.base,
            NamingInput::Curve(plate.curves[i]),
        )
    });
    built.release_all(kernel);
    let [side, other_side, split] = references;
    let (sketch, feature, keep) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    Ok(Slot {
        feature,
        keep,
        side: side?,
        other_side: other_side?,
        split: split?,
        changes: vec![
            EditChange::AddSketch {
                object: sketch,
                name: Some("Slot profile".into()),
                sketch: rectangle(plate.plane, [28., -2.], [32., 42.])?,
            },
            EditChange::AddFeature {
                object: feature,
                body: plate.body,
                name: Some("Slot".into()),
                feature: extrusion(sketch, SolidOperation::Cut, 11.)?,
            },
            EditChange::AddFeature {
                object: keep,
                body: plate.body,
                name: Some("Keep".into()),
                feature: FeatureAddition::KeepSolids {
                    selection: KeepSelection::All,
                },
            },
        ],
    })
}
