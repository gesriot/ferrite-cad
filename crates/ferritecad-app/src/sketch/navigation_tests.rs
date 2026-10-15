// SPDX-License-Identifier: MIT
//! §31J: zoom and pan of the Line, Circle and annulus canvases through the production
//! forms and real egui events (no window, no GPU, no kernel). A view is moved by wheel,
//! pinch, a held middle button or Shift + primary, and by nothing else; none of it may
//! edit a string, a history, a request or a selection.
#![allow(clippy::panic)]
use super::{
    analytic::Handle,
    analytic_tests::{self as analytic_t, context, open_new, saved},
    drag_tests,
    tests::{click, frame_running, text_at},
    *,
};

/// The six forms that hold a canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    LineNew,
    LineSaved,
    CircleNew,
    CircleSaved,
    AnnulusNew,
    AnnulusSaved,
}

const FORMS: [Form; 6] = [
    Form::LineNew,
    Form::LineSaved,
    Form::CircleNew,
    Form::CircleSaved,
    Form::AnnulusNew,
    Form::AnnulusSaved,
];

/// One form in one egui context, driven only by events.
struct Rig {
    form: Form,
    ctx: egui::Context,
    e: Editor,
    _root: Option<tempfile::TempDir>,
}

fn at(x: f32, y: f32) -> egui::Pos2 {
    egui::pos2(x, y)
}

fn moved(to: egui::Pos2) -> egui::Event {
    egui::Event::PointerMoved(to)
}

fn button(pos: egui::Pos2, button: egui::PointerButton, pressed: bool, shift: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: if shift {
            egui::Modifiers::SHIFT
        } else {
            Default::default()
        },
    }
}

fn middle(pos: egui::Pos2, pressed: bool) -> egui::Event {
    button(pos, egui::PointerButton::Middle, pressed, false)
}

fn primary(pos: egui::Pos2, pressed: bool) -> egui::Event {
    button(pos, egui::PointerButton::Primary, pressed, false)
}

fn shift_primary(pos: egui::Pos2, pressed: bool) -> egui::Event {
    button(pos, egui::PointerButton::Primary, pressed, true)
}

fn wheel_lines(y: f32) -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0., y),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    }
}

fn wheel_points(y: f32) -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0., y),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    }
}

fn escape() -> egui::Event {
    drag_tests::key(egui::Key::Escape)
}

impl Rig {
    fn open(form: Form, pixels_per_point: f32) -> Self {
        let ctx = context();
        let mut root = None;
        let mut e = Editor::default();
        match form {
            Form::LineNew => {
                e.begin();
                let draft = e.draft.as_mut().expect("draft");
                draft.points = [["0", "0"], ["60", "0"], ["60", "20"]]
                    .map(|p| p.map(str::to_owned))
                    .to_vec();
                draft.closed = false;
            }
            Form::LineSaved => {
                let (dir, path, reading) = crate::fillets::tests::plate();
                e.set_session(true, true, false);
                assert!(e.begin_edit(&path, &reading, reading.sketches[0].sketch));
                root = Some(dir);
            }
            Form::CircleNew => open_new(&ctx, &mut e, "Circle"),
            Form::AnnulusNew => open_new(&ctx, &mut e, "Circle with hole"),
            Form::CircleSaved => {
                let (dir, _ctx, saved_editor) = saved(&[10.5]);
                e = saved_editor;
                root = Some(dir);
            }
            Form::AnnulusSaved => {
                let (dir, _ctx, saved_editor) = saved(&[10.5, 4.75]);
                e = saved_editor;
                root = Some(dir);
            }
        }
        ctx.set_pixels_per_point(pixels_per_point);
        let mut rig = Self {
            form,
            ctx,
            e,
            _root: root,
        };
        for _ in 0..3 {
            rig.frame(vec![]);
        }
        rig
    }

    fn line(&self) -> bool {
        matches!(self.form, Form::LineNew | Form::LineSaved)
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        frame_running(&self.ctx, &mut self.e, events, false)
    }

    /// A frame as the window runs it with the focus and a job's state given.
    fn frame_as(
        &mut self,
        events: Vec<egui::Event>,
        running: bool,
        focused: bool,
        cover: Option<egui::Rect>,
    ) -> egui::FullOutput {
        let e = &mut self.e;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                events,
                focused,
                ..Default::default()
            },
            |ui| {
                e.draw(ui, true, running, "");
                if let Some(cover) = cover {
                    egui::Area::new(egui::Id::new("over the canvas"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(cover.min)
                        .show(ui.ctx(), |ui| {
                            ui.allocate_exact_size(cover.size(), egui::Sense::click_and_drag())
                        });
                }
            },
        );
        output.textures_delta.clear();
        output
    }

    fn rect(&mut self) -> egui::Rect {
        any_canvas(&self.frame(vec![]))
    }

    /// Lower-left corner and points per mm.
    fn view(&self) -> ([f64; 2], f32) {
        match self.form {
            Form::LineNew | Form::LineSaved => (self.e.canvas.minimum, self.e.canvas.scale),
            Form::CircleNew | Form::CircleSaved => {
                let v = self.e.circle_canvas.view.expect("a fitted view");
                (v.minimum, v.scale)
            }
            Form::AnnulusNew | Form::AnnulusSaved => {
                let v = self.e.annulus_canvas.view.expect("a fitted view");
                (v.minimum, v.scale)
            }
        }
    }

    /// The document point under the screen point `p` in the view now.
    fn under(&mut self, p: egui::Pos2) -> [f64; 2] {
        let rect = self.rect();
        let (minimum, scale) = self.view();
        canvas_mm(rect, minimum, scale, p)
    }

    /// A point of the canvas that is no vertex and no handle.
    fn free(&mut self) -> egui::Pos2 {
        self.rect().left_top() + egui::vec2(60., 50.)
    }

    /// The screen point of a vertex (Line) or the outer handle (Circle, annulus).
    fn grab(&mut self) -> egui::Pos2 {
        match self.form {
            Form::LineNew | Form::LineSaved => drag_tests::vertex(&self.ctx, &mut self.e, 1),
            _ => analytic_t::handle(&self.ctx, &mut self.e, Handle::Outer),
        }
    }

    /// The screen point of vertex `i` (Line) or of the centre (0) / outer (1) handle.
    fn grab_at(&mut self, i: usize) -> egui::Pos2 {
        match self.form {
            Form::LineNew | Form::LineSaved => drag_tests::vertex(&self.ctx, &mut self.e, i),
            _ => analytic_t::handle(
                &self.ctx,
                &mut self.e,
                if i == 0 {
                    Handle::Centre
                } else {
                    Handle::Outer
                },
            ),
        }
    }

    /// Depth of the shown draft's Undo and Redo.
    fn depth(&self) -> (usize, usize) {
        match self.form {
            Form::LineNew | Form::LineSaved => (self.e.undo.len(), self.e.redo.len()),
            _ => analytic_t::depth(&self.e),
        }
    }

    /// An edit gesture (a vertex or a handle) is held.
    fn editing(&self) -> bool {
        match self.form {
            Form::LineNew | Form::LineSaved => self.e.canvas.gesture.is_some(),
            Form::CircleNew | Form::CircleSaved => self.e.circle_canvas.held().is_some(),
            Form::AnnulusNew | Form::AnnulusSaved => self.e.annulus_canvas.held().is_some(),
        }
    }

    /// Everything an edit could touch: strings, histories, selection, requests.
    fn facts(&self) -> String {
        self.facts_with(true)
    }

    /// The same; a press on a vertex selects it, and Escape does not unselect it.
    fn facts_with(&self, selection: bool) -> String {
        let e = &self.e;
        if self.line() {
            format!(
                "{:?}",
                (
                    &e.draft,
                    &e.next,
                    &e.undo,
                    &e.redo,
                    e.canvas.selected.filter(|_| selection),
                    e.canvas.snap,
                    e.canvas.object_snap,
                    &e.canvas.length,
                    e.pending.is_some(),
                    e.pending_edit.is_some(),
                    e.pending_apply.is_some(),
                    e.editing.is_some(),
                )
            )
        } else {
            format!(
                "{:?}",
                (
                    (&e.circle, &e.circle_undo, &e.circle_redo, &e.circle_applied),
                    (
                        &e.annulus,
                        &e.annulus_undo,
                        &e.annulus_redo,
                        &e.annulus_applied,
                    ),
                    e.mode,
                    e.pending.is_some(),
                    (
                        e.pending_apply_circle.is_some(),
                        e.pending_circle_edit.is_some(),
                        e.pending_apply_annulus.is_some(),
                        e.pending_annulus_edit.is_some(),
                    ),
                )
            )
        }
    }

