// SPDX-License-Identifier: MIT
use super::widgets::{Ui, text};
use super::window::{apply, click, enter, finish, open};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_ui::ModelEdit;

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[
        (ferritecad_eval::solver_available(), "PLANEGCS"),
        (ferritecad_occt::is_available(), "OCCT"),
    ])
}

#[test]
#[cfg(not(feature = "planegcs"))]
fn native_without_solver_saved_circle_has_no_radius_edit() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    assert!(!ferritecad_eval::solver_available());
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("circle.fcad");
    crate::feature_tests::write_round(&path, [12.5, -7.25], &[10.5], 15.25, false);
    let before = std::fs::read(&path).expect("source bytes");
    let mut w = open(&path, Drawn::Native);
    let source = w.scene.edit_source.as_ref().expect("source");
    let sketch = source.built_sketches[0];
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click(&mut w, point(23., -7.25), false);
    let editor = &w.creates.sketch.viewport;
    let draft = editor.draft.as_ref().expect("open draft");
    let saved = draft.state.curves.clone();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        saved[0].geometry,
        SketchGeometry::Circle {
            center: point(12.5, -7.25),
            radius: 10.5,
        }
    );
    assert_eq!(draft.state.selected, vec![Selection::Curve(saved[0].id)]);
    assert!(draft.state.constraints.is_empty());
    for button in [rules::Button::Radius, rules::Button::Diameter] {
        assert_eq!(
            button.rule(
                &saved,
                &draft.state.selected,
                ferritecad_eval::solver_available()
            ),
            Err("This build has no sketch solver, so constraints cannot be added.".into())
        );
    }
    for label in ["Radius", "Diameter"] {
        ui.press(&mut w, label, false);
        let draft = w
            .creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("open draft");
        assert_eq!(draft.state.curves, saved);
        assert!(draft.state.constraints.is_empty());
        assert!(draft.undo.is_empty());
    }
    ui.press(&mut w, "Finish", false);
    assert!(w.creates.sketch.viewport.take_request().is_none());
    assert_eq!(std::fs::read(&path).expect("source bytes"), before);
}

#[test]
fn native_temporary_radius_matches_the_exact_cli_geometry_without_saved_constraints() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("circle.fcad");
    crate::feature_tests::write_round(&path, [12.5, -7.25], &[10.5], 15.25, false);
    let mut w = open(&path, Drawn::Native);
    let source = w.scene.edit_source.as_ref().expect("source");
    let sketch = source.built_sketches[0];
    let version = source.version;
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click(&mut w, point(23., -7.25), false);
    let saved = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("draft")
        .state
        .curves[0]
        .clone();

    // The peer writes its own literal, not a number copied from the solved draft.
    let changes = vec![EditChange::SetCurveGeometry {
        sketch,
        curve: saved.id,
        geometry: SketchGeometry::Circle {
            center: point(12.5, -7.25),
            radius: 6.75,
        },
    }];
    let input = root.path().join("radius.json");
    std::fs::write(
        &input,
        ferritecad_fixtures::edit_request_json(&changes).to_string(),
    )
    .expect("CLI request");
    let peer = root.path().join("cli.fcad");
    let output = std::process::Command::new(crate::creates::tests::ferritecad())
        .arg("edit")
        .arg(&path)
        .args(["--expect-version", &version.content.to_string()])
        .arg("--request")
        .arg(&input)
        .arg("-o")
        .arg(&peer)
        .output()
        .expect("CLI edit");
    assert!(output.status.success(), "{output:?}");
    let peer_document = ferritecad_document::Document::open_read_only(&peer).expect("CLI document");
    let ferritecad_document::ObjectPayload::Sketch(peer_sketch) = peer_document
        .object(sketch)
        .expect("object")
        .expect("sketch")
        .payload
    else {
        panic!("the CLI keeps the sketch");
    };
    assert!(peer_sketch.constraints.is_empty());
    assert_eq!(peer_sketch.curves[0].id, saved.id);

    press_rule(&mut ui, &mut w, "Radius");
    ui.type_value(&mut w, "10.5", "6.75");
    ui.press(&mut w, "Set", false);
    let dimension = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("draft")
        .state
        .constraints[0]
        .id;
    click_label(&mut w, &mut ui, dimension);
    ui.key(&mut w, egui::Key::Delete);
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    assert!(draft.state.constraints.is_empty());
    assert_eq!(draft.state.curves, peer_sketch.curves);
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, changes);
    assert!(apply(&mut w, request));
    let accepted = w.sessions.export_path().expect("accepted document");
    let document = ferritecad_document::Document::open_read_only(&accepted).expect("document");
    assert_eq!(
        document
            .object(sketch)
            .expect("object")
            .expect("sketch")
            .payload,
        ferritecad_document::ObjectPayload::Sketch(peer_sketch)
    );
}

