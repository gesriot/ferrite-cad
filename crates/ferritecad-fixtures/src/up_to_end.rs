// SPDX-License-Identifier: MIT
use ferritecad_document::*;
use ferritecad_kernel::{GeometryKernel, NamingInput, OperationContext, SubShapeKind};
use ferritecad_types::{CadError, ObjectId, Result};

#[derive(Debug)]
pub struct Ceiling {
    pub boss: crate::datum_planes::SupportedSketch,
    pub plane: ObjectId,
    pub result: ObjectId,
    pub face: EntityReference,
    pub vertex: EntityReference,
}

/// A circular boss and a separate ceiling; references come only from sweep provenance.
pub fn ceiling(document: &mut Document, kernel: &mut impl GeometryKernel) -> Result<Ceiling> {
    let mut boss = crate::datum_planes::boss(document, kernel)?;
    let FeatureAddition::Extrude { end_condition, .. } = (match boss.changes.last_mut() {
        Some(EditChange::AddFeature { feature, .. }) => feature,
        _ => return Err(CadError::input("boss fixture needs its final feature")),
    }) else {
        return Err(CadError::input("boss fixture is an extrusion"));
    };
    *end_condition = EndCondition::Blind {
        distance: Expression::constant(20.)?,
    };
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let result = ObjectId::new();
    let circle = crate::datum_planes::circle(plane, [25., 20.], 10.)?;
    boss.changes.extend([
        EditChange::AddDatumPlane {
            object: plane,
            name: Some("Ceiling plane".into()),
            plane: DatumPlane::Offset {
                from: boss.plane,
                distance_mm: Expression::constant(20.)?,
            },
        },
        EditChange::AddSketch {
            object: sketch,
            name: Some("Ceiling profile".into()),
            sketch: circle,
        },
        EditChange::AddBody {
            object: ObjectId::new(),
            name: Some("Ceiling".into()),
            feature_object: result,
            feature_name: None,
            feature: FeatureAddition::Extrude {
                profile: sketch,
                regions: vec![],
                operation: SolidOperation::NewBody,
                reversed: false,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(3.)?,
                },
            },
        },
    ]);
    document.write_changes(&boss.changes)?;
    let built = ferritecad_eval::rebuild_cold(document, kernel, &OperationContext::default())?;
    let refs = (|| -> Result<_> {
        let face = crate::datum_planes::sweep_face(
            &built,
            kernel,
            result,
            NamingInput::Cap { end: false },
        )?;
        let face_entity =
            built
                .topology()
                .resolve(EntityKind::Face, boss.feature, result, &[face])?[0];
        let topology = kernel.shape_topology(
            built
                .shape(result)
                .ok_or_else(|| CadError::input("ceiling did not build"))?,
        )?;
        let vertices: Vec<_> = topology
            .adjacent_faces
            .iter()
            .filter(|(entity, faces)| {
                entity.kind() == SubShapeKind::Vertex && faces.contains(&face_entity)
            })
            .map(|(e, _)| *e)
            .collect();
        if vertices.len() != 1 {
            return Err(CadError::input("circular cap has one seam vertex"));
        }
        let name = built.topology().features[&result].entities[&vertices[0]];
        let vertex = EntityReference::new(ReferenceMeaning {
            producer: result,
            kind: EntityKind::Vertex,
            digest: name.digest,
            selection: Selection::One,
        });
        Ok((face, vertex))
    })();
    built.release_all(kernel);
    let (face, vertex) = refs?;
    Ok(Ceiling {
        boss,
        plane,
        result,
        face,
        vertex,
    })
}

impl Ceiling {
    pub fn face_end(&self, offset: f64) -> Result<EndCondition> {
        Ok(EndCondition::UpToFace {
            result: self.result,
            face: self.face,
            offset_mm: Expression::constant(offset)?,
        })
    }
    pub fn vertex_end(&self) -> EndCondition {
        EndCondition::UpToVertex {
            result: self.result,
            vertex: self.vertex,
        }
    }
}

#[derive(Debug)]
pub struct Pockets {
    pub first: ObjectId,
    pub second: ObjectId,
    pub plane: ObjectId,
    pub face: EntityReference,
}

/// Two separate pockets from the plate top; the second consumes the first floor's meaning.
pub fn pockets(document: &mut Document, kernel: &mut impl GeometryKernel) -> Result<Pockets> {
    let mut supported = crate::datum_planes::boss(document, kernel)?;
    let body = crate::feature_changes::body(document)?;
    let Some(EditChange::AddFeature { feature, .. }) = supported.changes.last_mut() else {
        return Err(CadError::input("pocket fixture needs its final feature"));
    };
    let FeatureAddition::Extrude {
        operation,
        reversed,
        ..
    } = feature
    else {
        return Err(CadError::input("pocket fixture is an extrusion"));
    };
    *operation = SolidOperation::Cut;
    *reversed = true;
    document.write_changes(&supported.changes)?;
    use ferritecad_kernel::NamingOperation;
    use ferritecad_topology::generative::{cap_root, generated};
    let floor = generated(
        supported.feature,
        NamingOperation::Extrude,
        cap_root(supported.feature, true, Some(supported.plane)),
        SubShapeKind::Face,
    );
    let face = EntityReference::new(ReferenceMeaning {
        producer: supported.feature,
        kind: EntityKind::Face,
        digest: floor.digest,
        selection: Selection::One,
    });
    let second = ObjectId::new();
    let sketch = ObjectId::new();
    document.write_changes(&[
        EditChange::AddSketch {
            object: sketch,
            name: None,
            sketch: crate::datum_planes::circle(supported.plane, [45., 20.], 3.)?,
        },
        EditChange::AddFeature {
            object: second,
            body,
            name: Some("Following pocket".into()),
            feature: FeatureAddition::Extrude {
                profile: sketch,
                regions: vec![],
                operation: SolidOperation::Cut,
                reversed: true,
                end_condition: EndCondition::UpToFace {
                    result: supported.feature,
                    face,
                    offset_mm: Expression::constant(0.)?,
                },
            },
        },
    ])?;
    Ok(Pockets {
        first: supported.feature,
        second,
        plane: supported.plane,
        face,
    })
}
