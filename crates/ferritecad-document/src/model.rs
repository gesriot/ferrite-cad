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
    CadError, CanonicalHasher, ContentHash, Dimension, ImportedSourceId, ObjectId, Point3, Result,
    StableEntityId, Transform, normalize_f64,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::envelope::{Envelope, UnknownObject};

/// The capability every object this build writes depends on.
pub const CORE_CAPABILITY: &str = "core.part.v1";

/// The capability an [`ImportedStep`] object depends on.
///
/// Declared separately from [`CORE_CAPABILITY`] so a build that predates this
/// slice reaches the right conclusion on its own: it does not recognise the
/// capability, so it opens the document read-only and preserves the object it
/// cannot read, rather than rewriting a document whose source-of-truth bytes it
/// has no idea are there.
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
    /// type without losing what it means.
    pub fn required_capabilities(self) -> Vec<String> {
        match self {
            Self::ImportedStep => vec![IMPORTED_STEP_CAPABILITY.to_owned()],
            _ => vec![CORE_CAPABILITY.to_owned()],
        }
    }

    /// Layout version of this object type's payload.
    pub fn schema_version(self) -> u32 {
        1
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

/// Profile geometry on a plane.
///
/// Constraints are deliberately absent at this schema version: the solver
/// arrives in its own stage, and adding a constraint list before it exists
/// would mean guessing at its representation. Adding one later is an object
/// schema version bump, not a file format break.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    /// The datum plane this sketch lies on.
    pub plane: ObjectId,
    pub curves: Vec<SketchCurve>,
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
    #[serde(default)]
    pub source_name: Option<String>,
    /// The handle-free projection of the scene that import produced.
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
            known => known
                .kind()
                .map(ObjectKind::required_capabilities)
                .unwrap_or_else(|| vec![CORE_CAPABILITY.to_owned()]),
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
            Self::ImportedStep(v) => Envelope::encode(name, version, capabilities, v)?,
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

        // A payload whose layout or capability contract differs from the one
        // this build writes is not something to guess at. Keeping it verbatim
        // also prevents a same-version type with a future capability from
        // being re-written without that capability.
        if envelope.schema_version != kind.schema_version()
            || envelope.required_capabilities != kind.required_capabilities()
        {
            return Ok(Self::Unknown(UnknownObject::new(envelope, bytes.to_vec())));
        }

        let payload = match kind {
            ObjectKind::Parameter => Self::Parameter(envelope.decode()?),
            ObjectKind::DatumPlane => Self::DatumPlane(envelope.decode()?),
            ObjectKind::Sketch => Self::Sketch(envelope.decode()?),
            ObjectKind::Body => Self::Body(envelope.decode()?),
            ObjectKind::Extrude => Self::Extrude(envelope.decode()?),
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
            Self::Sketch(sketch) => {
                let mut seen = BTreeSet::new();
                for curve in &sketch.curves {
                    if !seen.insert(curve.id) {
                        return Err(CadError::input(format!(
                            "sketch contains duplicate curve id {}",
                            curve.id
                        )));
                    }
                    curve.geometry.validate()?;
                }
                Ok(())
            }
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
}
