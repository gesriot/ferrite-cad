// SPDX-License-Identifier: MIT
//! §31K: the accepted, solved drawing of a Sketch in the constraints form, and the targets a
//! click on it fills.
//!
//! The drawing is what the rebuild behind the shown picture saw (`SketchPresentation`), not
//! the coordinates the document stores and not a preview of the rules still pending in the
//! form: those are applied by the form's own Apply and drawn by the next accepted scene. A
//! click only fills the fields the lists already fill (the picked Line or Circle, the pin
//! end, pair slot A or B); it adds no rule, changes no number and spends no Undo.

use super::{
    DRAWABLE_MM, PICK_POINTS,
    navigation::{self, MAX_SCALE, MIN_SCALE, Navigation, Surface, View},
    paint_grid, paint_pointer_label, pointer_over,
};
use ferritecad_document::{
    ConstraintSketchChoice, DocumentVersion, ExtrudeEditSource, LineEndpoint, Sketch,
    SketchGeometry,
};
use ferritecad_scene::SketchPresentation;
use ferritecad_types::StableEntityId;

/// The canvas's size in logical points. Fixed, so it never widens or heightens the form.
pub(crate) const SIZE: egui::Vec2 = egui::vec2(268., 112.);
/// Space kept around the drawing by Fit, in points: the words painted on the top and bottom
/// edges of the canvas are kept clear of it.
const MARGIN_SIDE: f32 = 14.;
const MARGIN_TOP: f32 = 24.;
const MARGIN_BOTTOM: f32 = 22.;
/// A press and a release at most this far apart (egui's own `max_click_dist`) are a click.
const CLICK_POINTS: f32 = 6.;

pub(crate) const DRAWING_COLOUR: egui::Color32 = egui::Color32::from_gray(185);
pub(crate) const SELECTED_COLOUR: egui::Color32 = egui::Color32::from_rgb(90, 175, 255);
pub(crate) const SIDE_A_COLOUR: egui::Color32 = egui::Color32::from_rgb(255, 165, 60);
pub(crate) const SIDE_B_COLOUR: egui::Color32 = egui::Color32::from_rgb(64, 224, 160);
pub(crate) const HOVER_COLOUR: egui::Color32 = egui::Color32::LIGHT_YELLOW;

/// The scene's drawings, with the version of the reading they came from.
///
/// Held beside `edit_source` by the accepted scene and replaced with it, in the same
/// statement: both come out of one pinned reading and one rebuild (`LoadedScene`), so the
/// catalogue a form is opened from and the drawing it is handed are of one version. A form is
/// handed a drawing only when the two versions are equal.
#[derive(Debug, Default)]
pub(crate) struct Accepted {
    version: Option<DocumentVersion>,
    presentations: Vec<SketchPresentation>,
}

impl Accepted {
    pub(crate) fn new(
        version: Option<DocumentVersion>,
        presentations: Vec<SketchPresentation>,
    ) -> Self {
        Self {
            version,
            presentations,
        }
    }

    /// The picture is kept for a version that draws the same model (§30N); the drawings are
    /// the picture's and stay, and the version they are described by moves with the facts.
    pub(crate) fn follow(&mut self, version: DocumentVersion) {
        if self.version.is_some() {
            self.version = Some(version);
        }
    }

    /// What the form for `choice` is handed, from the facts it is opened from.
    pub(crate) fn drawing(
        &self,
        source: &ExtrudeEditSource,
        choice: &ConstraintSketchChoice,
    ) -> Drawing {
        match self.version {
            None => return Drawing::Unavailable(Unavailable::NotHanded),
            Some(version) if version != source.version => {
                return Drawing::Unavailable(Unavailable::OtherVersion);
            }
            Some(_) => {}
        }
        let (Some(presentation), Some(stored)) = (
            self.presentations
                .iter()
                .find(|p| p.sketch() == choice.sketch),
            choice.stored.as_ref(),
        ) else {
            return Drawing::Unavailable(Unavailable::NoDrawing);
        };
        Drawing::of(
            stored,
            presentation
                .curves()
                .iter()
                .map(|c| (c.id(), c.geometry().clone())),
        )
    }
}

