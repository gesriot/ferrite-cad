// SPDX-License-Identifier: MIT
//! §31N: the saved Horizontal, Vertical, Fixed, EqualLength, Parallel, Perpendicular and Collinear
//! rules of a constraints form, and a single Circle's Fixed centre, as markers on the accepted solved
//! drawing: which saved rules are markers, what a marker says, where it stands and what a pointer
//! reaches. Pure: the form owns the selection and the request; nothing here edits or reads a file.
//!
//! A marker **is** its saved rule: the rule's UUID, its own kind and the references exactly as the rule
//! stores them. A pair is never one curve, and a reversed `SketchSegmentRef` is never rewritten. The
//! lengths, the radius and the directed angles keep their own labels (`dimension`); a Coincident
//! closure link is nothing a person edits and is no marker.
//!
//! Where a marker stands is a pure function of the references and the view: no search, no stored
//! position, no layout. Markers may overlap; the rule stored first is on top and is the one a pointer
//! reaches, and the persisted rows of the form reach every rule.

use super::accepted::{Curve, Shape};
use super::dimension::{PAD, band, whole_line, within};
use super::navigation::View;
use ferritecad_document::{
    LineEndpoint, Sketch, SketchConstraintEdits, SketchConstraintRule, SketchCoordinateMm,
    SketchGeometry, SketchPointRef, SketchPointSelector, SketchSegmentRef,
};
use ferritecad_types::StableEntityId;

/// Distance between a marker's box and the point it stands beside, in logical points.
const GAP: f32 = 3.;

/// The seven families that have a marker. Its own kind: not a curve's, not a dimension's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Horizontal,
    Vertical,
    Fixed,
    EqualLength,
    Parallel,
    Perpendicular,
    Collinear,
}

impl Kind {
    /// The rule's name in the list of persisted constraints and in the hover.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
            Self::Fixed => "Fixed",
            Self::EqualLength => "Equal length",
            Self::Parallel => "Parallel",
            Self::Perpendicular => "Perpendicular",
            Self::Collinear => "Collinear",
        }
    }

    /// The marker's words: ASCII, so the canvas font always has every glyph.
    pub(crate) fn short(self) -> &'static str {
        match self {
            Self::Horizontal => "H",
            Self::Vertical => "V",
            Self::Fixed => "Fix",
            Self::EqualLength => "Eq",
            Self::Parallel => "Par",
            Self::Perpendicular => "Perp",
            Self::Collinear => "Col",
        }
    }
}

/// What a rule names, exactly as it stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refs {
    /// Horizontal or Vertical: two points of one Line, in the stored order.
    Line(SketchPointRef, SketchPointRef),
    /// Fixed: one point, a Line's Start or End or a Circle's Center.
    Point(SketchPointRef),
    /// EqualLength, Parallel, Perpendicular, Collinear: two whole Lines, each segment as stored (its
    /// `from` and `to` are kept the way they were written, reversed or not).
    Pair(SketchSegmentRef, SketchSegmentRef),
}

impl Refs {
    /// Every point the rule names, in the order it names them.
    pub(crate) fn points(&self) -> Vec<SketchPointRef> {
        match *self {
            Self::Line(a, b) => vec![a, b],
            Self::Point(p) => vec![p],
            Self::Pair(a, b) => vec![a.from, a.to, b.from, b.to],
        }
    }
}

/// One saved rule that has a marker: its UUID is its identity; the kind and the references refer to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mark {
    pub(crate) id: StableEntityId,
    pub(crate) kind: Kind,
    pub(crate) refs: Refs,
}

impl Mark {
    /// Whether the rule names a point or a segment of this curve.
    pub(crate) fn names(&self, curve: StableEntityId) -> bool {
        self.refs.points().iter().any(|p| p.curve == curve)
    }

    /// What the rule names, in words: the Lines or the point, as stored.
    pub(crate) fn refs_words(&self, curve_name: &dyn Fn(StableEntityId) -> String) -> String {
        let segment = |s: SketchSegmentRef| {
            format!(
                "{} {}>{}",
                curve_name(s.from.curve),
                s.from.at.as_str(),
                s.to.at.as_str()
            )
        };
        match self.refs {
            Refs::Line(a, _) => curve_name(a.curve),
            Refs::Point(p) => format!("{} {}", curve_name(p.curve), p.at.as_str()),
            Refs::Pair(a, b) => format!("{} · {}", segment(a), segment(b)),
        }
    }
}