    /// How the form paints its publication and history buttons: their text and the
    /// opacity egui gives them, which tells enabled from disabled.
    fn look(&mut self) -> Vec<(String, u32)> {
        let out = self.frame(vec![]);
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t)
                    if [
                        "Create",
                        "Apply",
                        "Save",
                        "Undo draft",
                        "Redo draft",
                        "Confirm",
                    ]
                    .iter()
                    .any(|p| t.galley.text().starts_with(p)) =>
                {
                    Some((t.galley.text().to_owned(), t.opacity_factor.to_bits()))
                }
                _ => None,
            })
            .collect()
    }

    fn pending_request(&self) -> bool {
        let e = &self.e;
        e.pending.is_some()
            || e.pending_edit.is_some()
            || e.pending_apply.is_some()
            || e.pending_apply_circle.is_some()
            || e.pending_circle_edit.is_some()
            || e.pending_apply_annulus.is_some()
            || e.pending_annulus_edit.is_some()
    }

    /// The form's own request, asked for and not taken: a job is queued.
    fn queue(&mut self) {
        match self.form {
            Form::LineNew => {
                self.e.pending = Some(NewDocument::SketchExtrude(
                    PolygonExtrusion::new(vec![[0., 0.], [60., 0.], [60., 20.]], 10.)
                        .expect("a triangle"),
                ));
            }
            Form::LineSaved => {
                self.e.pending_edit = Some(self.e.edit_request().expect("a request"));
            }
            Form::CircleNew => self.press_label(CREATE_CIRCLE),
            Form::AnnulusNew => self.press_label(CREATE_ANNULUS),
            // Apply is offered for a draft that differs from the saved numbers.
            Form::CircleSaved => {
                analytic_t::type_into(&self.ctx, &mut self.e, "Radius", "11");
                self.press_label("Apply circle");
            }
            Form::AnnulusSaved => {
                analytic_t::type_into(&self.ctx, &mut self.e, "Outer radius", "11");
                self.press_label("Apply annulus");
            }
        }
        assert!(self.pending_request(), "{:?} queued nothing", self.form);
    }

    fn press_label(&mut self, label: &str) {
        let out = self.frame(vec![]);
        click(&self.ctx, &mut self.e, text_at(&out, label));
    }
}

/// The painted canvas of whichever form is shown: 510 points wide, 250 (Line) or 230
/// (Circle, annulus) high.
fn any_canvas(out: &egui::FullOutput) -> egui::Rect {
    out.shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Rect(r)
                if (r.rect.width() - 510.).abs() < 1.
                    && ((r.rect.height() - 250.).abs() < 1.
                        || (r.rect.height() - 230.).abs() < 1.) =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .expect("painted canvas")
}

/// Two views are the same to the bit.
fn same(a: ([f64; 2], f32), b: ([f64; 2], f32)) -> bool {
    a.0.map(f64::to_bits) == b.0.map(f64::to_bits) && a.1.to_bits() == b.1.to_bits()
}

/// Two views are the same up to the rounding of a zoom out and in again.
fn close(a: ([f64; 2], f32), b: ([f64; 2], f32)) -> bool {
    near(a.0, b.0) && (f64::from(a.1) - f64::from(b.1)).abs() <= 1e-5 * f64::from(b.1)
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (0..2).all(|i| (a[i] - b[i]).abs() <= 1e-9 * (1. + a[i].abs()))
}

#[test]
fn wheel_and_pinch_zoom_about_the_cursor_on_every_canvas_and_edit_nothing() {
    // A trackpad may carry real movement on its first or last event too.
    for form in FORMS {
        for phase in [
            egui::TouchPhase::Start,
            egui::TouchPhase::Move,
            egui::TouchPhase::End,
        ] {
            let mut r = Rig::open(form, 1.);
            let p = r.free();
            let before = r.view();
            r.frame(vec![moved(p)]);
            r.frame(vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0., 20.),
                phase,
                modifiers: Default::default(),
            }]);
            assert!(
                r.view().1 > before.1,
                "{form:?}: the {phase:?} wheel delta was dropped"
            );
        }
    }
    for form in FORMS {
        for pixels_per_point in [1., 2.] {
            let mut r = Rig::open(form, pixels_per_point);
            let facts = r.facts();
            let look = r.look();
            let opening = r.view();
            // Half-point positions: a whole physical pixel at 2x.
            let p = r.free() + egui::vec2(100.5, 40.5);
            let q = r.free() + egui::vec2(-20., 120.);
            r.frame(vec![moved(p)]);
            let anchor = r.under(p);
            // One line of wheel is a step of e^0.2 about the cursor.
            r.frame(vec![wheel_lines(1.)]);
            let (_, scale) = r.view();
            let expected = f64::from(opening.1) * 0.2_f64.exp();
            assert!(
                (f64::from(scale) - expected).abs() <= 1e-5 * expected,
                "{form:?} at {pixels_per_point}x: {scale} against {expected}"
            );
            assert!(
                near(anchor, r.under(p)),
                "{form:?} moved the point under the cursor"
            );
            // Points are points: 40 of them are one line, the same step, any display.
            r.frame(vec![wheel_points(40.)]);
            assert!(near(anchor, r.under(p)));
            let two = f64::from(r.view().1);
            assert!((two / f64::from(scale) - 0.2_f64.exp()).abs() < 1e-4);
            // A pinch is its own factor, about wherever the pointer is.
            r.frame(vec![moved(q)]);
            let other = r.under(q);
            r.frame(vec![egui::Event::Zoom(1.5)]);
            assert!(near(other, r.under(q)), "{form:?} pinch");
            assert!((f64::from(r.view().1) / two - 1.5).abs() < 1e-4);
            // And out again; the view is bounded and the document point stays.
            r.frame(vec![wheel_lines(-3.)]);
            assert!(near(other, r.under(q)));
            assert!(r.view().1.is_finite() && r.view().1 > 0.);
            // Two events in one frame are two steps, in order.
            let before = r.view().1;
            r.frame(vec![wheel_lines(1.), egui::Event::Zoom(0.5)]);
            let after = r.view().1;
            assert!((f64::from(after) / f64::from(before) - 0.2_f64.exp() * 0.5).abs() < 1e-4);
            assert!(near(other, r.under(q)));
            // Nothing was edited, and nothing was started or held.
            assert_eq!(r.facts(), facts, "{form:?} at {pixels_per_point}x");
            assert_eq!(r.look(), look, "{form:?}: availability changed");
            assert!(!r.e.gesturing() && !r.pending_request());
            assert_ne!(r.view().1, opening.1);
        }
    }
    println!("\nFCAD_31J_ZOOM_WIDGETS_EXECUTED");
}

#[test]
fn a_pan_follows_the_pointer_holds_the_window_and_edits_nothing() {
    for form in FORMS {
        for pixels_per_point in [1., 2.] {
            let mut r = Rig::open(form, pixels_per_point);
            let facts = r.facts();
            let look = r.look();
            // Middle button, then Shift + primary: the same pan.
            for shifted in [false, true] {
                let start = r.view();
                let p = r.free();
                r.frame(vec![moved(p)]);
                let grabbed = r.under(p);
                r.frame(vec![if shifted {
                    shift_primary(p, true)
                } else {
                    middle(p, true)
                }]);
                assert!(r.e.gesturing(), "{form:?}: a held pan holds the window");
                assert!(!r.editing());
                let to = p + egui::vec2(45.5, -20.5);
                r.frame(vec![moved(p + egui::vec2(10., 0.))]);
                r.frame(vec![moved(to)]);
                // The pointer moved by (45.5, -20.5) points: the view moved by that.
                let (minimum, scale) = r.view();
                assert_eq!(scale, start.1, "a pan does not zoom");
                assert!(
                    near(
                        minimum,
                        [
                            start.0[0] - 45.5 / f64::from(scale),
                            start.0[1] - 20.5 / f64::from(scale)
                        ]
                    ),
                    "{form:?}: {minimum:?} from {:?}",
                    start.0
                );
                assert!(
                    near(grabbed, r.under(to)),
                    "{form:?}: the grabbed point left"
                );
                assert_eq!(r.facts(), facts, "{form:?}: a held pan edited");
                assert_eq!(
                    r.look(),
                    look,
                    "{form:?}: a held pan changed what can be pressed"
                );
                r.frame(vec![if shifted {
                    shift_primary(to, false)
                } else {
                    middle(to, false)
                }]);
                assert!(
                    !r.e.gesturing(),
                    "{form:?}: a released pan holds the window"
                );
                assert!(near(grabbed, r.under(to)));
                // The release ended it: the pointer's next move pans nothing.
                let kept = r.view();
                r.frame(vec![moved(to + egui::vec2(30., 30.))]);
                assert!(same(r.view(), kept));
                r.frame(vec![moved(p)]);
            }
            assert_eq!(r.facts(), facts, "{form:?} at {pixels_per_point}x");
            assert_eq!(r.look(), look);
            assert!(!r.pending_request());
        }
    }
    println!("\nFCAD_31J_PAN_WIDGETS_EXECUTED");
}

