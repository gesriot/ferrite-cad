// SPDX-License-Identifier: MIT
//! A specific versioned creation request for a full-turn revolution (§27A).
use clap::Args;
use ferritecad_jobs::{
    CreateDocumentRequest, CreatedDocument, FullTurnRevolution, NewDocument,
    create_document_with_kernel,
};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, Result};
use serde::Deserialize;
use std::{path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct RevolveArgs {
    /// UTF-8 JSON request v1: request_version, points_mm ([[x,y],...]),
    /// axis ("sketch_y"), angle ("full_turn"). XY Line polygon, implicit
    /// last-to-first closure; x is the radius and every x must be > 0.
    pub request: PathBuf,
    /// New .fcad destination; never replaced.
    #[arg(short, long)]
    pub output: PathBuf,
    /// Emit JSON v1 (exit 0 published, 2 refused, 7 report delivery failed).
    #[arg(long)]
    pub json: bool,
}

/// The only axis request v1 can name: the sketch's local Y axis.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Axis {
    SketchY,
}

/// The only angle request v1 can name: exactly one full turn.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Angle {
    FullTurn,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    points_mm: Vec<[f64; 2]>,
    axis: Axis,
    angle: Angle,
}

fn result(args: &RevolveArgs) -> Result<CreatedDocument> {
    if args.json {
        crate::json::require_utf8_path(&args.request)?;
        crate::json::require_utf8_path(&args.output)?;
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(&args.request)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(CadError::input("revolve request exceeds 65536 bytes"));
    }
    #[derive(Deserialize)]
    struct Declared {
        request_version: u32,
    }
    // The version first, so a request from a later format is refused as that
    // rather than as whatever field it happens to have that this one lacks.
    let declared: Declared = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid revolve request JSON: {e}")))?;
    if declared.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported revolve request_version; expected 1",
        ));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid revolve request JSON: {e}")))?;
    debug_assert_eq!(input.request_version, 1);
    let (Axis::SketchY, Angle::FullTurn) = (input.axis, input.angle);
    let profile = FullTurnRevolution::new(input.points_mm)?;
    create_document_with_kernel(
        CreateDocumentRequest::new(
            &args.output,
            NewDocument::SketchRevolve(profile),
            "choose a different file name",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
}

pub fn run(args: RevolveArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::CreateSketchRevolve,
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
