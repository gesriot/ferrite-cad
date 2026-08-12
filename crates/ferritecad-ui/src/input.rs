// SPDX-License-Identifier: MIT
//! What a gesture means, decided without anything that could deliver one.
//!
//! A reducer: events go in, a [`Camera`] moves, and a redraw is asked for. No
//! window, no event loop, no interface toolkit appears in any signature here,
//! so every rule below can be stated as a test rather than as a thing to try
//! by hand and hope.
//!
//! # Why the interface gets first refusal
//!
//! An interface drawn over a viewport occupies part of it. A click on a button
//! must press the button and must not also spin the model behind it, so every
//! event arrives with a flag saying whether the interface already wants it.
//! That flag is honoured here rather than in the layer that produced it,
//! because "was this click for the panel" is a rule about what the application
//! means and not about how the toolkit reports it.
//!
//! A consumed event still updates where the pointer is. Ignoring it entirely
//! would leave the last known position at the far side of a panel, and the
//! first drag afterwards would jump the model by the width of it.

use ferritecad_types::Result;
use ferritecad_viewport::{Camera, StandardView};

/// How far a wheel notch moves the camera.
///
/// Small enough that a notch is a step rather than a leap, and exponential at
/// the camera, so the same notch covers the same proportion at any distance.
const WHEEL_ZOOM: f32 = 0.12;

/// Radians of orbit per pixel dragged.
///
/// A full turn takes roughly the width of a large window, which is what makes
/// a drag feel like turning the object rather than flinging it.
const ORBIT_PER_PIXEL: f32 = 0.008;

/// Which pointer button a gesture used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerButton {
    /// Orbits.
    Primary,
    /// Pans, which is what a middle button does in every CAD tool.
    Middle,
    /// Pans as well, for pointers with no middle button.
    Secondary,
}

/// Something the user did.
///
/// This project's own vocabulary rather than a windowing system's: the
/// translation from one to the other is a small, obvious function, and keeping
/// it out of here is what lets these rules be tested at all.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ViewportEvent {
    PointerMoved {
        x: f32,
        y: f32,
    },
    PointerPressed(PointerButton),
    PointerReleased(PointerButton),
    /// Positive scrolls towards the model.
    Wheel {
        delta: f32,
    },
    /// A named direction, from a key or a panel.
    Look(StandardView),
}

/// The camera, and what the user is in the middle of doing to it.
#[derive(Debug, Clone)]
pub struct ViewportInput {
    camera: Camera,
    pointer: Option<(f32, f32)>,
    dragging: Option<PointerButton>,
    redraw: bool,
}