const CORNERS: [[f64; 2]; 4] = [[-30., -20.], [30., -20.], [30., 20.], [-30., 20.]];
const MIDS: [[f64; 2]; 4] = [[0., -20.], [30., 0.], [0., 20.], [-30., 0.]];
fn constraint_count(w: &Window) -> usize {
    w.creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft")
        .state
        .constraints
        .len()
}

pub(super) fn press_rule(ui: &mut Ui, w: &mut Window, label: &str) {
    let n = constraint_count(w);
    ui.press(w, label, false);
    assert_eq!(
        constraint_count(w),
        n + 1,
        "{label}: {}",
        w.creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("open draft")
            .message
    );
}

fn rectangle() -> (fixtures::Drawing, Window, Ui) {
    let sides = (0..4)
        .map(|i| line(CORNERS[i], CORNERS[(i + 1) % 4]))
        .collect();
    let f = fixtures::Drawing::new(sides, false);
    let mut w = open(&f.path, Drawn::Native);
    w.input
        .frame(([-40., -30., -20.], [40., 30., 20.]))
        .expect("fixture operation");
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    for (i, [x, y]) in MIDS.into_iter().enumerate() {
        click(&mut w, point(x, y), false);
        press_rule(
            &mut ui,
            &mut w,
            if i % 2 == 0 { "Horizontal" } else { "Vertical" },
        );
    }
    for [x, y] in CORNERS {
        click(&mut w, point(x, y), false);
        click(&mut w, point(x, y), true);
        press_rule(&mut ui, &mut w, "Coincident");
    }
    for [x, y] in &MIDS[..2] {
        click(&mut w, point(*x, *y), false);
        press_rule(&mut ui, &mut w, "Distance");
    }
    click(&mut w, point(-30., -20.), false);
    press_rule(&mut ui, &mut w, "Fix");
    (f, w, ui)
}

#[test]
fn rectangle_widgets_constrain_the_drawing_in_one_transaction_and_extrude_it() {
    if !native() {
        return;
    }
    let (f, mut w, mut ui) = rectangle();
    assert!(
        text(&ui.settle(&mut w))
            .iter()
            .any(|s| s == "Fully constrained")
    );
    let shown = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft")
        .state
        .clone();
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes.len(), 11);
    for (change, k) in request.changes.iter().zip(&shown.constraints) {
        assert_eq!(
            *change,
            EditChange::AddConstraint {
                sketch: f.sketch,
                constraint: k.clone()
            }
        );
    }
    assert!(apply(&mut w, request));
    let source = w.scene.edit_source.as_ref().expect("open draft");
    let picture = w
        .scene
        .sketches
        .presentation(source, f.sketch)
        .expect("fixture operation");
    for (seen, got) in shown.curves.iter().zip(picture.curves()) {
        assert_eq!(seen.id, got.id());
        assert_eq!(seen.geometry, *got.geometry());
    }
    enter(&mut w, f.sketch, ModelEdit::Extrude);
    ui.press(&mut w, "Apply", true);
    let sweep = w
        .creates
        .sketch
        .sweeps
        .take_request()
        .expect("fixture operation");
    assert!(apply(&mut w, sweep));
}

