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
use ferritecad_viewport::{Camera, RenderSnapshot, StandardView};

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
    /// The window can no longer promise a matching release event.
    GestureCancelled,
    /// Positive scrolls towards the model.
    Wheel {
        delta: f32,
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
                    // The interface wanted this press, so no gesture begins,
                    // the release that follows will find nothing to end, and
                    // nothing in the model was asked about.
                    return;
                }
                self.dragging = Some(button);
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
            ViewportEvent::GestureCancelled => {
                // Losing focus while a button is down need not be followed by
                // a release event. Forget both halves of the gesture so the
                // next move cannot continue a drag the user already ended or
                // jump from a position recorded in an earlier focus lifetime.
                self.dragging = None;
                self.pointer = None;
                self.pressed_at = None;
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
        if let Some(bounds) = snapshot.bounds() {
            self.camera.frame(bounds)?;
        }
        self.dragging = None;
        self.pressed_at = None;
        self.pick = None;
        self.redraw = true;
        Ok(snapshot)
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
