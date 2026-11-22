// SPDX-License-Identifier: MIT
use super::*;
use crate::sketch_mode::tests::{
    widgets::{Ui, text},
    window::{apply, enter, open, version},
};
use crate::{
    commands,
    sessions::tests::tabs::{Drawn, Window},
};
use ferritecad_document::{Document, Sketch};
use ferritecad_ui::ModelEdit;

#[path = "creation.rs"]
pub(crate) mod creation;

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
}
fn request(w: &mut Window, ui: &mut Ui) -> EditRequest {
    ui.press(w, "Apply", true);
    w.creates
        .sketch
        .sweeps
        .take_request()
        .expect("one Apply request")
}
fn objects(w: &Window) -> Vec<ObjectRecord> {
    Document::open(
        w.sessions
            .export_source()
            .expect("fixture operation")
            .path(),
    )
    .expect("fixture operation")
    .objects()
    .expect("fixture operation")
}
fn saved_extrude(w: &Window, id: ObjectId) -> ferritecad_document::Extrude {
    let ObjectPayload::Extrude(e) = objects(w)
        .into_iter()
        .find(|o| o.id == id)
        .expect("fixture operation")
        .payload
    else {
        panic!("Extrude")
    };
    e
}
fn feature_id(request: &EditRequest) -> ObjectId {
    match &request.changes[..] {
        [EditChange::AddBody { feature_object, .. }] => *feature_object,
        [EditChange::AddFeature { object, .. }] => *object,
        _ => panic!("one sweep"),
    }
}
fn choose(ui: &mut Ui, w: &mut Window, current: &str, next: &str) {
    ui.press(w, current, true);
    ui.press(w, next, true);
}
fn offset_rectangle() -> Vec<ferritecad_document::SketchCurve> {
    vec![
        line([5., 0.], [10., 0.]),
        line([10., 0.], [10., 8.]),
        line([10., 8.], [5., 8.]),
        line([5., 8.], [5., 0.]),
    ]
}
fn tool(f: &mut Drawing, curves: Vec<ferritecad_document::SketchCurve>) -> ObjectId {
    let id = ObjectId::new();
    fixtures::put(
        &mut f.doc,
        id,
        "Tool",
        ObjectPayload::Sketch(Sketch {
            plane: f.plane,
            curves,
            constraints: vec![],
        }),
    );
    id
}

#[test]
fn native_rectangle_drawn_with_widgets_becomes_one_body_and_one_undo_keeps_the_sketch() {
    if !native() {
        return;
    }
    use crate::sketch_mode::tests::window::{click, finish, start_new};
    let root = tempfile::tempdir().expect("fixture operation");
    let path = root.path().join("empty.fcad");
    Document::create(&path)
        .expect("fixture operation")
        .close()
        .expect("fixture operation");
    let bytes = std::fs::read(&path).expect("fixture operation");
    let mut w = open(&path, Drawn::Native);
    let mut ui = Ui::new(&mut w);
    start_new(&mut w, &mut ui);
    ui.press(&mut w, "Line", false);
    for p in [
        [-10., -10.],
        [10., -10.],
        [10., 10.],
        [-10., 10.],
        [-10., -10.],
    ] {
        click(&mut w, point(p[0], p[1]), false);
    }
    let drawing = finish(&mut w, &mut ui);
    assert!(apply(&mut w, drawing));
    let before = version(&w);
    let sketch = objects(&w)
        .into_iter()
        .find(|o| o.name.as_deref() == Some("Sketch 1"))
        .expect("fixture operation")
        .id;
    enter(&mut w, sketch, ModelEdit::Extrude);
    let labels = text(&ui.settle(&mut w));
    assert!(labels.iter().any(|s| s == "Extrude Sketch 1"));
    assert!(
        labels
            .iter()
            .any(|s| s == "All closed regions of the sketch are used.")
    );
    let asked = request(&mut w, &mut ui);
    let id = feature_id(&asked);
    assert!(apply(&mut w, asked));
    assert_ne!(version(&w), before);
    assert!(!w.creates.sketch.saved_object_open());
    let saved = objects(&w);
    assert_eq!(
        saved
            .iter()
            .filter(|o| o.name.as_deref() == Some("Body 1"))
            .count(),
        1
    );
    assert_eq!(
        saved
            .iter()
            .filter(|o| o.name.as_deref() == Some("Extrude 1"))
            .count(),
        1
    );
    assert_eq!(
        saved_extrude(&w, id).end_condition,
        EndCondition::Blind {
            distance: Expression::constant(10.).expect("fixture operation")
        }
    );
    assert_eq!(
        w.scene
            .edit_source
            .as_ref()
            .expect("fixture operation")
            .model_tree
            .rows
            .iter()
            .filter_map(|r| r.solid_count)
            .collect::<Vec<_>>(),
        vec![1]
    );
    w.step(true);
    assert_eq!(version(&w), before);
    assert_eq!(objects(&w).len(), 2);
    assert!(objects(&w).iter().any(|o| o.id == sketch));
    assert_eq!(std::fs::read(path).expect("fixture operation"), bytes);
}

