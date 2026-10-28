// SPDX-License-Identifier: MIT
//! The typed objects a document stores.
//!
//! These are the source of truth: parameters, sketch geometry, feature inputs
//! and the semantic contracts that name produced geometry. Nothing here refers
//! to a geometry kernel, a face index or a traversal order, because none of
//! those survive a rebuild.

use ferritecad_exchange::{Diagnostic as ImportDiagnostic, PersistedScene};
use ferritecad_kernel::KernelIdentity;
use ferritecad_types::{
    CadError, CanonicalHasher, ContentHash, Dimension, ImportedSourceId, ObjectId, ProfileJoint,
    Result, StableEntityId, Transform, normalize_f64,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::envelope::Envelope;

/// The `format` tag written to `imported_sources` for STEP bytes.
pub const STEP_SOURCE_FORMAT: &str = "exchange.step";

/// An object type this build implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ObjectKind {
    Parameter,
    DatumPlane,
    Sketch,
    Body,
    Extrude,
    ImportedStep,
    /// A profile turned about an axis (§27A: full turn, NewBody only).
    Revolve,
    /// One edge of an earlier feature's result, rounded (§28A).
    Fillet,
    /// One edge of an earlier feature's result, cut away at equal distances
    /// along its two faces (§29A).
    Chamfer,
}

impl ObjectKind {
    /// The discriminator written to the `kind` column and the envelope.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parameter => "parameter",
            Self::DatumPlane => "datum.plane",
            Self::Sketch => "sketch",
            Self::Body => "body",
            Self::Extrude => "feature.extrude",
            Self::ImportedStep => "exchange.step.imported",
            Self::Revolve => "feature.revolve",
            Self::Fillet => "feature.fillet",
            Self::Chamfer => "feature.chamfer",
        }
    }

    /// Returns `None` for a type this build does not implement, which is a
    /// normal outcome rather than an error.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "parameter" => Some(Self::Parameter),
            "datum.plane" => Some(Self::DatumPlane),
            "sketch" => Some(Self::Sketch),
            "body" => Some(Self::Body),
            "feature.extrude" => Some(Self::Extrude),
            "exchange.step.imported" => Some(Self::ImportedStep),
            "feature.revolve" => Some(Self::Revolve),
            "feature.fillet" => Some(Self::Fillet),
            "feature.chamfer" => Some(Self::Chamfer),
            _ => None,
        }
    }

    /// Whether an object of this kind participates in the rebuild as a feature.
    pub fn is_feature(self) -> bool {
        matches!(
            self,
            Self::Extrude | Self::Revolve | Self::Fillet | Self::Chamfer
        )
    }
}

/// A user-editable value together with how it was written.
///
/// Both halves are stored. The source text is what the user sees and edits and
/// is the only thing that can be re-evaluated when a referenced parameter
/// changes; the value is the normalised result in internal units, kept so a
/// document can be rebuilt without an expression evaluator being available.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    pub source: String,
    value: f64,
}

impl Expression {
    pub fn new(source: impl Into<String>, value: f64) -> Result<Self> {
        Ok(Self {
            source: source.into(),
            value: normalize_f64(value)?,
        })
    }

    /// An expression that is just a number.
    pub fn constant(value: f64) -> Result<Self> {
        let value = normalize_f64(value)?;
        Ok(Self {
            source: value.to_string(),
            value,
        })
    }

    /// The last evaluated value, in internal units.
    pub fn value(&self) -> f64 {
        self.value
    }

    fn feed(&self, hasher: &mut CanonicalHasher) {
        hasher.str(&self.source);
        hasher
            .f64(self.value)
            .expect("expression values are validated finite on construction");
    }

    fn validate(&self) -> Result<()> {
        normalize_f64(self.value)?;
        Ok(())
    }
}

/// A named value other objects can refer to by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub dimension: Dimension,
    pub expression: Expression,
}

/// A plane features and sketches can be built on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatumPlane {
    /// Maps the plane's local XY frame into model space.
    pub placement: Transform,
}

/// A point in a sketch's own plane, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point2 {
    pub x: f64,
    pub y: f64,
}

impl Point2 {
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Result<Self> {
        Ok(Self {
            x: normalize_f64(x)?,
            y: normalize_f64(y)?,
        })
    }

    fn validate(self) -> Result<()> {
        normalize_f64(self.x)?;
        normalize_f64(self.y)?;
        Ok(())
    }
}

/// The shape of one sketch element.
///
/// Angles are radians measured counter-clockwise from the sketch plane's local
/// X axis; an arc runs counter-clockwise from `start_angle` to `end_angle`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SketchGeometry {
    Point {
        at: Point2,
    },
    Line {
        start: Point2,
        end: Point2,
    },
    Circle {
        center: Point2,
        radius: f64,
    },
    Arc {
        center: Point2,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
}

impl SketchGeometry {
    /// What to call this shape in a message about it.
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Point { .. } => "point",
            Self::Line { .. } => "line",
            Self::Circle { .. } => "circle",
            Self::Arc { .. } => "arc",
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Point { at } => at.validate(),
            Self::Line { start, end } => {
                start.validate()?;
                end.validate()
            }
            Self::Circle { center, radius } => {
                center.validate()?;
                validate_positive(*radius, "circle radius")
            }
            Self::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                center.validate()?;
                validate_positive(*radius, "arc radius")?;
                normalize_f64(*start_angle)?;
                normalize_f64(*end_angle)?;
                Ok(())
            }
        }
    }
}

/// One element of a sketch.
///
/// The identifier is the durable half of the naming scheme: a topology
/// reference names *this segment*, not "the third curve", so inserting a
/// segment ahead of it does not silently retarget anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchCurve {
    pub id: StableEntityId,
    /// Construction geometry guides the sketch but produces no edges.
    pub construction: bool,
    pub geometry: SketchGeometry,
}

/// Which stored point of a curve a constraint names.
///
/// Only the points that are a stored coordinate pair of their own, because
/// only those have somewhere for an answer to be written back to. Measured
/// against all four [`SketchGeometry`] variants:
///
/// - `Point { at }` has one, [`Self::At`].
/// - `Line { start, end }` has two, [`Self::Start`] and [`Self::End`].
/// - `Circle { center, radius }` stores a centre, but a circle *is* its centre
///   and its radius together, and the solver contract has no radius parameter
///   and no relationship that names one. Constraining the centre alone would
///   let a solve move a circle while everything that met its rim quietly
///   stopped meeting it.
/// - `Arc` is worse: the two points a profile chain actually joins at are its
///   endpoints, and those are derived from a centre, a radius and two angles
///   rather than stored. There is no durable pair to name, and no angle the
///   contract could read or write.
///
/// So a reference into a circle or an arc has no selector at all, and
/// validation says so by name. A `Center` variant that every path refused
/// would be vocabulary this build cannot honour, written down as though it
/// could.
// Ordered so that a point reference can key a map. The order itself carries no
// meaning and nothing stored depends on it; what is stored is the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SketchPointSelector {
    /// The position of a [`SketchGeometry::Point`].
    At,
    /// The start of a [`SketchGeometry::Line`].
    Start,
    /// The end of a [`SketchGeometry::Line`].
    End,
    /// The centre of a [`SketchGeometry::Circle`].
    ///
    /// Its own word rather than `At` or `Start` borrowed for the occasion. A
    /// centre is not the position of a point and not an endpoint of a line;
    /// spelling it as either would make every stored constraint ambiguous the
    /// moment a sketch held both kinds of curve, and would make a document
    /// written by this build unreadable by any build that took the name at
    /// face value.
    Center,
}