/// One drag of a vertex or a handle, the grid, object snap and guides on for the Line,
/// with or without wheel, pinch and a middle-button pan attempted while it is held. The
/// pointer moves are the same in both runs, so only those events differ.
fn held_drag(form: Form, navigate: bool, cancel: bool) -> (String, String, ([f64; 2], f32)) {
    let mut r = Rig::open(form, 1.);
    if r.line() {
        for label in ["1 mm", "Object snap", "Orthogonal"] {
            r.press_label(label);
        }
    }
    let start = r.view();
    let initial = r.facts_with(false);
    let grab = r.grab();
    let p = r.free();
    r.frame(vec![moved(grab)]);
    r.frame(vec![primary(grab, true)]);
    assert!(r.editing(), "{form:?}: no gesture");
    // What a Line gesture froze at its press.
    let frozen = |r: &Rig| {
        r.line().then(|| {
            let g = r.e.canvas.gesture.as_ref().expect("held");
            format!(
                "{:?}",
                (
                    g.scale,
                    g.start,
                    g.snap,
                    g.objects.as_ref().map(Vec::len),
                    g.guide.is_some()
                )
            )
        })
    };
    let at_press = frozen(&r);
    r.frame(vec![moved(grab + egui::vec2(7., 3.))]);
    let steps: [Vec<egui::Event>; 4] = [
        vec![moved(p), wheel_lines(2.)],
        vec![egui::Event::Zoom(3.)],
        vec![middle(p, true), moved(p + egui::vec2(40., 25.))],
        vec![middle(p + egui::vec2(40., 25.), false), wheel_points(-120.)],
    ];
    for events in steps {
        // The control sends the same pointer moves and none of the rest.
        let kept: Vec<egui::Event> = if navigate {
            events
        } else {
            events
                .into_iter()
                .filter(|e| matches!(e, egui::Event::PointerMoved(_)))
                .collect()
        };
        r.frame(kept);
        assert!(
            same(r.view(), start),
            "{form:?}: the view moved while a gesture held it"
        );
        assert!(r.editing(), "{form:?}: navigation ended the gesture");
        assert_eq!(
            frozen(&r),
            at_press,
            "{form:?}: what the press froze changed"
        );
    }
    let end = grab + egui::vec2(31., 12.);
    r.frame(vec![moved(end)]);
    if cancel {
        r.frame(vec![escape()]);
        assert!(!r.editing(), "{form:?}: Escape left the gesture");
    }
    // The press is released in both runs; Escape only cancelled the preview.
    r.frame(vec![primary(end, false)]);
    assert!(!r.editing() && !r.e.gesturing());
    assert!(same(r.view(), start));
    (initial, r.facts_with(false), r.view())
}

#[test]
fn a_pan_that_does_not_move_is_no_click_and_places_or_selects_nothing_on_the_line_canvas() {
    for form in [Form::LineNew, Form::LineSaved] {
        let mut r = Rig::open(form, 1.);
        let facts = r.facts();
        let empty = r.free();
        let vertex = r.grab();
        for (what, spot) in [("empty canvas", empty), ("a vertex", vertex)] {
            // Shift + primary pressed and released where it stands: egui calls that a
            // click; the canvas must not. In separate frames and in one.
            r.frame(vec![moved(spot)]);
            r.frame(vec![shift_primary(spot, true)]);
            assert!(r.e.gesturing(), "{form:?} on {what}: no pan held");
            r.frame(vec![shift_primary(spot, false)]);
            assert!(!r.e.gesturing());
            assert_eq!(
                r.facts(),
                facts,
                "{form:?} on {what}: a Shift press acted as a click"
            );
            r.frame(vec![shift_primary(spot, true), shift_primary(spot, false)]);
            assert_eq!(r.facts(), facts, "{form:?} on {what}: one frame");
            // Escape ends the pan; the release that follows it is still no click.
            r.frame(vec![shift_primary(spot, true)]);
            r.frame(vec![escape()]);
            assert!(!r.e.gesturing());
            r.frame(vec![shift_primary(spot, false)]);
            assert_eq!(r.facts(), facts, "{form:?} on {what}: after Escape");
            // The same with the middle button, which egui never calls a primary click.
            r.frame(vec![middle(spot, true)]);
            r.frame(vec![middle(spot, false)]);
            assert_eq!(r.facts(), facts, "{form:?} on {what}: middle");
        }
        // The control: a plain click on the vertex still selects it.
        r.frame(vec![moved(vertex)]);
        r.frame(vec![primary(vertex, true)]);
        r.frame(vec![primary(vertex, false)]);
        assert_ne!(r.facts(), facts, "{form:?}: a plain click does nothing");
    }
    println!("\nFCAD_31J_PAN_CLICK_EXECUTED");
}

#[test]
fn a_held_edit_gesture_freezes_the_view_and_what_the_press_froze() {
    for form in FORMS {
        let control = held_drag(form, false, false);
        let navigated = held_drag(form, true, false);
        assert_eq!(navigated, control, "{form:?}: navigation changed the drag");
        assert_ne!(control.0, control.1, "{form:?}: the drag edited nothing");
        // One drag, one Undo step, whatever was attempted while it was held.
        let cancelled = held_drag(form, true, true);
        assert_eq!(cancelled.0, cancelled.1, "{form:?}: Escape did not restore");
    }
    // One step of Undo, then Redo: the same numbers, and the view untouched by both.
    for form in FORMS {
        let mut r = Rig::open(form, 1.);
        let start = r.view();
        let grab = r.grab();
        let p = r.free();
        r.frame(vec![moved(grab)]);
        r.frame(vec![primary(grab, true)]);
        r.frame(vec![moved(grab + egui::vec2(20., 0.)), wheel_lines(4.)]);
        r.frame(vec![primary(grab + egui::vec2(20., 0.), false)]);
        assert_eq!(r.depth().0, 1, "{form:?}: one drag, one step");
        let edited = r.facts();
        r.frame(vec![moved(p), wheel_lines(1.)]);
        assert!(
            !same(r.view(), start),
            "{form:?}: the wheel works again after the drag"
        );
        assert_eq!(r.facts(), edited, "{form:?}: a wheel after the drag edited");
        assert_eq!(r.depth().0, 1);
    }
    println!("\nFCAD_31J_FROZEN_EXECUTED");
}