#[test]
fn native_add_blind_widgets_use_the_selected_existing_body() {
    if !native() {
        return;
    }
    let mut f = Drawing::new(rectangle(), true);
    let sketch = tool(&mut f, offset_rectangle());
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, sketch, ModelEdit::Extrude);
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Add", true);
    let labels = text(&ui.settle(&mut w));
    assert!(
        labels
            .iter()
            .any(|s| s == "Inserted after Extrude 'Extrude'")
    );
    let asked = request(&mut w, &mut ui);
    let id = feature_id(&asked);
    assert!(
        matches!(asked.changes.as_slice(), [EditChange::AddFeature { body, .. }] if *body == f.body)
    );
    assert!(apply(&mut w, asked));
    assert_eq!(saved_extrude(&w, id).previous, Some(f.feature));
    assert_eq!(saved_extrude(&w, id).operation, SolidOperation::Add);
    assert_eq!(
        objects(&w)
            .iter()
            .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
            .count(),
        1
    );
}

#[test]
fn native_cut_through_all_widgets_add_faces_to_the_complete_body() {
    if !native() {
        return;
    }
    let mut f = Drawing::new(rectangle(), true);
    let sketch = tool(
        &mut f,
        vec![curve(SketchGeometry::Circle {
            center: point(5., 4.),
            radius: 1.,
        })],
    );
    let mut w = open(&f.path, Drawn::Native);
    let before = faces(&w);
    enter(&mut w, sketch, ModelEdit::Extrude);
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Cut", true);
    choose(&mut ui, &mut w, "Blind", "Through all");
    let asked = request(&mut w, &mut ui);
    let id = feature_id(&asked);
    assert!(apply(&mut w, asked));
    let e = saved_extrude(&w, id);
    assert_eq!(e.operation, SolidOperation::Cut);
    assert_eq!(e.end_condition, EndCondition::ThroughAll);
    assert!(faces(&w) > before);
}

#[test]
fn native_symmetric_and_reversed_widgets_store_both_intents() {
    if !native() {
        return;
    }
    let f = Drawing::new(rectangle(), false);
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Extrude);
    let mut ui = Ui::new(&mut w);
    choose(&mut ui, &mut w, "Blind", "Symmetric");
    ui.type_value(&mut w, "10", "6.25");
    ui.press(&mut w, "Reversed", true);
    let asked = request(&mut w, &mut ui);
    let id = feature_id(&asked);
    assert!(apply(&mut w, asked));
    let e = saved_extrude(&w, id);
    assert_eq!(
        e.end_condition,
        EndCondition::Symmetric {
            distance: Expression::constant(6.25).expect("fixture operation")
        }
    );
    assert!(e.reversed);
}

