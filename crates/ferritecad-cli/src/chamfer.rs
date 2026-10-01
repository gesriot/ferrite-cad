// SPDX-License-Identifier: MIT
//! `chamfer-edge-copy` (§29A): presentation and request decoding. The copy
//! operation belongs to jobs, and the UI submits the same request.
use clap::Args;
use ferritecad_document::{
    Document, DocumentVersion, EdgeChamfer, SemanticRole, SweptEdge, TopologyRef,
};
use ferritecad_jobs::{AddedEdgeChamfer, EdgeChamferRequest, chamfer_edge_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{
    CadError, ContentHash, DocumentId, ObjectId, ProfileJoint, Result, StableEntityId,
};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf, process::ExitCode};

use crate::json::FilletEdgeDto;

#[derive(Debug, Args)]
pub struct ChamferArgs {
    /// Existing FCAD. Read-only; every identity is retained in the new copy.
    source: PathBuf,
    /// Exact saved Body UUID from inspect --json (not a name).
    #[arg(long)]
    body: ObjectId,
    /// Full opaque content_version from inspect --json; required.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: `edge` (`feature_id` and the two Line UUIDs of the corner,
    /// `joint`, as `bodies[].chamfer_edge` lists them) and `distance_mm`: the
    /// distance in millimetres from the edge along each of its two faces.
    #[arg(long)]
    request: PathBuf,
    /// New destination, never overwritten. No --force.
    #[arg(short, long)]
    output: PathBuf,
    /// JSON v1; exit 0 published, 2 refused, 7 report lost. Usage remains text.
    #[arg(long)]
    json: bool,
}

/// The request as written. One edge, one distance: nothing here can name a
/// chain, a cap edge, a second distance, an angle or a position.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    edge: FilletEdgeDto,
    distance_mm: f64,
}

fn decode(bytes: &[u8]) -> Result<EdgeChamfer> {
    // One strict decode of the original bytes: the derived shapes refuse
    // unknown and duplicate keys at every level, escaped duplicates included.
    let input: Input = serde_json::from_slice(bytes)
        .map_err(|e| CadError::input(format!("invalid chamfer request JSON: {e}")))?;
    // The derive also takes a struct from a JSON array in field order. The
    // request and its edge are objects; only the joint is a pair.
    let objects = match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(serde_json::Value::Object(map)) => map.get("edge").is_some_and(|e| e.is_object()),
        _ => false,
    };
    if !objects {
        return Err(CadError::input(
            "invalid chamfer request JSON: the request and its edge must be objects",
        ));
    }
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported chamfer request_version; expected 1",
        ));
    }
    let [a, b] = input.edge.joint;
    Ok(EdgeChamfer {
        edge: SweptEdge {
            feature: input.edge.feature_id,
            joint: ProfileJoint::new(a, b)?,
        },
        distance_mm: input.distance_mm,
    })
}

fn result(args: &ChamferArgs) -> Result<AddedEdgeChamfer> {
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
        return Err(CadError::input("chamfer request exceeds 65536 bytes"));
    }
    let chamfer = decode(&bytes)?;
    // Identity only. The job compares the expected content against the exact
    // snapshot copied, and once more against the source path before publish.
    let document = Document::open_read_only(&args.source)?;
    let expected = DocumentVersion {
        document_id: document.meta().document_id,
        content: args.expect_version,
    };
    document.close()?;
    let request = EdgeChamferRequest {
        source: args.source.clone(),
        expected,
        body: args.body,
        chamfer,
        destination: args.output.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    chamfer_edge_copy(&request, &mut kernel, &OperationContext::default())
}

/// One name the new Chamfer persisted, so a caller can refer to the cut face
/// without reading the document again.
#[derive(Serialize)]
struct NewReference {
    reference_id: StableEntityId,
    role: &'static str,
}

fn role_name(reference: &TopologyRef) -> &'static str {
    match reference.output_role {
        SemanticRole::EdgeChamferFace { .. } => "edge_chamfer_face",
        SemanticRole::OriginCap { .. } => "origin_cap",
        SemanticRole::OriginSide { .. } => "origin_side",
        _ => "other",
    }
}

#[derive(Serialize)]
struct Published {
    destination: PathBuf,
    document_id: DocumentId,
    body_id: ObjectId,
    /// The new Chamfer, which the Body's tip now is.
    feature_id: ObjectId,
    /// The feature it cuts, which was the tip before.
    previous_feature_id: ObjectId,
    /// The edge cut, in canonical order.
    edge: FilletEdgeDto,
    /// The corner of the plate, which is exactly the stored one: the profile
    /// is free or closure-only.
    corner_mm: [f64; 2],
    adjacent_lengths_mm: [f64; 2],
    distance_unit: &'static str,
    distance_mm: f64,
    references: Vec<NewReference>,
}

pub fn run(args: ChamferArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::ChamferEdgeCopy,
            result.map(|r| Published {
                destination: r.destination,
                document_id: r.document_id,
                body_id: r.body,
                feature_id: r.feature,
                previous_feature_id: r.previous,
                edge: FilletEdgeDto {
                    feature_id: r.corner.feature,
                    joint: r.corner.joint.segments(),
                },
                corner_mm: r.corner.corner_mm,
                adjacent_lengths_mm: r.corner.adjacent_lengths_mm,
                distance_unit: "mm",
                distance_mm: r.distance_mm,
                references: r
                    .references
                    .iter()
                    .map(|reference| NewReference {
                        reference_id: reference.id,
                        role: role_name(reference),
                    })
                    .collect(),
            }),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, body {}, chamfer {} of {} mm on {} after {})",
            r.destination.display(),
            r.document_id,
            r.body,
            r.feature,
            r.distance_mm,
            r.corner.joint,
            r.previous
        );
        Ok(ExitCode::SUCCESS)
    }
}
