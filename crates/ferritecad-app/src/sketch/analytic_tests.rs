// SPDX-License-Identifier: MIT
//! Round drawings retain the input-owner checks of the retired analytic canvases.
use super::Editor;
use crate::sketch_mode::tests::curve_edits;
use ferritecad_ui::{PointerButton, ViewportEvent};

pub(crate) fn begin_saved_circle(e: &mut Editor, root: &std::path::Path) -> bool {
    let path = root.join("round.fcad");
    crate::feature_tests::write_round(&path, [12.5, -7.25], &[10.5], 15.25, false);
    let reading = crate::sketch_facts::read(&path).expect("reading");
    e.begin_test_drawing(&path, &reading, curve_edits::sketch_id(&reading))
}
pub(crate) fn hold_handle(e: &mut Editor) -> (egui::Context, egui::Pos2) {
    curve_edits::hold(e)
}
pub(crate) fn release_handle(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
    super::drag_tests::release_vertex(ctx, e, at);
}

#[test]
fn busy_queued_covered_and_outside_presses_start_nothing_and_held_fields_take_no_text() {
    use crate::sessions::tests::tabs::{Drawn, Window};
    use crate::sketch_mode::tests::window;
    let root = tempfile::tempdir().expect("root");
    for radii in [&[10.5][..], &[10.5, 4.75][..]] {
        let path = root.path().join(format!("round-{}.fcad", radii.len()));
        crate::feature_tests::write_round(&path, [12.5, -7.25], radii, 15.25, false);
        let mut w = Window::new(Drawn::Mock, None);
        w.open(&path);
        let id = curve_edits::sketch_id(w.scene.edit_source.as_ref().expect("source"));
        window::enter(&mut w, id, ferritecad_ui::ModelEdit::Sketch);
        w.input = curve_edits::input(&w.creates.sketch);
        let at = curve_edits::screen(
            &w.creates.sketch,
            &w.input,
            curve_edits::first_handle(&w.creates.sketch),
        );
        let initial = curve_edits::state(&w.creates.sketch);
        // A worker owns the session until its result is accepted, including queued work.
        w.sessions
            .begin_apply(|_, _, _| std::thread::spawn(|| {}))
            .expect("queued worker");
        for (claimed, editable, start) in [
            (false, w.sessions.takes_form_apply(), at),
            (true, true, at),
            (false, true, (-10., -10.)),
        ] {
            for event in [
                ViewportEvent::PointerMoved {
                    x: start.0,
                    y: start.1,
                },
                ViewportEvent::PointerPressed(PointerButton::Primary),
                ViewportEvent::PointerMoved {
                    x: at.0 + 24.,
                    y: at.1 + 12.,
                },
                ViewportEvent::PointerReleased(PointerButton::Primary),
            ] {
                w.creates
                    .sketch
                    .viewport
                    .route(&mut w.input, event, claimed, editable, false);
                w.creates.sketch.viewport.take_hover(&mut w.input);
                assert!(!w.creates.sketch.gesturing());
            }
            assert_eq!(curve_edits::state(&w.creates.sketch), initial);
        }
        w.sessions.stop_all();
        let (ctx, at) = hold_handle(&mut w.creates.sketch);
        let before = curve_edits::state(&w.creates.sketch);
        for event in [
            egui::Event::Text("9".into()),
            egui::Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            },
        ] {
            super::tests::frame(&ctx, &mut w.creates.sketch, vec![event]);
            assert_eq!(
                curve_edits::state(&w.creates.sketch),
                before,
                "a held handle owns the drawing's controls and keys"
            );
        }
        release_handle(&ctx, &mut w.creates.sketch, at);
        assert!(!w.creates.sketch.gesturing());
        assert!(w.creates.sketch.viewport.take_request().is_none());
    }
}
