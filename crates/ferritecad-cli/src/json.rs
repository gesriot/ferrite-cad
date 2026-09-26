// SPDX-License-Identifier: MIT
//! Opt-in CLI wire v1. Presentation only: document/jobs own the facts and edit.
//!
//! This schema is independent of .fcad versions and content-version hashing.
//! Keep its DTOs explicit rather than serializing an evolving domain object.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ferritecad_document::{Document, ExtrudeEditSource};
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result, StableEntityId};
use serde::{Deserialize, Serialize};

pub(crate) mod constraints;
mod fbx;
mod import;
mod validate;
pub use fbx::ExportedFbx;
pub use import::emit_import;
pub use validate::Validated;

const SCHEMA_VERSION: u32 = 1;
/// Delivery failed. An operation may have published; never retry it here.
const EXIT_REPORT_DELIVERY: u8 = 7;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    Inspect,
    EditExtrude,
    EditSketchCopy,
    EditCircle,
    EditAnnular,
    EditSketchConstraintsCopy,
    CutCircularCopy,
    EditCircularCut,
    EditRevolveAngle,
    Create,
    CreateSketchExtrude,
    CreateSketchRevolve,
    CreateCircleExtrude,
    CreateAnnularExtrude,
    ExportStl,
    ExportFbx,
    ImportStep,
    Validate,
}

#[derive(Serialize)]
struct Response<T> {
    schema_version: u32,
    operation: Operation,
    #[serde(flatten)]
    outcome: Outcome<T>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Outcome<T> {
    Success { ok: bool, result: T },
    Failure { ok: bool, error: Box<Failure> },
}

#[derive(Serialize)]
struct Failure {
    kind: &'static str,
    message: String,
    causes: Vec<String>,
    #[serde(flatten)]
    rejection: Option<import::ReaderRejection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    constraint_conflict: Option<constraints::Conflict>,
}

impl From<&CadError> for Failure {
    fn from(error: &CadError) -> Self {
        let mut causes = Vec::new();
        let mut source = std::error::Error::source(error);
        while let Some(cause) = source {
            causes.push(cause.to_string());
            source = cause.source();
        }
        Self {
            kind: error.kind().as_str(),
            message: error.to_string(),
            causes,
            rejection: None,
            constraint_conflict: None,
        }
    }
}

#[derive(Serialize)]
pub struct Inspection {
    document_id: DocumentId,
    content_version: ContentHash,
    display_units: DisplayUnits,
    distance_unit: &'static str,
    edit_extrude: EditAvailability,
    features: Vec<Feature>,
    bodies: Vec<Body>,
    sketches: Vec<Sketch>,
    /// Every saved Revolve (§27A), additive. Not a feature entry: `features`
    /// keeps listing Extrudes only, so no consumer of that array can take a
    /// Revolve for one.
    revolves: Vec<RevolveDiscovery>,
}

/// One saved Revolve, as the pinned reading found it. Its profile's
/// coordinates are edited through `edit-sketch-copy` (§27B); whether that is
/// allowed is the Sketch row's `editable`, not `profile.available`. Its angle
/// is edited through `edit-revolve-angle` (§27E), as `angle_edit` says.
#[derive(Serialize)]
struct RevolveDiscovery {
    feature_id: ObjectId,
    name: Option<String>,
    body_id: Option<ObjectId>,
    profile_sketch_id: ObjectId,
    plane_id: Option<ObjectId>,
    axis: &'static str,
    extent: &'static str,
    operation: &'static str,
    profile: RevolveProfile,
    /// §27C, additive: `"radial_clear"` for a part with a bore, whose every
    /// profile point is off the axis, or `"axis_closed"` for a solid part
    /// closed on the axis along `axis_curve_id`. What the Revolve states.
    closure: &'static str,
    /// The saved Line on the axis of a solid part; null for a part with a bore.
    axis_curve_id: Option<StableEntityId>,
    /// §27D, additive: the stored angle of a partial turn, in degrees
    /// (`extent` is then `"partial_turn"`); null for a full turn.
    angle_deg: Option<f64>,
    /// §27E, additive: whether `edit-revolve-angle` accepts this Revolve.
    angle_edit: AngleEditDiscovery,
}

/// What `edit-revolve-angle` would accept about one Revolve, from the same
/// reading. `available` folds in the document-wide refusal, which keeps its
/// priority; `refusal` is this Revolve's own reason and `document_refusal` the
/// shared one, as `circle_edit` and `annulus_edit` report them. The bounds are
/// the domain's, both inclusive; the saved angle stays in `angle_deg`.
#[derive(Serialize)]
struct AngleEditDiscovery {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    min_deg: f64,
    max_deg: f64,
}

impl AngleEditDiscovery {
    fn new(
        choice: ferritecad_document::RevolveAngleChoice,
        document_refusal: Option<String>,
    ) -> Self {
        Self {
            available: choice.refusal.is_none() && document_refusal.is_none(),
            refusal: choice.refusal,
            document_refusal,
            min_deg: ferritecad_document::RevolveAngle::MIN_DEGREES,
            max_deg: ferritecad_document::RevolveAngle::MAX_DEGREES,
        }
    }
}

#[derive(Serialize)]
struct RevolveProfile {
    /// Whether the stored profile is the class this build turns.
    available: bool,
    refusal: Option<String>,
    /// The Lines in stored order, with their saved identities; `null` when the
    /// profile holds anything else.
    segments: Option<Vec<RevolveSegment>>,
}

#[derive(Serialize)]
struct RevolveSegment {
    curve_id: StableEntityId,
    start_mm: [f64; 2],
    end_mm: [f64; 2],
}

impl RevolveDiscovery {
    fn new(r: ferritecad_document::RevolveChoice, angle_edit: AngleEditDiscovery) -> Self {
        Self {
            angle_edit,
            feature_id: r.feature,
            name: r.name,
            body_id: r.body,
            profile_sketch_id: r.profile_sketch,
            plane_id: r.plane,
            axis: match r.axis {
                ferritecad_document::RevolveAxis::SketchY => "sketch_y",
                _ => "unknown",
            },
            extent: match r.extent {
                ferritecad_document::RevolveExtent::FullTurn => "full_turn",
                ferritecad_document::RevolveExtent::Partial { .. } => "partial_turn",
                _ => "unknown",
            },
            angle_deg: match r.extent {
                ferritecad_document::RevolveExtent::Partial { degrees } => Some(degrees.degrees()),
                _ => None,
            },
            operation: match r.operation {
                ferritecad_document::SolidOperation::NewBody => "new_body",
                ferritecad_document::SolidOperation::Add => "add",
                ferritecad_document::SolidOperation::Cut => "cut",
                ferritecad_document::SolidOperation::Intersect => "intersect",
                _ => "unknown",
            },
            closure: if r.axis_segment.is_some() {
                "axis_closed"
            } else {
                "radial_clear"
            },
            axis_curve_id: r.axis_segment,
            profile: RevolveProfile {
                available: r.profile_refusal.is_none(),
                refusal: r.profile_refusal,
                segments: r.segments.map(|lines| {
                    lines
                        .into_iter()
                        .map(|l| RevolveSegment {
                            curve_id: l.curve_id,
                            start_mm: l.start_mm,
                            end_mm: l.end_mm,
                        })
                        .collect()
                }),
            },
        }
    }
}

#[derive(Serialize)]
struct Sketch {
    sketch_id: ObjectId,
    name: Option<String>,
    vertices: Option<Vec<SketchVertex>>,
    /// v1: `null` also when the history holds a ThroughAll Cut; see `cut_history_v2`.
    /// v1 and `_v2`: `null` unless the part is an axis-aligned rectangle; see `_v3`.
    cut_history: Option<SketchCutHistory<BlindDepth, RectangleOnly>>,
    cut_history_v2: Option<SketchCutHistory<ExplicitEnd, RectangleOnly>>,
    /// Any supported polygon, with its real boundary (§26I).
    cut_history_v3: Option<SketchCutHistory<ExplicitEnd, PolygonBoundary>>,
    constraint_edit: constraints::Discovery,
    /// Whether this Sketch's analytic circle can be moved or resized, and the
    /// circle itself. Its own answer: `editable`/`vertices` keep meaning what
    /// they always did, which is whether the Line editor accepts this Sketch.
    circle_edit: CircleDiscovery,
    /// Whether this Sketch's pair of analytic circles can be moved or resized,
    /// and the pair itself. Its own answer beside the others: `editable`,
    /// `vertices`, `constraint_edit` and `circle_edit` keep meaning exactly what
    /// they always did about their own editors.
    annulus_edit: AnnulusDiscovery,
    editable: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    /// §27B, additive: which saved feature turns `vertices` into a solid, and
    /// so which policy `edit-sketch-copy` applies. Null exactly when
    /// `vertices` is.
    profile_feature: Option<ProfileFeature>,
}

/// The feature a coordinate-editable profile feeds, stated by kind.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProfileFeature {
    BlindExtrude {
        feature_id: ObjectId,
        height_mm: f64,
    },
    /// Every profile point strictly off the axis, by more than the clearance.
    FullTurnRevolve {
        feature_id: ObjectId,
        body_id: ObjectId,
        axis: &'static str,
        extent: &'static str,
        axis_clearance_mm: f64,
    },
    /// §27C: a solid part closed on the axis along `axis_curve_id`, whose two
    /// ends are exactly on it; every other point beyond the clearance. A kind
    /// of its own, so a client that knows only the one above stops here
    /// rather than reading it as a profile strictly off the axis.
    FullTurnRevolveAxisClosed {
        feature_id: ObjectId,
        body_id: ObjectId,
        axis: &'static str,
        extent: &'static str,
        axis_curve_id: StableEntityId,
        off_axis_clearance_mm: f64,
    },
    /// §27F: a sector with a bore, edited as a full turn with a bore is, and
    /// turned through the saved `angle_deg`, which the edit keeps. Kinds of
    /// their own, so a client that knows only the full-turn ones stops here
    /// rather than reading a sector as a full turn.
    PartialTurnRevolve {
        feature_id: ObjectId,
        body_id: ObjectId,
        axis: &'static str,
        extent: &'static str,
        angle_deg: f64,
        axis_clearance_mm: f64,
    },
    /// §27F: a solid sector closed on the axis along `axis_curve_id`.
    PartialTurnRevolveAxisClosed {
        feature_id: ObjectId,
        body_id: ObjectId,
        axis: &'static str,
        extent: &'static str,
        angle_deg: f64,
        axis_curve_id: StableEntityId,
        off_axis_clearance_mm: f64,
    },
}