fn revolve_widgets(partial: bool, line_axis: bool) {
    if !native() {
        return;
    }
    let mut curves = offset_rectangle();
    let mut axis = line([0., -5.], [0., 10.]);
    axis.construction = true;
    let axis_id = axis.id;
    if line_axis {
        curves.push(axis);
    }
    let f = Drawing::new(curves, false);
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Revolve);
    let mut ui = Ui::new(&mut w);
    if partial {
        ui.press(&mut w, "Angle (°)", true);
    }
    if line_axis {
        choose(&mut ui, &mut w, "Sketch Y", "Line 5: (0, -5) → (0, 10) mm");
    }
    let asked = request(&mut w, &mut ui);
    let id = feature_id(&asked);
    assert!(apply(&mut w, asked));
    let ObjectPayload::Revolve(r) = objects(&w)
        .into_iter()
        .find(|o| o.id == id)
        .expect("fixture operation")
        .payload
    else {
        panic!("Revolve")
    };
    assert_eq!(
        r.extent,
        if partial {
            RevolveExtent::Partial {
                angle: Expression::constant(std::f64::consts::FRAC_PI_2)
                    .expect("fixture operation"),
            }
        } else {
            RevolveExtent::FullTurn
        }
    );
    assert_eq!(
        r.axis,
        if line_axis {
            RevolveAxis::Line(axis_id)
        } else {
            RevolveAxis::SketchY
        }
    );
}
#[test]
fn native_revolve_full_turn_widgets_build_about_sketch_y() {
    revolve_widgets(false, false);
}
#[test]
fn native_revolve_partial_widgets_store_ninety_degrees_as_radians() {
    revolve_widgets(true, false);
}
#[test]
fn native_revolve_widgets_accept_a_construction_line_axis() {
    revolve_widgets(false, true);
}

#[test]
fn open_polyline_keeps_both_commands_disabled_with_the_accepted_complete_reason() {
    let c = line([0., 0.], [10., 0.]);
    let curve_id = c.id;
    let f = Drawing::new(vec![c], false);
    let mut w = open(&f.path, Drawn::Mock);
    let source = w.scene.edit_source.as_ref().expect("fixture operation");
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|r| r.id.0 == f.sketch)
        .expect("fixture operation")
        .id;
    let offers = commands::offers(source, row, Ok(commands::Access::Idle));
    let reason = format!(
        "sketch 'Drawing' gives no profile: invalid input: the profile does not close: \
         curve {curve_id} has a free end at (10, 0)"
    );
    assert_eq!(
        commands::actions(source, row),
        vec![
            ModelEdit::Sketch,
            ModelEdit::Extrude,
            ModelEdit::Revolve,
            ModelEdit::Constraints
        ]
    );
    for (edit, kind) in [
        (ModelEdit::Extrude, Kind::Extrude),
        (ModelEdit::Revolve, Kind::Revolve),
    ] {
        assert_eq!(
            offers
                .iter()
                .find(|o| o.command == commands::Command::Edit(edit))
                .expect("fixture operation")
                .available,
            Err(reason.clone())
        );
        assert!(!w.creates.sketch.sweeps.begin_with(
            &f.path,
            source,
            &w.scene.sketches,
            f.sketch,
            kind
        ));
    }
    assert!(!w.creates.sketch.active());
}

fn faces(w: &Window) -> usize {
    ferritecad_scene::snapshot_of(
        w.sessions
            .export_source()
            .expect("fixture operation")
            .path(),
        &mut ferritecad_occt::OcctKernel::new().expect("fixture operation"),
        ferritecad_occt::OcctKernel::import_step,
        &Default::default(),
        &Default::default(),
    )
    .expect("fixture operation")
    .snapshot
    .meshes()
    .iter()
    .map(|m| m.face_count())
    .sum()
}

