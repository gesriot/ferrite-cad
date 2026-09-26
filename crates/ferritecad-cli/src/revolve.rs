// SPDX-License-Identifier: MIT
//! A specific versioned creation request for a revolution: a full turn
//! (§27A, request v1 or v2) or a partial angle (§27D, request v2 only).
use clap::Args;
use ferritecad_jobs::{
    CreateDocumentRequest, CreatedDocument, FullTurnRevolution, NewDocument, RevolveAngle,
    create_document_with_kernel,
};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, Result};
use serde::Deserialize;
use std::{path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct RevolveArgs {
    /// UTF-8 JSON request. v1: request_version, points_mm ([[x,y],...]),
    /// axis ("sketch_y"), angle ("full_turn"). v2: the same with extent
    /// ({"kind":"full_turn"} or {"kind":"angle","degrees":N}, 0.01 <= N <=
    /// 359.99) in place of angle. XY Line polygon, implicit last-to-first
    /// closure; x is the radius: every x > 1e-6, or exactly one whole Line on
    /// x = 0.
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

/// Request v2 (§27D): the turn stated as an extent, a full turn or a partial
/// angle in degrees. There is no `angle` field: v1's spelling is not accepted
/// here, so no request is read two ways.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputV2 {
    #[serde(rename = "request_version")]
    _request_version: u32,
    points_mm: Vec<[f64; 2]>,
    axis: Axis,
    extent: Extent,
}

/// `FullTurn {}` rather than a unit variant: serde lets a unit variant of an
/// internally tagged enum ignore extra fields, so `{"kind":"full_turn",
/// "degrees":90}` would be accepted with its angle silently dropped.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Extent {
    FullTurn {},
    Angle { degrees: f64 },
}

/// Decodes request v1 or v2 into the content the create job publishes.
fn decode(bytes: &[u8]) -> Result<NewDocument> {
    match crate::json::request_version(bytes, "revolve request")? {
        1 => {
            let input: Input = serde_json::from_slice(bytes)
                .map_err(|e| CadError::input(format!("invalid revolve request JSON: {e}")))?;
            debug_assert_eq!(input.request_version, 1);
            let (Axis::SketchY, Angle::FullTurn) = (input.axis, input.angle);
            Ok(NewDocument::SketchRevolve(FullTurnRevolution::new(
                input.points_mm,
            )?))
        }
        2 => {
            let input: InputV2 = serde_json::from_slice(bytes)
                .map_err(|e| CadError::input(format!("invalid revolve request JSON: {e}")))?;
            let Axis::SketchY = input.axis;
            let profile = FullTurnRevolution::new(input.points_mm)?;
            Ok(match input.extent {
                Extent::FullTurn {} => NewDocument::SketchRevolve(profile),
                Extent::Angle { degrees } => NewDocument::SketchPartialRevolve {
                    profile,
                    angle: RevolveAngle::new(degrees)?,
                },
            })
        }
        // The version first, so a request from a later format is refused as
        // that rather than as whatever field it happens to have that this one
        // lacks.
        _ => Err(CadError::unsupported(
            "unsupported revolve request_version; expected 1 or 2",
        )),
    }
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
    let content = decode(&bytes)?;
    create_document_with_kernel(
        CreateDocumentRequest::new(&args.output, content, "choose a different file name"),
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
