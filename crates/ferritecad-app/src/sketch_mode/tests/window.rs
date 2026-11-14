// SPDX-License-Identifier: MIT
use super::*;
use crate::{
    commands,
    sessions::tests::tabs::{Drawn, Window},
};
use ferritecad_document::{Document, ObjectPayload};
use ferritecad_ui::{PointerButton, ViewportEvent};
use std::{path::Path, sync::mpsc, time::Duration};
use widgets::{Ui, text};

fn open(path: &Path, drawn: Drawn) -> Window {
    let mut w = Window::new(drawn, None);
    w.open(path);
    w.input
        .frame(([-20., -20., -20.], [20., 20., 20.]))
        .expect("frame");
    w
}

fn enter(w: &mut Window, id: ObjectId, edit: ferritecad_ui::ModelEdit) {
    let source = w.scene.edit_source.as_ref().expect("source");
    let navigation = w.tabs.model_navigation(w.sessions.tab());
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|r| r.id.0 == id)
        .expect("row")
        .id;
    commands::select_row(source, navigation, row);
    let action = ferritecad_ui::ModelAction {
        tab: w.sessions.tab().key(),
        epoch: navigation.epoch,
        row,
        edit: commands::Command::Edit(edit),
    };
    let answer = commands::launch(
        action,
        source,
        &w.scene.sketches,
        &mut w.creates,
        &mut w.edits,
        &w.sessions,
        &w.tabs,
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &w.input,
        false,
    );
    assert!(answer.is_ok(), "{}", answer.err().unwrap_or_default());
    assert!(w.creates.sketch.saved_object_open());
    w.creates.sketch.viewport.look(&mut w.input);
}

fn start_new(w: &mut Window, ui: &mut Ui) {
    assert!(ui.press(w, "New sketch…", false).new_sketch);
    let source = w.scene.edit_source.as_ref().expect("source");
    assert_eq!(
        commands::new_sketch_available(source, Ok(commands::Access::Idle)),
        Ok(())
    );
    assert!(w.creates.sketch.viewport.begin_new(
        w.sessions.export_source().expect("shown").path(),
        source,
        None,
    ));
    w.creates.sketch.viewport.look(&mut w.input);
}

fn click(w: &mut Window, at: Point2, shift: bool) {
    let d = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    let pixel = geometry::screen(
        w.input.camera(),
        d.planes[d.plane].frame.expect("plane"),
        at,
    )
    .expect("screen");
    for event in [
        ViewportEvent::PointerMoved {
            x: pixel[0] as f32,
            y: pixel[1] as f32,
        },
        ViewportEvent::PointerPressed(PointerButton::Primary),
        ViewportEvent::PointerReleased(PointerButton::Primary),
    ] {
        w.input.handle(event, false);
    }
    let (x, y) = w
        .input
        .take_pick()
        .expect("existing reducer emitted a click");
    assert!(
        w.creates
            .sketch
            .viewport
            .take_click(&mut w.input, [x, y], shift, true),
        "the mode consumes the click before entity picking"
    );
}

