// SPDX-License-Identifier: MIT
//! The typed objects a document stores.
//!
//! These are the source of truth: parameters, sketch geometry, feature inputs
//! and the semantic contracts that name produced geometry. Nothing here refers
//! to a geometry kernel, a face index or a traversal order, because none of
//! those survive a rebuild.

use ferritecad_exchange::{
    Diagnostic as ImportDiagnostic, KeyedScene, LegacyScene, PersistedScene, StoredScene,
};
use ferritecad_kernel::KernelIdentity;
use ferritecad_types::{
    CadError, CanonicalHasher, ContentHash, Dimension, ImportedSourceId, ObjectId, Point3,
    ProfileJoint, Result, StableEntityId, Transform, normalize_f64,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::envelope::{Envelope, UnknownObject};

/// The capability every object this build writes depends on.
pub const CORE_CAPABILITY: &str = "core.part.v1";

/// What a document needs a reader to implement before it may name the edge an
/// extrusion left where one of its caps meets the face swept from a profile
/// segment.
///
/// A capability rather than a new payload version, and the difference is not a
/// matter of taste. The stored bytes are laid out exactly as before: a
/// `topology_ref` payload is still a role, a rule and an optional signature,
/// and a version says the layout changed. What changed is the vocabulary of
/// roles, which is precisely what a capability is for — a reader that does not
/// know this role can still read every other reference in the document, and
/// the envelope tells it so by name instead of failing to parse a tag it has
/// never seen.
///
/// Declared only on the references that use the role. A document written
/// before this build, and one written by it that names no such edge, requires
/// nothing new and stays writable by a reader that lacks this.
pub const EXTRUDE_CAP_EDGE_CAPABILITY: &str = "topology.extrude-cap-edge.v1";

/// The capability a stored [`SemanticRole::ExtrudeSweepEdge`] depends on.
///
/// Its own name rather than a widening of the cap-edge one. A reader that
/// understands cap edges does not thereby understand sweep edges, and telling
/// it otherwise would let it rewrite a document containing references whose
/// meaning it cannot reproduce. The payload layout is again unchanged, so this
/// is once more a vocabulary change and not a version bump.
pub const EXTRUDE_SWEEP_EDGE_CAPABILITY: &str = "topology.extrude-sweep-edge.v1";

/// The capability a stored [`SemanticRole::ExtrudeCapVertex`] depends on.
///
/// Again its own name. A reader that understands the edge running along a
/// corner does not thereby understand the point where that corner meets a cap:
/// the two roles select different geometry from the same pair of segments, and
/// a reader told it understood both would rewrite references whose meaning it
/// cannot reproduce. The envelope layout is unchanged once more, so this is a
/// vocabulary change and not a version bump.
pub const EXTRUDE_CAP_VERTEX_CAPABILITY: &str = "topology.extrude-cap-vertex.v1";

/// What a reader must implement before it may rewrite a sketch that carries
/// constraints.
///
/// Its own name beside [`CORE_CAPABILITY`] rather than a widening of it,
/// because the two answer different questions. A build that predates this one
/// can read every curve of such a sketch perfectly well; what it cannot do is
/// keep the relationships between them, and a rewrite that dropped those would
/// turn a constrained drawing back into loose coordinates that merely happen to
/// sit where the solver last left them. The capability is what makes that
/// build stop instead.
///
/// Declared only by a sketch that actually holds a constraint. An unconstrained
/// sketch declares exactly what it declared before this build existed, stays at
/// layout v1, and stays writable by a reader that lacks this.
pub const SKETCH_CONSTRAINTS_CAPABILITY: &str = "sketch.constraints.v1";

/// The capability an [`ImportedStep`] object depends on.
///
/// Declared separately from [`CORE_CAPABILITY`] so a reader that understands
/// SQL schema v3 but not this object reaches the right conclusion on its own:
/// it opens the document read-only and preserves the object it cannot read,
/// rather than rewriting a document whose source-of-truth bytes it has no idea
/// are there. A pre-v3 binary is stricter still and refuses the newer SQL
/// schema before it reaches capability negotiation.
///
/// # Still the only one, at payload layout 3
///
/// Placement identity arrived as a payload version rather than as a second
/// capability beside this, and the two are not interchangeable. A capability
/// says a reader must understand a vocabulary whose bytes are laid out as
/// before; scene layout 3 adds a field to every stored placement, so a reader
/// that has not heard of it cannot decode the payload at all. That reader is
/// already stopped, by the layout, through
/// [`ObjectKind::readable_schema_versions`] — the object is preserved verbatim
/// and never rewritten. A capability naming the same fact would add a name and
/// no refusal.
pub const IMPORTED_STEP_CAPABILITY: &str = "exchange.step.imported.v1";

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
            _ => None,
        }
    }

    /// The capabilities a reader must implement to rewrite an object of this
    /// type, at one particular payload layout, without losing what it means.
    ///
    /// Taken with the layout rather than with the type alone because for a
    /// sketch the two really do differ: v1 holds curves, which any build can
    /// keep, and v2 holds the relationships between them, which only a build
    /// that knows [`SKETCH_CONSTRAINTS_CAPABILITY`] can. Deciding it from the
    /// header keeps the answer available before anything is decoded, which is
    /// what capability negotiation at open time needs.
    pub fn required_capabilities(self, schema_version: u32) -> Vec<String> {
        match (self, schema_version) {
            (Self::ImportedStep, _) => vec![IMPORTED_STEP_CAPABILITY.to_owned()],
            (Self::Sketch, 2) => vec![
                CORE_CAPABILITY.to_owned(),
                SKETCH_CONSTRAINTS_CAPABILITY.to_owned(),
            ],
            _ => vec![CORE_CAPABILITY.to_owned()],
        }
    }

    /// Every capability this type may require at any layout this build reads.
    ///
    /// A payload naming something outside this set is one whose contract this
    /// build cannot honour whatever its version says, and is preserved verbatim
    /// rather than decoded. That is how a future constraint family arrives: it
    /// comes under a capability of its own, and this build keeps its bytes
    /// instead of reading a vocabulary it only half understands.
    pub fn known_capabilities(self) -> &'static [&'static str] {
        match self {
            Self::ImportedStep => &[IMPORTED_STEP_CAPABILITY],
            Self::Sketch => &[CORE_CAPABILITY, SKETCH_CONSTRAINTS_CAPABILITY],
            _ => &[CORE_CAPABILITY],
        }
    }

    /// Newest layout version of this object type's payload.
    ///
    /// What an object is *stored* at is decided by what it holds, not by this:
    /// see [`ObjectPayload::schema_version`].
    pub fn schema_version(self) -> u32 {
        match self {
            // v2 gave every definition the identity its source file wrote
            // down, and made instances name theirs by it rather than by
            // position. v3 gave every *placement* a durable identity of its
            // own, which no source file records and which nothing else in a
            // scene can stand in for. v1 and v2 objects are still read and
            // still written back as themselves; see [`ImportedStep::scene`].
            Self::ImportedStep => 3,
            // v2 added the constraint list. v1 sketches are still read and
            // still written, because a sketch with no constraints is a v1
            // sketch; see [`Sketch::schema_version`].
            Self::Sketch => 2,
            _ => 1,
        }
    }

    /// Payload layouts of this type that this build can still read.
    ///
    /// Newest first. A layout listed here is one whose meaning is fully
    /// recoverable; anything else is preserved verbatim and not interpreted.
    pub fn readable_schema_versions(self) -> &'static [u32] {
        match self {
            Self::ImportedStep => &[3, 2, 1],
            Self::Sketch => &[2, 1],
            _ => &[1],
        }
    }

    /// Whether an object of this kind participates in the rebuild as a feature.
    pub fn is_feature(self) -> bool {
        matches!(self, Self::Extrude)
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

    fn validate(&self) -> Result<()> {
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
    #[serde(default)]
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
}

impl SketchPointSelector {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::At => "at",
            Self::Start => "start",
            Self::End => "end",
        }
    }

    /// Whether this selector names a point that the given geometry stores.
    fn fits(self, geometry: &SketchGeometry) -> bool {
        matches!(
            (self, geometry),
            (Self::At, SketchGeometry::Point { .. })
                | (Self::Start | Self::End, SketchGeometry::Line { .. })
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
        x: f64,
        y: f64,
    },
    /// The distance between two points.
    Distance {
        a: SketchPointRef,
        b: SketchPointRef,
        distance: f64,
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
            Self::Fixed { point, .. } => vec![point],
            Self::Coincident { a, b }
            | Self::Distance { a, b, .. }
            | Self::Horizontal { a, b }
            | Self::Vertical { a, b } => vec![a, b],
            Self::EqualLength { a, b } | Self::Perpendicular { a, b } | Self::Parallel { a, b } => {
                vec![a.from, a.to, b.from, b.to]
            }
        }
    }

    /// The segments this rule names, which is none for the point families.
    fn segments(&self) -> Vec<SketchSegmentRef> {
        match *self {
            Self::EqualLength { a, b } | Self::Perpendicular { a, b } | Self::Parallel { a, b } => {
                vec![a, b]
            }
            _ => Vec::new(),
        }
    }

    fn validate(&self) -> Result<()> {
        match *self {
            Self::Fixed { x, y, .. } => {
                normalize_f64(x)?;
                normalize_f64(y)?;
            }
            Self::Distance { distance, .. } => {
                validate_positive(distance, "constraint distance")?;
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
    /// Skipped when empty, so a sketch that has no constraints is byte for byte
    /// the sketch this build wrote before constraints existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<SketchConstraint>,
}

impl Sketch {
    /// The layout this sketch has to be stored at.
    ///
    /// Decided by what it holds, not by what this build is capable of writing.
    /// A sketch with no constraints is a v1 sketch however new the build is,
    /// and stamping v2 on it would tell every older reader to keep its hands
    /// off a document it can handle perfectly well.
    pub fn schema_version(&self) -> u32 {
        if self.constraints.is_empty() { 1 } else { 2 }
    }

    /// What a reader must implement to rewrite this sketch. See
    /// [`SKETCH_CONSTRAINTS_CAPABILITY`].
    pub fn required_capabilities(&self) -> Vec<String> {
        ObjectKind::Sketch.required_capabilities(self.schema_version())
    }

    /// Refuses an envelope whose header does not describe the payload inside
    /// it.
    ///
    /// Both directions, and both matter. A v1 header over a payload that
    /// carries constraints is the dangerous one: it tells an older build the
    /// document is safe to rewrite, and that build drops the constraints
    /// without ever knowing they were there. A header that declares the
    /// capability over a payload with no constraints is the other half of the
    /// same dishonesty, and locks readers out of a document for nothing.
    pub(crate) fn require_declared_contract(&self, envelope: &Envelope) -> Result<()> {
        let version = self.schema_version();
        let capabilities = self.required_capabilities();
        if envelope.schema_version == version && envelope.required_capabilities == capabilities {
            return Ok(());
        }
        Err(CadError::input(format!(
            "a sketch holding {} constraint(s) belongs at schema v{version} requiring {}, but \
             its envelope says schema v{} requiring {}",
            self.constraints.len(),
            capabilities.join(", "),
            envelope.schema_version,
            envelope.required_capabilities.join(", "),
        )))
    }

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

/// Sweeps a sketch profile along the plane normal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extrude {
    /// The sketch supplying the profile.
    pub profile: ObjectId,
    pub end_condition: EndCondition,
    /// Runs against the plane normal when true.
    #[serde(default)]
    pub reversed: bool,
    pub operation: SolidOperation,
    /// The body being modified; `None` for [`SolidOperation::NewBody`].
    pub target_body: Option<ObjectId>,
}

impl Extrude {
    /// The cache key for this feature's geometric result.
    ///
    /// The caller adds the resolved inputs of `profile` and `target_body`; this
    /// covers only what the feature itself contributes.
    pub fn cache_key(&self, tolerance: ferritecad_types::Tolerance) -> ContentHash {
        let mut hasher = CanonicalHasher::new("feature.extrude");
        hasher.algorithm_version(ObjectKind::Extrude.schema_version());
        tolerance.feed(&mut hasher);

        hasher.field("profile").bytes(&self.profile.to_bytes());
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

        hasher.field("target_body");
        match &self.target_body {
            Some(body) => hasher.bytes(&body.to_bytes()),
            None => hasher.str("none"),
        };

        hasher.finish()
    }
}

/// What sort of geometry a reference expects to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EntityKind {
    Face,
    Edge,
    Vertex,
}

impl EntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Face => "face",
            Self::Edge => "edge",
            Self::Vertex => "vertex",
        }
    }

    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "face" => Ok(Self::Face),
            "edge" => Ok(Self::Edge),
            "vertex" => Ok(Self::Vertex),
            other => Err(CadError::input(format!("unknown entity kind {other:?}"))),
        }
    }
}

