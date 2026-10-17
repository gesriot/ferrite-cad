// SPDX-License-Identifier: MIT
//! §31J: zoom about the pointer and pan, shared by the three Sketch canvases.
//!
//! A view is the lower-left corner in mm and the scale in logical points per mm: the
//! transform of `canvas_point` / `canvas_mm`. This module owns the view arithmetic and
//! the one small state a canvas keeps for it (a held pan and the last pointer). It
//! knows no draft, history, selection or request: a canvas lends it a view for one
//! event and decides, from its own edit gesture, whether the view may change.

use super::{canvas_mm, pointer_over};

/// Fewest and most logical points per mm a view may have. The largest drawable extent is
/// 2·10⁶ mm (`DRAWABLE_MM` either side of 0), which the Fit of either canvas shows in at
/// least 170 points (≥ 8.5·10⁻⁵ points/mm), so no Fit is below the lower limit. A Fit of a
/// degenerate profile reaches 4.5·10⁷ points/mm (its extent floor is 10⁻⁵ mm in 450
/// points), so no Fit is above the upper limit, where one point is 10⁻⁸ mm: still far
/// above the f64 spacing at 10⁶ mm (1.2·10⁻¹⁰ mm).
pub(super) const MIN_SCALE: f32 = 1e-5;
pub(super) const MAX_SCALE: f32 = 1e8;
/// The view's corner stays within this many mm of the origin. Arithmetic safety, not a
/// drawing rule: it keeps every transformed coordinate far inside f32 whatever events arrive.
const MAX_CORNER_MM: f64 = 1e9;
/// One event multiplies the scale by at most this, or divides it.
const MAX_STEP: f64 = 8.0;
/// A wheel "line" is as many points as egui scrolls, and a zoom of one e-fold takes this many.
const POINTS_PER_LINE: f64 = 40.0;
const POINTS_PER_E_FOLD: f64 = 200.0;

/// Lower-left corner (mm) and logical points per mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct View {
    pub(super) minimum: [f64; 2],
    pub(super) scale: f32,
}

impl View {
    pub(super) fn screen(&self, rect: egui::Rect, p: [f64; 2]) -> egui::Pos2 {
        super::canvas_point(rect, self.minimum, self.scale, p)
    }
    fn document(&self, rect: egui::Rect, at: egui::Pos2) -> [f64; 2] {
        canvas_mm(rect, self.minimum, self.scale, at)
    }
}

/// `view` zoomed by `e^ln` about the screen point `at` of the canvas `rect`: the document
/// point under `at` is under it afterwards. None when nothing changes (a non-finite step
/// or view, or the scale already at its limit), so a limit never moves the view.
pub(super) fn zoomed(view: View, rect: egui::Rect, at: egui::Pos2, ln: f64) -> Option<View> {
    if !ln.is_finite()
        || !view.scale.is_finite()
        || view.scale <= 0.0
        || !rect.is_finite()
        || !rect.is_positive()
        || !at.is_finite()
    {
        return None;
    }
    let step = ln.clamp(-MAX_STEP.ln(), MAX_STEP.ln());
    let scale = (f64::from(view.scale) * step.exp())
        .clamp(f64::from(MIN_SCALE), f64::from(MAX_SCALE)) as f32;
    if scale == view.scale {
        return None;
    }
    let anchor = view.document(rect, at);
    let origin = rect.left_bottom();
    let per_mm = f64::from(scale);
    let minimum = [
        anchor[0] - f64::from(at.x - origin.x) / per_mm,
        anchor[1] - f64::from(origin.y - at.y) / per_mm,
    ];
    // Clamping this corner would move the document point under the cursor. A zoom
    // that cannot preserve its anchor inside the arithmetic bound changes nothing.
    if minimum.iter().any(|m| m.abs() > MAX_CORNER_MM) {
        return None;
    }
    corner(minimum, scale)
}

/// The view at `scale` whose document point `grab` is under the screen point `at`.
fn placed(rect: egui::Rect, scale: f32, grab: [f64; 2], at: egui::Pos2) -> Option<View> {
    let origin = rect.left_bottom();
    let per_mm = f64::from(scale);
    corner(
        [
            grab[0] - f64::from(at.x - origin.x) / per_mm,
            grab[1] - f64::from(origin.y - at.y) / per_mm,
        ],
        scale,
    )
}