/// One curve of the drawing, at the coordinates the rebuild built from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Curve {
    pub(crate) id: StableEntityId,
    pub(crate) shape: Shape,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Shape {
    Line { start: [f64; 2], end: [f64; 2] },
    Circle { center: [f64; 2], radius: f64 },
}

impl Shape {
    /// A Line with two distinct finite ends, or a Circle with a positive finite radius and
    /// a finite centre: something a pointer can be near. Anything else is drawn as nothing
    /// and never picked.
    fn live(&self) -> bool {
        match *self {
            Self::Line { start, end } => {
                start != end && start.iter().chain(&end).all(|v| v.is_finite())
            }
            Self::Circle { center, radius } => {
                radius > 0.0 && radius.is_finite() && center.iter().all(|v| v.is_finite())
            }
        }
    }
}

/// Why a form has no accepted drawing. Each is said to the person in the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unavailable {
    /// No accepted scene handed this form a drawing.
    NotHanded,
    /// The drawing is of another version of the document than the form describes.
    OtherVersion,
    /// The accepted rebuild has no drawing of this Sketch.
    NoDrawing,
    /// Its curves are not the form's stored curves (identity, order or kind).
    Differs,
    /// A profile the canvas does not cover: only a Line polygon or one Circle.
    Profile,
    /// A coordinate that cannot be drawn.
    NotDrawable,
}

impl Unavailable {
    /// A few words, for the line the form stands on.
    pub(crate) fn short(self) -> &'static str {
        match self {
            Self::NotHanded => "none was handed over",
            Self::OtherVersion => "another version",
            Self::NoDrawing => "none for this Sketch",
            Self::Differs => "not these curves",
            Self::Profile => "not for this profile",
            Self::NotDrawable => "not drawable",
        }
    }
    /// The whole sentence, for the hover text.
    pub(crate) fn why(self) -> &'static str {
        match self {
            Self::NotHanded => "No accepted scene handed this form a drawing.",
            Self::OtherVersion => {
                "The accepted drawing is of another version of the document than this form \
                 describes."
            }
            Self::NoDrawing => "The accepted rebuild has no drawing of this Sketch.",
            Self::Differs => {
                "The accepted drawing's curves are not the curves this form stores: they differ \
                 in identity, order or kind."
            }
            Self::Profile => {
                "The canvas shows a Line polygon or one Circle; this profile is neither."
            }
            Self::NotDrawable => {
                "A coordinate of the accepted drawing is not a finite number within the \
                 drawable range."
            }
        }
    }
}

/// The accepted solved drawing of the Sketch a form describes, or why there is none.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Drawing {
    Solved(Vec<Curve>),
    Unavailable(Unavailable),
}

impl Drawing {
    /// The drawing `presented` makes for a form whose stored Sketch is `stored`: the same
    /// curves in the same order, a Line polygon or one Circle, every number drawable.
    pub(crate) fn of(
        stored: &Sketch,
        presented: impl IntoIterator<Item = (StableEntityId, SketchGeometry)>,
    ) -> Self {
        let presented: Vec<_> = presented.into_iter().collect();
        let same = presented.len() == stored.curves.len()
            && presented
                .iter()
                .zip(&stored.curves)
                .all(|((id, shown), c)| {
                    *id == c.id
                        && std::mem::discriminant(shown) == std::mem::discriminant(&c.geometry)
                });
        if !same {
            return Self::Unavailable(Unavailable::Differs);
        }
        let mut curves = Vec::new();
        for (id, geometry) in presented {
            let shape = match geometry {
                SketchGeometry::Line { start, end } => Shape::Line {
                    start: [start.x, start.y],
                    end: [end.x, end.y],
                },
                SketchGeometry::Circle { center, radius } => Shape::Circle {
                    center: [center.x, center.y],
                    radius,
                },
                // A point or an arc (and any shape added later) is a profile the canvas does
                // not cover.
                _ => return Self::Unavailable(Unavailable::Profile),
            };
            curves.push(Curve { id, shape });
        }
        let polygon =
            !curves.is_empty() && curves.iter().all(|c| matches!(c.shape, Shape::Line { .. }));
        let circle = matches!(
            curves.as_slice(),
            [Curve {
                shape: Shape::Circle { .. },
                ..
            }]
        );
        if !(polygon || circle) {
            return Self::Unavailable(Unavailable::Profile);
        }
        let drawable = |v: f64| v.is_finite() && v.abs() <= DRAWABLE_MM;
        let numbers = curves.iter().flat_map(|c| match c.shape {
            Shape::Line { start, end } => vec![start[0], start[1], end[0], end[1]],
            Shape::Circle { center, radius } => vec![center[0], center[1], radius],
        });
        if !numbers.into_iter().all(drawable) {
            return Self::Unavailable(Unavailable::NotDrawable);
        }
        Self::Solved(curves)
    }
}