impl SketchPointSelector {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::At => "at",
            Self::Start => "start",
            Self::End => "end",
            Self::Center => "center",
        }
    }

    /// Whether this selector names a point that the given geometry stores.
    fn fits(self, geometry: &SketchGeometry) -> bool {
        matches!(
            (self, geometry),
            (Self::At, SketchGeometry::Point { .. })
                | (Self::Start | Self::End, SketchGeometry::Line { .. })
                | (Self::Center, SketchGeometry::Circle { .. })
        )
    }
}

/// One point of one curve, named the way the document names things.
///
/// A curve by its [`StableEntityId`] and a point of it by which point it is.
/// Nothing here is a position in an array, a coordinate, a solver identifier or
/// anything else that a later edit or a later session could re-issue: reordering
/// the curve list moves nothing, and two curves drawn on top of one another stay
/// two curves with two identities, which is exactly why `Coincident` has to be
/// said rather than inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SketchPointRef {
    pub curve: StableEntityId,
    pub at: SketchPointSelector,
}

impl SketchPointRef {
    pub fn new(curve: StableEntityId, at: SketchPointSelector) -> Self {
        Self { curve, at }
    }
}

impl std::fmt::Display for SketchPointRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.curve, self.at.as_str())
    }
}

/// A straight run between two named points.
///
/// Two point references and not a curve, because the relationships that take a
/// segment — equal length, perpendicular, parallel — are about the line joining
/// two points, and saying "the third curve" would make the meaning depend on an
/// ordinal that any edit can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SketchSegmentRef {
    pub from: SketchPointRef,
    pub to: SketchPointRef,
}

impl SketchSegmentRef {
    pub fn new(from: SketchPointRef, to: SketchPointRef) -> Self {
        Self { from, to }
    }
}

/// What one constraint says.
///
/// The eight families the solver comparison measured, in the document's own
/// words. Nothing here is wider than the sketch solver's contract, and nothing
/// here is that crate's types: a document that imported them would make its
/// stored meaning depend on which solver was chosen, and which solver was
/// chosen is the one thing a stored meaning has to outlive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SketchConstraintRule {
    /// Two points occupy the same place.
    Coincident {
        a: SketchPointRef,
        b: SketchPointRef,
    },
    /// A point is pinned where it is told.
    Fixed {
        point: SketchPointRef,
        x: Expression,
        y: Expression,
    },
    /// The distance between two points.
    Distance {
        a: SketchPointRef,
        b: SketchPointRef,
        distance: Expression,
    },
    /// Two points share a y coordinate.
    Horizontal {
        a: SketchPointRef,
        b: SketchPointRef,
    },
    /// Two points share an x coordinate.
    Vertical {
        a: SketchPointRef,
        b: SketchPointRef,
    },
    /// Two segments are the same length.
    EqualLength {
        a: SketchSegmentRef,
        b: SketchSegmentRef,
    },
    /// Two segments meet at a right angle.
    Perpendicular {
        a: SketchSegmentRef,
        b: SketchSegmentRef,
    },
    /// Two segments run in the same direction.
    Parallel {
        a: SketchSegmentRef,
        b: SketchSegmentRef,
    },
    /// A circle has the radius it is told, in millimetres.
    ///
    /// Names a curve rather than a point reference, because a radius is not a
    /// point of anything: it is the circle's own scalar. The centre is said
    /// the ordinary way, as a [`SketchPointRef`] with
    /// [`SketchPointSelector::Center`], so `Fixed` pins it with no second
    /// vocabulary.
    Radius {
        curve: StableEntityId,
        radius: Expression,
    },
    /// The vector `b.from -> b.to` is the vector `a.from -> a.to` turned
    /// counter-clockwise by `angle` radians in the sketch's local XY (§31F).
    ///
    /// `angle` is canonical in `(-π, π]`; the opposite direction is `+π` only.
    /// Each segment is a *directed* run: unlike `Parallel` and `Perpendicular`,
    /// which ignore which way a segment is written, here `from` and `to` are
    /// the vector, and exchanging them adds half a turn.
    DirectedAngle {
        a: SketchSegmentRef,
        b: SketchSegmentRef,
        angle: Expression,
    },
    /// The two segments lie on the same infinite line (§31G).
    ///
    /// Neither the direction nor the order of the ends matters, and the
    /// segments need not touch. Not `Parallel`, which two parallel segments on
    /// different lines also satisfy: the offset between the lines is part of
    /// what this holds.
    Collinear {
        a: SketchSegmentRef,
        b: SketchSegmentRef,
    },
}

impl SketchConstraintRule {
    /// Every point this rule names, in the order it names them.
    ///
    /// The one enumeration. Validation checks these, and the evaluator's
    /// translation into solver terms resolves these, so a constraint cannot be
    /// checked against one set of references and solved against another.
    /// Adding a family means adding an arm here and nowhere else.
    pub fn points(&self) -> Vec<SketchPointRef> {
        match *self {
            Self::Radius { .. } => Vec::new(),
            Self::Fixed { point, .. } => vec![point],
            Self::Coincident { a, b }
            | Self::Distance { a, b, .. }
            | Self::Horizontal { a, b }
            | Self::Vertical { a, b } => vec![a, b],
            Self::EqualLength { a, b }
            | Self::Perpendicular { a, b }
            | Self::Parallel { a, b }
            | Self::DirectedAngle { a, b, .. }
            | Self::Collinear { a, b } => vec![a.from, a.to, b.from, b.to],
        }
    }

    /// Every whole curve this rule names, which is none for the families that
    /// speak in points and segments.
    ///
    /// The counterpart of [`SketchConstraintRule::points`], and kept apart
    /// from it for the same reason the solver keeps its two identifier types
    /// apart: a list that mixed a curve with a point of a curve would have to
    /// be read with a rule about which entries meant which.
    pub fn curves(&self) -> Vec<StableEntityId> {
        match *self {
            Self::Radius { curve, .. } => vec![curve],
            _ => Vec::new(),
        }
    }

