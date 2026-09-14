// SPDX-License-Identifier: MIT
//! Text/JSON prepare one typed request; jobs owns solver/copy/publication.
use clap::Args;
use ferritecad_document::{
    AddLineConstraint, Document, DocumentVersion, LineConstraintKind, LineLengthMm,
    SketchConstraintEdits,
};
use ferritecad_jobs::{
    EditSketchConstraintsRequest, EditedSketchConstraints, edit_sketch_constraints_copy,
};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};
use serde::Deserialize;
use std::{io::Read, path::PathBuf, process::ExitCode};
#[derive(Debug, Args)]
pub struct EditConstraintsArgs {
    /// Saved native XY Line Sketch document; the source remains unchanged.
    source: PathBuf,
    #[arg(long)]
    sketch: ObjectId,
    /// Full content_version from the same inspect snapshot as all chosen UUIDs.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: remove exact constraint UUIDs, then add Line H/V or length in mm.
    #[arg(long)]
    request: PathBuf,
    /// New FCAD destination; no overwrite and no --force.
    #[arg(short, long)]
    output: PathBuf,
    /// JSON v1; 0 published, 2 refused, 7 report lost. Usage remains text.
    #[arg(long)]
    json: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    remove: Vec<StableEntityId>,
    add: Vec<Addition>,
}
#[derive(Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case", deny_unknown_fields)]
enum Addition {
    Horizontal {
        curve_id: StableEntityId,
    },
    Vertical {
        curve_id: StableEntityId,
    },
    Distance {
        curve_id: StableEntityId,
        distance_mm: f64,
    },
}
impl Addition {
    fn checked(self) -> Result<AddLineConstraint> {
        let (curve, kind) = match self {
            Self::Horizontal { curve_id } => (curve_id, LineConstraintKind::Horizontal),
            Self::Vertical { curve_id } => (curve_id, LineConstraintKind::Vertical),
            Self::Distance {
                curve_id,
                distance_mm,
            } => (
                curve_id,
                LineConstraintKind::Distance(LineLengthMm::new(distance_mm)?),
            ),
        };
        Ok(AddLineConstraint { curve, kind })
    }
}
fn result(args: &EditConstraintsArgs) -> Result<EditedSketchConstraints> {
    if args.json {
        for p in [&args.source, &args.request, &args.output] {
            crate::json::require_utf8_path(p)?;
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&args.request)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(CadError::input("constraint request exceeds 65536 bytes"));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid constraint request JSON: {e}")))?;
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported constraint request_version; expected 1",
        ));
    }
    let edits = SketchConstraintEdits {
        remove: input.remove,
        add: input
            .add
            .into_iter()
            .map(Addition::checked)
            .collect::<Result<_>>()?,
    };
    let document = Document::open_read_only(&args.source)?;
    let expected = DocumentVersion {
        document_id: document.meta().document_id,
        content: args.expect_version,
    };
    document.close()?;
    let request = EditSketchConstraintsRequest {
        source: args.source.clone(),
        expected,
        sketch: args.sketch,
        destination: args.output.clone(),
        edits,
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_sketch_constraints_copy(&request, &mut kernel, &OperationContext::default())
}
pub fn run(args: EditConstraintsArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::EditSketchConstraintsCopy,
            result.map(crate::json::constraints::Published::from),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, sketch {})",
            r.destination.display(),
            r.document_id,
            r.sketch
        );
        for c in r.added {
            println!("added constraint {}", c.id);
        }
        for id in r.removed {
            println!("removed constraint {id}");
        }
        println!("degrees of freedom: {}", r.solve.degrees_of_freedom());
        Ok(ExitCode::SUCCESS)
    }
}