/// What a click fills. The person chooses it; a click's position never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Target {
    /// A whole Line or Circle: the form's single selection.
    #[default]
    Object,
    /// A Line's named end (Start or End), or a Circle's centre: the selection and the pin end.
    Endpoint,
    /// A Line or Circle for pair slot A.
    PairA,
    /// A Line or Circle for pair slot B.
    PairB,
}

impl Target {
    const ALL: [Self; 4] = [Self::Object, Self::Endpoint, Self::PairA, Self::PairB];
    fn label(self) -> &'static str {
        match self {
            Self::Object => "Object",
            Self::Endpoint => "Endpoint",
            Self::PairA => "Pair A",
            Self::PairB => "Pair B",
        }
    }
    fn words(self) -> &'static str {
        match self {
            Self::Object => {
                "A click selects the nearest Line or Circle (its centre or its circle)."
            }
            Self::Endpoint => {
                "A click selects the nearest Line end, named Start or End, or a Circle's centre."
            }
            Self::PairA => "A click puts the nearest Line or Circle in pair slot A.",
            Self::PairB => "A click puts the nearest Line or Circle in pair slot B.",
        }
    }
}

/// What part of a curve was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Line,
    Start,
    End,
    Ring,
    Centre,
}

impl Part {
    fn word(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Start => "Start",
            Self::End => "End",
            Self::Ring => "circle",
            Self::Centre => "centre",
        }
    }
}

/// A curve of the drawing, and where on it the pointer was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hit {
    pub(crate) curve: StableEntityId,
    pub(crate) part: Part,
}

/// A click that ended on the canvas, and the target it was made under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Picked {
    pub(crate) target: Target,
    pub(crate) hit: Hit,
}

/// The form's current choices, for the canvas to highlight.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Marks {
    pub(crate) selected: Option<StableEntityId>,
    /// The end "Pin Start/End" names for the selected Line.
    pub(crate) pin: LineEndpoint,
    pub(crate) pair: [Option<StableEntityId>; 2],
    /// The end each pair Line's directed-angle vector starts at.
    pub(crate) from: [LineEndpoint; 2],
}

/// The nearest part of the drawing to `at`, within the pick radius of the visible canvas.
///
/// Distances are in logical points on the screen, so zoom and the display's pixels change how
/// many millimetres the radius covers and nothing else. The nearest wins; an exact tie goes to
/// the curve stored first, and within a curve to Start before End, the circle before its
/// centre. Which parts may be hit is the target's: Object and the pair slots take a whole
/// Line (anywhere along it) or a Circle (its circle or its centre); Endpoint takes only the
/// named ends of a Line or the centre of a Circle.
pub(super) fn pick(
    curves: &[Curve],
    target: Target,
    view: &View,
    rect: egui::Rect,
    visible: egui::Rect,
    at: egui::Pos2,
) -> Option<Hit> {
    let limit = PICK_POINTS * PICK_POINTS;
    let mut best: Option<(f32, usize, usize, Hit)> = None;
    for (order, curve) in curves.iter().enumerate() {
        if !curve.shape.live() {
            continue;
        }
        for (sub, (part, distance, nearest)) in candidates(&curve.shape, target, view, rect, at)
            .into_iter()
            .enumerate()
        {
            if !(distance <= limit && visible.contains(nearest)) {
                continue;
            }
            let better = best.as_ref().is_none_or(|(d, o, s, _)| {
                distance
                    .total_cmp(d)
                    .then(order.cmp(o))
                    .then(sub.cmp(s))
                    .is_lt()
            });
            if better {
                best = Some((
                    distance,
                    order,
                    sub,
                    Hit {
                        curve: curve.id,
                        part,
                    },
                ));
            }
        }
    }
    best.map(|(_, _, _, hit)| hit)
}

