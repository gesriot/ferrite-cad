// SPDX-License-Identifier: MIT
//! `edit-chamfer-distance` (§29A): presentation and request decoding. The copy
//! edit belongs to jobs, and the UI submits the same request.
use clap::Args;
use ferritecad_document::{Document, DocumentVersion};
use ferritecad_jobs::{
    EditChamferDistanceRequest, EditedChamferDistance, edit_chamfer_distance_copy,
};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf, process::ExitCode};

use crate::json::FilletEdgeDto;

#[derive(Debug, Args)]
pub struct EditChamferDistanceArgs {
    /// Existing FCAD. Read-only; every identity is retained in the new copy.
    source: PathBuf,
    /// Exact saved Chamfer feature UUID from inspect --json `chamfers[]` (not a name).
    #[arg(long)]
    feature: ObjectId,
    /// Full opaque content_version from inspect --json; required.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: {"request_version":1,"distance_mm":D}, D within the saved
    /// corner's bounds in `distance_edit`, in millimetres along each face.
    /// The edge is the saved one.
    #[arg(long)]
    request: PathBuf,
    /// New destination, never overwritten. No --force.
    #[arg(short, long)]
    output: PathBuf,
    /// JSON v1; exit 0 published, 2 refused, 7 report lost. Usage remains text.
    #[arg(long)]
    json: bool,
}

/// The request as written. The distance is the only intent: the edge cannot be
/// named here, so it cannot be retargeted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    distance_mm: f64,
}

fn decode(bytes: &[u8]) -> Result<f64> {
    // An object, by its first token: the derive also takes a struct from a
    // JSON array in field order. The original bytes are then decoded once,
    // strictly, so unknown, duplicate and escaped-duplicate keys all refuse.
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(CadError::input(
            "invalid Chamfer distance edit JSON: the request must be an object",
        ));
    }
    let input: Input = serde_json::from_slice(bytes)
        .map_err(|e| CadError::input(format!("invalid Chamfer distance edit JSON: {e}")))?;
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported Chamfer distance edit request_version; expected 1",
        ));
    }
    Ok(input.distance_mm)
}

fn result(args: &EditChamferDistanceArgs) -> Result<EditedChamferDistance> {
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
        return Err(CadError::input(
            "Chamfer distance request exceeds 65536 bytes",
        ));
    }
    let distance_mm = decode(&bytes)?;
    // Identity only. The job compares the expected content against the exact
    // snapshot copied, and once more against the source path before publish.
    let document = Document::open_read_only(&args.source)?;
    let expected = DocumentVersion {
        document_id: document.meta().document_id,
        content: args.expect_version,
    };
    document.close()?;
    let request = EditChamferDistanceRequest {
        source: args.source.clone(),
        expected,
        feature: args.feature,
        distance_mm,
        destination: args.output.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_chamfer_distance_copy(&request, &mut kernel, &OperationContext::default())
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
    distance_unit: &'static str,
    previous_distance_mm: f64,
    distance_mm: f64,
    previous_feature_id: ObjectId,
}

pub fn run(args: EditChamferDistanceArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::EditChamferDistance,
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
                distance_unit: "mm",
                previous_distance_mm: r.previous_distance_mm,
                distance_mm: r.distance_mm,
                previous_feature_id: r.previous,
            }),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, chamfer {} distance {} -> {} mm on {})",
            r.destination.display(),
            r.document_id,
            r.feature,
            r.previous_distance_mm,
            r.distance_mm,
            r.edge.joint
        );
        Ok(ExitCode::SUCCESS)
    }
}
