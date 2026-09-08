// SPDX-License-Identifier: MIT
//! Opt-in CLI wire v1. Presentation only: document/jobs own the facts and edit.
//!
//! This schema is independent of .fcad versions and content-version hashing.
//! Keep its DTOs explicit rather than serializing an evolving domain object.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ferritecad_document::{Document, ExtrudeEditSource};
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result};
use serde::Serialize;

const SCHEMA_VERSION: u32 = 1;
/// Delivery failed. A create or edit may have published; never retry it here.
const EXIT_REPORT_DELIVERY: u8 = 7;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    Inspect,
    EditExtrude,
    Create,
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
    Failure { ok: bool, error: Failure },
}

#[derive(Serialize)]
struct Failure {
    kind: &'static str,
    message: String,
    causes: Vec<String>,
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
        features: source
            .features
            .into_iter()
            .map(|feature| Feature {
                feature_id: feature.feature,
                name: feature.name,
                distance_mm: feature.distance_mm,
                editable: source.refusal.is_none() && feature.refusal.is_none(),
                refusal: feature.refusal,
            })
            .collect(),
    };
    document.close()?;
    Ok(result)
}

/// Runs after the operation has completed, even when stdout is already closed.
/// Serializing or delivering its report cannot undo publication or rerun work.
pub fn emit<T: Serialize>(operation: Operation, result: Result<T>) -> ExitCode {
    let (outcome, exit) = match result {
        Ok(result) => (Outcome::Success { ok: true, result }, ExitCode::SUCCESS),
        Err(error) => {
            // Diagnostics are best-effort. A closed stderr must not prevent
            // the operation error from reaching a still-readable JSON stdout.
            let _ = crate::report(&error);
            (
                Outcome::Failure {
                    ok: false,
                    error: Failure::from(&error),
                },
                ExitCode::from(crate::EXIT_FAILED),
            )
        }
    };
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
