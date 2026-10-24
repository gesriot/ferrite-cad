// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_types::{ObjectId, StableEntityId, Transform};
use serde_json::{Value, json};
use std::{path::Path, process::Command};
#[path = "support/sketch_transactions.rs"]
mod sketch_transactions;
#[path = "support/transaction.rs"]
mod transaction;

fn inspect(path: &Path) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg("inspect")
        .arg(path)
        .arg("--json")
        .output()
        .expect("inspect");
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice::<Value>(&out.stdout).expect("JSON")["result"].clone()
}
fn apply(source: &Path, output: &Path, changes: Value, code: i32) -> Value {
    let request = tempfile::NamedTempFile::new().expect("request file");
    std::fs::write(
        request.path(),
        serde_json::to_vec(&json!({"request_version":1,"changes":changes})).expect("JSON"),
    )
    .expect("write");
    let version = inspect(source)["content_version"]
        .as_str()
        .expect("version")
        .to_owned();
    let out = transaction::command(source, &version, request.path(), output)
        .output()
        .expect("edit");
    assert_eq!(out.status.code(), Some(code), "{out:?}");
    let reply: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(reply["operation"], "edit");
    reply
}
fn change(slot: &ExpressionSlotValue, value: f64, unit: &str) -> Value {
    json!({"kind":"set_expression","object":slot.object,"slot":slot.slot,"value":value,"unit":unit})
}
fn assert_saved(path: &Path, slot: &ExpressionSlotValue, internal: f64, display: f64) {
    let d = Document::open_read_only(path).expect("reopen");
    let slots = d.expression_slots().expect("slots");
    let saved = slots
        .iter()
        .find(|s| s.object == slot.object && s.slot == slot.slot)
        .expect("slot");
    assert!((saved.expression.value() - internal).abs() < 1e-12);
    assert_eq!(
        saved.expression,
        Expression::constant(internal).expect("constant")
    );
    let report = inspect(path);
    let row = report["expression_slots"]
        .as_array()
        .expect("slots")
        .iter()
        .find(|s| s["object"] == slot.object.to_string() && s["slot"] == json!(slot.slot))
        .expect("inspect slot");
    assert_eq!(row["display_value"], display);
}

#[test]
fn edit_requires_units_version_and_known_slots_without_changing_files() {
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let original = std::fs::read(&source).expect("source");
    let d = Document::open_read_only(&source).expect("doc");
    let slot = d.expression_slots().expect("slots").remove(0);
    let output = root.path().join("output.fcad");
    for request in [
        json!([{"kind":"set_expression","object":slot.object,"slot":slot.slot,"value":27}]),
        json!([change(&slot, 27.0, "deg")]),
        json!([change(&slot, 27.0, "unknown")]),
        json!([{"kind":"set_expression","object":slot.object,"slot":{"kind":"not_a_slot"},"value":27,"unit":"mm"}]),
    ] {
        let reply = apply(&source, &output, request, 2);
        assert_eq!(reply["error"]["kind"], "input");
        assert!(!output.exists());
    }
    let request = root.path().join("request.json");
    transaction::write(
        &request,
        &slot.object.to_string(),
        "extrude_distance",
        27.0,
        "mm",
    );
    let out = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg("edit")
        .arg(&source)
        .arg("--request")
        .arg(&request)
        .arg("--output")
        .arg(&output)
        .output()
        .expect("missing version");
    assert_eq!(out.status.code(), Some(2));
    assert!(!output.exists());
    assert_eq!(std::fs::read(&source).expect("unchanged"), original);
    if !ferritecad_occt::is_available() {
        let result = apply(&source, &output, json!([change(&slot, 27.0, "mm")]), 2);
        assert!(
            result["error"]["message"]
                .as_str()
                .expect("message")
                .contains("Open CASCADE")
        );
        assert!(!output.exists());
    }
}

