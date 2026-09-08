// SPDX-License-Identifier: MIT
//! The CLI's arguments and words over the same STL operation as the viewer.

use ferritecad_jobs::{BodySelection, StlExportRequest, export_document_as_stl};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::Result;
use std::process::ExitCode;

use crate::{ExportStlArgs, replacing};

pub fn export_stl(args: ExportStlArgs) -> Result<ExitCode> {
    let params = TessellationParams::new(args.linear_deflection, args.angular_deflection, false)?;
    let exported = export_document_as_stl(
        StlExportRequest {
            document: &args.path,
            destination: &args.output,
            body: args
                .solid
                .as_deref()
                .map(BodySelection::NameOrId)
                .unwrap_or(BodySelection::Only {
                    advice: "name one with --solid",
                }),
            params,
            existing: replacing(args.force),
        },
        OcctKernel::new,
        &OperationContext::default(),
    )?;
    println!(
        "wrote {} ({} triangles, {} bytes) from {}",
        exported.destination.display(),
        exported.triangles,
        exported.bytes,
        exported.body.label()
    );
    println!(
        "  deflection: {} mm linear, {} rad angular",
        params.linear_deflection(),
        params.angular_deflection()
    );
    Ok(ExitCode::SUCCESS)
}
