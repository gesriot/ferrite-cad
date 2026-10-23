// SPDX-License-Identifier: MIT
//! CLI presentation and request decoding; the copy edit belongs to jobs.
use clap::Args;
use ferritecad_document::{CircleEdit, Document, DocumentVersion};
use ferritecad_jobs::{EditCircleRequest, EditedCircle, edit_circle_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result, StableEntityId};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct EditCircleArgs {
    /// Existing FCAD. Read-only; identities are retained in the new copy.
    source: PathBuf,
    /// Exact saved Sketch UUID from inspect --json (not a name).
    #[arg(long)]
    sketch: ObjectId,
    /// Full opaque content_version from inspect --json; required.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: the saved curve UUID, a new centre and a new radius in mm.
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
/// `request_version` is this command's only version field, spelled the way the
/// sketch edit beside it spells one; there is no synonym. There is no height
/// here either: the saved extrusion keeps it, and `edit with extrude_distance` changes it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    curve_id: StableEntityId,
    center_mm: [f64; 2],
    radius_mm: f64,
}

#[cfg(test)]
mod tests {
    use super::Input;

    #[test]
    fn request_numbers_round_trip_the_canvas_coordinates_without_losing_a_bit() {
        let text = "11.017646992297319";
        let expected: f64 = text.parse().expect("coordinate");
        let request = format!(
            r#"{{"request_version":1,"curve_id":"01890f3e-e712-7a00-8000-000000000001","center_mm":[{text},-7.9911765038513405],"radius_mm":9.427973129520453}}"#
        );
        let parsed: Input = serde_json::from_str(&request).expect("request");
        assert_eq!(parsed.center_mm[0].to_bits(), expected.to_bits());
        let encoded = serde_json::to_string(&parsed.center_mm).expect("encode");
        let decoded: [f64; 2] = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(
            decoded.map(f64::to_bits),
            parsed.center_mm.map(f64::to_bits)
        );
    }
}

fn result(args: &EditCircleArgs) -> Result<EditedCircle> {
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
        return Err(CadError::input("circle edit request exceeds 65536 bytes"));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid circle edit JSON: {e}")))?;
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported circle edit request_version; expected 1",
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
    let request = EditCircleRequest {
        source: args.source.clone(),
        expected,
        sketch: args.sketch,
        edit: CircleEdit {
            curve_id: input.curve_id,
            center_mm: input.center_mm,
            radius_mm: input.radius_mm,
        },
        destination: args.output.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_circle_copy(&request, &mut kernel, &OperationContext::default())
}

#[derive(Serialize)]
struct Published {
    rebuild: ferritecad_eval::RebuildReport,
    destination: PathBuf,
    document_id: DocumentId,
    sketch_id: ObjectId,
    curve_id: StableEntityId,
}

pub fn run(args: EditCircleArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::EditCircle,
            result.map(|r| Published {
                rebuild: r.rebuild,
                destination: r.destination,
                document_id: r.document_id,
                sketch_id: r.sketch,
                curve_id: r.curve,
            }),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, sketch {}, circle {})",
            r.destination.display(),
            r.document_id,
            r.sketch,
            r.curve
        );
        Ok(ExitCode::SUCCESS)
    }
}
