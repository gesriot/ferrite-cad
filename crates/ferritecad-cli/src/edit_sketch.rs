// SPDX-License-Identifier: MIT
//! CLI presentation and request decoding; the copy edit belongs to jobs.
use clap::Args;
use ferritecad_document::{Document, DocumentVersion, SketchVertex};
use ferritecad_jobs::{EditSketchRequest, EditedSketch, edit_sketch_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result, StableEntityId};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct EditSketchArgs {
    /// Existing FCAD. Read-only; identities are retained in the new copy.
    source: PathBuf,
    /// Exact saved Sketch UUID from inspect --json (not a name).
    #[arg(long)]
    sketch: ObjectId,
    /// Full opaque content_version from inspect --json; required.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: all curve_id/start_mm entries in saved order, implicit closure.
    #[arg(long)]
    request: PathBuf,
    /// New destination, never overwritten. No --force.
    #[arg(short, long)]
    output: PathBuf,
    /// JSON v1; exit 0 published, 2 refused, 7 report lost. Usage remains text.
    #[arg(long)]
    json: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    vertices: Vec<Vertex>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Vertex {
    curve_id: StableEntityId,
    start_mm: [f64; 2],
}

fn result(args: &EditSketchArgs) -> Result<EditedSketch> {
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
        return Err(CadError::input("sketch request exceeds 65536 bytes"));
    }
    // One strict decode of the original bytes: the derived shapes refuse
    // unknown and duplicate keys at every level, escaped duplicates included,
    // which a detour through `Value` would silently collapse (§27E review).
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid sketch edit JSON: {e}")))?;
    // The derive also takes a struct from a JSON array in field order;
    // `[1, [...]]` is not a request anybody wrote, at the top or per vertex.
    // Only the shape is read here — the request itself is `input`.
    let objects = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(serde_json::Value::Object(map)) => map
            .get("vertices")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|all| all.iter().all(serde_json::Value::is_object)),
        _ => false,
    };
    if !objects {
        return Err(CadError::input(
            "invalid sketch edit JSON: the request and each vertex must be objects",
        ));
    }
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported sketch edit request_version; expected 1",
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
    let request = EditSketchRequest {
        source: args.source.clone(),
        expected,
        sketch: args.sketch,
        vertices: input
            .vertices
            .into_iter()
            .map(|v| SketchVertex {
                curve_id: v.curve_id,
                start_mm: v.start_mm,
            })
            .collect(),
        destination: args.output.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_sketch_copy(&request, &mut kernel, &OperationContext::default())
}

#[derive(Serialize)]
struct Published {
    destination: PathBuf,
    document_id: DocumentId,
    sketch_id: ObjectId,
}

pub fn run(args: EditSketchArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::EditSketchCopy,
            result.map(|r| Published {
                destination: r.destination,
                document_id: r.document_id,
                sketch_id: r.sketch,
            }),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, sketch {})",
            r.destination.display(),
            r.document_id,
            r.sketch
        );
        Ok(ExitCode::SUCCESS)
    }
}