fn version(w: &Window) -> ferritecad_document::DocumentVersion {
    w.sessions.export_source().expect("version").version()
}
fn draft(w: &Window) -> &Draft {
    w.creates.sketch.viewport.draft.as_ref().expect("draft")
}
fn finish(w: &mut Window, ui: &mut Ui) -> EditRequest {
    ui.press(w, "Finish", false);
    w.creates
        .sketch
        .viewport
        .take_request()
        .expect("one request")
}
fn apply(w: &mut Window, request: EditRequest) -> bool {
    assert_eq!(request.expected, version(w));
    let (tx, rx) = mpsc::channel();
    let address = w
        .sessions
        .apply(request.expected, request.changes, move |_, answer| {
            tx.send(answer).expect("answer");
        })
        .expect("owner accepts request");
    let answer = rx.recv_timeout(Duration::from_secs(60)).expect("answer");
    match w.sessions.finish_apply(address, answer) {
        crate::sessions::Edited::Show(path) => {
            let (tx, rx) = mpsc::channel();
            w.sessions
                .draw_staged(address, move |s| tx.send(s).expect("scene"));
            let scene = rx
                .recv_timeout(Duration::from_secs(60))
                .expect("scene")
                .expect("built");
            let cold = ferritecad_scene::snapshot_of(
                &path,
                &mut ferritecad_occt::OcctKernel::new().expect("OCCT"),
                ferritecad_occt::OcctKernel::import_step,
                &Default::default(),
                &Default::default(),
            )
            .expect("cold scene");
            assert_eq!(
                scene, cold,
                "candidate must match a cold load before publication"
            );
            let result = w.present(&path, Ok(scene), crate::sessions::Bind::Staged, Ok(()));
            assert!(result.is_ok());
            assert!(w.sessions.finish_scene(address, result));
            true
        }
        crate::sessions::Edited::Failed => false,
        other => panic!("unexpected completion: {other:?}"),
    }
}

