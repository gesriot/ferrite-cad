// SPDX-License-Identifier: MIT
use std::{io::Read, path::PathBuf, process::ExitCode};

use clap::Args;
use ferritecad_document::{Document, DocumentVersion, ExpressionChange, ExpressionSlot};
use ferritecad_jobs::{EditRequest, EditedDocument};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, ObjectId, Quantity, Result, Unit};
use serde::Deserialize;

#[derive(Debug, Args)]
pub struct EditArgs {
    /// Source document, read without modification.
    source: PathBuf,
    /// Complete content_version from inspect --json.
    #[arg(long)]
    expect_version: ContentHash,
    /// JSON object with request_version: 1 and an ordered changes array.
    #[arg(long)]
    request: PathBuf,
    /// New destination; existing files are refused.
    #[arg(short, long)]
    output: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    changes: Vec<Change>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    object: ObjectId,
    slot: ExpressionSlot,
    value: f64,
    unit: String,
}

fn result(args: &EditArgs) -> Result<EditedDocument> {
    if args.json {
        for path in [&args.source, &args.request, &args.output] {
            crate::json::require_utf8_path(path)?;
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&args.request)?
        .take(1_048_577)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err(CadError::input("edit request exceeds 1048576 bytes"));
    }
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(CadError::input("edit request must be a JSON object"));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input_because("invalid edit request JSON", e))?;
    if input.request_version != 1 {
        return Err(CadError::input("edit request_version must be 1"));
    }
    let changes = input
        .changes
        .into_iter()
        .map(|change| {
            let unit: Unit = change.unit.parse()?;
            if unit.dimension() != change.slot.dimension() {
                return Err(CadError::input(format!(
                    "slot {} requires {}, got {}",
                    change.slot,
                    change.slot.dimension(),
                    unit.dimension()
                )));
            }
            ExpressionChange::constant(
                change.object,
                change.slot,
                Quantity::new(change.value, unit)?.base_value(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let document = Document::open_read_only(&args.source)?;
    let expected = DocumentVersion {
        document_id: document.meta().document_id,
        content: args.expect_version,
    };
    document.close()?;
    ferritecad_jobs::edit_copy(
        &EditRequest {
            source: args.source.clone(),
            expected,
            changes,
            destination: args.output.clone(),
        },
        &mut ferritecad_occt::OcctKernel::new()?,
        &OperationContext::default(),
    )
}

pub fn run(args: EditArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::Edit,
            result.map(crate::json::Edited::from),
        ))
    } else {
        let edited = result?;
        println!(
            "saved {} ({}, version {})",
            edited.destination.display(),
            edited.version.document_id,
            edited.version.content
        );
        Ok(ExitCode::SUCCESS)
    }
}