/// A view with this corner, or None when the corner is not finite. A corner beyond
/// [`MAX_CORNER_MM`] is held at it.
fn corner(minimum: [f64; 2], scale: f32) -> Option<View> {
    if !minimum.iter().all(|m| m.is_finite()) {
        return None;
    }
    Some(View {
        minimum: minimum.map(|m| m.clamp(-MAX_CORNER_MM, MAX_CORNER_MM)),
        scale,
    })
}

/// The natural log of the zoom factor one wheel event asks for (positive: closer), or None
/// for a delta that is not a number. Only the vertical part counts.
fn wheel_ln(unit: egui::MouseWheelUnit, delta: egui::Vec2, rect: egui::Rect) -> Option<f64> {
    let points = match unit {
        egui::MouseWheelUnit::Point => f64::from(delta.y),
        egui::MouseWheelUnit::Line => f64::from(delta.y) * POINTS_PER_LINE,
        egui::MouseWheelUnit::Page => f64::from(delta.y) * f64::from(rect.height()),
    };
    points.is_finite().then(|| points / POINTS_PER_E_FOLD)
}

/// The natural log of a pinch's factor, or None for one that is not a positive number.
fn pinch_ln(factor: f32) -> Option<f64> {
    (factor.is_finite() && factor > 0.0).then(|| f64::from(factor).ln())
}

/// A pan held: the button that began it, the document point it holds under the pointer,
/// and the view at the press (what Escape returns to).
#[derive(Debug)]
struct Pan {
    button: egui::PointerButton,
    grab: [f64; 2],
    start: View,
}

/// What a canvas lends for one event.
pub(super) struct Surface<'a> {
    pub(super) ui: &'a egui::Ui,
    pub(super) rect: egui::Rect,
    /// The part of the canvas that is on the screen.
    pub(super) visible: egui::Rect,
    /// The form is usable now: enabled, with no request waiting to be taken.
    pub(super) open: bool,
    /// An edit gesture holds the view.
    pub(super) frozen: bool,
}

impl Surface<'_> {
    fn over(&self, at: egui::Pos2) -> bool {
        pointer_over(self.ui, self.visible, at)
    }
}

/// One canvas's navigation state. It travels with the canvas, so with the form and its tab.
#[derive(Default)]
pub(super) struct Navigation {
    /// The pointer as the last event showed it; none when unknown.
    pointer: Option<egui::Pos2>,
    pan: Option<Pan>,
    /// The egui frame this canvas was last drawn in.
    drawn: u64,
}

/// What an `Editor`'s `Debug` shows of it, and tests compare: the held pan. The last pointer
/// and the frame number are bookkeeping of the frames that were drawn, not state of the editor.
impl std::fmt::Debug for Navigation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Navigation")
            .field("pan", &self.pan)
            .finish_non_exhaustive()
    }
}

impl Navigation {
    /// A pan is held: the window is held with it, as with a vertex drag.
    pub(super) fn panning(&self) -> bool {
        self.pan.is_some()
    }

