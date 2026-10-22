// SPDX-License-Identifier: MIT
//! §31L: the saved whole-Line length and the single Circle's radius of a constraints form, as
//! labels on the accepted solved drawing: which saved rules are such dimensions, what a label
//! says, where it stands and what a pointer reaches. Pure: the form owns the selection, the
//! typed number and the request; nothing here edits or reads a file.
//!
//! §31M adds the saved DirectedAngle between two Lines as a sibling family (`Turn`): it belongs to
//! two Lines, each with a direction, so it is its own type and is told by its rule's UUID; the
//! Length/Radius model above is unchanged.
//!
//! A label states the **target** of the saved rule (the number the solver is asked to meet),
//! never a length measured on the stored starting coordinates and never a preview of a
//! replacement: a replacement that is only prepared is told beside the target, and the curve
//! stays where the accepted rebuild put it until Apply.

use super::accepted::{Curve, Shape};
use super::navigation::View;
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, CircleConstraintKind, CircleRadiusMm, DirectedAngle,
    DirectedLine, LineConstraintKind, LineEndpoint, LineLengthMm, Sketch, SketchConstraintEdits,
    SketchConstraintRule, SketchGeometry, SketchPointRef, SketchSegmentRef,
};
use ferritecad_types::StableEntityId;

/// Distance between a label's box and the curve it stands beside, in logical points.
const GAP: f32 = 3.;
/// Room kept inside the canvas for the words painted on its edges (the top line and the
/// bottom line and hint): a label stands inside the band that is left, never over them.
const SIDE: f32 = 3.;
const TOP: f32 = 20.;
const BOTTOM: f32 = 24.;
/// Between a label's text and the edge of its box.
pub(crate) const PAD: egui::Vec2 = egui::vec2(4., 2.);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The distance between the two ends of one whole Line.
    Length,
    /// The radius of the one analytic Circle.
    Radius,
}

/// One saved dimension of the form's Sketch: the UUID of the stored rule, the curve it
/// dimensions and the target in mm that the rule holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Dimension {
    /// The rule's own UUID: the dimension's identity. The curve and the kind only refer to it.
    pub(crate) id: StableEntityId,
    pub(crate) curve: StableEntityId,
    pub(crate) kind: Kind,
    pub(crate) target_mm: f64,
}

impl Dimension {
    /// The words the list of persisted constraints uses for this rule.
    pub(crate) fn name(&self) -> String {
        match self.kind {
            Kind::Length => format!("Line length {} mm", self.target_mm),
            Kind::Radius => format!("Radius {} mm", self.target_mm),
        }
    }
}

/// The Line a pair of points spans when they are its Start and End, in either order.
pub(super) fn whole_line(a: SketchPointRef, b: SketchPointRef) -> Option<StableEntityId> {
    let (x, y) = (LineEndpoint::of(a.at)?, LineEndpoint::of(b.at)?);
    (a.curve == b.curve && x != y).then_some(a.curve)
}

/// The saved dimensions of `stored` in the order of its curves: for a Line polygon each Line's
/// whole-Line Distance, for a profile that is one Circle its Radius. A rule whose number is not
/// a valid length or radius is none, and every other rule, an annulus and a mixed profile have
/// no dimension here. The catalogue is the one the form was opened with: the labels, the rows
/// of the persisted list and the replacement all read this one reading.
pub(crate) fn saved(stored: &Sketch) -> Vec<Dimension> {
    let lines = stored
        .curves
        .iter()
        .all(|c| matches!(c.geometry, SketchGeometry::Line { .. }));
    let circle = matches!(
        stored.curves.as_slice(),
        [c] if matches!(c.geometry, SketchGeometry::Circle { .. })
    );
    stored
        .curves
        .iter()
        .filter_map(|c| {
            stored
                .constraints
                .iter()
                .find_map(|k| match k.rule.clone() {
                    SketchConstraintRule::Distance { a, b, distance }
                        if lines
                            && whole_line(a, b) == Some(c.id)
                            && LineLengthMm::new(distance.value()).is_ok() =>
                    {
                        Some(Dimension {
                            id: k.id,
                            curve: c.id,
                            kind: Kind::Length,
                            target_mm: distance.value(),
                        })
                    }
                    SketchConstraintRule::Radius { curve, radius }
                        if circle
                            && curve == c.id
                            && CircleRadiusMm::new(radius.value()).is_ok() =>
                    {
                        Some(Dimension {
                            id: k.id,
                            curve: c.id,
                            kind: Kind::Radius,
                            target_mm: radius.value(),
                        })
                    }
                    _ => None,
                })
        })
        .collect()
}