/// The focused Value field and Set share the panel's job and handle-capture guard.
#[test]
fn focused_dimensions_freeze_during_a_job_or_handle_capture_and_resume_on_click() {
    if !native() {
        return;
    }
    let circle = curve(SketchGeometry::Circle {
        center: point(0., 0.),
        radius: 10.,
    });
    for (label, curves, first_pick, initial, handle) in [
        (
            "Radius",
            vec![circle.clone()],
            point(10., 0.),
            "10",
            point(0., 0.),
        ),
        (
            "Diameter",
            vec![circle],
            point(10., 0.),
            "20",
            point(0., 0.),
        ),
        (
            "Distance",
            vec![line([-10., 0.], [10., 0.])],
            point(0., 0.),
            "20",
            point(-10., 0.),
        ),
        (
            "Angle",
            vec![line([-10., 0.], [0., 0.]), line([5., 0.], [5., 10.])],
            point(5., 5.),
            "-90",
            point(5., 10.),
        ),
    ] {
        let f = fixtures::Drawing::new(curves, false);
        let mut w = open(&f.path, Drawn::Native);
        enter(&mut w, f.sketch, ModelEdit::Sketch);
        let mut ui = Ui::new(&mut w);
        click(&mut w, first_pick, false);
        if label == "Angle" {
            click(&mut w, point(-5., 0.), true);
        }
        press_rule(&mut ui, &mut w, label);
        ui.type_value(&mut w, initial, "20");
        let field = |w: &Window| {
            w.creates
                .sketch
                .viewport
                .value_field
                .as_ref()
                .expect("selected dimension")
                .2
                .clone()
        };
        let state = |w: &Window| {
            let draft = w
                .creates
                .sketch
                .viewport
                .draft
                .as_ref()
                .expect("open sketch");
            (draft.state.clone(), draft.undo.clone(), draft.redo.clone())
        };
        assert_eq!(field(&w), "20");
        let before = state(&w);
        let request = EditRequest {
            source: f.path.clone(),
            expected: super::window::version(&w),
            changes: vec![],
            destination: f.path.with_extension("copy"),
        };
        let (finish, wait) = std::sync::mpsc::channel();
        w.edits
            .start(request, |_, _, _| {
                std::thread::spawn(move || {
                    let _ = wait.recv();
                })
            })
            .expect("start edit worker");
        assert!(w.edits.running());
        ui.frame(&mut w, vec![egui::Event::Text("7".into())]);
        assert_eq!(field(&w), "20", "{label} during a job");
        assert_eq!(state(&w), before);
        finish.send(()).expect("release worker");
        w.edits.stop_all();
        ui.type_value(&mut w, "20", "13.5");
        assert_eq!(field(&w), "13.5");
        assert_eq!(state(&w), before);

        super::dragging::press(&mut w, handle);
        assert!(w.creates.sketch.viewport.gesturing(), "{label} handle");
        ui.frame(&mut w, vec![egui::Event::Text("7".into())]);
        assert_eq!(field(&w), "13.5", "{label} during capture");
        assert_eq!(state(&w), before);
        super::dragging::release(&mut w);
    }
}

