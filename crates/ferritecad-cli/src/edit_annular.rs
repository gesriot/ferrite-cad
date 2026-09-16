// SPDX-License-Identifier: MIT
//! CLI presentation and request decoding; the copy edit belongs to jobs.
use clap::Args;
use ferritecad_document::{AnnulusEdit, Document, DocumentVersion};
use ferritecad_jobs::{EditAnnulusRequest, EditedAnnulus, edit_annulus_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result, StableEntityId};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct EditAnnularArgs {
    /// Existing FCAD. Read-only; identities are retained in the new copy.
    source: PathBuf,
    /// Exact saved Sketch UUID from inspect --json (not a name).
    #[arg(long)]
    sketch: ObjectId,
    /// Full opaque content_version from inspect --json; required.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: both saved curve UUIDs in their roles, a new shared centre
    /// and both new radii in mm.
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
/// circle and sketch edits beside it spell one; there is no synonym. There is no
/// height here either: the saved extrusion keeps it, and `edit-extrude` changes
/// it. There is one centre, because the class this edits is concentric.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    outer_curve_id: StableEntityId,
    inner_curve_id: StableEntityId,
    center_mm: [f64; 2],
    outer_radius_mm: f64,
    inner_radius_mm: f64,
}

fn result(args: &EditAnnularArgs) -> Result<EditedAnnulus> {
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
        return Err(CadError::input("annulus edit request exceeds 65536 bytes"));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid annulus edit JSON: {e}")))?;
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported annulus edit request_version; expected 1",
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
    let request = EditAnnulusRequest {
        source: args.source.clone(),
        expected,
        sketch: args.sketch,
        edit: AnnulusEdit {
            outer_curve_id: input.outer_curve_id,
            inner_curve_id: input.inner_curve_id,
            center_mm: input.center_mm,
            outer_radius_mm: input.outer_radius_mm,
            inner_radius_mm: input.inner_radius_mm,
        },
        destination: args.output.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_annulus_copy(&request, &mut kernel, &OperationContext::default())
}

/// What the published copy is, named object by object.
///
/// Both circle UUIDs are reported in their roles, so a reader addresses the
/// outer wall and the bore separately without inferring either from the other.
#[derive(Serialize)]
struct Published {
    destination: PathBuf,
    document_id: DocumentId,
    sketch_id: ObjectId,
    outer_curve_id: StableEntityId,
    inner_curve_id: StableEntityId,
}

pub fn run(args: EditAnnularArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::EditAnnular,
            result.map(|r| Published {
                destination: r.destination,
                document_id: r.document_id,
                sketch_id: r.sketch,
                outer_curve_id: r.outer_curve,
                inner_curve_id: r.inner_curve,
            }),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, sketch {}, outer circle {}, inner circle {})",
            r.destination.display(),
            r.document_id,
            r.sketch,
            r.outer_curve,
            r.inner_curve
        );
        Ok(ExitCode::SUCCESS)
    }
}
