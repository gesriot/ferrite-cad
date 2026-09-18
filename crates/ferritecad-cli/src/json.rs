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
use serde::Serialize;

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
    Create,
    CreateSketchExtrude,
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
}

#[derive(Serialize)]
struct Sketch {
    sketch_id: ObjectId,
    name: Option<String>,
    vertices: Option<Vec<SketchVertex>>,
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
    cut_edit: CutDiscovery,
}

/// What `cut-circular-copy` would accept about one Body, from the same reading.
#[derive(Serialize)]
struct CutDiscovery {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    target: Option<SavedCutTarget>,
}

/// The part a cut would go into, as stored.
///
/// The plane and the direction are reported rather than left to be assumed: a
/// caller that said "on the base plane, along +Z" without asking would be
/// promising the only thing this slice does as if it were a choice.
#[derive(Serialize)]
struct SavedCutTarget {
    body_id: ObjectId,
    plane_id: ObjectId,
    /// The feature the cut would modify, which is the body's tip today.
    tip_feature_id: ObjectId,
    profile_sketch_id: ObjectId,
    height_mm: f64,
    /// `[[min_x, min_y], [max_x, max_y]]` of the rectangular part, in mm, so a
    /// form can offer a centre inside it without guessing.
    extents_mm: [[f64; 2]; 2],
    /// The one direction a cut runs here, said out loud.
    direction: &'static str,
    /// How far the tool must stay from the part's outer wall.
    wall_clearance_mm: f64,
}

impl CutDiscovery {
    fn new(choice: ferritecad_document::CutChoice, document_refusal: Option<String>) -> Self {
        Self {
            available: choice.refusal.is_none() && document_refusal.is_none(),
            refusal: choice.refusal,
            document_refusal,
            target: choice.target.map(|t| SavedCutTarget {
                body_id: t.body,
                plane_id: t.plane,
                tip_feature_id: t.tip_feature,
                profile_sketch_id: t.profile,
                height_mm: t.height_mm,
                extents_mm: t.extents_mm,
                direction: "+z along the plane normal",
                wall_clearance_mm: ferritecad_document::WALL_CLEARANCE_MM,
            }),
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
    circular_cut_edit: CutParameterDiscovery,
}

/// What `edit-circular-cut` would accept about one feature, from the same
/// reading.
#[derive(Serialize)]
struct CutParameterDiscovery {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    saved: Option<SavedCircularCut>,
}

/// The cut as stored, and what an edit of it is measured against.
///
/// Every identity it names is one the copy keeps. The plane and the direction
/// are repeated here rather than assumed for the reason the Cut catalogue
/// beside it repeats them.
#[derive(Serialize)]
struct SavedCircularCut {
    feature_id: ObjectId,
    body_id: ObjectId,
    plane_id: ObjectId,
    /// The feature this cut modifies; unchanged by an edit of its numbers.
    previous_feature_id: ObjectId,
    /// The part's own profile; unchanged by an edit of its numbers.
    profile_sketch_id: ObjectId,
    /// The sketch holding the tool circle, whose numbers an edit rewrites.
    tool_sketch_id: ObjectId,
    /// The circle inside it. A request must name exactly this identity.
    tool_curve_id: StableEntityId,
    center_mm: [f64; 2],
    radius_mm: f64,
    depth_mm: f64,
    height_mm: f64,
    /// `[[min_x, min_y], [max_x, max_y]]` of the rectangular part, in mm.
    extents_mm: [[f64; 2]; 2],
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

impl CutParameterDiscovery {
    fn new(
        choice: ferritecad_document::CutParameterChoice,
        document_refusal: Option<String>,
    ) -> Self {
        Self {
            available: choice.refusal.is_none() && document_refusal.is_none(),
            refusal: choice.refusal,
            document_refusal,
            saved: choice.saved.map(|c| SavedCircularCut {
                feature_id: c.feature,
                body_id: c.body,
                plane_id: c.plane,
                previous_feature_id: c.base_feature,
                profile_sketch_id: c.profile_sketch,
                tool_sketch_id: c.tool_sketch,
                tool_curve_id: c.tool_curve,
                center_mm: c.center_mm,
                radius_mm: c.radius_mm,
                depth_mm: c.depth_mm,
                height_mm: c.height_mm,
                extents_mm: c.extents_mm,
                direction: "+z along the plane normal",
                wall_clearance_mm: ferritecad_document::WALL_CLEARANCE_MM,
                leaves_a_floor: c.depth_mm < c.height_mm,
                floor_reference_id: c.floor_reference,
                through_allowed: c.through_allowed(),
            }),
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
            .map(|feature| Feature {
                circular_cut_edit: CutParameterDiscovery::new(
                    cut_parameter_choices
                        .remove(&feature.feature)
                        .expect("same snapshot feature catalogue"),
                    source.refusal.clone(),
                ),
                feature_id: feature.feature,
                name: feature.name,
                distance_mm: feature.distance_mm,
                editable: source.refusal.is_none() && feature.refusal.is_none(),
                refusal: feature.refusal,
            })
            .collect(),
        bodies: {
            // Project the same Cut catalogue the UI receives; do not read and
            // classify the objects again for this JSON view.
            ferritecad_jobs::stl_bodies(&document)?
                .into_iter()
                .map(|body| Body {
                    body_id: body.id,
                    name: body.name,
                    cut_edit: CutDiscovery::new(
                        cut_choices
                            .remove(&body.id)
                            .expect("same snapshot Body catalogue"),
                        source.refusal.clone(),
                    ),
                })
                .collect()
        },
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
