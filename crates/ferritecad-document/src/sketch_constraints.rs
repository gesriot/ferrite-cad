// SPDX-License-Identifier: MIT
//! Bounded persisted Line orientation/length/pin/equality/relation editing. No solver or wire format.
use crate::{
    Sketch, SketchConstraint, SketchConstraintRule, SketchGeometry, SketchPointRef,
    SketchPointSelector, SketchSegmentRef,
};
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

/// A finite, strictly positive Euclidean Line length in millimetres.
/// Private validated bits make exact Eq safe: NaN and both zeros are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineLengthMm(u64);
impl LineLengthMm {
    pub fn new(value: f64) -> Result<Self> {
        if !value.is_finite() || value <= 0. {
            return Err(CadError::input(
                "Line length must be finite and positive in mm",
            ));
        }
        Ok(Self(value.to_bits()))
    }
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// A finite signed sketch coordinate in millimetres. Zero is normalised to one
/// pattern so exact Eq agrees with f64 equality; NaN and infinity are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SketchCoordinateMm(u64);
impl SketchCoordinateMm {
    pub fn new(value: f64) -> Result<Self> {
        if !value.is_finite() {
            return Err(CadError::input("sketch coordinate must be finite in mm"));
        }
        Ok(Self(if value == 0. {
            0f64.to_bits()
        } else {
            value.to_bits()
        }))
    }
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// Which end of a Line a pin names. The model's `At` selector belongs to point
/// geometry, so a Line endpoint request cannot spell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndpoint {
    Start,
    End,
}
impl LineEndpoint {
    pub fn selector(self) -> SketchPointSelector {
        match self {
            Self::Start => SketchPointSelector::Start,
            Self::End => SketchPointSelector::End,
        }
    }
    pub fn of(selector: SketchPointSelector) -> Option<Self> {
        match selector {
            SketchPointSelector::Start => Some(Self::Start),
            SketchPointSelector::End => Some(Self::End),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        self.selector().as_str()
    }
    /// The other end of the same Line.
    pub fn other(self) -> Self {
        match self {
            Self::Start => Self::End,
            Self::End => Self::Start,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineConstraintKind {
    Horizontal,
    Vertical,
    Distance(LineLengthMm),
    /// One endpoint of this Line pinned at explicit millimetres.
    Fixed {
        at: LineEndpoint,
        x: SketchCoordinateMm,
        y: SketchCoordinateMm,
    },
}
/// Which relative orientation two Lines keep.
///
/// This says nothing about either Line's own direction, which is what makes it
/// different from `Horizontal`/`Vertical`: it holds on a profile at any angle,
/// and the profile is free to rotate while it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRelation {
    Parallel,
    Perpendicular,
    /// Both Lines on one infinite line (§31G): their offset is held as well
    /// as their direction. Neither Line's own direction is part of it.
    Collinear,
}
impl LineRelation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parallel => "Parallel",
            Self::Perpendicular => "Perpendicular",
            Self::Collinear => "Collinear",
        }
    }
}

/// A whole Line read as a vector: from one of its ends to the other (§31F).
///
/// The direction is part of what the user says and is never sorted away: unlike
/// the unordered Line pairs, a directed angle changes by half a turn when one
/// of its vectors is reversed. Both ends are named so a request cannot leave
/// the direction to a default; they must be the two different ends, which also
/// makes a vector of no length unspellable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectedLine {
    curve: StableEntityId,
    from: LineEndpoint,
}
impl DirectedLine {
    pub fn new(curve: StableEntityId, from: LineEndpoint, to: LineEndpoint) -> Result<Self> {
        if from == to {
            return Err(CadError::input(
                "a directed Line runs from one end to the other; this one names the same end \
                 twice, which is a vector of no length",
            ));
        }
        Ok(Self { curve, from })
    }
    pub fn curve(self) -> StableEntityId {
        self.curve
    }
    pub fn from(self) -> LineEndpoint {
        self.from
    }
    pub fn to(self) -> LineEndpoint {
        self.from.other()
    }
    fn segment(self) -> SketchSegmentRef {
        SketchSegmentRef::new(
            SketchPointRef::new(self.curve, self.from.selector()),
            SketchPointRef::new(self.curve, self.to().selector()),
        )
    }
}