    /// The segments this rule names, which is none for the point families.
    fn segments(&self) -> Vec<SketchSegmentRef> {
        match *self {
            Self::EqualLength { a, b }
            | Self::Perpendicular { a, b }
            | Self::Parallel { a, b }
            | Self::DirectedAngle { a, b, .. }
            | Self::Collinear { a, b } => vec![a, b],
            _ => Vec::new(),
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Fixed { x, y, .. } => {
                normalize_f64(x.value())?;
                normalize_f64(y.value())?;
            }
            Self::Distance { distance, .. } => {
                validate_positive(distance.value(), "constraint distance")?;
            }
            Self::Radius { radius, .. } => {
                validate_positive(radius.value(), "constraint radius")?;
            }
            // Canonical or refused: a stored `3π/2` would be read as `-π/2` by
            // arithmetic nobody chose, so the range is the persistence rule.
            Self::DirectedAngle { angle, .. } => {
                let angle = normalize_f64(angle.value())?;
                if angle <= -std::f64::consts::PI || angle > std::f64::consts::PI {
                    return Err(CadError::input(format!(
                        "a directed angle is canonical in (-pi, pi] radians, found {angle}"
                    )));
                }
            }
            _ => {}
        }

        // A segment is two points. The solver contract does not refuse one that
        // names a single point twice, but it does not mean anything either:
        // perpendicular and parallel over a zero-length segment score exactly
        // zero, so such a constraint is silently satisfied and never diagnosed.
        // Refusing it here is the only place it can be said out loud.
        //
        // Nothing else structural is refused. `Distance` between a point and
        // itself is impossible to satisfy, but that is a conflict, and telling
        // the user which constraint conflicts is what the solver is for; a
        // document that would not store it is a document in which they never
        // find out.
        for segment in self.segments() {
            if segment.from == segment.to {
                return Err(CadError::input(format!(
                    "a segment runs between two points, but this one names {} twice",
                    segment.from
                )));
            }
        }
        Ok(())
    }
}

/// One durably identified constraint of a sketch.
///
/// The identifier is the whole point. A solver reports a conflict against
/// whichever constraint conflicts, and that report has to survive being handed
/// back to a document that has since had constraints added to it, removed from
/// it or reordered. An ordinal would not; this does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchConstraint {
    pub id: StableEntityId,
    pub rule: SketchConstraintRule,
}

/// Profile geometry on a plane, and the relationships between it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    /// The datum plane this sketch lies on.
    pub plane: ObjectId,
    pub curves: Vec<SketchCurve>,
    /// Stored in the order the user added them, and read back in it, because a
    /// document should give back what it was given. No answer depends on that
    /// order: a diagnosis names a [`SketchConstraint::id`].
    ///
    pub constraints: Vec<SketchConstraint>,
}

impl Sketch {
    /// Everything the persistence boundary has to be sure of about a sketch.
    ///
    /// One implementation, called from [`ObjectPayload::validate`], which is
    /// on both the read and the write path. There is no second copy of these
    /// rules in document validation or in the evaluator to drift away from
    /// this one.
    fn validate(&self) -> Result<()> {
        let mut geometry_of = BTreeMap::new();
        for curve in &self.curves {
            if geometry_of.insert(curve.id, &curve.geometry).is_some() {
                return Err(CadError::input(format!(
                    "sketch contains duplicate curve id {}",
                    curve.id
                )));
            }
            curve.geometry.validate()?;
        }

        let mut named = BTreeSet::new();
        for constraint in &self.constraints {
            if !named.insert(constraint.id) {
                return Err(CadError::input(format!(
                    "sketch contains duplicate constraint id {}",
                    constraint.id
                )));
            }
            constraint.rule.validate()?;

            // Construction geometry is reached through exactly this lookup and
            // is never filtered out of it. A construction line is what most
            // constrained sketches are held together by, and a document that
            // dropped constraints on it would lose the sketch's skeleton while
            // keeping its skin.
            for curve in constraint.rule.curves() {
                let Some(geometry) = geometry_of.get(&curve) else {
                    return Err(CadError::input(format!(
                        "constraint {} refers to curve {curve}, which is not a curve of this \
                         sketch",
                        constraint.id
                    )));
                };
                if !matches!(geometry, SketchGeometry::Circle { .. }) {
                    return Err(CadError::input(format!(
                        "constraint {} gives a radius to {curve}, which is a {} and has none",
                        constraint.id,
                        geometry.kind_name(),
                    )));
                }
            }
            for point in constraint.rule.points() {
                let Some(geometry) = geometry_of.get(&point.curve) else {
                    return Err(CadError::input(format!(
                        "constraint {} refers to {}, which is not a curve of this sketch",
                        constraint.id, point
                    )));
                };
                if !point.at.fits(geometry) {
                    return Err(CadError::input(format!(
                        "constraint {} names the {} of {}, which is a {} and has no such point",
                        constraint.id,
                        point.at.as_str(),
                        point.curve,
                        geometry.kind_name(),
                    )));
                }
            }
        }
        Ok(())
    }
}

/// A solid produced by the feature chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Body {
    /// The last feature that contributed to this body, or `None` while it is
    /// still empty.
    pub tip_feature: Option<ObjectId>,
}

/// How far an extrusion runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum EndCondition {
    /// A fixed distance in one direction.
    Blind { distance: Expression },
    /// The same distance either side of the sketch plane.
    Symmetric { distance: Expression },
    /// Through everything present, in the given direction.
    ThroughAll,
}

/// How a feature's result combines with an existing body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SolidOperation {
    NewBody,
    Add,
    Cut,
    Intersect,
}

/// The axis a [`Revolve`] turns its profile about.
///
/// A sketch coordinate axis or an infinite Line, directed from its stored
/// start to its end. Construction Lines may supply the axis too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RevolveAxis {
    SketchX,
    SketchY,
    Line(StableEntityId),
}

impl std::fmt::Display for RevolveAxis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SketchX => f.write_str("the sketch X axis"),
            Self::SketchY => f.write_str("the sketch Y axis"),
            Self::Line(line) => write!(f, "Line {line}"),
        }
    }
}

/// How far a [`Revolve`] turns.
///
/// §27A: exactly one full turn, 2π, which closes on itself and has no end
/// faces. §27D: a partial angle, a sector with a start and an end face. The
/// two are different topology and never stand in for each other: 2π is not
/// a partial angle, and no partial angle is widened to a full turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RevolveExtent {
    FullTurn,
    Partial { angle: Expression },
}

