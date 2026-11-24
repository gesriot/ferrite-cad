// SPDX-License-Identifier: MIT
//! The surviving viewport owns camera gestures; the form owns only its captured handles.
use super::Editor;
use crate::sketch_mode::tests::curve_edits;
use ferritecad_ui::{PointerButton, ViewportEvent};

#[test]
fn a_pan_ends_without_a_trace_by_release_outside_escape_focus_pointer_gone_or_a_busy_form() {
    use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};
    let (root, path, source) = crate::fillets::tests::plate();
    for end in [
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Middle,
        },
        WindowEvent::Focused(false),
    ] {
        let mut e = Editor::default();
        assert!(e.begin_test_drawing(&path, &source, curve_edits::sketch_id(&source)));
        let before = curve_edits::state(&e);
        let mut input = curve_edits::input(&e);
        input.handle(ViewportEvent::PointerMoved { x: 400., y: 300. }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Middle), false);
        input.handle(ViewportEvent::PointerMoved { x: 440., y: 325. }, false);
        assert!(input.is_dragging());
        e.viewport
            .window_event(&mut input, &end, false, false, true, false);
        assert!(!input.is_dragging());
        assert_eq!(
            curve_edits::state(&e),
            before,
            "navigation cannot edit the draft"
        );
        assert!(!e.gesturing());
    }
    let mut e = Editor::default();
    assert!(e.begin_test_drawing(&path, &source, curve_edits::sketch_id(&source)));
    let before = curve_edits::state(&e);
    let mut input = curve_edits::input(&e);
    hold_camera(&mut input);
    input.handle(ViewportEvent::PointerMoved { x: 50., y: 40. }, false);
    let held = *input.camera();
    e.viewport.window_event(
        &mut input,
        &WindowEvent::CursorLeft {
            device_id: DeviceId::dummy(),
        },
        false,
        false,
        true,
        false,
    );
    assert!(e.viewport.take_hover(&mut input));
    assert!(!input.is_dragging(), "PointerGone must end the pan");
    assert_eq!(
        *input.camera(),
        held,
        "pointer loss keeps the view where it was"
    );
    assert_eq!(curve_edits::state(&e), before);
    let (ctx, _) = curve_edits::hold(&mut e);
    super::tests::frame(
        &ctx,
        &mut e,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert!(!e.gesturing());
    assert_eq!(curve_edits::state(&e), before);
    drop(root);
}

#[test]
fn navigation_over_a_canvas_never_reaches_the_3d_camera_and_a_viewport_drag_still_crosses_the_form()
{
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
