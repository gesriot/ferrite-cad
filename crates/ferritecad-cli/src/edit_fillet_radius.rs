// SPDX-License-Identifier: MIT
//! `edit-fillet-radius` (§28B): presentation and request decoding. The copy
//! edit belongs to jobs, and the UI submits the same request.
use clap::Args;
use ferritecad_document::{Document, DocumentVersion};
use ferritecad_jobs::{EditFilletRadiusRequest, EditedFilletRadius, edit_fillet_radius_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf, process::ExitCode};

use crate::json::FilletEdgeDto;

#[derive(Debug, Args)]
pub struct EditFilletRadiusArgs {
    /// Existing FCAD. Read-only; every identity is retained in the new copy.
    source: PathBuf,
    /// Exact saved Fillet feature UUID from inspect --json `fillets[]` (not a name).
    #[arg(long)]
    feature: ObjectId,
    /// Full opaque content_version from inspect --json; required.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: {"request_version":1,"radius_mm":R}, R within the saved
    /// corner's bounds in `radius_edit`. The edge is the saved one.
    #[arg(long)]
    request: PathBuf,
    /// New destination, never overwritten. No --force.
    #[arg(short, long)]
    output: PathBuf,
    /// JSON v1; exit 0 published, 2 refused, 7 report lost. Usage remains text.
    #[arg(long)]
    json: bool,
}

/// The request as written. The radius is the only intent: the edge cannot be
/// named here, so it cannot be retargeted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    radius_mm: f64,
}

fn decode(bytes: &[u8]) -> Result<f64> {
    // An object, by its first token: the derive also takes a struct from a
    // JSON array in field order. The original bytes are then decoded once,
    // strictly, so unknown, duplicate and escaped-duplicate keys all refuse.
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(CadError::input(
            "invalid Fillet radius edit JSON: the request must be an object",
        ));
    }
    let input: Input = serde_json::from_slice(bytes)
        .map_err(|e| CadError::input(format!("invalid Fillet radius edit JSON: {e}")))?;
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported Fillet radius edit request_version; expected 1",
        ));
    }
    Ok(input.radius_mm)
}

fn result(args: &EditFilletRadiusArgs) -> Result<EditedFilletRadius> {
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
        return Err(CadError::input("Fillet radius request exceeds 65536 bytes"));
    }
    let radius_mm = decode(&bytes)?;
    // Identity only. The job compares the expected content against the exact
    // snapshot copied, and once more against the source path before publish.
    let document = Document::open_read_only(&args.source)?;
    let expected = DocumentVersion {
        document_id: document.meta().document_id,
        content: args.expect_version,
    };
    document.close()?;
    let request = EditFilletRadiusRequest {
        source: args.source.clone(),
        expected,
        feature: args.feature,
        radius_mm,
        destination: args.output.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_fillet_radius_copy(&request, &mut kernel, &OperationContext::default())
}

#[derive(Serialize)]
struct Published {
    destination: PathBuf,
    document_id: DocumentId,
    body_id: ObjectId,
    feature_id: ObjectId,
    /// The saved edge, unchanged, in canonical order.
    edge: FilletEdgeDto,
    corner_mm: [f64; 2],
    previous_radius_mm: f64,
    radius_mm: f64,
}

pub fn run(args: EditFilletRadiusArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::EditFilletRadius,
            result.map(|r| Published {
                destination: r.destination,
                document_id: r.document_id,
                body_id: r.body,
                feature_id: r.feature,
                edge: FilletEdgeDto {
                    feature_id: r.edge.feature,
                    joint: r.edge.joint.segments(),
                },
                corner_mm: r.corner_mm,
                previous_radius_mm: r.previous_radius_mm,
                radius_mm: r.radius_mm,
            }),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, fillet {} radius {} -> {} mm on {})",
            r.destination.display(),
            r.document_id,
            r.feature,
            r.previous_radius_mm,
            r.radius_mm,
            r.edge.joint
        );
        Ok(ExitCode::SUCCESS)
    }
}