#[test]
fn pending_finish_blocks_shortcuts_and_viewport_edits_until_publication() {
    use super::window::version;
    use winit::keyboard::{Key, ModifiersState};

    if !native() {
        return;
    }
    let circle = curve(SketchGeometry::Circle {
        center: point(0., 0.),
        radius: 10.,
    });
    let f = fixtures::Drawing::new(vec![circle], false);
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click(&mut w, point(10., 0.), false);
    press_rule(&mut ui, &mut w, "Radius");
    let snapshot = |w: &Window| {
        let d = w
            .creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("open sketch");
        (
            d.state.clone(),
            d.undo.clone(),
            d.redo.clone(),
            d.tool,
            d.pending,
        )
    };
    let radius = snapshot(&w).0.constraints[0].clone();
    assert_eq!(snapshot(&w).0.selected, [Selection::Constraint(radius.id)]);

    // An edit worker can finish while this same draft is still on screen.
    let original = version(&w);
    let request = EditRequest {
        source: f.path.clone(),
        expected: original,
        changes: vec![],
        destination: f.path.with_extension("copy"),
    };
    w.edits
        .start(request, |_, _, _| std::thread::spawn(|| {}))
        .expect("start worker");
    let before = snapshot(&w);
    assert!(!input_editable(&w.creates, &w.edits, &w.sessions));
    super::dragging::press(&mut w, point(0., 0.));
    assert!(!w.creates.sketch.viewport.gesturing());
    super::dragging::release(&mut w);
    click(&mut w, point(-4., 3.), false);
    let editable = input_editable(&w.creates, &w.edits, &w.sessions);
    assert!(
        w.creates
            .sketch
            .viewport
            .take_history(&mut w.input, true, editable)
    );
    assert_eq!(snapshot(&w), before);
    w.edits.stop_all();
    assert!(input_editable(&w.creates, &w.edits, &w.sessions));
    ui.key(&mut w, egui::Key::Delete);
    assert_eq!(constraint_count(&w), 0);
    ui.press(&mut w, "Undo", true);
    assert_eq!(snapshot(&w).0, before.0);

    // Leave a first point pending so an unguarded Escape or click is observable.
    ui.press(&mut w, "Line", false);
    click(&mut w, point(-8., 6.), false);
    let before = snapshot(&w);
    assert!(matches!(before.4, Some(Pending::Line { .. })));
    assert!(!before.1.is_empty() && !before.2.is_empty());
    let request = finish(&mut w, &mut ui);
    assert_eq!(
        request.changes,
        [EditChange::AddConstraint {
            sketch: f.sketch,
            constraint: radius,
        }]
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let address = w
        .sessions
        .apply(request.expected, request.changes, move |_, result| {
            let _ = tx.send(result);
        })
        .expect("owner accepts Finish");
    assert!(w.sessions.busy());
    ui.press(&mut w, "Delete", true);
    for key in [egui::Key::Delete, egui::Key::Backspace, egui::Key::Escape] {
        ui.key(&mut w, key);
        assert_eq!(snapshot(&w), before, "{key:?} during Finish");
    }
    let primary = if cfg!(target_os = "macos") {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    };
    let editable = input_editable(&w.creates, &w.edits, &w.sessions);
    for (modifiers, expected) in [
        (primary, crate::DocumentCommand::Undo),
        (
            primary | ModifiersState::SHIFT,
            crate::DocumentCommand::Redo,
        ),
    ] {
        let command =
            crate::document_command(&Key::Character("z".into()), Some("z"), modifiers, false);
        assert_eq!(command, Some(expected));
        assert!(w.creates.sketch.viewport.take_history(
            &mut w.input,
            command == Some(crate::DocumentCommand::Undo),
            editable,
        ));
        assert_eq!(snapshot(&w), before, "{expected:?} during Finish");
    }
    click(&mut w, point(7., 9.), false);
    assert_eq!(snapshot(&w), before);
    assert_eq!(version(&w), original);
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("Finish reply");
    let crate::sessions::Edited::Show(path) = w.sessions.finish_apply(address, result) else {
        panic!("accepted step");
    };
    w.show_staged(address, path.clone());
    let accepted = super::curve_edits::stored(&path).1;
    assert_eq!(accepted.constraints, before.0.constraints);
    assert_eq!(accepted.curves, before.0.curves);
    assert_ne!(version(&w), original);
}

#[test]
fn saved_distance_changes_the_width_to_eighty_through_the_value_field() {
    if !native() {
        return;
    }
    let (f, mut w, mut ui) = rectangle();
    let ask = finish(&mut w, &mut ui);
    assert!(apply(&mut w, ask));
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let d = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft");
    let width = |c: &&SketchConstraint| match &c.rule {
        SketchConstraintRule::Distance { distance, .. } => distance.value() == 60.,
        _ => false,
    };
    let id = d
        .state
        .constraints
        .iter()
        .find(width)
        .expect("width dimension")
        .id;
    // Paint and click the hit rectangle used by the actual viewport overlay.
    click_label(&mut w, &mut ui, id);
    ui.type_value(&mut w, "60", "80");
    ui.press(&mut w, "Set", false);
    let d = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft");
    assert_eq!(
        draft::dimension(
            d.state
                .constraints
                .iter()
                .find(|k| k.id == id)
                .expect("fixture operation")
        )
        .expect("fixture operation")
        .1
        .value(),
        80.
    );
    let SketchGeometry::Line { start, end } = d.state.curves[0].geometry else {
        unreachable!()
    };
    assert!(((end.x - start.x) - 80.).abs() < 1e-6);
    let request = finish(&mut w, &mut ui);
    assert_eq!(
        request.changes.last(),
        Some(
            &EditChange::constant(
                f.sketch,
                ferritecad_document::ExpressionSlot::Distance { constraint: id },
                80.
            )
            .expect("fixture operation")
        )
    );
    assert!(apply(&mut w, request));
}

pub(super) fn click_label(w: &mut Window, ui: &mut Ui, id: StableEntityId) {
    let editor = &mut w.creates.sketch.viewport;
    let camera = w.input.camera();
    let region = w.input.region();
    ui.paint(editor, camera, region);
    let [min, max] = editor
        .labels
        .iter()
        .find(|(key, _)| *key == id)
        .expect("visible marker")
        .1;
    editor.pointer(
        camera,
        [(min[0] + max[0]) / 2., (min[1] + max[1]) / 2.],
        true,
        false,
    );
    assert_eq!(
        editor.draft.as_ref().expect("open draft").state.selected,
        vec![Selection::Constraint(id)]
    );
}

#[test]
fn radius_and_diameter_start_at_the_measured_size_and_history_never_solves() {
    if !native() {
        return;
    }
    let circle = curve(SketchGeometry::Circle {
        center: point(0., 0.),
        radius: 5.,
    });
    let f = fixtures::Drawing::new(vec![circle.clone()], false);
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    let calls = || draft::SOLVES.with(|n| n.get());
    let start = calls();
    click(&mut w, point(5., 0.), false);
    press_rule(&mut ui, &mut w, "Radius");
    let radius = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft")
        .state
        .clone();
    assert_eq!(radius.curves[0].geometry, circle.geometry);
    assert_eq!(
        draft::dimension(&radius.constraints[0])
            .expect("fixture operation")
            .1
            .value(),
        5.
    );
    ui.press(&mut w, "Undo", true);
    click(&mut w, point(5., 0.), false);
    press_rule(&mut ui, &mut w, "Diameter");
    assert_eq!(calls() - start, 2);
    let id = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft")
        .state
        .constraints[0]
        .id;
    assert!(text(&ui.settle(&mut w)).iter().any(|s| s == "10"));
    ui.type_value(&mut w, "10", "14");
    ui.press(&mut w, "Set", false);
    let edited = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft")
        .state
        .clone();
    assert_eq!(
        draft::dimension(&edited.constraints[0])
            .expect("fixture operation")
            .1
            .value(),
        7.
    );
    click_label(&mut w, &mut ui, id);
    ui.key(&mut w, egui::Key::Delete);
    assert_eq!(constraint_count(&w), 0);
    let after_delete = calls();
    assert_eq!(
        after_delete - start,
        3,
        "an empty constraint list does not call PlaneGCS"
    );
    ui.press(&mut w, "Undo", true);
    assert_eq!(
        w.creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("open draft")
            .state,
        edited
    );
    ui.press(&mut w, "Undo", true);
    assert_eq!(
        draft::dimension(
            &w.creates
                .sketch
                .viewport
                .draft
                .as_ref()
                .expect("open draft")
                .state
                .constraints[0]
        )
        .expect("fixture operation")
        .1
        .value(),
        5.
    );
    ui.press(&mut w, "Redo", true);
    ui.settle(&mut w);
    assert_eq!(calls(), after_delete, "neither history nor a frame solves");
    let request = finish(&mut w, &mut ui);
    assert_eq!(
        calls() - after_delete,
        1,
        "the measured radius already predicts the shown circle"
    );
    assert_eq!(request.changes.len(), 1);
    assert!(
        matches!(&request.changes[0],EditChange::AddConstraint{constraint,..} if constraint.id==id)
    );
    assert!(apply(&mut w, request));
}

#[test]
fn angle_is_measured_in_pick_order_and_set_accepts_degrees() {
    if !native() {
        return;
    }
    let f = fixtures::Drawing::new(
        vec![line([-10., 0.], [0., 0.]), line([5., 0.], [5., 10.])],
        false,
    );
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click(&mut w, point(5., 5.), false);
    click(&mut w, point(-5., 0.), true);
    press_rule(&mut ui, &mut w, "Angle");
    ui.type_value(&mut w, "-90", "30");
    ui.key(&mut w, egui::Key::Enter);
    let state = &w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft")
        .state;
    let SketchConstraintRule::DirectedAngle { angle, .. } = &state.constraints[0].rule else {
        unreachable!()
    };
    assert_eq!(angle.value(), 30.0_f64.to_radians());
    // The field reads the stored radians back as the degrees typed, not 29.999999999999996.
    assert!(text(&ui.settle(&mut w)).iter().any(|s| s == "30"));
    let ask = finish(&mut w, &mut ui);
    assert!(apply(&mut w, ask));
}

fn saved_horizontal_lines() -> (fixtures::Drawing, [StableEntityId; 2], [StableEntityId; 2]) {
    let lines = [line([-20., 0.], [20., 0.]), line([-20., 12.], [20., 12.])];
    let curves = [lines[0].id, lines[1].id];
    let rules = [StableEntityId::new(), StableEntityId::new()];
    let mut f = fixtures::Drawing::new(Vec::from(lines), false);
    let changes = [0, 1].map(|i| EditChange::AddConstraint {
        sketch: f.sketch,
        constraint: SketchConstraint {
            id: rules[i],
            rule: SketchConstraintRule::Horizontal {
                a: SketchPointRef::new(curves[i], SketchPointSelector::Start),
                b: SketchPointRef::new(curves[i], SketchPointSelector::End),
            },
        },
    });
    f.doc
        .write_changes(&changes)
        .expect("save both horizontal lines");
    (f, curves, rules)
}

#[test]
fn deleting_a_saved_viewport_marker_removes_only_its_rule() {
    if !native() {
        return;
    }
    let (f, _, rules) = saved_horizontal_lines();
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, rules[0]);
    ui.press(&mut w, "Delete", true);
    assert_eq!(constraint_count(&w), 1);
    let request = finish(&mut w, &mut ui);
    assert_eq!(
        request.changes,
        vec![EditChange::RemoveConstraint {
            sketch: f.sketch,
            constraint: rules[0],
        }]
    );
    assert!(apply(&mut w, request));
}

