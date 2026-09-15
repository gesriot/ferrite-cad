// SPDX-License-Identifier: MIT
//! A specific versioned creation request, not a model interchange format.
use clap::Args;
use ferritecad_jobs::{
    CircleExtrusion, CreateDocumentRequest, CreatedDocument, NewDocument,
    create_document_with_kernel,
};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, Result};
use serde::Deserialize;
use std::{path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct CircleArgs {
    /// UTF-8 JSON request v1: schema_version, center_mm ([x,y]), radius_mm, height_mm.
    /// One analytic XY circle; Blind extrusion; NewBody. Millimetres throughout.
    pub request: PathBuf,
    /// New .fcad destination; never replaced.
    #[arg(short, long)]
    pub output: PathBuf,
    /// Emit JSON v1 (exit 0 published, 2 refused, 7 report delivery failed).
    #[arg(long)]
    pub json: bool,
}

/// The request as written, before any of it means millimetres.
///
/// `schema_version` rather than the polygon request's `request_version`: this
/// is the field this command's published contract names, and a command that
/// accepted both would have two spellings of one fact.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema_version: u32,
    center_mm: [f64; 2],
    radius_mm: f64,
    height_mm: f64,
}

fn result(args: &CircleArgs) -> Result<CreatedDocument> {
    if args.json {
        crate::json::require_utf8_path(&args.request)?;
        crate::json::require_utf8_path(&args.output)?;
    }
    // The same bounded read the polygon request uses: at most limit+1 bytes,
    // rather than trusting the metadata of a file read independently.
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(&args.request)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(CadError::input("circle request exceeds 65536 bytes"));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid circle request JSON: {e}")))?;
    if input.schema_version != 1 {
        return Err(CadError::unsupported(
            "unsupported circle request schema_version; expected 1",
        ));
    }
    let circle = CircleExtrusion::new(input.center_mm, input.radius_mm, input.height_mm)?;
    create_document_with_kernel(
        CreateDocumentRequest::new(
            &args.output,
            NewDocument::CircleExtrude(circle),
            "choose a different file name",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
}

pub fn run(args: CircleArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::CreateCircleExtrude,
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
