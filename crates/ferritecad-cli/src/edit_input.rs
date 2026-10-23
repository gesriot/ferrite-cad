// SPDX-License-Identifier: MIT
use ferritecad_document::{
    EditChange, Expression, ExpressionSlot, Point2, SketchConstraint, SketchConstraintRule,
    SketchGeometry, SketchPointRef, SketchPointSelector, SketchSegmentRef,
};
use ferritecad_types::{CadError, Dimension, ObjectId, Quantity, Result, StableEntityId, Unit};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Change {
    SetExpression {
        object: ObjectId,
        slot: ExpressionSlot,
        value: f64,
        unit: String,
    },
    SetCurveGeometry {
        sketch: ObjectId,
        curve: StableEntityId,
        geometry: Geometry,
    },
    AddConstraint {
        sketch: ObjectId,
        constraint: Constraint,
    },
    RemoveConstraint {
        sketch: ObjectId,
        constraint: StableEntityId,
    },
}

impl Change {
    pub(super) fn into_edit(self) -> Result<EditChange> {
        Ok(match self {
            Self::SetExpression {
                object,
                slot,
                value,
                unit,
            } => {
                EditChange::constant(object, slot, Number { value, unit }.base(slot.dimension())?)?
            }
            Self::SetCurveGeometry {
                sketch,
                curve,
                geometry,
            } => EditChange::SetCurveGeometry {
                sketch,
                curve,
                geometry: geometry.into_geometry()?,
            },
            Self::AddConstraint { sketch, constraint } => EditChange::AddConstraint {
                sketch,
                constraint: SketchConstraint {
                    id: constraint.id,
                    rule: constraint.rule.into_rule()?,
                },
            },
            Self::RemoveConstraint { sketch, constraint } => {
                EditChange::RemoveConstraint { sketch, constraint }
            }
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Number {
    value: f64,
    unit: String,
}
impl Number {
    fn base(self, dimension: Dimension) -> Result<f64> {
        let unit: Unit = self.unit.parse()?;
        if unit.dimension() != dimension {
            return Err(CadError::input(format!(
                "expected {dimension}, got {}",
                unit.dimension()
            )));
        }
        Ok(Quantity::new(self.value, unit)?.base_value())
    }
    fn expression(self, dimension: Dimension) -> Result<Expression> {
        Expression::constant(self.base(dimension)?)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Point {
    x: Number,
    y: Number,
}
impl Point {
    fn into_point(self) -> Result<Point2> {
        Point2::new(
            self.x.base(Dimension::Length)?,
            self.y.base(Dimension::Length)?,
        )
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Geometry {
    Point {
        at: Point,
    },
    Line {
        start: Point,
        end: Point,
    },
    Circle {
        center: Point,
        radius: Number,
    },
    Arc {
        center: Point,
        radius: Number,
        start_angle: Number,
        end_angle: Number,
    },
}
impl Geometry {
    fn into_geometry(self) -> Result<SketchGeometry> {
        Ok(match self {
            Self::Point { at } => SketchGeometry::Point {
                at: at.into_point()?,
            },
            Self::Line { start, end } => SketchGeometry::Line {
                start: start.into_point()?,
                end: end.into_point()?,
            },
            Self::Circle { center, radius } => SketchGeometry::Circle {
                center: center.into_point()?,
                radius: radius.base(Dimension::Length)?,
            },
            Self::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => SketchGeometry::Arc {
                center: center.into_point()?,
                radius: radius.base(Dimension::Length)?,
                start_angle: start_angle.base(Dimension::Angle)?,
                end_angle: end_angle.base(Dimension::Angle)?,
            },
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PointRef {
    curve: StableEntityId,
    at: SketchPointSelector,
}
impl From<PointRef> for SketchPointRef {
    fn from(p: PointRef) -> Self {
        Self::new(p.curve, p.at)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Segment {
    from: PointRef,
    to: PointRef,
}
impl From<Segment> for SketchSegmentRef {
    fn from(s: Segment) -> Self {
        Self::new(s.from.into(), s.to.into())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Constraint {
    id: StableEntityId,
    rule: Rule,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Rule {
    Coincident {
        a: PointRef,
        b: PointRef,
    },
    Horizontal {
        a: PointRef,
        b: PointRef,
    },
    Vertical {
        a: PointRef,
        b: PointRef,
    },
    Fixed {
        point: PointRef,
        x: Number,
        y: Number,
    },
    Distance {
        a: PointRef,
        b: PointRef,
        distance: Number,
    },
    Radius {
        curve: StableEntityId,
        radius: Number,
    },
    EqualLength {
        a: Segment,
        b: Segment,
    },
    Parallel {
        a: Segment,
        b: Segment,
    },
    Perpendicular {
        a: Segment,
        b: Segment,
    },
    Collinear {
        a: Segment,
        b: Segment,
    },
    DirectedAngle {
        a: Segment,
        b: Segment,
        angle: Number,
    },
}
impl Rule {
    fn into_rule(self) -> Result<SketchConstraintRule> {
        use SketchConstraintRule as R;
        Ok(match self {
            Self::Coincident { a, b } => R::Coincident {
                a: a.into(),
                b: b.into(),
            },
            Self::Horizontal { a, b } => R::Horizontal {
                a: a.into(),
                b: b.into(),
            },
            Self::Vertical { a, b } => R::Vertical {
                a: a.into(),
                b: b.into(),
            },
            Self::Fixed { point, x, y } => R::Fixed {
                point: point.into(),
                x: x.expression(Dimension::Length)?,
                y: y.expression(Dimension::Length)?,
            },
            Self::Distance { a, b, distance } => R::Distance {
                a: a.into(),
                b: b.into(),
                distance: distance.expression(Dimension::Length)?,
            },
            Self::Radius { curve, radius } => R::Radius {
                curve,
                radius: radius.expression(Dimension::Length)?,
            },
            Self::EqualLength { a, b } => R::EqualLength {
                a: a.into(),
                b: b.into(),
            },
            Self::Parallel { a, b } => R::Parallel {
                a: a.into(),
                b: b.into(),
            },
            Self::Perpendicular { a, b } => R::Perpendicular {
                a: a.into(),
                b: b.into(),
            },
            Self::Collinear { a, b } => R::Collinear {
                a: a.into(),
                b: b.into(),
            },
            Self::DirectedAngle { a, b, angle } => R::DirectedAngle {
                a: a.into(),
                b: b.into(),
                angle: {
                    let radians = angle.base(Dimension::Angle)?;
                    // The two half-turn spellings name the same directed angle.
                    Expression::constant(if radians == -std::f64::consts::PI {
                        std::f64::consts::PI
                    } else {
                        radians
                    })?
                },
            },
        })
    }
}