#[test]
fn draft_history_restores_each_saved_marker_removal_and_cancel_discards_both() {
    if !native() {
        return;
    }
    let (f, _, ids) = saved_horizontal_lines();
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let (_, saved) = super::curve_edits::stored(&f.path);
    let accepted = super::window::version(&w);
    let original = std::fs::read(&f.path).expect("source bytes");
    let mut ui = Ui::new(&mut w);
    let changes = |w: &Window| {
        w.creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("open draft")
            .changes()
            .expect("request")
    };
    let removed = ids.map(|constraint| EditChange::RemoveConstraint {
        sketch: f.sketch,
        constraint,
    });
    for (i, id) in ids.into_iter().enumerate() {
        click_label(&mut w, &mut ui, id);
        ui.press(&mut w, "Delete", true);
        assert_eq!(changes(&w), removed[..=i]);
    }
    for n in [1, 0] {
        ui.press(&mut w, "Undo", true);
        assert_eq!(changes(&w), removed[..n]);
    }
    for n in [1, 2] {
        ui.press(&mut w, "Redo", true);
        assert_eq!(changes(&w), removed[..n]);
    }
    ui.press(&mut w, "Cancel", true);
    assert!(!w.creates.sketch.viewport.active());
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let d = w.creates.sketch.viewport.draft.as_ref().expect("reopened");
    assert_eq!(d.state.constraints, saved.constraints);
    assert!(d.undo.is_empty() && d.redo.is_empty());
    assert!(changes(&w).is_empty());
    assert_eq!(super::window::version(&w), accepted);
    assert_eq!(std::fs::read(&f.path).expect("source bytes"), original);
    assert!(!w.sessions.can_undo());
}