/// The angle of a partial [`Revolve`], in radians: the one domain policy for
/// it, applied by the UI draft, the CLI request, the create job and the payload
/// decoder alike (§27D); the evaluator reads only payloads that passed it.
///
/// A right-handed turn about the directed [`RevolveAxis`], starting at the
/// profile. Kept exactly as given: nothing is rounded, reduced modulo a turn or
/// turned into another operation. Accepted from [`Self::MIN_RADIANS`] to
/// [`Self::MAX_RADIANS`], both inclusive; 0, negative angles, angles closer
/// than the minimum to 0 or to a full turn, exactly 2π, several turns, NaN
/// and infinities are refused. The limits come from kernel and single-precision
/// mesh checks.
///
/// The refusals name no unit: the UI and the CLI take degrees, and each shows
/// the accepted range in its own unit beside the value it took.
#[derive(Debug, Clone, Copy)]
pub struct RevolveAngle(f64);

impl RevolveAngle {
    /// The thinnest sector this build keeps (0.01°).
    pub const MIN_RADIANS: f64 = 0.000_174_532_925_199_432_96;
    /// The widest partial sector: the minimum short of a full turn.
    pub const MAX_RADIANS: f64 = std::f64::consts::TAU - Self::MIN_RADIANS;

    pub fn new(radians: f64) -> Result<Self> {
        if !radians.is_finite() {
            return Err(CadError::input("the revolve angle is not a finite number"));
        }
        if radians <= 0.0 {
            return Err(CadError::input(
                "the revolve angle is not positive: a partial revolve turns right-handed about +Y, \
                 and a mirrored sweep is not supported",
            ));
        }
        if radians == std::f64::consts::TAU {
            return Err(CadError::input(
                "the revolve angle is a full turn, which has no end faces: state a full turn \
                 instead of a partial angle",
            ));
        }
        if radians > std::f64::consts::TAU {
            return Err(CadError::input(
                "the revolve angle is more than one turn: a solid turns at most once",
            ));
        }
        if radians < Self::MIN_RADIANS {
            return Err(CadError::input(
                "the revolve angle is below the minimum: the sector would be too thin to keep \
                 every face",
            ));
        }
        if radians > Self::MAX_RADIANS {
            return Err(CadError::input(
                "the revolve angle is within the minimum sector of a full turn: a partial revolve \
                 stops short of that, and a full turn is stated as one",
            ));
        }
        Ok(Self(radians))
    }

    pub fn radians(self) -> f64 {
        self.0
    }
}

// Validated finite and positive, so equality by bits is equality by value.
impl PartialEq for RevolveAngle {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl Eq for RevolveAngle {}

/// Turns a sketch profile about an axis in its own plane.
///
/// The intent, stored whole: which profile, about which axis, how far. Never a
/// mesh, never an equivalent chain of extrusions. §27A writes and evaluates
/// exactly one class — a full turn about the sketch's Y axis starting a body —
/// and refuses every other combination rather than approximating it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revolve {
    /// The sketch supplying the profile.
    pub profile: ObjectId,
    /// Curves on selected outer boundaries; empty selects every region.
    pub regions: Vec<StableEntityId>,
    pub axis: RevolveAxis,
    pub extent: RevolveExtent,
    pub operation: SolidOperation,
    /// The result this feature combines with; absent exactly for NewBody.
    pub previous: Option<ObjectId>,
}

impl Revolve {
    /// The cache key for this feature's own contribution; the caller adds the
    /// resolved profile and the kernel identity.
    pub fn cache_key(&self, tolerance: ferritecad_types::Tolerance) -> ContentHash {
        let mut hasher = CanonicalHasher::new("feature.revolve");
        hasher.algorithm_version(1);
        tolerance.feed(&mut hasher);
        hasher.field("profile").bytes(&self.profile.to_bytes());
        hasher.field("regions");
        for curve in self
            .regions
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
        {
            hasher.bytes(&curve.to_bytes());
        }
        hasher.field("axis");
        match self.axis {
            RevolveAxis::SketchX => {
                hasher.str("sketch_x");
            }
            RevolveAxis::SketchY => {
                hasher.str("sketch_y");
            }
            RevolveAxis::Line(line) => {
                hasher.str("line").bytes(&line.to_bytes());
            }
        }
        match &self.extent {
            RevolveExtent::FullTurn => {
                hasher.field("extent").str("full_turn");
            }
            RevolveExtent::Partial { angle } => {
                hasher.field("extent").str("partial");
                angle.feed(&mut hasher);
            }
        }
        hasher.field("operation").str(match self.operation {
            SolidOperation::NewBody => "new_body",
            SolidOperation::Add => "add",
            SolidOperation::Cut => "cut",
            SolidOperation::Intersect => "intersect",
        });
        if let Some(previous) = self.previous {
            hasher.field("previous").bytes(&previous.to_bytes());
        }
        hasher.finish()
    }
}

/// The one edge a [`Fillet`] rounds or a [`Chamfer`] cuts, by what it means
/// (§29A: the neutral name of what §28A called `FilletEdge`; the fields, and so
/// the stored and wire form of a Fillet, are unchanged).
///
/// The feature that made the edge and the corner of that feature's profile it
/// was swept from: the same unordered pair of Line UUIDs
/// recorded by the profile. Never an index, a
/// coordinate or a reference UUID; the evaluator finds exactly this edge in
/// the producer's output or refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweptEdge {
    /// The feature whose output the edge belongs to.
    pub feature: ObjectId,
    /// The profile corner the edge was swept from.
    pub joint: ProfileJoint,
}

/// The edge a [`Fillet`] rounds; see [`SweptEdge`].
pub type FilletEdge = SweptEdge;

/// Cuts one edge of an earlier feature's result away, at one equal distance
/// along both adjacent faces (§29A).
///
/// Modifies a result, so it names the feature it consumes (ADR 0004). The
/// distance is an expression in millimetres and is **not** the width of
/// the slanted face, which is `distance_mm * sqrt(2)`. In this slice the edge
/// is swept by the very feature the Chamfer consumes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chamfer {
    /// The feature whose result this cuts.
    pub previous: ObjectId,
    pub edge: SweptEdge,
    pub distance_mm: Expression,
}

impl Chamfer {
    /// The cache key for this feature's own statement; the caller adds the
    /// predecessor's key and the kernel identity.
    pub fn cache_key(&self, tolerance: ferritecad_types::Tolerance) -> ContentHash {
        let mut hasher = CanonicalHasher::new("feature.chamfer");
        hasher.algorithm_version(1);
        tolerance.feed(&mut hasher);
        hasher.field("previous").bytes(&self.previous.to_bytes());
        let [one, other] = self.edge.joint.segments();
        hasher
            .field("edge")
            .bytes(&self.edge.feature.to_bytes())
            .bytes(&one.to_bytes())
            .bytes(&other.to_bytes());
        hasher.field("distance_mm");
        self.distance_mm.feed(&mut hasher);
        hasher.finish()
    }
}