#[test]
fn widgets_finish_empty_difference_and_cancel_leave_version_and_file_unchanged() {
    let f = fixtures::Drawing::new(vec![], false);
    let bytes = std::fs::read(&f.path).expect("bytes");
    let mut w = open(&f.path, Drawn::Mock);
    let before = version(&w);
    enter(&mut w, f.sketch, ferritecad_ui::ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    let output = ui.settle(&mut w);
    for label in [
        "Select",
        "Line",
        "Circle",
        "Point",
        "Delete",
        "Construction",
        "Undo",
        "Redo",
        "Look at sketch",
        "Finish",
        "Cancel",
        "Plane: XY",
        "0 curves: 0 added, 0 removed, 0 changed",
    ] {
        assert!(text(&output).iter().any(|t| t == label), "visible {label}");
    }
    ui.press(&mut w, "Finish", false);
    assert!(!w.creates.sketch.viewport.active());
    assert!(
        w.creates.sketch.viewport.take_request().is_none(),
        "empty difference has no request"
    );
    assert_eq!(version(&w), before);
    enter(&mut w, f.sketch, ferritecad_ui::ModelEdit::Sketch);
    ui.press(&mut w, "Point", false);
    click(&mut w, point(2., 3.), false);
    ui.press(&mut w, "Cancel", false);
    assert!(!w.creates.sketch.active());
    assert_eq!(version(&w), before);
    assert_eq!(std::fs::read(&f.path).expect("bytes"), bytes);
}

#[test]
fn widgets_circle_point_construction_selection_delete_and_draft_history_own_keys() {
    let f = fixtures::Drawing::new(vec![], false);
    let mut w = open(&f.path, Drawn::Mock);
    enter(&mut w, f.sketch, ferritecad_ui::ModelEdit::Sketch);
    let before = version(&w);
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Construction", false);
    ui.press(&mut w, "Circle", false);
    click(&mut w, point(-10., 0.), false);
    click(&mut w, point(-5., 0.), false);
    ui.press(&mut w, "Point", false);
    click(&mut w, point(10., 5.), false);
    assert_eq!(draft(&w).state.curves.len(), 2);
    assert!(draft(&w).state.curves.iter().all(|c| c.construction));
    ui.press(&mut w, "Select", false);
    click(&mut w, point(-5., 0.), false);
    click(&mut w, point(10., 5.), true);
    assert_eq!(draft(&w).state.selected.len(), 2);
    ui.press(&mut w, "Construction", true);
    assert!(draft(&w).state.curves.iter().all(|c| !c.construction));
    for key in [egui::Key::Delete, egui::Key::Backspace] {
        ui.key(&mut w, key);
        assert!(draft(&w).state.curves.is_empty());
        assert_eq!(version(&w), before);
        ui.press(&mut w, "Undo", true);
        assert_eq!(draft(&w).state.curves.len(), 2);
        ui.press(&mut w, "Redo", true);
        assert!(draft(&w).state.curves.is_empty());
        let primary = if cfg!(target_os = "macos") {
            winit::keyboard::ModifiersState::SUPER
        } else {
            winit::keyboard::ModifiersState::CONTROL
        };
        assert_eq!(
            crate::document_command(
                &winit::keyboard::Key::Character("z".into()),
                Some("z"),
                primary,
                false,
            ),
            Some(crate::DocumentCommand::Undo)
        );
        assert!(
            w.creates
                .sketch
                .viewport
                .take_history(&mut w.input, true, true)
        );
        assert_eq!(draft(&w).state.curves.len(), 2);
        assert_eq!(version(&w), before);
    }
    ui.press(&mut w, "Cancel", false);
    assert!(
        !w.creates
            .sketch
            .viewport
            .take_history(&mut w.input, true, true)
    );
}

#[test]
fn widgets_escape_and_edge_on_use_exact_messages_and_keep_navigation_gestures() {
    let f = fixtures::Drawing::new(vec![], false);
    let mut w = open(&f.path, Drawn::Mock);
    enter(&mut w, f.sketch, ferritecad_ui::ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Line", false);
    click(&mut w, point(0., 0.), false);
    ui.key(&mut w, egui::Key::Escape);
    assert!(draft(&w).pending.is_none());
    assert_eq!(draft(&w).tool, Tool::Line);
    ui.key(&mut w, egui::Key::Escape);
    assert_eq!(draft(&w).tool, Tool::Select);
    ui.press(&mut w, "Line", false);
    w.input.look_along([0., 1., 0.], [0., 0., 1.]);
    let region = w.input.region();
    assert!(w.creates.sketch.viewport.take_click(
        &mut w.input,
        [region.width() as f32 / 2., region.height() as f32 / 2.],
        false,
        true,
    ));
    assert!(draft(&w).state.curves.is_empty());
    assert_eq!(w.creates.sketch.viewport.status, geometry::EDGE_ON);
    assert!(
        text(&ui.settle(&mut w))
            .iter()
            .any(|t| t == geometry::EDGE_ON)
    );
    for button in [
        PointerButton::Primary,
        PointerButton::Secondary,
        PointerButton::Middle,
    ] {
        let before = *w.input.camera();
        for event in [
            ViewportEvent::PointerMoved { x: 300., y: 300. },
            ViewportEvent::PointerPressed(button),
            ViewportEvent::PointerMoved { x: 340., y: 320. },
            ViewportEvent::PointerReleased(button),
        ] {
            w.input.handle(event, false);
        }
        assert!(w.input.take_pick().is_none());
        assert!(w.input.take_context_pick().is_none());
        assert_ne!(*w.input.camera(), before);
        assert!(draft(&w).state.curves.is_empty());
    }
    ui.press(&mut w, "Look at sketch", false);
    w.creates.sketch.viewport.look(&mut w.input);
    assert_eq!(w.input.camera().up(), [0., 1., 0.]);
}

#[test]
fn native_new_sketch_rectangle_widgets_commit_one_version_and_one_document_undo() {
    rectangle_widgets(false);
}
#[test]
fn native_new_sketch_rectangle_with_reverse_side_reads_the_same_closed_profile() {
    rectangle_widgets(true);
}
fn rectangle_widgets(reverse: bool) {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("empty.fcad");
    Document::create(&path)
        .expect("create")
        .close()
        .expect("close");
    let mut w = open(&path, Drawn::Native);
    let before = version(&w);
    let mut ui = Ui::new(&mut w);
    start_new(&mut w, &mut ui);
    ui.press(&mut w, "Line", false);
    let points = [
        point(-10., -10.),
        point(10., -10.),
        point(10., 10.),
        point(-10., 10.),
    ];
    if reverse {
        for at in [points[0], points[1], points[2]] {
            click(&mut w, at, false);
        }
        ui.key(&mut w, egui::Key::Escape);
        for at in [points[0], points[3], points[2]] {
            click(&mut w, at, false);
        }
    } else {
        for at in points.into_iter().chain([points[0]]) {
            click(&mut w, at, false);
        }
    }
    let curves = draft(&w).state.curves.clone();
    let sketch = draft(&w).sketch;
    assert_eq!(curves.len(), 4);
    let request = finish(&mut w, &mut ui);
    assert!(matches!(
        &request.changes[..],
        [
            EditChange::AddDatumPlane { .. },
            EditChange::AddSketch { .. }
        ]
    ));
    assert!(apply(&mut w, request));
    assert_ne!(version(&w), before);
    assert!(!w.creates.sketch.active());
    let doc = Document::open(w.sessions.export_source().expect("source").path()).expect("open");
    let objects = doc.objects().expect("objects");
    assert_eq!(objects.len(), 2);
    assert_eq!(
        objects
            .iter()
            .filter_map(|o| o.name.as_deref())
            .collect::<Vec<_>>(),
        vec!["XY", "Sketch 1"]
    );
    let ObjectPayload::Sketch(saved) = objects
        .iter()
        .find(|o| o.id == sketch)
        .expect("sketch")
        .payload
        .clone()
    else {
        panic!("sketch payload");
    };
    assert_eq!(saved.curves, curves);
    let ends: Vec<_> = saved
        .curves
        .iter()
        .flat_map(|c| match c.geometry {
            SketchGeometry::Line { start, end } => vec![
                [start.x.to_bits(), start.y.to_bits()],
                [end.x.to_bits(), end.y.to_bits()],
            ],
            _ => panic!("line"),
        })
        .collect();
    for end in &ends {
        assert_eq!(ends.iter().filter(|p| *p == end).count(), 2);
    }
    drop(doc);
    w.step(true);
    assert!(
        Document::open(w.sessions.export_source().expect("source").path())
            .expect("open")
            .objects()
            .expect("objects")
            .is_empty()
    );
    w.step(false);
    let body = ObjectId::new();
    let feature = ObjectId::new();
    let request = EditRequest {
        source: w
            .sessions
            .export_source()
            .expect("source")
            .path()
            .to_owned(),
        destination: Default::default(),
        expected: version(&w),
        changes: vec![EditChange::AddBody {
            object: body,
            name: Some("Part".into()),
            feature_object: feature,
            feature_name: Some("Extrude".into()),
            feature: ferritecad_document::FeatureAddition::Extrude {
                profile: sketch,
                regions: vec![],
                reversed: false,
                operation: ferritecad_document::SolidOperation::NewBody,
                end_condition: ferritecad_document::EndCondition::Blind {
                    distance: ferritecad_document::Expression::constant(3.).expect("distance"),
                },
            },
        }],
    };
    assert!(apply(&mut w, request));
}

#[test]
fn native_refusal_keeps_draft_history_full_message_and_accepted_file() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let curves = fixtures::rectangle();
    let open_end = curves[3].id;
    let f = fixtures::Drawing::new(curves, true);
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.sketch, ferritecad_ui::ModelEdit::Sketch);
    let version = version(&w);
    let path = w
        .sessions
        .export_source()
        .expect("source")
        .path()
        .to_owned();
    let bytes = std::fs::read(&path).expect("bytes");
    let mut ui = Ui::new(&mut w);
    click(&mut w, point(5., 0.), false);
    ui.press(&mut w, "Delete", true);
    let state = draft(&w).state.clone();
    let history = draft(&w).undo.clone();
    let request = finish(&mut w, &mut ui);
    assert!(!apply(&mut w, request));
    let reason = format!(
        "invalid input: sketch {} gives no profile: invalid input: the profile does not \
         close: curve {open_end} has a free end at (0, 0)",
        f.sketch,
    );
    let expected = format!(
        "Could not apply the change: Extrude ({}), Part ({}): edit would leave features unbuilt:\n\
         {}: {reason}\n{}: not built: depends on {}: {reason}\n",
        f.feature, f.body, f.feature, f.body, f.feature,
    );
    assert_eq!(w.sessions.status, expected);
    assert!(text(&ui.settle(&mut w)).iter().any(|t| t == &expected));
    assert_eq!(draft(&w).state, state);
    assert_eq!(draft(&w).undo, history);
    assert_eq!(self::version(&w), version);
    assert_eq!(std::fs::read(&path).expect("bytes"), bytes);
    ui.press(&mut w, "Undo", true);
    assert_eq!(draft(&w).state.curves.len(), 4);
}