/// Which end of an extrusion a cap belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CapSide {
    Start,
    End,
}

/// What a produced entity *is*, in terms of the feature that made it.
///
/// This is the intent that survives a rebuild. It never mentions a face index,
/// a traversal position or a kernel handle, because a reference expressed that
/// way silently points at different geometry as soon as anything upstream
/// changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SemanticRole {
    /// Geometry derived from one identified sketch segment.
    SketchSegment { segment: StableEntityId },
    /// The planar face closing one end of an extrusion.
    ExtrudeCap { side: CapSide },
    /// The swept face raised from one profile segment.
    ExtrudeSide { profile_segment: StableEntityId },
    /// The edge where one end of an extrusion meets the face swept from one
    /// profile segment.
    ///
    /// A triple, and every part of it is needed: the producer feature comes
    /// from the reference around this role, the side says which of the two
    /// ends, and the segment says which of that end's boundary edges. Two of
    /// the three would name four edges of a plate instead of one.
    ///
    /// Deliberately narrow. It says nothing about the edges running along the
    /// sweep, nothing about vertices, and nothing about edges a fillet or an
    /// import produced: those have no name this build could keep, and a role
    /// that pretended otherwise would be a reference that resolves to whatever
    /// the next rebuild happened to put in that position.
    ExtrudeCapEdge {
        side: CapSide,
        profile_segment: StableEntityId,
    },
    /// The edge running along the sweep where two adjacent profile segments
    /// meet.
    ///
    /// It belongs to the joint, not to either segment: the corner is where the
    /// two swept faces meet, and picking one of them to own it would be a
    /// choice the geometry does not make. So the name is the unordered pair,
    /// and it stays the same name when the profile is walked from a different
    /// starting segment or in the other direction.
    ExtrudeSweepEdge { joint: ProfileJoint },
    /// The vertex where two adjacent profile segments reach one end of the
    /// sweep.
    ///
    /// The corner and the cap together, and neither alone is a name: the pair
    /// of segments says which of the profile's corners, and the side says
    /// which of that corner's two ends. It is the same unordered pair the
    /// sweep edge is named by, and it stays the same pair when the profile is
    /// walked from another segment or in the other direction.
    ///
    /// A pair that meets at two corners names neither vertex. That is not a
    /// gap to be filled by taking the first: the two are genuinely different
    /// points, and nothing durable tells them apart.
    ExtrudeCapVertex { side: CapSide, joint: ProfileJoint },
    /// A face introduced by filleting an identified edge.
    FilletFace { source_edge: StableEntityId },
}

