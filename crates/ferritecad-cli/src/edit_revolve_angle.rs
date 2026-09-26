// SPDX-License-Identifier: MIT
//! CLI presentation and request decoding; the copy edit belongs to jobs.
use clap::Args;
use ferritecad_document::{Document, DocumentVersion};
use ferritecad_jobs::{EditRevolveAngleRequest, EditedDocument, edit_revolve_angle_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct EditRevolveAngleArgs {
    /// Existing FCAD. Read-only; every identity is retained in the new copy.
    source: PathBuf,
    /// Exact saved partial Revolve feature UUID from inspect --json (not a name).
    #[arg(long)]
    feature: ObjectId,
    /// Full opaque content_version from inspect --json; required.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: {"request_version":1,"angle_deg":N}, N from 0.01 to 359.99
    /// degrees inclusive, stored exactly as given. A full turn is not an angle.
    #[arg(long)]
    request: PathBuf,
    /// New destination, never overwritten. No --force.
    #[arg(short, long)]
    output: PathBuf,
    /// JSON v1; exit 0 published, 2 refused, 7 report lost. Usage remains text.
    #[arg(long)]
    json: bool,
}

/// The request as written. The angle is the only intent: no axis, direction
/// or extent kind, because none of them can change here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    angle_deg: f64,
}

fn result(args: &EditRevolveAngleArgs) -> Result<EditedDocument> {
    if args.json {
        for path in [&args.source, &args.request, &args.output] {
            crate::json::require_utf8_path(path)?;
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&args.request)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(CadError::input("Revolve angle request exceeds 65536 bytes"));
    }
    // An object, by name: serde's derive also takes a struct from a JSON
    // array in field order, and `[1, 90]` is not a request anybody wrote.
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid Revolve angle edit JSON: {e}")))?;
    if !value.is_object() {
        return Err(CadError::input(
            "invalid Revolve angle edit JSON: the request must be an object",
        ));
    }
    let input = Input::deserialize(value)
        .map_err(|e| CadError::input(format!("invalid Revolve angle edit JSON: {e}")))?;
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported Revolve angle edit request_version; expected 1",
        ));
    }
    // Identity only. The job compares the expected content against the exact
    // snapshot copied, and once more against the source path before publish.
    let document = Document::open_read_only(&args.source)?;
    let expected = DocumentVersion {
        document_id: document.meta().document_id,
        content: args.expect_version,
    };
    document.close()?;
    let request = EditRevolveAngleRequest {
        source: args.source.clone(),
        expected,
        feature: args.feature,
        degrees: input.angle_deg,
        destination: args.output.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_revolve_angle_copy(&request, &mut kernel, &OperationContext::default())
}

#[derive(Serialize)]
struct Published {
    destination: PathBuf,
    document_id: DocumentId,
    feature_id: ObjectId,
}

pub fn run(args: EditRevolveAngleArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::EditRevolveAngle,
            result.map(|r| Published {
                destination: r.destination,
                document_id: r.document_id,
                feature_id: r.feature,
            }),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, feature {})",
            r.destination.display(),
            r.document_id,
            r.feature
        );
        Ok(ExitCode::SUCCESS)
    }
}