#[test]
fn native_axis_crossing_refusal_preserves_the_form_version_and_file_until_cancel() {
    if !native() {
        return;
    }
    let curves = vec![
        line([-5., 0.], [5., 0.]),
        line([5., 0.], [5., 8.]),
        line([5., 8.], [-5., 8.]),
        line([-5., 8.], [-5., 0.]),
    ];
    let f = Drawing::new(curves, false);
    let mut w = open(&f.path, Drawn::Native);
    let before = version(&w);
    let bytes = std::fs::read(&f.path).expect("fixture operation");
    let accepted = w
        .sessions
        .export_source()
        .expect("fixture operation")
        .path()
        .to_owned();
    let accepted_bytes = std::fs::read(&accepted).expect("fixture operation");
    enter(&mut w, f.sketch, ModelEdit::Revolve);
    let mut ui = Ui::new(&mut w);
    let asked = request(&mut w, &mut ui);
    let [
        EditChange::AddBody {
            object: body,
            feature_object: feature,
            ..
        },
    ] = &asked.changes[..]
    else {
        panic!("AddBody")
    };
    let reason = "geometry kernel failure: revolving a profile: the revolution did not produce \
                  a shape: the profile may cross the axis of revolution";
    let expected = format!(
        "Could not apply the change: Revolve 1 ({feature}), Body 1 ({body}): \
         edit would leave features unbuilt:\n\
         {feature}: {reason}\n{body}: not built: depends on {feature}: {reason}\n"
    );
    assert!(!apply(&mut w, asked));
    assert!(w.creates.sketch.sweeps.active());
    assert_eq!(w.sessions.status, expected);
    assert!(text(&ui.settle(&mut w)).iter().any(|t| t == &expected));
    assert_eq!(version(&w), before);
    assert_eq!(
        std::fs::read(&accepted).expect("fixture operation"),
        accepted_bytes
    );
    ui.press(&mut w, "Cancel", true);
    assert!(!w.creates.sketch.saved_object_open());
    assert_eq!(version(&w), before);
    assert_eq!(std::fs::read(&f.path).expect("fixture operation"), bytes);
}

#[test]
fn native_add_at_rollback_names_and_uses_the_shown_predecessor() {
    if !native() {
        return;
    }
    let mut f = Drawing::new(rectangle(), true);
    let second = ObjectId::new();
    let profile = tool(&mut f, offset_rectangle());
    fixtures::put(
        &mut f.doc,
        second,
        "Later",
        ObjectPayload::Extrude(ferritecad_document::Extrude {
            profile,
            regions: vec![],
            previous: Some(f.feature),
            operation: SolidOperation::Add,
            end_condition: EndCondition::Blind {
                distance: Expression::constant(5.).expect("fixture operation"),
            },
            reversed: false,
        }),
    );
    fixtures::put(
        &mut f.doc,
        f.body,
        "Part",
        ObjectPayload::Body(ferritecad_document::Body {
            tip_feature: Some(second),
            rollback: Some(f.feature),
        }),
    );
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, profile, ModelEdit::Extrude);
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Add", true);
    assert!(
        text(&ui.settle(&mut w))
            .iter()
            .any(|s| s == "Inserted after Extrude 'Extrude'")
    );
    let asked = request(&mut w, &mut ui);
    let inserted = feature_id(&asked);
    assert!(apply(&mut w, asked));
    assert_eq!(saved_extrude(&w, inserted).previous, Some(f.feature));
    assert_eq!(saved_extrude(&w, second).previous, Some(inserted));
    let ObjectPayload::Body(body) = objects(&w)
        .into_iter()
        .find(|o| o.id == f.body)
        .expect("fixture operation")
        .payload
    else {
        panic!("Body")
    };
    assert_eq!(body.rollback, Some(inserted));
    assert_eq!(body.tip_feature, Some(second));
}