/// A rule's valid whole Line: the two points are the Start and the End of one Line of the Sketch.
fn line_of(held: &dyn Fn(StableEntityId) -> bool, a: SketchPointRef, b: SketchPointRef) -> bool {
    whole_line(a, b).is_some_and(held)
}

/// The saved markers of `stored` in the order of its rules: for a Line polygon every Horizontal,
/// Vertical, Fixed endpoint, EqualLength, Parallel, Perpendicular and Collinear rule that names whole
/// Lines of the Sketch; for a profile that is one Circle its Fixed centre. A rule of any other shape,
/// every other family, an annulus and a mixed profile have no marker here.
pub(crate) fn saved(stored: &Sketch) -> Vec<Mark> {
    let lines = !stored.curves.is_empty()
        && stored
            .curves
            .iter()
            .all(|c| matches!(c.geometry, SketchGeometry::Line { .. }));
    let circle = match stored.curves.as_slice() {
        [c] if matches!(c.geometry, SketchGeometry::Circle { .. }) => Some(c.id),
        _ => None,
    };
    let held = |id: StableEntityId| stored.curves.iter().any(|c| c.id == id);
    let pair = |a: SketchSegmentRef, b: SketchSegmentRef| {
        lines
            && matches!(
                (whole_line(a.from, a.to), whole_line(b.from, b.to)),
                (Some(x), Some(y)) if x != y && held(x) && held(y)
            )
    };
    stored
        .constraints
        .iter()
        .filter_map(|k| {
            let (kind, refs) = match k.rule.clone() {
                SketchConstraintRule::Horizontal { a, b } if lines && line_of(&held, a, b) => {
                    (Kind::Horizontal, Refs::Line(a, b))
                }
                SketchConstraintRule::Vertical { a, b } if lines && line_of(&held, a, b) => {
                    (Kind::Vertical, Refs::Line(a, b))
                }
                SketchConstraintRule::Fixed { point, x, y }
                    if lines
                        && LineEndpoint::of(point.at).is_some()
                        && held(point.curve)
                        && SketchCoordinateMm::new(x.value()).is_ok()
                        && SketchCoordinateMm::new(y.value()).is_ok() =>
                {
                    (Kind::Fixed, Refs::Point(point))
                }
                SketchConstraintRule::Fixed { point, .. }
                    if circle == Some(point.curve) && point.at == SketchPointSelector::Center =>
                {
                    (Kind::Fixed, Refs::Point(point))
                }
                SketchConstraintRule::EqualLength { a, b } if pair(a, b) => {
                    (Kind::EqualLength, Refs::Pair(a, b))
                }
                SketchConstraintRule::Parallel { a, b } if pair(a, b) => {
                    (Kind::Parallel, Refs::Pair(a, b))
                }
                SketchConstraintRule::Perpendicular { a, b } if pair(a, b) => {
                    (Kind::Perpendicular, Refs::Pair(a, b))
                }
                SketchConstraintRule::Collinear { a, b } if pair(a, b) => {
                    (Kind::Collinear, Refs::Pair(a, b))
                }
                _ => return None,
            };
            Some(Mark {
                id: k.id,
                kind,
                refs,
            })
        })
        .collect()
}

/// A marker with the state of the pending request: whether its removal is prepared. Prepared only:
/// the rule stays in the drawing until Apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shown {
    pub(crate) mark: Mark,
    pub(crate) removed: bool,
}

impl Shown {
    pub(crate) fn of(edits: &SketchConstraintEdits, mark: &Mark) -> Self {
        Self {
            mark: *mark,
            removed: edits.remove.contains(&mark.id),
        }
    }

    /// The marker's words, and after an arrow what is prepared in its place.
    pub(crate) fn text(&self) -> String {
        let kind = self.mark.kind.short();
        if self.removed {
            format!("{kind} -> removed")
        } else {
            kind.to_owned()
        }
    }
}

