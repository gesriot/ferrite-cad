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

use ferritecad_types::{CadError, Result};
use ferritecad_viewport::{Camera, Projection, RenderSnapshot, StandardView};

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

/// What the pointer wants to know about what is under it.
///
/// Three answers rather than two. "Nothing new to ask" and "whatever was under
/// the pointer no longer is" are different situations: the first leaves what a
/// window already knows alone, and the second is the pointer leaving, a drag
/// starting or a panel taking the movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hover {
    /// No question, and nothing to forget.
    Unchanged,
    /// Whatever was under the pointer is not any more.
    Cleared,
    /// Ask the picture what is at this point.
    At(f32, f32),
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
    /// The window can no longer promise a matching release event.
    GestureCancelled,
    /// The pointer is no longer over the model at all.
    PointerLeft,
    /// Positive scrolls towards the model.
    Wheel {
        delta: f32,
    },
    /// Two fingers spreading or closing on a trackpad. Positive comes towards
    /// the model, and the number is already a magnification delta rather than
    /// a wheel's notches.
    Pinch {
        delta: f32,
    },
    /// Two fingers turning on a trackpad. Positive turns the world
    /// counterclockwise on screen, and the angle is in radians: whatever units
    /// a windowing system reports are converted before they reach here.
    Roll {
        radians: f32,
    },
    /// A named direction, from a key or a panel.
    Look(StandardView),
}

/// How far a pointer may wander between press and release and still be a click.
///
/// A hand on a mouse is not still. Zero would mean that anyone who moved by a
/// pixel while clicking selected nothing, and a large number would mean that
/// the end of a slow orbit selected whatever it happened to stop over.
const CLICK_SLOP: f32 = 4.0;

/// The camera, and what the user is in the middle of doing to it.
#[derive(Debug, Clone)]
pub struct ViewportInput {
    camera: Camera,
    pointer: Option<(f32, f32)>,
    dragging: Option<PointerButton>,
    /// Where an unclaimed primary press landed, while it might still become a
    /// click rather than a drag.
    pressed_at: Option<(f32, f32)>,
    /// Where the user asked what something is, until somebody answers.
    pick: Option<(f32, f32)>,
    /// What the pointer is asking about, until somebody answers. Overwritten
    /// rather than queued: a hand crossing the window asks about where it
    /// stopped, and answering every place it passed through would be a
    /// readback for each one.
    hover: Hover,
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
            pressed_at: None,
            pick: None,
            hover: Hover::Unchanged,
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

    /// Frames whatever the caller wants seen, if there is anything to see.
    ///
    /// One decision for every way of asking. A button and a key are two ways
    /// to say the same thing, and one part of a model and the whole of it are
    /// two things to say it about; a second copy of "where should the camera
    /// go" is the copy that would drift.
    ///
    /// `bounds` is `None` when there is nothing to show: nothing chosen, a
    /// definition that draws no triangles, a choice belonging to a picture no
    /// longer on screen, or a picture with nothing in it at all. They mean the
    /// same here, so the camera does not move and no frame is owed. Returns
    /// whether anything happened, which is what stops an unavailable action
    /// from asking for a redraw that would show the same picture again.
    ///
    /// Direction is kept. Framing answers "let me see this", not "look at it
    /// from somewhere I did not choose".
    pub fn frame_extent(&mut self, bounds: Option<([f32; 3], [f32; 3])>) -> Result<bool> {
        let Some(bounds) = bounds else {
            return Ok(false);
        };
        self.frame(bounds)?;
        Ok(true)
    }