#[test]
fn native_solved_seed_is_not_written_and_deleted_line_lists_its_constraint_explicitly() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PlaneGCS"),
    ]) {
        return;
    }
    let mut f = fixtures::Drawing::new(vec![line([-10., -2.], [10., 2.])], false);
    let curve = f.saved().curves[0].id;
    let constraint = StableEntityId::new();
    f.doc
        .write_changes(&[EditChange::AddConstraint {
            sketch: f.sketch,
            constraint: SketchConstraint {
                id: constraint,
                rule: SketchConstraintRule::Horizontal {
                    a: SketchPointRef {
                        curve,
                        at: SketchPointSelector::Start,
                    },
                    b: SketchPointRef {
                        curve,
                        at: SketchPointSelector::End,
                    },
                },
            },
        }])
        .expect("constraint");
    let stored = f.saved();
    let mut w = open(&f.path, Drawn::Native);
    let before = version(&w);
    enter(&mut w, f.sketch, ferritecad_ui::ModelEdit::Sketch);
    assert_ne!(
        draft(&w).state.curves,
        stored.curves,
        "use solved coordinates"
    );
    let source = w.scene.edit_source.as_ref().expect("source");
    let presentation = w
        .scene
        .sketches
        .presentation(source, f.sketch)
        .expect("accepted");
    assert_eq!(
        &draft(&w).state.curves[0].geometry,
        presentation.curves()[0].geometry()
    );
    assert!(
        draft(&w).changes().is_empty(),
        "solving does not rewrite saved geometry"
    );
    let mut ui = Ui::new(&mut w);
    let SketchGeometry::Line { start, end } = draft(&w).state.curves[0].geometry else {
        panic!("line");
    };
    click(
        &mut w,
        point((start.x + end.x) / 2., (start.y + end.y) / 2.),
        false,
    );
    ui.press(&mut w, "Delete", true);
    assert!(
        text(&ui.settle(&mut w))
            .iter()
            .any(|t| t == "Removes 1 constraint of deleted curves.")
    );
    let request = finish(&mut w, &mut ui);
    assert_eq!(
        request.changes,
        vec![
            EditChange::RemoveConstraint {
                sketch: f.sketch,
                constraint
            },
            EditChange::RemoveCurve {
                sketch: f.sketch,
                curve
            },
        ]
    );
    assert!(apply(&mut w, request));
    assert_ne!(version(&w), before);
    assert!(!w.creates.sketch.active());
    w.step(true);
    let doc = Document::open(w.sessions.export_source().expect("source").path()).expect("doc");
    assert_eq!(
        doc.object(f.sketch)
            .expect("object")
            .expect("sketch")
            .payload,
        ObjectPayload::Sketch(stored)
    );
}