#[test]
fn navigation_reaches_only_the_visible_uncovered_canvas_of_a_form_that_is_free() {
    for form in FORMS {
        let mut r = Rig::open(form, 1.);
        let facts = r.facts();
        let start = r.view();
        let p = r.free();
        let inside_not_covered = p + egui::vec2(200., 60.);
        let covered_at = p;
        let cover = egui::Rect::from_center_size(covered_at, egui::vec2(60., 60.));

        // A job runs: the form is disabled; nothing takes the wheel, nothing pans.
        r.frame_as(vec![moved(p)], true, true, None);
        r.frame_as(
            vec![wheel_lines(2.), egui::Event::Zoom(2.)],
            true,
            true,
            None,
        );
        r.frame_as(
            vec![middle(p, true), moved(p + egui::vec2(30., 5.))],
            true,
            true,
            None,
        );
        assert!(!r.e.gesturing(), "{form:?}: a busy form panned");
        r.frame_as(
            vec![middle(p + egui::vec2(30., 5.), false)],
            true,
            true,
            None,
        );
        assert!(
            same(r.view(), start),
            "{form:?}: a busy form moved its view"
        );

        // The window has lost the focus.
        r.frame_as(vec![moved(p), wheel_lines(2.)], false, false, None);
        r.frame_as(vec![shift_primary(p, true)], false, false, None);
        assert!(
            !r.e.gesturing() && same(r.view(), start),
            "{form:?}: unfocused"
        );
        r.frame_as(vec![shift_primary(p, false)], false, true, None);

        // Outside the canvas: beside it, below it, away from the form.
        let rect = r.rect();
        for outside in [
            at(rect.left() - 4., p.y),
            at(rect.right() + 4., p.y),
            at(rect.center().x, rect.bottom() + 6.),
            at(990., 890.),
        ] {
            r.frame(vec![moved(outside)]);
            r.frame(vec![wheel_lines(2.), egui::Event::Zoom(2.)]);
            r.frame(vec![middle(outside, true)]);
            assert!(!r.e.gesturing(), "{form:?}: a press at {outside:?} panned");
            r.frame(vec![
                moved(outside + egui::vec2(0., -30.)),
                middle(outside, false),
            ]);
            assert!(
                same(r.view(), start),
                "{form:?}: {outside:?} moved the view"
            );
        }

        // Another window over a part of the canvas takes the wheel and the press there.
        for _ in 0..2 {
            r.frame_as(vec![], false, true, Some(cover));
        }
        r.frame_as(vec![moved(covered_at)], false, true, Some(cover));
        r.frame_as(vec![wheel_lines(2.)], false, true, Some(cover));
        r.frame_as(vec![middle(covered_at, true)], false, true, Some(cover));
        assert!(
            !r.e.gesturing(),
            "{form:?}: a press through another window panned"
        );
        r.frame_as(vec![middle(covered_at, false)], false, true, Some(cover));
        assert!(
            same(r.view(), start),
            "{form:?}: the view moved under another window"
        );
        // The part of the same canvas that is not covered still answers.
        r.frame_as(vec![moved(inside_not_covered)], false, true, Some(cover));
        r.frame_as(vec![wheel_lines(2.)], false, true, Some(cover));
        assert!(
            !same(r.view(), start),
            "{form:?}: the uncovered canvas is dead"
        );
        let moved_view = r.view();

        assert_eq!(
            r.facts(),
            facts,
            "{form:?}: unavailable navigation edited the draft before a request was queued"
        );

        // A request waiting to be taken: the same, until it is taken.
        r.queue();
        let before_refused_navigation = r.facts();
        r.frame(vec![moved(p)]);
        r.frame(vec![wheel_lines(-2.), egui::Event::Zoom(0.5)]);
        r.frame(vec![middle(p, true), moved(p + egui::vec2(30., 5.))]);
        assert!(!r.e.gesturing(), "{form:?}: a queued form panned");
        r.frame(vec![middle(p + egui::vec2(30., 5.), false)]);
        let grabbed = r.grab();
        let canvas = r.rect();
        assert!(
            canvas.contains(p),
            "the queued-navigation probe must be on the canvas"
        );
        // A vertex outside a navigated canvas may lie on another form widget. It
        // is not a navigation target there; always probe the visible canvas point.
        for spot in [p, grabbed]
            .into_iter()
            .filter(|spot| canvas.contains(*spot))
        {
            r.frame(vec![shift_primary(spot, true), shift_primary(spot, false)]);
            r.frame(vec![shift_primary(spot, true)]);
            r.frame(vec![moved(spot + egui::vec2(12., 7.))]);
            r.frame(vec![shift_primary(spot + egui::vec2(12., 7.), false)]);
        }
        assert!(
            same(r.view(), moved_view),
            "{form:?}: a queued form moved its view"
        );

        assert_eq!(
            r.facts(),
            before_refused_navigation,
            "{form:?}: queued navigation changed the draft"
        );

        // Nothing of all that edited anything (the request was the form's own).
        assert!(r.pending_request());
        let queued = r.facts();
        assert_ne!(queued, facts);
        r.frame(vec![wheel_lines(1.)]);
        assert_eq!(r.facts(), queued, "{form:?}: refused navigation edited");
    }
    println!("\nFCAD_31J_REFUSED_EXECUTED");
}

#[test]
fn a_pan_ends_without_a_trace_by_release_outside_escape_focus_pointer_gone_or_a_busy_form() {
    for form in FORMS {
        let mut r = Rig::open(form, 1.);
        let facts = r.facts();
        let p = r.free();
        let far = at(900., 700.);
        let begin = |r: &mut Rig| {
            let start = r.view();
            r.frame(vec![moved(p)]);
            r.frame(vec![middle(p, true), moved(p + egui::vec2(25., 15.))]);
            assert!(r.e.gesturing(), "{form:?}");
            start
        };

        // A release far outside the canvas still ends it, where the pointer is.
        let start = begin(&mut r);
        r.frame(vec![moved(far)]);
        let held = r.view();
        assert!(!same(held, start));
        r.frame(vec![middle(far, false)]);
        assert!(!r.e.gesturing(), "{form:?}: a release outside left a pan");
        assert!(!same(r.view(), start));

        // Escape puts the view back to the press and ends the pan.
        let start = begin(&mut r);
        r.frame(vec![escape()]);
        assert!(
            !r.e.gesturing() && same(r.view(), start),
            "{form:?}: Escape"
        );
        r.frame(vec![moved(p + egui::vec2(50., 50.)), middle(p, false)]);
        assert!(same(r.view(), start), "{form:?}: the pan came back");

        // The window loses the focus: the pan is over where it was.
        begin(&mut r);
        let held = r.view();
        r.frame(vec![egui::Event::WindowFocused(false)]);
        assert!(
            !r.e.gesturing() && same(r.view(), held),
            "{form:?}: focus loss"
        );
        r.frame_as(
            vec![middle(p, false), egui::Event::WindowFocused(true)],
            false,
            true,
            None,
        );

        // The pointer is gone: the same.
        begin(&mut r);
        let held = r.view();
        r.frame(vec![egui::Event::PointerGone]);
        assert!(
            !r.e.gesturing() && same(r.view(), held),
            "{form:?}: PointerGone"
        );
        r.frame(vec![moved(p), middle(p, false)]);

        // A job starts under a held pan: the pan stops, and the view stays.
        begin(&mut r);
        let held = r.view();
        r.frame_as(vec![moved(p + egui::vec2(40., 40.))], true, true, None);
        assert!(!r.e.gesturing(), "{form:?}: a busy form kept a pan");
        assert!(same(r.view(), held));
        r.frame(vec![middle(p, false)]);
        r.frame(vec![moved(p + egui::vec2(60., 60.))]);
        assert!(same(r.view(), held), "{form:?}: a stopped pan went on");

        // A second press during a pan is neither an edit nor a second pan.
        let start = begin(&mut r);
        let at_grab = r.under(p + egui::vec2(25., 15.));
        r.frame(vec![primary(p, true)]);
        assert!(!r.editing(), "{form:?}: a press began an edit during a pan");
        r.frame(vec![primary(p, false), moved(p + egui::vec2(25., 15.))]);
        r.frame(vec![middle(p + egui::vec2(25., 15.), false)]);
        assert!(!r.e.gesturing() && !same(r.view(), start));
        assert!(near(at_grab, r.under(p + egui::vec2(25., 15.))));
        assert_eq!(
            r.facts(),
            facts,
            "{form:?}: a cancelled or ended pan edited"
        );
    }
    println!("\nFCAD_31J_PAN_ENDS_EXECUTED");
}

