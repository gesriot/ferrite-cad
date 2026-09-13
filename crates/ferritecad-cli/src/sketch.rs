// SPDX-License-Identifier: MIT
//! A specific versioned creation request, not a model interchange format.
use clap::Args;
use ferritecad_jobs::{
    CreateDocumentRequest, CreatedDocument, NewDocument, PolygonExtrusion,
    create_document_with_kernel,
};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, Result};
use serde::Deserialize;
use std::{path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct SketchArgs {
    /// UTF-8 JSON request v1: request_version, points_mm ([[x,y],...]), height_mm.
    /// XY Line polygon; implicit last-to-first closure; no repeated closing point.
    pub request: PathBuf,
    /// New .fcad destination; never replaced.
    #[arg(short, long)]
    pub output: PathBuf,
    /// Emit JSON v1 (exit 0 published, 2 refused, 7 report delivery failed).
    #[arg(long)]
    pub json: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    points_mm: Vec<[f64; 2]>,
    height_mm: f64,
}

fn result(args: &SketchArgs) -> Result<CreatedDocument> {
    if args.json {
        crate::json::require_utf8_path(&args.request)?;
        crate::json::require_utf8_path(&args.output)?;
    }
    // Limit parser allocation for this small, explicitly bounded request. Read
    // at most limit+1 rather than trusting metadata on an independently read file.
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(&args.request)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(CadError::input("sketch request exceeds 65536 bytes"));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid sketch request JSON: {e}")))?;
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported sketch request_version; expected 1",
        ));
    }
    let profile = PolygonExtrusion::new(input.points_mm, input.height_mm)?;
    create_document_with_kernel(
        CreateDocumentRequest::new(
            &args.output,
            NewDocument::SketchExtrude(profile),
            "choose a different file name",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
}

pub fn run(args: SketchArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::CreateSketchExtrude,
            result.map(crate::json::Created::from),
        ))
    } else {
        let made = result?;
        println!(
            "created {} ({})",
            made.destination().display(),
            made.document_id()
        );
        Ok(ExitCode::SUCCESS)
    }
}
