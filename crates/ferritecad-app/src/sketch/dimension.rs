// SPDX-License-Identifier: MIT
//! §31L: the saved whole-Line length and the single Circle's radius of a constraints form, as
//! labels on the accepted solved drawing: which saved rules are such dimensions, what a label
//! says, where it stands and what a pointer reaches. Pure: the form owns the selection, the
//! typed number and the request; nothing here edits or reads a file.
//!
//! A label states the **target** of the saved rule (the number the solver is asked to meet),
//! never a length measured on the stored starting coordinates and never a preview of a
//! replacement: a replacement that is only prepared is told beside the target, and the curve
//! stays where the accepted rebuild put it until Apply.

use super::accepted::{Curve, Shape};
use super::navigation::View;
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, CircleConstraintKind, CircleRadiusMm,
    LineConstraintKind, LineEndpoint, LineLengthMm, Sketch, SketchConstraintEdits,
    SketchConstraintRule, SketchGeometry, SketchPointRef,
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
fn whole_line(a: SketchPointRef, b: SketchPointRef) -> Option<StableEntityId> {
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
            stored.constraints.iter().find_map(|k| match k.rule {
                SketchConstraintRule::Distance { a, b, distance }
                    if lines
                        && whole_line(a, b) == Some(c.id)
                        && LineLengthMm::new(distance).is_ok() =>
                {
                    Some(Dimension {
                        id: k.id,
                        curve: c.id,
                        kind: Kind::Length,
                        target_mm: distance,
                    })
                }
                SketchConstraintRule::Radius { curve, radius }
                    if circle && curve == c.id && CircleRadiusMm::new(radius).is_ok() =>
                {
                    Some(Dimension {
                        id: k.id,
                        curve: c.id,
                        kind: Kind::Radius,
                        target_mm: radius,
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

/// The region of the canvas a label may stand in.
fn band(canvas: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        canvas.min + egui::vec2(SIDE, TOP),
        canvas.max - egui::vec2(SIDE, BOTTOM),
    )
}

/// `rect` moved the least that puts it inside `band`; along an axis where it is larger than
/// the band it is aligned to the band's start.
fn within(rect: egui::Rect, band: egui::Rect) -> egui::Rect {
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
            let (along, length) = (b - a, (b - a).length());
            let up = if length > 1e-3 {
                // The two normals; the one pointing up the screen, else to the right.
                let n = egui::vec2(-along.y, along.x) / length;
                if n.y < -1e-4 || (n.y.abs() <= 1e-4 && n.x > 0.) {
                    n
                } else {
                    -n
                }
            } else {
                egui::vec2(0., -1.)
            };
            (a + along * 0.5, up)
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