#[test]
fn events_of_one_frame_keep_their_order_and_gestures_read_the_view_the_person_saw() {
    for form in FORMS {
        let a = |r: &mut Rig| r.grab_at(1);
        let b = |r: &mut Rig| r.grab_at(0);
        // Control: the same drag in one frame with no navigation.
        let control = {
            let mut r = Rig::open(form, 1.);
            let (g, _) = (a(&mut r), ());
            r.frame(vec![moved(g)]);
            r.frame(vec![
                primary(g, true),
                moved(g + egui::vec2(20., 0.)),
                primary(g + egui::vec2(20., 0.), false),
            ]);
            (r.facts(), r.view())
        };
        // 1. A wheel while the gesture is held is ignored: nothing changes.
        let mut r = Rig::open(form, 1.);
        let start = r.view();
        let g = a(&mut r);
        r.frame(vec![moved(g)]);
        r.frame(vec![
            primary(g, true),
            moved(g + egui::vec2(20., 0.)),
            wheel_lines(3.),
            egui::Event::Zoom(2.),
            primary(g + egui::vec2(20., 0.), false),
        ]);
        assert_eq!(
            (r.facts(), r.view()),
            control,
            "{form:?}: a wheel inside a gesture"
        );
        assert!(same(r.view(), start));

        // 2. A wheel after the release applies, about the pointer; the step stays one.
        let mut r = Rig::open(form, 1.);
        let g = a(&mut r);
        r.frame(vec![moved(g)]);
        let anchor = r.under(g + egui::vec2(20., 0.));
        r.frame(vec![
            primary(g, true),
            moved(g + egui::vec2(20., 0.)),
            primary(g + egui::vec2(20., 0.), false),
            wheel_lines(2.),
        ]);
        assert_eq!(r.facts(), control.0, "{form:?}: the drag differs");
        assert!(!same(r.view(), start));
        assert!(
            near(anchor, r.under(g + egui::vec2(20., 0.))),
            "{form:?}: anchor"
        );
        assert_eq!(r.depth().0, 1);

        // 3. A wheel before a press and a whole drag in the same frame: the press reads
        //    the view the person saw, the wheel takes effect at the end of the frame.
        let mut r = Rig::open(form, 1.);
        let g = a(&mut r);
        let p = r.free();
        r.frame(vec![moved(p)]);
        r.frame(vec![
            wheel_lines(2.),
            moved(g),
            primary(g, true),
            moved(g + egui::vec2(20., 0.)),
            primary(g + egui::vec2(20., 0.), false),
        ]);
        assert_eq!(
            r.facts(),
            control.0,
            "{form:?}: the drag read the zoomed view"
        );
        assert!(!same(r.view(), start), "{form:?}: the wheel was lost");

        // 4. A wheel before a press that is still held at the end of the frame is dropped:
        //    a held gesture's view does not change.
        let mut r = Rig::open(form, 1.);
        let g = a(&mut r);
        let p = r.free();
        r.frame(vec![moved(p)]);
        r.frame(vec![wheel_lines(2.), moved(g), primary(g, true)]);
        assert!(r.editing());
        assert!(
            same(r.view(), start),
            "{form:?}: a view changed under a held press"
        );
        r.frame(vec![primary(g, false)]);
        assert_eq!(r.depth().0, 0, "{form:?}: a press alone made a step");

        // 5. The release of one gesture, the press of the next and a wheel, in one frame:
        //    the first is one step, the second is held, unconfirmed, and nothing zooms.
        let mut r = Rig::open(form, 1.);
        let first = a(&mut r);
        let second = b(&mut r);
        r.frame(vec![moved(first)]);
        r.frame(vec![
            primary(first, true),
            moved(first + egui::vec2(20., 0.)),
        ]);
        r.frame(vec![
            primary(first + egui::vec2(20., 0.), false),
            moved(second),
            primary(second, true),
            wheel_lines(3.),
            moved(second + egui::vec2(9., 4.)),
        ]);
        assert!(r.editing(), "{form:?}: the next gesture is not held");
        assert!(
            same(r.view(), start),
            "{form:?}: zoomed under the next gesture"
        );
        assert_eq!(r.depth().0, 1, "{form:?}: the first gesture is one step");
        let after_first = r.facts();
        r.frame(vec![escape()]);
        assert!(!r.editing());
        assert_eq!(r.depth().0, 1, "{form:?}: Escape touched the history");
        assert_ne!(
            r.facts(),
            after_first,
            "{form:?}: Escape restored the first gesture's strings"
        );
        // (Escape restores the strings of the second press, which are the first's result.)
        assert!(same(r.view(), start));

        // 6. A whole pan and a wheel in one frame: the pan first, the wheel about its end.
        let mut r = Rig::open(form, 1.);
        let p = r.free();
        r.frame(vec![moved(p)]);
        let grabbed = r.under(p);
        let to = p + egui::vec2(30., -20.);
        r.frame(vec![
            middle(p, true),
            moved(to),
            middle(to, false),
            wheel_lines(1.),
        ]);
        assert!(!r.e.gesturing());
        assert!(
            near(grabbed, r.under(to)),
            "{form:?}: the pan and the wheel disagree"
        );
        assert!(!same(r.view(), start));
    }
    println!("\nFCAD_31J_ORDER_EXECUTED");
}

/// What a Line drag of the third vertex to a point `off` points above the middle of the
/// first segment (0.4 mm long) writes with Object snap on, at the opening scale or after
/// the view was panned to bring that middle to the centre of the canvas and zoomed in about
/// it to 100 points/mm or more; and the scale it was made at.
fn midpoint_drag(zoomed: bool, pixels_per_point: f32, off: f32) -> ([String; 2], f64) {
    let mut r = Rig::open(Form::LineNew, pixels_per_point);
    r.e.draft.as_mut().expect("draft").points = [["0", "0"], ["0.4", "0"], ["0.55", "0.3"]]
        .map(|p| p.map(str::to_owned))
        .to_vec();
    r.press_label("Object snap");
    if zoomed {
        let rect = r.rect();
        let (minimum, scale) = r.view();
        let middle_now = canvas_point(rect, minimum, scale, [0.2, 0.]);
        let by = rect.center() - middle_now;
        let grab = rect.left_top() + egui::vec2(330., 40.);
        r.frame(vec![moved(grab)]);
        r.frame(vec![middle(grab, true)]);
        r.frame(vec![moved(grab + by)]);
        r.frame(vec![middle(grab + by, false)]);
        r.frame(vec![moved(rect.center())]);
        while r.view().1 < 100. {
            r.frame(vec![wheel_lines(4.)]);
        }
        // (A screen position is an f32, so the point under it is 1e-4 mm exact.)
        let under = r.under(rect.center());
        assert!(
            (under[0] - 0.2).abs() < 1e-4 && under[1].abs() < 1e-4,
            "panned and zoomed about the segment's middle: {under:?}"
        );
    }
    // The middle of the segment, where the pointer is put.
    let rect = r.rect();
    let (minimum, scale) = r.view();
    let target = canvas_point(rect, minimum, scale, [0.2, 0.]) - egui::vec2(0., off);
    let from = r.grab_at(2);
    r.frame(vec![moved(from)]);
    r.frame(vec![primary(from, true)]);
    r.frame(vec![moved(target)]);
    r.frame(vec![primary(target, false)]);
    (
        r.e.draft.as_ref().expect("draft").points[2].clone(),
        f64::from(scale),
    )
}