/// How many entities a reference selects, and which.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SelectionRule {
    /// Exactly the one entity carrying this role.
    Exact,
    /// Every entity descended from one named ancestor — "all edges raised from
    /// segment S" — which stays correct when the count changes.
    AllDerivedFrom { ancestor: StableEntityId },
}

/// A geometric description used only as a last-resort match.
///
/// Reached only after semantic origin, ancestry and the deterministic key have
/// all failed. It is a hint for a human, never grounds for silently choosing a
/// neighbouring face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeomSignature {
    pub kind: EntityKind,
    /// Area of a face or length of an edge, in internal units.
    pub measure: f64,
    pub centroid: Point3,
}

impl GeomSignature {
    fn validate(&self) -> Result<()> {
        let measure = normalize_f64(self.measure)?;
        if measure < 0.0 {
            return Err(CadError::input(format!(
                "geometric signature measure must not be negative, found {measure}"
            )));
        }
        normalize_f64(self.centroid.x)?;
        normalize_f64(self.centroid.y)?;
        normalize_f64(self.centroid.z)?;
        Ok(())
    }
}

/// A durable, semantic reference to geometry a feature produced.
#[derive(Debug, Clone, PartialEq)]
pub struct TopologyRef {
    pub id: StableEntityId,
    /// The object holding this reference.
    pub owner: ObjectId,
    /// The feature whose output is being named.
    pub producer_feature: ObjectId,
    pub expected_kind: EntityKind,
    pub output_role: SemanticRole,
    pub selection: SelectionRule,
    pub fallback_signature: Option<GeomSignature>,
}