#[test]
fn sweep_form_uses_the_existing_command_tab_close_and_quit_lifecycle() {
    let f = Drawing::new(rectangle(), false);
    let other = Drawing::new(vec![], false);
    let mut w = open(&f.path, Drawn::Mock);
    let tab = w.sessions.tab();
    enter(&mut w, f.sketch, ModelEdit::Extrude);
    let mut ui = Ui::new(&mut w);
    ui.type_value(&mut w, "10", "42");
    assert_eq!(
        w.creates
            .sketch
            .sweeps
            .form
            .as_ref()
            .expect("fixture operation")
            .draft
            .distance,
        "42"
    );
    let idle = commands::availability(
        &w.creates,
        &w.edits,
        &w.sessions,
        &w.tabs,
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &w.input,
        false,
    );
    let source = w.scene.edit_source.as_ref().expect("fixture operation");
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|r| r.id.0 == f.sketch)
        .expect("fixture operation")
        .id;
    for edit in [ModelEdit::Extrude, ModelEdit::Revolve] {
        assert_eq!(
            commands::offers(source, row, idle)
                .into_iter()
                .find(|c| c.command == commands::Command::Edit(edit))
                .expect("fixture operation")
                .available,
            Err("Apply or Cancel the open form before opening another editor.".into())
        );
    }
    w.open(&other.path);
    assert!(!w.creates.sketch.sweeps.active());
    assert!(w.tabs.has_draft(tab));
    w.switch(tab);
    assert_eq!(
        w.creates
            .sketch
            .sweeps
            .form
            .as_ref()
            .expect("fixture operation")
            .draft
            .distance,
        "42"
    );
    assert_eq!(
        w.tabs
            .close_step(&w.sessions, tab, w.creates.sketch.active()),
        Some(crate::tabs::CloseStep::Form)
    );
    let id = w
        .tabs
        .ask_form_close(
            &w.sessions,
            &mut crate::tabs::Forms {
                edits: &mut w.edits,
                editor: &mut w.creates.sketch,
            },
        )
        .expect("fixture operation");
    assert!(!w.creates.sketch.active());
    assert!(w.tabs.cancel_form_close(
        &mut w.sessions,
        id,
        &mut crate::tabs::Forms {
            edits: &mut w.edits,
            editor: &mut w.creates.sketch
        }
    ));
    w.tabs.begin_quit();
    let id = w
        .tabs
        .ask_quit_form(
            &w.sessions,
            &mut crate::tabs::Forms {
                edits: &mut w.edits,
                editor: &mut w.creates.sketch,
            },
        )
        .expect("fixture operation");
    assert!(w.tabs.back_quit_form(&w.sessions, id));
    crate::restore_window_quit(&mut w.tabs, &mut w.sessions, &mut w.edits, &mut w.creates);
    assert_eq!(
        w.creates
            .sketch
            .sweeps
            .form
            .as_ref()
            .expect("fixture operation")
            .draft
            .distance,
        "42"
    );
}

#[test]
fn no_body_disables_existing_body_operations_and_invalid_number_keeps_draft() {
    let f = Drawing::new(rectangle(), false);
    let mut w = open(&f.path, Drawn::Mock);
    enter(&mut w, f.sketch, ModelEdit::Extrude);
    let mut ui = Ui::new(&mut w);
    for label in ["Add", "Cut", "Intersect"] {
        ui.press(&mut w, label, true);
        assert_eq!(
            w.creates
                .sketch
                .sweeps
                .form
                .as_ref()
                .expect("form")
                .draft
                .operation,
            SolidOperation::NewBody
        );
    }
    ui.type_value(&mut w, "10", "bad");
    ui.press(&mut w, "Apply", true);
    assert!(w.creates.sketch.sweeps.take_request().is_none());
    assert_eq!(
        w.creates.sketch.sweeps.form.as_ref().expect("form").problem,
        "invalid input: enter a finite distance in mm"
    );
    assert!(
        text(&ui.settle(&mut w))
            .iter()
            .any(|t| t == "invalid input: enter a finite distance in mm")
    );
}

#[test]
fn sweep_entry_requires_the_accepted_drawing_and_an_available_source() {
    let f = Drawing::new(rectangle(), false);
    let w = open(&f.path, Drawn::Mock);
    let mut source = w.scene.edit_source.expect("source");
    let mut editor = Editor::default();
    assert!(!editor.begin_with(
        &f.path,
        &source,
        &Accepted::default(),
        f.sketch,
        Kind::Extrude
    ));
    source.refusal = Some("The source is unavailable.".into());
    assert!(!editor.begin_with(&f.path, &source, &w.scene.sketches, f.sketch, Kind::Extrude));
    source.refusal = None;
    assert!(editor.begin_with(&f.path, &source, &w.scene.sketches, f.sketch, Kind::Extrude));
    assert!(!editor.begin_with(&f.path, &source, &w.scene.sketches, f.sketch, Kind::Revolve));
    assert_eq!(
        editor.form.as_ref().expect("unchanged draft").draft.kind,
        Kind::Extrude
    );
}