pub use ferritecad_types::DirectedAngle;

/// One requested addition.
///
/// The first four families say something about one Line, so they name one. An
/// equal length and a relative orientation are relationships between two whole
/// Lines and have no leading side, so they name both: a pair cannot be spelled
/// by a single curve field without hiding one half of what the request means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddLineConstraint {
    /// This Line's own orientation, length, or the profile's single pinned endpoint.
    Line {
        curve: StableEntityId,
        kind: LineConstraintKind,
    },
    /// These two Lines of the same profile keep one Euclidean length between them.
    EqualLength {
        a: StableEntityId,
        b: StableEntityId,
    },
    /// These two Lines of the same profile keep one relative orientation.
    Relation {
        a: StableEntityId,
        b: StableEntityId,
        relation: LineRelation,
    },
    /// The second directed Line is the first turned counter-clockwise by
    /// `angle` (§31F). The same pair slot as [`Self::Relation`]: it answers the
    /// same question about the pair, with a sign and a direction.
    DirectedAngle {
        a: DirectedLine,
        b: DirectedLine,
        angle: DirectedAngle,
    },
}
/// A finite, strictly positive circle radius in millimetres.
///
/// Its own type beside [`LineLengthMm`] rather than that one reused: a length
/// between two points and the radius of a circle are different quantities of
/// the same unit, and a request that could spell one where the other belongs
/// would be one field away from giving a circle a line's dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircleRadiusMm(u64);
impl CircleRadiusMm {
    pub fn new(value: f64) -> Result<Self> {
        if !value.is_finite() || value <= 0. {
            return Err(CadError::input(
                "circle radius must be finite and positive in mm",
            ));
        }
        Ok(Self(value.to_bits()))
    }
    /// The radius of a circle whose diameter is typed: `R = D / 2`, the one
    /// conversion both the editor and the command line use.
    ///
    /// There is no Diameter constraint; only the stored `Radius` exists. The
    /// result is checked, not assumed: halving can lose precision when the
    /// resulting radius is subnormal, even if `D` itself is normal. The smallest
    /// positive `D` halves to zero. A `D` that is not exactly twice the radius it
    /// yields is refused rather than stored as a different circle.
    pub fn from_diameter_mm(diameter: f64) -> Result<Self> {
        if !diameter.is_finite() || diameter <= 0. {
            return Err(CadError::input(
                "circle diameter must be finite and positive in mm",
            ));
        }
        let radius = diameter / 2.;
        if radius * 2. != diameter {
            return Err(CadError::input(
                "circle diameter is too small to halve into a positive radius in mm",
            ));
        }
        Self::new(radius)
    }
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// What one addition says about one circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircleConstraintKind {
    /// This circle's own radius in mm.
    Radius(CircleRadiusMm),
    /// This circle's centre pinned at explicit millimetres.
    FixedCenter {
        x: SketchCoordinateMm,
        y: SketchCoordinateMm,
    },
}

/// One requested addition, whichever geometry it is about.
///
/// Split by geometry rather than widened: [`AddLineConstraint`] keeps saying
/// exactly what it always said about lines, and a circle addition cannot be
/// spelled in its vocabulary by accident. The two families never mix in one
/// sketch — a profile this editor manages is all lines or one circle — so the
/// split is also what the request is really made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddSketchConstraint {
    Line(AddLineConstraint),
    /// This circle's radius, or its pinned centre.
    Circle {
        curve: StableEntityId,
        kind: CircleConstraintKind,
    },
    /// These two circles of the same profile keep one centre.
    ///
    /// Two named circles rather than one leading circle and an implied other:
    /// concentricity is a relationship, it has no side that owns it, and a
    /// request that named one circle would be deciding on the reader's behalf
    /// which second circle it meant.
    Concentric {
        a: StableEntityId,
        b: StableEntityId,
    },
}

