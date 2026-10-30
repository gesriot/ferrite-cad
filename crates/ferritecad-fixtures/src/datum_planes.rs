// SPDX-License-Identifier: MIT
use ferritecad_document::{
    DatumPlane, Document, EditChange, EndCondition, EntityKind, EntityReference, Expression,
    FeatureAddition, Point2, ReferenceMeaning, Selection, Sketch, SketchCurve, SketchGeometry,
    SolidOperation,
};
use ferritecad_eval::RebuildResult;
use ferritecad_kernel::{GeometryKernel, NamingInput, OperationContext, SubShapeKind};
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

/// Selects a fixture's stated sweep input by actual provenance, never coordinates.
pub fn sweep_face(
    built: &RebuildResult,
    kernel: &mut (impl GeometryKernel + ?Sized),
    feature: ObjectId,
    input: NamingInput,
) -> Result<EntityReference> {
    let shape = built
        .shape(feature)
        .ok_or_else(|| CadError::input("fixture has no shape"))?;
    let history = kernel.naming_history(shape)?;
    let faces: std::collections::BTreeSet<_> = history
        .relations
        .iter()
        .filter(|(source, _, _)| *source == input)
        .filter_map(|(_, _, output)| *output)
        .filter(|entity| entity.kind() == SubShapeKind::Face)
        .collect();
    if faces.len() != 1 {
        return Err(CadError::input(
            "fixture sweep input must produce exactly one face",
        ));
    }
    let entity = faces.first().expect("one face");
    let name = built.topology().features[&feature].entities[entity];
    Ok(EntityReference::new(ReferenceMeaning {
        producer: built
            .topology()
            .producer(&name.digest)
            .expect("published owner"),
        kind: EntityKind::Face,
        digest: name.digest,
        selection: Selection::One,
    }))
}

#[derive(Debug)]
pub struct SupportedSketch {
    pub plane: ObjectId,
    pub sketch: ObjectId,
    pub feature: ObjectId,
    pub base: ObjectId,
    pub face: EntityReference,
    pub changes: Vec<EditChange>,
}

pub fn boss(
    document: &Document,
    kernel: &mut (impl GeometryKernel + ?Sized),
) -> Result<SupportedSketch> {
    let body = crate::feature_changes::body(document)?;
    let (base, _, _) = crate::feature_changes::base_profile(document, body)?;
    let built = ferritecad_eval::rebuild_cold(document, kernel, &OperationContext::default())?;
    let face = sweep_face(&built, kernel, base, NamingInput::Cap { end: true });
    built.release_all(kernel);
    let face = face?;
    let (plane, sketch, feature) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    Ok(SupportedSketch {
        plane,
        sketch,
        feature,
        base,
        face,
        changes: vec![
            EditChange::AddDatumPlane {
                object: plane,
                name: Some("Top support".into()),
                plane: DatumPlane::Face {
                    result: base,
                    face,
                    offset_mm: Expression::constant(0.)?,
                },
            },
            EditChange::AddSketch {
                object: sketch,
                name: Some("Boss profile".into()),
                sketch: circle(plane, [25., 20.], 3.)?,
            },
            EditChange::AddFeature {
                object: feature,
                body,
                name: Some("Boss".into()),
                feature: FeatureAddition::Extrude {
                    profile: sketch,
                    regions: vec![],
                    operation: SolidOperation::Add,
                    reversed: false,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(4.)?,
                    },
                },
            },
        ],
    })
}

pub fn circle(plane: ObjectId, center: [f64; 2], radius: f64) -> Result<Sketch> {
    Ok(Sketch {
        plane,
        constraints: vec![],
        curves: vec![SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Circle {
                center: Point2::new(center[0], center[1])?,
                radius,
            },
        }],
    })
}