    /// Applies one event.
    ///
    /// `claimed_by_ui` is what the interface said when asked whether it wanted
    /// this event. A claimed pointer event never moves the camera and never
    /// starts a gesture; it only keeps track of where the pointer is, so that
    /// letting go of a panel and dragging the model does not jump. Cancellation
    /// always ends a gesture because focus loss belongs to the window rather
    /// than to either region inside it.
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
                    // Nothing is being dragged, so a movement is a question
                    // about what it is over. A movement the interface wanted
                    // is a question about the interface, and the model under
                    // a panel was not being pointed at.
                    self.hover = if claimed_by_ui {
                        Hover::Cleared
                    } else {
                        Hover::At(x, y)
                    };
                    return;
                };

                // A gesture is under way: the pointer is moving the camera,
                // not asking about what it passes over.
                self.hover = Hover::Cleared;
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
                    // The interface wanted this press, so no gesture begins,
                    // the release that follows will find nothing to end, and
                    // nothing in the model was asked about.
                    return;
                }
                self.dragging = Some(button);
                // A press begins either a gesture or a click, and neither is a
                // question about what is merely under the pointer.
                self.hover = Hover::Cleared;
                if button == PointerButton::Primary {
                    self.pressed_at = self.pointer;
                }
            }
            ViewportEvent::PointerReleased(button) => {
                if self.dragging == Some(button) {
                    self.dragging = None;
                }
                if button != PointerButton::Primary {
                    return;
                }
                // A press and a release in nearly the same place is a question
                // about what is there. One that travelled was an orbit, and
                // answering it would select whatever the model stopped under.
                if let (Some((from_x, from_y)), Some((to_x, to_y))) =
                    (self.pressed_at.take(), self.pointer)
                    && (to_x - from_x).abs() <= CLICK_SLOP
                    && (to_y - from_y).abs() <= CLICK_SLOP
                {
                    self.pick = Some((from_x, from_y));
                    // The application answers picks while drawing a frame.
                    // Without a redraw request here, an ordinary click would
                    // be answered only if egui or the OS happened to ask for
                    // an unrelated frame afterwards.
                    self.redraw = true;
                }
            }
            ViewportEvent::PointerLeft => {
                // Away from the model entirely: there is nothing under the
                // pointer to say anything about.
                self.pointer = None;
                self.hover = Hover::Cleared;
            }
            ViewportEvent::GestureCancelled => {
                // Losing focus while a button is down need not be followed by
                // a release event. Forget both halves of the gesture so the
                // next move cannot continue a drag the user already ended or
                // jump from a position recorded in an earlier focus lifetime.
                self.dragging = None;
                self.pointer = None;
                self.pressed_at = None;
                self.hover = Hover::Cleared;
            }
            ViewportEvent::Wheel { delta } => {
                if claimed_by_ui {
                    return;
                }
                // A notch is a step, not a camera amount, so it is scaled into
                // one. A pinch already arrives as a magnification delta.
                self.zoom_by(delta * WHEEL_ZOOM);
            }
            ViewportEvent::Pinch { delta } => {
                if claimed_by_ui {
                    return;
                }
                self.zoom_by(delta);
            }
            ViewportEvent::Roll { radians } => {
                if claimed_by_ui {
                    return;
                }
                let before = self.camera;
                self.camera.roll(radians);
                self.settle_view(before);
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

    /// Zooms by an exponential amount, however the user asked for it.
    ///
    /// One rule for a wheel and for two fingers on a trackpad: where the zoom
    /// is anchored, what the camera will refuse, and what a change of view
    /// invalidates are properties of zooming and not of the device that asked
    /// for it. The devices differ only in what a unit of theirs is worth, and
    /// that conversion is made before this is called.
    ///
    /// `amount` is what the camera scale is multiplied by the exponential of,
    /// negated: positive comes towards the model.
    fn zoom_by(&mut self, amount: f32) {
        let before = self.camera;
        match self.pointer {
            // A zoom is aimed. What the user is pointing at is what they are
            // zooming towards, and a view that came closer to the middle of
            // the window instead would have to be dragged back afterwards.
            Some((x, y)) => {
                // Pixels from the middle, positive right and up. This is where
                // the window's idea of which way `y` grows meets the camera's,
                // the same as it does for a drag.
                let right = x - self.camera.width() as f32 * 0.5;
                let up = self.camera.height() as f32 * 0.5 - y;
                self.camera.zoom_at(amount, right, up);
            }
            // Nowhere to aim: the pointer has left, or a gesture was
            // cancelled, and the middle of the view is the only place left to
            // zoom about. A pointer over a panel is not this case; that event
            // belongs to the panel and never reaches here.
            None => self.camera.zoom(amount),
        }
        self.settle_view(before);
    }

    /// What a change of view invalidates, once it is known to have happened.
    ///
    /// One rule for every way of moving the camera without moving the mouse.
    /// A camera that did not move invalidates nothing: wound past its limit,
    /// or asked for a step too small to represent, nothing that was asked
    /// about has been answered differently and no frame is owed.
    ///
    /// When it did move, every pixel now shows something else, so a question
    /// about what was under the pointer, a click waiting to be answered, and a
    /// press that might still become one all belonged to a picture that is no
    /// longer on screen. The gesture itself is not one of them: a zoom or a
    /// turn during a drag is exactly that, and the button is still down.
    fn settle_view(&mut self, before: Camera) {
        if self.camera == before {
            return;
        }
        self.hover = Hover::Cleared;
        self.pick = None;
        self.pressed_at = None;
        self.redraw = true;
    }

    /// Takes what a finished load produced, and says what to do about it.
    ///
    /// A scene that arrived is pointed at: a document opened under the camera
    /// the last one left behind is usually off screen entirely, and a viewer
    /// that showed nothing while insisting it had loaded something would be
    /// worse than one that failed.
    ///
    /// A load that failed changes nothing – not the camera, not the frame that
    /// is owed. What is on screen stays on screen, because the alternative is
    /// going blank on a problem the user may well be able to fix, and losing
    /// the model they were looking at while they do.
    ///
    /// A successful load also ends any gesture or pick request begun in the
    /// previous scene. Window events and load answers share one queue, so a
    /// load may arrive between a press and its release, or after a click asked
    /// for a redraw but before that redraw answers it. Carrying either across
    /// the replacement would let an old click choose something in the new
    /// document immediately after Open cleared the selection.
    pub fn accept_load(&mut self, loaded: Result<RenderSnapshot>) -> Result<RenderSnapshot> {
        let snapshot = loaded?;
        // Stage the whole reducer, not only Camera::frame. Resetting the
        // projection is itself a camera change, and framing can still refuse
        // a finite picture whose combined extent exceeds f32. Such a refusal
        // leaves the old picture current, so its projection and pending
        // interactions have to remain current with it.
        let mut candidate = self.clone();
        // Frame first. Camera::frame establishes both the perspective distance
        // and its matching orthographic scale, so changing projection after it
        // cannot inherit an extreme scale from the previous document.
        if let Some(bounds) = snapshot.bounds() {
            candidate.camera.frame(bounds)?;
        }
        // A document is opened to be understood before it is measured, so it
        // arrives drawn the way an eye sees it, whatever the last one was left
        // in. The same reason every other transient state starts afresh here.
        if candidate.camera.projection_mode() != Projection::default()
            && !candidate.camera.set_projection(Projection::default())
        {
            return Err(CadError::input(
                "the camera cannot represent the default projection for this document",
            ));
        }
        candidate.forget_pending();
        *self = candidate;
        Ok(snapshot)
    }

    /// Forgets every gesture and question in flight, and asks to draw again.
    ///
    /// What both replacing the picture and changing its visibility need: a
    /// click recorded against the frame on screen would be answered against
    /// the one about to replace it, and a question about what the pointer was
    /// over would be answered about something that is no longer there or has
    /// only just returned.
    ///
    /// The camera is deliberately untouched. Forgetting a gesture is not a way
    /// to move, and the two callers differ in exactly that: one frames what
    /// arrived, the other must leave the view alone.
    pub fn forget_pending(&mut self) {
        self.dragging = None;
        self.pressed_at = None;
        self.pick = None;
        // A question about a picture nobody is looking at any more.
        self.hover = Hover::Cleared;
        self.redraw = true;
    }

    /// Draws through a different projection, and says whether that changed
    /// anything.
    ///
    /// The camera owns what a projection is and what switching one preserves;
    /// this only carries the request to it and reports what happened, so the
    /// window and the tests reach the same rule.
    pub fn set_projection(&mut self, projection: Projection) -> bool {
        self.camera.set_projection(projection)
    }

    /// Which projection the model is drawn through.
    pub fn projection(&self) -> Projection {
        self.camera.projection_mode()
    }

    /// Asks for the next frame to be drawn.
    ///
    /// Used for anything this reducer did not cause: a model that finished
    /// rebuilding, or an interface that wants to animate.
    pub fn request_redraw(&mut self) {
        self.redraw = true;
    }

    /// Takes the place the user asked about, if they asked.
    ///
    /// Cleared by the taking. Answering means drawing the model again offscreen
    /// to read one pixel of it, and a question that stayed asked would mean
    /// doing that for every frame after a click rather than once for the click.
    pub fn take_pick(&mut self) -> Option<(f32, f32)> {
        self.pick.take()
    }

    /// Takes what the pointer is asking about, if it is asking.
    ///
    /// Cleared by the taking, like the click question beside it. Answering
    /// means drawing the model again offscreen to read one pixel, so a
    /// question that stayed asked would mean doing that for every frame after
    /// the pointer stopped moving.
    pub fn take_hover(&mut self) -> Hover {
        std::mem::replace(&mut self.hover, Hover::Unchanged)
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
    fn a_wheel_zooms_towards_where_the_pointer_last_was() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 650.0, y: 120.0 }, false);
        // A later position is the one that counts: a hand that crossed the
        // window is zooming from where it stopped.
        input.handle(ViewportEvent::PointerMoved { x: 700.0, y: 90.0 }, false);
        let before = *input.camera();
        let anchor = target_plane_point(&before, 700.0, 90.0);

        input.handle(ViewportEvent::Wheel { delta: 3.0 }, false);

        let after = *input.camera();
        assert!(
            after.distance() < before.distance(),
            "scrolling towards the model did not come closer"
        );
        let (was, now) = (pixel_of(&before, anchor), pixel_of(&after, anchor));
        assert!(
            (was.0 - now.0).abs() <= 0.2 && (was.1 - now.1).abs() <= 0.2,
            "what was under the pointer at {was:?} moved to {now:?}"
        );
        assert!(
            (was.0 - 700.0).abs() < 0.2 && (was.1 - 90.0).abs() < 0.2,
            "the gate measured the wrong place: {was:?} is not where the pointer was"
        );
        assert!(input.take_redraw(), "a zoom did not ask to be drawn");
    }

    #[test]
    fn a_wheel_with_no_pointer_to_aim_zooms_about_the_middle() {
        for leaving in [ViewportEvent::PointerLeft, ViewportEvent::GestureCancelled] {
            let mut input = ready();
            input.handle(ViewportEvent::PointerMoved { x: 700.0, y: 90.0 }, false);
            input.handle(leaving, false);
            let before = *input.camera();

            input.handle(ViewportEvent::Wheel { delta: 3.0 }, false);

            let mut centred = before;
            centred.zoom(3.0 * WHEEL_ZOOM);
            assert_eq!(
                *input.camera(),
                centred,
                "after {leaving:?} the wheel aimed at something"
            );
        }
    }

    #[test]
    fn a_wheel_the_interface_claimed_changes_neither_the_camera_nor_what_is_waiting() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 300.0, y: 200.0 }, false);
        click(&mut input, (120.0, 80.0), false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        let before = *input.camera();
        let _ = input.take_redraw();

        input.handle(ViewportEvent::Wheel { delta: 5.0 }, true);

        assert_eq!(*input.camera(), before, "a claimed wheel moved the camera");
        assert_eq!(
            input.take_pick(),
            Some((120.0, 80.0)),
            "a claimed wheel forgot a click nobody had answered"
        );
        assert!(!input.take_redraw(), "a claimed wheel asked for a frame");
    }

    #[test]
    fn a_wheel_with_no_vertical_step_changes_nothing_that_is_waiting() {
        let mut input = ready();
        // A horizontal-only trackpad gesture reaches the reducer with a zero
        // vertical delta. Put the perspective eye at a pose whose direction
        // cannot be normalised and rebuilt bit-for-bit, which reproduced the
        // old one-ULP movement.
        for step in 1..=2 {
            input
                .camera
                .orbit(step as f32 * 0.0137, step as f32 * -0.0089);
            input.camera.pan(step as f32 * 0.17, step as f32 * -0.11);
            input
                .camera
                .zoom_at((step % 7) as f32 * 0.031 - 0.08, 173.0, -91.0);
        }
        click(&mut input, (120.0, 80.0), false);
        input.handle(ViewportEvent::PointerMoved { x: 240.0, y: 160.0 }, false);
        let before = *input.camera();
        let _ = input.take_redraw();

        input.handle(ViewportEvent::Wheel { delta: 0.0 }, false);

        assert_eq!(
            *input.camera(),
            before,
            "a zero wheel step moved the camera"
        );
        assert_eq!(
            input.take_hover(),
            Hover::At(240.0, 160.0),
            "a zero wheel step forgot the pending hover"
        );
        assert_eq!(
            input.take_pick(),
            Some((120.0, 80.0)),
            "a zero wheel step forgot the pending click"
        );
        assert!(!input.take_redraw(), "a zero wheel step asked for a frame");
    }

    #[test]
    fn a_zoom_forgets_what_was_asked_about_the_picture_it_replaced() {
        let mut input = ready();
        click(&mut input, (120.0, 80.0), false);
        input.handle(ViewportEvent::PointerMoved { x: 240.0, y: 160.0 }, false);
        // A press that has not yet decided whether it is a click or a drag.
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 244.0, y: 162.0 }, false);
        assert!(matches!(input.take_hover(), Hover::Cleared | Hover::At(..)));
        let _ = input.take_redraw();

        input.handle(ViewportEvent::Wheel { delta: 2.0 }, false);

        assert_eq!(input.take_hover(), Hover::Cleared, "a stale hover survived");
        assert_eq!(input.take_pick(), None, "a stale click survived");
        assert!(
            input.is_dragging(),
            "a wheel during a drag ended the drag the button is still holding"
        );
        assert!(input.take_redraw(), "a zoom did not ask to be drawn");

        // The press half is gone, so the release that follows answers nothing.
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        assert_eq!(
            input.take_pick(),
            None,
            "a press from before the zoom still chose something after it"
        );
    }

    #[test]
    fn several_wheels_before_a_frame_owe_one_frame() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 500.0, y: 400.0 }, false);
        for _ in 0..5 {
            input.handle(ViewportEvent::Wheel { delta: 1.0 }, false);
        }
        assert!(input.take_redraw(), "five notches owed no frame");
        assert!(!input.take_redraw(), "a flag counted instead of latching");
    }

    #[test]
    fn a_wheel_changes_neither_the_projection_nor_what_is_being_dragged() {
        let mut input = ready();
        assert!(input.set_projection(Projection::Orthographic));
        input.handle(ViewportEvent::PointerMoved { x: 620.0, y: 140.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Middle), false);
        let before = *input.camera();
        let anchor = target_plane_point(&before, 620.0, 140.0);

        input.handle(ViewportEvent::Wheel { delta: 2.0 }, false);

        assert_eq!(
            input.projection(),
            Projection::Orthographic,
            "a wheel changed how the model is drawn"
        );
        assert!(input.is_dragging(), "a wheel ended a pan in progress");
        let after = *input.camera();
        assert!(
            after.world_per_pixel() < before.world_per_pixel(),
            "the drawing did not change scale"
        );
        assert!(
            (after.distance() - before.distance()).abs() <= before.distance() * 1e-5,
            "a drawing's wheel moved the eye instead of changing the scale"
        );
        let (was, now) = (pixel_of(&before, anchor), pixel_of(&after, anchor));
        assert!(
            (was.0 - now.0).abs() <= 0.2 && (was.1 - now.1).abs() <= 0.2,
            "the drawing let {was:?} slide to {now:?}"
        );
    }

    /// Where a world point lands, in window pixels from the top left.
    fn pixel_of(camera: &Camera, point: [f32; 3]) -> (f32, f32) {
        let matrix = camera.view_projection();
        let clip = [
            matrix[0] * point[0] + matrix[4] * point[1] + matrix[8] * point[2] + matrix[12],
            matrix[1] * point[0] + matrix[5] * point[1] + matrix[9] * point[2] + matrix[13],
            matrix[3] * point[0] + matrix[7] * point[1] + matrix[11] * point[2] + matrix[15],
        ];
        assert!(clip[2] > 0.0, "a point behind the eye has no pixel");
        let (width, height) = (camera.width() as f32, camera.height() as f32);
        (
            (clip[0] / clip[2] + 1.0) * 0.5 * width,
            (1.0 - clip[1] / clip[2]) * 0.5 * height,
        )
    }

    /// The point on the target plane under a window pixel, found by the
    /// pixel-to-world scale rather than by the anchoring arithmetic.
    fn target_plane_point(camera: &Camera, x: f32, y: f32) -> [f32; 3] {
        let scale = camera.world_per_pixel();
        let (eye, target) = (camera.eye(), camera.target());
        let away = [eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]];
        let length = away[0].hypot(away[1]).hypot(away[2]);
        let away = [away[0] / length, away[1] / length, away[2] / length];
        // The camera's up is the world's, which is what every gesture here
        // leaves it as.
        let side = [-away[1], away[0], 0.0];
        let length = side[0].hypot(side[1]).hypot(side[2]);
        let side = [side[0] / length, side[1] / length, side[2] / length];
        let up = [
            away[1] * side[2] - away[2] * side[1],
            away[2] * side[0] - away[0] * side[2],
            away[0] * side[1] - away[1] * side[0],
        ];
        let right = (x - camera.width() as f32 * 0.5) * scale;
        let above = (camera.height() as f32 * 0.5 - y) * scale;
        [
            target[0] + side[0] * right + up[0] * above,
            target[1] + side[1] * right + up[1] * above,
            target[2] + side[2] * right + up[2] * above,
        ]
    }

    #[test]
    fn two_fingers_spreading_come_closer_and_closing_go_away_by_exactly_that_much() {
        for delta in [0.3f32, -0.3, 0.05, -1.2] {
            let mut input = ready();
            let before = input.camera().world_per_pixel();

            input.handle(ViewportEvent::Pinch { delta }, false);

            let after = input.camera().world_per_pixel();
            // A pinch is already a magnification delta. Nothing scales it on
            // the way in.
            assert!(
                (after / before - (-delta).exp()).abs() < 1e-4,
                "a pinch of {delta} changed the scale by {} where it means {}",
                after / before,
                (-delta).exp()
            );
            assert_eq!(
                delta > 0.0,
                after < before,
                "a pinch of {delta} went the wrong way"
            );
        }
    }

    #[test]
    fn a_pinch_and_a_wheel_ask_the_camera_for_the_same_thing() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 610.0, y: 130.0 }, false);
        let mut directly = *input.camera();

        input.handle(ViewportEvent::Pinch { delta: 0.37 }, false);

        // The one operation, asked for by hand at the one place a pointer
        // becomes camera axes.
        directly.zoom_at(0.37, 610.0 - 400.0, 300.0 - 130.0);
        assert_eq!(
            *input.camera(),
            directly,
            "a pinch reached the camera differently from a zoom"
        );
    }

    #[test]
    fn a_pinch_keeps_what_it_was_pointed_at_under_the_pointer() {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            for delta in [0.4f32, -0.4] {
                let mut input = ready();
                assert!(
                    projection == Projection::Perspective || input.set_projection(projection),
                    "the camera refused a projection to zoom in"
                );
                // Somewhere with no symmetry to hide a mistake in.
                input.handle(ViewportEvent::PointerMoved { x: 660.0, y: 110.0 }, false);
                let before = *input.camera();
                let anchor = target_plane_point(&before, 660.0, 110.0);

                input.handle(ViewportEvent::Pinch { delta }, false);

                let after = *input.camera();
                let (was, now) = (pixel_of(&before, anchor), pixel_of(&after, anchor));
                assert!(
                    (was.0 - now.0).abs() <= 0.2 && (was.1 - now.1).abs() <= 0.2,
                    "{projection:?} pinch of {delta}: {was:?} slid to {now:?}"
                );
                assert!(
                    (was.0 - 660.0).abs() < 0.2 && (was.1 - 110.0).abs() < 0.2,
                    "the gate measured the wrong place: {was:?}"
                );
                // The picture really changed, so holding everything still
                // could not pass.
                assert!(
                    (after.world_per_pixel() - before.world_per_pixel()).abs()
                        > before.world_per_pixel() * 0.1,
                    "{projection:?}: the scale did not change"
                );
                if projection == Projection::Orthographic {
                    assert!(
                        (after.distance() - before.distance()).abs() <= before.distance() * 1e-5,
                        "a pinch in a drawing moved the eye instead of changing the scale"
                    );
                }
            }
        }
    }

    #[test]
    fn a_pinch_aims_at_the_latest_pointer_and_at_the_middle_when_there_is_none() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 120.0, y: 500.0 }, false);
        input.handle(ViewportEvent::PointerMoved { x: 700.0, y: 90.0 }, false);
        let mut aimed = *input.camera();
        aimed.zoom_at(0.3, 700.0 - 400.0, 300.0 - 90.0);

        input.handle(ViewportEvent::Pinch { delta: 0.3 }, false);
        assert_eq!(
            *input.camera(),
            aimed,
            "a pinch aimed at somewhere the pointer had already left"
        );

        for leaving in [ViewportEvent::PointerLeft, ViewportEvent::GestureCancelled] {
            let mut input = ready();
            input.handle(ViewportEvent::PointerMoved { x: 700.0, y: 90.0 }, false);
            input.handle(leaving, false);
            let mut centred = *input.camera();
            centred.zoom(0.3);

            input.handle(ViewportEvent::Pinch { delta: 0.3 }, false);
            assert_eq!(
                *input.camera(),
                centred,
                "after {leaving:?} a pinch aimed at something"
            );
        }
    }

    #[test]
    fn a_pinch_the_interface_claimed_changes_nothing_at_all() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 300.0, y: 200.0 }, false);
        click(&mut input, (120.0, 80.0), false);
        // A second press has not yet become either a click or a drag. A pinch
        // the interface owns must not decide that model interaction for it.
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        let before = *input.camera();
        let pressed_before = input.pressed_at;
        let _ = input.take_redraw();

        input.handle(ViewportEvent::Pinch { delta: 0.6 }, true);

        assert_eq!(*input.camera(), before, "a claimed pinch moved the camera");
        assert_eq!(
            input.take_pick(),
            Some((120.0, 80.0)),
            "a claimed pinch forgot a click nobody had answered"
        );
        assert_eq!(
            input.pressed_at, pressed_before,
            "a claimed pinch forgot a press that belonged to the model"
        );
        assert!(
            input.is_dragging(),
            "a claimed pinch ended the model's press"
        );
        assert!(!input.take_redraw(), "a claimed pinch asked for a frame");
    }

    #[test]
    fn a_pinch_that_moved_the_view_forgets_the_questions_but_not_the_gesture() {
        let mut input = ready();
        assert!(input.set_projection(Projection::Orthographic));
        click(&mut input, (120.0, 80.0), false);
        input.handle(ViewportEvent::PointerMoved { x: 240.0, y: 160.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Middle), false);
        input.handle(ViewportEvent::PointerMoved { x: 244.0, y: 162.0 }, false);
        let _ = input.take_redraw();

        input.handle(ViewportEvent::Pinch { delta: 0.35 }, false);

        assert_eq!(input.take_hover(), Hover::Cleared, "a stale hover survived");
        assert_eq!(input.take_pick(), None, "a stale click survived");
        assert!(
            input.is_dragging(),
            "a pinch ended a drag whose button is still down"
        );
        assert_eq!(
            input.projection(),
            Projection::Orthographic,
            "a pinch changed how the model is drawn"
        );
        assert!(input.take_redraw(), "a pinch did not ask to be drawn");

        // And the press half is gone, so the release answers nothing.
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        let _ = input.take_redraw();
        input.handle(ViewportEvent::Pinch { delta: 0.35 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        assert_eq!(
            input.take_pick(),
            None,
            "a press from before the pinch still chose something after it"
        );
    }

    #[test]
    fn a_pinch_that_cannot_move_the_view_leaves_everything_exactly_as_it_was() {
        // The phases a trackpad reports around a real gesture carry no
        // magnification at all, and neither does a device that reports
        // nonsense. None of them is a camera operation.
        for delta in [0.0f32, f32::NAN, f32::INFINITY, 1e-30, -1e-30] {
            let mut input = ready();
            // A camera at a pose whose direction cannot be normalised and
            // rebuilt bit for bit, which is where an accidental movement of
            // one ULP would show.
            input.camera.orbit(0.0137, -0.0089);
            input.camera.pan(0.17, -0.11);
            input.camera.zoom_at(0.031, 173.0, -91.0);
            click(&mut input, (120.0, 80.0), false);
            input.handle(ViewportEvent::PointerMoved { x: 240.0, y: 160.0 }, false);
            let before = *input.camera();
            let _ = input.take_redraw();

            input.handle(ViewportEvent::Pinch { delta }, false);

            assert_eq!(
                *input.camera(),
                before,
                "a pinch of {delta} moved the camera"
            );
            assert_eq!(
                input.take_hover(),
                Hover::At(240.0, 160.0),
                "a pinch of {delta} forgot the pending hover"
            );
            assert_eq!(
                input.take_pick(),
                Some((120.0, 80.0)),
                "a pinch of {delta} forgot the pending click"
            );
            assert!(!input.take_redraw(), "a pinch of {delta} asked for a frame");

            // The other half of a click cannot coexist with a pending hover
            // in a real reducer state: pressing clears hover. Exercise it on
            // its own reducer rather than manufacturing an impossible state.
            let mut pressed = ready();
            pressed.handle(ViewportEvent::PointerMoved { x: 310.0, y: 210.0 }, false);
            pressed.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
            let camera_before = *pressed.camera();
            let pressed_before = pressed.pressed_at;
            let _ = pressed.take_redraw();

            pressed.handle(ViewportEvent::Pinch { delta }, false);

            assert_eq!(
                *pressed.camera(),
                camera_before,
                "a pinch of {delta} moved the camera during a pending press"
            );
            assert_eq!(
                pressed.pressed_at, pressed_before,
                "a pinch of {delta} forgot a pending press"
            );
            assert!(pressed.is_dragging(), "a pinch of {delta} ended a press");
            assert!(
                !pressed.take_redraw(),
                "a pinch of {delta} redrew a pending press"
            );
        }
    }

    #[test]
    fn several_pinch_updates_before_a_frame_owe_one_frame() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 500.0, y: 400.0 }, false);
        for _ in 0..6 {
            input.handle(ViewportEvent::Pinch { delta: 0.05 }, false);
        }
        assert!(input.take_redraw(), "six pinch updates owed no frame");
        assert!(!input.take_redraw(), "a flag counted instead of latching");
    }

    #[test]
    fn a_pinch_wound_past_the_limit_stops_asking_for_frames() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 500.0, y: 400.0 }, false);
        for _ in 0..200 {
            input.handle(ViewportEvent::Pinch { delta: 1.0 }, false);
        }
        click(&mut input, (120.0, 80.0), false);
        input.handle(ViewportEvent::PointerMoved { x: 500.0, y: 400.0 }, false);
        let before = *input.camera();
        let _ = input.take_redraw();

        input.handle(ViewportEvent::Pinch { delta: 1.0 }, false);

        assert_eq!(*input.camera(), before, "a clamped pinch moved the camera");
        assert_eq!(
            input.take_hover(),
            Hover::At(500.0, 400.0),
            "a clamped pinch forgot the pending hover"
        );
        assert_eq!(
            input.take_pick(),
            Some((120.0, 80.0)),
            "a clamped pinch forgot the pending click"
        );
        assert!(!input.take_redraw(), "a clamped pinch asked for a frame");
    }

    #[test]
    fn a_turn_reaches_the_camera_as_the_camera_turns() {
        let mut input = ready();
        let mut directly = *input.camera();

        input.handle(ViewportEvent::Roll { radians: 0.43 }, false);

        directly.roll(0.43);
        assert_eq!(
            *input.camera(),
            directly,
            "a turn reached the camera differently from a roll"
        );
        assert!(input.take_redraw(), "a turn did not ask to be drawn");
    }

    #[test]
    fn a_turn_that_moved_the_view_forgets_the_questions_but_not_the_gesture() {
        let mut input = ready();
        assert!(input.set_projection(Projection::Orthographic));
        click(&mut input, (120.0, 80.0), false);
        input.handle(ViewportEvent::PointerMoved { x: 240.0, y: 160.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Middle), false);
        input.handle(ViewportEvent::PointerMoved { x: 244.0, y: 162.0 }, false);
        let target = input.camera().target();
        let _ = input.take_redraw();

        input.handle(ViewportEvent::Roll { radians: 0.5 }, false);

        assert_eq!(input.take_hover(), Hover::Cleared, "a stale hover survived");
        assert_eq!(input.take_pick(), None, "a stale click survived");
        assert!(
            input.is_dragging(),
            "a turn ended a drag whose button is still down"
        );
        assert_eq!(
            input.projection(),
            Projection::Orthographic,
            "a turn changed how the model is drawn"
        );
        assert_eq!(
            input.camera().target(),
            target,
            "a turn moved what is being looked at"
        );
        assert!(input.take_redraw(), "a turn did not ask to be drawn");
    }

    #[test]
    fn a_turn_that_changes_nothing_leaves_every_waiting_thing_alone() {
        // Claimed by the interface, and the angles that cannot move a basis.
        let cases: [(f32, bool); 6] = [
            (0.6, true),
            (0.0, false),
            (f32::NAN, false),
            (f32::INFINITY, false),
            (-f32::INFINITY, false),
            (1e-30, false),
        ];
        for (radians, claimed) in cases {
            let mut input = ready();
            input.camera.orbit(0.0137, -0.0089);
            input.camera.pan(0.17, -0.11);
            click(&mut input, (120.0, 80.0), false);
            input.handle(ViewportEvent::PointerMoved { x: 240.0, y: 160.0 }, false);
            // A press that has not yet decided what it is.
            input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
            input.handle(ViewportEvent::PointerMoved { x: 242.0, y: 161.0 }, false);
            let before = *input.camera();
            let dragging = input.is_dragging();
            let _ = input.take_redraw();
            let _ = input.take_hover();

            input.handle(ViewportEvent::Roll { radians }, claimed);

            assert_eq!(
                *input.camera(),
                before,
                "a turn of {radians} (claimed {claimed}) moved the camera"
            );
            assert_eq!(
                input.take_pick(),
                Some((120.0, 80.0)),
                "a turn of {radians} forgot the pending click"
            );
            assert_eq!(
                input.take_hover(),
                Hover::Unchanged,
                "a turn of {radians} answered a question nobody asked"
            );
            assert_eq!(
                input.is_dragging(),
                dragging,
                "a turn of {radians} ended the gesture"
            );
            assert!(
                !input.take_redraw(),
                "a turn of {radians} asked for a frame"
            );

            // The press half is still there, so a release still chooses.
            input.handle(
                ViewportEvent::PointerReleased(PointerButton::Primary),
                false,
            );
            assert_eq!(
                input.take_pick(),
                Some((240.0, 160.0)),
                "a turn of {radians} forgot a press that was still waiting"
            );
        }
    }

    #[test]
    fn several_turns_before_a_frame_owe_one_frame() {
        let mut input = ready();
        for _ in 0..7 {
            input.handle(ViewportEvent::Roll { radians: 0.02 }, false);
        }
        assert!(input.take_redraw(), "seven turns owed no frame");
        assert!(!input.take_redraw(), "a flag counted instead of latching");
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
    fn losing_focus_ends_a_gesture_without_moving_the_camera() {
        let mut input = ready();

        input.handle(ViewportEvent::PointerMoved { x: 400.0, y: 300.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        let before = *input.camera();

        input.handle(ViewportEvent::GestureCancelled, false);
        input.handle(ViewportEvent::PointerMoved { x: 700.0, y: 500.0 }, false);

        assert!(!input.is_dragging(), "focus loss left a gesture running");
        assert_eq!(
            *input.camera(),
            before,
            "a move after focus returned continued the abandoned gesture"
        );
        assert!(
            !input.take_redraw(),
            "ending a gesture changed no pixels and owed no frame"
        );
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

    /// One triangle somewhere far from where the camera is now looking.
    fn distant_scene() -> RenderSnapshot {
        use ferritecad_kernel::{
            Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeHandle, SubShapeKind,
        };
        use ferritecad_types::Transform;

        let mut mesh = Mesh::default();
        mesh.positions.extend_from_slice(&[
            900.0, 900.0, 900.0, 910.0, 900.0, 900.0, 900.0, 910.0, 900.0,
        ]);
        mesh.normals
            .extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        mesh.indices.extend_from_slice(&[0, 1, 2]);
        mesh.faces.push(MeshFaceRange {
            face: SubShapeHandle::new(ShapeHandle::new(SessionId::new(), 1), SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 3,
        });

        let mut builder = ferritecad_viewport::SnapshotBuilder::new();
        let definition = builder.add_mesh(&mesh).expect("the mesh is valid");
        builder
            .place(definition, None, &Transform::IDENTITY, [0.5, 0.5, 0.5])
            .expect("places it");
        builder.build()
    }

    /// A picture whose vertices are finite and placeable, but whose combined
    /// extent is wider than the camera's number format can represent.
    fn unframeable_scene() -> RenderSnapshot {
        use ferritecad_kernel::{
            Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeHandle, SubShapeKind,
        };
        use ferritecad_types::Transform;

        let shape = ShapeHandle::new(SessionId::new(), 2);
        let mesh = Mesh {
            positions: vec![-f32::MAX, 0.0, 0.0, f32::MAX, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
        };
        let mut builder = ferritecad_viewport::SnapshotBuilder::new();
        let definition = builder.add_mesh(&mesh).expect("finite mesh packs");
        builder
            .place(definition, None, &Transform::IDENTITY, [0.5, 0.5, 0.5])
            .expect("finite vertices are placeable");
        builder.build()
    }

    #[test]
    fn a_loaded_scene_is_pointed_at_and_asks_for_a_frame() {
        let mut input = ready();
        let before = *input.camera();

        let snapshot = input
            .accept_load(Ok(distant_scene()))
            .expect("a scene that loaded is a scene to show");
        assert_eq!(snapshot.draws().len(), 1, "the scene was not handed back");
        assert_ne!(
            input.camera().target(),
            before.target(),
            "the camera stayed where the previous document left it"
        );
        assert!(input.take_redraw(), "a new model did not ask to be drawn");
    }

    #[test]
    fn a_loaded_scene_drops_interaction_with_the_previous_one() {
        let mut input = ready();

        // A complete click whose requested redraw has not answered it yet.
        click(&mut input, (120.0, 80.0), false);
        assert_eq!(
            input.clone().take_pick(),
            Some((120.0, 80.0)),
            "the gate began without a pending pick"
        );
        input
            .accept_load(Ok(distant_scene()))
            .expect("the replacement loads");
        assert_eq!(
            input.take_pick(),
            None,
            "a pick requested from the old scene reached its replacement"
        );

        // And a press whose release arrives only after the replacement. It is
        // not a click in either picture, because its two halves saw different
        // documents.
        input.handle(ViewportEvent::PointerMoved { x: 200.0, y: 120.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        assert!(input.is_dragging(), "the gate began without a gesture");
        input
            .accept_load(Ok(distant_scene()))
            .expect("another replacement loads");
        assert!(
            !input.is_dragging(),
            "the old scene's gesture survived Open"
        );
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        assert_eq!(
            input.take_pick(),
            None,
            "a press in the old scene and release in the new scene became a click"
        );
    }

    #[test]
    fn a_load_that_failed_leaves_the_picture_alone() {
        let mut input = ready();
        let before = *input.camera();

        // Unlike a successful replacement, an answer that changes no scene
        // must not discard a question about the scene that remains current.
        click(&mut input, (120.0, 80.0), false);
        assert!(
            input.take_redraw(),
            "the click asked for no answering frame"
        );

        let error = input
            .accept_load(Err(ferritecad_types::CadError::input("no such document")))
            .expect_err("a failed load must not produce a scene");
        assert!(error.to_string().contains("no such document"));

        // Nothing moved and no frame is owed, so whatever was drawn is still
        // what is on screen.
        assert_eq!(input.camera().eye(), before.eye());
        assert_eq!(input.camera().target(), before.target());
        assert_eq!(input.take_pick(), Some((120.0, 80.0)));
        assert!(
            !input.take_redraw(),
            "a failed load asked for a frame that would draw the same picture"
        );
    }

    #[test]
    fn a_scene_the_camera_cannot_frame_changes_none_of_the_old_view() {
        let mut input = ready();
        assert!(input.set_projection(Projection::Orthographic));
        let _ = input.take_redraw();

        // A click and a hover question belonging to the picture that remains
        // current if accepting its replacement fails.
        click(&mut input, (120.0, 80.0), false);
        input.handle(ViewportEvent::PointerMoved { x: 40.0, y: 50.0 }, false);
        let before = *input.camera();

        let error = input
            .accept_load(Ok(unframeable_scene()))
            .expect_err("an extent wider than f32 cannot be framed");
        assert!(error.to_string().contains("extent exceeds"));

        assert_eq!(*input.camera(), before, "a failed frame changed the camera");
        assert_eq!(input.take_pick(), Some((120.0, 80.0)));
        assert_eq!(input.take_hover(), Hover::At(40.0, 50.0));
        assert!(
            input.take_redraw(),
            "the old picture's pending redraw was discarded"
        );
    }

    #[test]
    fn an_empty_document_does_not_move_the_camera_nowhere() {
        let mut input = ready();
        let before = *input.camera();

        // A document with no geometry has no extent to point at. Framing it
        // would have to invent one, and a camera at an invented distance from
        // nothing is worse than one left where it was.
        let empty = ferritecad_viewport::SnapshotBuilder::new().build();
        input.accept_load(Ok(empty)).expect("an empty scene loads");
        assert_eq!(input.camera().eye(), before.eye());
        assert_eq!(input.camera().target(), before.target());
    }

    /// Presses and releases at one place, which is what a click is.
    fn click(input: &mut ViewportInput, at: (f32, f32), claimed: bool) {
        input.handle(ViewportEvent::PointerMoved { x: at.0, y: at.1 }, claimed);
        input.handle(
            ViewportEvent::PointerPressed(PointerButton::Primary),
            claimed,
        );
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            claimed,
        );
    }

    #[test]
    fn moving_over_the_model_asks_what_is_under_the_pointer() {
        let mut input = ready();

        input.handle(ViewportEvent::PointerMoved { x: 40.0, y: 50.0 }, false);
        assert_eq!(input.take_hover(), Hover::At(40.0, 50.0));

        // Taken once. Answering means drawing the model again to read a pixel,
        // and a question left standing would mean doing that for every frame
        // after the pointer stopped.
        assert_eq!(input.take_hover(), Hover::Unchanged);
    }

    #[test]
    fn a_hand_crossing_the_window_asks_about_where_it_stopped() {
        let mut input = ready();

        // A pointer reports every place it passed through. Asking about each
        // one would be a readback for each one, and every answer but the last
        // would be about somewhere the pointer no longer is.
        for step in 0..20u8 {
            input.handle(
                ViewportEvent::PointerMoved {
                    x: 10.0 * f32::from(step),
                    y: 30.0,
                },
                false,
            );
        }
        assert_eq!(input.take_hover(), Hover::At(190.0, 30.0));
        assert_eq!(input.take_hover(), Hover::Unchanged);
    }

    #[test]
    fn nothing_under_the_pointer_is_asked_about_while_it_is_busy_elsewhere() {
        let mut input = ready();

        // The interface wanted this movement, so the question is about the
        // interface. What is behind a panel was not being pointed at.
        input.handle(ViewportEvent::PointerMoved { x: 20.0, y: 8.0 }, true);
        assert_eq!(input.take_hover(), Hover::Cleared);

        // A drag is moving the camera, not asking about what it passes over.
        input.handle(ViewportEvent::PointerMoved { x: 60.0, y: 60.0 }, false);
        assert_eq!(input.take_hover(), Hover::At(60.0, 60.0));
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        assert_eq!(input.take_hover(), Hover::Cleared);
        for step in 0..5u8 {
            input.handle(
                ViewportEvent::PointerMoved {
                    x: 60.0 + 10.0 * f32::from(step),
                    y: 90.0,
                },
                false,
            );
            assert_eq!(
                input.take_hover(),
                Hover::Cleared,
                "a drag asked what it was passing over"
            );
        }

        // Losing the window, and leaving the model, both end it.
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 70.0, y: 70.0 }, false);
        input.handle(ViewportEvent::GestureCancelled, false);
        assert_eq!(input.take_hover(), Hover::Cleared);

        input.handle(ViewportEvent::PointerMoved { x: 70.0, y: 70.0 }, false);
        input.handle(ViewportEvent::PointerLeft, false);
        assert_eq!(input.take_hover(), Hover::Cleared);
    }

    #[test]
    fn a_loaded_scene_forgets_what_was_under_the_pointer_too() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 30.0, y: 30.0 }, false);

        input
            .accept_load(Ok(distant_scene()))
            .expect("a scene that loaded is a scene to show");

        // The question was about the document that has just been replaced, and
        // its answer would name a definition of a picture nobody is looking at.
        assert_eq!(input.take_hover(), Hover::Cleared);
    }

    #[test]
    fn a_click_asks_what_is_there_and_asks_only_once() {
        let mut input = ready();
        click(&mut input, (120.0, 80.0), false);

        assert!(
            input.take_redraw(),
            "the click queued a question but no frame in which to answer it"
        );
        assert_eq!(input.take_pick(), Some((120.0, 80.0)));
        // Answering means drawing the model again to read one pixel. A
        // question that stayed asked would mean doing that every frame from
        // then on.
        assert_eq!(input.take_pick(), None, "one click asked twice");
    }

    #[test]
    fn a_drag_is_not_a_question_about_what_is_under_the_pointer() {
        let mut input = ready();
        input.handle(ViewportEvent::PointerMoved { x: 100.0, y: 100.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 160.0, y: 140.0 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );

        assert_eq!(
            input.take_pick(),
            None,
            "an orbit selected whatever it stopped over"
        );

        // A hand is not still, so a press and release a pixel apart is still a
        // click rather than the shortest drag anyone ever made.
        input.handle(ViewportEvent::PointerMoved { x: 200.0, y: 200.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 201.0, y: 199.0 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        assert_eq!(input.take_pick(), Some((200.0, 200.0)));
    }

    #[test]
    fn pressing_a_panel_asks_nothing_of_the_model() {
        let mut input = ready();

        // The interface wanted this press. The model behind the panel is not
        // what the user was pointing at, and selecting it would be selecting
        // by accident.
        click(&mut input, (30.0, 12.0), true);
        assert_eq!(input.take_pick(), None, "a panel press selected the model");

        // Losing the window mid-click is not a click either: the release may
        // never arrive, and the one after it belongs to another gesture.
        input.handle(ViewportEvent::PointerMoved { x: 60.0, y: 60.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::GestureCancelled, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        assert_eq!(input.take_pick(), None);
    }

    #[test]
    fn showing_what_is_chosen_keeps_the_direction_and_asks_for_one_frame() {
        let mut input = ready();
        let before = *input.camera();

        // Somewhere else entirely, which is the case this exists for: the
        // chosen definition is off screen.
        let happened = input
            .frame_extent(Some(([900.0, 900.0, 900.0], [910.0, 910.0, 910.0])))
            .expect("a box with an extent can be framed");
        assert!(happened, "framing reported that nothing happened");
        assert!(
            input.take_redraw(),
            "the camera moved and no frame followed"
        );
        assert!(
            !input.take_redraw(),
            "one framing asked for more than one frame"
        );

        // The camera went there and is still looking the way it was: framing
        // answers "let me see this", not "look at it from somewhere else".
        assert_ne!(input.camera().target(), before.target());
        let direction = |camera: &ferritecad_viewport::Camera| {
            let (eye, target) = (camera.eye(), camera.target());
            let away = [eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]];
            let length = (away[0] * away[0] + away[1] * away[1] + away[2] * away[2]).sqrt();
            [away[0] / length, away[1] / length, away[2] / length]
        };
        let (was, now) = (direction(&before), direction(input.camera()));
        for axis in 0..3 {
            assert!(
                (was[axis] - now[axis]).abs() < 1e-3,
                "the viewing direction turned: {was:?} to {now:?}"
            );
        }
    }

    #[test]
    fn nowhere_to_go_is_not_a_reason_to_move() {
        let mut input = ready();
        let before = *input.camera();

        // Nothing chosen, a definition that draws nothing, and a choice made
        // in a picture that has been replaced all arrive here as the same
        // answer: there is nowhere to go.
        for _ in 0..3 {
            assert!(
                !input
                    .frame_extent(None)
                    .expect("having nowhere to go is not a failure")
            );
        }
        assert_eq!(*input.camera(), before);
        assert!(
            !input.take_redraw(),
            "an action that did nothing asked for a frame anyway"
        );
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