#[test]
fn native_feature_slots_round_trip_and_the_second_body_is_editable() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    for mode in [
        "blind",
        "symmetric",
        "fillet",
        "chamfer",
        "revolve",
        "two_bodies",
    ] {
        let source = root.path().join(format!("{mode}.fcad"));
        ferritecad_fixtures::write_plate(&source).expect("plate");
        let mut d = Document::open(&source).expect("document");
        let objects = d.objects().expect("objects");
        let base = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("base");
        let body = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
            .expect("body");
        let mut chosen = base.id;
        let kind = match mode {
            "symmetric" => {
                let ObjectPayload::Extrude(mut e) = base.payload.clone() else {
                    panic!("extrude");
                };
                e.end_condition = EndCondition::Symmetric {
                    distance: Expression::constant(10.0).expect("height"),
                };
                d.write(|w| {
                    w.put_object(
                        base.id,
                        base.parent,
                        base.ordinal,
                        base.name.as_deref(),
                        &ObjectPayload::Extrude(e),
                    )
                })
                .expect("symmetric");
                ExpressionSlot::ExtrudeDistance
            }
            "fillet" | "chamfer" => {
                let target = ExtrudeEditSource::read(&d).expect("reading").fillet_bodies[0]
                    .target
                    .clone()
                    .expect("target");
                let edge = SweptEdge {
                    feature: base.id,
                    joint: target.corners[0].joint,
                };
                if mode == "fillet" {
                    let p = prepare_edge_fillet(
                        &d,
                        body.id,
                        &EdgeFillet {
                            edge,
                            radius_mm: 2.0,
                        },
                    )
                    .expect("fillet");
                    d.write_edge_fillet(&p).expect("write");
                    chosen = p.feature().id;
                    ExpressionSlot::FilletRadius
                } else {
                    let p = prepare_edge_chamfer(
                        &d,
                        body.id,
                        &EdgeChamfer {
                            edge,
                            distance_mm: 2.0,
                        },
                    )
                    .expect("chamfer");
                    d.write_edge_chamfer(&p).expect("write");
                    chosen = p.feature().id;
                    ExpressionSlot::ChamferDistance
                }
            }
            "revolve" => {
                // Build a separate radial-clear profile, with no extrusion references.
                d.close().expect("close");
                std::fs::remove_file(&source).expect("replace fixture");
                polygon(&source, false, true);
                d = Document::open(&source).expect("revolve");
                chosen = d
                    .objects()
                    .expect("objects")
                    .iter()
                    .find(|o| matches!(o.payload, ObjectPayload::Revolve(_)))
                    .expect("revolve")
                    .id;
                ExpressionSlot::RevolveAngle
            }
            "two_bodies" => {
                chosen = ObjectId::new();
                let second_body = ObjectId::new();
                let ObjectPayload::Extrude(e) = &base.payload else {
                    panic!("extrude");
                };
                d.write(|w| {
                    w.put_object(chosen, None, 20, Some("Second extrusion"), &base.payload)?;
                    w.add_dependency(Dependency {
                        dependent: chosen,
                        dependency: e.profile,
                        role: DependencyRole::Profile,
                    })?;
                    w.put_object(
                        second_body,
                        None,
                        21,
                        Some("Second body"),
                        &ObjectPayload::Body(Body {
                            tip_feature: Some(chosen),
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: second_body,
                        dependency: chosen,
                        role: DependencyRole::BodyTip,
                    })
                })
                .expect("second body");
                ExpressionSlot::ExtrudeDistance
            }
            _ => ExpressionSlot::ExtrudeDistance,
        };
        let slot = d
            .expression_slots()
            .expect("slots")
            .into_iter()
            .find(|s| s.object == chosen && s.slot == kind)
            .expect("slot");
        d.close().expect("close");
        for value in if mode == "revolve" {
            vec![90.0, 30.0]
        } else {
            vec![3.0]
        } {
            let output = root.path().join(format!("{mode}-{value}.fcad"));
            let unit = if mode == "revolve" { "deg" } else { "mm" };
            let reply = apply(&source, &output, json!([change(&slot, value, unit)]), 0);
            assert_eq!(
                reply["result"]["content_version"],
                inspect(&output)["content_version"]
            );
            assert_saved(
                &output,
                &slot,
                if mode == "revolve" {
                    value.to_radians()
                } else {
                    value
                },
                value,
            );
        }
    }
}