impl From<AddLineConstraint> for AddSketchConstraint {
    fn from(line: AddLineConstraint) -> Self {
        Self::Line(line)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SketchConstraintEdits {
    pub remove: Vec<StableEntityId>,
    pub add: Vec<AddSketchConstraint>,
}

fn endpoints(curve: StableEntityId) -> (SketchPointRef, SketchPointRef) {
    (
        SketchPointRef::new(curve, SketchPointSelector::Start),
        SketchPointRef::new(curve, SketchPointSelector::End),
    )
}
fn unordered(a: SketchPointRef, b: SketchPointRef) -> [SketchPointRef; 2] {
    if a < b { [a, b] } else { [b, a] }
}
fn closures(sketch: &Sketch) -> Vec<[SketchPointRef; 2]> {
    sketch
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let next = sketch.curves[(i + 1) % sketch.curves.len()].id;
            unordered(endpoints(c.id).1, endpoints(next).0)
        })
        .collect()
}
fn segment(curve: StableEntityId) -> SketchSegmentRef {
    let (start, end) = endpoints(curve);
    SketchSegmentRef::new(start, end)
}
/// The centre of a circle, as a point of the sketch.
fn center(curve: StableEntityId) -> SketchPointRef {
    SketchPointRef::new(curve, SketchPointSelector::Center)
}
/// Which of a managed profile's circles bounds the part and which is the bore,
/// or `None` when the profile is not two circles.
///
/// The catalogue's answer for a reader — `inspect`, and the form in the app —
/// so neither has to decide roles for itself. It is the same reading the editor
/// uses, asked of the same stored sketch.
pub fn constraint_circle_roles(sketch: &Sketch) -> Option<(StableEntityId, StableEntityId)> {
    let [a, b] = sketch.curves.as_slice() else {
        return None;
    };
    let (SketchGeometry::Circle { radius: ar, .. }, SketchGeometry::Circle { radius: br, .. }) =
        (&a.geometry, &b.geometry)
    else {
        return None;
    };
    Some(if ar >= br { (a.id, b.id) } else { (b.id, a.id) })
}

