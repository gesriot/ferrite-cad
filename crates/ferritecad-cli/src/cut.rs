// SPDX-License-Identifier: MIT
//! CLI presentation and request decoding; the copy operation belongs to jobs.
use clap::Args;
use ferritecad_document::{CircularCut, CutExtent, Document, DocumentVersion};
use ferritecad_jobs::{AddedCircularCut, CircularCutRequest, circular_cut_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result, StableEntityId};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct CutArgs {
    /// Existing FCAD. Read-only; every identity is retained in the new copy.
    source: PathBuf,
    /// Exact saved Body UUID from inspect --json (not a name).
    #[arg(long)]
    body: ObjectId,
    /// Full opaque content_version from inspect --json; required.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: the tool circle's centre and radius in mm on the part's own
    /// base XY plane, and the finite Blind depth in mm it is cut to along +Z.
    /// Request v2 replaces the depth with an explicit `extent`: Blind with a
    /// depth, or ThroughAll.
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
/// every other edit beside it spells one. There is no plane here and no face:
/// this slice cuts on the part's own base XY datum, along its normal, and a
/// field that could name something else would promise attachment nothing
/// implements. There is no `through` either — the depth is stated, and a cut
/// that happens to equal the part's height is a hole because the numbers say
/// so, not because a flag did. Request v2 exists for the one intent v1 cannot
/// state: a Cut that stays through the part whatever height it later has.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    /// Read by `request_version` before this shape is chosen.
    #[serde(rename = "request_version")]
    _request_version: u32,
    center_mm: [f64; 2],
    radius_mm: f64,
    depth_mm: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputV2 {
    #[serde(rename = "request_version")]
    _request_version: u32,
    center_mm: [f64; 2],
    radius_mm: f64,
    extent: crate::json::Extent,
}

/// Decodes request v1 or v2 into the one domain value both mean.
fn decode(bytes: &[u8]) -> Result<CircularCut> {
    match crate::json::request_version(bytes, "cut request")? {
        1 => {
            let input: Input = serde_json::from_slice(bytes)
                .map_err(|e| CadError::input(format!("invalid cut request JSON: {e}")))?;
            Ok(CircularCut {
                center_mm: input.center_mm,
                radius_mm: input.radius_mm,
                extent: CutExtent::Blind {
                    depth_mm: input.depth_mm,
                },
            })
        }
        2 => {
            let input: InputV2 = serde_json::from_slice(bytes)
                .map_err(|e| CadError::input(format!("invalid cut request JSON: {e}")))?;
            Ok(CircularCut {
                center_mm: input.center_mm,
                radius_mm: input.radius_mm,
                extent: input.extent.into(),
            })
        }
        _ => Err(CadError::unsupported(
            "unsupported cut request_version; expected 1 or 2",
        )),
    }
}

fn result(args: &CutArgs) -> Result<AddedCircularCut> {
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
        return Err(CadError::input("cut request exceeds 65536 bytes"));
    }
    let cut = decode(&bytes)?;
    // Identity only. The job compares the expected content against the exact
    // snapshot copied, and once more against the source path before publish.
    let document = Document::open_read_only(&args.source)?;
    let expected = DocumentVersion {
        document_id: document.meta().document_id,
        content: args.expect_version,
    };
    document.close()?;
    let request = CircularCutRequest {
        source: args.source.clone(),
        expected,
        body: args.body,
        cut,
        destination: args.output.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    circular_cut_copy(&request, &mut kernel, &OperationContext::default())
}

/// What the published copy is, named object by object.
///
/// The body keeps the identity it had, which is the point of the operation;
/// the feature it now tips at and the feature that one modifies are both
/// reported, so a reader can follow the history rather than infer it from the
/// order rows happen to sit in.
#[derive(Serialize)]
struct Published {
    destination: PathBuf,
    document_id: DocumentId,
    body_id: ObjectId,
    feature_id: ObjectId,
    sketch_id: ObjectId,
    tool_curve_id: StableEntityId,
    previous_feature_id: ObjectId,
    extent: crate::json::Extent,
}

pub fn run(args: CutArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::CutCircularCopy,
            result.map(|r| Published {
                destination: r.destination,
                document_id: r.document_id,
                body_id: r.body,
                feature_id: r.feature,
                sketch_id: r.sketch,
                tool_curve_id: r.tool_curve,
                previous_feature_id: r.previous,
                extent: r.extent.into(),
            }),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, body {}, cut {} after {})",
            r.destination.display(),
            r.document_id,
            r.body,
            r.feature,
            r.previous
        );
        Ok(ExitCode::SUCCESS)
    }
}
