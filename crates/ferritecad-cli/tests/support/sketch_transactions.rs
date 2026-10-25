// SPDX-License-Identifier: MIT
use super::*;

fn geometry(sketch: ObjectId, curve: StableEntityId, geometry: SketchGeometry) -> Value {
    ferritecad_fixtures::edit_request_json(&[EditChange::SetCurveGeometry {
        sketch,
        curve,
        geometry,
    }])["changes"][0]
        .clone()
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf, ObjectId, Sketch) {
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let document = Document::open_read_only(&source).expect("document");
    let (id, sketch) = ferritecad_fixtures::sketch(&document).expect("sketch");
    document.close().expect("close");
    (root, source, id, sketch)
}

#[test]
fn sketch_changes_require_kinds_complete_addresses_and_dimensioned_numbers() {
    let (root, source, id, sketch) = fixture();
    let before = std::fs::read(&source).expect("bytes");
    let curve = sketch.curves[0].id;
    let output = root.path().join("refused.fcad");
    let valid = geometry(id, curve, sketch.curves[0].geometry.clone());
    let mut no_kind = valid.clone();
    no_kind.as_object_mut().expect("object").remove("kind");
    let mut no_unit = valid.clone();
    no_unit["geometry"]["start"]["x"]
        .as_object_mut()
        .expect("x")
        .remove("unit");
    let mut wrong_dimension = valid.clone();
    wrong_dimension["geometry"]["end"]["y"]["unit"] = json!("deg");
    let mut unknown_unit = valid.clone();
    unknown_unit["geometry"]["start"]["x"]["unit"] = json!("furlong-ish");
    let mut no_curve = valid.clone();
    no_curve.as_object_mut().expect("object").remove("curve");
    let constraint = StableEntityId::new();
    for invalid in [
        no_kind,
        no_unit,
        wrong_dimension,
        unknown_unit,
        no_curve,
        json!({"kind":"remove_constraint","sketch":id}),
        json!({"kind":"add_constraint","sketch":id,"constraint":{"id":constraint,"rule":{
            "kind":"fixed","point":{"curve":curve,"at":"start"},"x":{"value":2,"unit":"mm"},"y":{"value":3}
        }}}),
        json!({"kind":"add_constraint","sketch":id,"constraint":{"id":constraint,"rule":{
            "kind":"radius","curve":curve,"radius":{"value":2,"unit":"rad"}
        }}}),
    ] {
        let reply = apply(&source, &output, json!([invalid]), 2);
        assert_eq!(reply["error"]["kind"], "input");
        assert!(!output.exists());
        assert_eq!(std::fs::read(&source).expect("bytes"), before);
    }
    let arc = geometry(
        id,
        curve,
        SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0).expect("center"),
            radius: 2.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        },
    );
    for key in ["start_angle", "end_angle"] {
        let mut wrong = arc.clone();
        wrong["geometry"][key]["unit"] = json!("mm");
        let reply = apply(&source, &output, json!([wrong]), 2);
        assert_eq!(reply["error"]["kind"], "input");
        assert!(!output.exists());
    }
}

#[test]
fn inspect_reports_every_sketch_and_preserves_geometry_and_constraint_identities() {
    let (root, source, _, sketch) = fixture();
    let mut document = Document::open(&source).expect("document");
    let id = ObjectId::new();
    let [point, circle, arc] = std::array::from_fn(|_| StableEntityId::new());
    let constraint = StableEntityId::new();
    let extra = Sketch {
        plane: sketch.plane,
        curves: vec![
            SketchCurve {
                id: point,
                construction: true,
                geometry: SketchGeometry::Point {
                    at: Point2::new(2.5, 3.0).expect("point"),
                },
            },
            SketchCurve {
                id: circle,
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(0.0, 0.0).expect("center"),
                    radius: 12.5,
                },
            },
            SketchCurve {
                id: arc,
                construction: true,
                geometry: SketchGeometry::Arc {
                    center: Point2::new(1.0, 2.0).expect("center"),
                    radius: 3.0,
                    start_angle: 0.0,
                    end_angle: std::f64::consts::FRAC_PI_2,
                },
            },
        ],
        constraints: vec![SketchConstraint {
            id: constraint,
            rule: SketchConstraintRule::Radius {
                curve: circle,
                radius: Expression::constant(12.5).expect("radius"),
            },
        }],
    };
    document
        .write(|w| {
            w.put_object(
                id,
                None,
                20,
                Some("Additional sketch"),
                &ObjectPayload::Sketch(extra.clone()),
            )?;
            w.add_dependency(Dependency {
                dependent: id,
                dependency: sketch.plane,
                role: DependencyRole::Plane,
            })
        })
        .expect("additional sketch");
    document.close().expect("close");
    let before = std::fs::read(&source).expect("bytes");
    let report = inspect(&source);
    let sketches = report["sketches"].as_array().expect("sketches");
    assert_eq!(sketches.len(), 2);
    let row = sketches
        .iter()
        .find(|s| s["sketch_id"] == id.to_string())
        .expect("second sketch");
    assert_eq!(row["name"], "Additional sketch");
    assert_eq!(row["plane_id"], sketch.plane.to_string());
    assert_eq!(row["curves"][0]["curve_id"], point.to_string());
    assert_eq!(
        row["curves"][0]["geometry"],
        json!({"kind":"point","at_mm":[2.5,3.0]})
    );
    assert_eq!(row["curves"][1]["geometry"]["radius_mm"], 12.5);
    assert_eq!(row["curves"][2]["geometry"]["end_angle_deg"], 90.0);
    assert_eq!(
        row["constraints"][0]["constraint_id"],
        constraint.to_string()
    );
    assert_eq!(
        row["constraints"][0]["rule"]["radius"],
        json!({"value":12.5,"unit":"mm"})
    );
    for row in sketches {
        assert_eq!(row.as_object().expect("sketch").len(), 5);
    }
    assert_eq!(std::fs::read(&source).expect("bytes"), before);
    assert!(!root.path().join("source.fcad-wal").exists());
}

