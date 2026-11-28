// SPDX-License-Identifier: MIT
//! Replies to a failed Finish and pointer ownership on saved viewport rules.
use super::constraint_widgets::click_label;
use super::widgets::Ui;
use super::window::{enter, finish, open, version};
use super::*;
use crate::sessions::{Edited, tests::tabs::Drawn};
use ferritecad_types::CadError;
use ferritecad_ui::ModelEdit;

#[derive(Clone, Copy)]
enum Pick {
    Distance,
    Angle,
    Marker,
}

use Pick::{Angle, Distance, Marker};

fn source(
    pick: Pick,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    ObjectId,
    StableEntityId,
) {
    let root = tempfile::tempdir().expect("fixture directory");
    let path = root.path().join("part.fcad");
    match pick {
        Distance => crate::feature_tests::write_dimensioned_rectangle(&path),
        Angle => crate::feature_tests::write_angled_quad(&path),
        Marker => crate::feature_tests::write_marked_rectangle(&path),
    }
    let (sketch, stored) = super::curve_edits::stored(&path);
    let id = stored
        .constraints
        .iter()
        .find(|c| match (pick, &c.rule) {
            (Distance, SketchConstraintRule::Distance { distance, .. }) => distance.value() == 60.,
            (Angle, SketchConstraintRule::DirectedAngle { angle, .. }) => {
                (angle.value().to_degrees() + 100.).abs() < 1e-9
            }
            (Marker, SketchConstraintRule::Horizontal { .. }) => true,
            _ => false,
        })
        .expect("saved rule")
        .id;
    (root, path, sketch, id)
}

fn preserves_draft(pick: Pick) {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_eval::solver_available(), "PLANEGCS"),
        (ferritecad_occt::is_available(), "OCCT"),
    ]) {
        return;
    }
    let (_root, path, sketch, id) = source(pick);
    let bytes = std::fs::read(&path).expect("saved file");
    let mut w = open(&path, Drawn::Native);
    let accepted = version(&w);
    enter(&mut w, sketch, ModelEdit::Sketch);
    w.input
        .frame(([-20., -20., -1.], [80., 50., 1.]))
        .expect("fit");
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, id);
    match pick {
        Distance | Angle => {
            let (value, other, typed) = if matches!(pick, Distance) {
                ("70", "68", "58.25")
            } else {
                ("-130", "-115", "-125.25")
            };
            for value in [value, other] {
                ui.replace_dimension_text(&mut w, value);
                ui.press(&mut w, "Set", false);
            }
            ui.press(&mut w, "Undo", true);
            ui.replace_dimension_text(&mut w, typed);
        }
        Marker => {
            ui.press(&mut w, "Delete", true);
            ui.press(&mut w, "Undo", true);
            ui.press(&mut w, "Redo", true);
        }
    }
    let d = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    let (state, undo, redo) = (d.state.clone(), d.undo.clone(), d.redo.clone());
    let field = w.creates.sketch.viewport.value_field.clone();
    match pick {
        Distance | Angle => {
            assert_eq!(state.selected, [Selection::Constraint(id)]);
            let typed = if matches!(pick, Distance) {
                "58.25"
            } else {
                "-125.25"
            };
            assert_eq!(field.as_ref().expect("unsubmitted Value").2, typed);
            assert_eq!((undo.len(), redo.len()), (1, 1));
        }
        Marker => {
            assert!(state.constraints.iter().all(|c| c.id != id));
            assert_eq!((undo.len(), redo.len()), (1, 0));
        }
    }
    let changes = d.changes().expect("pending edit");
    assert!(!changes.is_empty() && !undo.is_empty());
    for error in [
        CadError::topology("lost stored reference"),
        CadError::Cancelled,
    ] {
        let request = finish(&mut w, &mut ui);
        assert_eq!(request.expected, accepted);
        assert_eq!(request.changes, changes);
        assert!(w.creates.sketch.viewport.take_request().is_none());
        let owner = w
            .sessions
            .begin_apply(|_, _, _| std::thread::spawn(|| {}))
            .expect("start");
        assert!(
            w.sessions
                .begin_apply(|_, _, _| panic!("duplicate request"))
                .is_none()
        );
        let status = w.sessions.status.clone();
        ui.settle(&mut w);
        assert_eq!(w.creates.sketch.viewport.value_field, field);
        assert_eq!(
            w.sessions
                .finish_apply(owner + 1, Err(CadError::input("stale"))),
            Edited::Ignore
        );
        assert!(w.sessions.busy());
        assert_eq!(w.sessions.status, status);
        let reason = match &error {
            CadError::Cancelled => "Cancelled; nothing was changed.".to_owned(),
            _ => format!("Could not apply the change: {error}"),
        };
        assert_eq!(w.sessions.finish_apply(owner, Err(error)), Edited::Failed);
        assert_eq!(w.sessions.status, reason);
        ui.settle(&mut w);
        let d = w
            .creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("retryable draft");
        assert_eq!(d.state, state);
        assert_eq!(d.undo, undo);
        assert_eq!(d.redo, redo);
        assert_eq!(d.changes().expect("retry"), changes);
        assert_eq!(w.creates.sketch.viewport.value_field, field);
        assert_eq!(version(&w), accepted);
        assert!(!w.sessions.can_undo() && !w.sessions.can_redo());
        assert_eq!(std::fs::read(&path).expect("after the reply"), bytes);
    }
}