#[test]
fn zooming_in_makes_close_vertices_short_midpoints_and_a_tiny_bore_pickable_in_logical_points() {
    for pixels_per_point in [1., 2.] {
        // Two vertices 0.4 mm apart: 1.6 points at the opening scale of 4 points/mm.
        let mut r = Rig::open(Form::LineNew, pixels_per_point);
        r.e.draft.as_mut().expect("draft").points = [["0", "0"], ["0.4", "0"], ["5", "5"]]
            .map(|p| p.map(str::to_owned))
            .to_vec();
        // Closed: a press that takes no vertex adds none.
        r.e.draft.as_mut().expect("draft").closed = true;
        let facts = r.facts_with(false);
        let v1 = r.grab_at(1);
        let near_v1 = v1 + egui::vec2(-1., 0.);
        let pick = |r: &mut Rig, at: egui::Pos2| {
            r.frame(vec![moved(at)]);
            r.frame(vec![primary(at, true)]);
            let picked = r.e.canvas.selected;
            r.frame(vec![primary(at, false)]);
            picked
        };
        assert_eq!(
            pick(&mut r, near_v1),
            Some(0),
            "1.6 points apart: the nearer one"
        );
        r.frame(vec![moved(v1)]);
        while r.view().1 < 40. {
            r.frame(vec![wheel_lines(4.)]);
        }
        let v1 = r.grab_at(1);
        assert!(
            (v1 - r.grab_at(0)).length() >= 16.,
            "0.4 mm is 16 points or more"
        );
        assert_eq!(pick(&mut r, v1 + egui::vec2(-1., 0.)), Some(1));
        // The radius is nine logical points of this display, whatever the scale.
        for (away, expected) in [(8.5, Some(1)), (9.5, None)] {
            let spot = v1 + egui::vec2(away, 0.);
            r.e.canvas.selected = None;
            assert_eq!(
                pick(&mut r, spot),
                expected,
                "{away} points at {pixels_per_point}x"
            );
        }
        assert_eq!(r.facts_with(false), facts, "picking edited");

        // The middle of the 0.4 mm segment: an endpoint wins within a point of it, and the
        // middle itself is taken once the segment is long enough on the screen. The snap
        // aperture is in points: the same few points from the middle at any zoom.
        let (before, _) = midpoint_drag(false, pixels_per_point, 0.);
        assert!(
            before == ["0".to_owned(), "0".to_owned()]
                || before == ["0.4".to_owned(), "0".to_owned()],
            "at the opening scale an endpoint wins: {before:?}"
        );
        let middle = ["0.2".to_owned(), "0".to_owned()];
        let aperture = object_snap::APERTURE_POINTS as f32;
        for off in [0., aperture - 2.] {
            let (written, scale) = midpoint_drag(true, pixels_per_point, off);
            assert_eq!(
                written, middle,
                "{off} points from the middle at {scale} points/mm"
            );
        }
        let (free, scale) = midpoint_drag(true, pixels_per_point, aperture + 2.);
        let n = |s: &str| s.parse::<f64>().expect("a number");
        assert_ne!(free, middle, "beyond the aperture the middle is not taken");
        assert!(
            (n(&free[0]) - 0.2).abs() < 1e-3
                && (n(&free[1]) - f64::from(aperture + 2.) / scale).abs() < 1e-3,
            "the free point is where the pointer is: {free:?} at {scale} points/mm"
        );

        // A bore so small that its handle is one point with the centre's, then zoomed.
        let mut r = Rig::open(Form::AnnulusNew, pixels_per_point);
        analytic_t::type_into(&r.ctx, &mut r.e, "Inner radius", "0.01");
        let facts = r.facts();
        let c = r.grab_at(0);
        let apart = |r: &mut Rig| {
            let bore = analytic_t::handle(&r.ctx, &mut r.e, Handle::Bore);
            (bore - r.grab_at(0)).length()
        };
        assert!(apart(&mut r) < 0.2, "the bore is one point with the centre");
        r.frame(vec![moved(c)]);
        for _ in 0..40 {
            if apart(&mut r) >= 30. {
                break;
            }
            r.frame(vec![wheel_lines(5.)]);
        }
        assert!(
            apart(&mut r) >= 30.,
            "zoomed in, the bore is a handle of its own"
        );
        let held_at = |r: &mut Rig, at: egui::Pos2| {
            r.frame(vec![moved(at)]);
            r.frame(vec![primary(at, true)]);
            let held = r.e.annulus_canvas.held();
            r.frame(vec![primary(at, false)]);
            held
        };
        let c = r.grab_at(0);
        let bore = analytic_t::handle(&r.ctx, &mut r.e, Handle::Bore);
        let away = (bore - c).normalized();
        assert_eq!(held_at(&mut r, bore + away * 8.5), Some(Handle::Bore));
        assert_eq!(held_at(&mut r, bore + away * 9.5), None);
        assert_eq!(held_at(&mut r, c - away * 8.5), Some(Handle::Centre));
        assert_eq!(held_at(&mut r, c), Some(Handle::Centre));
        assert_eq!(r.facts(), facts, "picking edited");
    }
    println!("\nFCAD_31J_SMALL_GEOMETRY_EXECUTED");
}

#[test]
fn fit_is_the_only_way_back_and_typing_dragging_and_invalid_text_never_refit() {
    for form in FORMS {
        let mut r = Rig::open(form, 1.);
        let opening = r.view();
        // Away from the opening view by zoom and pan.
        let p = r.free();
        r.frame(vec![moved(p)]);
        r.frame(vec![wheel_lines(3.)]);
        r.frame(vec![middle(p, true), moved(p + egui::vec2(35., -12.))]);
        r.frame(vec![middle(p + egui::vec2(35., -12.), false)]);
        let away = r.view();
        assert!(!same(away, opening));
        let label = if r.line() {
            "Fit drawing"
        } else {
            "Fit profile"
        };

        // Typing, including text that cannot be drawn, does not move the view.
        if r.line() {
            let (old, new) = if form == Form::LineNew {
                ("60", "61")
            } else {
                ("33", "34")
            };
            crate::sketch::tests::replace_field(&r.ctx, &mut r.e, old, new);
        } else {
            let radius = if matches!(form, Form::CircleNew | Form::CircleSaved) {
                "Radius"
            } else {
                "Outer radius"
            };
            analytic_t::type_into(&r.ctx, &mut r.e, radius, "-");
            assert!(
                close(r.view(), away),
                "{form:?}: invalid text moved the view"
            );
            // The view is still the person's to move, and the draft is still the text.
            let typed = r.facts();
            r.frame(vec![moved(p), wheel_lines(-1.)]);
            assert_eq!(r.facts(), typed, "{form:?}: a wheel edited undrawable text");
            r.frame(vec![wheel_lines(1.)]);
            assert!(close(r.view(), away), "{form:?}: back to the same view");
            analytic_t::type_into(&r.ctx, &mut r.e, radius, "12.50");
        }
        assert!(close(r.view(), away), "{form:?}: typing moved the view");
        let g = r.grab();
        r.frame(vec![moved(g)]);
        r.frame(vec![primary(g, true), moved(g + egui::vec2(10., 0.))]);
        r.frame(vec![primary(g + egui::vec2(10., 0.), false)]);
        assert!(close(r.view(), away), "{form:?}: a drag moved the view");

        // Fit is a button, and it alone brings the drawing back.
        let before = r.facts();
        r.press_label(label);
        let fitted = r.view();
        assert!(!same(fitted, away), "{form:?}: Fit did nothing");
        assert_eq!(r.facts(), before, "{form:?}: Fit edited");
        if r.line() {
            // The shown contour in the canvas's 510 x 250 points with 30-point margins.
            let pts = Canvas::points(r.e.draft.as_ref().expect("draft")).expect("numbers");
            let lo = [
                pts.iter().map(|p| p[0]).fold(f64::MAX, f64::min),
                pts.iter().map(|p| p[1]).fold(f64::MAX, f64::min),
            ];
            let hi = [
                pts.iter().map(|p| p[0]).fold(f64::MIN, f64::max),
                pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max),
            ];
            let (minimum, scale) = fit_view(lo, hi, egui::vec2(510., 250.));
            assert!(
                same(fitted, (minimum, scale)),
                "{form:?}: not the shared fit"
            );
            // Reset view is the other explicit way, to the opening of a new editor.
            r.press_label("Reset view");
            assert!(same(
                r.view(),
                (Canvas::default().minimum, Canvas::default().scale)
            ));
        } else if matches!(form, Form::CircleSaved | Form::AnnulusSaved) {
            // The saved numbers were typed over, so the fit is of the draft: it is a fit.
            assert!(fitted.1.is_finite() && fitted.1 > 0.);
        }
    }
    println!("\nFCAD_31J_FIT_EXPLICIT_EXECUTED");
}

#[test]
fn the_hint_names_the_gestures_and_says_when_the_view_is_locked() {
    let painted = |out: &egui::FullOutput, text: &str| {
        out.shapes
            .iter()
            .any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == text))
    };
    const IDLE: &str = "Wheel or pinch: zoom · Shift+drag or middle-drag: pan";
    for form in FORMS {
        let mut r = Rig::open(form, 1.);
        let out = r.frame(vec![]);
        assert!(painted(&out, IDLE), "{form:?}: no hint");
        let rect = r.rect();
        // The hint is on the canvas, so it costs the form no height.
        let hint = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == IDLE => Some(t.visual_bounding_rect()),
                _ => None,
            })
            .expect("the hint");
        assert!(
            rect.contains_rect(hint),
            "{form:?}: the hint is off the canvas"
        );
        // A pan says what Escape does.
        let p = r.free();
        r.frame(vec![moved(p)]);
        let out = r.frame(vec![middle(p, true), moved(p + egui::vec2(10., 0.))]);
        assert!(
            painted(&out, "Panning · Esc puts the view back"),
            "{form:?}"
        );
        r.frame(vec![middle(p, false)]);
        // A held vertex or handle says the view is locked.
        let g = r.grab();
        r.frame(vec![moved(g)]);
        let out = r.frame(vec![primary(g, true)]);
        assert!(painted(&out, "View locked while you drag"), "{form:?}");
        r.frame(vec![primary(g, false)]);
        // A busy form says so.
        let out = r.frame_as(vec![], true, true, None);
        assert!(
            painted(&out, "View locked while the form is busy"),
            "{form:?}"
        );
    }
    println!("\nFCAD_31J_HINT_EXECUTED");
}

