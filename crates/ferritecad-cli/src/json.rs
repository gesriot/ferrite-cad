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

mod checkpoint;
pub(crate) mod constraints;
mod fbx;
mod import;
mod recovery;
mod validate;
pub use checkpoint::{CheckpointCopied, CheckpointList};
pub use fbx::ExportedFbx;
pub use import::emit_import;
pub use recovery::{RecoveryList, emit_recovery_extract};
pub use validate::Validated;

const SCHEMA_VERSION: u32 = 1;
/// Delivery failed. An operation may have published; never retry it here.
const EXIT_REPORT_DELIVERY: u8 = 7;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    Inspect,
    Edit,
    Create,
    CreateSketchExtrude,
    CreateSketchRevolve,
    CreateCircleExtrude,
    CreateAnnularExtrude,
    ExportStl,
    ExportFbx,
    ImportStep,
    Validate,
    ListRecovery,
    ExtractRecovery,
    ListCheckpoints,
    CreateCheckpoint,
    DeleteCheckpoint,
    ExtractCheckpoint,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    rebuild: Option<ferritecad_eval::RebuildReport>,
    kind: &'static str,
    message: String,
    causes: Vec<String>,
    #[serde(flatten)]
    rejection: Option<import::ReaderRejection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    constraint_conflict: Option<constraints::Conflict>,
    /// §30M: why a recovery record was refused (`active`, `damaged`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_refusal: Option<&'static str>,
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
            rebuild: ferritecad_eval::RebuildRefusal::from_error(error).map(|r| r.report.clone()),
            kind: error.kind().as_str(),
            message: error.to_string(),
            causes,
            rejection: None,
            constraint_conflict: None,
            recovery_refusal: None,
        }
    }
}