impl ProfileFeature {
    fn of(profile_use: ferritecad_document::SketchProfileUse) -> Self {
        match profile_use {
            ferritecad_document::SketchProfileUse::BlindExtrude { feature, height_mm } => {
                Self::BlindExtrude {
                    feature_id: feature,
                    height_mm,
                }
            }
            ferritecad_document::SketchProfileUse::FullTurnRevolve {
                feature,
                body,
                axis_segment: None,
            } => Self::FullTurnRevolve {
                feature_id: feature,
                body_id: body,
                axis: "sketch_y",
                extent: "full_turn",
                axis_clearance_mm: ferritecad_document::FullTurnRevolution::AXIS_CLEARANCE_MM,
            },
            ferritecad_document::SketchProfileUse::FullTurnRevolve {
                feature,
                body,
                axis_segment: Some(axis),
            } => Self::FullTurnRevolveAxisClosed {
                feature_id: feature,
                body_id: body,
                axis: "sketch_y",
                extent: "full_turn",
                axis_curve_id: axis,
                off_axis_clearance_mm: ferritecad_document::FullTurnRevolution::AXIS_CLEARANCE_MM,
            },
            ferritecad_document::SketchProfileUse::PartialRevolve {
                feature,
                body,
                axis_segment: None,
                degrees,
            } => Self::PartialTurnRevolve {
                feature_id: feature,
                body_id: body,
                axis: "sketch_y",
                extent: "partial_turn",
                angle_deg: degrees.degrees(),
                axis_clearance_mm: ferritecad_document::FullTurnRevolution::AXIS_CLEARANCE_MM,
            },
            ferritecad_document::SketchProfileUse::PartialRevolve {
                feature,
                body,
                axis_segment: Some(axis),
                degrees,
            } => Self::PartialTurnRevolveAxisClosed {
                feature_id: feature,
                body_id: body,
                axis: "sketch_y",
                extent: "partial_turn",
                angle_deg: degrees.degrees(),
                axis_curve_id: axis,
                off_axis_clearance_mm: ferritecad_document::FullTurnRevolution::AXIS_CLEARANCE_MM,
            },
        }
    }
}
/// Additional coordinate policy for the base of a supported nonempty history.
#[derive(Serialize)]
struct SketchCutHistory<E, P> {
    body_id: ObjectId,
    base_feature_id: ObjectId,
    tools: Vec<ExistingCut<E>>,
    wall_clearance_mm: f64,
    #[serde(flatten)]
    profile: P,
}
impl<E: EndForm, P: ProfileForm> SketchCutHistory<E, P> {
    /// `None` when this form cannot describe every tool or the part; see
    /// [`EndForm`] and [`ProfileForm`].
    fn of(h: &ferritecad_document::SketchCutHistory) -> Option<Self> {
        Some(Self {
            profile: P::of(&h.boundary)?,
            body_id: h.body,
            base_feature_id: h.base_feature,
            tools: ExistingCut::all(&h.tools)?,
            wall_clearance_mm: ferritecad_document::WALL_CLEARANCE_MM,
        })
    }
}
#[derive(Serialize)]
struct SketchVertex {
    curve_id: ferritecad_types::StableEntityId,
    start_mm: [f64; 2],
}