/// The parts of `shape` the target can take: each with its squared screen distance from `at`
/// and the point of it nearest to `at`.
fn candidates(
    shape: &Shape,
    target: Target,
    view: &View,
    rect: egui::Rect,
    at: egui::Pos2,
) -> Vec<(Part, f32, egui::Pos2)> {
    let point = |part, p: egui::Pos2| (part, p.distance_sq(at), p);
    match *shape {
        Shape::Line { start, end } => {
            let (a, b) = (view.screen(rect, start), view.screen(rect, end));
            match target {
                Target::Endpoint => vec![point(Part::Start, a), point(Part::End, b)],
                _ => {
                    let ab = b - a;
                    let length_sq = ab.length_sq();
                    let t = if length_sq > 0.0 {
                        ((at - a).dot(ab) / length_sq).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    vec![point(Part::Line, a + ab * t)]
                }
            }
        }
        Shape::Circle { center, radius } => {
            let c = view.screen(rect, center);
            let centre = point(Part::Centre, c);
            match target {
                Target::Endpoint => vec![centre],
                _ => {
                    let r = (radius * f64::from(view.scale)) as f32;
                    let out = at - c;
                    let reach = out.length();
                    let nearest = if reach > 0.0 {
                        c + out * (r / reach)
                    } else {
                        c + egui::vec2(r, 0.0)
                    };
                    vec![point(Part::Ring, nearest), centre]
                }
            }
        }
    }
}

/// The view that shows the whole drawing on the canvas, with its margins; None when there is
/// nothing to show.
pub(super) fn fitted(curves: &[Curve]) -> Option<View> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut grow = |p: [f64; 2], reach: f64| {
        for axis in 0..2 {
            lo[axis] = lo[axis].min(p[axis] - reach);
            hi[axis] = hi[axis].max(p[axis] + reach);
        }
    };
    for curve in curves.iter().filter(|c| c.shape.live()) {
        match curve.shape {
            Shape::Line { start, end } => {
                grow(start, 0.0);
                grow(end, 0.0);
            }
            Shape::Circle { center, radius } => grow(center, radius),
        }
    }
    if !(lo.iter().chain(&hi).all(|v| v.is_finite())) {
        return None;
    }
    let extent = [(hi[0] - lo[0]).max(1e-5), (hi[1] - lo[1]).max(1e-5)];
    let room = egui::vec2(
        SIZE.x - 2. * MARGIN_SIDE,
        SIZE.y - MARGIN_TOP - MARGIN_BOTTOM,
    );
    let scale = ((f64::from(room.x) / extent[0]).min(f64::from(room.y) / extent[1]))
        .clamp(f64::from(MIN_SCALE), f64::from(MAX_SCALE)) as f32;
    let per_mm = f64::from(scale);
    let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    // The box's middle is the middle of the room left between the two edges' words (the
    // canvas's lower left corner is the view's corner; +Y is up).
    let below = f64::from(MARGIN_BOTTOM) + f64::from(room.y) / 2.0;
    Some(View {
        minimum: [
            middle[0] - f64::from(SIZE.x) / 2.0 / per_mm,
            middle[1] - below / per_mm,
        ],
        scale,
    })
}

/// The canvas and what belongs to looking at it: the view, the held pan, the held press, the
/// target the person chose. It goes with the form, so with its tab; nothing is saved.
#[derive(Debug, Default)]
pub(crate) struct Canvas {
    view: Option<View>,
    nav: Navigation,
    target: Target,
    /// A primary press on the canvas that may still become a click.
    press: Option<egui::Pos2>,
    /// Which part of a Circle was last picked, so the highlight can tell its centre from its
    /// circle. Only said for the Circle that is still the form's selection.
    circle_part: Option<(StableEntityId, Part)>,
    /// Where the canvas was last drawn: the tests' way to aim at the document.
    #[cfg(test)]
    drawn_at: Option<egui::Rect>,
}

