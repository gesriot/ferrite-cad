// SPDX-License-Identifier: MIT
//! CLI presentation and request decoding; the copy edit belongs to jobs.
use clap::Args;
use ferritecad_document::{
    CircularCutEdit, CutExtent, Document, DocumentVersion, ExtentVocabulary,
};
use ferritecad_jobs::{EditCircularCutRequest, EditedCircularCut, edit_circular_cut_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result, StableEntityId};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct EditCutArgs {
    /// Existing FCAD. Read-only; every identity is retained in the new copy.
    source: PathBuf,
    /// Exact saved Cut feature UUID from inspect --json (not a name).
    #[arg(long)]
    feature: ObjectId,
    /// Full opaque content_version from inspect --json; required.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: the saved tool curve UUID, a new centre and radius in mm on
    /// the part's own base XY plane, and a new finite Blind depth in mm along
    /// +Z. Request v2 replaces the depth with an explicit `extent`. A v1
    /// request is refused for a Cut saved as ThroughAll.
    #[arg(long)]
    request: PathBuf,
    /// New destination, never overwritten. No --force.
    #[arg(short, long)]
    output: PathBuf,
    /// JSON v1; exit 0 published, 2 refused, 7 report lost. Usage remains text.
    #[arg(long)]
    json: bool,
}

/// The request as written, before any of it means millimetres.
///
/// `request_version` is this command's only version field, spelled the way
/// every other edit beside it spells one. The curve is named as well as the
/// feature, for the reason the saved circle editor names one: a caller sends
/// back the identity it read rather than whatever the document happens to hold
/// when the job runs. There is no plane here and no face — this slice edits a
/// cut on the part's own base XY datum — and no `through` flag: the depth is
/// stated, and a cut that equals the part's height runs through it because the
/// numbers say so.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    /// Read by `request_version` before this shape is chosen.
    #[serde(rename = "request_version")]
    _request_version: u32,
    tool_curve_id: StableEntityId,
    center_mm: [f64; 2],
    radius_mm: f64,
    depth_mm: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputV2 {
    #[serde(rename = "request_version")]
    _request_version: u32,
    tool_curve_id: StableEntityId,
    center_mm: [f64; 2],
    radius_mm: f64,
    extent: crate::json::Extent,
}

/// Decodes request v1 or v2. What v1 could not say travels with the edit, so
/// preparation refuses to read it as "make this ThroughAll Cut Blind".
fn decode(bytes: &[u8]) -> Result<CircularCutEdit> {
    match crate::json::request_version(bytes, "cut edit request")? {
        1 => {
            let input: Input = serde_json::from_slice(bytes)
                .map_err(|e| CadError::input(format!("invalid cut edit request JSON: {e}")))?;
            Ok(CircularCutEdit {
                tool_curve: input.tool_curve_id,
                center_mm: input.center_mm,
                radius_mm: input.radius_mm,
                extent: CutExtent::Blind {
                    depth_mm: input.depth_mm,
                },
                vocabulary: ExtentVocabulary::BlindOnly,
            })
        }
        2 => {
            let input: InputV2 = serde_json::from_slice(bytes)
                .map_err(|e| CadError::input(format!("invalid cut edit request JSON: {e}")))?;
            Ok(CircularCutEdit {
                tool_curve: input.tool_curve_id,
                center_mm: input.center_mm,
                radius_mm: input.radius_mm,
                extent: input.extent.into(),
                vocabulary: ExtentVocabulary::BlindOrThroughAll,
            })
        }
        _ => Err(CadError::unsupported(
            "unsupported cut edit request_version; expected 1 or 2",
        )),
    }
}

fn result(args: &EditCutArgs) -> Result<EditedCircularCut> {
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
        return Err(CadError::input("cut edit request exceeds 65536 bytes"));
    }
    let edit = decode(&bytes)?;
    // Identity only. The job compares the expected content against the exact
    // snapshot copied, and once more against the source path before publish.
    let document = Document::open_read_only(&args.source)?;
    let expected = DocumentVersion {
        document_id: document.meta().document_id,
        content: args.expect_version,
    };
    document.close()?;
    let request = EditCircularCutRequest {
        source: args.source.clone(),
        expected,
        cut: args.feature,
        edit,
        destination: args.output.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_circular_cut_copy(&request, &mut kernel, &OperationContext::default())
}

/// What the published copy is, named object by object.
///
/// Every identity here is one the source already had — that is the point of the
/// operation — and `leaves_a_floor` says which of the two shapes the edited cut
/// is, so a caller need not recompute it from two numbers.
#[derive(Serialize)]
struct Published {
    destination: PathBuf,
    document_id: DocumentId,
    body_id: ObjectId,
    feature_id: ObjectId,
    sketch_id: ObjectId,
    tool_curve_id: StableEntityId,
    previous_feature_id: ObjectId,
    leaves_a_floor: bool,
    extent: crate::json::Extent,
}

pub fn run(args: EditCutArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::EditCircularCut,
            result.map(|r| Published {
                destination: r.destination,
                document_id: r.document_id,
                body_id: r.body,
                feature_id: r.feature,
                sketch_id: r.sketch,
                tool_curve_id: r.tool_curve,
                previous_feature_id: r.previous,
                leaves_a_floor: r.leaves_a_floor,
                extent: r.extent.into(),
            }),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, body {}, cut {} {})",
            r.destination.display(),
            r.document_id,
            r.body,
            r.feature,
            if r.leaves_a_floor {
                "leaves a floor"
            } else {
                "runs through the part"
            }
        );
        Ok(ExitCode::SUCCESS)
    }
}
