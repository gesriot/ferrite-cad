// SPDX-License-Identifier: MIT
//! Saved circles edited through the viewport's real controls and point gestures.

use super::constraint_widgets::{click_label, press_rule};
use super::dragging::{frame, movement, press, release};
use super::widgets::Ui;
use super::window::{click, enter, finish, open};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_ui::{ModelEdit, PointerButton, ViewportEvent};

pub(crate) fn edit(
    path: &std::path::Path,
    sketch: ObjectId,
    center: [f64; 2],
    radii: &[f64],
    navigate: bool,
    after_navigation: impl FnOnce(),
) -> (Window, EditRequest) {
    let drawn = if ferritecad_occt::is_available() {
        Drawn::Native
    } else {
        Drawn::Mock
    };
    edit_in(
        path,
        sketch,
        center,
        radii,
        navigate,
        after_navigation,
        drawn,
    )
}

fn edit_in(
    path: &Path,
    sketch: ObjectId,
    center: [f64; 2],
    radii: &[f64],
    navigate: bool,
    after_navigation: impl FnOnce(),
    drawn: Drawn,
) -> (Window, EditRequest) {
    let mut w = open(path, drawn);
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    let original = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("draft")
        .state
        .clone();
    assert!(original.constraints.is_empty());
    assert_eq!(original.curves.len(), radii.len());
    if navigate {
        let before = *w.input.camera();
        for event in [
            ViewportEvent::PointerMoved { x: 400., y: 300. },
            ViewportEvent::PointerPressed(PointerButton::Middle),
            ViewportEvent::PointerMoved { x: 430., y: 320. },
            ViewportEvent::PointerReleased(PointerButton::Middle),
            ViewportEvent::Wheel { delta: 2. },
        ] {
            w.input.handle(event, false);
        }
        assert_ne!(*w.input.camera(), before);
        assert_eq!(
            w.creates
                .sketch
                .viewport
                .draft
                .as_ref()
                .expect("draft")
                .state,
            original
        );
        ui.press(&mut w, "Look at sketch", false);
        w.creates.sketch.viewport.look(&mut w.input);
    }
    after_navigation();
    if ferritecad_eval::solver_available() {
        for (saved, radius) in original.curves.iter().zip(radii) {
            let SketchGeometry::Circle {
                center,
                radius: before,
            } = saved.geometry
            else {
                panic!("round fixture");
            };
            click(&mut w, point(center.x + before, center.y), false);
            assert_eq!(
                w.creates
                    .sketch
                    .viewport
                    .draft
                    .as_ref()
                    .expect("draft")
                    .state
                    .selected,
                vec![Selection::Curve(saved.id)]
            );
            press_rule(&mut ui, &mut w, "Radius");
            ui.type_value(&mut w, &before.to_string(), &radius.to_string());
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
            assert!(
                w.creates
                    .sketch
                    .viewport
                    .draft
                    .as_ref()
                    .expect("draft")
                    .state
                    .constraints
                    .is_empty()
            );
        }
    }
    // Dimensions need the solver; the viewport has no circle-rim drag yet.
    ui.press(&mut w, "Off", false);
    ui.press(&mut w, "0.1 mm", false);
    let target = point(center[0], center[1]);
    let mut expected = original.curves.clone();
    for (saved, radius) in expected.iter_mut().zip(radii) {
        let SketchGeometry::Circle {
            center: before,
            radius: saved_radius,
        } = saved.geometry
        else {
            panic!("round fixture");
        };
        press(&mut w, before);
        assert!(w.creates.sketch.viewport.gesturing());
        movement(&mut w, target);
        frame(&mut w);
        release(&mut w);
        saved.geometry = SketchGeometry::Circle {
            center: target,
            radius: if ferritecad_eval::solver_available() {
                *radius
            } else {
                saved_radius
            },
        };
    }
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    assert_eq!(draft.state.constraints, original.constraints);
    assert_eq!(draft.state.curves, expected);
    let request = finish(&mut w, &mut ui);
    assert_eq!(
        request.changes,
        expected
            .into_iter()
            .map(|curve| EditChange::SetCurveGeometry {
                sketch,
                curve: curve.id,
                geometry: curve.geometry,
            })
            .collect::<Vec<_>>()
    );
    (w, request)
}

/// Keep the fixture path alive independently of the temporary window used to drive widgets.
pub(crate) fn detached(
    path: &Path,
    source: &Source,
    sketch: ObjectId,
    center: [f64; 2],
    radii: &[f64],
) -> (crate::sketch::Editor, EditRequest) {
    let (mut window, mut request) = edit_in(path, sketch, center, radii, false, || {}, Drawn::Mock);
    assert_eq!(request.expected, source.version);
    let mut forms = std::mem::take(&mut window.creates.sketch);
    forms.viewport.source = path.to_path_buf();
    forms.viewport.clear_pointer();
    request.source = path.to_path_buf();
    (forms, request)
}
