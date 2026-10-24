// SPDX-License-Identifier: MIT
use ferritecad_document::{EditChange, Point2, SketchGeometry};
use ferritecad_types::{ObjectId, Result, StableEntityId};

pub(crate) fn circle_change(
    sketch: ObjectId,
    curve: StableEntityId,
    center: [f64; 2],
    radius: f64,
) -> Result<EditChange> {
    let geometry = SketchGeometry::Circle {
        center: Point2::new(center[0], center[1])?,
        radius,
    };
    geometry.validate()?;
    Ok(EditChange::SetCurveGeometry {
        sketch,
        curve,
        geometry,
    })
}

pub(crate) fn circle_values(change: &EditChange) -> ([f64; 2], f64) {
    let EditChange::SetCurveGeometry {
        geometry: SketchGeometry::Circle { center, radius },
        ..
    } = change
    else {
        unreachable!("circle form contains circle geometry")
    };
    ([center.x, center.y], *radius)
}
