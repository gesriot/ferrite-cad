// SPDX-License-Identifier: MIT
//! Explicit CLI projections, never serialization of evolving document structs.
use ferritecad_document::{SketchConstraint, SketchConstraintRule, SketchGeometry, SketchPointRef};
use ferritecad_types::{ObjectId, StableEntityId};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct Point {
    curve_id: StableEntityId,
    at: &'static str,
}
impl From<SketchPointRef> for Point {
    fn from(p: SketchPointRef) -> Self {
        Self {
            curve_id: p.curve,
            at: p.at.as_str(),
        }
    }
}
#[derive(Serialize)]
struct Segment {
    from: Point,
    to: Point,
}
impl From<ferritecad_document::SketchSegmentRef> for Segment {
    fn from(s: ferritecad_document::SketchSegmentRef) -> Self {
        Self {
            from: s.from.into(),
            to: s.to.into(),
        }
    }
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Rule {
    Coincident {
        a: Point,
        b: Point,
    },
    Horizontal {
        a: Point,
        b: Point,
    },
    Vertical {
        a: Point,
        b: Point,
    },
    Fixed {
        point: Point,
        x: Number,
        y: Number,
    },
    Distance {
        a: Point,
        b: Point,
        distance: Number,
    },
    EqualLength {
        a: Segment,
        b: Segment,
    },
    Perpendicular {
        a: Segment,
        b: Segment,
    },
    Parallel {
        a: Segment,
        b: Segment,
    },
    /// A circle's own radius. Names the curve, not a point of it.
    Radius {
        curve_id: StableEntityId,
        radius: Number,
    },
    /// §31F, additive: the counter-clockwise turn from vector `a` to vector
    /// `b` in degrees, canonical in (-180, 180]. Each segment is a directed
    /// run `from -> to`, exactly as stored; unlike `parallel`/`perpendicular`
    /// the direction matters.
    DirectedAngle {
        a: Segment,
        b: Segment,
        angle: Number,
    },
    /// §31G, additive: both segments lie on one infinite line. Each is named
    /// whole, as stored; the direction of neither is part of the relation.
    Collinear {
        a: Segment,
        b: Segment,
    },
    Unknown,
}
impl Rule {
    fn display(
        r: SketchConstraintRule,
        length: ferritecad_types::Unit,
        angle_unit: ferritecad_types::Unit,
    ) -> Self {
        match r {
            SketchConstraintRule::Coincident { a, b } => Self::Coincident {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Horizontal { a, b } => Self::Horizontal {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Vertical { a, b } => Self::Vertical {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Fixed { point, x, y } => Self::Fixed {
                point: point.into(),
                x: Number::display(x.value(), length),
                y: Number::display(y.value(), length),
            },
            SketchConstraintRule::Distance { a, b, distance } => Self::Distance {
                a: a.into(),
                b: b.into(),
                distance: Number::display(distance.value(), length),
            },
            SketchConstraintRule::EqualLength { a, b } => Self::EqualLength {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Perpendicular { a, b } => Self::Perpendicular {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Parallel { a, b } => Self::Parallel {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Radius { curve, radius } => Self::Radius {
                curve_id: curve,
                radius: Number::display(radius.value(), length),
            },
            SketchConstraintRule::Collinear { a, b } => Self::Collinear {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::DirectedAngle { a, b, angle } => Self::DirectedAngle {
                a: a.into(),
                b: b.into(),
                angle: Number::display(angle.value(), angle_unit),
            },
            _ => Self::Unknown,
        }
    }
}
#[derive(Serialize)]
struct Number {
    value: f64,
    unit: &'static str,
}
impl Number {
    fn display(base: f64, unit: ferritecad_types::Unit) -> Self {
        Self {
            value: if unit == ferritecad_types::Unit::Degree {
                ferritecad_types::display_degrees(base)
            } else {
                base / unit.per_base_unit()
            },
            unit: unit.symbol(),
        }
    }
}
#[derive(Serialize)]
pub(crate) struct Constraint {
    constraint_id: StableEntityId,
    rule: Rule,
}
impl From<&SketchConstraint> for Constraint {
    fn from(c: &SketchConstraint) -> Self {
        Self {
            constraint_id: c.id,
            rule: Rule::display(
                c.rule.clone(),
                ferritecad_types::Unit::Millimeter,
                ferritecad_types::Unit::Degree,
            ),
        }
    }
}
impl Constraint {
    pub(crate) fn display(
        c: &SketchConstraint,
        length: ferritecad_types::Unit,
        angle: ferritecad_types::Unit,
    ) -> Self {
        Self {
            constraint_id: c.id,
            rule: Rule::display(c.rule.clone(), length, angle),
        }
    }
}
#[derive(Serialize)]
pub(crate) struct Curve {
    curve_id: StableEntityId,
    construction: bool,
    geometry: Geometry,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Geometry {
    Point {
        at_mm: [f64; 2],
    },
    Line {
        start_mm: [f64; 2],
        end_mm: [f64; 2],
    },
    Circle {
        center_mm: [f64; 2],
        radius_mm: f64,
    },
    Arc {
        center_mm: [f64; 2],
        radius_mm: f64,
        start_angle_deg: f64,
        end_angle_deg: f64,
    },
    Unknown,
}
impl From<&ferritecad_document::SketchCurve> for Curve {
    fn from(c: &ferritecad_document::SketchCurve) -> Self {
        use ferritecad_types::display_degrees;
        Self {
            curve_id: c.id,
            construction: c.construction,
            geometry: match c.geometry {
                SketchGeometry::Point { at } => Geometry::Point {
                    at_mm: [at.x, at.y],
                },
                SketchGeometry::Line { start, end } => Geometry::Line {
                    start_mm: [start.x, start.y],
                    end_mm: [end.x, end.y],
                },
                SketchGeometry::Circle { center, radius } => Geometry::Circle {
                    center_mm: [center.x, center.y],
                    radius_mm: radius,
                },
                SketchGeometry::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                } => Geometry::Arc {
                    center_mm: [center.x, center.y],
                    radius_mm: radius,
                    start_angle_deg: display_degrees(start_angle),
                    end_angle_deg: display_degrees(end_angle),
                },
                _ => Geometry::Unknown,
            },
        }
    }
}

#[derive(Serialize)]
pub(super) struct Conflict {
    sketch_id: ObjectId,
    constraints: Vec<Constraint>,
}
impl From<&ferritecad_eval::SketchConflict> for Conflict {
    fn from(c: &ferritecad_eval::SketchConflict) -> Self {
        Self {
            sketch_id: c.sketch(),
            constraints: c
                .constraints()
                .iter()
                .map(|c| Constraint {
                    constraint_id: c.id(),
                    rule: Rule::display(
                        c.rule().clone(),
                        ferritecad_types::Unit::Millimeter,
                        ferritecad_types::Unit::Degree,
                    ),
                })
                .collect(),
        }
    }
}