/// Translate form gestures to explicit changes; rebuild decides whether they fit.
pub fn sketch_constraint_changes(
    id: ObjectId,
    sketch: &Sketch,
    edits: &SketchConstraintEdits,
) -> Result<Vec<crate::EditChange>> {
    let mut kept = sketch
        .constraints
        .iter()
        .filter(|c| !edits.remove.contains(&c.id))
        .cloned()
        .collect::<Vec<_>>();
    let mut added = Vec::new();
    // A circle has no joints to close, so nothing is appended on its behalf.
    if sketch
        .curves
        .iter()
        .all(|c| matches!(c.geometry, SketchGeometry::Line { .. }))
        && !edits.add.is_empty()
    {
        for (i, pair) in closures(sketch).into_iter().enumerate() {
            if kept.iter().any(
                |c| matches!(c.rule,SketchConstraintRule::Coincident{a,b} if unordered(a,b)==pair),
            ) {
                continue;
            }
            let c = SketchConstraint {
                id: StableEntityId::new(),
                rule: SketchConstraintRule::Coincident {
                    a: endpoints(sketch.curves[i].id).1,
                    b: endpoints(sketch.curves[(i + 1) % sketch.curves.len()].id).0,
                },
            };
            kept.push(c.clone());
            added.push(c);
        }
    }
    for add in &edits.add {
        let line = match *add {
            AddSketchConstraint::Concentric { a, b } => {
                // The existing Coincident of two points, which is what "one
                // shared centre" already means: a circle's centre is a point of
                // the sketch, so there is nothing else to say and no second way
                // to compute a centre.
                let c = SketchConstraint {
                    id: StableEntityId::new(),
                    rule: SketchConstraintRule::Coincident {
                        a: center(a),
                        b: center(b),
                    },
                };
                kept.push(c.clone());
                added.push(c);
                continue;
            }
            AddSketchConstraint::Circle { curve, kind } => {
                let rule = match kind {
                    CircleConstraintKind::Radius(radius) => SketchConstraintRule::Radius {
                        curve,
                        radius: crate::Expression::constant(radius.get())?,
                    },
                    CircleConstraintKind::FixedCenter { x, y } => SketchConstraintRule::Fixed {
                        point: SketchPointRef::new(curve, SketchPointSelector::Center),
                        x: crate::Expression::constant(x.get())?,
                        y: crate::Expression::constant(y.get())?,
                    },
                };
                let c = SketchConstraint {
                    id: StableEntityId::new(),
                    rule,
                };
                kept.push(c.clone());
                added.push(c);
                continue;
            }
            AddSketchConstraint::Line(line) => line,
        };
        let rule = match line {
            AddLineConstraint::Line { curve, kind } => {
                let (a, b) = endpoints(curve);
                match kind {
                    LineConstraintKind::Horizontal => SketchConstraintRule::Horizontal { a, b },
                    LineConstraintKind::Vertical => SketchConstraintRule::Vertical { a, b },
                    LineConstraintKind::Distance(length) => SketchConstraintRule::Distance {
                        a,
                        b,
                        distance: crate::Expression::constant(length.get())?,
                    },
                    LineConstraintKind::Fixed { at, x, y } => SketchConstraintRule::Fixed {
                        point: SketchPointRef::new(curve, at.selector()),
                        x: crate::Expression::constant(x.get())?,
                        y: crate::Expression::constant(y.get())?,
                    },
                }
            }
            AddLineConstraint::EqualLength { a, b } => SketchConstraintRule::EqualLength {
                a: segment(a),
                b: segment(b),
            },
            AddLineConstraint::Relation { a, b, relation } => {
                let (a, b) = (segment(a), segment(b));
                match relation {
                    LineRelation::Parallel => SketchConstraintRule::Parallel { a, b },
                    LineRelation::Perpendicular => SketchConstraintRule::Perpendicular { a, b },
                    LineRelation::Collinear => SketchConstraintRule::Collinear { a, b },
                }
            }
            // The directions are stored as said, never sorted or rewritten.
            AddLineConstraint::DirectedAngle { a, b, angle } => {
                SketchConstraintRule::DirectedAngle {
                    a: a.segment(),
                    b: b.segment(),
                    angle: crate::Expression::constant(angle.radians())?,
                }
            }
        };
        let c = SketchConstraint {
            id: StableEntityId::new(),
            rule,
        };
        kept.push(c.clone());
        added.push(c);
    }
    let changes: Vec<_> = edits
        .remove
        .iter()
        .map(|constraint| crate::EditChange::RemoveConstraint {
            sketch: id,
            constraint: *constraint,
        })
        .chain(
            added
                .into_iter()
                .map(|constraint| crate::EditChange::AddConstraint {
                    sketch: id,
                    constraint,
                }),
        )
        .collect();
    crate::validate_sketch_changes(id, sketch, &changes)?;
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §31E: the one conversion every adapter uses. The marker is printed after
    /// the last assertion so a CI step can tell that the body ran.
    #[test]
    fn a_circle_diameter_halves_into_the_stored_radius_and_refuses_what_cannot_be() {
        for (diameter, radius) in [
            (13.5, 6.75),
            (2., 1.),
            (0.5, 0.25),
            (4.25, 2.125),
            (f64::MAX, f64::MAX / 2.),
            (f64::MIN_POSITIVE, f64::MIN_POSITIVE / 2.),
            (
                f64::from_bits(f64::MIN_POSITIVE.to_bits() + 2),
                f64::from_bits((f64::MIN_POSITIVE.to_bits() + 2) / 2),
            ),
            (f64::from_bits(2), f64::from_bits(1)),
        ] {
            let converted = CircleRadiusMm::from_diameter_mm(diameter).expect("a positive D");
            assert_eq!(converted.get(), radius, "D {diameter:e}");
            assert_eq!(
                converted.get() * 2.,
                diameter,
                "R = D/2 exactly: {diameter:e}"
            );
            // The same stored value a typed radius gives, so the slot, the
            // solver and the wire cannot tell which unit was typed.
            assert_eq!(converted, CircleRadiusMm::new(radius).expect("R"));
        }
        for bad in [
            0.,
            -0.,
            -13.5,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN,
            // Halving rounds among the subnormals: the smallest positive value
            // becomes zero, and an odd one comes back as a different diameter.
            f64::from_bits(1),
            f64::from_bits(3),
            // Normal input can also round when its half is subnormal.
            f64::from_bits(f64::MIN_POSITIVE.to_bits() + 1),
        ] {
            assert!(
                CircleRadiusMm::from_diameter_mm(bad).is_err(),
                "diameter {bad:e} was accepted"
            );
        }
    }
}