impl Default for ViewportInput {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewportInput {
    pub fn new() -> Self {
        Self {
            camera: Camera::new(),
            pointer: None,
            dragging: None,
            // The first frame has never been drawn, so it is owed.
            redraw: true,
        }
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Whether a gesture is in progress.
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// Records a new window size and returns the size to configure with.
    ///
    /// One call, one size, used for the camera here and returned for the
    /// surface. The two must agree exactly – a camera with one aspect ratio
    /// drawing into a surface with another stretches the model – and the way
    /// to be sure they do is for there to be only one place the number comes
    /// from.
    pub fn resize(&mut self, width: u32, height: u32) -> (u32, u32) {
        self.camera.resize(width, height);
        self.redraw = true;
        (self.camera.width(), self.camera.height())
    }

    /// Points the camera at a box.
    pub fn frame(&mut self, bounds: ([f32; 3], [f32; 3])) -> Result<()> {
        self.camera.frame(bounds)?;
        self.redraw = true;
        Ok(())
    }

    /// Applies one event.
    ///
    /// `claimed_by_ui` is what the interface said when asked whether it wanted
    /// this event. A claimed event never moves the camera and never starts a
    /// gesture; it only keeps track of where the pointer is, so that letting go
    /// of a panel and dragging the model does not jump.
    ///
    /// A gesture already under way is not interrupted by the interface
    /// claiming a later event. Once a drag has started in the viewport it
    /// belongs to the viewport until the button comes up, which is what stops
    /// a model from stopping dead when the cursor crosses a panel.
    pub fn handle(&mut self, event: ViewportEvent, claimed_by_ui: bool) {
        match event {
            ViewportEvent::PointerMoved { x, y } => {
                let previous = self.pointer;
                self.pointer = Some((x, y));

                let Some(button) = self.dragging else {
                    return;
                };
                let Some((last_x, last_y)) = previous else {
                    return;
                };
                let (dx, dy) = (x - last_x, y - last_y);

                match button {
                    PointerButton::Primary => {
                        // Dragging right turns the model to the right, which
                        // means swinging the camera the other way.
                        self.camera
                            .orbit(-dx * ORBIT_PER_PIXEL, dy * ORBIT_PER_PIXEL);
                    }
                    PointerButton::Middle | PointerButton::Secondary => {
                        // Screen coordinates grow downwards and the camera's up
                        // axis does not, so the vertical delta is negated once,
                        // here, where the two conventions meet.
                        self.camera.pan(-dx, dy);
                    }
                }
                self.redraw = true;
            }
            ViewportEvent::PointerPressed(button) => {
                if claimed_by_ui {
                    // The interface wanted this press, so no gesture begins and
                    // the release that follows will find nothing to end.
                    return;
                }
                self.dragging = Some(button);
            }
            ViewportEvent::PointerReleased(button) => {
                if self.dragging == Some(button) {
                    self.dragging = None;
                }
            }
            ViewportEvent::Wheel { delta } => {
                if claimed_by_ui {
                    return;
                }
                self.camera.zoom(delta * WHEEL_ZOOM);
                self.redraw = true;
            }
            ViewportEvent::Look(view) => {
                if claimed_by_ui {
                    return;
                }
                self.camera.look_from(view);
                self.redraw = true;
            }
        }
    }

    /// Asks for the next frame to be drawn.
    ///
    /// Used for anything this reducer did not cause: a model that finished
    /// rebuilding, or an interface that wants to animate.
    pub fn request_redraw(&mut self) {
        self.redraw = true;
    }

    /// Whether a frame is owed, clearing the request.
    ///
    /// Several requests between two frames collapse into one. A viewport that
    /// drew once per request would spend a drag rendering the same model at
    /// every intermediate position the pointer passed through, and a viewport
    /// that drew unconditionally would keep a laptop's fan on while nothing
    /// moved at all.
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> ViewportInput {
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(([-5.0, -5.0, -5.0], [5.0, 5.0, 5.0]))
            .expect("frames");
        // Clear the frame owed by construction so each test starts level.
        input.take_redraw();
        input
    }

    fn drag(input: &mut ViewportInput, button: PointerButton, claimed: bool) {
        input.handle(ViewportEvent::PointerMoved { x: 400.0, y: 300.0 }, claimed);
        input.handle(ViewportEvent::PointerPressed(button), claimed);
        input.handle(ViewportEvent::PointerMoved { x: 460.0, y: 340.0 }, claimed);
        input.handle(ViewportEvent::PointerReleased(button), claimed);
    }

    #[test]
    fn a_drag_orbits_and_a_wheel_zooms() {
        let mut input = ready();
        let before = *input.camera();

        drag(&mut input, PointerButton::Primary, false);
        assert_ne!(input.camera().eye(), before.eye(), "a drag did not orbit");
        assert!(
            (input.camera().distance() - before.distance()).abs() < 1e-3,
            "orbiting changed the distance"
        );
        assert!(input.take_redraw());

        let before = *input.camera();
        input.handle(ViewportEvent::Wheel { delta: 1.0 }, false);
        assert!(
            input.camera().distance() < before.distance(),
            "scrolling towards the model did not come closer"
        );
        assert!(input.take_redraw());
    }

    #[test]
    fn a_middle_drag_pans_rather_than_turning() {
        let mut input = ready();
        let before = *input.camera();

        drag(&mut input, PointerButton::Middle, false);

        // Panning moves what is looked at; orbiting does not.
        assert_ne!(
            input.camera().target(),
            before.target(),
            "panning moved nothing"
        );
        assert!(
            (input.camera().distance() - before.distance()).abs() < 1e-3,
            "panning changed the distance"
        );
    }

    #[test]
    fn an_event_the_interface_claimed_never_moves_the_camera() {
        let mut input = ready();
        let before = *input.camera();

        // A click on a panel, a drag across it, and a wheel over it.
        drag(&mut input, PointerButton::Primary, true);
        input.handle(ViewportEvent::Wheel { delta: 4.0 }, true);
        input.handle(ViewportEvent::Look(StandardView::Top), true);

        assert_eq!(
            *input.camera(),
            before,
            "the interface claimed these events and the camera moved anyway"
        );
        assert!(
            !input.take_redraw(),
            "nothing changed, so nothing was owed a frame"
        );
        assert!(!input.is_dragging(), "a claimed press started a gesture");
    }

    #[test]
    fn letting_go_of_a_panel_does_not_jump_the_model() {
        let mut input = ready();

        // The pointer crosses a panel, where every move is claimed, and then
        // comes back. A reducer that ignored claimed moves outright would
        // still believe the pointer was where it entered, and the first real
        // drag would swing the model by the width of the panel.
        input.handle(ViewportEvent::PointerMoved { x: 10.0, y: 300.0 }, true);
        input.handle(ViewportEvent::PointerMoved { x: 700.0, y: 300.0 }, true);

        let before = *input.camera();
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 710.0, y: 300.0 }, false);
        let orbited = *input.camera();