#[test]
fn native_single_vertex_of_the_second_body_moves_in_a_mixed_transaction() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let (root, source, first, sketch) = fixture();
    let mut document = Document::open(&source).expect("document");
    let second = ObjectId::new();
    let feature = ObjectId::new();
    let body = ObjectId::new();
    let mut copy = sketch.clone();
    for curve in &mut copy.curves {
        curve.id = StableEntityId::new();
    }
    document
        .write(|w| {
            w.put_object(
                second,
                None,
                20,
                Some("Second sketch"),
                &ObjectPayload::Sketch(copy.clone()),
            )?;
            w.add_dependency(Dependency {
                dependent: second,
                dependency: copy.plane,
                role: DependencyRole::Plane,
            })?;
            w.put_object(
                feature,
                None,
                21,
                Some("Second extrusion"),
                &ObjectPayload::Extrude(Extrude {
                    profile: second,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(10.0)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    previous: None,
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: feature,
                dependency: second,
                role: DependencyRole::Profile,
            })?;
            w.put_object(
                body,
                None,
                22,
                Some("Second body"),
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
        .expect("second body");
    document.close().expect("close");
    let original = std::fs::read(&source).expect("bytes");
    let mut changes = move_sketch_vertices(
        second,
        &copy,
        &[SketchVertex {
            curve_id: copy.curves[0].id,
            start_mm: [-5.0, 2.0],
        }],
    )
    .expect("move vertex");
    changes.push(
        EditChange::constant(feature, ExpressionSlot::ExtrudeDistance, 20.0).expect("height"),
    );
    let mut wire = ferritecad_fixtures::edit_request_json(&changes)["changes"].clone();
    // Length aliases are parsed by Unit at every numeric edge.
    for change in wire.as_array_mut().expect("changes") {
        if let Some(geometry) = change.get_mut("geometry") {
            for point in ["start", "end"] {
                for axis in ["x", "y"] {
                    let number = &mut geometry[point][axis];
                    number["value"] = json!(number["value"].as_f64().expect("number") / 10.0);
                    number["unit"] = json!("cm");
                }
            }
        }
    }
    let output = root.path().join("edited.fcad");
    let reply = apply(&source, &output, wire, 0);
    assert_eq!(
        reply["result"]["destination"],
        output.to_str().expect("path")
    );
    let saved = Document::open_read_only(&output).expect("reopen");
    assert_eq!(
        saved.object(first).expect("row").expect("first").payload,
        ObjectPayload::Sketch(sketch)
    );
    let ObjectPayload::Sketch(edited) = saved.object(second).expect("row").expect("second").payload
    else {
        panic!("sketch")
    };
    assert_eq!(edited.curves[0].id, copy.curves[0].id);
    assert!(
        matches!(edited.curves[0].geometry,SketchGeometry::Line {start,..} if start.x == -5.0 && start.y == 2.0)
    );
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = ferritecad_eval::rebuild_cold(
        &saved,
        &mut kernel,
        &ferritecad_kernel::OperationContext::default(),
    )
    .expect("cold");
    assert!(built.report().all_built());
    crate::names::assert_primary(&built);
    built.release_all(&mut kernel);
    saved.close().expect("close");
    assert_eq!(std::fs::read(&source).expect("bytes"), original);
}