/// Rounds one edge of an earlier feature's result (§28A).
///
/// Modifies a result, so it names the feature it consumes, exactly as a Cut
/// does (ADR 0004): the Body names its tip and ownership is derived. The
/// radius is an expression in millimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fillet {
    /// The feature whose result this rounds.
    pub previous: ObjectId,
    pub edge: FilletEdge,
    pub radius_mm: Expression,
}

impl Fillet {
    /// The cache key for this feature's own statement; the caller adds the
    /// predecessor's key and the kernel identity.
    pub fn cache_key(&self, tolerance: ferritecad_types::Tolerance) -> ContentHash {
        let mut hasher = CanonicalHasher::new("feature.fillet");
        hasher.algorithm_version(1);
        tolerance.feed(&mut hasher);
        hasher.field("previous").bytes(&self.previous.to_bytes());
        let [one, other] = self.edge.joint.segments();
        hasher
            .field("edge")
            .bytes(&self.edge.feature.to_bytes())
            .bytes(&one.to_bytes())
            .bytes(&other.to_bytes());
        hasher.field("radius_mm");
        self.radius_mm.feed(&mut hasher);
        hasher.finish()
    }
}

/// Sweeps a sketch profile along the plane normal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extrude {
    /// The sketch supplying the profile.
    pub profile: ObjectId,
    /// Curves on selected outer boundaries; empty selects every region.
    pub regions: Vec<StableEntityId>,
    pub end_condition: EndCondition,
    /// Runs against the plane normal when true.
    pub reversed: bool,
    pub operation: SolidOperation,
    /// The feature whose result this one modifies.
    ///
    /// `None` exactly when [`operation`][Self::operation] is
    /// [`SolidOperation::NewBody`]: a feature that starts a body consumes no
    /// earlier result, and one that changes an existing solid must say which.
    /// It names a **feature**, never a body, which is what keeps the evaluation
    /// graph acyclic by construction — see
    /// [`DependencyRole::Predecessor`][crate::DependencyRole::Predecessor].
    pub previous: Option<ObjectId>,
}

impl Extrude {
    /// The cache key for this feature's geometric result.
    ///
    /// The caller adds the resolved inputs of `profile` and `previous`; this
    /// covers only what the feature itself contributes.
    pub fn cache_key(&self, tolerance: ferritecad_types::Tolerance) -> ContentHash {
        let mut hasher = CanonicalHasher::new("feature.extrude");
        hasher.algorithm_version(1);
        tolerance.feed(&mut hasher);

        hasher.field("profile").bytes(&self.profile.to_bytes());
        hasher.field("regions");
        for curve in self
            .regions
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
        {
            hasher.bytes(&curve.to_bytes());
        }
        hasher.field("reversed").bool(self.reversed);
        hasher.field("operation").str(match self.operation {
            SolidOperation::NewBody => "new_body",
            SolidOperation::Add => "add",
            SolidOperation::Cut => "cut",
            SolidOperation::Intersect => "intersect",
        });

        hasher.field("end_condition");
        match &self.end_condition {
            EndCondition::Blind { distance } => {
                hasher.str("blind");
                distance.feed(&mut hasher);
            }
            EndCondition::Symmetric { distance } => {
                hasher.str("symmetric");
                distance.feed(&mut hasher);
            }
            EndCondition::ThroughAll => {
                hasher.str("through_all");
            }
        }

        // Which earlier result this one consumes is part of what it is. Two
        // cuts of one tool against two different solids are two features, and
        // a key that omitted this would let the cache serve either for the
        // other.
        hasher.field("previous");
        match &self.previous {
            Some(feature) => hasher.bytes(&feature.to_bytes()),
            None => hasher.str("none"),
        };

        hasher.finish()
    }
}

/// Which kernel read an imported file, at the moment it was read.
///
/// [`KernelIdentity`] written down. It is provenance and nothing else: a
/// document whose import was read by another build, another Open CASCADE or
/// another operating system still opens, still re-imports and is still checked
/// against what was stored. The identity is what lets a difference in the
/// result be explained rather than merely noticed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterIdentity {
    pub id: String,
    pub version: String,
    /// Whatever else changes results — patch level, compiler, target triple.
    /// This is why a differing identity is not grounds to refuse a document:
    /// the same release built for another platform differs here by design.
    pub build: String,
}

impl ImporterIdentity {
    pub fn of(kernel: &KernelIdentity) -> Self {
        Self {
            id: kernel.id().to_owned(),
            version: kernel.version().to_owned(),
            build: kernel.build().to_owned(),
        }
    }

    /// Reconstructs the kernel contract's own type, applying its validation.
    pub fn to_kernel_identity(&self) -> Result<KernelIdentity> {
        KernelIdentity::new(&self.id, &self.version, &self.build)
    }
}

impl std::fmt::Display for ImporterIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.build.is_empty() {
            write!(f, "{} {}", self.id, self.version)
        } else {
            write!(f, "{} {} ({})", self.id, self.version, self.build)
        }
    }
}

/// A lasting way to say "that part of that imported file".
///
/// Both halves are needed and neither is enough. The key identifies a
/// definition *inside one file*: `step.product_definition#31` names something
/// in the file that wrote it and something else, or nothing, in the next one.
/// The corpus makes that concrete — `01-single-part.step` and
/// `02-flat-assembly.step` both contain `step.product_definition#5`, and they
/// are a plate and a bracket. A reference carrying only the key would resolve
/// to whichever import it was asked about.
///
/// So the two travel together as one validated value rather than as two fields
/// a caller has to remember to check against each other.
///
/// # What it deliberately cannot do
///
/// It names a definition — a part the file describes — and not an occurrence
/// of one. A definition has an identity its source wrote down; an occurrence
/// is still only a position in a tree, and a reference mixing the two would
/// look durable while resting on an index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawImportedDefinitionRef")]
pub struct ImportedDefinitionRef {
    source: ImportedSourceId,
    definition_key: String,
}

impl ImportedDefinitionRef {
    /// Names a definition in one imported source.
    ///
    /// Refuses an empty key: a reference that names nothing could only be
    /// resolved by guessing, and guessing is what this type exists to prevent.
    pub fn new(source: ImportedSourceId, definition_key: impl Into<String>) -> Result<Self> {
        let definition_key = definition_key.into();
        if definition_key.trim().is_empty() {
            return Err(CadError::input(
                "a reference into an imported file must name a definition; an empty key \
                 could only be resolved by guessing",
            ));
        }
        Ok(Self {
            source,
            definition_key,
        })
    }

    /// The source whose bytes this reference is about, and the only one it may
    /// ever be resolved against.
    pub fn source(&self) -> ImportedSourceId {
        self.source
    }