#[test]
fn a_saved_distance_edit_survives_stale_refused_and_cancelled_finish_replies() {
    preserves_draft(Distance);
}
#[test]
fn a_saved_angle_edit_survives_stale_refused_and_cancelled_finish_replies() {
    preserves_draft(Angle);
}
#[test]
fn a_saved_marker_removal_survives_stale_refused_and_cancelled_finish_replies() {
    preserves_draft(Marker);
}

/// Route the physical pointer through the same press owner and delayed pick as the window.
fn route(
    w: &mut crate::sessions::tests::tabs::Window,
    event: ferritecad_ui::ViewportEvent,
    claimed: bool,
) {
    let editable = input_editable(&w.creates, &w.edits, &w.sessions);
    let editor = &mut w.creates.sketch.viewport;
    editor.route(&mut w.input, event, claimed, editable, false);
    if let Some((x, y)) = w.input.take_pick() {
        assert!(editor.take_click(&mut w.input, [x, y], false, editable));
    }
}

fn label_ownership(pick: Pick) {
    use ferritecad_ui::PointerButton::Primary;
    use ferritecad_ui::ViewportEvent::{
        GestureCancelled, PointerMoved, PointerPressed, PointerReleased,
    };
    use ferritecad_viewport::PixelRect;
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_eval::solver_available(), "PLANEGCS"),
        (ferritecad_occt::is_available(), "OCCT"),
    ]) {
        return;
    }
    let (_root, path, sketch, id) = source(pick);
    let bytes = std::fs::read(&path).expect("source");
    for scale in [1., 2.] {
        let mut w = open(&path, Drawn::Native);
        let accepted = version(&w);
        enter(&mut w, sketch, ModelEdit::Sketch);
        let region = PixelRect::new(80, 32, 960, 640);
        w.input.set_region(region);
        w.input
            .frame(([-20., -20., -1.], [80., 50., 1.]))
            .expect("fit");
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(scale);
        let editor = &mut w.creates.sketch.viewport;
        let mut painted = ctx.run_ui(egui::RawInput::default(), |ui| {
            editor.paint(ui.ctx(), w.input.camera(), region);
        });
        painted.textures_delta.clear();
        let [a, b] = editor
            .labels
            .iter()
            .find(|(key, _)| *key == id)
            .expect("label")
            .1;
        let (x, y) = region.window((((a[0] + b[0]) / 2.) as f32, ((a[1] + b[1]) / 2.) as f32));
        let at = PointerMoved { x, y };
        let state = editor.draft.as_ref().expect("open").state.clone();
        let blocked = |w: &crate::sessions::tests::tabs::Window| {
            let d = w.creates.sketch.viewport.draft.as_ref().expect("draft");
            assert_eq!(d.state, state);
            assert!(d.undo.is_empty() && d.redo.is_empty());
            assert_eq!(version(w), accepted);
        };

        // A press owned by another layer cannot turn into a pick on release.
        route(&mut w, at, true);
        route(&mut w, PointerPressed(Primary), true);
        route(&mut w, PointerReleased(Primary), false);
        blocked(&w);
        route(&mut w, at, false);
        route(&mut w, PointerPressed(Primary), false);
        route(&mut w, GestureCancelled, false);
        route(&mut w, at, false);
        route(&mut w, PointerReleased(Primary), false);
        blocked(&w);

        // Even returning to the same layout cannot revive a press on the old picture.
        route(&mut w, PointerPressed(Primary), false);
        w.input.set_region(PixelRect::new(82, 32, 960, 640));
        w.input.set_region(region);
        route(&mut w, PointerReleased(Primary), false);
        blocked(&w);
        let owner = w
            .sessions
            .begin_apply(|_, _, _| std::thread::spawn(|| {}))
            .expect("worker");
        route(&mut w, at, false);
        route(&mut w, PointerPressed(Primary), false);
        route(&mut w, PointerReleased(Primary), false);
        blocked(&w);
        assert_eq!(
            w.sessions.finish_apply(owner, Err(CadError::Cancelled)),
            Edited::Failed
        );

        route(&mut w, at, false);
        route(&mut w, PointerPressed(Primary), false);
        route(&mut w, PointerReleased(Primary), false);
        let mut selected = state;
        selected.selected = vec![Selection::Constraint(id)];
        let d = w.creates.sketch.viewport.draft.as_ref().expect("draft");
        assert_eq!(d.state, selected);
        assert!(d.undo.is_empty() && d.redo.is_empty());
        assert!(d.changes().expect("pick").is_empty());
        assert_eq!(version(&w), accepted);
        assert!(!w.sessions.can_undo() && !w.sessions.can_redo());
        assert_eq!(std::fs::read(&path).expect("after picks"), bytes);
    }
}

#[test]
fn saved_distance_label_accepts_only_a_click_on_its_current_idle_view() {
    label_ownership(Distance);
}

#[test]
fn saved_angle_label_accepts_only_a_click_on_its_current_idle_view() {
    label_ownership(Angle);
}

#[test]
fn saved_rule_marker_accepts_only_a_click_on_its_current_idle_view() {
    label_ownership(Marker);
}