/// What `edit-circle` would accept about one Sketch, from the same reading.
///
/// `available` folds in the document-wide refusal, which keeps its priority;
/// `refusal` is this Sketch's own reason and `document_refusal` the shared
/// one, exactly as `constraint_edit` reports them. `circle` is present for a
/// supported Sketch and null otherwise — never an invented circle.
#[derive(Serialize)]
struct CircleDiscovery {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    circle: Option<SavedCircle>,
}
#[derive(Serialize)]
struct SavedCircle {
    curve_id: ferritecad_types::StableEntityId,
    center_mm: [f64; 2],
    radius_mm: f64,
    height_mm: f64,
}
impl CircleDiscovery {
    fn new(choice: ferritecad_document::CircleChoice, document_refusal: Option<String>) -> Self {
        Self {
            available: choice.refusal.is_none() && document_refusal.is_none(),
            refusal: choice.refusal,
            document_refusal,
            circle: choice.circle.map(|c| SavedCircle {
                curve_id: c.curve_id,
                center_mm: c.center_mm,
                radius_mm: c.radius_mm,
                height_mm: c.height_mm,
            }),
        }
    }
}

/// What `edit-annular` would accept about one Sketch, from the same reading.
///
/// `available` folds in the document-wide refusal, which keeps its priority;
/// `refusal` is this Sketch's own reason and `document_refusal` the shared one,
/// exactly as the two discoveries beside it report them. `annulus` is present
/// for a supported Sketch and null otherwise — never an invented pair.
#[derive(Serialize)]
struct AnnulusDiscovery {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    annulus: Option<SavedAnnulus>,
}
/// Both circles as stored, each named in the role the radii give it.
///
/// Both centres are reported because concentric means "within the kernel's
/// linear tolerance", not "bit-identical": a caller that showed one of them as
/// the pair's centre would be saying something the document does not.
#[derive(Serialize)]
struct SavedAnnulus {
    outer_curve_id: ferritecad_types::StableEntityId,
    inner_curve_id: ferritecad_types::StableEntityId,
    center_mm: [f64; 2],
    inner_center_mm: [f64; 2],
    outer_radius_mm: f64,
    inner_radius_mm: f64,
    height_mm: f64,
}
impl AnnulusDiscovery {
    fn new(choice: ferritecad_document::AnnulusChoice, document_refusal: Option<String>) -> Self {
        Self {
            available: choice.refusal.is_none() && document_refusal.is_none(),
            refusal: choice.refusal,
            document_refusal,
            annulus: choice.annulus.map(|a| SavedAnnulus {
                outer_curve_id: a.outer_curve_id,
                inner_curve_id: a.inner_curve_id,
                center_mm: a.center_mm,
                inner_center_mm: a.inner_center_mm,
                outer_radius_mm: a.outer_radius_mm,
                inner_radius_mm: a.inner_radius_mm,
                height_mm: a.height_mm,
            }),
        }
    }
}

#[derive(Serialize)]
struct Body {
    body_id: ObjectId,
    name: Option<String>,
    /// Whether a circular cut can be added to this body, and what the operation
    /// would be cutting into. Its own answer beside the sketch editors, which
    /// keep saying exactly what they always said about their own classes.
    cut_edit: CutDiscovery<BlindDepth>,
    /// The same answer in the form that can describe ThroughAll (§26H).
    cut_edit_v2: CutDiscovery<ExplicitEnd>,
    /// The same answer for any supported polygon, with its real boundary (§26I).
    cut_edit_v3: CutDiscovery<ExplicitEnd, PolygonBoundary>,
}

/// What `cut-circular-copy` would accept about one Body, from the same reading.
#[derive(Serialize)]
struct CutDiscovery<E, P = RectangleExtents> {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    target: Option<SavedCutTarget<E, P>>,
}

