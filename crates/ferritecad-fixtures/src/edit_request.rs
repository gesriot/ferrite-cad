// SPDX-License-Identifier: MIT
use ferritecad_document::EditChange;
use serde_json::{Value, json};

/// Encode a test's domain changes at the CLI boundary, with explicit units.
pub fn edit_request_json(changes: &[EditChange]) -> Value {
    let changes: Vec<_> = changes.iter().map(|change| match change {
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