impl Canvas {
    /// A pan of the view is held: the window is held with it. It edits nothing.
    pub(crate) fn panning(&self) -> bool {
        self.nav.panning()
    }
    /// See `Navigation::forget_undrawn`.
    pub(crate) fn forget_undrawn(&mut self, frame: u64) {
        self.nav.forget_undrawn(frame);
    }
    #[cfg(test)]
    pub(crate) fn target(&self) -> Target {
        self.target
    }
    #[cfg(test)]
    pub(crate) fn view(&self) -> Option<(egui::Vec2, f32)> {
        self.view.map(|v| {
            (
                egui::vec2(v.minimum[0] as f32, v.minimum[1] as f32),
                v.scale,
            )
        })
    }
    #[cfg(test)]
    pub(crate) fn pressed(&self) -> bool {
        self.press.is_some()
    }
    /// The rectangle the canvas was last drawn in.
    #[cfg(test)]
    pub(crate) fn rect(&self) -> Option<egui::Rect> {
        self.drawn_at
    }
    /// Where the document point `p` (mm) is on the screen now.
    #[cfg(test)]
    pub(crate) fn screen(&self, p: [f64; 2]) -> Option<egui::Pos2> {
        Some(self.view?.screen(self.drawn_at?, p))
    }
    #[cfg(test)]
    pub(crate) fn set_target(&mut self, target: Target) {
        self.target = target;
    }
    /// What a click at `at` would pick in the view now, for the tests that aim a click.
    #[cfg(test)]
    pub(crate) fn would_pick(
        &self,
        curves: &[Curve],
        target: Target,
        at: egui::Pos2,
    ) -> Option<Hit> {
        let (view, rect) = (self.view?, self.drawn_at?);
        pick(curves, target, &view, rect, rect, at)
    }
    /// The part of a Circle the last click on `curve` took, if that was the last pick.
    pub(crate) fn circle_part(&self, curve: StableEntityId) -> Option<Part> {
        self.circle_part
            .filter(|(id, _)| *id == curve)
            .map(|(_, part)| part)
    }
    /// A click the form took: remember which part of a Circle it was.
    pub(crate) fn took(&mut self, picked: Picked) {
        self.circle_part = matches!(picked.hit.part, Part::Ring | Part::Centre)
            .then_some((picked.hit.curve, picked.hit.part));
    }