impl TopologyRef {
    /// A deterministic identity for what this reference durably means.
    ///
    /// The geometric fallback is deliberately absent: it is a recovery hint,
    /// never identity. This key lets a transient picture bind its pick values
    /// to the portable face meanings interpreted beside it without retaining
    /// any document object or kernel handle.
    pub fn meaning_hash(&self) -> ContentHash {
        let mut hasher = CanonicalHasher::new("topology-reference.meaning");
        hasher.algorithm_version(1);
        hasher.field("reference").bytes(&self.id.to_bytes());
        hasher.field("owner").bytes(&self.owner.to_bytes());
        hasher
            .field("producer_feature")
            .bytes(&self.producer_feature.to_bytes());
        hasher
            .field("expected_kind")
            .str(self.expected_kind.as_str());

        hasher.field("output_role");
        match &self.output_role {
            SemanticRole::SketchSegment { segment } => {
                hasher.str("sketch_segment").bytes(&segment.to_bytes());
            }
            SemanticRole::ExtrudeCap { side } => {
                hasher.str("extrude_cap").str(match side {
                    CapSide::Start => "start",
                    CapSide::End => "end",
                });
            }
            SemanticRole::ExtrudeSide { profile_segment } => {
                hasher
                    .str("extrude_side")
                    .bytes(&profile_segment.to_bytes());
            }
            SemanticRole::ExtrudeCapEdge {
                side,
                profile_segment,
            } => {
                // Both, and separately: an edge of the start cap and an edge of
                // the end cap raised from one segment are different edges, and
                // two segments' edges on one cap are different edges too.
                hasher
                    .str("extrude_cap_edge")
                    .str(match side {
                        CapSide::Start => "start",
                        CapSide::End => "end",
                    })
                    .bytes(&profile_segment.to_bytes());
            }
            SemanticRole::ExtrudeSweepEdge { joint } => {
                // The pair is already canonical, so the two segments hash in
                // one order only and naming them the other way round produces
                // the same meaning rather than a second one.
                let [one, other] = joint.segments();
                hasher
                    .str("extrude_sweep_edge")
                    .bytes(&one.to_bytes())
                    .bytes(&other.to_bytes());
            }
            SemanticRole::ExtrudeCapVertex { side, joint } => {
                // Its own tag, then the side, then both segments of the
                // canonical pair. Dropping the side would make the two ends of
                // one corner one meaning, and dropping either segment would
                // make two neighbouring corners one meaning; the tag keeps the
                // whole from colliding with the sweep edge named by the same
                // pair.
                let [one, other] = joint.segments();
                hasher
                    .str("extrude_cap_vertex")
                    .str(match side {
                        CapSide::Start => "start",
                        CapSide::End => "end",
                    })
                    .bytes(&one.to_bytes())
                    .bytes(&other.to_bytes());
            }
            SemanticRole::FilletFace { source_edge } => {
                hasher.str("fillet_face").bytes(&source_edge.to_bytes());
            }
        }

        hasher.field("selection");
        match &self.selection {
            SelectionRule::Exact => {
                hasher.str("exact");
            }
            SelectionRule::AllDerivedFrom { ancestor } => {
                hasher.str("all_derived_from").bytes(&ancestor.to_bytes());
            }
        }
        hasher.finish()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(signature) = &self.fallback_signature {
            signature.validate()?;
        }
        Ok(())
    }
}