#[derive(Serialize)]
pub struct Inspection {
    rebuild: Option<ferritecad_eval::RebuildReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rebuild_error: Option<Failure>,
    document_id: DocumentId,
    content_version: ContentHash,
    display_units: DisplayUnits,
    distance_unit: &'static str,
    expression_slots: Vec<Slot>,
    features: Vec<Feature>,
    bodies: Vec<Body>,
    sketches: Vec<Sketch>,
    /// Every saved Revolve (§27A), additive. Not a feature entry: `features`
    /// keeps listing Extrudes only, so no consumer of that array can take a
    /// Revolve for one.
    revolves: Vec<RevolveDiscovery>,
    /// Saved Fillet identity, edge and expression facts.
    fillets: Vec<serde_json::Value>,
    /// Saved Chamfer identity, edge and expression facts.
    chamfers: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct Slot {
    object: ObjectId,
    name: Option<String>,
    slot: ferritecad_document::ExpressionSlot,
    expression: ferritecad_document::Expression,
    dimension: ferritecad_types::Dimension,
    display_value: f64,
    display_unit: &'static str,
    refusal: Option<String>,
}

/// One saved Revolve, as the pinned reading found it. Its profile's
/// coordinates are edited through `edit` (§27B); whether that is
/// allowed is the Sketch row's `editable`, not `profile.available`. Its angle
/// is addressed by the `revolve_angle` expression slot.
#[derive(Serialize)]
struct RevolveDiscovery {
    feature_id: ObjectId,
    name: Option<String>,
    body_id: Option<ObjectId>,
    profile_sketch_id: ObjectId,
    regions: Vec<StableEntityId>,
    plane_id: Option<ObjectId>,
    axis: &'static str,
    extent: &'static str,
    operation: &'static str,
    profile: RevolveProfile,
    /// A chosen axis Line, including construction geometry; null for a sketch axis.
    axis_curve_id: Option<StableEntityId>,
    /// §27D, additive: the stored angle of a partial turn, in degrees
    /// (`extent` is then `"partial_turn"`); null for a full turn.
    angle_deg: Option<f64>,
}

#[derive(Serialize)]
struct RevolveProfile {
    /// Whether the stored profile can be read for inspection.
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
    fn new(r: ferritecad_document::RevolveChoice) -> Self {
        Self {
            feature_id: r.feature,
            name: r.name,
            body_id: r.body,
            profile_sketch_id: r.profile_sketch,
            regions: r.regions,
            plane_id: r.plane,
            axis: match r.axis {
                ferritecad_document::RevolveAxis::SketchY => "sketch_y",
                ferritecad_document::RevolveAxis::SketchX => "sketch_x",
                ferritecad_document::RevolveAxis::Line(_) => "line",
                _ => "unknown",
            },
            extent: match r.extent {
                ferritecad_document::RevolveExtent::FullTurn => "full_turn",
                ferritecad_document::RevolveExtent::Partial { .. } => "partial_turn",
                _ => "unknown",
            },
            angle_deg: match r.extent {
                ferritecad_document::RevolveExtent::Partial { angle } => {
                    Some(ferritecad_types::display_degrees(angle.value()))
                }
                _ => None,
            },
            operation: match r.operation {
                ferritecad_document::SolidOperation::NewBody => "new_body",
                ferritecad_document::SolidOperation::Add => "add",
                ferritecad_document::SolidOperation::Cut => "cut",
                ferritecad_document::SolidOperation::Intersect => "intersect",
                _ => "unknown",
            },
            axis_curve_id: match r.axis {
                ferritecad_document::RevolveAxis::Line(id) => Some(id),
                _ => None,
            },
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
    plane_id: ObjectId,
    name: Option<String>,
    curves: Vec<constraints::Curve>,
    constraints: Vec<constraints::Constraint>,
}

#[derive(Serialize)]
struct Body {
    body_id: ObjectId,
    name: Option<String>,
    history: Vec<ObjectId>,
}

pub(crate) fn request_version(bytes: &[u8], what: &str) -> Result<u32> {
    #[derive(Deserialize)]
    struct Declared {
        request_version: u32,
    }
    serde_json::from_slice::<Declared>(bytes)
        .map(|d| d.request_version)
        .map_err(|e| CadError::input(format!("invalid {what} JSON: {e}")))
}

#[derive(Serialize)]
pub struct ExportedStl {
    complete: bool,
    omissions: Vec<StlOmission>,
    rebuild: ferritecad_eval::RebuildReport,
    destination: PathBuf,
    body_id: ObjectId,
    body_name: Option<String>,
    triangles: usize,
    bytes: usize,
    length_unit: &'static str,
}

#[derive(Serialize)]
struct StlOmission {
    body_id: ObjectId,
    body_name: Option<String>,
}

impl From<ferritecad_jobs::StlExport> for ExportedStl {
    fn from(exported: ferritecad_jobs::StlExport) -> Self {
        Self {
            destination: exported.destination,
            complete: exported.complete,
            omissions: exported
                .omissions
                .into_iter()
                .map(|b| StlOmission {
                    body_id: b.id,
                    body_name: b.name,
                })
                .collect(),
            rebuild: exported.rebuild,
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
struct Feature {
    feature_id: ObjectId,
    name: Option<String>,
    operation: ferritecad_document::SolidOperation,
    profile_sketch_id: ObjectId,
    regions: Vec<StableEntityId>,
    previous_feature_id: Option<ObjectId>,
    end_condition: serde_json::Value,
    reversed: bool,
}

#[derive(Serialize)]
pub struct Edited {
    rebuild: ferritecad_eval::RebuildReport,
    destination: PathBuf,
    document_id: DocumentId,
    content_version: ContentHash,
}
impl From<ferritecad_jobs::EditedDocument> for Edited {
    fn from(edited: ferritecad_jobs::EditedDocument) -> Self {
        Self {
            rebuild: edited.rebuild,
            destination: edited.destination,
            document_id: edited.version.document_id,
            content_version: edited.version.content,
        }
    }
}

#[derive(Serialize)]
pub struct Created {
    rebuild: Option<ferritecad_eval::RebuildReport>,
    destination: PathBuf,
    document_id: DocumentId,
}

impl From<ferritecad_jobs::CreatedDocument> for Created {
    fn from(created: ferritecad_jobs::CreatedDocument) -> Self {
        Self {
            rebuild: created.rebuild().cloned(),
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
    pub rebuild: Option<ferritecad_eval::RebuildReport>,
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
            rebuild: created.rebuild().cloned(),
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
    let (rebuild, rebuild_error) = if ferritecad_occt::is_available() {
        let evaluated = (|| -> Result<_> {
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            let built = ferritecad_eval::rebuild_cold(
                &document,
                &mut kernel,
                &ferritecad_kernel::OperationContext::default(),
            )?;
            let report = built.report();
            built.release_all(&mut kernel);
            Ok(report)
        })();
        match evaluated {
            Ok(report) => (Some(report), None),
            Err(error) => (None, Some(Failure::from(&error))),
        }
    } else {
        (None, None)
    };
    let revolves = source
        .revolves
        .into_iter()
        .map(RevolveDiscovery::new)
        .collect();
    let edge_features = |fillet: bool| -> Result<Vec<serde_json::Value>> {
        Ok(document
            .objects()?
            .into_iter()
            .filter_map(|o| {
                let (previous, edge, value) = match o.payload {
                    ferritecad_document::ObjectPayload::Fillet(f) if fillet => {
                        (f.previous, f.edge, f.radius_mm)
                    }
                    ferritecad_document::ObjectPayload::Chamfer(c) if !fillet => {
                        (c.previous, c.edge, c.distance_mm)
                    }
                    _ => return None,
                };
                let unit = document.meta().display_length_unit;
                Some(serde_json::json!({
                    "feature_id": o.id,
                    "name": o.name,
                    "previous_feature_id": previous,
                    "edge": {"feature_id": edge.feature, "joint": edge.joint.segments()},
                    "display_value": value.value() * unit.per_base_unit(),
                    "display_unit": unit.symbol(),
                    "expression": value,
                }))
            })
            .collect())
    };
    let fillets = edge_features(true)?;
    let chamfers = edge_features(false)?;
    let result = Inspection {
        rebuild,
        rebuild_error,
        document_id: source.version.document_id,
        content_version: source.version.content,
        display_units: DisplayUnits {
            length: document.meta().display_length_unit.symbol(),
            angle: document.meta().display_angle_unit.symbol(),
        },
        distance_unit: "mm",
        expression_slots: source
            .expression_slots
            .into_iter()
            .map(|s| {
                let angle = s.dimension == ferritecad_types::Dimension::Angle;
                Slot {
                    object: s.object,
                    name: s.name,
                    slot: s.slot,
                    dimension: s.dimension,
                    display_value: if angle {
                        ferritecad_types::display_degrees(s.expression.value())
                    } else {
                        s.expression.value()
                    },
                    display_unit: if angle { "deg" } else { "mm" },
                    expression: s.expression,
                    refusal: source.refusal.clone().or(s.refusal),
                }
            })
            .collect(),
        sketches: document
            .objects()?
            .into_iter()
            .filter_map(|object| {
                let ferritecad_document::ObjectPayload::Sketch(sketch) = object.payload else {
                    return None;
                };
                Some(Sketch {
                    sketch_id: object.id,
                    plane_id: sketch.plane,
                    name: object.name,
                    curves: sketch.curves.iter().map(constraints::Curve::from).collect(),
                    constraints: sketch
                        .constraints
                        .iter()
                        .map(|c| {
                            constraints::Constraint::display(
                                c,
                                document.meta().display_length_unit,
                                document.meta().display_angle_unit,
                            )
                        })
                        .collect(),
                })
            })
            .collect(),
        features: source
            .objects
            .iter()
            .filter_map(|o| {
                let ferritecad_document::ObjectPayload::Extrude(e) = &o.payload else {
                    return None;
                };
                let distance = |value: f64| {
                    serde_json::json!({
                        "value": value * document.meta().display_length_unit.per_base_unit(),
                        "unit": document.meta().display_length_unit.symbol(),
                    })
                };
                let end_condition = match &e.end_condition {
                    ferritecad_document::EndCondition::Blind { distance: d } => {
                        serde_json::json!({"kind":"blind", "distance":distance(d.value())})
                    }
                    ferritecad_document::EndCondition::Symmetric { distance: d } => {
                        serde_json::json!({"kind":"symmetric", "distance":distance(d.value())})
                    }
                    ferritecad_document::EndCondition::ThroughAll => {
                        serde_json::json!({"kind":"through_all"})
                    }
                    _ => return None,
                };
                Some(Feature {
                    feature_id: o.id,
                    name: o.name.clone(),
                    operation: e.operation,
                    profile_sketch_id: e.profile,
                    regions: e.regions.clone(),
                    previous_feature_id: e.previous,
                    end_condition,
                    reversed: e.reversed,
                })
            })
            .collect(),
        bodies: source
            .objects
            .iter()
            .filter(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Body(_)))
            .map(|o| Body {
                body_id: o.id,
                name: o.name.clone(),
                history: source
                    .model_tree
                    .rows
                    .iter()
                    .filter(|r| r.id.1 == Some(o.id) && r.id.2.is_none())
                    .map(|r| r.id.0)
                    .collect(),
            })
            .collect(),
        revolves,
        fillets,
        chamfers,
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
                        if matches!(operation, Operation::Edit) {
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