    pub fn definition_key(&self) -> &str {
        &self.definition_key
    }
}

impl std::fmt::Display for ImportedDefinitionRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} in source {}", self.definition_key, self.source)
    }
}

/// The wire form, so a stored reference is validated on the way in rather than
/// trusted for having decoded.
#[derive(Deserialize)]
struct RawImportedDefinitionRef {
    source: ImportedSourceId,
    definition_key: String,
}

impl TryFrom<RawImportedDefinitionRef> for ImportedDefinitionRef {
    type Error = CadError;

    fn try_from(raw: RawImportedDefinitionRef) -> Result<Self> {
        Self::new(raw.source, raw.definition_key)
    }
}

/// A STEP file this document carries, and what reading it once produced.
///
/// The bytes are not here. They live in `imported_sources`, addressed by
/// [`source`][Self::source], because a payload is something a reader decodes to
/// learn a document's shape and a source file is something it must not have to
/// decode for that. What is here is the reading: the portable scene, who read
/// it, and what they said about it.
///
/// # Two of these facts are repeated from the source row on purpose
///
/// [`source_hash`][Self::source_hash] and [`source_byte_len`][Self::source_byte_len]
/// also appear as columns beside the bytes. The duplication is the check: the
/// row proves its own bytes are intact, and this object proves the intact bytes
/// are the ones *it* was built from. A source row swapped underneath an object
/// would satisfy the first and fail the second.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedStep {
    /// The row holding the exact bytes. Immutable: importing different bytes
    /// mints a new source rather than editing this one.
    pub source: ImportedSourceId,
    pub source_hash: ContentHash,
    pub source_byte_len: u64,
    /// What the file was called where it came from, if that was worth keeping.
    /// A hint for a person, never a path anything opens: the bytes in this
    /// document are the source, and no external file is consulted.
    pub source_name: Option<String>,
    /// The handle-free projection of the scene that import produced.
    ///
    pub scene: PersistedScene,
    pub imported_by: ImporterIdentity,
    /// What the importer reported *then*.
    ///
    /// Historical, and never rewritten. Re-importing in a later session
    /// produces its own diagnostics from its own reading, possibly by a
    /// different kernel build; presenting either as the other would claim an
    /// observation nobody made.
    pub diagnostics_at_import: Vec<ImportDiagnostic>,
}

impl ImportedStep {
    fn validate(&self) -> Result<()> {
        self.scene.validate()?;
        self.imported_by.to_kernel_identity()?;
        if self.source_byte_len > i64::MAX as u64 {
            return Err(CadError::input(format!(
                "an imported source of {} bytes is beyond what this document addresses",
                self.source_byte_len
            )));
        }
        Ok(())
    }
}

/// The decoded content of a stored object.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ObjectPayload {
    Parameter(Parameter),
    DatumPlane(DatumPlane),
    Sketch(Sketch),
    Body(Body),
    Extrude(Extrude),
    /// A profile turned about an axis.
    Revolve(Revolve),
    /// One rounded edge of an earlier result (§28A).
    Fillet(Fillet),
    /// One cut edge of an earlier result (§29A).
    Chamfer(Chamfer),
    /// A STEP file and the scene one reading of it produced.
    ImportedStep(ImportedStep),
}

impl ObjectPayload {
    /// References implied by the payload, independent of modelling policy.
    pub fn dependencies(&self, object: ObjectId) -> Vec<crate::Dependency> {
        use crate::DependencyRole as R;
        let mut references = Vec::new();
        match self {
            Self::Sketch(s) => references.push((s.plane, R::Plane)),
            Self::Extrude(e) => references.push((e.profile, R::Profile)),
            Self::Revolve(r) => references.push((r.profile, R::Profile)),
            Self::Body(b) => references.extend(b.tip_feature.map(|tip| (tip, R::BodyTip))),
            _ => {}
        }
        references.extend(
            self.previous_feature()
                .map(|previous| (previous, R::Predecessor)),
        );
        references
            .into_iter()
            .map(|(dependency, role)| crate::Dependency {
                dependent: object,
                dependency,
                role,
            })
            .collect()
    }

    /// The discriminator to store.
    pub fn type_name(&self) -> &str {
        match self {
            Self::Parameter(_) => ObjectKind::Parameter.as_str(),
            Self::DatumPlane(_) => ObjectKind::DatumPlane.as_str(),
            Self::Sketch(_) => ObjectKind::Sketch.as_str(),
            Self::Body(_) => ObjectKind::Body.as_str(),
            Self::Extrude(_) => ObjectKind::Extrude.as_str(),
            Self::Revolve(_) => ObjectKind::Revolve.as_str(),
            Self::Fillet(_) => ObjectKind::Fillet.as_str(),
            Self::Chamfer(_) => ObjectKind::Chamfer.as_str(),
            Self::ImportedStep(_) => ObjectKind::ImportedStep.as_str(),
        }
    }

    pub fn kind(&self) -> Option<ObjectKind> {
        ObjectKind::parse(self.type_name())
    }