/// The part a cut would go into, as stored.
///
/// The plane and the direction are reported rather than left to be assumed: a
/// caller that said "on the base plane, along +Z" without asking would be
/// promising the only thing this slice does as if it were a choice.
#[derive(Serialize)]
struct SavedCutTarget<E, P> {
    body_id: ObjectId,
    plane_id: ObjectId,
    /// The feature the cut would modify, which is the body's tip today.
    tip_feature_id: ObjectId,
    base_feature_id: ObjectId,
    existing_cut: Option<ExistingCut<E>>,
    tools: Vec<ExistingCut<E>>,
    disk_clearance_mm: f64,
    profile_sketch_id: ObjectId,
    height_mm: f64,
    /// v1/`_v2`: `extents_mm`, `[[min_x, min_y], [max_x, max_y]]` of the
    /// rectangular part. `_v3`: `bounds_mm` and the real `boundary`.
    #[serde(flatten)]
    profile: P,
    /// The one direction a cut runs here, said out loud.
    direction: &'static str,
    /// How far the tool must stay from the part's outer wall.
    wall_clearance_mm: f64,
    /// `_v2`/`_v3` only: request versions `cut-circular-copy` accepts.
    #[serde(skip_serializing_if = "Option::is_none")]
    request_versions: Option<&'static [u32]>,
}