#[test]
fn deleting_a_constrained_line_solves_the_remaining_drawing_once() {
    if !native() {
        return;
    }
    let (f, curves, rules) = saved_horizontal_lines();
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click(&mut w, point(-10., 0.), false);
    let calls = draft::SOLVES.with(|n| n.get());
    ui.press(&mut w, "Delete", true);
    assert_eq!(draft::SOLVES.with(|n| n.get()) - calls, 1);
    assert_eq!(constraint_count(&w), 1);
    let request = finish(&mut w, &mut ui);
    assert_eq!(
        request.changes,
        vec![
            EditChange::RemoveConstraint {
                sketch: f.sketch,
                constraint: rules[0]
            },
            EditChange::RemoveCurve {
                sketch: f.sketch,
                curve: curves[0]
            },
        ]
    );
    assert!(apply(&mut w, request));
}

#[test]
fn a_saved_parameter_dimension_is_read_only_and_keeps_its_source() {
    if !native() {
        return;
    }
    let side = line([-5., 0.], [5., 0.]);
    let id = side.id;
    let mut f = fixtures::Drawing::new(vec![side], false);
    let constraint = StableEntityId::new();
    f.doc
        .write_changes(&[
            EditChange::AddParameter {
                object: ObjectId::new(),
                name: "width".into(),
                dimension: ferritecad_types::Dimension::Length,
                expression: "10 mm".into(),
            },
            EditChange::AddConstraint {
                sketch: f.sketch,
                constraint: SketchConstraint {
                    id: constraint,
                    rule: SketchConstraintRule::Distance {
                        a: SketchPointRef::new(id, SketchPointSelector::Start),
                        b: SketchPointRef::new(id, SketchPointSelector::End),
                        distance: ferritecad_document::Expression::new("width", 10.)
                            .expect("fixture operation"),
                    },
                },
            },
        ])
        .expect("fixture operation");
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, constraint);
    let labels = text(&ui.settle(&mut w));
    assert!(labels.iter().any(|s| s == "width"));
    assert!(
        labels
            .iter()
            .any(|s| s == "This value is an expression; edit it in Parameters.")
    );
    assert!(!labels.iter().any(|s| s == "Set"));
    let d = w
        .creates
        .sketch
        .viewport
        .draft
        .as_mut()
        .expect("open draft");
    d.set_value(constraint, "12");
    assert_eq!(
        d.message,
        "This value is an expression; edit it in Parameters."
    );
    assert_eq!(
        draft::dimension(&d.state.constraints[0])
            .expect("fixture operation")
            .1
            .source,
        "width"
    );
    assert!(d.changes().expect("Finish prediction").is_empty());
}