#[test]
fn mode_uses_existing_availability_tab_parking_close_and_quit_questions() {
    let a = fixtures::Drawing::new(vec![], false);
    let b = fixtures::Drawing::new(vec![], false);
    let mut w = open(&a.path, Drawn::Mock);
    let tab = w.sessions.tab();
    enter(&mut w, a.sketch, ferritecad_ui::ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Point", false);
    click(&mut w, point(4., 5.), false);
    let state = draft(&w).state.clone();
    let history = draft(&w).undo.clone();
    let expected = "Apply or Cancel the open form before opening another editor.";
    let available = commands::availability(
        &w.creates,
        &w.edits,
        &w.sessions,
        &w.tabs,
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &w.input,
        false,
    );
    assert_eq!(available, Err(expected));
    let source = w.scene.edit_source.as_ref().expect("source");
    assert_eq!(
        commands::new_sketch_available(source, available),
        Err(expected.into())
    );
    w.open(&b.path);
    assert!(!w.creates.sketch.active());
    assert!(w.tabs.has_draft(tab));
    w.switch(tab);
    assert_eq!(draft(&w).state, state);
    assert_eq!(draft(&w).undo, history);
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
        .expect("question");
    assert_eq!(
        w.tabs.form_close_question(&w.sessions).expect("address").0,
        id
    );
    assert!(!w.creates.sketch.active());
    assert!(w.tabs.cancel_form_close(
        &mut w.sessions,
        id,
        &mut crate::tabs::Forms {
            edits: &mut w.edits,
            editor: &mut w.creates.sketch,
        }
    ));
    assert_eq!(draft(&w).state, state);
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
        .expect("quit question");
    assert_eq!(
        w.tabs.quit_form_question(&w.sessions).expect("question").0,
        id
    );
    assert!(w.tabs.back_quit_form(&w.sessions, id));
    crate::restore_window_quit(&mut w.tabs, &mut w.sessions, &mut w.edits, &mut w.creates);
    assert_eq!(draft(&w).state, state);
    assert_eq!(draft(&w).undo, history);
}

#[test]
fn catalogue_offers_mode_only_for_accepted_drawings_and_new_sketch_only_for_planes() {
    for curves in [
        vec![],
        vec![curve(SketchGeometry::Arc {
            center: point(0., 0.),
            radius: 3.,
            start_angle: 0.,
            end_angle: 1.,
        })],
    ] {
        let f = fixtures::Drawing::new(curves, false);
        let w = open(&f.path, Drawn::Mock);
        let source = w.scene.edit_source.as_ref().expect("source");
        assert!(
            commands::actions(source, (f.sketch, None, None))
                .contains(&ferritecad_ui::ModelEdit::Sketch)
        );
        assert!(
            !commands::actions(source, (f.sketch, None, None))
                .contains(&ferritecad_ui::ModelEdit::NewSketch)
        );
        assert_eq!(
            commands::actions(source, (f.plane, None, None)),
            vec![ferritecad_ui::ModelEdit::NewSketch]
        );
        let mut unbuilt = source.clone();
        unbuilt.built_sketches.clear();
        assert!(
            !commands::actions(&unbuilt, (f.sketch, None, None))
                .contains(&ferritecad_ui::ModelEdit::Sketch)
        );
        let mut editor = Editor::default();
        assert!(!editor.begin_with(&f.path, &unbuilt, &Accepted::default(), f.sketch));
    }
}

#[test]
fn existing_arcs_can_be_selected_toggled_deleted_and_undone_through_widgets() {
    let arc = curve(SketchGeometry::Arc {
        center: point(0., 0.),
        radius: 10.,
        start_angle: 0.,
        end_angle: -2.,
    });
    let f = fixtures::Drawing::new(vec![arc.clone()], false);
    let mut w = open(&f.path, Drawn::Mock);
    enter(&mut w, f.sketch, ferritecad_ui::ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click(&mut w, point(10., 0.), false);
    assert_eq!(
        draft(&w).state.selected.iter().copied().collect::<Vec<_>>(),
        vec![arc.id]
    );
    ui.press(&mut w, "Construction", true);
    assert_eq!(
        draft(&w).changes(),
        vec![EditChange::SetConstruction {
            sketch: f.sketch,
            curve: arc.id,
            construction: true,
        }]
    );
    ui.key(&mut w, egui::Key::Backspace);
    assert_eq!(
        draft(&w).changes(),
        vec![EditChange::RemoveCurve {
            sketch: f.sketch,
            curve: arc.id
        }]
    );
    ui.press(&mut w, "Undo", true);
    ui.press(&mut w, "Undo", true);
    assert_eq!(draft(&w).state.curves, vec![arc]);
    assert!(draft(&w).changes().is_empty());
}

#[test]
fn mode_consumes_hover_and_readonly_clicks_without_entity_requests_or_draft_changes() {
    let f = fixtures::Drawing::new(vec![line([-10., 0.], [10., 0.])], false);
    let mut w = open(&f.path, Drawn::Mock);
    enter(&mut w, f.sketch, ferritecad_ui::ModelEdit::Sketch);
    let state = draft(&w).state.clone();
    let pixel =
        geometry::screen(w.input.camera(), SketchPlane::world_xy(), point(5., 0.)).expect("pixel");
    w.input.handle(
        ViewportEvent::PointerMoved {
            x: pixel[0] as f32,
            y: pixel[1] as f32,
        },
        false,
    );
    assert!(w.creates.sketch.viewport.take_hover(&mut w.input));
    assert_eq!(w.input.take_hover(), ferritecad_ui::Hover::Unchanged);
    assert_eq!(w.creates.sketch.viewport.hovered, Some(state.curves[0].id));
    assert!(w.creates.sketch.viewport.take_click(
        &mut w.input,
        pixel.map(|v| v as f32),
        false,
        false
    ));
    assert!(
        w.creates
            .sketch
            .viewport
            .take_history(&mut w.input, true, false)
    );
    assert_eq!(draft(&w).state, state);
    w.input.handle(ViewportEvent::PointerLeft, false);
    assert!(w.creates.sketch.viewport.take_hover(&mut w.input));
    assert_eq!(w.creates.sketch.viewport.hovered, None);
    assert_eq!(w.creates.sketch.viewport.cursor, None);
    assert_eq!(w.creates.sketch.viewport.snapped, None);
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Cancel", false);
    assert!(!w.creates.sketch.viewport.take_hover(&mut w.input));
    assert!(
        !w.creates
            .sketch
            .viewport
            .take_click(&mut w.input, [0., 0.], false, true)
    );
}

#[test]
fn entry_refuses_absent_objects_planes_and_mismatched_accepted_versions() {
    let a = fixtures::Drawing::new(vec![], false);
    let b = fixtures::Drawing::new(vec![], false);
    let w = open(&a.path, Drawn::Mock);
    let foreign = open(&b.path, Drawn::Mock);
    let source = w.scene.edit_source.as_ref().expect("source");
    let mut editor = Editor::default();
    assert!(!editor.begin_with(&a.path, source, &foreign.scene.sketches, a.sketch));
    assert!(!editor.begin_with(&a.path, source, &w.scene.sketches, a.plane));
    assert!(!editor.begin_new(&a.path, source, Some(ObjectId::new())));
    let mut refused = source.clone();
    refused.refusal = Some("Document facts are unavailable.".into());
    assert!(!editor.begin_new(&a.path, &refused, None));
    assert!(!editor.begin_with(&a.path, &refused, &w.scene.sketches, a.sketch));
    assert!(editor.begin_with(&a.path, source, &w.scene.sketches, a.sketch));
    assert!(!editor.begin_new(&a.path, source, None));
    assert!(!editor.begin_with(&a.path, source, &w.scene.sketches, a.sketch));
}

#[test]
fn native_finish_of_new_empty_sketch_reuses_selected_plane_without_an_extra_datum() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let f = fixtures::Drawing::new(vec![], false);
    let mut w = open(&f.path, Drawn::Native);
    enter(&mut w, f.plane, ferritecad_ui::ModelEdit::NewSketch);
    let mut ui = Ui::new(&mut w);
    let request = finish(&mut w, &mut ui);
    assert!(
        matches!(&request.changes[..],[EditChange::AddSketch {sketch,..}]
        if sketch.plane == f.plane && sketch.curves.is_empty())
    );
    assert!(apply(&mut w, request));
    assert!(!w.creates.sketch.active());
}