/// What the pending request does to a saved dimension. Prepared only: nothing is solved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Pending {
    /// The saved rule is removed and a rule of the same kind on the same curve, with this
    /// target in mm, is added.
    Replaced(f64),
    /// The saved rule is removed and nothing takes its place.
    Removed,
}

/// The pending request's word on `dimension`: none when its rule is not removed.
pub(crate) fn pending(edits: &SketchConstraintEdits, dimension: &Dimension) -> Option<Pending> {
    if !edits.remove.contains(&dimension.id) {
        return None;
    }
    let target = edits
        .add
        .iter()
        .find_map(|add| match (*add, dimension.kind) {
            (
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve,
                    kind: LineConstraintKind::Distance(length),
                }),
                Kind::Length,
            ) if curve == dimension.curve => Some(length.get()),
            (
                AddSketchConstraint::Circle {
                    curve,
                    kind: CircleConstraintKind::Radius(radius),
                },
                Kind::Radius,
            ) if curve == dimension.curve => Some(radius.get()),
            _ => None,
        });
    Some(target.map_or(Pending::Removed, Pending::Replaced))
}

/// A dimension with the state of the pending request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shown {
    pub(crate) dimension: Dimension,
    pub(crate) pending: Option<Pending>,
}

impl Shown {
    /// The label's words: the target, and after an arrow what is prepared in its place.
    pub(crate) fn text(&self) -> String {
        let prefix = match self.dimension.kind {
            Kind::Length => "",
            Kind::Radius => "R ",
        };
        let target = compact(self.dimension.target_mm);
        match self.pending {
            None => format!("{prefix}{target} mm"),
            Some(Pending::Replaced(next)) => {
                format!("{prefix}{target} → {} mm", compact(next))
            }
            Some(Pending::Removed) => format!("{prefix}{target} mm → removed"),
        }
    }
}

/// A number short enough for a label: its shortest exact spelling when that has at most nine
/// characters, else four decimals followed by an ellipsis (or an exponent, outside the range
/// where decimals read well). The list and the form give every digit.
fn compact(value: f64) -> String {
    let exact = value.to_string();
    if exact.chars().count() <= 9 {
        return exact;
    }
    if (1e-3..1e6).contains(&value.abs()) {
        let fixed = format!("{value:.4}");
        let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
        return format!("{trimmed}…");
    }
    format!("{value:.3e}")
}

/// §31M: one saved DirectedAngle of the form's Sketch, as a dimension of a **pair**. Its identity is
/// the saved rule's own UUID; the two Lines are named in order, each with the direction its vector
/// runs, and the target is the validated `DirectedAngle`, never degrees in a millimetre field. Like
/// `Dimension` it is read once from the form's catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Turn {
    pub(crate) id: StableEntityId,
    pub(crate) a: DirectedLine,
    pub(crate) b: DirectedLine,
    pub(crate) angle: DirectedAngle,
}

impl Turn {
    /// The words the list of persisted constraints uses for this rule.
    pub(crate) fn name(&self) -> String {
        format!("Directed angle {}°", editable_degrees(self.angle))
    }

    /// Whether `add` is the addition that takes this rule's place: a DirectedAngle on the same two
    /// exact directed vectors. Any other answer about the pair is somebody else's and is left to
    /// the relation slot and the validator.
    pub(crate) fn is_replaced_by(&self, add: &AddSketchConstraint) -> bool {
        matches!(
            *add,
            AddSketchConstraint::Line(AddLineConstraint::DirectedAngle { a, b, .. })
                if a == self.a && b == self.b
        )
    }

    /// Whether the editor's displayed degrees read back as exactly this saved angle. Arbitrary
    /// stored radians need not have a representable degree input; never hide that with a tolerance.
    pub(crate) fn typeable(&self) -> bool {
        DirectedAngle::from_degrees(editable_degrees(self.angle))
            .is_ok_and(|angle| angle == self.angle)
    }

    /// The addition that replaces this rule with `angle`: the same two exact vectors.
    pub(crate) fn replaced_by(&self, angle: DirectedAngle) -> AddSketchConstraint {
        AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
            a: self.a,
            b: self.b,
            angle,
        })
    }
}