#[test]
fn each_profile_keeps_its_own_view_and_cancelling_the_draft_forgets_all_of_them() {
    let mut r = Rig::open(Form::AnnulusNew, 1.);
    let line_opening = (r.e.canvas.minimum, r.e.canvas.scale);
    let p = r.free();
    // Zoom the annulus canvas.
    r.frame(vec![moved(p)]);
    r.frame(vec![wheel_lines(2.)]);
    let annulus = r.e.annulus_canvas.view.expect("view");
    // The circle canvas, once shown, fits and is zoomed by another amount.
    r.press_label("Circle");
    r.frame(vec![]);
    let circle_opening = r.e.circle_canvas.view.expect("a fitted view");
    let p = r.free();
    r.frame(vec![moved(p)]);
    r.frame(vec![wheel_lines(-2.)]);
    let circle = r.e.circle_canvas.view.expect("view");
    assert_ne!(circle, circle_opening);
    assert_eq!(
        r.e.annulus_canvas.view,
        Some(annulus),
        "the annulus view moved"
    );
    // The Line canvas is no part of either.
    r.press_label("Line polygon");
    r.frame(vec![]);
    let p = r.free();
    r.frame(vec![moved(p)]);
    r.frame(vec![wheel_lines(3.)]);
    let line = (r.e.canvas.minimum, r.e.canvas.scale);
    assert!(!same(line, line_opening));
    assert_eq!(r.e.circle_canvas.view, Some(circle));
    assert_eq!(r.e.annulus_canvas.view, Some(annulus));
    // Back and forth: each is where its person left it.
    for (profile, expected) in [("Circle", circle), ("Circle with hole", annulus)] {
        r.press_label(profile);
        r.frame(vec![]);
        let shown = if profile == "Circle" {
            r.e.circle_canvas.view
        } else {
            r.e.annulus_canvas.view
        };
        assert_eq!(shown, Some(expected), "{profile}");
    }
    assert!(same((r.e.canvas.minimum, r.e.canvas.scale), line));
    // A pan held on one canvas while another profile is chosen under it cannot hold the
    // window for good: the canvas is no longer drawn, and its pan ends with the next frame.
    r.press_label("Circle with hole");
    r.frame(vec![]);
    let p = r.free();
    r.frame(vec![moved(p)]);
    r.frame(vec![middle(p, true), moved(p + egui::vec2(12., 5.))]);
    assert!(r.e.annulus_canvas.panning() && r.e.gesturing());
    let spot = text_at(&r.frame(vec![]), "Circle");
    click(&r.ctx, &mut r.e, spot);
    r.frame(vec![]);
    r.frame(vec![]);
    assert!(
        !r.e.gesturing(),
        "a pan on a canvas that is gone holds the window"
    );
    assert!(!r.e.annulus_canvas.panning());
    // It stopped: the pointer's next moves, and the release, pan nothing.
    let stopped = r.e.annulus_canvas.view;
    r.frame(vec![moved(spot + egui::vec2(50., 40.))]);
    r.frame(vec![middle(spot + egui::vec2(50., 40.), false)]);
    assert!(!r.e.gesturing());
    assert_eq!(r.e.annulus_canvas.view, stopped, "a stopped pan went on");
    // Cancel draft ends the form and every view with it.
    r.press_label("Cancel draft");
    assert!(!r.e.active());
    assert_eq!(r.e.circle_canvas.view, None);
    assert_eq!(r.e.annulus_canvas.view, None);
    assert!(same(
        (r.e.canvas.minimum, r.e.canvas.scale),
        (Canvas::default().minimum, Canvas::default().scale)
    ));
    println!("\nFCAD_31J_PROFILES_EXECUTED");
}

#[test]
fn navigation_over_a_canvas_never_reaches_the_3d_camera_and_a_viewport_drag_still_crosses_the_form()
{
    use winit::{
        dpi::PhysicalPosition,
        event::{DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent},
    };
    for form in [Form::LineSaved, Form::CircleNew, Form::AnnulusSaved] {
        for pixels_per_point in [1., 2.] {
            let mut r = Rig::open(form, pixels_per_point);
            let on_canvas = r.free();
            let outside = at(10., 880.);
            r.frame(vec![moved(outside)]);
            assert!(!r.ctx.egui_wants_pointer_input());
            let mut input = ferritecad_ui::ViewportInput::new();
            input.resize(
                (1000. * pixels_per_point) as u32,
                (900. * pixels_per_point) as u32,
            );
            input.frame(([-10.; 3], [10.; 3])).expect("frame");
            let camera = *input.camera();
            let motion = |p: egui::Pos2| WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(
                    f64::from(p.x * pixels_per_point),
                    f64::from(p.y * pixels_per_point),
                ),
            };
            let mouse = |button, state| WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button,
            };
            let wheel = WindowEvent::MouseWheel {
                device_id: DeviceId::dummy(),
                delta: MouseScrollDelta::LineDelta(0., 3.),
                phase: TouchPhase::Moved,
            };
            let pinch = WindowEvent::PinchGesture {
                device_id: DeviceId::dummy(),
                delta: 0.4,
                phase: TouchPhase::Moved,
            };
            let ctx = r.ctx.clone();
            let run = |input: &mut ferritecad_ui::ViewportInput, events: &[WindowEvent]| {
                for event in events {
                    let claimed = crate::window_pointer_claimed(&ctx, input, event);
                    crate::apply_viewport_input(input, event, false, claimed);
                }
            };
            // Over the canvas: wheel, pinch, a middle pan and a primary press and drag.
            run(
                &mut input,
                &[
                    motion(on_canvas),
                    wheel.clone(),
                    pinch.clone(),
                    mouse(MouseButton::Middle, ElementState::Pressed),
                    motion(on_canvas + egui::vec2(24., 12.)),
                    mouse(MouseButton::Middle, ElementState::Released),
                    mouse(MouseButton::Left, ElementState::Pressed),
                    motion(on_canvas + egui::vec2(48., 6.)),
                    mouse(MouseButton::Left, ElementState::Released),
                ],
            );
            assert_eq!(
                *input.camera(),
                camera,
                "{form:?} at {pixels_per_point}x: the canvas gestures moved the 3D view"
            );
            assert!(input.take_pick().is_none(), "{form:?}: the canvas picked");
            // The same wheel and pinch over the unobstructed viewport do move the camera.
            run(&mut input, &[motion(outside), wheel, pinch]);
            assert_ne!(
                *input.camera(),
                camera,
                "{form:?}: the viewport lost its wheel"
            );
            let zoomed = *input.camera();
            // A pan held on the canvas whose pointer then leaves it belongs to the canvas.
            r.frame(vec![moved(on_canvas)]);
            r.frame(vec![
                middle(on_canvas, true),
                moved(on_canvas + egui::vec2(8., 8.)),
            ]);
            assert!(r.e.gesturing());
            run(
                &mut input,
                &[
                    motion(on_canvas),
                    mouse(MouseButton::Middle, ElementState::Pressed),
                    motion(outside),
                    mouse(MouseButton::Middle, ElementState::Released),
                ],
            );
            assert_eq!(
                *input.camera(),
                zoomed,
                "{form:?}: a canvas pan panned the 3D view"
            );
            r.frame(vec![moved(outside), middle(outside, false)]);
            assert!(!r.e.gesturing());
            // A drag that starts in the viewport still belongs to it across the form.
            run(
                &mut input,
                &[
                    motion(outside),
                    mouse(MouseButton::Middle, ElementState::Pressed),
                    motion(on_canvas),
                    mouse(MouseButton::Middle, ElementState::Released),
                ],
            );
            assert_ne!(
                *input.camera(),
                zoomed,
                "{form:?}: the form swallowed a viewport pan"
            );
        }
    }
    println!("\nFCAD_31J_ROUTING_EXECUTED");
}

/// §31J: a pan of the view of the canvas the form shows, begun and still held, as
/// `hold_handle` holds a handle. `release_pan` ends it.
pub(crate) fn hold_pan(e: &mut Editor) -> (egui::Context, egui::Pos2) {
    let ctx = context();
    for _ in 0..3 {
        frame_running(&ctx, e, vec![], false);
    }
    let from = any_canvas(&frame_running(&ctx, e, vec![], false)).left_top() + egui::vec2(60., 50.);
    frame_running(&ctx, e, vec![moved(from)], false);
    frame_running(&ctx, e, vec![middle(from, true)], false);
    let to = from + egui::vec2(18., 9.);
    frame_running(&ctx, e, vec![moved(to)], false);
    assert!(e.gesturing(), "the pan is not held");
    (ctx, to)
}

