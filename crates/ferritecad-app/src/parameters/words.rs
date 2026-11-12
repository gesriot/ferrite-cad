// SPDX-License-Identifier: MIT
use ferritecad_document::{ExpressionSlot, ObjectPayload};
use ferritecad_types::{Dimension, ObjectId};

use crate::sketch_facts::Source;

pub(super) fn value(value: f64, dimension: Dimension) -> String {
    match dimension {
        Dimension::Length => format!("{} mm", crate::mm(value)),
        Dimension::Angle => format!("{}°", ferritecad_types::display_degrees(value)),
        Dimension::Scalar => value.to_string(),
        // Dimension is non-exhaustive; never assign an unknown dimension a known unit.
        _ => "Unsupported dimension".into(),
    }
}

/// The same slot words in Values and in the list of parameter consumers.
pub(super) fn slot(source: &Source, object: ObjectId, slot: ExpressionSlot) -> String {
    let (label, constraint) = match slot {
        ExpressionSlot::ExtrudeDistance => ("Extrude distance", None),
        ExpressionSlot::ExtrudeEndOffset => ("Extrude end offset", None),
        ExpressionSlot::DatumOffset => ("Datum plane offset", None),
        ExpressionSlot::FilletRadius => ("Fillet radius", None),
        ExpressionSlot::ChamferDistance => ("Chamfer distance", None),
        ExpressionSlot::RevolveAngle => ("Revolve angle", None),
        ExpressionSlot::Distance { constraint } => ("Distance", Some(constraint)),
        ExpressionSlot::Radius { constraint } => ("Radius", Some(constraint)),
        ExpressionSlot::DirectedAngle { constraint } => ("Directed angle", Some(constraint)),
        ExpressionSlot::FixedX { constraint } => ("Fixed X", Some(constraint)),
        ExpressionSlot::FixedY { constraint } => ("Fixed Y", Some(constraint)),
    };
    let Some(id) = constraint else {
        return label.into();
    };
    if let Some(row) = source.objects.iter().find(|o| o.id == object)
        && let ObjectPayload::Sketch(sketch) = &row.payload
        && let Some(constraint) = sketch.constraints.iter().find(|c| c.id == id)
    {
        let words = crate::ConstraintWords::of(id, &constraint.rule);
        return format!("{label} · {} · {}", words.identifier, words.says);
    }
    format!("{label} · {id}")
}
