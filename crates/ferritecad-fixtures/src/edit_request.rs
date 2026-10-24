// SPDX-License-Identifier: MIT
use ferritecad_document::EditChange;
use serde_json::{Value, json};

/// Encode a test's domain changes at the CLI boundary, with explicit units.
pub fn edit_request_json(changes: &[EditChange]) -> Value {
    let changes: Vec<_> = changes.iter().map(|change| match change {
        EditChange::AddSketch { object, name, sketch } => {
            let curves: Vec<_> = sketch.curves.iter().map(|c| {
                let mut geometry = serde_json::to_value(&c.geometry).expect("geometry");
                geometry_units(&mut geometry, "mm");
                json!({"id": c.id, "construction": c.construction, "geometry": geometry})
            }).collect();
            let constraints: Vec<_> = sketch.constraints.iter().map(|constraint| {
                let encoded = edit_request_json(&[EditChange::AddConstraint { sketch: *object, constraint: constraint.clone() }]);
                encoded["changes"][0]["constraint"].clone()
            }).collect();
            json!({"kind":"add_sketch", "object":object, "name":name, "plane":sketch.plane, "curves":curves, "constraints":constraints})
        },
        EditChange::AddFeature { object, body, name, feature } => {
            use ferritecad_document::FeatureAddition as F;
            let feature = match feature {
                F::Cut { profile, end_condition, reversed } => json!({"kind":"cut", "profile":profile, "end_condition":end_json(end_condition), "reversed":reversed}),
                F::Fillet { edge, radius_mm } => json!({"kind":"fillet", "edge":edge, "radius":{"value":radius_mm.value(),"unit":"mm"}}),
                F::Chamfer { edge, distance_mm } => json!({"kind":"chamfer", "edge":edge, "distance":{"value":distance_mm.value(),"unit":"mm"}}),
            };
            json!({"kind":"add_feature", "object":object, "body":body, "name":name, "feature":feature})
        },
        EditChange::SetEndCondition { object, end_condition } => json!({"kind":"set_end_condition", "object":object, "end_condition":end_json(end_condition)}),
        EditChange::SetExpression { object, slot, expression } => json!({
            "kind": "set_expression", "object": object, "slot": slot,
            "value": expression.value(),
            "unit": if slot.dimension() == ferritecad_types::Dimension::Angle { "rad" } else { "mm" },
        }),
        EditChange::SetCurveGeometry { sketch, curve, geometry } => {
            let mut geometry = serde_json::to_value(geometry).expect("fixture geometry");
            geometry_units(&mut geometry, "mm");
            json!({"kind":"set_curve_geometry", "sketch":sketch, "curve":curve, "geometry":geometry})
        }
        EditChange::AddConstraint { sketch, constraint } => {
            let mut rule = serde_json::to_value(&constraint.rule).expect("fixture rule");
            for key in ["x", "y", "distance", "radius", "angle"] {
                if let Some(expression) = rule.get_mut(key) {
                    *expression = json!({"value": expression["value"], "unit": if key == "angle" {"rad"} else {"mm"}});
                }
            }
            json!({"kind":"add_constraint", "sketch":sketch, "constraint":{"id":constraint.id,"rule":rule}})
        }
        EditChange::RemoveConstraint { sketch, constraint } => json!({
            "kind":"remove_constraint", "sketch":sketch, "constraint":constraint,
        }),
    }).collect();
    json!({"request_version":1, "changes":changes})
}

fn geometry_units(value: &mut Value, unit: &str) {
    match value {
        Value::Number(_) => *value = json!({"value":value, "unit":unit}),
        Value::Object(fields) => {
            for (key, value) in fields {
                geometry_units(value, if key.ends_with("angle") { "rad" } else { unit });
            }
        }
        _ => {}
    }
}

fn end_json(end: &ferritecad_document::EndCondition) -> Value {
    let mut value = serde_json::to_value(end).expect("end condition");
    if let Some(distance) = value.get_mut("distance") {
        *distance = json!({"value":distance["value"], "unit":"mm"});
    }
    value
}
