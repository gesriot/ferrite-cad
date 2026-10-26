// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_kernel::{OperationContext, TessellationParams};

fn blind(distance: f64) -> EndCondition {
    EndCondition::Blind {
        distance: Expression::constant(distance).expect("distance"),
    }
}
fn checked_volume(path: &Path, feature: ObjectId) -> f64 {
    let d = Document::open_read_only(path).expect("document");
    assert!(d.validate().expect("validate").is_ok());
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built =
        ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default()).expect("rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    let shape = built.shape(feature).expect("shape");
    assert!(k.is_valid(shape).expect("valid solid"));
    let volume = k.shape_stats(shape).expect("stats").1;
    built.release_all(&mut k);
    volume
}
fn publish_with_history(source: &Path, output: &Path, changes: &[EditChange]) {
    let original = std::fs::read(source).expect("original");
    let json = ferritecad_fixtures::edit_request_json(changes);
    apply(source, output, json["changes"].clone(), 0);
    let accepted = Document::open_read_only(output).expect("accepted");
    let model = accepted.model_version().expect("model");
    let expected_names = names::snapshot(&accepted);
    let mut session = ferritecad_jobs::DocumentSession::open(source).expect("session");
    let baseline = session.current().model();
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    session
        .current()
        .load_scene(
            &mut k,
            |_, _| unreachable!("no imports"),
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("baseline");
    let step = session
        .begin_step()
        .edit(
            changes.to_vec(),
            session.current().version(),
            &mut k,
            |_, _| unreachable!("no imports"),
            &OperationContext::default(),
        )
        .expect("step");
    session.commit_step(step).expect("commit");
    assert_eq!(session.current().model(), model);
    session
        .commit_move(session.begin_undo().expect("undo"))
        .expect("commit undo");
    assert_eq!(session.current().model(), baseline);
    session
        .commit_move(session.begin_redo().expect("redo"))
        .expect("commit redo");
    assert_eq!(session.current().model(), model);
    let redone = Document::open_read_only(session.current().path()).expect("redone");
    assert_eq!(names::snapshot(&redone), expected_names);
    assert_eq!(std::fs::read(source).expect("source preserved"), original);
}
fn circle_changes(
    d: &Document,
    center: [f64; 2],
    radius: f64,
) -> (ObjectId, StableEntityId, Vec<EditChange>) {
    let plane = d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::DatumPlane(_)))
        .expect("plane")
        .id;
    let sketch = ObjectId::new();
    let axis = StableEntityId::new();
    (
        sketch,
        axis,
        vec![EditChange::AddSketch {
            object: sketch,
            name: Some("Tool".into()),
            sketch: Sketch {
                plane,
                constraints: vec![],
                curves: vec![
                    SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Circle {
                            center: Point2::new(center[0], center[1]).expect("center"),
                            radius,
                        },
                    },
                    SketchCurve {
                        id: axis,
                        construction: true,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(0., 0.).expect("start"),
                            end: Point2::new(0., 1.).expect("end"),
                        },
                    },
                ],
            },
        }],
    )
}