/// A degree input for the UI, verified through the existing document converter. Its inverse
/// calculation can round to the neighbour of the original input. Prefer the document's display;
/// otherwise try four adjacent floats each way, accepting only exact equality of stored radians.
/// This bounded search changes neither the converter nor CLI output. If no input is found (some
/// arbitrary stored radians have none), keep the display and let `Turn::typeable` tell the person.
pub(crate) fn editable_degrees(angle: DirectedAngle) -> f64 {
    let shown = angle.degrees();
    let matches = |value| {
        value > -180.
            && DirectedAngle::from_degrees(value).is_ok_and(|candidate| candidate == angle)
    };
    if matches(shown) {
        return shown;
    }
    let (mut lower, mut upper) = (shown, shown);
    for _ in 0..4 {
        lower = lower.next_down();
        upper = upper.next_up();
        for value in [lower, upper] {
            if matches(value) {
                return value;
            }
        }
    }
    shown
}

/// A stored segment as the vector it is: one Line, from one end to the other. Anything else (two
/// curves, a point that is not an end) is none.
fn directed(segment: SketchSegmentRef) -> Option<DirectedLine> {
    let (from, to) = (
        LineEndpoint::of(segment.from.at)?,
        LineEndpoint::of(segment.to.at)?,
    );
    if segment.from.curve != segment.to.curve {
        return None;
    }
    DirectedLine::new(segment.from.curve, from, to).ok()
}

/// The saved DirectedAngles of `stored` in the order of its rules, for a Line polygon: a rule whose
/// vectors are not two different Lines of the Sketch, or whose number is not a canonical stored
/// angle, is none. Every other rule, a Circle and a mixed profile have no angle here.
pub(crate) fn saved_turns(stored: &Sketch) -> Vec<Turn> {
    let lines = !stored.curves.is_empty()
        && stored
            .curves
            .iter()
            .all(|c| matches!(c.geometry, SketchGeometry::Line { .. }));
    if !lines {
        return Vec::new();
    }
    let held = |id: StableEntityId| stored.curves.iter().any(|c| c.id == id);
    stored
        .constraints
        .iter()
        .filter_map(|k| {
            let SketchConstraintRule::DirectedAngle { a, b, angle } = k.rule.clone() else {
                return None;
            };
            let (a, b) = (directed(a)?, directed(b)?);
            let angle = DirectedAngle::from_radians(angle.value()).ok()?;
            (a.curve() != b.curve() && held(a.curve()) && held(b.curve())).then_some(Turn {
                id: k.id,
                a,
                b,
                angle,
            })
        })
        .collect()
}

/// What the pending request does to a saved angle. Prepared only: nothing is solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnPending {
    /// The saved rule is removed and a DirectedAngle on the same two vectors, with this angle, is
    /// added.
    Replaced(DirectedAngle),
    /// The saved rule is removed and nothing on its two vectors takes its place.
    Removed,
}

/// The pending request's word on `turn`: none when its rule is not removed.
pub(crate) fn pending_turn(edits: &SketchConstraintEdits, turn: &Turn) -> Option<TurnPending> {
    if !edits.remove.contains(&turn.id) {
        return None;
    }
    let target = edits.add.iter().find_map(|add| match *add {
        AddSketchConstraint::Line(AddLineConstraint::DirectedAngle { angle, .. })
            if turn.is_replaced_by(add) =>
        {
            Some(angle)
        }
        _ => None,
    });
    Some(target.map_or(TurnPending::Removed, TurnPending::Replaced))
}

/// A saved angle with the state of the pending request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShownTurn {
    pub(crate) turn: Turn,
    pub(crate) pending: Option<TurnPending>,
}

impl ShownTurn {
    /// The label's words: the order of the vectors, the target in degrees, and after an arrow what
    /// is prepared in its place. `A>B` says the number is the turn from A to B.
    pub(crate) fn text(&self) -> String {
        let target = compact_degrees(self.turn.angle);
        match self.pending {
            None => format!("A>B {target}°"),
            Some(TurnPending::Replaced(next)) => {
                format!("A>B {target} -> {}°", compact_degrees(next))
            }
            Some(TurnPending::Removed) => format!("A>B {target}° -> removed"),
        }
    }
}