/// The portion of a [`TopologyRef`] stored as CBOR; the rest are columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TopologyRefPayload {
    pub output_role: SemanticRole,
    pub selection: SelectionRule,
    pub fallback_signature: Option<GeomSignature>,
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
#[derive(Debug, Clone, PartialEq)]
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
    /// Stored at whichever layout it was written with. A document written
    /// before definitions had identities keeps binding by position and keeps
    /// working; what it cannot do is answer a durable reference, because
    /// inventing a key from a position would produce something that looks like
    /// an identity and behaves like an index.
    pub scene: StoredScene,
    pub imported_by: ImporterIdentity,
    /// What the importer reported *then*.
    ///
    /// Historical, and never rewritten. Re-importing in a later session
    /// produces its own diagnostics from its own reading, possibly by a
    /// different kernel build; presenting either as the other would claim an
    /// observation nobody made.
    pub diagnostics_at_import: Vec<ImportDiagnostic>,
}

/// The stored form of an imported object, at one scene layout.
///
/// Generic over the scene because the two layouts differ in that field and
/// nothing else: two near-identical structs would be two things obliged to
/// agree about source hashes, lengths and provenance forever after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredImport<S> {
    source: ImportedSourceId,
    source_hash: ContentHash,
    source_byte_len: u64,
    #[serde(default)]
    source_name: Option<String>,
    scene: S,
    imported_by: ImporterIdentity,
    diagnostics_at_import: Vec<ImportDiagnostic>,
}