pub(crate) fn release_pan(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
    frame_running(ctx, e, vec![middle(at, false)], false);
    assert!(!e.gesturing());
}

/// §31J: the canvas the form shows, zoomed by `lines` wheel lines about a point of it,
/// in a context of its own.
pub(crate) fn zoom_form(e: &mut Editor, lines: f32) {
    let ctx = context();
    for _ in 0..3 {
        frame_running(&ctx, e, vec![], false);
    }
    let at = any_canvas(&frame_running(&ctx, e, vec![], false)).left_top() + egui::vec2(90., 70.);
    frame_running(&ctx, e, vec![moved(at)], false);
    frame_running(&ctx, e, vec![wheel_lines(lines)], false);
}

/// §31J: the canvas the form shows, panned by `by` points with a middle-button drag.
pub(crate) fn pan_form(e: &mut Editor, by: egui::Vec2) {
    let (ctx, to) = hold_pan(e);
    frame_running(&ctx, e, vec![moved(to + by)], false);
    release_pan(&ctx, e, to + by);
}

/// §31J: the saved Circle or annulus form's canvas zoomed in about its centre, panned,
/// fitted once (an explicit button) and zoomed and panned again, in a context of its own.
/// Returns the view it ends at. The centre handle stays where the cursor was at every zoom.
fn navigate_round(e: &mut Editor) -> ([f64; 2], f32) {
    let opening = analytic_t::canvas_facts(e);
    let ctx = context();
    for _ in 0..3 {
        frame_running(&ctx, e, vec![], false);
    }
    let zoom_about_the_centre = |e: &mut Editor, lines: f32| {
        let c = analytic_t::handle(&ctx, e, Handle::Centre);
        frame_running(&ctx, e, vec![moved(c)], false);
        frame_running(&ctx, e, vec![wheel_lines(lines)], false);
        let kept = analytic_t::handle(&ctx, e, Handle::Centre);
        assert!(
            (kept - c).length() < 1e-3,
            "the centre left the cursor: {c:?} {kept:?}"
        );
    };
    let pan = |e: &mut Editor, by: egui::Vec2| {
        let from =
            any_canvas(&frame_running(&ctx, e, vec![], false)).left_top() + egui::vec2(60., 50.);
        frame_running(&ctx, e, vec![moved(from)], false);
        frame_running(&ctx, e, vec![middle(from, true)], false);
        frame_running(&ctx, e, vec![moved(from + by)], false);
        frame_running(&ctx, e, vec![middle(from + by, false)], false);
    };
    zoom_about_the_centre(e, 2.);
    pan(e, egui::vec2(20., 12.));
    let moved_once = analytic_t::canvas_facts(e);
    assert_eq!(
        (&moved_once.0, moved_once.1),
        (&opening.0, opening.1),
        "navigation changed a string or a history"
    );
    assert_ne!(moved_once.2, opening.2, "the view did not move");
    // Fit is a button and it brings the opening view back, the numbers being the same.
    analytic_t::press_on_form(e, "Fit profile");
    assert_eq!(
        analytic_t::canvas_facts(e),
        opening,
        "Fit is not the opening fit"
    );
    zoom_about_the_centre(e, 2.);
    pan(e, egui::vec2(-15., 10.));
    let end = analytic_t::canvas_facts(e);
    assert_eq!((&end.0, end.1), (&opening.0, opening.1));
    let (minimum, scale) = end.2.expect("a view");
    assert!(scale > opening.2.expect("a view").1, "zoomed in");
    (minimum, scale)
}

/// §31J: the saved Circle form on `path`, its canvas navigated (see `navigate_round`), then
/// its centre and radius dragged by real events at that scale and **Apply circle**. The
/// form and its request, whose numbers are what the canvas wrote at the new scale.
/// `navigated` runs once the view has moved and nothing has been dragged.
pub(crate) fn navigated_circle_apply(
    path: &std::path::Path,
    reading: &ExtrudeEditSource,
    sketch: ferritecad_types::ObjectId,
    navigated: impl FnOnce(),
) -> (Editor, EditCircleRequest) {
    let mut e = Editor::default();
    e.set_session(true, false, true);
    e.set_analytic_apply(true);
    assert!(e.begin_circle_edit(path, reading, sketch));
    analytic_t::settle_form(&mut e);
    let stored = analytic_t::shown(&e);
    let (_, scale) = navigate_round(&mut e);
    assert_eq!(analytic_t::shown(&e), stored, "navigation edited a string");
    assert!(
        e.take_apply_circle_request().is_none() && e.take_circle_edit_request().is_none(),
        "navigation asked for a job"
    );
    navigated();
    analytic_t::drag_role(&mut e, "centre", egui::vec2(-31.25, 18.75));
    analytic_t::drag_role(&mut e, "outer", egui::vec2(-12.5, -9.75));
    let typed = analytic_t::shown(&e);
    analytic_t::press_on_form(&mut e, "Apply circle");
    let request = e.take_apply_circle_request().expect("Apply circle asked");
    let n = |s: &str| s.parse::<f64>().expect("a number");
    assert_eq!(
        (request.edit.center_mm, request.edit.radius_mm),
        ([n(&typed.centre[0]), n(&typed.centre[1])], n(&typed.outer)),
        "the request is not what the canvas wrote"
    );
    // The numbers are the drag in points over the navigated scale, not the opening one.
    let per_mm = f64::from(scale);
    let close = |a: f64, b: f64| (a - b).abs() < 1e-3;
    assert!(close(request.edit.center_mm[0], 12.5 - 31.25 / per_mm));
    assert!(close(request.edit.center_mm[1], -7.25 - 18.75 / per_mm));
    assert!(close(
        request.edit.radius_mm,
        (10.5 - 12.5 / per_mm).hypot(9.75 / per_mm)
    ));
    (e, request)
}

/// §31J: the same for a saved annulus: centre, outer and bore dragged at the navigated scale.
pub(crate) fn navigated_annulus_apply(
    path: &std::path::Path,
    reading: &ExtrudeEditSource,
    sketch: ferritecad_types::ObjectId,
    navigated: impl FnOnce(),
) -> (Editor, EditAnnulusRequest) {
    let mut e = Editor::default();
    e.set_session(true, false, true);
    e.set_analytic_apply(true);
    assert!(e.begin_annulus_edit(path, reading, sketch));
    analytic_t::settle_form(&mut e);
    let stored = analytic_t::shown(&e);
    let (_, scale) = navigate_round(&mut e);
    assert_eq!(analytic_t::shown(&e), stored, "navigation edited a string");
    assert!(
        e.take_apply_annulus_request().is_none() && e.take_annulus_edit_request().is_none(),
        "navigation asked for a job"
    );
    navigated();
    analytic_t::drag_role(&mut e, "centre", egui::vec2(-31.25, 18.75));
    analytic_t::drag_role(&mut e, "outer", egui::vec2(-12.5, -9.75));
    analytic_t::drag_role(&mut e, "bore", egui::vec2(-7.25, 4.5));
    let typed = analytic_t::shown(&e);
    analytic_t::press_on_form(&mut e, "Apply annulus");
    let request = e.take_apply_annulus_request().expect("Apply annulus asked");
    let n = |s: &str| s.parse::<f64>().expect("a number");
    assert_eq!(
        (
            request.edit.center_mm,
            request.edit.outer_radius_mm,
            request.edit.inner_radius_mm
        ),
        (
            [n(&typed.centre[0]), n(&typed.centre[1])],
            n(&typed.outer),
            n(typed.bore.as_deref().expect("bore"))
        ),
        "the request is not what the canvas wrote"
    );
    let per_mm = f64::from(scale);
    let close = |a: f64, b: f64| (a - b).abs() < 1e-3;
    assert!(close(request.edit.center_mm[0], 12.5 - 31.25 / per_mm));
    assert!(close(request.edit.center_mm[1], -7.25 - 18.75 / per_mm));
    assert!(close(
        request.edit.outer_radius_mm,
        (10.5 - 12.5 / per_mm).hypot(9.75 / per_mm)
    ));
    assert!(close(
        request.edit.inner_radius_mm,
        (-7.25 / per_mm).hypot(4.75 - 4.5 / per_mm)
    ));
    (e, request)
}
