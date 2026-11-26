// SPDX-License-Identifier: MIT
//! The surviving viewport owns camera gestures; the form owns only its captured handles.
use super::Editor;
use crate::sketch_mode::tests::curve_edits;
use ferritecad_ui::{PointerButton, ViewportEvent};

#[test]
fn a_viewport_pan_holds_transitions_until_release_or_focus_loss_and_edits_nothing() {
    use crate::sessions::tests::tabs::Drawn;
    use crate::sketch_mode::tests::window;
    use ferritecad_ui::ModelEdit;
    use winit::{
        dpi::PhysicalPosition,
        event::{DeviceId, ElementState, MouseButton, WindowEvent},
    };
    let (_root, path, source) = crate::fillets::tests::plate();
    let saved = std::fs::read(&path).expect("source bytes");
    for lost_focus in [false, true] {
        let mut w = window::open(&path, Drawn::Mock);
        window::enter(&mut w, curve_edits::sketch_id(&source), ModelEdit::Sketch);
        let before = curve_edits::state(&w.creates.sketch);
        let start = *w.input.camera();
        assert!(crate::idle_but_for_new(
            &w.creates,
            &w.edits,
            &w.sessions,
            &w.input
        ));
        hold_camera(&mut w.input);
        w.creates.sketch.viewport.window_event(
            &mut w.input,
            &WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(1400., 1100.),
            },
            false,
            true,
            true,
            false,
        );
        let held = *w.input.camera();
        assert_ne!(held, start, "a pan continues beyond the picture");
        w.creates.sketch.viewport.window_event(
            &mut w.input,
            &WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            },
            false,
            true,
            true,
            false,
        );
        assert!(
            w.input.is_dragging(),
            "leaving the window keeps the held button"
        );
        assert!(!crate::idle_but_for_new(
            &w.creates,
            &w.edits,
            &w.sessions,
            &w.input
        ));
        assert_eq!(*w.input.camera(), held);
        let end = if lost_focus {
            WindowEvent::Focused(false)
        } else {
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state: ElementState::Released,
                button: MouseButton::Middle,
            }
        };
        w.creates
            .sketch
            .viewport
            .window_event(&mut w.input, &end, false, true, true, false);
        assert!(!w.input.is_dragging());
        assert_eq!(*w.input.camera(), held, "ending a pan keeps its final view");
        assert!(crate::idle_but_for_new(
            &w.creates,
            &w.edits,
            &w.sessions,
            &w.input
        ));
        assert_eq!(curve_edits::state(&w.creates.sketch), before);
        assert!(!w.creates.sketch.gesturing());
        assert!(w.creates.sketch.viewport.take_request().is_none());
        assert!(!w.sessions.dirty());
    }

    let mut w = window::open(&path, Drawn::Mock);
    window::enter(&mut w, curve_edits::sketch_id(&source), ModelEdit::Sketch);
    let before = curve_edits::state(&w.creates.sketch);
    let at = curve_edits::screen(
        &w.creates.sketch,
        &w.input,
        curve_edits::first_handle(&w.creates.sketch),
    );
    for event in [
        ViewportEvent::PointerMoved { x: at.0, y: at.1 },
        ViewportEvent::PointerPressed(PointerButton::Middle),
        ViewportEvent::PointerPressed(PointerButton::Primary),
        ViewportEvent::PointerMoved {
            x: at.0 + 40.,
            y: at.1 + 30.,
        },
    ] {
        w.creates
            .sketch
            .viewport
            .route(&mut w.input, event, false, true, false);
        assert!(
            !w.creates.sketch.gesturing(),
            "a camera gesture cannot acquire a handle"
        );
    }
    assert!(w.creates.sketch.viewport.take_hover(&mut w.input));
    assert_eq!(curve_edits::state(&w.creates.sketch), before);
    w.creates.sketch.viewport.route(
        &mut w.input,
        ViewportEvent::GestureCancelled,
        false,
        true,
        false,
    );
    assert!(!w.input.is_dragging());
    assert!(crate::idle_but_for_new(
        &w.creates,
        &w.edits,
        &w.sessions,
        &w.input
    ));
    assert_eq!(curve_edits::state(&w.creates.sketch), before);
    assert!(w.creates.sketch.viewport.take_request().is_none());
    assert!(!w.sessions.dirty());
    assert_eq!(std::fs::read(&path).expect("unchanged source"), saved);
}

#[test]
fn sketch_handles_own_input_and_a_viewport_drag_still_crosses_the_form() {
    use winit::{
        dpi::PhysicalPosition,
        event::{DeviceId, ElementState, MouseButton, WindowEvent},
    };
    for pixels_per_point in [1., 2.] {
        for radii in [&[10.5][..], &[10.5, 4.75][..]] {
            let root = tempfile::tempdir().expect("root");
            let path = root.path().join("round.fcad");
            crate::feature_tests::write_round(&path, [12.5, -7.25], radii, 15.25, false);
            let source = crate::sketch_facts::read(&path).expect("source");
            let mut e = Editor::default();
            assert!(e.begin_test_drawing(&path, &source, curve_edits::sketch_id(&source)));
            let mut input = curve_edits::input(&e);
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(pixels_per_point);
            for _ in 0..3 {
                super::tests::frame(&ctx, &mut e, vec![]);
            }
            let before = curve_edits::state(&e);
            let camera = *input.camera();
            let at = curve_edits::screen(&e, &input, curve_edits::first_handle(&e));
            for event in [
                ViewportEvent::PointerMoved { x: at.0, y: at.1 },
                ViewportEvent::PointerPressed(PointerButton::Primary),
                ViewportEvent::PointerMoved {
                    x: at.0 + 24.,
                    y: at.1 + 12.,
                },
            ] {
                e.viewport.route(&mut input, event, false, true, false);
            }
            assert!(e.gesturing());
            assert_eq!(
                *input.camera(),
                camera,
                "the handle must not orbit the view"
            );
            assert!(
                input.take_pick().is_none(),
                "the handle must not pick the body behind it"
            );
            e.viewport.route(
                &mut input,
                ViewportEvent::GestureCancelled,
                false,
                true,
                false,
            );
            assert_eq!(curve_edits::state(&e), before);

            // A camera drag started away from a handle retains ownership across the panel.
            let motion = |x, y| WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(x, y),
            };
            let button = |state| WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button: MouseButton::Middle,
            };
            for (event, claimed) in [
                (motion(700., 550.), false),
                (button(ElementState::Pressed), false),
                (motion(50. * f64::from(pixels_per_point), 50.), true),
                (button(ElementState::Released), true),
            ] {
                e.viewport
                    .window_event(&mut input, &event, false, claimed, true, false);
            }
            assert_ne!(*input.camera(), camera, "the panel swallowed a camera drag");
            assert!(!input.is_dragging());
            assert_eq!(curve_edits::state(&e), before);
            assert!(e.viewport.take_request().is_none());
        }
    }
}

pub(crate) fn hold_camera(input: &mut ferritecad_ui::ViewportInput) {
    let (x, y) = input.region().window((10., 10.));
    input.handle(ViewportEvent::PointerMoved { x, y }, false);
    input.handle(ViewportEvent::PointerPressed(PointerButton::Middle), false);
    assert!(input.is_dragging());
}
pub(crate) fn release_camera(input: &mut ferritecad_ui::ViewportInput) {
    input.handle(ViewportEvent::PointerReleased(PointerButton::Middle), false);
    assert!(!input.is_dragging());
}
