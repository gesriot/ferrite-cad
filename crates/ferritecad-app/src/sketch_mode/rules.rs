// SPDX-License-Identifier: MIT
//! Selection order, measured dimensions and button availability have no screen dependencies.
use ferritecad_document::{
    Expression, LineLengthMm, Point2, SketchConstraint, SketchConstraintRule as Rule, SketchCurve,
    SketchGeometry as G, SketchPointRef as P, SketchPointSelector as S, SketchSegmentRef,
};
use ferritecad_types::StableEntityId as Id;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Selection {
    Curve(Id),
    Point(P),
    Constraint(Id),
}

impl Selection {
    pub fn whole(self) -> Option<Id> {
        match self {
            Self::Curve(id)
            | Self::Point(P {
                curve: id,
                at: S::At,
            }) => Some(id),
            Self::Point(_) | Self::Constraint(_) => None,
        }
    }
}

pub(super) fn handles(curve: &SketchCurve) -> Vec<(P, Point2)> {
    let at = |s, p| (P::new(curve.id, s), p);
    match curve.geometry {
        G::Line { start, end } => vec![at(S::Start, start), at(S::End, end)],
        G::Point { at: p } => vec![at(S::At, p)],
        G::Circle { center, .. } => vec![at(S::Center, center)],
        _ => vec![],
    }
}

/// The point and every point that a chain of `Coincident` rules keeps in its place.
pub(super) fn coincident_with(constraints: &[SketchConstraint], point: P) -> Vec<P> {
    let mut group = vec![point];
    let mut next = 0;
    while let Some(&p) = group.get(next) {
        next += 1;
        for c in constraints {
            if let Rule::Coincident { a, b } = c.rule {
                let other = if a == p {
                    b
                } else if b == p {
                    a
                } else {
                    continue;
                };
                if !group.contains(&other) {
                    group.push(other);
                }
            }
        }
    }
    group
}

pub(super) fn position(curves: &[SketchCurve], point: P) -> Option<Point2> {
    handles(curves.iter().find(|c| c.id == point.curve)?)
        .into_iter()
        .find(|(p, _)| *p == point)
        .map(|(_, at)| at)
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(super) enum Button {
    Coincident,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Equal,
    Collinear,
    Fix,
    Distance,
    Radius,
    Diameter,
    Angle,
}

pub(super) const NO_SOLVER: &str =
    "This build has no sketch solver, so constraints cannot be added.";
pub(super) const READ_ONLY: &str = "This value is an expression; edit it in Parameters.";

impl Button {
    pub const ALL: [Self; 12] = [
        Self::Coincident,
        Self::Horizontal,
        Self::Vertical,
        Self::Parallel,
        Self::Perpendicular,
        Self::Equal,
        Self::Collinear,
        Self::Fix,
        Self::Distance,
        Self::Radius,
        Self::Diameter,
        Self::Angle,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Coincident => "Coincident",
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
            Self::Parallel => "Parallel",
            Self::Perpendicular => "Perpendicular",
            Self::Equal => "Equal",
            Self::Collinear => "Collinear",
            Self::Fix => "Fix",
            Self::Distance => "Distance",
            Self::Radius => "Radius",
            Self::Diameter => "Diameter",
            Self::Angle => "Angle",
        }
    }
    pub fn rule(
        self,
        curves: &[SketchCurve],
        selection: &[Selection],
        available: bool,
    ) -> Result<Rule, String> {
        if !available {
            return Err(NO_SOLVER.into());
        }
        let line = |id| {
            let c = curves.iter().find(|c| c.id == id)?;
            matches!(c.geometry, G::Line { .. })
                .then(|| SketchSegmentRef::new(P::new(id, S::Start), P::new(id, S::End)))
        };
        let points = || match selection {
            [Selection::Point(a), Selection::Point(b)] => Ok((*a, *b)),
            [Selection::Curve(id)] if self != Self::Coincident => line(*id)
                .map(|s| (s.from, s.to))
                .ok_or("Select two points or one line."),
            _ if self == Self::Coincident => Err("Select two points."),
            _ => Err("Select two points or one line."),
        };
        let lines = || {
            match selection {
                [Selection::Curve(a), Selection::Curve(b)] => line(*a).zip(line(*b)),
                _ => None,
            }
            .ok_or("Select two lines.")
        };
        let at = |p| position(curves, p).ok_or("That point is no longer in the sketch.");
        let expr = |v| Expression::constant(v).map_err(|e| e.to_string());
        Ok(match self {
            Self::Fix => {
                let [Selection::Point(p)] = selection else {
                    return Err("Select one point.".into());
                };
                let pxy = at(*p)?;
                Rule::Fixed {
                    point: *p,
                    x: expr(pxy.x)?,
                    y: expr(pxy.y)?,
                }
            }
            Self::Coincident => {
                let (a, b) = points()?;
                Rule::Coincident { a, b }
            }
            Self::Horizontal => {
                let (a, b) = points()?;
                Rule::Horizontal { a, b }
            }
            Self::Vertical => {
                let (a, b) = points()?;
                Rule::Vertical { a, b }
            }
            Self::Distance => {
                let (a, b) = points()?;
                let (p, q) = (at(a)?, at(b)?);
                let value =
                    LineLengthMm::new((p.x - q.x).hypot(p.y - q.y)).map_err(|e| e.to_string())?;
                Rule::Distance {
                    a,
                    b,
                    distance: expr(value.get())?,
                }
            }
            Self::Parallel => {
                let (a, b) = lines()?;
                Rule::Parallel { a, b }
            }
            Self::Perpendicular => {
                let (a, b) = lines()?;
                Rule::Perpendicular { a, b }
            }
            Self::Equal => {
                let (a, b) = lines()?;
                Rule::EqualLength { a, b }
            }
            Self::Collinear => {
                let (a, b) = lines()?;
                Rule::Collinear { a, b }
            }
            Self::Angle => {
                let (a, b) = lines()?;
                let vector = |s: SketchSegmentRef| -> Result<_, String> {
                    let (p, q) = (at(s.from)?, at(s.to)?);
                    Ok((q.x - p.x, q.y - p.y))
                };
                let ((x, y), (u, v)) = (vector(a)?, vector(b)?);
                let turn = (x * v - y * u).atan2(x * u + y * v);
                let radians = if turn == -std::f64::consts::PI {
                    -turn
                } else {
                    turn
                };
                Rule::DirectedAngle {
                    a,
                    b,
                    angle: expr(radians)?,
                }
            }
            Self::Radius | Self::Diameter => {
                let circle = match selection {
                    [Selection::Curve(id)] => curves.iter().find(|c| c.id == *id),
                    _ => None,
                };
                let Some(SketchCurve {
                    id,
                    geometry: G::Circle { radius, .. },
                    ..
                }) = circle
                else {
                    return Err("Select one circle.".into());
                };
                Rule::Radius {
                    curve: *id,
                    radius: expr(*radius)?,
                }
            }
        })
    }
}

pub(super) fn set_number(rule: &mut Rule, text: &str, diameter: bool) -> Result<(), String> {
    use crate::constraints::{parse_angle, parse_length, parse_radius};
    let (slot, value) = match rule {
        Rule::Distance { distance, .. } => (distance, parse_length(text)?.get()),
        Rule::Radius { radius, .. } => (radius, parse_radius(text, diameter)?.get()),
        Rule::DirectedAngle { angle, .. } => (angle, parse_angle(text)?.radians()),
        _ => return Err("Select a dimension.".into()),
    };
    *slot = Expression::constant(value).map_err(|e| e.to_string())?;
    Ok(())
}