    /// The start of a frame. A pan does not outlive a form that cannot be used. Without a
    /// pointer event in the frame, egui's own pointer position is the one to trust.
    pub(super) fn begin(&mut self, ui: &egui::Ui, open: bool) {
        self.drawn = ui.ctx().cumulative_frame_nr();
        if !open {
            self.pan = None;
        }
        let moved = ui.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::PointerMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::PointerGone
                )
            })
        });
        if !moved || self.pointer.is_none() {
            self.pointer = ui.input(|i| i.pointer.latest_pos());
        }
    }

    /// A canvas that was not drawn in the frame before `frame` (another profile was chosen
    /// under a held pan) cannot see its pan's release: the pan ends where it was.
    pub(super) fn forget_undrawn(&mut self, frame: u64) {
        if self.drawn + 1 < frame {
            self.pan = None;
        }
    }

    /// The end of a frame: a pan whose button is no longer down, or whose window lost the
    /// focus, is over (its release was lost).
    pub(super) fn finish(&mut self, ui: &egui::Ui) {
        if let Some(pan) = &self.pan
            && (!ui.input(|i| i.focused) || !ui.input(|i| i.pointer.button_down(pan.button)))
        {
            self.pan = None;
        }
    }

    /// One event, in order, against `view`. Returns whether it belongs to navigation: the
    /// canvas's editing then does not see it. A press that begins a pan is such an event;
    /// so is every press while one is held, and the moves and the release of the pan.
    pub(super) fn event(
        &mut self,
        surface: &Surface<'_>,
        event: &egui::Event,
        view: &mut View,
    ) -> bool {
        match *event {
            egui::Event::PointerMoved(pos) => {
                self.pointer = Some(pos);
                self.pan_to(surface.rect, pos, view)
            }
            egui::Event::PointerGone => {
                self.pointer = None;
                self.pan = None;
                false
            }
            egui::Event::WindowFocused(false) => {
                self.pan = None;
                false
            }
            egui::Event::MouseWheel {
                unit,
                delta,
                phase: egui::TouchPhase::Start | egui::TouchPhase::Move | egui::TouchPhase::End,
                ..
            } => {
                self.zoom(surface, wheel_ln(unit, delta, surface.rect), view);
                false
            }
            egui::Event::Zoom(factor) => {
                self.zoom(surface, pinch_ln(factor), view);
                false
            }
            egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers,
            } => {
                self.pointer = Some(pos);
                let reserved = button == egui::PointerButton::Middle
                    || (button == egui::PointerButton::Primary && modifiers.shift);
                let reserved_here = reserved && surface.over(pos);
                if self.pan.is_none() && reserved_here && surface.open && !surface.frozen {
                    self.pan = Some(Pan {
                        button,
                        grab: view.document(surface.rect, pos),
                        start: *view,
                    });
                }
                // A queued request or an edit gesture may prevent navigation. The
                // reserved press still must not turn into a model edit or a click.
                self.pan.is_some() || reserved
            }
            egui::Event::PointerButton {
                pos,
                button,
                pressed: false,
                ..
            } => {
                self.pointer = Some(pos);
                if self.pan.as_ref().is_some_and(|pan| pan.button == button) {
                    self.pan_to(surface.rect, pos, view);
                    self.pan = None;
                    return true;
                }
                false
            }
            egui::Event::Key {
                key: egui::Key::Escape,
                pressed: true,
                ..
            } if self.pan.is_some() => {
                surface
                    .ui
                    .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                if let Some(pan) = self.pan.take() {
                    *view = pan.start;
                }
                true
            }
            _ => false,
        }
    }

    /// The held pan's view with the pointer at `pos`. True while a pan is held.
    fn pan_to(&self, rect: egui::Rect, pos: egui::Pos2, view: &mut View) -> bool {
        let Some(pan) = &self.pan else {
            return false;
        };
        if let Some(next) = placed(rect, view.scale, pan.grab, pos) {
            *view = next;
        }
        true
    }

    /// A zoom about the pointer, when the pointer is on the visible, uncovered canvas of a
    /// form that is open and whose view no edit gesture holds.
    fn zoom(&self, surface: &Surface<'_>, ln: Option<f64>, view: &mut View) {
        let (Some(at), Some(ln)) = (self.pointer, ln) else {
            return;
        };
        if !surface.open || surface.frozen || !surface.over(at) {
            return;
        }
        if let Some(next) = zoomed(*view, surface.rect, at, ln) {
            *view = next;
        }
    }

    /// The words painted on the canvas.
    pub(super) fn hint(&self, open: bool, frozen: bool) -> &'static str {
        if self.pan.is_some() {
            "Panning · Esc puts the view back"
        } else if frozen {
            "View locked while you drag"
        } else if !open {
            "View locked while the form is busy"
        } else {
            "Wheel or pinch: zoom · Shift+drag or middle-drag: pan"
        }
    }
}