/// An angle short enough for a label, and never one that reads as another value: its shortest
/// exact spelling when that has at most nine characters, else four decimals followed by an
/// ellipsis. A value that four decimals would turn into `0`, `180` or `-180` while it is not that
/// number is written with all its digits (in scientific form when it is tiny), because a label
/// that says `180…` for 179.9999999 would show the opposite direction.
fn compact_degrees(angle: DirectedAngle) -> String {
    let value = editable_degrees(angle);
    let exact = value.to_string();
    if exact.chars().count() <= 9 {
        return exact;
    }
    let fixed = format!("{value:.4}");
    let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
    let approx = trimmed.parse::<f64>().unwrap_or(f64::NAN);
    let collapsed = (approx == 0. && value != 0.) || (approx.abs() == 180. && value.abs() != 180.);
    if !approx.is_finite() || collapsed {
        return if value.abs() < 1e-3 {
            format!("{value:e}")
        } else {
            exact
        };
    }
    format!("{trimmed}…")
}

/// The region of the canvas a label may stand in.
pub(super) fn band(canvas: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        canvas.min + egui::vec2(SIDE, TOP),
        canvas.max - egui::vec2(SIDE, BOTTOM),
    )
}

/// `rect` moved the least that puts it inside `band`; along an axis where it is larger than
/// the band it is aligned to the band's start.
pub(super) fn within(rect: egui::Rect, band: egui::Rect) -> egui::Rect {
    let shift = |lo: f32, hi: f32, from: f32, to: f32| {
        if hi - lo >= to - from || lo < from {
            from - lo
        } else if hi > to {
            to - hi
        } else {
            0.
        }
    };
    rect.translate(egui::vec2(
        shift(rect.min.x, rect.max.x, band.min.x, band.max.x),
        shift(rect.min.y, rect.max.y, band.min.y, band.max.y),
    ))
}

/// The unit normal of a Line running `along` on the screen that a length label stands on: of the two
/// normals the one pointing up the screen, else to the right; straight up for a Line too short to
/// have a direction. A §31N marker stands on the other side.
pub(super) fn upper_side(along: egui::Vec2) -> egui::Vec2 {
    let length = along.length();
    if length > 1e-3 {
        let n = egui::vec2(-along.y, along.x) / length;
        if n.y < -1e-4 || (n.y.abs() <= 1e-4 && n.x > 0.) {
            n
        } else {
            -n
        }
    } else {
        egui::vec2(0., -1.)
    }
}

/// Where the box of a label of `size` stands for a curve of the drawing, or None when the
/// curve cannot be drawn or the point it is anchored to is not on the canvas.
///
/// A Line's label stands on the upper side of the Line (the right side of a vertical one),
/// whichever way the Line is stored, at its middle; a Circle's stands outside its ring toward
/// the upper right. The box is then slid into the band of the canvas that the edge words leave.
/// Nothing else is considered: two boxes may overlap, and which one a pointer reaches is
/// `pick`'s rule.
pub(super) fn place(
    shape: &Shape,
    view: &View,
    canvas: egui::Rect,
    size: egui::Vec2,
) -> Option<egui::Rect> {
    if !shape.live() || !size.is_finite() {
        return None;
    }
    let (anchor, away) = match *shape {
        Shape::Line { start, end } => {
            let (a, b) = (view.screen(canvas, start), view.screen(canvas, end));
            let along = b - a;
            (a + along * 0.5, upper_side(along))
        }
        Shape::Circle { center, radius } => {
            let c = view.screen(canvas, center);
            let away = egui::vec2(1., -1.).normalized();
            (c + away * (radius * f64::from(view.scale)) as f32, away)
        }
    };
    if !(anchor.is_finite() && canvas.contains(anchor)) {
        return None;
    }
    let half = 0.5 * (away.x.abs() * size.x + away.y.abs() * size.y);
    let rect = egui::Rect::from_center_size(anchor + away * (half + GAP), size);
    rect.is_finite().then(|| within(rect, band(canvas)))
}

/// A label with the box it stands in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    pub(crate) shown: Shown,
    pub(crate) rect: egui::Rect,
}