impl<S> StoredImport<S> {
    fn around(imported: &ImportedStep, scene: S) -> Self {
        Self {
            source: imported.source,
            source_hash: imported.source_hash,
            source_byte_len: imported.source_byte_len,
            source_name: imported.source_name.clone(),
            scene,
            imported_by: imported.imported_by.clone(),
            diagnostics_at_import: imported.diagnostics_at_import.clone(),
        }
    }

    fn into_imported(self, scene: StoredScene) -> ImportedStep {
        ImportedStep {
            source: self.source,
            source_hash: self.source_hash,
            source_byte_len: self.source_byte_len,
            source_name: self.source_name,
            scene,
            imported_by: self.imported_by,
            diagnostics_at_import: self.diagnostics_at_import,
        }
    }
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
    /// A STEP file and the scene one reading of it produced.
    ImportedStep(ImportedStep),
    /// An object of a type this build does not implement, preserved verbatim.
    Unknown(UnknownObject),
}

impl ObjectPayload {
    /// The discriminator to store, which for an unknown object is whatever the
    /// writing build called it.
    pub fn type_name(&self) -> &str {
        match self {
            Self::Parameter(_) => ObjectKind::Parameter.as_str(),
            Self::DatumPlane(_) => ObjectKind::DatumPlane.as_str(),
            Self::Sketch(_) => ObjectKind::Sketch.as_str(),
            Self::Body(_) => ObjectKind::Body.as_str(),
            Self::Extrude(_) => ObjectKind::Extrude.as_str(),
            Self::ImportedStep(_) => ObjectKind::ImportedStep.as_str(),
            Self::Unknown(unknown) => &unknown.type_name,
        }
    }

    pub fn kind(&self) -> Option<ObjectKind> {
        ObjectKind::parse(self.type_name())
    }

    pub fn schema_version(&self) -> u32 {
        match self {
            Self::Unknown(unknown) => unknown.schema_version,
            // An imported scene is stored at the layout it holds. A version 1
            // scene rewritten under a version 2 header would claim identities
            // it does not have, and the next reader would believe the header.
            Self::ImportedStep(imported) => imported.scene.version(),
            // Same rule, same reason: see [`Sketch::schema_version`].
            Self::Sketch(sketch) => sketch.schema_version(),
            known => known
                .kind()
                .map(ObjectKind::schema_version)
                .unwrap_or_default(),
        }
    }

    /// The capabilities a reader needs to modify this object.
    pub fn required_capabilities(&self) -> Vec<String> {
        match self {
            Self::Unknown(unknown) => unknown.required_capabilities.clone(),
            known => {
                let version = known.schema_version();
                known
                    .kind()
                    .map(|kind| kind.required_capabilities(version))
                    .unwrap_or_else(|| vec![CORE_CAPABILITY.to_owned()])
            }
        }
    }

    /// Produces the bytes to store.
    ///
    /// An unknown object returns the bytes it arrived with, untouched.
    pub fn to_storage_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let capabilities = self.required_capabilities();
        let version = self.schema_version();
        let name = self.type_name().to_owned();