    /// Draws the canvas, the target buttons and Fit, and takes the frame's pointer events in
    /// order. Returns the clicks that ended on the canvas in this frame; the form applies
    /// them. `queued`: a request of the form waits to be taken. `pairs`: the form has pair
    /// slots to fill.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        curves: &[Curve],
        marks: &Marks,
        queued: bool,
        pairs: bool,
    ) -> Vec<Picked> {
        let open = ui.is_enabled() && !queued;
        if !pairs && matches!(self.target, Target::PairA | Target::PairB) {
            self.target = Target::Object;
        }
        if self.view.is_none() {
            self.view = fitted(curves);
        }
        let events = ui.input(|i| i.events.clone());
        let (response, painter) = ui.allocate_painter(SIZE, egui::Sense::click_and_drag());
        let (rect, visible) = (response.rect, response.rect.intersect(ui.clip_rect()));
        #[cfg(test)]
        {
            self.drawn_at = Some(rect);
        }
        self.nav.begin(ui, open);
        // Losing the press's surface cancels the click, even if it becomes
        // available again before release. Never resume a press taken by a cover.
        if self
            .press
            .is_some_and(|at| !open || !pointer_over(ui, visible, at))
        {
            self.press = None;
        }
        let mut working = self.view;
        let mut clicks = Vec::new();
        for event in &events {
            if let Some(view) = working.as_mut()
                && self.nav.event(
                    &Surface {
                        ui,
                        rect,
                        visible,
                        open,
                        frozen: self.press.is_some(),
                    },
                    event,
                    view,
                )
            {
                // A reserved press (Shift or the middle button) is navigation's, whether or
                // not it could begin a pan: it never becomes a click.
                self.press = None;
                continue;
            }
            match *event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => {
                    self.press = (open && pointer_over(ui, visible, pos)).then_some(pos);
                }
                egui::Event::PointerMoved(pos) => {
                    if self.press.is_some_and(|p| p.distance(pos) > CLICK_POINTS) {
                        self.press = None;
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    if let Some(press) = self.press.take()
                        && open
                        && pointer_over(ui, visible, pos)
                        && press.distance(pos) <= CLICK_POINTS
                        && let Some(view) = self.view
                        && let Some(hit) = pick(curves, self.target, &view, rect, visible, press)
                    {
                        clicks.push(Picked {
                            target: self.target,
                            hit,
                        });
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } if self.press.is_some() => {
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                    self.press = None;
                }
                egui::Event::WindowFocused(false) | egui::Event::PointerGone => {
                    self.press = None;
                }
                _ => {}
            }
        }
        self.nav.finish(ui);
        // A lost release, a busy form or a lost focus must not leave a press to be taken for a
        // click by a later release.
        if !open || !ui.input(|i| i.focused) || !ui.input(|i| i.pointer.primary_down()) {
            self.press = None;
        }
        // The copy replaces the view at the end of the frame, unless a press is held by then.
        if self.press.is_none() {
            self.view = working;
        }
        let hovered = response
            .hover_pos()
            .filter(|at| {
                open && self.press.is_none()
                    && !self.nav.panning()
                    && pointer_over(ui, visible, *at)
            })
            .and_then(|at| {
                let view = self.view?;
                pick(curves, self.target, &view, rect, visible, at).map(|hit| (hit, at))
            });
        self.paint(&painter, rect, curves, marks, hovered, open);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.;
            ui.spacing_mut().button_padding = egui::vec2(4., 1.);
            for target in Target::ALL {
                let usable = pairs || !matches!(target, Target::PairA | Target::PairB);
                let button = ui
                    .add_enabled(
                        open && usable,
                        egui::Button::selectable(self.target == target, target.label()),
                    )
                    .on_hover_text(target.words());
                if button.clicked() {
                    self.target = target;
                }
            }
            let fit = ui
                .add_enabled(
                    open && self.press.is_none() && !self.nav.panning(),
                    egui::Button::new("Fit"),
                )
                .on_hover_text("Show the whole drawing again.");
            if fit.clicked()
                && let Some(view) = fitted(curves)
            {
                self.view = Some(view);
            }
        });
        clicks
    }

    fn paint(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        curves: &[Curve],
        marks: &Marks,
        hovered: Option<(Hit, egui::Pos2)>,
        open: bool,
    ) {
        let Some(view) = self.view else {
            painter.rect_filled(rect, 0., egui::Color32::from_gray(25));
            standing(
                painter,
                rect.left_top() + egui::vec2(5., 5.),
                "Nothing to show",
            );
            return;
        };
        paint_grid(painter, rect, view.minimum, view.scale);
        let at = |p: [f64; 2]| view.screen(rect, p);
        for curve in curves.iter().filter(|c| c.shape.live()) {
            let is = |id: Option<StableEntityId>| id == Some(curve.id);
            let stroke = |colour, width| egui::Stroke::new(width, colour);
            let mut strokes = vec![stroke(DRAWING_COLOUR, 1.5)];
            if is(marks.selected) {
                strokes.push(stroke(SELECTED_COLOUR, 3.5));
            }
            if is(marks.pair[0]) {
                strokes.push(stroke(SIDE_A_COLOUR, 3.5));
            }
            if is(marks.pair[1]) {
                strokes.push(stroke(SIDE_B_COLOUR, 3.5));
            }
            if hovered.is_some_and(|(hit, _)| hit.curve == curve.id) {
                strokes.push(stroke(HOVER_COLOUR, 2.));
            }
            // The latest stroke is the one on top: selection, then A, then B, then hover.
            for stroke in strokes {
                match curve.shape {
                    Shape::Line { start, end } => {
                        painter.line_segment([at(start), at(end)], stroke);
                    }
                    Shape::Circle { center, radius } => {
                        painter.circle_stroke(
                            at(center),
                            (radius * f64::from(view.scale)) as f32,
                            stroke,
                        );
                    }
                }
            }
            self.paint_marks(painter, &view, rect, curve, marks);
        }
        if let Some((hit, pointer)) = hovered {
            let name = curve_name(curves, hit.curve);
            let words = match hit.part {
                Part::Line => name,
                part => format!("{name} · {}", part.word()),
            };
            paint_pointer_label(painter, rect, pointer, &words, HOVER_COLOUR);
        }
        standing(
            painter,
            rect.left_top() + egui::vec2(5., 4.),
            &self.top_words(curves, marks),
        );
        standing_at_bottom(painter, rect, "Accepted · solved");
        navigation::paint_hint(painter, rect, self.hint(open));
    }

    /// The marks drawn on one curve: its ends in Endpoint mode, the pin end of the selected
    /// Line, a Circle's centre, the A/B letters and the direction of each pair vector.
    fn paint_marks(
        &self,
        painter: &egui::Painter,
        view: &View,
        rect: egui::Rect,
        curve: &Curve,
        marks: &Marks,
    ) {
        let at = |p: [f64; 2]| view.screen(rect, p);
        match curve.shape {
            Shape::Line { start, end } => {
                if self.target == Target::Endpoint {
                    for (word, p) in [("S", start), ("E", end)] {
                        painter.circle_filled(at(p), 2.5, DRAWING_COLOUR);
                        painter.text(
                            at(p) + egui::vec2(4., -4.),
                            egui::Align2::LEFT_BOTTOM,
                            word,
                            egui::FontId::proportional(10.),
                            egui::Color32::from_gray(150),
                        );
                    }
                    if marks.selected == Some(curve.id) {
                        let (p, word) = match marks.pin {
                            LineEndpoint::Start => (start, "Start"),
                            LineEndpoint::End => (end, "End"),
                        };
                        painter.circle_filled(at(p), 5., SELECTED_COLOUR);
                        painter.circle_stroke(
                            at(p),
                            PICK_POINTS,
                            egui::Stroke::new(1.5, SELECTED_COLOUR),
                        );
                        painter.text(
                            at(p) + egui::vec2(8., 8.),
                            egui::Align2::LEFT_TOP,
                            word,
                            egui::FontId::proportional(11.),
                            SELECTED_COLOUR,
                        );
                    }
                }
                for (slot, (letter, colour)) in [("A", SIDE_A_COLOUR), ("B", SIDE_B_COLOUR)]
                    .into_iter()
                    .enumerate()
                {
                    if marks.pair[slot] != Some(curve.id) {
                        continue;
                    }
                    // Which way the Line runs as a directed-angle vector: from the named end
                    // toward the other, as the form's A:/B: buttons say.
                    let (from, to) = match marks.from[slot] {
                        LineEndpoint::Start => (start, end),
                        LineEndpoint::End => (end, start),
                    };
                    let (a, b) = (at(from), at(to));
                    let along = (b - a).normalized();
                    if along.is_finite() {
                        let tip = b - along * 3.;
                        let side = egui::vec2(-along.y, along.x) * 4.;
                        painter.add(egui::Shape::convex_polygon(
                            vec![tip, tip - along * 8. + side, tip - along * 8. - side],
                            colour,
                            egui::Stroke::NONE,
                        ));
                    }
                    painter.text(
                        a + (b - a) * 0.5 + egui::vec2(5., -5.),
                        egui::Align2::LEFT_BOTTOM,
                        letter,
                        egui::FontId::proportional(12.),
                        colour,
                    );
                }
            }
            Shape::Circle { center, radius } => {
                let c = at(center);
                let stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
                painter.line_segment([c - egui::vec2(5., 0.), c + egui::vec2(5., 0.)], stroke);
                painter.line_segment([c - egui::vec2(0., 5.), c + egui::vec2(0., 5.)], stroke);
                if marks.selected == Some(curve.id)
                    && self.circle_part(curve.id) == Some(Part::Centre)
                {
                    painter.circle_stroke(c, PICK_POINTS, egui::Stroke::new(2., SELECTED_COLOUR));
                }
                for (slot, (letter, colour)) in [("A", SIDE_A_COLOUR), ("B", SIDE_B_COLOUR)]
                    .into_iter()
                    .enumerate()
                {
                    if marks.pair[slot] == Some(curve.id) {
                        let top = c - egui::vec2(0., (radius * f64::from(view.scale)) as f32);
                        painter.text(
                            top + egui::vec2(4., -2.),
                            egui::Align2::LEFT_BOTTOM,
                            letter,
                            egui::FontId::proportional(12.),
                            colour,
                        );
                    }
                }
            }
        }
    }

    /// The line on the top edge: the pair slots while a pair slot is the target, else the
    /// form's selection.
    fn top_words(&self, curves: &[Curve], marks: &Marks) -> String {
        match self.target {
            Target::PairA | Target::PairB => pair_words(curves, marks),
            Target::Object | Target::Endpoint => self.selection_words(curves, marks),
        }
    }

    /// The form's selection, with the part of it that was picked.
    fn selection_words(&self, curves: &[Curve], marks: &Marks) -> String {
        let Some(id) = marks.selected else {
            return "Selected: none".to_owned();
        };
        let name = curve_name(curves, id);
        match curves.iter().find(|c| c.id == id).map(|c| c.shape) {
            Some(Shape::Circle { .. }) => format!(
                "Selected: {name} · {}",
                self.circle_part(id).unwrap_or(Part::Ring).word()
            ),
            Some(Shape::Line { .. }) if self.target == Target::Endpoint => {
                let end = match marks.pin {
                    LineEndpoint::Start => Part::Start,
                    LineEndpoint::End => Part::End,
                };
                format!("Selected: {name} · {}", end.word())
            }
            _ => format!("Selected: {name}"),
        }
    }

    fn hint(&self, open: bool) -> &'static str {
        if self.nav.panning() {
            "Panning · Esc puts the view back"
        } else if !open {
            "View locked while the form is busy"
        } else {
            "Wheel: zoom · Shift+drag: pan"
        }
    }
}

