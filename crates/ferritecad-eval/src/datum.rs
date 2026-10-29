// SPDX-License-Identifier: MIT
use std::collections::BTreeMap;

use ferritecad_document::{DatumPlane, EntityKind};
use ferritecad_kernel::{GeometryKernel, OperationContext, SketchPlane};
use ferritecad_topology::ModelNaming;
use ferritecad_types::{CadError, ObjectId, Point3, Result};

pub(crate) fn evaluate(
    object: ObjectId,
    datum: &DatumPlane,
    planes: &BTreeMap<ObjectId, SketchPlane>,
    naming: &ModelNaming,
    kernel: &mut (impl GeometryKernel + ?Sized),
    context: &OperationContext,
) -> Result<SketchPlane> {
    let (plane, offset) = match datum {
        DatumPlane::Fixed { .. } => return crate::plane_from_datum(datum),
        DatumPlane::Offset { from, distance_mm } => {
            let plane = planes.get(from).copied().ok_or_else(|| {
                CadError::input(format!(
                    "datum plane {object} has no built source plane {from}"
                ))
            })?;
            (plane, distance_mm.value())
        }
        DatumPlane::Face {
            result,
            face,
            offset_mm,
        } => {
            let faces = naming.resolve(EntityKind::Face, object, *result, &[*face])?;
            let plane = kernel.support_plane(&faces, context).map_err(|source| {
                CadError::Context {
                    kind: source.kind(),
                    message: format!(
                        "datum plane {object}, face reference {} cannot support a sketch plane: \
                         {source}",
                        face.id
                    ),
                    source: Box::new(source),
                }
            })?;
            (plane.sketch_plane(context.tolerance())?, offset_mm.value())
        }
    };
    let p = plane.origin();
    let n = plane.normal();
    SketchPlane::new(
        Point3::new(p.x + offset * n.x, p.y + offset * n.y, p.z + offset * n.z)?,
        plane.x_axis(),
        n,
    )
}