/// The labels of `shown` that stand on the canvas, in the order of `shown`. `text_size` measures
/// the words of a label; the box is that size and `PAD` on every side.
pub(super) fn placed(
    shown: &[Shown],
    curves: &[Curve],
    view: &View,
    canvas: egui::Rect,
    text_size: &dyn Fn(&str) -> egui::Vec2,
) -> Vec<Placed> {
    shown
        .iter()
        .filter_map(|s| {
            let curve = curves.iter().find(|c| c.id == s.dimension.curve)?;
            let size = text_size(&s.text()) + PAD * 2.;
            place(&curve.shape, view, canvas, size).map(|rect| Placed { shown: *s, rect })
        })
        .collect()
}

/// The label under `at`: the point must be on the visible canvas and inside the box the label
/// is drawn in, edges included. Where boxes overlap, the label first in `placed` (the curve
/// stored first) is the one reached; it is the one drawn on top. The pick radius of the curves
/// plays no part: a label is exactly as large as it looks.
pub(crate) fn pick(placed: &[Placed], visible: egui::Rect, at: egui::Pos2) -> Option<&Placed> {
    if !visible.contains(at) {
        return None;
    }
    placed.iter().find(|p| p.rect.contains(at))
}

/// §31M: where the box of a pair's label of `size` stands: the middle of the segment that joins the
/// middles of the two solved Lines (centred on that point), then slid into the band of the canvas
/// that the edge words leave. None when either is not a drawable Line or the point is not on the
/// canvas. It needs the Lines to meet nowhere and no sector to be chosen between them, so 0° and 180°,
/// parallel, nearly parallel, collinear and crossing Lines all have a finite place; no angle is
/// drawn that a sector could show smaller than the signed number.
pub(super) fn place_pair(
    a: &Shape,
    b: &Shape,
    view: &View,
    canvas: egui::Rect,
    size: egui::Vec2,
) -> Option<egui::Rect> {
    let (Shape::Line { start: a0, end: a1 }, Shape::Line { start: b0, end: b1 }) = (*a, *b) else {
        return None;
    };
    if !(a.live() && b.live() && size.is_finite()) {
        return None;
    }
    let middle = |p: [f64; 2], q: [f64; 2]| {
        let (p, q) = (view.screen(canvas, p), view.screen(canvas, q));
        p + (q - p) * 0.5
    };
    let (ma, mb) = (middle(a0, a1), middle(b0, b1));
    let anchor = ma + (mb - ma) * 0.5;
    if !(anchor.is_finite() && canvas.contains(anchor)) {
        return None;
    }
    let rect = egui::Rect::from_center_size(anchor, size);
    rect.is_finite().then(|| within(rect, band(canvas)))
}

/// A pair's label with the box it stands in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlacedTurn {
    pub(crate) shown: ShownTurn,
    pub(crate) rect: egui::Rect,
}

/// The labels of `shown` that stand on the canvas, in the order of `shown` (the order of the rules).
pub(super) fn placed_turns(
    shown: &[ShownTurn],
    curves: &[Curve],
    view: &View,
    canvas: egui::Rect,
    text_size: &dyn Fn(&str) -> egui::Vec2,
) -> Vec<PlacedTurn> {
    shown
        .iter()
        .filter_map(|s| {
            let shape = |id: StableEntityId| curves.iter().find(|c| c.id == id).map(|c| &c.shape);
            let (a, b) = (shape(s.turn.a.curve())?, shape(s.turn.b.curve())?);
            let size = text_size(&s.text()) + PAD * 2.;
            place_pair(a, b, view, canvas, size).map(|rect| PlacedTurn { shown: *s, rect })
        })
        .collect()
}

/// What a point on the canvas reaches among all the labels.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Reached<'a> {
    Size(&'a Placed),
    Turn(&'a PlacedTurn),
}

/// The label under `at` among the labels of a curve (`pick`) and then those of a pair, each in its
/// saved order: the point must be on the visible canvas and inside the drawn box, edges included. The
/// first of the curves' labels is the one drawn on top, then the first angle; the persisted rows reach
/// the rest. No UUID and no hash decides anything.
pub(crate) fn reach<'a>(
    sizes: &'a [Placed],
    turns: &'a [PlacedTurn],
    visible: egui::Rect,
    at: egui::Pos2,
) -> Option<Reached<'a>> {
    if !visible.contains(at) {
        return None;
    }
    pick(sizes, visible, at).map(Reached::Size).or_else(|| {
        turns
            .iter()
            .find(|p| p.rect.contains(at))
            .map(Reached::Turn)
    })
}
