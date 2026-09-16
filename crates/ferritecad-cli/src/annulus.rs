// SPDX-License-Identifier: MIT
//! A specific versioned creation request, not a model interchange format.
use clap::Args;
use ferritecad_jobs::{
    AnnularExtrusion, CreateDocumentRequest, CreatedDocument, NewDocument,
    create_document_with_kernel,
};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, Result};
use serde::Deserialize;
use std::{path::PathBuf, process::ExitCode};

#[derive(Debug, Args)]
pub struct AnnulusArgs {
    /// UTF-8 JSON request v1: schema_version, center_mm ([x,y]), outer_radius_mm,
    /// inner_radius_mm, height_mm. Two concentric analytic XY circles; Blind
    /// extrusion; NewBody. Millimetres throughout.
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
/// `schema_version` rather than `request_version`, following the
/// `create-circle-extrude` request this one extends: the two creation commands
/// spell the version the same way, and each accepts only its own spelling.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema_version: u32,
    center_mm: [f64; 2],
    outer_radius_mm: f64,
    inner_radius_mm: f64,
    height_mm: f64,
}

fn result(args: &AnnulusArgs) -> Result<CreatedDocument> {
    if args.json {
        crate::json::require_utf8_path(&args.request)?;
        crate::json::require_utf8_path(&args.output)?;
    }
    // The same bounded read the circle request uses: at most limit+1 bytes,
    // rather than trusting the metadata of a file read independently.
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(&args.request)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(CadError::input("annulus request exceeds 65536 bytes"));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid annulus request JSON: {e}")))?;
    if input.schema_version != 1 {
        return Err(CadError::unsupported(
            "unsupported annulus request schema_version; expected 1",
        ));
    }
    let annulus = AnnularExtrusion::new(
        input.center_mm,
        input.outer_radius_mm,
        input.inner_radius_mm,
        input.height_mm,
    )?;
    create_document_with_kernel(
        CreateDocumentRequest::new(
            &args.output,
            NewDocument::AnnularExtrude(annulus),
            "choose a different file name",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
}

pub fn run(args: AnnulusArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::CreateAnnularExtrude,
            result.and_then(crate::json::CreatedAnnulus::try_from),
        ))
    } else {
        let made = result?;
        let identities = crate::json::CreatedAnnulus::try_from(made)?;
        println!(
            "created {} ({})\nsketch {}\nextrude {}\nbody {}\nouter circle {}\ninner circle {}",
            identities.destination.display(),
            identities.document_id,
            identities.sketch_id,
            identities.extrude_id,
            identities.body_id,
            identities.outer_curve_id,
            identities.inner_curve_id,
        );
        Ok(ExitCode::SUCCESS)
    }
}