/// "Line 3f2c1a8b0e11" or "Circle 3f2c1a8b0e11": what the form's lists call the curve.
fn curve_name(curves: &[Curve], id: StableEntityId) -> String {
    let kind = match curves.iter().find(|c| c.id == id).map(|c| c.shape) {
        Some(Shape::Circle { .. }) => "Circle",
        _ => "Line",
    };
    format!("{kind} {}", crate::constraints::short(id))
}

/// "A: Line … Start>End · B: none": the pair slots, with the direction each vector runs.
fn pair_words(curves: &[Curve], marks: &Marks) -> String {
    let slot = |letter: &str, i: usize| match marks.pair[i] {
        None => format!("{letter}: none"),
        Some(id) => match curves.iter().find(|c| c.id == id).map(|c| c.shape) {
            Some(Shape::Line { .. }) => format!(
                "{letter}: {} {}>{}",
                curve_name(curves, id),
                marks.from[i].as_str(),
                marks.from[i].other().as_str()
            ),
            _ => format!("{letter}: {}", curve_name(curves, id)),
        },
    };
    format!("{} · {}", slot("A", 0), slot("B", 1))
}

fn standing(painter: &egui::Painter, corner: egui::Pos2, words: &str) {
    let galley = painter.layout_no_wrap(
        words.into(),
        egui::FontId::proportional(11.),
        egui::Color32::from_gray(225),
    );
    painter.rect_filled(
        egui::Rect::from_min_size(corner, galley.size()).expand(2.),
        2.,
        egui::Color32::from_black_alpha(190),
    );
    painter.galley(corner, galley, egui::Color32::from_gray(225));
}

fn standing_at_bottom(painter: &egui::Painter, rect: egui::Rect, words: &str) {
    let galley = painter.layout_no_wrap(
        words.into(),
        egui::FontId::proportional(10.),
        egui::Color32::from_gray(170),
    );
    let corner = rect.left_bottom() + egui::vec2(5., -galley.size().y - 5.);
    painter.rect_filled(
        egui::Rect::from_min_size(corner, galley.size()).expand(2.),
        2.,
        egui::Color32::from_black_alpha(170),
    );
    painter.galley(corner, galley, egui::Color32::from_gray(170));
}