        // Ten pixels of drag, not seven hundred.
        let mut expected = before;
        expected.orbit(-10.0 * ORBIT_PER_PIXEL, 0.0);
        for axis in 0..3 {
            assert!(
                (orbited.eye()[axis] - expected.eye()[axis]).abs() < 1e-3,
                "the model jumped: {:?} instead of {:?}",
                orbited.eye(),
                expected.eye()
            );
        }
    }

    #[test]
    fn a_gesture_already_under_way_is_not_taken_over_by_the_interface() {
        let mut input = ready();

        input.handle(ViewportEvent::PointerMoved { x: 400.0, y: 300.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        let before = *input.camera();

        // The cursor crosses a panel mid-drag. Stopping dead there would be
        // the model refusing to follow the hand that is still holding it.
        input.handle(ViewportEvent::PointerMoved { x: 450.0, y: 300.0 }, true);
        assert_ne!(
            input.camera().eye(),
            before.eye(),
            "the drag stopped when the cursor crossed a panel"
        );
        assert!(input.is_dragging());
    }

    #[test]
    fn one_size_reaches_the_camera_and_the_surface() {
        let mut input = ViewportInput::new();

        // The camera and the surface must agree exactly: a camera with one
        // aspect ratio drawing into a surface with another stretches the
        // model. There is one call and one answer, so they cannot disagree.
        let size = input.resize(1024, 768);
        assert_eq!(size, (1024, 768));
        assert_eq!((input.camera().width(), input.camera().height()), size);

        // Including the sizes a window really produces when it is minimised.
        let size = input.resize(0, 0);
        assert_eq!(size, (0, 0));
        assert_eq!((input.camera().width(), input.camera().height()), size);
        assert!(!input.camera().is_drawable());
    }

    #[test]
    fn several_requests_between_frames_are_one_frame() {
        let mut input = ready();

        // A drag delivers a move per pointer position the hardware sampled.
        // Drawing once per event would render the same model at every
        // intermediate place the cursor passed through.
        input.handle(ViewportEvent::PointerMoved { x: 400.0, y: 300.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        for step in 1..20 {
            input.handle(
                ViewportEvent::PointerMoved {
                    x: 400.0 + step as f32,
                    y: 300.0,
                },
                false,
            );
        }
        input.request_redraw();
        input.request_redraw();

        assert!(input.take_redraw(), "a frame was owed and not offered");
        assert!(
            !input.take_redraw(),
            "one frame paid for every request that came before it"
        );
    }

    #[test]
    fn nothing_happening_owes_nothing() {
        let mut input = ready();

        // A viewport that redrew unconditionally would keep a fan running
        // while the user read the screen.
        input.handle(ViewportEvent::PointerMoved { x: 100.0, y: 100.0 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        assert!(!input.take_redraw(), "moving the cursor asked for a frame");
    }

    #[test]
    fn a_named_direction_turns_the_camera_and_asks_for_a_frame() {
        let mut input = ready();
        let before = *input.camera();

        input.handle(ViewportEvent::Look(StandardView::Top), false);
        assert_ne!(input.camera().eye(), before.eye());
        assert!(
            (input.camera().distance() - before.distance()).abs() < 1e-3,
            "a named view stepped back from the model"
        );
        assert!(input.take_redraw());
    }
}