fn polygon(path: &Path, constraints: bool, revolve: bool) {
    let mut d = Document::create(path).expect("doc");
    let [plane, sketch, feature, body] = std::array::from_fn(|_| ObjectId::new());
    let edges: [StableEntityId; 4] = std::array::from_fn(|_| StableEntityId::new());
    let points = [[5.0, 0.0], [60.0, 0.0], [60.0, 40.0], [5.0, 40.0]];
    let start = |i| SketchPointRef::new(edges[i], SketchPointSelector::Start);
    let end = |i| SketchPointRef::new(edges[i], SketchPointSelector::End);
    let mut rules = Vec::new();
    if constraints {
        for i in 0..4 {
            rules.push(SketchConstraintRule::Coincident {
                a: end(i),
                b: start((i + 1) % 4),
            });
        }
        rules.extend([
            SketchConstraintRule::Fixed {
                point: start(0),
                x: Expression::constant(5.0).expect("x"),
                y: Expression::constant(0.0).expect("y"),
            },
            SketchConstraintRule::Horizontal {
                a: start(0),
                b: end(0),
            },
            SketchConstraintRule::Distance {
                a: start(0),
                b: end(0),
                distance: Expression::constant(55.0).expect("distance"),
            },
            SketchConstraintRule::DirectedAngle {
                a: SketchSegmentRef::new(start(0), end(0)),
                b: SketchSegmentRef::new(start(1), end(1)),
                angle: Expression::constant(std::f64::consts::FRAC_PI_2).expect("angle"),
            },
        ]);
    }
    d.write(|w| {
        w.put_object(
            plane,
            None,
            0,
            None,
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        w.put_object(
            sketch,
            None,
            1,
            None,
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves: (0..4)
                    .map(|i| SketchCurve {
                        id: edges[i],
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(points[i][0], points[i][1]).expect("point"),
                            end: Point2::new(points[(i + 1) % 4][0], points[(i + 1) % 4][1])
                                .expect("point"),
                        },
                    })
                    .collect(),
                constraints: rules
                    .into_iter()
                    .map(|rule| SketchConstraint {
                        id: StableEntityId::new(),
                        rule,
                    })
                    .collect(),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: sketch,
            dependency: plane,
            role: DependencyRole::Plane,
        })?;
        let payload = if revolve {
            ObjectPayload::Revolve(Revolve {
                profile: sketch,
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::Partial {
                    angle: Expression::constant(std::f64::consts::PI)?,
                },
                operation: SolidOperation::NewBody,
                axis_segment: None,
            })
        } else {
            ObjectPayload::Extrude(Extrude {
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(10.0)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                previous: None,
            })
        };
        w.put_object(feature, None, 2, None, &payload)?;
        w.add_dependency(Dependency {
            dependent: feature,
            dependency: sketch,
            role: DependencyRole::Profile,
        })?;
        w.put_object(
            body,
            None,
            3,
            None,
            &ObjectPayload::Body(Body {
                tip_feature: Some(feature),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: body,
            dependency: feature,
            role: DependencyRole::BodyTip,
        })
    })
    .expect("fixture");
    d.close().expect("close");
}

#[test]
fn native_constraint_slots_use_the_solver_and_internal_units() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("constraints.fcad");
    polygon(&source, true, false);
    let d = Document::open_read_only(&source).expect("doc");
    for (index, slot) in d.expression_slots().expect("slots").iter().enumerate() {
        if slot.slot == ExpressionSlot::ExtrudeDistance {
            continue;
        }
        let angular = matches!(slot.slot, ExpressionSlot::DirectedAngle { .. });
        let value = if angular {
            30.0
        } else {
            slot.expression.value() + 2.0
        };
        let output = root.path().join(format!("constraint-{index}.fcad"));
        apply(
            &source,
            &output,
            json!([change(slot, value, if angular { "deg" } else { "mm" })]),
            0,
        );
        assert_saved(
            &output,
            slot,
            if angular { value.to_radians() } else { value },
            value,
        );
    }
    d.close().expect("close");
    let mut d = Document::open(&source).expect("doc");
    let record = d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch");
    let ObjectPayload::Sketch(mut sketch) = record.payload else {
        panic!("sketch");
    };
    let circle = StableEntityId::new();
    sketch.curves = vec![SketchCurve {
        id: circle,
        construction: false,
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0).expect("point"),
            radius: 10.0,
        },
    }];
    sketch.constraints = vec![SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Radius {
            curve: circle,
            radius: Expression::constant(10.0).expect("radius"),
        },
    }];
    d.write(|w| {
        w.put_object(
            record.id,
            record.parent,
            record.ordinal,
            record.name.as_deref(),
            &ObjectPayload::Sketch(sketch),
        )
    })
    .expect("circle");
    let slot = d
        .expression_slots()
        .expect("slots")
        .into_iter()
        .find(|s| matches!(s.slot, ExpressionSlot::Radius { .. }))
        .expect("radius slot");
    d.close().expect("close");
    let output = root.path().join("radius.fcad");
    apply(&source, &output, json!([change(&slot, 12.0, "mm")]), 0);
    assert_saved(&output, &slot, 12.0, 12.0);
}