#[test]
fn generic_sweeps_edit_cli_and_session_history_share_one_path() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    for kind in [
        "add",
        "intersect",
        "revolve_cut",
        "new_extrude",
        "new_revolve",
    ] {
        let source = root.path().join(format!("{kind}-source.fcad"));
        ferritecad_fixtures::write_plate(&source).expect("plate");
        let d = Document::open_read_only(&source).expect("source");
        let body = ferritecad_fixtures::feature_changes::body(&d).expect("body");
        let (center, radius) = if kind == "new_revolve" {
            ([10., 10.], 2.)
        } else if kind == "new_extrude" {
            ([100., 100.], 3.)
        } else {
            ([20., 20.], 3.)
        };
        let (profile, axis, mut changes) = circle_changes(&d, center, radius);
        let feature = ObjectId::new();
        let addition = match kind {
            "revolve_cut" | "new_revolve" => FeatureAddition::Revolve {
                regions: Vec::new(),
                profile,
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
                operation: if kind == "revolve_cut" {
                    SolidOperation::Cut
                } else {
                    SolidOperation::NewBody
                },
            },
            _ => FeatureAddition::Extrude {
                regions: Vec::new(),
                profile,
                end_condition: blind(14.),
                reversed: false,
                operation: match kind {
                    "add" => SolidOperation::Add,
                    "intersect" => SolidOperation::Intersect,
                    _ => SolidOperation::NewBody,
                },
            },
        };
        changes.push(if kind.starts_with("new_") {
            EditChange::AddBody {
                object: ObjectId::new(),
                name: Some("Second".into()),
                feature_object: feature,
                feature_name: Some("Sweep".into()),
                feature: addition,
            }
        } else {
            EditChange::AddFeature {
                object: feature,
                body,
                name: Some("Sweep".into()),
                feature: addition,
            }
        });
        let output = root.path().join(format!("{kind}.fcad"));
        publish_with_history(&source, &output, &changes);
        let volume = checked_volume(&output, feature);
        let pi = std::f64::consts::PI;
        let expected = match kind {
            "add" => Some(24000. + 36. * pi),
            "intersect" => Some(90. * pi),
            "new_extrude" => Some(126. * pi),
            "new_revolve" => Some(80. * pi * pi),
            _ => None,
        };
        if let Some(expected) = expected {
            assert!(
                (volume - expected).abs() < expected * 1e-8,
                "{kind}: {volume} != {expected}"
            );
        } else {
            assert!(volume > 0. && volume < 24000.);
        }
        let edits = match kind {
            "add" => vec![EditChange::SetOperation {
                object: feature,
                operation: SolidOperation::Intersect,
            }],
            "new_extrude" => vec![
                EditChange::SetReversed {
                    object: feature,
                    reversed: true,
                },
                EditChange::SetEndCondition {
                    object: feature,
                    end_condition: EndCondition::Symmetric {
                        distance: Expression::constant(7.).expect("distance"),
                    },
                },
            ],
            "new_revolve" => vec![
                EditChange::SetRevolveAxis {
                    object: feature,
                    axis: RevolveAxis::Line(axis),
                },
                EditChange::SetRevolveExtent {
                    object: feature,
                    extent: RevolveExtent::Partial {
                        angle: Expression::constant(pi).expect("angle"),
                    },
                },
            ],
            _ => vec![],
        };
        if !edits.is_empty() {
            let edited = root.path().join(format!("{kind}-edited.fcad"));
            publish_with_history(&output, &edited, &edits);
            let v = checked_volume(&edited, feature);
            let expected = match kind {
                "add" => 90. * pi,
                "new_revolve" => 40. * pi * pi,
                _ => volume,
            };
            assert!((v - expected).abs() < expected * 1e-8);
            if kind == "new_revolve" {
                let other = root.path().join("other-axis.fcad");
                publish_with_history(
                    &edited,
                    &other,
                    &[EditChange::SetRevolveAxis {
                        object: feature,
                        axis: RevolveAxis::SketchX,
                    }],
                );
                assert!((checked_volume(&other, feature) - expected).abs() < expected * 1e-8);
            }
        }
    }
}

#[test]
fn changing_an_add_to_cut_refuses_the_dependent_empty_intersection_atomically() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let d = Document::open_read_only(&source).expect("source");
    let body = ferritecad_fixtures::feature_changes::body(&d).expect("body");
    let (profile, _, mut changes) = circle_changes(&d, [20., 20.], 3.);
    let add = ObjectId::new();
    let dependent = ObjectId::new();
    for (object, operation) in [
        (add, SolidOperation::Add),
        (dependent, SolidOperation::Intersect),
    ] {
        changes.push(EditChange::AddFeature {
            object,
            body,
            name: Some("Sweep".into()),
            feature: FeatureAddition::Extrude {
                regions: Vec::new(),
                profile,
                operation,
                reversed: false,
                end_condition: blind(14.),
            },
        });
    }
    let accepted = root.path().join("accepted.fcad");
    publish_with_history(&source, &accepted, &changes);
    let before = std::fs::read(&accepted).expect("bytes");
    let out = root.path().join("refused.fcad");
    let edits = [EditChange::SetOperation {
        object: add,
        operation: SolidOperation::Cut,
    }];
    let request = ferritecad_fixtures::edit_request_json(&edits);
    let result = apply(&accepted, &out, request["changes"].clone(), 2);
    assert!(
        result["error"]["message"]
            .as_str()
            .expect("message")
            .contains(&dependent.to_string()),
        "{result}"
    );
    assert!(!out.exists());
    assert_eq!(std::fs::read(&accepted).expect("preserved"), before);
}