    /// Encodes the current payload for storage.
    pub fn to_storage_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let name = self.type_name();
        let envelope = match self {
            Self::Parameter(v) => Envelope::encode(name, v)?,
            Self::DatumPlane(v) => Envelope::encode(name, v)?,
            Self::Sketch(v) => Envelope::encode(name, v)?,
            Self::Body(v) => Envelope::encode(name, v)?,
            Self::Extrude(v) => Envelope::encode(name, v)?,
            Self::Revolve(v) => Envelope::encode(name, v)?,
            Self::Fillet(v) => Envelope::encode(name, v)?,
            Self::Chamfer(v) => Envelope::encode(name, v)?,
            Self::ImportedStep(v) => Envelope::encode(name, v)?,
        };
        envelope.to_bytes()
    }

    /// Decodes a supported object; an unknown type or invalid payload is an error.
    pub fn from_storage_bytes(bytes: &[u8]) -> Result<Self> {
        let envelope = Envelope::from_bytes(bytes)?;
        let kind = ObjectKind::parse(&envelope.type_name).ok_or_else(|| {
            CadError::input(format!("unknown object type {}", envelope.type_name))
        })?;
        let payload = match kind {
            ObjectKind::Parameter => Self::Parameter(envelope.decode()?),
            ObjectKind::DatumPlane => Self::DatumPlane(envelope.decode()?),
            ObjectKind::Sketch => Self::Sketch(envelope.decode()?),
            ObjectKind::Body => Self::Body(envelope.decode()?),
            ObjectKind::Extrude => Self::Extrude(envelope.decode()?),
            ObjectKind::Revolve => Self::Revolve(envelope.decode()?),
            ObjectKind::Fillet => Self::Fillet(envelope.decode()?),
            ObjectKind::Chamfer => Self::Chamfer(envelope.decode()?),
            ObjectKind::ImportedStep => Self::ImportedStep(envelope.decode()?),
        };
        payload.validate()?;
        Ok(payload)
    }

    /// Enforces numeric and local semantic invariants at the persistence
    /// boundary. `serde` can construct public structs without using their
    /// constructors, so constructors alone cannot keep non-finite values out
    /// of a document.
    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Parameter(parameter) => parameter.expression.validate(),
            Self::DatumPlane(plane) => {
                for row in plane.placement.rows() {
                    for value in row {
                        normalize_f64(*value)?;
                    }
                }
                Ok(())
            }
            Self::Sketch(sketch) => sketch.validate(),
            Self::Body(_) => Ok(()),
            Self::Extrude(extrude) => {
                match &extrude.end_condition {
                    EndCondition::Blind { distance } | EndCondition::Symmetric { distance } => {
                        distance.validate()?;
                        if distance.value() <= 0.0 {
                            return Err(CadError::input(format!(
                                "extrude distance must be positive, found {}",
                                distance.value()
                            )));
                        }
                    }
                    EndCondition::ThroughAll
                        if matches!(
                            extrude.operation,
                            SolidOperation::NewBody | SolidOperation::Add
                        ) =>
                    {
                        return Err(CadError::input(
                            "a new body or additive boss has nothing to go ThroughAll; \
                             ThroughAll requires Cut or Intersect",
                        ));
                    }
                    EndCondition::ThroughAll => {}
                }
                match (extrude.operation, extrude.previous.is_some()) {
                    (SolidOperation::NewBody, false) => Ok(()),
                    (SolidOperation::NewBody, true) => Err(CadError::input(
                        "a new-body extrude consumes no earlier result",
                    )),
                    (_, false) => Err(CadError::input(
                        "an additive, cut, or intersect extrude must name the result it modifies",
                    )),
                    (_, true) => Ok(()),
                }
            }

            // The layout records any axis, extent and operation a later build
            // may write; this build stores and rebuilds only the §27A class,
            // and refuses to write anything else under its name.
            Self::Revolve(revolve) => {
                if let RevolveExtent::Partial { angle } = &revolve.extent {
                    angle.validate()?;
                    RevolveAngle::new(angle.value())?;
                }
                match (revolve.operation, revolve.previous) {
                    (SolidOperation::NewBody, None) => Ok(()),
                    (SolidOperation::NewBody, Some(_)) => Err(CadError::input(
                        "a new-body revolve consumes no earlier result",
                    )),
                    (_, None) => Err(CadError::input(
                        "an additive, cut, or intersect revolve must name the result it modifies",
                    )),
                    (_, Some(_)) => Ok(()),
                }
            }
            // One finite positive radius. Whether it fits the edge is a
            // question about geometry, asked by the policy that knows the
            // part (`fillet::FilletCorner::check_radius`) and again by the evaluator.
            Self::Fillet(fillet) => {
                let radius = normalize_f64(fillet.radius_mm.value())?;
                if radius <= 0.0 {
                    return Err(CadError::input(format!(
                        "a fillet radius must be positive, found {radius} mm"
                    )));
                }
                Ok(())
            }
            // One finite positive distance, at the feature's own bounds;
            // whether it fits the part is `chamfer::ChamferCorner::check_distance`,
            // asked again by the evaluator. The edge is the previous
            // feature's own.
            Self::Chamfer(chamfer) => {
                let distance = normalize_f64(chamfer.distance_mm.value())?;
                if distance <= 0.0 {
                    return Err(CadError::input(format!(
                        "a chamfer distance must be positive, found {distance} mm"
                    )));
                }
                if chamfer.edge.feature != chamfer.previous {
                    return Err(CadError::unsupported(
                        "this build chamfers an edge of the feature a Chamfer consumes",
                    ));
                }
                Ok(())
            }
            Self::ImportedStep(imported) => imported.validate(),
        }
    }

    /// The feature this one consumes, for a feature that modifies a result.
    ///
    /// One answer for every feature kind that has one, so the history checks
    /// (`feature.forked-history`, `body.shared-history`) and the readers that
    /// walk a Body's history cannot forget a kind.
    pub fn previous_feature(&self) -> Option<ObjectId> {
        match self {
            Self::Extrude(extrude) => extrude.previous,
            Self::Revolve(revolve) => revolve.previous,
            Self::Fillet(fillet) => Some(fillet.previous),
            Self::Chamfer(chamfer) => Some(chamfer.previous),
            _ => None,
        }
    }
}