#[test]
fn native_line_axis_labels_use_the_solved_drawing_without_rewriting_stored_coordinates() {
    use ferritecad_document::{
        SketchConstraint, SketchConstraintRule, SketchPointRef, SketchPointSelector,
    };
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PlaneGCS"),
    ]) {
        return;
    }
    let mut axis = line([-10., -2.], [10., 2.]);
    axis.construction = true;
    let id = axis.id;
    let mut f = Drawing::new(
        vec![
            curve(SketchGeometry::Circle {
                center: point(20., 20.),
                radius: 2.,
            }),
            axis,
        ],
        false,
    );
    f.doc
        .write_changes(&[EditChange::AddConstraint {
            sketch: f.sketch,
            constraint: SketchConstraint {
                id: StableEntityId::new(),
                rule: SketchConstraintRule::Horizontal {
                    a: SketchPointRef {
                        curve: id,
                        at: SketchPointSelector::Start,
                    },
                    b: SketchPointRef {
                        curve: id,
                        at: SketchPointSelector::End,
                    },
                },
            },
        }])
        .expect("horizontal construction line");
    let saved = f.saved();
    let bytes = std::fs::read(&f.path).expect("original bytes");
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Revolve);
    let source = w.scene.edit_source.as_ref().expect("source");
    let p = w
        .scene
        .sketches
        .presentation(source, f.sketch)
        .expect("accepted drawing");
    assert_ne!(p.curves()[1].geometry(), &saved.curves[1].geometry);
    let SketchGeometry::Line { start, end } = p.curves()[1].geometry() else {
        panic!("line")
    };
    assert_eq!(
        w.creates.sketch.sweeps.form.as_ref().expect("form").axes[2],
        (
            RevolveAxis::Line(id),
            format!(
                "Line 1: ({}, {}) → ({}, {}) mm",
                crate::numeric::mm(start.x),
                crate::numeric::mm(start.y),
                crate::numeric::mm(end.x),
                crate::numeric::mm(end.y)
            )
        )
    );
    assert_eq!(std::fs::read(f.path).expect("unchanged bytes"), bytes);
}

#[test]
fn apply_requires_idle_and_enabled_owner_permission() {
    let f = Drawing::new(rectangle(), false);
    let mut w = open(&f.path, Drawn::Mock);
    enter(&mut w, f.sketch, ModelEdit::Extrude);
    let ctx = egui::Context::default();
    let editor = &mut w.creates.sketch.sweeps;
    for (permission, running) in [(false, false), (true, true), (true, false)] {
        editor.set_apply(permission);
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(988., 768.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| editor.draw(ui, running, ""),
            );
            output.textures_delta.clear();
            output
        };
        frame(vec![]);
        let output = frame(vec![]);
        let at = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Apply" => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("Apply visible");
        frame(vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]);
        assert_eq!(editor.take_request().is_some(), permission && !running);
    }
}

#[test]
fn native_failed_sketch_has_no_drawing_or_sweep_commands() {
    use ferritecad_document::{SketchConstraint, SketchConstraintRule};
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PlaneGCS"),
    ]) {
        return;
    }
    let circle = curve(SketchGeometry::Circle {
        center: point(0., 0.),
        radius: 3.,
    });
    let id = circle.id;
    let mut f = Drawing::new(vec![circle], false);
    f.doc
        .write_changes(
            &[3., 4.]
                .into_iter()
                .map(|r| EditChange::AddConstraint {
                    sketch: f.sketch,
                    constraint: SketchConstraint {
                        id: StableEntityId::new(),
                        rule: SketchConstraintRule::Radius {
                            curve: id,
                            radius: Expression::constant(r).expect("radius"),
                        },
                    },
                })
                .collect::<Vec<_>>(),
        )
        .expect("stored conflicting constraints");
    let mut w = open(&f.path, Drawn::Native);
    let source = w.scene.edit_source.as_ref().expect("source");
    let offered = commands::actions(source, (f.sketch, None, None));
    assert_eq!(
        offered
            .into_iter()
            .filter(|c| matches!(
                c,
                ModelEdit::Sketch | ModelEdit::Extrude | ModelEdit::Revolve
            ))
            .collect::<Vec<_>>(),
        vec![]
    );
    for kind in [Kind::Extrude, Kind::Revolve] {
        assert!(!w.creates.sketch.sweeps.begin_with(
            &f.path,
            source,
            &w.scene.sketches,
            f.sketch,
            kind
        ));
    }
}