#[test]
fn contradictory_distance_keeps_every_curve_rule_and_history_step() {
    if !native() {
        return;
    }
    let side = line([-5., 0.], [5., 0.]);
    let (a, b) = (
        SketchPointRef::new(side.id, SketchPointSelector::Start),
        SketchPointRef::new(side.id, SketchPointSelector::End),
    );
    let mut f = fixtures::Drawing::new(vec![side], false);
    let dist = StableEntityId::new();
    let distance = |v| SketchConstraintRule::Distance {
        a,
        b,
        distance: ferritecad_document::Expression::constant(v).expect("fixture operation"),
    };
    f.doc
        .write_changes(&[EditChange::AddConstraint {
            sketch: f.sketch,
            constraint: SketchConstraint {
                id: dist,
                rule: distance(10.),
            },
        }])
        .expect("fixture operation");
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click(&mut w, point(0., 0.), false);
    press_rule(&mut ui, &mut w, "Distance");
    let before = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft")
        .state
        .clone();
    let history = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft")
        .undo
        .clone();
    // Read the exact diagnostic and typed blame from the evaluator for these rule UUIDs.
    let mut candidate = f.saved();
    candidate.constraints = before.constraints.clone();
    candidate.constraints[1].rule = distance(12.);
    let refusal =
        ferritecad_eval::solve_sketch(&candidate, f.sketch).expect_err("inconsistent lengths");
    let blame = ferritecad_eval::SketchConflict::of(&refusal).expect("typed conflict");
    let mut expected = refusal.to_string();
    for c in blame.constraints() {
        expected.push_str(&format!(
            "\n{}: {}",
            c.id(),
            crate::describe_constraint(c.rule())
        ));
    }
    ui.type_value(&mut w, "10", "12");
    ui.press(&mut w, "Set", false);
    let d = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft");
    assert_eq!(d.message, expected);
    assert_eq!(d.state, before);
    assert_eq!(d.undo, history);
    ui.press(&mut w, "Undo", true);
    assert_eq!(
        w.creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("open draft")
            .state
            .constraints
            .len(),
        1
    );
}

#[test]
fn unavailable_solver_leaves_constraint_controls_disabled_and_drawing_usable() {
    if ferritecad_eval::solver_available() {
        return;
    }
    let f = fixtures::Drawing::new(vec![line([-5., 0.], [5., 0.])], false);
    let mut w = open(&f.path, Drawn::Mock);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click(&mut w, point(0., 0.), false);
    for button in rules::Button::ALL {
        ui.press(&mut w, button.label(), false);
        assert_eq!(constraint_count(&w), 0);
    }
    assert_eq!(
        rules::Button::Distance.rule(&[], &[], false),
        Err("This build has no sketch solver, so constraints cannot be added.".into())
    );
    ui.press(&mut w, "Point", false);
    click(&mut w, point(2., 3.), false);
    assert_eq!(
        w.creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("open draft")
            .state
            .curves
            .len(),
        2
    );
}