/// How one discovery form spells the end of a saved tool.
///
/// JSON v1 discovery has always spelt it as a required `depth_mm` number, and
/// that type is part of its contract. A ThroughAll Cut has no such number, so
/// the v1 form cannot describe a history that holds one: rather than change
/// the field's type or report the computed height as a Blind depth, every v1
/// block that would need it reports itself unavailable in the way it was
/// already allowed to (a `null` object, and a reason where the block has a
/// `refusal`). The additive `_v2` blocks carry the explicit end.
trait EndForm: Serialize + Sized {
    fn of(extent: ferritecad_document::CutExtent) -> Option<Self>;
    /// Request versions a `_v2` block advertises; v1 blocks advertise none.
    fn request_versions(versions: &'static [u32]) -> Option<&'static [u32]>;
}

/// The v1 spelling: a Blind depth, exactly as before §26H.
#[derive(Serialize)]
struct BlindDepth {
    depth_mm: f64,
}

impl EndForm for BlindDepth {
    fn of(extent: ferritecad_document::CutExtent) -> Option<Self> {
        extent.blind_depth_mm().map(|depth_mm| Self { depth_mm })
    }
    fn request_versions(_: &'static [u32]) -> Option<&'static [u32]> {
        None
    }
}

/// The `_v2` spelling: the explicit end, Blind with its depth or ThroughAll.
#[derive(Serialize)]
struct ExplicitEnd {
    extent: Extent,
}

impl EndForm for ExplicitEnd {
    fn of(extent: ferritecad_document::CutExtent) -> Option<Self> {
        Some(Self {
            extent: extent.into(),
        })
    }
    fn request_versions(versions: &'static [u32]) -> Option<&'static [u32]> {
        Some(versions)
    }
}

/// Why a v1 discovery block is unavailable although the operation exists.
fn v1_cannot_describe(block: &str) -> String {
    format!(
        "this Cut history holds a ThroughAll Cut, which the v1 form of this discovery cannot \
         describe; read {block}_v2"
    )
}

/// How one discovery form states the part's outer wall.
///
/// v1 and `_v2` have always stated it as `extents_mm`, the corners of an
/// axis-aligned rectangle, and a consumer of those blocks may take that as the
/// part. The bounding box of any other polygon would be a rectangle the part
/// is not, so those blocks report themselves unavailable for it in the ways
/// they already could, and the additive `_v3` blocks carry the real boundary.
trait ProfileForm: Serialize + Sized {
    fn of(boundary: &ferritecad_document::CutBoundary) -> Option<Self>;
}

/// v1/`_v2`: the extents of a part that is an axis-aligned rectangle.
#[derive(Serialize)]
struct RectangleExtents {
    extents_mm: [[f64; 2]; 2],
}

impl ProfileForm for RectangleExtents {
    fn of(boundary: &ferritecad_document::CutBoundary) -> Option<Self> {
        boundary
            .rectangle_mm()
            .map(|extents_mm| Self { extents_mm })
    }
}

/// v1/`_v2` blocks that never stated the part: present only for a rectangle.
#[derive(Serialize)]
struct RectangleOnly {}

impl ProfileForm for RectangleOnly {
    fn of(boundary: &ferritecad_document::CutBoundary) -> Option<Self> {
        boundary.rectangle_mm().map(|_| Self {})
    }
}

/// `_v3`: the real outer wall, and its bounding box named as bounds only.
#[derive(Serialize)]
struct PolygonBoundary {
    /// `[[min_x, min_y], [max_x, max_y]]`. A range, never proof of containment.
    bounds_mm: [[f64; 2]; 2],
    boundary: Boundary,
}

#[derive(Serialize)]
struct Boundary {
    kind: &'static str,
    orientation: &'static str,
    area_mm2: f64,
    /// Every saved Line in stored order; the part is inside them.
    segments: Vec<BoundarySegment>,
}

#[derive(Serialize)]
struct BoundarySegment {
    curve_id: StableEntityId,
    start_mm: [f64; 2],
    end_mm: [f64; 2],
}

impl ProfileForm for PolygonBoundary {
    fn of(boundary: &ferritecad_document::CutBoundary) -> Option<Self> {
        Some(Self {
            bounds_mm: boundary.bounds_mm(),
            boundary: Boundary {
                kind: "line_polygon",
                orientation: match boundary.orientation() {
                    ferritecad_document::BoundaryOrientation::CounterClockwise => {
                        "counter_clockwise"
                    }
                    ferritecad_document::BoundaryOrientation::Clockwise => "clockwise",
                },
                area_mm2: boundary.area_mm2(),
                segments: boundary
                    .segments()
                    .iter()
                    .map(|s| BoundarySegment {
                        curve_id: s.curve_id,
                        start_mm: s.start_mm,
                        end_mm: s.end_mm,
                    })
                    .collect(),
            },
        })
    }
}

/// Why a v1/`_v2` discovery block is unavailable for a non-rectangular part.
fn cannot_describe_polygon(block: &str) -> String {
    format!(
        "this Cut history's part is not an axis-aligned rectangle, and this form of the \
         discovery can state only a rectangle; read {block}_v3"
    )
}

/// One saved tool.
#[derive(Serialize)]
struct ExistingCut<E> {
    feature_id: ObjectId,
    tool_sketch_id: ObjectId,
    tool_curve_id: StableEntityId,
    center_mm: [f64; 2],
    radius_mm: f64,
    #[serde(flatten)]
    end: E,
}

impl<E: EndForm> ExistingCut<E> {
    fn of(t: &ferritecad_document::SavedCutTool) -> Option<Self> {
        Some(Self {
            feature_id: t.feature,
            tool_sketch_id: t.tool_sketch,
            tool_curve_id: t.tool_curve,
            center_mm: t.center_mm,
            radius_mm: t.radius_mm,
            end: E::of(t.extent)?,
        })
    }
    fn all(tools: &[ferritecad_document::SavedCutTool]) -> Option<Vec<Self>> {
        tools.iter().map(Self::of).collect()
    }
}

/// The end of a circular Cut on the wire: request v2, `_v2` discovery and results.
///
/// `ThroughAll {}` rather than a unit variant: serde lets a unit variant of an
/// internally tagged enum ignore extra fields, so `{"kind":"through_all",
/// "depth_mm":4}` would be accepted with its depth silently dropped.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Extent {
    Blind { depth_mm: f64 },
    ThroughAll {},
}

impl From<Extent> for ferritecad_document::CutExtent {
    fn from(e: Extent) -> Self {
        match e {
            Extent::Blind { depth_mm } => Self::Blind { depth_mm },
            Extent::ThroughAll {} => Self::ThroughAll,
        }
    }
}

impl From<ferritecad_document::CutExtent> for Extent {
    fn from(e: ferritecad_document::CutExtent) -> Self {
        match e {
            ferritecad_document::CutExtent::Blind { depth_mm } => Self::Blind { depth_mm },
            ferritecad_document::CutExtent::ThroughAll => Self::ThroughAll {},
        }
    }
}

/// The version a Cut request declares, read before its shape is.
pub(crate) fn request_version(bytes: &[u8], what: &str) -> Result<u32> {
    #[derive(Deserialize)]
    struct Declared {
        request_version: u32,
    }
    serde_json::from_slice::<Declared>(bytes)
        .map(|d| d.request_version)
        .map_err(|e| CadError::input(format!("invalid {what} JSON: {e}")))
}

impl<E: EndForm, P: ProfileForm> CutDiscovery<E, P> {
    fn new(
        choice: &ferritecad_document::CutChoice,
        document_refusal: Option<String>,
        block: &str,
    ) -> Self {
        let described = choice.target.as_ref().map(|t| {
            let profile = P::of(&t.boundary).ok_or_else(|| cannot_describe_polygon(block))?;
            let tools = || v1_cannot_describe(block);
            Ok(SavedCutTarget {
                body_id: t.body,
                plane_id: t.plane,
                tip_feature_id: t.tip_feature,
                base_feature_id: t.base_feature,
                tools: ExistingCut::all(&t.tools).ok_or_else(tools)?,
                existing_cut: match &t.existing_cut {
                    Some(c) => Some(ExistingCut::of(&c.tool()).ok_or_else(tools)?),
                    None => None,
                },
                request_versions: E::request_versions(&[1, 2]),
                disk_clearance_mm: ferritecad_document::WALL_CLEARANCE_MM,
                profile_sketch_id: t.profile,
                height_mm: t.height_mm,
                profile,
                direction: "+z along the plane normal",
                wall_clearance_mm: ferritecad_document::WALL_CLEARANCE_MM,
            })
        });
        let (target, refusal) = match described {
            Some(Err(reason)) => (None, Some(reason)),
            Some(Ok(t)) => (Some(t), choice.refusal.clone()),
            None => (None, choice.refusal.clone()),
        };
        Self {
            available: refusal.is_none() && document_refusal.is_none(),
            refusal,
            document_refusal,
            target,
        }
    }
}

#[derive(Serialize)]
pub struct ExportedStl {
    destination: PathBuf,
    body_id: ObjectId,
    body_name: Option<String>,
    triangles: usize,
    bytes: usize,
    length_unit: &'static str,
}

impl From<ferritecad_jobs::StlExport> for ExportedStl {
    fn from(exported: ferritecad_jobs::StlExport) -> Self {
        Self {
            destination: exported.destination,
            body_id: exported.body.id,
            body_name: exported.body.name,
            triangles: exported.triangles,
            bytes: exported.bytes,
            length_unit: "mm",
        }
    }
}

#[derive(Serialize)]
struct DisplayUnits {
    length: &'static str,
    angle: &'static str,
}

#[derive(Serialize)]
struct EditAvailability {
    available: bool,
    /// Also explains an empty catalog or a catalog with no supported feature.
    refusal: Option<String>,
    /// Only the document-wide copy_access restriction, separate from a feature.
    document_refusal: Option<String>,
}

#[derive(Serialize)]
struct Feature {
    feature_id: ObjectId,
    name: Option<String>,
    distance_mm: Option<f64>,
    /// Effective availability, including the document-wide copy restriction.
    editable: bool,
    /// Feature-local refusal. None does not override document_refusal.
    refusal: Option<String>,
    /// Whether this feature is a saved circular cut whose tool and depth can be
    /// changed, and what it is today. Its own answer beside `editable`, which
    /// keeps meaning exactly what it always did about `edit-extrude` — and
    /// which still refuses a Cut.
    circular_cut_edit: CutParameterDiscovery<BlindDepth>,
    /// The same answer in the form that can describe ThroughAll (§26H).
    circular_cut_edit_v2: CutParameterDiscovery<ExplicitEnd>,
    /// The same answer for any supported polygon, with its real boundary (§26I).
    circular_cut_edit_v3: CutParameterDiscovery<ExplicitEnd, PolygonBoundary>,
    /// v1: `null` also when the history holds a ThroughAll Cut; see `_v2`.
    /// v1 and `_v2`: `null` unless the part is an axis-aligned rectangle; see `_v3`.
    base_height_edit: Option<BaseHeightDiscovery<BlindDepth>>,
    base_height_edit_v2: Option<BaseHeightDiscovery<ExplicitEnd>>,
    base_height_edit_v3: Option<BaseHeightDiscovery<ExplicitEnd, PolygonBoundary>>,
}

/// Absolute tools and protected historical/descendant floor UUIDs for base height edits.
#[derive(Serialize)]
struct BaseHeightDiscovery<E, P = RectangleExtents> {
    body_id: ObjectId,
    profile_sketch_id: ObjectId,
    /// v1/`_v2`: `extents_mm`; `_v3`: `bounds_mm` and `boundary`.
    #[serde(flatten)]
    profile: P,
    tools: Vec<ExistingCut<E>>,
    protected_floors: Vec<ProtectedFloor>,
}
#[derive(Serialize)]
struct ProtectedFloor {
    feature_id: ObjectId,
    reference_ids: Vec<ferritecad_types::StableEntityId>,
}
impl<E: EndForm, P: ProfileForm> BaseHeightDiscovery<E, P> {
    /// `None` when this form cannot describe every tool or the part; see
    /// [`EndForm`] and [`ProfileForm`].
    fn of(h: &ferritecad_document::BaseHeightContext) -> Option<Self> {
        Some(Self {
            body_id: h.body,
            profile_sketch_id: h.profile,
            profile: P::of(&h.boundary)?,
            tools: ExistingCut::all(&h.tools)?,
            protected_floors: h
                .protected_floors
                .iter()
                .map(|p| ProtectedFloor {
                    feature_id: p.feature,
                    reference_ids: p.references.clone(),
                })
                .collect(),
        })
    }
}

/// What `edit-circular-cut` would accept about one feature, from the same
/// reading.
#[derive(Serialize)]
struct CutParameterDiscovery<E, P = RectangleExtents> {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    saved: Option<SavedCircularCut<E, P>>,
}

/// The cut as stored, and what an edit of it is measured against.
///
/// Every identity it names is one the copy keeps. The plane and the direction
/// are repeated here rather than assumed for the reason the Cut catalogue
/// beside it repeats them.
#[derive(Serialize)]
struct SavedCircularCut<E, P> {
    feature_id: ObjectId,
    body_id: ObjectId,
    plane_id: ObjectId,
    /// The feature this cut modifies; unchanged by an edit of its numbers.
    previous_feature_id: ObjectId,
    base_feature_id: ObjectId,
    tip_feature_id: ObjectId,
    protected_floor_reference_ids: Vec<StableEntityId>,
    neighboring_tool: Option<ExistingCut<E>>,
    tools: Vec<ExistingCut<E>>,
    disk_clearance_mm: f64,
    /// The part's own profile; unchanged by an edit of its numbers.
    profile_sketch_id: ObjectId,
    /// The sketch holding the tool circle, whose numbers an edit rewrites.
    tool_sketch_id: ObjectId,
    /// The circle inside it. A request must name exactly this identity.
    tool_curve_id: StableEntityId,
    center_mm: [f64; 2],
    radius_mm: f64,
    /// v1: `depth_mm`; `_v2`: `extent`.
    #[serde(flatten)]
    end: E,
    /// `_v2` only: request versions that express this Cut without loss.
    #[serde(skip_serializing_if = "Option::is_none")]
    request_versions: Option<&'static [u32]>,
    height_mm: f64,
    /// v1/`_v2`: `extents_mm`, `[[min_x, min_y], [max_x, max_y]]` of the
    /// rectangular part. `_v3`: `bounds_mm` and the real `boundary`.
    #[serde(flatten)]
    profile: P,
    /// The one direction a cut runs here, said out loud.
    direction: &'static str,
    /// How far the tool must stay from the part's outer wall.
    wall_clearance_mm: f64,
    /// Whether the saved cut stops inside the part.
    leaves_a_floor: bool,
    /// The saved name of that floor, when there is one.
    floor_reference_id: Option<StableEntityId>,
    /// Whether the depth may be raised to the part's height.
    ///
    /// False for a cut that has a floor: the face its saved reference names
    /// would stop existing, and this slice refuses rather than dropping a saved
    /// name or resolving it to the part's far side. A cut that already runs
    /// through the part may be shortened, because that only adds a name.
    through_allowed: bool,
}

impl<E: EndForm, P: ProfileForm> CutParameterDiscovery<E, P> {
    fn new(
        choice: &ferritecad_document::CutParameterChoice,
        document_refusal: Option<String>,
        block: &str,
    ) -> Self {
        let described = choice.saved.as_ref().map(|c| {
            let profile = P::of(&c.boundary).ok_or_else(|| cannot_describe_polygon(block))?;
            let tools = || v1_cannot_describe(block);
            Ok(SavedCircularCut {
                feature_id: c.feature,
                body_id: c.body,
                plane_id: c.plane,
                previous_feature_id: c.previous_feature,
                base_feature_id: c.base_feature,
                tools: ExistingCut::all(&c.tools).ok_or_else(tools)?,
                tip_feature_id: c.tip_feature,
                protected_floor_reference_ids: c.protected_floor_references.clone(),
                neighboring_tool: match &c.neighboring_tool {
                    Some(t) => Some(ExistingCut::of(t).ok_or_else(tools)?),
                    None => None,
                },
                disk_clearance_mm: ferritecad_document::WALL_CLEARANCE_MM,
                profile_sketch_id: c.profile_sketch,
                tool_sketch_id: c.tool_sketch,
                tool_curve_id: c.tool_curve,
                center_mm: c.center_mm,
                radius_mm: c.radius_mm,
                end: E::of(c.extent).ok_or_else(tools)?,
                request_versions: E::request_versions(c.request_versions()),
                height_mm: c.height_mm,
                profile,
                direction: "+z along the plane normal",
                wall_clearance_mm: ferritecad_document::WALL_CLEARANCE_MM,
                leaves_a_floor: c.leaves_a_floor(),
                floor_reference_id: c.floor_reference,
                through_allowed: c.through_allowed(),
            })
        });
        let (saved, refusal) = match described {
            Some(Err(reason)) => (None, Some(reason)),
            Some(Ok(s)) => (Some(s), choice.refusal.clone()),
            None => (None, choice.refusal.clone()),
        };
        Self {
            available: refusal.is_none() && document_refusal.is_none(),
            refusal,
            document_refusal,
            saved,
        }
    }
}

#[derive(Serialize)]
pub struct Edited {
    destination: PathBuf,
    document_id: DocumentId,
    feature_id: ObjectId,
}

impl From<ferritecad_jobs::EditedDocument> for Edited {
    fn from(edited: ferritecad_jobs::EditedDocument) -> Self {
        Self {
            destination: edited.destination,
            document_id: edited.document_id,
            feature_id: edited.feature,
        }
    }
}

#[derive(Serialize)]
pub struct Created {
    destination: PathBuf,
    document_id: DocumentId,
}

impl From<ferritecad_jobs::CreatedDocument> for Created {
    fn from(created: ferritecad_jobs::CreatedDocument) -> Self {
        Self {
            destination: created.destination().to_path_buf(),
            document_id: created.document_id(),
        }
    }
}

/// What `create-annular-extrude` published, named object by object.
///
/// Every identifier comes from the creation itself rather than from a second
/// reading of the file, so nothing here is matched by name, by object order or
/// by which circle happens to be bigger. Both circles are named, because a
/// reader of this report has to be able to address the outer wall and the bore
/// separately and neither is findable from the other.
#[derive(Serialize)]
pub struct CreatedAnnulus {
    pub destination: PathBuf,
    pub document_id: DocumentId,
    pub sketch_id: ObjectId,
    pub extrude_id: ObjectId,
    pub body_id: ObjectId,
    pub outer_curve_id: ferritecad_types::StableEntityId,
    pub inner_curve_id: ferritecad_types::StableEntityId,
}

impl TryFrom<ferritecad_jobs::CreatedDocument> for CreatedAnnulus {
    type Error = CadError;