/// The hint, at the lower right of the canvas, on a dark plate: no height of the form.
pub(super) fn paint_hint(painter: &egui::Painter, rect: egui::Rect, words: &str) {
    let galley = painter.layout_no_wrap(
        words.into(),
        egui::FontId::proportional(11.),
        egui::Color32::from_gray(190),
    );
    let corner = rect.right_bottom() - galley.size() - egui::vec2(6., 5.);
    painter.rect_filled(
        egui::Rect::from_min_size(corner, galley.size()).expand(2.),
        2.,
        egui::Color32::from_black_alpha(170),
    );
    painter.galley(corner, galley, egui::Color32::from_gray(190));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(40., 60.), egui::vec2(510., 230.))
    }

    fn view(minimum: [f64; 2], scale: f32) -> View {
        View { minimum, scale }
    }

    /// The document point under `at`, as the canvas reads it, for a view.
    fn under(view: View, at: egui::Pos2) -> [f64; 2] {
        view.document(rect(), at)
    }

    fn near(a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
        (0..2).all(|i| (a[i] - b[i]).abs() <= tolerance * (1.0 + a[i].abs()))
    }

    #[test]
    fn zoom_keeps_the_document_point_under_the_cursor_at_every_scale_and_display() {
        // 1x and 2x give the same view in logical points: the rectangle and the cursor
        // are points either way, and what differs is only the OS pixel they came from.
        for pixels_per_point in [1.0_f32, 2.0] {
            for (minimum, scale) in [
                ([-8.75, -7.5], 4.0),
                ([1.0e5, -3.0e5], 0.003),
                ([-12.5, 7.25], 2.5e6),
                ([0.0, 0.0], MIN_SCALE),
                ([0.0, 0.0], MAX_SCALE),
            ] {
                let before = view(minimum, scale);
                // Whole physical pixels, as the OS reports them.
                let pixel = |x: f32, y: f32| egui::pos2(x / pixels_per_point, y / pixels_per_point);
                for at in [
                    pixel(81.0, 121.0),
                    pixel(299.0, 301.0),
                    pixel(1099.0, 579.0),
                    rect().left_bottom(),
                    rect().right_top(),
                ] {
                    for ln in [0.2, -0.2, 1.7, -1.7, 0.001] {
                        let Some(after) = zoomed(before, rect(), at, ln) else {
                            // Only a scale already at its limit may stay put.
                            assert!(
                                scale == MIN_SCALE || scale == MAX_SCALE,
                                "{scale} {ln} gave no zoom"
                            );
                            continue;
                        };
                        assert!(after.scale > 0.0 && after.scale.is_finite());
                        assert!(
                            near(under(before, at), under(after, at), 1e-9),
                            "{minimum:?} {scale} {ln} at {at:?}: {:?} then {:?}",
                            under(before, at),
                            under(after, at)
                        );
                        assert_eq!(after.scale > scale, ln > 0.0, "closer means a larger scale");
                    }
                }
            }
        }
        // Zoom in then out by the same step is the view it was.
        let start = view([3.0, -4.0], 6.0);
        let at = egui::pos2(300.0, 200.0);
        let there = zoomed(start, rect(), at, 0.5).expect("zoomed");
        let back = zoomed(there, rect(), at, -0.5).expect("zoomed");
        assert!(near(start.minimum, back.minimum, 1e-9));
        assert!((back.scale - start.scale).abs() <= 1e-5 * start.scale);
    }

    #[test]
    fn the_scale_stops_at_its_limits_without_moving_the_point_under_the_cursor() {
        // Reaching the arithmetic corner bound must not move the zoom anchor.
        let edge = view([-MAX_CORNER_MM, -MAX_CORNER_MM], MIN_SCALE * 2.);
        let at_edge = rect().center();
        if let Some(next) = zoomed(edge, rect(), at_edge, -0.5) {
            assert!(
                near(under(edge, at_edge), under(next, at_edge), 1e-12),
                "zoom clamped the corner and moved the anchor: {:?} -> {:?}",
                under(edge, at_edge),
                under(next, at_edge)
            );
        }
        let at = egui::pos2(123.0, 234.0);
        for (limit, ln) in [(MAX_SCALE, 1.0), (MIN_SCALE, -1.0)] {
            // Walk to the limit in steps of the largest size; the last step is partial.
            let mut v = view([5.0, -5.0], 1.0);
            let anchor = under(v, at);
            for _ in 0..80 {
                if let Some(next) = zoomed(v, rect(), at, ln * 3.0) {
                    assert!(near(anchor, under(next, at), 1e-9), "{v:?} -> {next:?}");
                    v = next;
                }
            }
            assert_eq!(v.scale, limit, "the walk ends on the limit");
            // At the limit, more of the same is nothing at all: not one bit moves.
            assert_eq!(zoomed(v, rect(), at, ln), None);
            assert_eq!(zoomed(v, rect(), at, ln * 1e300), None);
            assert_eq!(zoomed(v, rect(), egui::pos2(500.0, 100.0), ln), None);
            // The way back works from the limit, with the anchor kept.
            let back = zoomed(v, rect(), at, -ln).expect("back");
            assert!(near(anchor, under(back, at), 1e-9));
            assert!(back.scale > MIN_SCALE && back.scale < MAX_SCALE);
        }
        // A step is at most 8x, however large the delta.
        let big = zoomed(view([0.0, 0.0], 4.0), rect(), at, 1e9).expect("zoomed");
        assert!(
            (f64::from(big.scale) / 4.0 - MAX_STEP).abs() < 1e-4,
            "{big:?}"
        );
        let small = zoomed(view([0.0, 0.0], 4.0), rect(), at, -1e9).expect("zoomed");
        assert!((f64::from(small.scale) * MAX_STEP / 4.0 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn nothing_that_is_not_a_finite_number_reaches_the_view() {
        let v = view([-8.75, -7.5], 4.0);
        let at = egui::pos2(300.0, 200.0);
        for ln in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(zoomed(v, rect(), at, ln), None, "{ln}");
        }
        // A zero step is no step. A view that is not a view is not zoomed.
        assert_eq!(zoomed(v, rect(), at, 0.0), None);
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(zoomed(view([0.0; 2], bad), rect(), at, 0.5), None);
        }
        // A cursor or a rectangle that is not finite cannot place a corner.
        for at in [
            egui::pos2(f32::NAN, 5.0),
            egui::pos2(f32::INFINITY, 5.0),
            egui::pos2(3.0e38, -3.0e38),
        ] {
            if let Some(next) = zoomed(v, rect(), at, 0.5) {
                assert!(next.minimum.iter().all(|m| m.abs() <= MAX_CORNER_MM));
            }
        }
        let empty = egui::Rect::from_min_size(egui::pos2(f32::NAN, 0.0), egui::Vec2::ZERO);
        assert_eq!(zoomed(v, empty, at, 0.5), None);
        for size in [egui::Vec2::ZERO, egui::vec2(0., 100.), egui::vec2(100., 0.)] {
            let empty = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            assert_eq!(zoomed(v, empty, at, 0.5), None);
        }
        // Wheel and pinch deltas that are not numbers ask for nothing.
        let r = rect();
        for delta in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for unit in [
                egui::MouseWheelUnit::Point,
                egui::MouseWheelUnit::Line,
                egui::MouseWheelUnit::Page,
            ] {
                assert_eq!(wheel_ln(unit, egui::vec2(0.0, delta), r), None);
            }
        }
        // Finite deltas: sign, units, and only the vertical part.
        assert_eq!(
            wheel_ln(egui::MouseWheelUnit::Point, egui::vec2(50., 0.), r),
            Some(0.0)
        );
        assert_eq!(
            wheel_ln(egui::MouseWheelUnit::Point, egui::vec2(0., 20.), r),
            Some(0.1)
        );
        assert_eq!(
            wheel_ln(egui::MouseWheelUnit::Line, egui::vec2(0., 1.), r),
            Some(0.2)
        );
        assert_eq!(
            wheel_ln(egui::MouseWheelUnit::Line, egui::vec2(0., -2.), r),
            Some(-0.4)
        );
        assert_eq!(
            wheel_ln(egui::MouseWheelUnit::Page, egui::vec2(0., 1.), r),
            Some(1.15)
        );
        for factor in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(pinch_ln(factor), None, "{factor}");
        }
        assert_eq!(pinch_ln(1.0), Some(0.0));
        assert!(pinch_ln(2.0).is_some_and(|ln| (ln - std::f64::consts::LN_2).abs() < 1e-6));
        assert!(pinch_ln(0.5).is_some_and(|ln| ln < 0.0));
    }

    #[test]
    fn a_pan_holds_the_grabbed_point_under_the_pointer_and_the_corner_stays_finite() {
        let r = rect();
        let start = view([-8.75, -7.5], 4.0);
        let press = egui::pos2(200.0, 150.0);
        let grab = under(start, press);
        for to in [
            press,
            egui::pos2(260.0, 90.0),
            egui::pos2(-500.0, 900.0),
            egui::pos2(1.0e5, -1.0e5),
        ] {
            let moved = placed(r, start.scale, grab, to).expect("placed");
            // The same point is under the pointer, and the scale did not change.
            assert!(near(grab, under(moved, to), 1e-9), "{to:?}");
            assert_eq!(moved.scale, start.scale);
        }
        // Pressing and not moving is no pan at all.
        assert!(near(
            placed(r, start.scale, grab, press).expect("placed").minimum,
            start.minimum,
            1e-12
        ));
        // A pan moves the corner by the pointer's movement over the scale.
        let moved = placed(r, 4.0, grab, press + egui::vec2(40.0, -20.0)).expect("placed");
        assert!(near(
            moved.minimum,
            [start.minimum[0] - 10.0, start.minimum[1] - 5.0],
            1e-12
        ));
        // Whatever the pointer says, the corner is a number within the bound.
        for to in [
            egui::pos2(f32::MAX, f32::MAX),
            egui::pos2(f32::MIN, f32::MIN),
        ] {
            let far = placed(r, MIN_SCALE, grab, to).expect("placed");
            assert!(far.minimum.iter().all(|m| m.abs() <= MAX_CORNER_MM));
        }
        assert_eq!(
            placed(r, 4.0, grab, egui::pos2(f32::NAN, 0.0)).map(|v| v.minimum),
            None
        );
    }
}