fn validate_positive(value: f64, what: &str) -> Result<()> {
    let value = normalize_f64(value)?;
    if value <= 0.0 {
        return Err(CadError::input(format!(
            "{what} must be positive, found {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_types::Tolerance;

    #[test]
    fn a_partial_angle_validates_its_value_and_keys_geometry() {
        for bad in [
            0.0,
            -0.0,
            -90.0,
            0.009_999,
            359.990_001,
            360.0,
            400.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(RevolveAngle::new(f64::to_radians(bad)).is_err(), "{bad}");
        }
        for good in [0.01, 1.0, 90.0, 137.5, 100.0 / 3.0, 359.99] {
            assert_eq!(
                RevolveAngle::new(f64::to_radians(good))
                    .expect("sector")
                    .radians(),
                f64::to_radians(good)
            );
        }
        assert!(RevolveAngle::new(RevolveAngle::MIN_RADIANS.next_down()).is_err());
        assert!(RevolveAngle::new(RevolveAngle::MAX_RADIANS.next_up()).is_err());
        let mut revolve = Revolve {
            regions: Vec::new(),
            profile: ObjectId::new(),
            axis: RevolveAxis::SketchY,
            extent: RevolveExtent::FullTurn,
            operation: SolidOperation::NewBody,
            previous: None,
        };
        let tolerance = Tolerance::default();
        let full = revolve.cache_key(tolerance);
        revolve.extent = RevolveExtent::Partial {
            angle: crate::Expression::constant(f64::to_radians(90.0)).expect("sector"),
        };
        let quarter = revolve.cache_key(tolerance);
        assert_ne!(full, quarter);
        let payload = ObjectPayload::Revolve(revolve.clone());
        let bytes = payload.to_storage_bytes().expect("encode");
        assert_eq!(
            ObjectPayload::from_storage_bytes(&bytes).expect("decode"),
            payload
        );
        revolve.extent = RevolveExtent::Partial {
            angle: crate::Expression::constant(f64::to_radians(90.000_000_001)).expect("sector"),
        };
        assert_ne!(quarter, revolve.cache_key(tolerance));
        revolve.axis = RevolveAxis::Line(StableEntityId::new());
        assert_ne!(quarter, revolve.cache_key(tolerance));
    }

    #[test]
    fn dimensional_constraints_round_trip_expressions_in_internal_units() {
        let point = SketchPointRef::new(StableEntityId::new(), SketchPointSelector::Start);
        let other = SketchPointRef::new(point.curve, SketchPointSelector::End);
        let segment = SketchSegmentRef::new(point, other);
        let constant = |value| Expression::constant(value).expect("finite dimension");
        for rule in [
            SketchConstraintRule::Fixed {
                point,
                x: constant(-12.5),
                y: constant(3.25),
            },
            SketchConstraintRule::Distance {
                a: point,
                b: other,
                distance: constant(20.0),
            },
            SketchConstraintRule::Radius {
                curve: point.curve,
                radius: constant(4.5),
            },
            SketchConstraintRule::DirectedAngle {
                a: segment,
                b: segment,
                angle: constant(std::f64::consts::FRAC_PI_2),
            },
        ] {
            rule.validate().expect("valid rule");
            let mut bytes = Vec::new();
            ciborium::into_writer(&rule, &mut bytes).expect("write rule");
            let read: SketchConstraintRule =
                ciborium::from_reader(bytes.as_slice()).expect("read rule");
            assert_eq!(read, rule);
            let values = match &read {
                SketchConstraintRule::Fixed { x, y, .. } => vec![x, y],
                SketchConstraintRule::Distance { distance, .. } => vec![distance],
                SketchConstraintRule::Radius { radius, .. } => vec![radius],
                SketchConstraintRule::DirectedAngle { angle, .. } => vec![angle],
                _ => unreachable!("dimensional rules only"),
            };
            for expression in values {
                assert_eq!(expression.source, expression.value().to_string());
            }
        }
    }

    #[test]
    fn persistence_rejects_nonfinite_expression_values_in_every_dimension() {
        let point = SketchPointRef::new(StableEntityId::new(), SketchPointSelector::Start);
        let other = SketchPointRef::new(point.curve, SketchPointSelector::End);
        let segment = SketchSegmentRef::new(point, other);
        let edge = SweptEdge {
            feature: ObjectId::new(),
            joint: ProfileJoint::new(point.curve, StableEntityId::new()).expect("distinct lines"),
        };
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let invalid = Expression {
                source: "1".into(),
                value,
            };
            for rule in [
                SketchConstraintRule::Fixed {
                    point,
                    x: invalid.clone(),
                    y: Expression::constant(0.0).expect("zero"),
                },
                SketchConstraintRule::Fixed {
                    point,
                    x: Expression::constant(0.0).expect("zero"),
                    y: invalid.clone(),
                },
                SketchConstraintRule::Distance {
                    a: point,
                    b: other,
                    distance: invalid.clone(),
                },
                SketchConstraintRule::Radius {
                    curve: point.curve,
                    radius: invalid.clone(),
                },
                SketchConstraintRule::DirectedAngle {
                    a: segment,
                    b: segment,
                    angle: invalid.clone(),
                },
            ] {
                assert!(rule.validate().is_err());
            }
            for payload in [
                ObjectPayload::Fillet(Fillet {
                    previous: edge.feature,
                    edge,
                    radius_mm: invalid.clone(),
                }),
                ObjectPayload::Chamfer(Chamfer {
                    previous: edge.feature,
                    edge,
                    distance_mm: invalid.clone(),
                }),
                ObjectPayload::Revolve(Revolve {
                    regions: Vec::new(),
                    profile: ObjectId::new(),
                    axis: RevolveAxis::SketchY,
                    extent: RevolveExtent::Partial { angle: invalid },
                    operation: SolidOperation::NewBody,
                    previous: None,
                }),
            ] {
                assert!(payload.to_storage_bytes().is_err());
            }
        }
    }

    fn sample_extrude() -> Extrude {
        Extrude {
            regions: Vec::new(),
            profile: ObjectId::new(),
            end_condition: EndCondition::Blind {
                distance: Expression::constant(10.0).expect("finite"),
            },
            reversed: false,
            operation: SolidOperation::NewBody,
            previous: None,
        }
    }

    #[test]
    fn every_known_payload_round_trips() {
        let payloads = vec![
            ObjectPayload::Parameter(Parameter {
                name: "width".to_owned(),
                dimension: Dimension::Length,
                expression: Expression::new("2 * 25", 50.0).expect("finite"),
            }),
            ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
            ObjectPayload::Sketch(Sketch {
                plane: ObjectId::new(),
                curves: vec![SketchCurve {
                    id: StableEntityId::new(),
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: Point2::ORIGIN,
                        end: Point2::new(10.0, 0.0).expect("finite"),
                    },
                }],
                constraints: Vec::new(),
            }),
            ObjectPayload::Body(Body { tip_feature: None }),
            ObjectPayload::Extrude(sample_extrude()),
        ];

        for payload in payloads {
            let bytes = payload.to_storage_bytes().expect("encodes");
            let read = ObjectPayload::from_storage_bytes(&bytes).expect("decodes");
            assert_eq!(read, payload, "{} did not round trip", payload.type_name());
        }
    }

    #[test]
    fn cache_key_tracks_every_input() {
        let tolerance = Tolerance::default();
        let base = sample_extrude();
        let key = base.cache_key(tolerance);

        let mut deeper = base.clone();
        deeper.end_condition = EndCondition::Blind {
            distance: Expression::constant(11.0).expect("finite"),
        };
        assert_ne!(deeper.cache_key(tolerance), key, "distance must matter");

        let mut flipped = base.clone();
        flipped.reversed = true;
        assert_ne!(flipped.cache_key(tolerance), key, "direction must matter");

        let mut cut = base.clone();
        cut.operation = SolidOperation::Cut;
        assert_ne!(cut.cache_key(tolerance), key, "operation must matter");

        let coarse = Tolerance::new(1e-3, 1e-6).expect("positive");
        assert_ne!(base.cache_key(coarse), key, "tolerance must matter");
    }

    #[test]
    fn the_same_inputs_produce_the_same_cache_key() {
        let tolerance = Tolerance::default();
        let extrude = sample_extrude();
        assert_eq!(extrude.cache_key(tolerance), extrude.cache_key(tolerance));
    }

    #[test]
    fn symmetric_and_blind_at_the_same_distance_differ() {
        let tolerance = Tolerance::default();
        let mut blind = sample_extrude();
        blind.end_condition = EndCondition::Blind {
            distance: Expression::constant(10.0).expect("finite"),
        };
        let mut symmetric = blind.clone();
        symmetric.end_condition = EndCondition::Symmetric {
            distance: Expression::constant(10.0).expect("finite"),
        };

        assert_ne!(blind.cache_key(tolerance), symmetric.cache_key(tolerance));
    }
}
