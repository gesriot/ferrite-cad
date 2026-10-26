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
            let feature = feature_json(feature);
            json!({"kind":"add_feature", "object":object, "body":body, "name":name, "feature":feature})
        },
        EditChange::AddBody { object, name, feature_object, feature_name, feature } => json!({
            "kind":"add_body", "object":object, "name":name, "feature_object":feature_object,
            "feature_name":feature_name, "feature":feature_json(feature),
        }),
        EditChange::SetRegions { object, regions } => json!({
            "kind":"set_regions", "object":object, "regions":regions,
        }),
        EditChange::SetOperation { object, operation } => json!({
            "kind":"set_operation", "object":object, "operation":operation,
        }),
        EditChange::SetReversed { object, reversed } => json!({
            "kind":"set_reversed", "object":object, "reversed":reversed,
        }),
        EditChange::SetRevolveAxis { object, axis } => json!({
            "kind":"set_revolve_axis", "object":object, "axis":axis,
        }),
        EditChange::SetRevolveExtent { object, extent } => json!({
            "kind":"set_revolve_extent", "object":object, "extent":turn_json(extent),
        }),
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

fn feature_json(feature: &ferritecad_document::FeatureAddition) -> Value {
    use ferritecad_document::FeatureAddition as F;
    match feature {
        F::Extrude {
            profile,
            regions,
            end_condition,
            reversed,
            operation,
        } => json!({
            "kind":"extrude", "profile":profile, "regions":regions,
            "end_condition":end_json(end_condition),
            "reversed":reversed, "operation":operation,
        }),
        F::Revolve {
            profile,
            regions,
            axis,
            extent,
            operation,
        } => json!({
            "kind":"revolve", "profile":profile, "regions":regions, "axis":axis,
            "extent":turn_json(extent), "operation":operation,
        }),
        F::Fillet { edge, radius_mm } => json!({"kind":"fillet", "edge":edge,
            "radius":{"value":radius_mm.value(),"unit":"mm"}}),
        F::Chamfer { edge, distance_mm } => json!({"kind":"chamfer", "edge":edge,
            "distance":{"value":distance_mm.value(),"unit":"mm"}}),
    }
}
fn turn_json(extent: &ferritecad_document::RevolveExtent) -> Value {
    match extent {
        ferritecad_document::RevolveExtent::FullTurn => json!({"kind":"full_turn"}),
        ferritecad_document::RevolveExtent::Partial { angle } => json!({"kind":"partial",
            "angle":{"value":angle.value(), "unit":"rad"}}),
        _ => unreachable!("fixture covers every current revolution extent"),
    }
}