/// The point on the screen of a stored point reference of a drawn curve, with the way a box leaves it:
/// the end of a Line away from the Line, or a Circle's centre toward the lower right.
fn point_anchor(
    point: SketchPointRef,
    curves: &[Curve],
    view: &View,
    canvas: egui::Rect,
) -> Option<(egui::Pos2, egui::Vec2)> {
    let shape = curves.iter().find(|c| c.id == point.curve)?.shape;
    if !shape.live() {
        return None;
    }
    match (shape, point.at) {
        (
            Shape::Line { start, end },
            at @ (SketchPointSelector::Start | SketchPointSelector::End),
        ) => {
            let (here, other) = if at == SketchPointSelector::Start {
                (start, end)
            } else {
                (end, start)
            };
            let (here, other) = (view.screen(canvas, here), view.screen(canvas, other));
            let out = (here - other).normalized();
            Some((
                here,
                if out.is_finite() {
                    out
                } else {
                    egui::vec2(1., 0.)
                },
            ))
        }
        (Shape::Circle { center, .. }, SketchPointSelector::Center) => {
            Some((view.screen(canvas, center), egui::vec2(1., 1.).normalized()))
        }
        _ => None,
    }
}

/// Where the box of a marker of `size` stands, or None when a curve it names cannot be drawn or the
/// point it is anchored to is not on the canvas.
///
/// Horizontal and Vertical: the middle of the Line, on the side **opposite** to the Line's length label
/// (`dimension::place` puts that one on the upper side). Fixed: just beyond the pinned end of a Line,
/// or to the lower right of a Circle's centre. A pair: the middle of the segment that joins the middles
/// of the two Lines. The box is then slid into the band the canvas's edge words leave. Nothing else is
/// considered: two boxes may overlap, and which one a pointer reaches is `reach`'s rule.
pub(super) fn place(
    mark: &Mark,
    curves: &[Curve],
    view: &View,
    canvas: egui::Rect,
    size: egui::Vec2,
) -> Option<egui::Rect> {
    if !size.is_finite() {
        return None;
    }
    let shape = |id: StableEntityId| curves.iter().find(|c| c.id == id).map(|c| c.shape);
    let beside = |anchor: egui::Pos2, away: egui::Vec2| {
        let half = 0.5 * (away.x.abs() * size.x + away.y.abs() * size.y);
        (
            anchor,
            egui::Rect::from_center_size(anchor + away * (half + GAP), size),
        )
    };
    let (anchor, rect) = match mark.refs {
        Refs::Line(a, _) => {
            let line = shape(a.curve)?;
            let Shape::Line { start, end } = line else {
                return None;
            };
            if !line.live() {
                return None;
            }
            let (s, e) = (view.screen(canvas, start), view.screen(canvas, end));
            let away = -super::dimension::upper_side(e - s);
            beside(s + (e - s) * 0.5, away)
        }
        Refs::Point(p) => {
            let (anchor, away) = point_anchor(p, curves, view, canvas)?;
            beside(anchor, away)
        }
        Refs::Pair(a, b) => {
            let (first, second) = (shape(a.from.curve)?, shape(b.from.curve)?);
            let (Shape::Line { start: a0, end: a1 }, Shape::Line { start: b0, end: b1 }) =
                (first, second)
            else {
                return None;
            };
            if !(first.live() && second.live()) {
                return None;
            }
            let middle = |p: [f64; 2], q: [f64; 2]| {
                let (p, q) = (view.screen(canvas, p), view.screen(canvas, q));
                p + (q - p) * 0.5
            };
            let (ma, mb) = (middle(a0, a1), middle(b0, b1));
            let anchor = ma + (mb - ma) * 0.5;
            (anchor, egui::Rect::from_center_size(anchor, size))
        }
    };
    (anchor.is_finite() && canvas.contains(anchor) && rect.is_finite())
        .then(|| within(rect, band(canvas)))
}

/// A marker with the box it stands in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    pub(crate) shown: Shown,
    pub(crate) rect: egui::Rect,
}

/// The markers of `shown` that stand on the canvas, in the order of `shown` (the order of the rules).
/// `text_size` measures the words of a marker; the box is that size and `PAD` on every side.
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
            let size = text_size(&s.text()) + PAD * 2.;
            place(&s.mark, curves, view, canvas, size).map(|rect| Placed { shown: *s, rect })
        })
        .collect()
}

/// The marker under `at`: the point must be on the visible canvas and inside the drawn box, edges
/// included. Where boxes overlap, the first in `placed` (the rule stored first) is the one reached; it
/// is the one drawn on top. No UUID and no hash decides anything.
pub(crate) fn reach(placed: &[Placed], visible: egui::Rect, at: egui::Pos2) -> Option<&Placed> {
    if !visible.contains(at) {
        return None;
    }
    placed.iter().find(|p| p.rect.contains(at))
}