    /// Fallible because the creation reports identities only for the content
    /// that has them. A missing set means this command was handed a creation of
    /// some other kind, which is a defect here rather than something to paper
    /// over with a placeholder UUID.
    fn try_from(created: ferritecad_jobs::CreatedDocument) -> Result<Self> {
        let annulus = created.annulus().ok_or_else(|| {
            CadError::kernel("the creation published no annular identities to report")
        })?;
        Ok(Self {
            destination: created.destination().to_path_buf(),
            document_id: created.document_id(),
            sketch_id: annulus.sketch,
            extrude_id: annulus.extrude,
            body_id: annulus.body,
            outer_curve_id: annulus.outer_curve,
            inner_curve_id: annulus.inner_curve,
        })
    }
}

pub fn require_utf8_path(path: &Path) -> Result<()> {
    if path.to_str().is_none() {
        return Err(CadError::input(
            "JSON v1 requires UTF-8 source and output paths",
        ));
    }
    Ok(())
}

pub fn inspect(path: &Path) -> Result<Inspection> {
    require_utf8_path(path)?;
    let document = Document::open_read_only(path)?;
    // One pinned read, one catalog and one content hash. In particular, do not
    // call read_extrude_source(path) or the text renderer beside this reading.
    let source = ExtrudeEditSource::read(&document)?;
    let refusal = source.unavailable_reason().map(str::to_owned);
    let mut constraint_choices: std::collections::BTreeMap<_, _> = source
        .constraint_sketches
        .into_iter()
        .map(|c| (c.sketch, c))
        .collect();
    let mut circle_choices: std::collections::BTreeMap<_, _> = source
        .circle_sketches
        .into_iter()
        .map(|c| (c.sketch, c))
        .collect();
    let mut annulus_choices: std::collections::BTreeMap<_, _> = source
        .annulus_sketches
        .into_iter()
        .map(|c| (c.sketch, c))
        .collect();
    let mut cut_choices: std::collections::BTreeMap<_, _> =
        source.cut_bodies.into_iter().map(|c| (c.body, c)).collect();
    let mut cut_parameter_choices: std::collections::BTreeMap<_, _> = source
        .cut_features
        .into_iter()
        .map(|c| (c.feature, c))
        .collect();
    let mut angle_choices: std::collections::BTreeMap<_, _> = source
        .revolve_angles
        .into_iter()
        .map(|c| (c.feature, c))
        .collect();
    let revolves = source
        .revolves
        .into_iter()
        .map(|r| {
            let angle = angle_choices
                .remove(&r.feature)
                .expect("same snapshot Revolve catalogue");
            RevolveDiscovery::new(r, AngleEditDiscovery::new(angle, source.refusal.clone()))
        })
        .collect();
    let result = Inspection {
        document_id: source.version.document_id,
        content_version: source.version.content,
        display_units: DisplayUnits {
            length: document.meta().display_length_unit.symbol(),
            angle: document.meta().display_angle_unit.symbol(),
        },
        distance_unit: "mm",
        edit_extrude: EditAvailability {
            available: refusal.is_none(),
            refusal,
            document_refusal: source.refusal.clone(),
        },
        sketches: source
            .sketches
            .into_iter()
            .map(|s| Sketch {
                constraint_edit: constraints::Discovery::new(
                    constraint_choices
                        .remove(&s.sketch)
                        .expect("same snapshot Sketch catalogue"),
                    source.refusal.clone(),
                ),
                circle_edit: CircleDiscovery::new(
                    circle_choices
                        .remove(&s.sketch)
                        .expect("same snapshot Sketch catalogue"),
                    source.refusal.clone(),
                ),
                annulus_edit: AnnulusDiscovery::new(
                    annulus_choices
                        .remove(&s.sketch)
                        .expect("same snapshot Sketch catalogue"),
                    source.refusal.clone(),
                ),
                sketch_id: s.sketch,
                name: s.name,
                editable: source.refusal.is_none() && s.refusal.is_none(),
                refusal: s.refusal,
                document_refusal: source.refusal.clone(),
                cut_history: s.cut_history.as_ref().and_then(SketchCutHistory::of),
                cut_history_v2: s.cut_history.as_ref().and_then(SketchCutHistory::of),
                cut_history_v3: s.cut_history.as_ref().and_then(SketchCutHistory::of),
                profile_feature: s.profile_use.map(ProfileFeature::of),
                vertices: s.vertices.map(|vs| {
                    vs.into_iter()
                        .map(|v| SketchVertex {
                            curve_id: v.curve_id,
                            start_mm: v.start_mm,
                        })
                        .collect()
                }),
            })
            .collect(),
        features: source
            .features
            .into_iter()
            .map(|feature| {
                let choice = cut_parameter_choices
                    .remove(&feature.feature)
                    .expect("same snapshot feature catalogue");
                Feature {
                    circular_cut_edit: CutParameterDiscovery::new(
                        &choice,
                        source.refusal.clone(),
                        "circular_cut_edit",
                    ),
                    circular_cut_edit_v2: CutParameterDiscovery::new(
                        &choice,
                        source.refusal.clone(),
                        "circular_cut_edit",
                    ),
                    circular_cut_edit_v3: CutParameterDiscovery::new(
                        &choice,
                        source.refusal.clone(),
                        "circular_cut_edit",
                    ),
                    base_height_edit: feature
                        .cut_history
                        .as_ref()
                        .and_then(BaseHeightDiscovery::of),
                    base_height_edit_v2: feature
                        .cut_history
                        .as_ref()
                        .and_then(BaseHeightDiscovery::of),
                    base_height_edit_v3: feature
                        .cut_history
                        .as_ref()
                        .and_then(BaseHeightDiscovery::of),
                    feature_id: feature.feature,
                    name: feature.name,
                    distance_mm: feature.distance_mm,
                    editable: source.refusal.is_none() && feature.refusal.is_none(),
                    refusal: feature.refusal,
                }
            })
            .collect(),
        bodies: {
            // Project the same Cut catalogue the UI receives; do not read and
            // classify the objects again for this JSON view.
            ferritecad_jobs::stl_bodies(&document)?
                .into_iter()
                .map(|body| {
                    let choice = cut_choices
                        .remove(&body.id)
                        .expect("same snapshot Body catalogue");
                    Body {
                        body_id: body.id,
                        name: body.name,
                        cut_edit: CutDiscovery::new(&choice, source.refusal.clone(), "cut_edit"),
                        cut_edit_v2: CutDiscovery::new(&choice, source.refusal.clone(), "cut_edit"),
                        cut_edit_v3: CutDiscovery::new(&choice, source.refusal.clone(), "cut_edit"),
                    }
                })
                .collect()
        },
        revolves,
    };
    document.close()?;
    Ok(result)
}

/// Runs after the operation has completed, even when stdout is already closed.
/// Serializing or delivering its report cannot undo publication or rerun work.
pub fn emit<T: Serialize>(operation: Operation, result: Result<T>) -> ExitCode {
    emit_with_exit(operation, result.map(|result| (result, 0)))
}

/// A published noticed import (4) or partial FBX (6) is successful. Delivery failure
/// still takes precedence, through exactly the same envelope and fallible I/O.
pub fn emit_with_exit<T: Serialize>(operation: Operation, result: Result<(T, u8)>) -> ExitCode {
    let (outcome, exit) = match result {
        Ok((result, exit)) => (Outcome::Success { ok: true, result }, ExitCode::from(exit)),
        Err(error) => {
            // Diagnostics are best-effort. A closed stderr must not prevent
            // the operation error from reaching a still-readable JSON stdout.
            let _ = crate::report(&error);
            (
                Outcome::Failure {
                    ok: false,
                    error: {
                        let mut failure = Failure::from(&error);
                        if matches!(operation, Operation::EditSketchConstraintsCopy) {
                            failure.constraint_conflict =
                                ferritecad_eval::SketchConflict::of(&error)
                                    .map(constraints::Conflict::from);
                        }
                        Box::new(failure)
                    },
                },
                ExitCode::from(crate::EXIT_FAILED),
            )
        }
    };
    emit_outcome(operation, outcome, exit)
}

// The single serialization/delivery path also serves a typed reader rejection.
fn emit_outcome<T: Serialize>(
    operation: Operation,
    outcome: Outcome<T>,
    exit: ExitCode,
) -> ExitCode {
    let response = Response {
        schema_version: SCHEMA_VERSION,
        operation,
        outcome,
    };
    let delivered = (|| -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        let mut stdout = io::stdout().lock();
        stdout.write_all(&bytes)?;
        stdout.flush()?;
        Ok(())
    })();
    if let Err(error) = delivered {
        // Use fallible I/O here too: losing the diagnostic pipe must not panic.
        let _ = writeln!(
            io::stderr().lock(),
            "error [io]: JSON report delivery failed: {error}; the operation has already completed; a file may have been published"
        );
        return ExitCode::from(EXIT_REPORT_DELIVERY);
    }
    exit
}

#[cfg(test)]
mod extent_tests {
    use super::Extent;

    #[test]
    fn a_cut_extent_is_strict_in_both_kinds() {
        for (text, expected) in [
            (r#"{"kind":"through_all"}"#, Some(Extent::ThroughAll {})),
            (
                r#"{"kind":"blind","depth_mm":4.5}"#,
                Some(Extent::Blind { depth_mm: 4.5 }),
            ),
            (r#"{"kind":"through_all","depth_mm":4}"#, None),
            (r#"{"kind":"blind","depth_mm":4,"x":1}"#, None),
            (r#"{"kind":"blind"}"#, None),
            (r#"{"kind":"through"}"#, None),
        ] {
            assert_eq!(
                serde_json::from_str::<Extent>(text).ok(),
                expected,
                "{text}"
            );
        }
        assert_eq!(
            serde_json::to_string(&Extent::ThroughAll {}).expect("json"),
            r#"{"kind":"through_all"}"#
        );
    }
}