        let envelope = match self {
            Self::Parameter(v) => Envelope::encode(name, version, capabilities, v)?,
            Self::DatumPlane(v) => Envelope::encode(name, version, capabilities, v)?,
            Self::Sketch(v) => Envelope::encode(name, version, capabilities, v)?,
            Self::Body(v) => Envelope::encode(name, version, capabilities, v)?,
            Self::Extrude(v) => Envelope::encode(name, version, capabilities, v)?,
            // Written back at the layout it was read at. A version 1 scene
            // has no keys and a version 2 scene has no placement identities,
            // and inventing either while writing would turn a document that
            // honestly lacks them into one that claims them — with values
            // indexed by the traversal that happened to be running.
            Self::ImportedStep(v) => match &v.scene {
                StoredScene::V1(scene) => Envelope::encode(
                    name,
                    version,
                    capabilities,
                    &StoredImport::around(v, scene.clone()),
                )?,
                StoredScene::V2(scene) => Envelope::encode(
                    name,
                    version,
                    capabilities,
                    &StoredImport::around(v, scene.clone()),
                )?,
                StoredScene::V3(scene) => Envelope::encode(
                    name,
                    version,
                    capabilities,
                    &StoredImport::around(v, scene.clone()),
                )?,
                _ => {
                    return Err(CadError::unsupported(format!(
                        "this build cannot write a v{} imported scene",
                        v.scene.version()
                    )));
                }
            },
            Self::Unknown(unknown) => return Ok(unknown.raw_envelope().to_vec()),
        };
        envelope.to_bytes()
    }

    /// Interprets stored bytes, falling back to verbatim preservation.
    pub fn from_storage_bytes(bytes: &[u8]) -> Result<Self> {
        let envelope = Envelope::from_bytes(bytes)?;

        let Some(kind) = ObjectKind::parse(&envelope.type_name) else {
            return Ok(Self::Unknown(UnknownObject::new(envelope, bytes.to_vec())));
        };

        // A payload whose layout this build does not read, or which names a
        // capability outside this type's vocabulary, is not something to guess
        // at. Keeping it verbatim is also what carries a constraint family this
        // build has never heard of: a future one arrives under a capability of
        // its own, lands here, and is written back exactly as it came.
        if !kind
            .readable_schema_versions()
            .contains(&envelope.schema_version)
            || !envelope
                .required_capabilities
                .iter()
                .all(|name| kind.known_capabilities().contains(&name.as_str()))
        {
            return Ok(Self::Unknown(UnknownObject::new(envelope, bytes.to_vec())));
        }

        let payload = match kind {
            ObjectKind::Parameter => Self::Parameter(envelope.decode()?),
            ObjectKind::DatumPlane => Self::DatumPlane(envelope.decode()?),
            ObjectKind::Sketch => Self::Sketch(envelope.decode()?),
            ObjectKind::Body => Self::Body(envelope.decode()?),
            ObjectKind::Extrude => Self::Extrude(envelope.decode()?),
            ObjectKind::ImportedStep => Self::ImportedStep(match envelope.schema_version {
                1 => {
                    let stored: StoredImport<LegacyScene> = envelope.decode()?;
                    let scene = StoredScene::V1(stored.scene.clone());
                    stored.into_imported(scene)
                }
                2 => {
                    let stored: StoredImport<KeyedScene> = envelope.decode()?;
                    let scene = StoredScene::V2(stored.scene.clone());
                    stored.into_imported(scene)
                }
                // Named layouts and no fall-through. A wildcard here would
                // decode an unreadable future layout as the newest one this
                // build knows, and the check above — which sends anything not
                // in `readable_schema_versions` to `Unknown` — would have been
                // the only thing standing in the way.
                _ => {
                    let stored: StoredImport<PersistedScene> = envelope.decode()?;
                    let scene = StoredScene::V3(stored.scene.clone());
                    stored.into_imported(scene)
                }
            }),
        };
        payload.require_declared_contract(&envelope)?;
        payload.validate()?;
        Ok(payload)
    }

    /// Refuses a decoded payload whose envelope header misdescribes it.
    ///
    /// The header is what capability negotiation reads at open time, before
    /// anything is decoded, so a header that disagrees with its payload makes
    /// two readers of the same file reach two different conclusions about what
    /// they may do to it. Checked in both directions: under-declaring hands an
    /// older build permission to discard meaning it cannot see, over-declaring
    /// locks readers out of a document that has nothing they cannot handle.
    fn require_declared_contract(&self, envelope: &Envelope) -> Result<()> {
        if let Self::Sketch(sketch) = self {
            return sketch.require_declared_contract(envelope);
        }
        if envelope.schema_version == self.schema_version()
            && envelope.required_capabilities == self.required_capabilities()
        {
            return Ok(());
        }
        Err(CadError::input(format!(
            "a {} payload belongs at schema v{} requiring {}, but its envelope says schema v{} \
             requiring {}",
            self.type_name(),
            self.schema_version(),
            self.required_capabilities().join(", "),
            envelope.schema_version,
            envelope.required_capabilities.join(", "),
        )))
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
                    EndCondition::ThroughAll => {}
                }
                match (extrude.operation, extrude.target_body) {
                    (SolidOperation::NewBody, None)
                    | (
                        SolidOperation::Add | SolidOperation::Cut | SolidOperation::Intersect,
                        Some(_),
                    ) => Ok(()),
                    (SolidOperation::NewBody, Some(_)) => Err(CadError::input(
                        "a new-body extrude must not target an existing body",
                    )),
                    (_, None) => Err(CadError::input(
                        "an additive, cut, or intersect extrude must target a body",
                    )),
                }
            }
            Self::ImportedStep(imported) => imported.validate(),
            Self::Unknown(_) => Ok(()),
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

    fn sample_extrude() -> Extrude {
        Extrude {
            profile: ObjectId::new(),
            end_condition: EndCondition::Blind {
                distance: Expression::constant(10.0).expect("finite"),
            },
            reversed: false,
            operation: SolidOperation::NewBody,
            target_body: None,
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
    fn an_unknown_type_survives_a_load_and_save_cycle_byte_for_byte() {
        let original = Envelope::new(
            "feature.loft",
            1,
            vec!["future.loft.v1".to_owned()],
            vec![0x83, 0x01, 0x02, 0x03],
        )
        .to_bytes()
        .expect("serialises");

        let payload = ObjectPayload::from_storage_bytes(&original).expect("header is readable");
        assert!(matches!(payload, ObjectPayload::Unknown(_)));
        assert_eq!(payload.type_name(), "feature.loft");
        assert_eq!(payload.required_capabilities(), vec!["future.loft.v1"]);

        let written = payload.to_storage_bytes().expect("writes back");
        assert_eq!(written, original);
    }

    #[test]
    fn a_known_type_from_a_newer_layout_is_preserved_not_guessed_at() {
        let future = Envelope::new(
            ObjectKind::Extrude.as_str(),
            ObjectKind::Extrude.schema_version() + 1,
            vec![CORE_CAPABILITY.to_owned()],
            vec![0xf6],
        )
        .to_bytes()
        .expect("serialises");

        let payload = ObjectPayload::from_storage_bytes(&future).expect("header is readable");
        assert!(matches!(payload, ObjectPayload::Unknown(_)));
        assert_eq!(payload.to_storage_bytes().expect("writes back"), future);
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

    #[test]
    fn topology_meaning_hash_tracks_portable_meaning_and_not_the_fallback_hint() {
        let reference = TopologyRef {
            id: StableEntityId::new(),
            owner: ObjectId::new(),
            producer_feature: ObjectId::new(),
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::ExtrudeCap {
                side: CapSide::Start,
            },
            selection: SelectionRule::Exact,
            fallback_signature: None,
        };
        let key = reference.meaning_hash();

        let mut with_hint = reference.clone();
        with_hint.fallback_signature = Some(GeomSignature {
            kind: EntityKind::Face,
            measure: 12.0,
            centroid: Point3::ORIGIN,
        });
        assert_eq!(
            with_hint.meaning_hash(),
            key,
            "a recovery hint became identity"
        );

        let mut changed = Vec::new();
        let mut value = reference.clone();
        value.id = StableEntityId::new();
        changed.push(value.meaning_hash());
        let mut value = reference.clone();
        value.owner = ObjectId::new();
        changed.push(value.meaning_hash());
        let mut value = reference.clone();
        value.producer_feature = ObjectId::new();
        changed.push(value.meaning_hash());
        let mut value = reference.clone();
        value.expected_kind = EntityKind::Edge;
        changed.push(value.meaning_hash());
        let mut value = reference.clone();
        value.output_role = SemanticRole::ExtrudeCap { side: CapSide::End };
        changed.push(value.meaning_hash());
        let mut value = reference;
        value.selection = SelectionRule::AllDerivedFrom {
            ancestor: StableEntityId::new(),
        };
        changed.push(value.meaning_hash());

        assert!(
            changed.into_iter().all(|changed| changed != key),
            "a portable field was absent from the meaning hash"
        );
    }
}
